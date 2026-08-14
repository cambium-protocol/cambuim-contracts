#![cfg_attr(not(test), no_std)]

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
    /// This mock validates the *shape* of a valid call and enforces the
    /// bindings the real Groth16 circuit will also commit to:
    /// * the proof and its public inputs are non-empty,
    /// * a canonical verifying key is supplied (`vkey_version > 0`, non-zero
    ///   `vkey_key`), and
    /// * `public_inputs[0]` commits to the `project_id` being minted for —
    ///   preventing proof replay across projects.
    ///
    /// The real implementation will use Soroban's native BN254 host functions
    /// (Protocol 25) to verify Groth16 proofs against the supplied key, with
    /// the same public-input bindings. The interface is production-shaped.
    pub fn verify(
        _env: Env,
        proof: Proof,
        public_inputs: Vec<BytesN<32>>,
        project_id: BytesN<32>,
        vkey_version: u32,
        vkey_key: BytesN<32>,
    ) -> Result<bool, Error> {
        // MOCK: replace with real BN254/Groth16 verification, see Day 3.
        if proof.proof_data.is_empty() {
            return Err(Error::InvalidProof);
        }
        if public_inputs.is_empty() {
            return Err(Error::InvalidProof);
        }
        if vkey_version == 0 || vkey_key == BytesN::from_array(&_env, &[0u8; 32]) {
            return Err(Error::InvalidProof);
        }
        // The claim must be committed to the project being minted. The real
        // circuit enforces this inside the proof; the mock enforces it here.
        if public_inputs.get(0).unwrap() != project_id {
            return Err(Error::InvalidProof);
        }

        // MOCK: always return true — real verification happens here on Day 3.
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
            public_inputs: soroban_sdk::vec![env, BytesN::from_array(env, &[7u8; 32])],
        }
    }

    /// A fully-formed valid call: the project the inputs commit to, a live
    /// canonical verifying key, and a valid proof.
    fn valid_args(env: &Env) -> (Proof, Vec<BytesN<32>>, BytesN<32>, u32, BytesN<32>) {
        let project_id = BytesN::from_array(env, &[7u8; 32]);
        let public_inputs = soroban_sdk::vec![env, project_id.clone()];
        let vkey_key = BytesN::from_array(env, &[5u8; 32]);
        (sample_proof(env), public_inputs, project_id, 1, vkey_key)
    }

    #[test]
    fn verify_returns_true_for_valid_proof() {
        let (env, client) = setup();
        let (proof, public_inputs, project_id, vkey_version, vkey_key) = valid_args(&env);

        let result = client.verify(
            &proof,
            &public_inputs,
            &project_id,
            &vkey_version,
            &vkey_key,
        );
        assert!(result);
    }

    #[test]
    fn verify_rejects_empty_proof_data() {
        let (env, client) = setup();
        let (mut proof, public_inputs, project_id, vkey_version, vkey_key) = valid_args(&env);
        proof.proof_data = Bytes::new(&env);

        let result = client.try_verify(
            &proof,
            &public_inputs,
            &project_id,
            &vkey_version,
            &vkey_key,
        );
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn verify_rejects_empty_public_inputs() {
        let (env, client) = setup();
        let (proof, _public_inputs, project_id, vkey_version, vkey_key) = valid_args(&env);
        let public_inputs: Vec<BytesN<32>> = soroban_sdk::vec![&env];

        let result = client.try_verify(
            &proof,
            &public_inputs,
            &project_id,
            &vkey_version,
            &vkey_key,
        );
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn verify_rejects_project_id_not_bound_in_inputs() {
        let (env, client) = setup();
        let (proof, _public_inputs, _project_id, vkey_version, vkey_key) = valid_args(&env);
        // The claim's public inputs commit to a *different* project than the
        // one the registry is minting for.
        let project_id = BytesN::from_array(&env, &[7u8; 32]);
        let other_project = BytesN::from_array(&env, &[99u8; 32]);
        let public_inputs = soroban_sdk::vec![&env, other_project];

        let result = client.try_verify(
            &proof,
            &public_inputs,
            &project_id,
            &vkey_version,
            &vkey_key,
        );
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn verify_rejects_zero_vkey_version() {
        let (env, client) = setup();
        let (proof, public_inputs, project_id, _vkey_version, vkey_key) = valid_args(&env);

        let result = client.try_verify(&proof, &public_inputs, &project_id, &0, &vkey_key);
        assert_eq!(result, Err(Ok(Error::InvalidProof)));
    }

    #[test]
    fn verify_rejects_zero_vkey_key() {
        let (env, client) = setup();
        let (proof, public_inputs, project_id, vkey_version, _vkey_key) = valid_args(&env);
        let zero = BytesN::from_array(&env, &[0u8; 32]);

        let result = client.try_verify(&proof, &public_inputs, &project_id, &vkey_version, &zero);
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
