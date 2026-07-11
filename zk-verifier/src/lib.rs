#![no_std]

use cambium_shared::{Error, Proof};
use soroban_sdk::{contract, contractimpl, contracttype, BytesN, Env, Vec};

#[derive(Clone)]
#[contracttype]
enum DataKey {
    Initialized,
}

#[contract]
pub struct ZkVerifierContract;

#[contractimpl]
impl ZkVerifierContract {
    /// Initialize the verifier. Can only be called once.
    pub fn initialize(env: Env) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    /// Verify a zero-knowledge proof against public inputs.
    ///
    /// # MOCK: replace with real BN254/Groth16 verification, see Day 3
    ///
    /// This mock always returns `true` for any non-empty proof with at least
    /// one public input. The real implementation will use Soroban's native
    /// BN254 host functions (CAP-0074 / Protocol 25) to verify Groth16 proofs.
    ///
    /// The public interface (`verify(env, proof, public_inputs) -> Result<bool, Error>`)
    /// is production-shaped and will remain stable when the real verification
    /// logic replaces this mock.
    pub fn verify(_env: Env, proof: Proof, public_inputs: Vec<BytesN<32>>) -> Result<bool, Error> {
        // MOCK: replace with real BN254/Groth16 verification, see Day 3
        //
        // For now, accept any proof that has non-empty proof_data and
        // at least one public input. This lets the rest of the system
        // build against a stable interface while zk-circuits matures.
        if proof.proof_data.is_empty() {
            return Err(Error::InvalidProof);
        }
        if public_inputs.is_empty() {
            return Err(Error::InvalidProof);
        }

        // MOCK: always return true — real verification happens here on Day 3
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Bytes, Env};

    fn setup() -> (Env, ZkVerifierContractClient<'static>) {
        let env = Env::default();
        let contract_id = env.register_contract(None, ZkVerifierContract);
        let client = ZkVerifierContractClient::new(&env, &contract_id);
        env.mock_all_auths();
        client.initialize();
        let client: ZkVerifierContractClient<'static> = unsafe { core::mem::transmute(client) };
        (env, client)
    }

    fn sample_proof(env: &Env) -> Proof {
        Proof {
            proof_data: Bytes::from_array(env, &[1u8, 2, 3, 4]),
            public_inputs: soroban_sdk::vec![env, BytesN::from_array(env, &[0u8; 32])],
        }
    }

    #[test]
    fn verify_returns_true_for_valid_proof() {
        let (env, client) = setup();
        let proof = sample_proof(&env);
        let public_inputs = soroban_sdk::vec![&env, BytesN::from_array(&env, &[1u8; 32])];

        let result = client.verify(&proof, &public_inputs);
        assert!(result);
    }

    #[test]
    fn verify_rejects_empty_proof_data() {
        let (env, client) = setup();
        let proof = Proof {
            proof_data: Bytes::new(&env),
            public_inputs: soroban_sdk::vec![&env, BytesN::from_array(&env, &[1u8; 32])],
        };
        let public_inputs = soroban_sdk::vec![&env, BytesN::from_array(&env, &[1u8; 32])];

        let result = client.try_verify(&proof, &public_inputs);
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn verify_rejects_empty_public_inputs() {
        let (env, client) = setup();
        let proof = sample_proof(&env);
        let public_inputs: Vec<BytesN<32>> = soroban_sdk::vec![&env];

        let result = client.try_verify(&proof, &public_inputs);
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn initialize_panics_on_double_init() {
        let env = Env::default();
        let contract_id = env.register_contract(None, ZkVerifierContract);
        let client = ZkVerifierContractClient::new(&env, &contract_id);
        env.mock_all_auths();
        client.initialize();

        let result = client.try_initialize();
        assert!(result.is_err(), "double-init must panic");
    }
}
