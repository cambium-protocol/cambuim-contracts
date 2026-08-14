use cambium_credit_token::CreditTokenContract;
use cambium_shared::Proof;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Symbol,
};

use super::{
    GovernanceConfig, Project, ProposalTarget, RegistryContract, RegistryContractClient, Vintage,
};
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

fn sample_proof(env: &Env, project_id: &BytesN<32>) -> Proof {
    Proof {
        proof_data: Bytes::from_array(env, &[1u8, 2, 3, 4]),
        public_inputs: soroban_sdk::vec![env, project_id.clone()],
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

/// Bootstrap a canonical verifying key (version 1) for the "VM0007"
/// methodology so that projects registered at `verifying_key_version: 1`
/// (see `make_project`) can mint. Returns the governance signer.
fn bootstrap_vkey(env: &Env, client: &RegistryContractClient<'static>) -> Address {
    let signer = Address::generate(env);
    client.init_governance(&1, &soroban_sdk::vec![env, signer.clone()], &3600);
    let proposal_id = client.propose_vkey_update(
        &signer,
        &Symbol::new(env, "VM0007"),
        &BytesN::from_array(env, &[5u8; 32]),
    );
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    let vkey = client.execute_vkey_update(&proposal_id);
    assert_eq!(vkey.version, 1);
    signer
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
    bootstrap_vkey(&env, &client);

    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));

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
    bootstrap_vkey(&env, &client);

    client.request_mint(&project_id, &2025, &500, &sample_proof(&env, &project_id));
    client.request_mint(&project_id, &2025, &300, &sample_proof(&env, &project_id));

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
    let result = client.try_request_mint(&missing, &2025, &1000, &sample_proof(&env, &missing));
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn request_mint_fails_on_zero_amount() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let result = client.try_request_mint(&project_id, &2025, &0, &sample_proof(&env, &project_id));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn request_mint_fails_on_negative_amount() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let result =
        client.try_request_mint(&project_id, &2025, &-100, &sample_proof(&env, &project_id));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn request_mint_fails_on_empty_proof() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);
    bootstrap_vkey(&env, &client);

    let result = client.try_request_mint(&project_id, &2025, &1000, &empty_proof(&env));
    assert_eq!(result, Err(Ok(Error::InvalidProof)));
}

#[test]
fn request_mint_fails_without_canonical_vkey() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);
    // No governance/vkey has ever been configured for the methodology.

    let result =
        client.try_request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));
    assert_eq!(result, Err(Ok(Error::VkeyNotFound)));
}

#[test]
fn request_mint_fails_on_stale_project_key_version() {
    let (env, _, client, _) = setup();
    // Register the project at key version 1, but rotate the canonical key to
    // version 2 before minting.
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);

    let signer = bootstrap_vkey(&env, &client);
    let proposal_id = client.propose_vkey_update(
        &signer,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[6u8; 32]),
    );
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    let vkey = client.execute_vkey_update(&proposal_id);
    assert_eq!(vkey.version, 2);

    let result =
        client.try_request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));
    assert_eq!(result, Err(Ok(Error::VkeyMismatch)));
}

#[test]
fn request_mint_separate_vintages() {
    let (env, _, client, _) = setup();
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);
    bootstrap_vkey(&env, &client);

    client.request_mint(&project_id, &2024, &500, &sample_proof(&env, &project_id));
    client.request_mint(&project_id, &2025, &700, &sample_proof(&env, &project_id));

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
    bootstrap_vkey(&env, &client);

    let token_client = cambium_credit_token::CreditTokenContractClient::new(&env, &credit_token_id);

    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));

    // Registry (the caller of mint) should hold the minted tokens.
    assert_eq!(token_client.balance(&registry_addr), 1000);
}

// ---- governance tests ----

fn governance_setup() -> (
    Env,
    RegistryContractClient<'static>,
    soroban_sdk::Vec<Address>,
) {
    let (env, _registry_addr, client, _credit_token_id) = setup();
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let signer3 = Address::generate(&env);
    let signers = soroban_sdk::vec![&env, signer1.clone(), signer2.clone(), signer3.clone()];
    client.init_governance(&2, &signers, &3600);
    (env, client, signers)
}

