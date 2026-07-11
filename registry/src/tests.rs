use cambium_credit_token::CreditTokenContract;
use cambium_shared::Proof;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Symbol};

use super::{Project, RegistryContract, RegistryContractClient, Vintage};
use cambium_shared::Error;

/// Register both the registry and credit-token contracts and wire them together.
/// Returns (env, registry_contract_address, registry_client, credit_token_contract_address).
fn setup() -> (Env, Address, RegistryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy credit-token first.
    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let token_client = cambium_credit_token::CreditTokenContractClient::new(&env, &credit_token_id);

    // Deploy zk-verifier (mock implementation).
    let zk_verifier_id = env.register_contract(None, cambium_zk_verifier::ZkVerifierContract);
    let zk_verifier_client =
        cambium_zk_verifier::ZkVerifierContractClient::new(&env, &zk_verifier_id);
    zk_verifier_client.initialize();

    // Deploy registry.
    let registry_id = env.register_contract(None, RegistryContract);
    let registry_client = RegistryContractClient::new(&env, &registry_id);

    // Initialize credit-token with registry as admin (so registry can mint).
    token_client.initialize(&registry_id);

    // Initialize registry with credit-token and zk-verifier addresses.
    registry_client.initialize(&credit_token_id, &zk_verifier_id);

    // SAFETY: env and clients share the same lifetime in tests; the 'static
    // transmute is safe because this test function owns env and it outlives
    // any use of the client.
    let registry_client: RegistryContractClient<'static> =
        unsafe { core::mem::transmute(registry_client) };
    (env, registry_id, registry_client, credit_token_id)
}

fn sample_proof(env: &Env) -> Proof {
    Proof {
        proof_data: Bytes::from_array(env, &[1u8, 2, 3, 4]),
        public_inputs: soroban_sdk::vec![env, BytesN::from_array(env, &[0u8; 32])],
    }
}

fn empty_proof(env: &Env) -> Proof {
    Proof {
        proof_data: Bytes::new(env),
        public_inputs: soroban_sdk::vec![env],
    }
}

fn make_project(env: &Env, id_byte: u8) -> Project {
    Project {
        id: BytesN::from_array(env, &[id_byte; 32]),
        methodology: Symbol::new(env, "VM0007"),
        geography: Symbol::new(env, "BRA"),
        external_registry_ref: None,
        verifying_key_version: 1,
    }
}

// ---- initialize tests ----

#[test]
fn initialize_sets_credit_token_address() {
    // A second call to initialize on an already-initialized registry must fail.
    let env = Env::default();
    env.mock_all_auths();

    let registry_id = env.register_contract(None, RegistryContract);
    let client = RegistryContractClient::new(&env, &registry_id);
    let credit_token = Address::generate(&env);
    let zk_verifier = Address::generate(&env);

    client.initialize(&credit_token, &zk_verifier);

    // Second call should panic.
    let result = client.try_initialize(&credit_token, &zk_verifier);
    assert!(result.is_err(), "double-init must fail");
}

// ---- register_project tests ----

#[test]
fn register_project_succeeds() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();

    client.register_project(&project);

    let fetched = client.get_project(&project_id);
    assert_eq!(fetched, project);
}

#[test]
fn register_project_duplicate_fails() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);

    client.register_project(&project);
    let result = client.try_register_project(&project);
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn register_project_with_external_registry_ref() {
    let (env, _, client, _) = setup();
    let project = Project {
        id: BytesN::from_array(&env, &[2u8; 32]),
        methodology: Symbol::new(&env, "ARR"),
        geography: Symbol::new(&env, "KEN"),
        external_registry_ref: Some(Bytes::from_array(&env, b"VCS-1234")),
        verifying_key_version: 1,
    };
    let project_id = project.id.clone();
    client.register_project(&project);

    let fetched = client.get_project(&project_id);
    assert_eq!(fetched, project);
}

// ---- get_project tests ----

#[test]
fn get_project_not_found() {
    let (env, _, client, _) = setup();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_get_project(&missing);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ---- get_vintage tests ----

#[test]
fn get_vintage_not_found() {
    let (env, _, client, _) = setup();
    let project_id = BytesN::from_array(&env, &[1u8; 32]);
    let result = client.try_get_vintage(&project_id, &2025);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ---- request_mint tests ----

#[test]
fn request_mint_creates_vintage_and_updates_issued() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env));

    let vintage = client.get_vintage(&project_id, &2025);
    assert_eq!(
        vintage,
        Vintage {
            project_id: project_id.clone(),
            year: 2025,
            total_issued: 1000,
            total_retired: 0,
        }
    );
}

#[test]
fn request_mint_accumulates_issuance() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    client.request_mint(&project_id, &2025, &500, &sample_proof(&env));
    client.request_mint(&project_id, &2025, &300, &sample_proof(&env));

    let vintage = client.get_vintage(&project_id, &2025);
    assert_eq!(
        vintage,
        Vintage {
            project_id: project_id.clone(),
            year: 2025,
            total_issued: 800,
            total_retired: 0,
        }
    );
}

#[test]
fn request_mint_fails_on_missing_project() {
    let (env, _, client, _) = setup();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_request_mint(&missing, &2025, &1000, &sample_proof(&env));
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn request_mint_fails_on_zero_amount() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let result = client.try_request_mint(&project_id, &2025, &0, &sample_proof(&env));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn request_mint_fails_on_negative_amount() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let result = client.try_request_mint(&project_id, &2025, &-100, &sample_proof(&env));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn request_mint_fails_on_empty_proof() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let result = client.try_request_mint(&project_id, &2025, &1000, &empty_proof(&env));
    assert_eq!(result, Err(Ok(Error::InvalidProof)));
}

#[test]
fn request_mint_separate_vintages() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    client.request_mint(&project_id, &2024, &500, &sample_proof(&env));
    client.request_mint(&project_id, &2025, &700, &sample_proof(&env));

    let v2024 = client.get_vintage(&project_id, &2024);
    let v2025 = client.get_vintage(&project_id, &2025);

    assert_eq!(
        v2024,
        Vintage {
            project_id: project_id.clone(),
            year: 2024,
            total_issued: 500,
            total_retired: 0,
        }
    );
    assert_eq!(
        v2025,
        Vintage {
            project_id: project_id.clone(),
            year: 2025,
            total_issued: 700,
            total_retired: 0,
        }
    );
}

/// Verify that after a successful request_mint, the credit-token contract
/// has actually recorded the minted balance — confirming the end-to-end
/// registry → credit-token mint path works.
#[test]
fn request_mint_issues_tokens_to_registry() {
    let (env, registry_addr, client, credit_token_id) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let token_client = cambium_credit_token::CreditTokenContractClient::new(&env, &credit_token_id);

    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env));

    // Registry (the caller of mint) should hold the minted tokens.
    assert_eq!(token_client.balance(&registry_addr), 1000);
}
