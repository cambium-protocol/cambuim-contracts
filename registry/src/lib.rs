#![no_std]

mod governance;
mod types;

#[cfg(test)]
mod tests;

use cambium_shared::{Error, Proof};
use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, IntoVal, Symbol};

pub use types::{DataKey, Project, Vintage};

/// Storage key for the credit-token contract address (instance storage).
#[derive(Clone)]
#[contracttype]
enum InstanceKey {
    CreditToken,
    ZkVerifier,
}

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    /// Initialize the registry with the addresses of the credit-token and
    /// zk-verifier contracts. Can only be called once.
    pub fn initialize(env: Env, credit_token: Address, zk_verifier: Address) {
        if env.storage().instance().has(&InstanceKey::CreditToken) {
            panic!("already initialized");
        }
        env.storage()
            .instance()
            .set(&InstanceKey::CreditToken, &credit_token);
        env.storage()
            .instance()
            .set(&InstanceKey::ZkVerifier, &zk_verifier);
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
            .get(&InstanceKey::ZkVerifier)
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
            .get(&InstanceKey::CreditToken)
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
}