#[test]
fn init_governance_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let registry_id = env.register_contract(None, RegistryContract);
    let client = RegistryContractClient::new(&env, &registry_id);

    let signers = soroban_sdk::vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    client.init_governance(&2, &signers, &3600);

    let config = client.get_governance();
    assert_eq!(config.threshold, 2);
    assert_eq!(config.signers.len(), 3);
    assert_eq!(config.timelock_secs, 3600);
}

#[test]
fn init_governance_validates_config() {
    let (env, _registry_addr, client, _credit_token_id) = setup();

    // threshold 0
    let signers = soroban_sdk::vec![&env, Address::generate(&env)];
    assert_eq!(
        client.try_init_governance(&0, &signers, &3600),
        Err(Ok(Error::InvalidConfig))
    );

    // threshold > signers
    let signers2 = soroban_sdk::vec![&env, Address::generate(&env)];
    assert_eq!(
        client.try_init_governance(&2, &signers2, &3600),
        Err(Ok(Error::InvalidConfig))
    );

    // empty signers
    let empty: soroban_sdk::Vec<Address> = soroban_sdk::vec![&env];
    assert_eq!(
        client.try_init_governance(&1, &empty, &3600),
        Err(Ok(Error::InvalidConfig))
    );

    // zero timelock
    let signers3 = soroban_sdk::vec![&env, Address::generate(&env)];
    assert_eq!(
        client.try_init_governance(&1, &signers3, &0),
        Err(Ok(Error::InvalidConfig))
    );
}

#[test]
fn init_governance_panics_on_double_init() {
    let env = Env::default();
    env.mock_all_auths();
    let registry_id = env.register_contract(None, RegistryContract);
    let client = RegistryContractClient::new(&env, &registry_id);

    let signers = soroban_sdk::vec![&env, Address::generate(&env)];
    client.init_governance(&1, &signers, &3600);

    let result = client.try_init_governance(&1, &signers, &3600);
    assert!(result.is_err(), "double init_governance must panic");
}

#[test]
fn get_vkey_returns_zero_default() {
    let (_env, client, _signers) = governance_setup();
    let vkey = client.get_vkey(&Symbol::new(&_env, "VM0007"));
    assert_eq!(vkey.version, 0);
}

#[test]
fn propose_requires_signer() {
    let (env, client, _signers) = governance_setup();
    let outsider = Address::generate(&env);
    let new_key = BytesN::from_array(&env, &[7u8; 32]);
    let result = client.try_propose_vkey_update(&outsider, &Symbol::new(&env, "VM0007"), &new_key);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn full_governance_flow_updates_vkey() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();
    let methodology = Symbol::new(&env, "VM0007");
    let new_key = BytesN::from_array(&env, &[7u8; 32]);

    // Propose (signer 1)
    let proposal_id = client.propose_vkey_update(&s1, &methodology, &new_key);
    assert_ne!(proposal_id, BytesN::from_array(&env, &[0u8; 32]));

    // Not enough approvals -> cannot execute
    assert_eq!(
        client.try_execute_vkey_update(&proposal_id),
        Err(Ok(Error::ThresholdNotMet))
    );

    // Approve with signer 2 -> threshold (2) reached
    let approvals = client.approve_update(&s2, &proposal_id);
    assert_eq!(approvals, 2);

    // Timelock not elapsed -> cannot execute
    assert_eq!(
        client.try_execute_vkey_update(&proposal_id),
        Err(Ok(Error::TimelockPending))
    );

    // Fast-forward the ledger past the timelock and execute.
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    let vkey = client.execute_vkey_update(&proposal_id);
    assert_eq!(vkey.version, 1);
    assert_eq!(vkey.key, new_key);

    // The canonical key is stored per methodology.
    let canonical = client.get_vkey(&methodology);
    assert_eq!(canonical.version, 1);
    assert_eq!(canonical.key, new_key);

    // Executing again is rejected.
    assert_eq!(
        client.try_execute_vkey_update(&proposal_id),
        Err(Ok(Error::OrderClosed))
    );
}

