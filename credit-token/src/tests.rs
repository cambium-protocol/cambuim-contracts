use soroban_sdk::{testutils::Address as _, Address, Env};

use super::{CreditTokenContract, CreditTokenContractClient, TokenError};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let client = CreditTokenContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    client.initialize(&admin);
    (env, admin, user, contract_id)
}

fn client<'a>(env: &'a Env, contract_id: &'a Address) -> CreditTokenContractClient<'a> {
    CreditTokenContractClient::new(env, contract_id)
}

// ---- initialize tests ----

#[test]
fn initialize_sets_admin() {
    let (env, admin, _user, contract_id) = setup();
    assert_eq!(client(&env, &contract_id).admin(), admin);
}

#[test]
fn initialize_panics_on_double_init() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = client(&env, &contract_id);
    env.mock_all_auths();
    c.initialize(&admin);

    let admin2 = Address::generate(&env);
    let result = c.try_initialize(&admin2);
    assert!(result.is_err(), "double-init must panic");
}

// ---- balance tests ----

#[test]
fn balance_defaults_to_zero() {
    let (env, _admin, user, contract_id) = setup();
    assert_eq!(client(&env, &contract_id).balance(&user), 0);
}

// ---- mint authorization tests (100% coverage on auth paths) ----

#[test]
fn mint_by_admin_succeeds() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &1000);
    assert_eq!(c.balance(&user), 1000);
}

/// Unauthorized callers must be rejected — 100% auth path coverage.
///
/// This test explicitly disables mock_all_auths so that `admin.require_auth()`
/// inside mint() is checked against real authorization entries. With no valid
/// auth for the admin, the call must fail.
#[test]
fn mint_unauthorized_fails_without_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);

    // Initialize using mock auth so initialize() itself passes.
    env.mock_all_auths();
    c.initialize(&admin);

    // Remove all mocked auths — now require_auth() must be satisfied by real
    // authorization entries. None are set, so any call requiring admin auth fails.
    env.set_auths(&[]);
    let result = c.try_mint(&user, &1000);
    assert!(
        result.is_err(),
        "mint must fail when admin auth is not provided; got {:?}",
        result
    );
    // Balance must remain zero — no tokens were minted.
    assert_eq!(c.balance(&user), 0);
}

#[test]
fn mint_zero_fails() {
    let (env, _admin, user, contract_id) = setup();
    let result = client(&env, &contract_id).try_mint(&user, &0);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn mint_negative_fails() {
    let (env, _admin, user, contract_id) = setup();
    let result = client(&env, &contract_id).try_mint(&user, &-100);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn mint_accumulates_balance() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &500);
    c.mint(&user, &300);
    assert_eq!(c.balance(&user), 800);
}

// ---- transfer tests ----

#[test]
fn transfer_succeeds() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);
    c.transfer(&user, &recipient, &400);
    assert_eq!(c.balance(&user), 600);
    assert_eq!(c.balance(&recipient), 400);
}

#[test]
fn transfer_insufficient_balance() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    c.mint(&user, &100);
    let result = c.try_transfer(&user, &recipient, &200);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
    assert_eq!(c.balance(&user), 100);
    assert_eq!(c.balance(&recipient), 0);
}

#[test]
fn transfer_zero_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);
    let result = c.try_transfer(&user, &recipient, &0);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn transfer_negative_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);
    let result = c.try_transfer(&user, &recipient, &-1);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

// ---- approve & transfer_from tests ----

#[test]
fn approve_and_transfer_from_succeeds() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.mint(&user, &1000);
    c.approve(&user, &spender, &500);
    assert_eq!(c.allowance(&user, &spender), 500);

    c.transfer_from(&spender, &user, &recipient, &300);
    assert_eq!(c.balance(&user), 700);
    assert_eq!(c.balance(&recipient), 300);
    assert_eq!(c.allowance(&user, &spender), 200);
}

#[test]
fn transfer_from_exceeds_allowance() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.mint(&user, &1000);
    c.approve(&user, &spender, &100);

    let result = c.try_transfer_from(&spender, &user, &recipient, &200);
    assert_eq!(result, Err(Ok(TokenError::AllowanceUnderflow)));
    assert_eq!(c.balance(&user), 1000);
}

#[test]
fn transfer_from_insufficient_balance() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.mint(&user, &50);
    c.approve(&user, &spender, &1000);

    let result = c.try_transfer_from(&spender, &user, &recipient, &100);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn transfer_from_zero_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.mint(&user, &1000);
    c.approve(&user, &spender, &500);

    let result = c.try_transfer_from(&spender, &user, &recipient, &0);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn approve_zero_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let result = c.try_approve(&user, &spender, &0);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn approve_negative_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let result = c.try_approve(&user, &spender, &-1);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

// ---- burn tests ----

#[test]
fn burn_by_admin_succeeds() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &1000);
    c.burn(&user, &400);
    assert_eq!(c.balance(&user), 600);
}

/// Once a burner contract is configured, only it (not the admin) may burn.
#[test]
fn burn_by_burner_succeeds() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let burner = Address::generate(&env);

    c.mint(&user, &1000);
    c.set_burner(&burner);
    assert_eq!(c.get_burner(), Some(burner.clone()));

    // With a burner set, the burner contract burns; admin no longer can.
    // (mock_all_auths makes burner.require_auth() pass regardless of caller.)
    c.burn(&user, &300);
    assert_eq!(c.balance(&user), 700);
}

#[test]
fn set_burner_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    c.initialize(&admin);

    // Remove mocked auths — set_burner must now fail without admin auth.
    env.set_auths(&[]);
    let burner = Address::generate(&env);
    let result = c.try_set_burner(&burner);
    assert!(result.is_err(), "set_burner must fail without admin auth");
    assert_eq!(c.get_burner(), None);
}

/// Non-admin burn must be rejected — 100% auth path coverage.
#[test]
fn burn_unauthorized_fails_without_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    c.initialize(&admin);
    c.mint(&user, &1000);

    // Remove all mocked auths — burn must now fail without admin auth.
    env.set_auths(&[]);
    let result = c.try_burn(&user, &100);
    assert!(
        result.is_err(),
        "burn must fail when admin auth is not provided; got {:?}",
        result
    );
    assert_eq!(c.balance(&user), 1000);
}

#[test]
fn burn_insufficient_balance() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &100);
    let result = c.try_burn(&user, &200);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
    assert_eq!(c.balance(&user), 100);
}

#[test]
fn burn_zero_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &100);
    let result = c.try_burn(&user, &0);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}

#[test]
fn burn_negative_fails() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &100);
    let result = c.try_burn(&user, &-1);
    assert_eq!(result, Err(Ok(TokenError::NegativeAmount)));
}
