use super::*;
use cambium_credit_token::{CreditTokenContract, CreditTokenContractClient};
use cambium_registry::{Project, RegistryContract, RegistryContractClient};
use cambium_shared::{Error, Proof};
use cambium_zk_verifier::ZkVerifierContract;
use soroban_sdk::{
    testutils::Address as _, testutils::Ledger as _, Address, Bytes, BytesN, Env, Symbol,
};

struct Stack {
    env: Env,
    registry_id: Address,
    credit_token_id: Address,
    retirement_id: Address,
}

/// Deploy a fully-wired stack: credit-token (admin = registry), zk-verifier,
/// registry (governance initialized, retirement contract registered), and the
/// retirement contract (authorized as the token's burner).
fn setup() -> Stack {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy credit-token; registry is admin.
    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let token_client = CreditTokenContractClient::new(&env, &credit_token_id);

    // Deploy zk-verifier (mock).
    let zk_verifier_id = env.register_contract(None, ZkVerifierContract);
    let zk_verifier_client =
        cambium_zk_verifier::ZkVerifierContractClient::new(&env, &zk_verifier_id);
    zk_verifier_client.initialize();

    // Deploy registry and wire it.
    let registry_id = env.register_contract(None, RegistryContract);
    let registry_client = RegistryContractClient::new(&env, &registry_id);
    token_client.initialize(&registry_id);
    registry_client.initialize(&credit_token_id, &zk_verifier_id);

    // Bootstrap governance with a single signer.
    let signer = Address::generate(&env);
    registry_client.init_governance(&1, &soroban_sdk::vec![&env, signer.clone()], &3600);

    // Bootstrap a canonical verifying key (version 1) for the "VM0007"
    // methodology so request_mint passes the canonical key binding.
    let vkey_proposal = registry_client.propose_vkey_update(
        &signer,
        &Symbol::new(&env, "VM0007"),
        &BytesN::from_array(&env, &[5u8; 32]),
    );
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + 4000);
    registry_client.execute_vkey_update(&vkey_proposal);

    // Deploy retirement and wire it as the burner + recorder.
    let retirement_id = env.register_contract(None, RetirementContract);
    let retirement_client = RetirementContractClient::new(&env, &retirement_id);
    retirement_client.initialize(&credit_token_id, &registry_id);
    token_client.set_burner(&retirement_id);
    registry_client.set_retirement_contract(&signer, &retirement_id);

    Stack {
        env,
        registry_id,
        credit_token_id,
        retirement_id,
    }
}

fn sample_project_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn nullifier(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[42u8; 32])
}

fn sample_proof(env: &Env, project_id: &BytesN<32>) -> Proof {
    Proof {
        proof_data: Bytes::from_array(env, &[1u8, 2, 3, 4]),
        public_inputs: soroban_sdk::vec![env, project_id.clone()],
    }
}

/// Register a project, mint `amount` credits, and move them from the registry
/// (the mint recipient) to `from` so `from` can spend them.
fn fund(stack: &Stack, from: &Address, amount: i128) {
    let env = &stack.env;
    let token_client = CreditTokenContractClient::new(env, &stack.credit_token_id);
    let registry_client = RegistryContractClient::new(env, &stack.registry_id);
    let project_id = sample_project_id(env);
    let project = Project {
        id: project_id.clone(),
        methodology: Symbol::new(env, "VM0007"),
        geography: Symbol::new(env, "BRA"),
        external_registry_ref: None,
        verifying_key_version: 1,
    };
    registry_client.register_project(&project);
    registry_client.request_mint(&project_id, &2025, &amount, &sample_proof(env, &project_id));
    token_client.transfer(&stack.registry_id, from, &amount);
}

// ---- initialize tests ----

#[test]
fn initialize_sets_addresses() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    // This should not panic — retire succeeds after initialization
    let _record = client.retire(&from, &project_id, &2025, &100, &false, &zero(env));
}

#[test]
fn initialize_panics_on_double_init() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RetirementContract);
    let client = RetirementContractClient::new(&env, &contract_id);
    env.mock_all_auths();

    let credit_token = Address::generate(&env);
    let registry = Address::generate(&env);
    client.initialize(&credit_token, &registry);

    let result = client.try_initialize(&credit_token, &registry);
    assert!(result.is_err(), "double-init must panic");
}

// ---- retire tests ----

#[test]
fn retire_succeeds() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    let record = client.retire(&from, &project_id, &2025, &100, &false, &zero(env));

    assert_eq!(record.project_id, project_id);
    assert_eq!(record.vintage_year, 2025);
    assert_eq!(record.amount, 100);
    assert_eq!(record.retiree, RetireeRef::Public(from.clone()));
}

#[test]
fn retire_burns_tokens() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let token_client = CreditTokenContractClient::new(&stack.env, &stack.credit_token_id);

    let record = client.retire(&from, &project_id, &2025, &400, &false, &zero(env));

    // 400 credits were permanently burned.
    assert_eq!(token_client.balance(&from), 600);
    assert_eq!(record.amount, 400);
}

#[test]
fn retire_updates_vintage_totals() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let registry_client = RegistryContractClient::new(&stack.env, &stack.registry_id);

    client.retire(&from, &project_id, &2025, &400, &false, &zero(env));

    let vintage = registry_client.get_vintage(&project_id, &2025);
    assert_eq!(vintage.total_issued, 1000);
    assert_eq!(vintage.total_retired, 400);
}

#[test]
fn retire_cannot_exceed_issued_supply() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 500);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    // 500 were issued and held; retiring 1000 must fail.
    let result = client.try_retire(&from, &project_id, &2025, &1000, &false, &zero(env));
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
}

