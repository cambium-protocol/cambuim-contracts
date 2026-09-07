#![cfg_attr(not(test), no_std)]

use cambium_shared::{Error, RetireeRef, MIN_LEDGER_TTL, TARGET_LEDGER_TTL};
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, IntoVal, Symbol, Vec,
};

/// Refresh the TTL of a persistent entry so retirement records and nullifiers
/// are never evicted. Called after every `persistent().set()` and on every hit
/// of a critical `persistent().get()`.
fn refresh_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_LEDGER_TTL, TARGET_LEDGER_TTL);
}

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
    /// Ids of retirement records per project, in retirement order.
    RetirementIdsByProject(BytesN<32>),
    /// Monotonic counter used to mint collision-proof record ids.
    RetirementCount,
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
    pub fn initialize(env: Env, credit_token: Address, registry: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage()
            .instance()
            .set(&DataKey::CreditToken, &credit_token);
        env.storage().instance().set(&DataKey::Registry, &registry);
        Ok(())
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

        // Generate a unique, collision-proof retirement record ID. The
        // monotonic counter guarantees two identical retirements in the same
        // ledger (same project, year, amount, sequence) still diverge.
        let counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RetirementCount)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::RetirementCount, &(counter + 1));

        let mut id_bytes = soroban_sdk::Bytes::new(&env);
        id_bytes.extend_from_slice(&project_id.to_array());
        id_bytes.extend_from_slice(&vintage_year.to_be_bytes());
        id_bytes.extend_from_slice(&amount.to_be_bytes());
        id_bytes.extend_from_slice(&(env.ledger().sequence() as u64).to_be_bytes());
        id_bytes.extend_from_slice(&counter.to_be_bytes());
        let record_id: BytesN<32> = env.crypto().keccak256(&id_bytes).into();

        // Consume the nullifier so the shielded claim cannot be replayed.
        if shield {
            env.storage()
                .persistent()
                .set(&DataKey::Nullified(nullifier.clone()), &true);
            refresh_ttl(&env, &DataKey::Nullified(nullifier.clone()));
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

        // Store the record and append its id to the project's list so
        // retirements are enumerable on-chain.
        env.storage()
            .persistent()
            .set(&DataKey::Retirement(record_id.clone()), &record);
        refresh_ttl(&env, &DataKey::Retirement(record_id.clone()));
        let total_ids_key = DataKey::RetirementIdsByProject(project_id.clone());
        let mut project_ids: Vec<BytesN<32>> = match env.storage().persistent().get(&total_ids_key)
        {
            Some(ids) => {
                refresh_ttl(&env, &total_ids_key);
                ids
            }
            None => Vec::new(&env),
        };
        project_ids.push_back(record_id.clone());
        env.storage().persistent().set(
            &DataKey::RetirementIdsByProject(project_id.clone()),
            &project_ids,
        );
        refresh_ttl(&env, &DataKey::RetirementIdsByProject(project_id.clone()));

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

    /// Retrieve a single retirement record by its unique ID.
    ///
    /// Looks up the retirement record stored under the given `id`. Returns the
    /// full [`RetirementRecord`] if found, or an error if no record exists for
    /// the provided identifier.
    ///
    /// # Arguments
    /// * `id` - The unique 32-byte identifier of the retirement record.
    ///
    /// # Returns
    /// The [`RetirementRecord`] associated with `id`, or an error.
    ///
    /// # Errors
    /// * [`Error::RetirementNotFound`] - No record exists for the given `id`.
    pub fn get_retirement(env: Env, id: BytesN<32>) -> Result<RetirementRecord, Error> {
        let key = DataKey::Retirement(id);
        match env.storage().persistent().get(&key) {
            Some(record) => {
                refresh_ttl(&env, &key);
                Ok(record)
            }
            None => Err(Error::RetirementNotFound),
        }
    }

    /// Total number of retirements recorded, across all projects.
    ///
    /// Returns the current value of the monotonic retirement counter, which
    /// increments with each successful retirement. This count covers all
    /// projects and is not filtered by project or vintage year.
    ///
    /// # Returns
    /// The total number of retirement records created since the contract was
    /// initialized. Returns `0` if no retirements have been recorded.
    pub fn total_retirements(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::RetirementCount)
            .unwrap_or(0)
    }

    /// IDs of all retirements for a project, in retirement order.
    ///
    /// Returns the list of unique 32-byte record IDs for every retirement
    /// associated with the given project, ordered chronologically by the
    /// order in which they were created. An empty vector indicates no
    /// retirements have been recorded for the project.
    ///
    /// # Arguments
    /// * `project_id` - The project whose retirement IDs to retrieve.
    ///
    /// # Returns
    /// A [`Vec<BytesN<32>>`] of retirement record IDs, in creation order.
    /// Returns an empty vector if the project has no retirements.
    pub fn get_retirement_ids(env: Env, project_id: BytesN<32>) -> Vec<BytesN<32>> {
        let key = DataKey::RetirementIdsByProject(project_id);
        match env.storage().persistent().get(&key) {
            Some(ids) => {
                refresh_ttl(&env, &key);
                ids
            }
            None => Vec::new(&env),
        }
    }

    /// Full retirement records for a project, in retirement order.
    ///
    /// Fetches the complete [`RetirementRecord`] for every retirement
    /// associated with the given project. Records are returned in creation
    /// order (the same order as `get_retirement_ids`). If a record
    /// referenced by an ID is missing from storage it is silently skipped.
    ///
    /// # Arguments
    /// * `project_id` - The project whose retirement records to retrieve.
    ///
    /// # Returns
    /// A [`Vec<RetirementRecord>`] of all retirement records for the project,
    /// in chronological order. Returns an empty vector if the project has no
    /// retirements.
    pub fn get_retirements_by_project(env: Env, project_id: BytesN<32>) -> Vec<RetirementRecord> {
        let ids = Self::get_retirement_ids(env.clone(), project_id);
        let mut records = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            let key = DataKey::Retirement(id);
            if let Some(record) = env.storage().persistent().get(&key) {
                refresh_ttl(&env, &key);
                records.push_back(record);
            }
        }
        records
    }
}

#[cfg(test)]
mod tests;