#[test]
fn governance_requires_threshold_not_all_signers() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let methodology = Symbol::new(&env, "ARR");
    let new_key = BytesN::from_array(&env, &[9u8; 32]);

    let proposal_id = client.propose_vkey_update(&s1, &methodology, &new_key);

    // Proposing counts as signer 1's approval; a second vote is rejected.
    let result = client.try_approve_update(&s1, &proposal_id);
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));

    // Only one approval exists, so the threshold (2) is never reached —
    // even after the timelock elapses the update cannot execute.
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    assert_eq!(
        client.try_execute_vkey_update(&proposal_id),
        Err(Ok(Error::ThresholdNotMet))
    );
}

#[test]
fn approve_requires_signer() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let proposal_id = client.propose_vkey_update(
        &s1,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[7u8; 32]),
    );

    let outsider = Address::generate(&env);
    let result = client.try_approve_update(&outsider, &proposal_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn approve_missing_proposal_fails() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_approve_update(&s1, &missing);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ---- governance-config updates (signer rotation) ----

#[test]
fn governance_update_rotates_signer_set() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    // Replace the signer set with a fresh set of two signers, threshold 2.
    let new_signer1 = Address::generate(&env);
    let new_signer2 = Address::generate(&env);
    let new_config = GovernanceConfig {
        threshold: 2,
        signers: soroban_sdk::vec![&env, new_signer1.clone(), new_signer2.clone()],
        timelock_secs: 7200,
    };

    let proposal_id = client.propose_governance_update(&s1, &new_config);
    assert_ne!(proposal_id, BytesN::from_array(&env, &[0u8; 32]));

    // Old signer set still governs approval of the rotation.
    let approvals = client.approve_update(&s2, &proposal_id);
    assert_eq!(approvals, 2);

    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    client.execute_governance_update(&proposal_id);

    let config = client.get_governance();
    assert_eq!(config.threshold, 2);
    assert_eq!(config.signers.len(), 2);
    assert_eq!(config.signers.get(0).unwrap(), new_signer1);
    assert_eq!(config.signers.get(1).unwrap(), new_signer2);
    assert_eq!(config.timelock_secs, 7200);

    // The old signer is no longer authorized.
    let result = client.try_propose_governance_update(&s1, &new_config);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn execute_governance_update_rejects_vkey_proposal() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    // A vkey proposal cannot be executed through the governance path...
    let proposal_id = client.propose_vkey_update(
        &s1,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[7u8; 32]),
    );
    client.approve_update(&s2, &proposal_id);
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    assert_eq!(
        client.try_execute_governance_update(&proposal_id),
        Err(Ok(Error::InvalidProposalTarget))
    );

    // ...and a governance proposal cannot be executed as a vkey update.
    let new_config = GovernanceConfig {
        threshold: 2,
        signers: soroban_sdk::vec![&env, s1.clone(), s2.clone()],
        timelock_secs: 7200,
    };
    let gov_proposal = client.propose_governance_update(&s1, &new_config);
    client.approve_update(&s2, &gov_proposal);
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    assert_eq!(
        client.try_execute_vkey_update(&gov_proposal),
        Err(Ok(Error::InvalidProposalTarget))
    );
}

#[test]
fn execute_governance_update_rejects_invalid_config() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    // Threshold 0 is invalid; execution must be rejected without applying.
    let bad_config = GovernanceConfig {
        threshold: 0,
        signers: soroban_sdk::vec![&env, s1.clone()],
        timelock_secs: 3600,
    };
    let proposal_id = client.propose_governance_update(&s1, &bad_config);
    client.approve_update(&s2, &proposal_id);
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    assert_eq!(
        client.try_execute_governance_update(&proposal_id),
        Err(Ok(Error::InvalidConfig))
    );

    // The current configuration is untouched.
    let config = client.get_governance();
    assert_eq!(config.signers.len(), 3);
    assert_eq!(config.threshold, 2);
}

// ---- proposal cancellation ----

