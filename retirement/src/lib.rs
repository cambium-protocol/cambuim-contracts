#![cfg_attr(not(test), no_std)]

use cambium_shared::{Error, RetireeRef};
use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, IntoVal, Symbol};

/// A retirement record storing details about a credit retirement event.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct RetirementRecord {
    /// Unique identifier for this retirement record.
    pub id: BytesN<32>,
    /// The project this retirement is for.
    pub project_id: BytesN<32>,
    /// The vintage year of the retired credits.
    pub vintage_year: u32,
    /// Amount of credits retired.
    pub amount: i128,
    /// Timestamp of the retirement (ledger sequence).
    pub retired_at: u64,
    /// Who performed the retirement (public or shielded).
    pub retiree: RetireeRef,
}

/// Storage keys for the retirement contract.
#[derive(Clone)]
#[contracttype]
enum DataKey {
    Retirement(BytesN<32>),
    /// Nullifiers consumed by shielded retirements (replay guard).
    Nullified(BytesN<32>),
    CreditToken,
    Registry,
    Initialized,
}

#[contract]
pub struct RetirementContract;

#[contractimpl]
impl RetirementContract {
    /// Initialize the retirement contract with references to credit-token and
    /// registry contracts. Can only be called once.
    pub fn initialize(env: Env, credit_token: Address, registry: Address) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage()
            .instance()
            .set(&DataKey::CreditToken, &credit_token);
        env.storage().instance().set(&DataKey::Registry, &registry);
    }

    /// Retire carbon credits permanently.
    ///
    /// Burns the specified amount of credit tokens from the caller and creates
    /// an immutable retirement record.
    ///
    /// Retirements are public by default (per the protocol's design principle
    /// that environmental claims stay public). When `shield` is true, the
    /// retiring party's identity is hidden: only the caller-supplied `nullifier`
    /// is stored (`RetireeRef::Shielded`). The nullifier must be derived
    /// off-chain from a secret (e.g. keccak of a commitment) so the contract
    /// cannot link it to the caller, and it is recorded to prevent a shielded
    /// retirement from being claimed twice.
    ///
    /// # Arguments
    /// * `from` - The address retiring the credits (must authorize this call).
    /// * `project_id` - The project these credits belong to.
    /// * `vintage_year` - The vintage year of the credits.
    /// * `amount` - Number of credits to retire (must be > 0).
    /// * `shield` - If true, record only the `nullifier` instead of `from`.
    /// * `nullifier` - Identity-hiding commitment for shielded retirements
    ///   (ignored when `shield` is false; must be non-zero when shielded).
    ///
    /// # Returns
    /// The created `RetirementRecord` with a unique ID.
    pub fn retire(
        env: Env,
        from: Address,
        project_id: BytesN<32>,
        vintage_year: u32,
        amount: i128,
        shield: bool,
        nullifier: BytesN<32>,
    ) -> Result<RetirementRecord, Error> {
        if amount <= 0 {
            return Err(Error::NonPositiveAmount);
        }
        if shield && nullifier == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(Error::InvalidNullifier);
        }

        from.require_auth();

        // Replay guard: a shielded retirement's nullifier may only be used
        // once, so a replayed claim is rejected before anything is burned.
        if shield
            && env
                .storage()
                .persistent()
                .has(&DataKey::Nullified(nullifier.clone()))
        {
            return Err(Error::AlreadyRegistered);
        }

        // Permanently burn the retired credits before recording the event.
        // `credit_token::burn` is authorized to this contract (the burner),
        // and `from`'s authorization was validated above. Any burn failure
        // (e.g. insufficient balance) aborts the retirement.
        let credit_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::CreditToken)
            .expect("not initialized");
        let burn_result: Result<Result<(), _>, _> = env
            .try_invoke_contract::<(), soroban_sdk::Error>(
                &credit_token,
                &Symbol::new(&env, "burn"),
                soroban_sdk::vec![&env, from.into_val(&env), amount.into_val(&env)],
            );
        if !matches!(burn_result, Ok(Ok(()))) {
            return Err(Error::InsufficientBalance);
        }

        // Record the retirement against the project's vintage so cumulative
        // retired supply is tracked and double-counting is prevented. The
        // registry only accepts this call from this contract.
        let registry: Address = env
            .storage()
            .instance()
            .get(&DataKey::Registry)
            .expect("not initialized");
        let record_result: Result<Result<Result<(), cambium_shared::Error>, _>, _> =
            env.try_invoke_contract::<Result<(), cambium_shared::Error>, soroban_sdk::Error>(
                &registry,
                &Symbol::new(&env, "record_retirement"),
                soroban_sdk::vec![
                    &env,
                    project_id.into_val(&env),
                    vintage_year.into_val(&env),
                    amount.into_val(&env),
                ],
            );
        match record_result {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => return Err(e),
            _ => return Err(Error::RetirementNotFound),
        }

        // Generate a unique retirement record ID
        let mut id_bytes = soroban_sdk::Bytes::new(&env);
        id_bytes.extend_from_slice(&project_id.to_array());
        id_bytes.extend_from_slice(&vintage_year.to_be_bytes());
        id_bytes.extend_from_slice(&amount.to_be_bytes());
        id_bytes.extend_from_slice(&(env.ledger().sequence() as u64).to_be_bytes());
        let record_id: BytesN<32> = env.crypto().keccak256(&id_bytes).into();

        // Consume the nullifier so the shielded claim cannot be replayed.
        if shield {
            env.storage()
                .persistent()
                .set(&DataKey::Nullified(nullifier.clone()), &true);
        }

        // Create the retirement record
        let retiree = if shield {
            RetireeRef::Shielded(nullifier.clone())
        } else {
            RetireeRef::Public(from.clone())
        };
        let record = RetirementRecord {
            id: record_id.clone(),
            project_id: project_id.clone(),
            vintage_year,
            amount,
            retired_at: env.ledger().sequence() as u64,
            retiree,
        };

        // Store the record
        env.storage()
            .persistent()
            .set(&DataKey::Retirement(record_id.clone()), &record);

        // Emit retirement event. For shielded retirements the caller's
        // address is deliberately omitted so identity never leaks on-chain.
        let event_retiree = if shield {
            RetireeRef::Shielded(nullifier.clone())
        } else {
            RetireeRef::Public(from.clone())
        };
        env.events().publish(
            (Symbol::new(&env, "retire"), project_id, event_retiree),
            (vintage_year, amount),
        );

        Ok(record)
    }

    /// Retrieve a retirement record by its ID.
    pub fn get_retirement(env: Env, id: BytesN<32>) -> Result<RetirementRecord, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Retirement(id))
            .ok_or(Error::RetirementNotFound)
    }
}

#[cfg(test)]
mod tests;