#[test]
fn retire_insufficient_balance_fails() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 100);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let token_client = CreditTokenContractClient::new(&stack.env, &stack.credit_token_id);

    let result = client.try_retire(&from, &project_id, &2025, &200, &false, &zero(env));
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));

    // No record was created and no tokens were lost.
    assert_eq!(token_client.balance(&from), 100);
}

#[test]
fn retire_creates_stored_record() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    let record = client.retire(&from, &project_id, &2025, &100, &false, &zero(env));
    let fetched = client.get_retirement(&record.id);

    assert_eq!(fetched, record);
}

#[test]
fn retire_zero_amount_fails() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    let result = client.try_retire(&from, &project_id, &2025, &0, &false, &zero(env));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_negative_amount_fails() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    let result = client.try_retire(&from, &project_id, &2025, &-100, &false, &zero(env));
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_shielded_stores_nullifier_only() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let registry_client = RegistryContractClient::new(&stack.env, &stack.registry_id);
    let token_client = CreditTokenContractClient::new(&stack.env, &stack.credit_token_id);

    let nullifier = nullifier(env);
    let record = client.retire(&from, &project_id, &2025, &100, &true, &nullifier);

    // The record and event carry the nullifier, never the caller.
    assert_eq!(record.retiree, RetireeRef::Shielded(nullifier.clone()));

    // Credits were still burned and the vintage total updated.
    assert_eq!(token_client.balance(&from), 900);
    assert_eq!(
        registry_client
            .get_vintage(&project_id, &2025)
            .total_retired,
        100
    );
}

#[test]
fn retire_shielded_replay_rejected() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let token_client = CreditTokenContractClient::new(&stack.env, &stack.credit_token_id);

    let nullifier = nullifier(env);
    client.retire(&from, &project_id, &2025, &100, &true, &nullifier);

    // The same shielded claim cannot be replayed.
    let result = client.try_retire(&from, &project_id, &2025, &100, &true, &nullifier);
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));

    // No additional credits were burned.
    assert_eq!(token_client.balance(&from), 900);
}

#[test]
fn retire_shielded_empty_nullifier_fails() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let token_client = CreditTokenContractClient::new(&stack.env, &stack.credit_token_id);

    let result = client.try_retire(&from, &project_id, &2025, &100, &true, &zero(env));
    assert_eq!(result, Err(Ok(Error::InvalidNullifier)));

    // Nothing burned.
    assert_eq!(token_client.balance(&from), 1000);
}

#[test]
fn retire_shielded_ignores_nullifier_when_public() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let project_id = sample_project_id(&stack.env);
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);

    // Public retirement records the caller even when a nullifier is passed.
    let record = client.retire(&from, &project_id, &2025, &100, &false, &nullifier(env));
    assert_eq!(record.retiree, RetireeRef::Public(from.clone()));
}

#[test]
fn retire_multiple_projects() {
    let stack = setup();
    let env = &stack.env;
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let client = RetirementContractClient::new(env, &stack.retirement_id);
    let registry_client = RegistryContractClient::new(env, &stack.registry_id);
    let token_client = CreditTokenContractClient::new(env, &stack.credit_token_id);
    let project1 = BytesN::from_array(env, &[1u8; 32]);
    let project2 = BytesN::from_array(env, &[2u8; 32]);

    // Register + fund a second project too.
    let project2_struct = Project {
        id: project2.clone(),
        methodology: Symbol::new(env, "VM0007"),
        geography: Symbol::new(env, "KEN"),
        external_registry_ref: None,
        verifying_key_version: 1,
    };
    registry_client.register_project(&project2_struct);
    registry_client.request_mint(&project2, &2025, &1000, &sample_proof(env, &project2));
    token_client.transfer(&stack.registry_id, &from, &500);

    let record1 = client.retire(&from, &project1, &2025, &100, &false, &zero(env));
    let record2 = client.retire(&from, &project2, &2025, &200, &false, &zero(env));

    assert_ne!(record1.id, record2.id);
    assert_eq!(record1.amount, 100);
    assert_eq!(record2.amount, 200);
}

#[test]
fn retire_same_project_different_vintages() {
    let stack = setup();
    let from = Address::generate(&stack.env);
    fund(&stack, &from, 1000);
    let env = &stack.env;
    let client = RetirementContractClient::new(env, &stack.retirement_id);
    let registry_client = RegistryContractClient::new(env, &stack.registry_id);
    let token_client = CreditTokenContractClient::new(env, &stack.credit_token_id);
    let project_id = sample_project_id(env);

    // Fund a 2024 vintage too.
    registry_client.request_mint(&project_id, &2024, &1000, &sample_proof(env, &project_id));
    token_client.transfer(&stack.registry_id, &from, &1000);

    let record1 = client.retire(&from, &project_id, &2024, &100, &false, &zero(env));
    let record2 = client.retire(&from, &project_id, &2025, &200, &false, &zero(env));

    assert_ne!(record1.id, record2.id);
    assert_eq!(record1.vintage_year, 2024);
    assert_eq!(record2.vintage_year, 2025);
}

// ---- get_retirement tests ----

#[test]
fn get_retirement_not_found() {
    let stack = setup();
    let client = RetirementContractClient::new(&stack.env, &stack.retirement_id);
    let missing = BytesN::from_array(&stack.env, &[99u8; 32]);
    let result = client.try_get_retirement(&missing);
    assert_eq!(result, Err(Ok(Error::RetirementNotFound)));
}