#[test]
fn cancel_update_prevents_execution() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let proposal_id = client.propose_vkey_update(
        &s1,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[7u8; 32]),
    );
    client.approve_update(&s2, &proposal_id);

    // A signer cancels the proposal before the timelock elapses.
    client.cancel_update(&s2, &proposal_id);

    // Even after the timelock, the cancelled proposal cannot execute.
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    assert_eq!(
        client.try_execute_vkey_update(&proposal_id),
        Err(Ok(Error::OrderClosed))
    );
    // And the canonical key was never changed.
    assert_eq!(client.get_vkey(&Symbol::new(&env, "VM0007")).version, 0);

    // Cancelling again is rejected.
    assert_eq!(
        client.try_cancel_update(&s1, &proposal_id),
        Err(Ok(Error::OrderClosed))
    );
}

#[test]
fn cancel_update_requires_signer() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let proposal_id = client.propose_vkey_update(
        &s1,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[7u8; 32]),
    );

    let outsider = Address::generate(&env);
    let result = client.try_cancel_update(&outsider, &proposal_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn cancel_missing_proposal_fails() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_cancel_update(&s1, &missing);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn get_proposal_returns_stored_proposal() {
    let (env, client, signers) = governance_setup();
    let s1 = signers.get(0).unwrap();
    let new_key = BytesN::from_array(&env, &[7u8; 32]);
    let methodology = Symbol::new(&env, "VM0007");

    let proposal_id = client.propose_vkey_update(&s1, &methodology, &new_key);
    let proposal = client.get_proposal(&proposal_id);
    assert_eq!(proposal.id, proposal_id);
    assert_eq!(proposal.approvals.len(), 1);
    assert!(!proposal.executed);
    assert!(!proposal.cancelled);
    assert_eq!(
        proposal.target,
        ProposalTarget::Vkey(methodology.clone(), new_key.clone())
    );

    // Missing proposal -> NotFound.
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    assert_eq!(client.try_get_proposal(&missing), Err(Ok(Error::NotFound)));
}

// ---- retirement recording tests ----

#[test]
fn set_retirement_contract_requires_signer() {
    let (env, _registry_addr, client, _credit_token_id) = setup();
    let signer = Address::generate(&env);
    client.init_governance(&1, &soroban_sdk::vec![&env, signer.clone()], &3600);
    let outsider = Address::generate(&env);
    let retirement = Address::generate(&env);
    let result = client.try_set_retirement_contract(&outsider, &retirement);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn record_retirement_updates_vintage() {
    let (env, _registry_addr, client, _credit_token_id) = setup();
    let retirement = Address::generate(&env);
    let signer = bootstrap_vkey(&env, &client);
    client.set_retirement_contract(&signer, &retirement);

    // Register project + mint 1000 so a vintage exists.
    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);
    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));

    // With mock auths, the registered retirement contract is authorized.
    client.record_retirement(&project_id, &2025, &400);
    let vintage = client.get_vintage(&project_id, &2025);
    assert_eq!(vintage.total_issued, 1000);
    assert_eq!(vintage.total_retired, 400);

    // Retiring more than issued is rejected (double-counting guard).
    let result = client.try_record_retirement(&project_id, &2025, &700);
    assert_eq!(result, Err(Ok(Error::ExceedsIssued)));
    assert_eq!(client.get_vintage(&project_id, &2025).total_retired, 400);
}

#[test]
fn record_retirement_requires_authorized_contract() {
    let (env, _registry_addr, client, _credit_token_id) = setup();
    let retirement = Address::generate(&env);
    let signer = bootstrap_vkey(&env, &client);
    client.set_retirement_contract(&signer, &retirement);

    let project = make_project(&env, 1);
    let project_id = project.id.clone();
    client.register_project(&project);
    client.request_mint(&project_id, &2025, &1000, &sample_proof(&env, &project_id));

    // Remove all mocked auths: recording without the retirement contract's
    // authorization must fail, since the caller is not the registered contract.
    env.set_auths(&[]);
    let result = client.try_record_retirement(&project_id, &2025, &100);
    assert!(
        result.is_err(),
        "record_retirement must require the retirement contract"
    );
}

#[test]
fn record_retirement_unknown_vintage_fails() {
    let (env, _registry_addr, client, _credit_token_id) = setup();
    let signer = Address::generate(&env);
    let retirement = Address::generate(&env);
    client.init_governance(&1, &soroban_sdk::vec![&env, signer.clone()], &3600);
    client.set_retirement_contract(&signer, &retirement);

    let unknown = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_record_retirement(&unknown, &2025, &100);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}
