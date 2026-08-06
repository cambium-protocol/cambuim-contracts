use super::*;
use cambium_credit_token::{CreditTokenContract, CreditTokenContractClient};
use cambium_shared::Error;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

/// Deploy a real credit-token (admin = registry) and the retirement contract,
/// wiring the retirement contract as the token's authorized burner.
fn setup() -> (
    Env,
    CreditTokenContractClient<'static>,
    RetirementContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let registry = Address::generate(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let token_client = CreditTokenContractClient::new(&env, &credit_token_id);
    token_client.initialize(&registry);

    let retirement_id = env.register_contract(None, RetirementContract);
    let retirement_client = RetirementContractClient::new(&env, &retirement_id);
    retirement_client.initialize(&credit_token_id, &registry);

    token_client.set_burner(&retirement_id);

    let token_client: CreditTokenContractClient<'static> =
        unsafe { core::mem::transmute(token_client) };
    let retirement_client: RetirementContractClient<'static> =
        unsafe { core::mem::transmute(retirement_client) };
    (env, token_client, retirement_client)
}

fn sample_project_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn fund(token_client: &CreditTokenContractClient<'static>, from: &Address, amount: i128) {
    token_client.mint(from, &amount);
}

// ---- initialize tests ----

#[test]
fn initialize_sets_addresses() {
    let (env, token_client, client) = setup();
    // Verify initialization worked by attempting a retirement
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
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

    let credit_token = Address::generate(&env);
    let registry = Address::generate(&env);
    client.initialize(&credit_token, &registry);

    let result = client.try_initialize(&credit_token, &registry);
    assert!(result.is_err(), "double-init must panic");
}

// ---- retire tests ----

#[test]
fn retire_succeeds() {
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
    let project_id = sample_project_id(&env);

    let record = client.retire(&from, &project_id, &2025, &100, &false);

    assert_eq!(record.project_id, project_id);
    assert_eq!(record.vintage_year, 2025);
    assert_eq!(record.amount, 100);
    assert_eq!(record.retiree, RetireeRef::Public(from.clone()));
}

#[test]
fn retire_burns_tokens() {
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
    let project_id = sample_project_id(&env);

    let record = client.retire(&from, &project_id, &2025, &400, &false);

    // 400 credits were permanently burned.
    assert_eq!(token_client.balance(&from), 600);
    assert_eq!(record.amount, 400);
}

#[test]
fn retire_insufficient_balance_fails() {
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 100);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &200, &false);
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));

    // No record was created and no tokens were lost.
    assert_eq!(token_client.balance(&from), 100);
}

#[test]
fn retire_creates_stored_record() {
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
    let project_id = sample_project_id(&env);

    let record = client.retire(&from, &project_id, &2025, &100, &false);
    let fetched = client.get_retirement(&record.id);

    assert_eq!(fetched, record);
}

#[test]
fn retire_zero_amount_fails() {
    let (env, _token_client, client) = setup();
    let from = Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &0, &false);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_negative_amount_fails() {
    let (env, _token_client, client) = setup();
    let from = Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &-100, &false);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn retire_shield_true_fails_with_not_yet_implemented() {
    let (env, _token_client, client) = setup();
    let from = Address::generate(&env);
    let project_id = sample_project_id(&env);

    let result = client.try_retire(&from, &project_id, &2025, &100, &true);
    assert_eq!(result, Err(Ok(Error::NotYetImplemented)));
}

#[test]
fn retire_multiple_projects() {
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
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
    let (env, token_client, client) = setup();
    let from = Address::generate(&env);
    fund(&token_client, &from, 1000);
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
    let (env, _token_client, client) = setup();
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_get_retirement(&missing);
    assert_eq!(result, Err(Ok(Error::RetirementNotFound)));
}
