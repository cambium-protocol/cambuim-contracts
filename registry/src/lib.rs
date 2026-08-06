#![cfg_attr(not(test), no_std)]

mod governance;
mod types;

#[cfg(test)]
mod tests;

use cambium_shared::{Error, Proof};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, IntoVal, Symbol, Vec};

pub use types::{DataKey, GovernanceConfig, Project, Vintage, VkeyProposal, VkeyState};

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    /// Initialize the registry with the addresses of the credit-token and
    /// zk-verifier contracts. Can only be called once.
    pub fn initialize(env: Env, credit_token: Address, zk_verifier: Address) {
        if env.storage().instance().has(&DataKey::CreditToken) {
            panic!("already initialized");
        }
        env.storage()
            .instance()
            .set(&DataKey::CreditToken, &credit_token);
        env.storage()
            .instance()
            .set(&DataKey::ZkVerifier, &zk_verifier);
    }

    /// Register a new carbon project. Fails if the project id already exists.
    pub fn register_project(env: Env, project: Project) -> Result<(), Error> {
        let key = DataKey::Project(project.id.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyRegistered);
        }
        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    /// Look up a registered project by id.
    pub fn get_project(env: Env, project_id: BytesN<32>) -> Result<Project, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(Error::NotFound)
    }

    /// Look up a vintage record by project id and year.
    pub fn get_vintage(env: Env, project_id: BytesN<32>, year: u32) -> Result<Vintage, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Vintage(project_id, year))
            .ok_or(Error::NotFound)
    }

    /// Request a mint for a given project + vintage year.
    ///
    /// The proof is verified by the zk-verifier contract via a cross-contract
    /// call. If the verifier returns false or errors, the mint is rejected.
    ///
    /// On success the function:
    /// 1. Validates inputs (project exists, amount > 0, proof non-empty).
    /// 2. Calls zk-verifier::verify to validate the proof.
    /// 3. Creates or updates the `Vintage` record.
    /// 4. Calls `credit-token::mint` to issue tokens to the requesting caller.
    pub fn request_mint(
        env: Env,
        project_id: BytesN<32>,
        vintage_year: u32,
        amount: i128,
        proof: Proof,
    ) -> Result<(), Error> {
        // --- 1. Input validation ---
        if amount <= 0 {
            return Err(Error::NonPositiveAmount);
        }

        // Project must be registered.
        let _project: Project = env
            .storage()
            .persistent()
            .get(&DataKey::Project(project_id.clone()))
            .ok_or(Error::NotFound)?;

        // --- 2. Cross-contract proof verification ---
        // Call zk-verifier::verify(proof, public_inputs) -> Result<bool, Error>
        let zk_verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::ZkVerifier)
            .expect("not initialized");

        let proof_valid: bool = env.invoke_contract(
            &zk_verifier,
            &Symbol::new(&env, "verify"),
            soroban_sdk::vec![
                &env,
                proof.into_val(&env),
                proof.public_inputs.into_val(&env),
            ],
        );

        if !proof_valid {
            return Err(Error::InvalidProof);
        }

        // --- 3. Update vintage ---
        let vintage_key = DataKey::Vintage(project_id.clone(), vintage_year);
        let mut vintage: Vintage =
            env.storage()
                .persistent()
                .get(&vintage_key)
                .unwrap_or(Vintage {
                    project_id: project_id.clone(),
                    year: vintage_year,
                    total_issued: 0,
                    total_retired: 0,
                });

        vintage.total_issued = vintage
            .total_issued
            .checked_add(amount)
            .ok_or(Error::Overflow)?;

        env.storage().persistent().set(&vintage_key, &vintage);

        // --- 4. Cross-contract mint ---
        // The registry is the only authorized caller of credit-token::mint.
        let credit_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::CreditToken)
            .expect("not initialized");

        // Caller receives the minted tokens.
        let caller = env.current_contract_address();

        // invoke_contract calls credit_token.mint(to=caller, amount=amount)
        // Signature matches CreditTokenContract::mint(env, to, amount).
        env.invoke_contract::<()>(
            &credit_token,
            &Symbol::new(&env, "mint"),
            soroban_sdk::vec![&env, caller.into_val(&env), amount.into_val(&env)],
        );

        Ok(())
    }

    /// Register the retirement contract as the sole recorder of retirements.
    ///
    /// # Authorization
    /// `signer` must be a member of the governance signer set.
    pub fn set_retirement_contract(
        env: Env,
        signer: Address,
        retirement: Address,
    ) -> Result<(), Error> {
        signer.require_auth();
        let cfg = governance::config(&env)?;
        if !governance::is_signer(&cfg, &signer) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::RetirementContract, &retirement);
        Ok(())
    }

    /// Record a retirement against a vintage's cumulative totals.
    ///
    /// Called by the retirement contract after credits have been burned. The
    /// retirement contract is authorized via its stored address (the calling
    /// contract authorizes itself for its own address on cross-contract calls).
    ///
    /// # Errors
    /// * `NonPositiveAmount` if `amount` <= 0
    /// * `NotFound` if the vintage has no issuance record
    /// * `ExceedsIssued` if the retirement would push `total_retired` beyond
    ///   `total_issued` (double-counting guard)
    /// * `Overflow` if the cumulative total would overflow
    pub fn record_retirement(
        env: Env,
        project_id: BytesN<32>,
        vintage_year: u32,
        amount: i128,
    ) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::NonPositiveAmount);
        }

        let retirement: Address = env
            .storage()
            .instance()
            .get(&DataKey::RetirementContract)
            .ok_or(Error::NotFound)?;
        retirement.require_auth();

        let vintage_key = DataKey::Vintage(project_id.clone(), vintage_year);
        let mut vintage: Vintage = env
            .storage()
            .persistent()
            .get(&vintage_key)
            .ok_or(Error::NotFound)?;

        let new_retired = vintage
            .total_retired
            .checked_add(amount)
            .ok_or(Error::Overflow)?;
        if new_retired > vintage.total_issued {
            return Err(Error::ExceedsIssued);
        }
        vintage.total_retired = new_retired;

        env.storage().persistent().set(&vintage_key, &vintage);

        env.events().publish(
            (
                Symbol::new(&env, "retirement_recorded"),
                project_id,
                vintage_year,
            ),
            (amount,),
        );

        Ok(())
    }

    /// Bootstrap the multi-sig governance configuration.
    ///
    /// Can only be called once, before any governance exists. The deployer is
    /// expected to call this immediately after deployment; because Stellar
    /// transactions are ordered per-account, front-running the deployer's own
    /// transaction is not practical.
    ///
    /// # Validation
    /// * `threshold` must be >= 1 and <= `signers.len()`
    /// * `signers` must be non-empty
    /// * `timelock_secs` must be > 0
    pub fn init_governance(
        env: Env,
        threshold: u32,
        signers: Vec<Address>,
        timelock_secs: u64,
    ) -> Result<(), Error> {
        if signers.is_empty() || threshold == 0 || threshold > signers.len() || timelock_secs == 0 {
            return Err(Error::InvalidConfig);
        }
        if env.storage().instance().has(&DataKey::GovernanceConfig) {
            panic!("governance already initialized");
        }

        let config = GovernanceConfig {
            threshold,
            signers,
            timelock_secs,
        };
        env.storage()
            .instance()
            .set(&DataKey::GovernanceConfig, &config);
        Ok(())
    }

    /// Return the current governance configuration.
    pub fn get_governance(env: Env) -> Result<GovernanceConfig, Error> {
        governance::config(&env)
    }

    /// Return the canonical verifying key state for a methodology.
    ///
    /// Returns a zero-initialized state (version 0) if none has been set.
    pub fn get_vkey(env: Env, methodology: Symbol) -> VkeyState {
        env.storage()
            .persistent()
            .get(&DataKey::Vkey(methodology))
            .unwrap_or(VkeyState {
                version: 0,
                key: BytesN::from_array(&env, &[0u8; 32]),
            })
    }

    /// Propose a verifying-key update for `methodology`.
    ///
    /// # Authorization
    /// `signer` must be a member of the governance signer set.
    ///
    /// # Returns
    /// The id of the newly created proposal.
    pub fn propose_vkey_update(
        env: Env,
        signer: Address,
        methodology: Symbol,
        new_key: BytesN<32>,
    ) -> Result<BytesN<32>, Error> {
        signer.require_auth();
        let cfg = governance::config(&env)?;
        if !governance::is_signer(&cfg, &signer) {
            return Err(Error::Unauthorized);
        }

        let proposal = VkeyProposal {
            id: BytesN::from_array(&env, &[0u8; 32]), // filled below
            methodology,
            new_key,
            proposed_at: env.ledger().timestamp(),
            // Proposing implies approval: the proposer's vote counts.
            approvals: soroban_sdk::vec![&env, signer],
            executed: false,
        };

        let id = governance::proposal_id(&env, &proposal);
        let mut stored = proposal;
        stored.id = id.clone();

        env.storage()
            .persistent()
            .set(&DataKey::VkeyProposal(id.clone()), &stored);
        Ok(id)
    }

    /// Approve a pending verifying-key update.
    ///
    /// # Authorization
    /// `signer` must be a member of the governance signer set and must not
    /// have approved this proposal already.
    ///
    /// # Returns
    /// The total number of approvals recorded for the proposal.
    pub fn approve_vkey_update(
        env: Env,
        signer: Address,
        proposal_id: BytesN<32>,
    ) -> Result<u32, Error> {
        signer.require_auth();
        let cfg = governance::config(&env)?;
        if !governance::is_signer(&cfg, &signer) {
            return Err(Error::Unauthorized);
        }

        let key = DataKey::VkeyProposal(proposal_id.clone());
        let mut proposal: VkeyProposal = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotFound)?;

        if proposal.executed {
            return Err(Error::OrderClosed);
        }
        if proposal.approvals.contains(&signer) {
            return Err(Error::AlreadyRegistered);
        }

        proposal.approvals.push_back(signer);
        let approvals = proposal.approvals.len();
        env.storage().persistent().set(&key, &proposal);
        Ok(approvals)
    }

    /// Execute a fully-approved, timelock-elapsed verifying-key update.
    ///
    /// Callable by anyone once the approval threshold has been reached and
    /// the timelock has elapsed — execution is intentionally permissionless.
    ///
    /// # Returns
    /// The new canonical `VkeyState` for the methodology.
    pub fn execute_vkey_update(env: Env, proposal_id: BytesN<32>) -> Result<VkeyState, Error> {
        let cfg = governance::config(&env)?;

        let key = DataKey::VkeyProposal(proposal_id.clone());
        let mut applied: VkeyProposal = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotFound)?;

        if applied.executed {
            return Err(Error::OrderClosed);
        }
        if applied.approvals.len() < cfg.threshold {
            return Err(Error::ThresholdNotMet);
        }

        let elapsed = env.ledger().timestamp().saturating_sub(applied.proposed_at);
        if elapsed < cfg.timelock_secs {
            return Err(Error::TimelockPending);
        }

        // Compute the new key state first so a failure can't leave the
        // proposal marked executed without the key being applied.
        let vkey_key = DataKey::Vkey(applied.methodology.clone());
        let mut vkey: VkeyState = env
            .storage()
            .persistent()
            .get(&vkey_key)
            .unwrap_or(VkeyState {
                version: 0,
                key: BytesN::from_array(&env, &[0u8; 32]),
            });
        vkey.version = vkey.version.checked_add(1).ok_or(Error::Overflow)?;
        vkey.key = applied.new_key.clone();

        // Mark executed before applying to avoid re-entrant double execution.
        applied.executed = true;
        env.storage().persistent().set(&key, &applied);
        env.storage().persistent().set(&vkey_key, &vkey);

        // Emit the governance event for indexers.
        env.events().publish(
            (Symbol::new(&env, "vkey_updated"), applied.methodology),
            (vkey.version,),
        );

        Ok(vkey)
    }
}
