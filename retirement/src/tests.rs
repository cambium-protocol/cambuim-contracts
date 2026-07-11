use super::*;
use cambium_shared::Error;
use soroban_sdk::{testutils::Address as _, BytesN, Env};

fn setup() -> (Env, RetirementContractClient<'static>) {
    let env = Env::default();
    let contract_id = env.register_contract(None, RetirementContract);
    let client = RetirementContractClient::new(&env, &contract_id);
    env.mock_all_auths();

    let credit_token = soroban_sdk::Address::generate(&env);
    let registry = soroban_sdk::Address::generate(&env);
    client.initialize(&credit_token, &registry);

    let client: RetirementContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client)
}

fn sample_project_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

// ---- initialize tests ----

#[test]
fn initialize_sets_addresses() {
    let (env, client) = setup();
    // Verify initialization worked by attempting a retirement
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    // This should not panic — retire succeeds after initialization
    let _record = client.retire(&from, &project_id, &2025, &100, &false);
}

#[test]
fn initialize_panics_on_double_init() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RetirementContract);
    let client = RetirementContractClient::new(&env, &contract_id);
    env.mock_all_auths();

    let credit_token = soroban_sdk::Address::generate(&env);
    let registry = soroban_sdk::Address::generate(&env);
    client.initialize(&credit_token, &registry);

    let result = client.try_initialize(&credit_token, &registry);
    assert!(result.is_err(), "double-init must panic");
}

// ---- retire tests ----

#[test]
fn retire_succeeds() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let record = client.retire(&from, &project_id, &2025, &100, &false);

    assert_eq!(record.project_id, project_id);
    assert_eq!(record.vintage_year, 2025);
    assert_eq!(record.amount, 100);
    assert_eq!(record.retiree, RetireeRef::Public(from.clone()));
}

#[test]
fn retire_creates_stored_record() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let record = client.retire(&from, &project_id, &2025, &100, &false);
    let fetched = client.get_retirement(&record.id);

    assert_eq!(fetched, record);
}

#[test]
fn retire_zero_amount_fails() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &0, &false);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_negative_amount_fails() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &-100, &false);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_shield_true_fails_with_not_yet_implemented() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &100, &true);
    assert_eq!(result, Err(Ok(Error::NotYetImplemented)));
}

#[test]
fn retire_multiple_projects() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project1 = BytesN::from_array(&env, &[1u8; 32]);
    let project2 = BytesN::from_array(&env, &[2u8; 32]);

    let record1 = client.retire(&from, &project1, &2025, &100, &false);
    let record2 = client.retire(&from, &project2, &2025, &200, &false);

    assert_ne!(record1.id, record2.id);
    assert_eq!(record1.amount, 100);
    assert_eq!(record2.amount, 200);
}

#[test]
fn retire_same_project_different_vintages() {
    let (env, client) = setup();
    let from = soroban_sdk::Address::generate(&env);
    let project_id = sample_project_id(&env);

    let record1 = client.retire(&from, &project_id, &2024, &100, &false);
    let record2 = client.retire(&from, &project_id, &2025, &200, &false);

    assert_ne!(record1.id, record2.id);
    assert_eq!(record1.vintage_year, 2024);
    assert_eq!(record2.vintage_year, 2025);
}

// ---- get_retirement tests ----

#[test]
fn get_retirement_not_found() {
    let (env, client) = setup();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_get_retirement(&missing);
    assert_eq!(result, Err(Ok(Error::RetirementNotFound)));
}
