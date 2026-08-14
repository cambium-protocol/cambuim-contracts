use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, TryIntoVal,
};

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

// ---- SEP-41 metadata tests ----

#[test]
fn initialize_seeds_default_metadata() {
    let (env, _admin, _user, contract_id) = setup();
    let c = client(&env, &contract_id);
    assert_eq!(c.decimals(), 7);
    assert_eq!(
        c.name(),
        soroban_sdk::String::from_str(&env, "Cambium Carbon Credit")
    );
    assert_eq!(c.symbol(), soroban_sdk::String::from_str(&env, "CAMB"));
    assert_eq!(c.total_supply(), 0);
}

#[test]
fn set_metadata_overrides_defaults() {
    let (env, _admin, _user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.set_metadata(
        &6,
        &soroban_sdk::String::from_str(&env, "Cambium Verified"),
        &soroban_sdk::String::from_str(&env, "CVC"),
    );
    assert_eq!(c.decimals(), 6);
    assert_eq!(
        c.name(),
        soroban_sdk::String::from_str(&env, "Cambium Verified")
    );
    assert_eq!(c.symbol(), soroban_sdk::String::from_str(&env, "CVC"));
}

#[test]
fn set_metadata_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    c.initialize(&admin);

    env.set_auths(&[]);
    let result = c.try_set_metadata(
        &6,
        &soroban_sdk::String::from_str(&env, "x"),
        &soroban_sdk::String::from_str(&env, "y"),
    );
    assert!(result.is_err(), "set_metadata must fail without admin auth");
}

#[test]
fn set_metadata_rejects_empty_name_or_symbol() {
    let (env, _admin, _user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let empty = soroban_sdk::String::from_str(&env, "");
    assert_eq!(
        c.try_set_metadata(&7, &empty, &soroban_sdk::String::from_str(&env, "CAMB")),
        Err(Ok(TokenError::Unauthorized))
    );
    assert_eq!(
        c.try_set_metadata(&7, &soroban_sdk::String::from_str(&env, "name"), &empty),
        Err(Ok(TokenError::Unauthorized))
    );
}

// ---- total supply tests ----

#[test]
fn total_supply_tracks_mint_and_burn() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);

    c.mint(&user, &1000);
    c.mint(&user, &500);
    assert_eq!(c.total_supply(), 1500);

    c.transfer(&user, &Address::generate(&env), &300);
    assert_eq!(c.total_supply(), 1500, "transfers must not change supply");

    c.burn(&user, &400);
    assert_eq!(c.total_supply(), 1100);
    assert_eq!(c.balance(&user), 800);
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

// ---- compliance allowlist tests ----

#[test]
fn allowlist_disabled_by_default() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    assert!(!c.is_allowlisted(&user));
    // Transfers work while the allowlist is off.
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);
    c.transfer(&user, &recipient, &100);
    assert_eq!(c.balance(&recipient), 100);
}

#[test]
fn enable_allowlist_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    c.initialize(&admin);

    env.set_auths(&[]);
    let result = c.try_enable_allowlist(&true);
    assert!(
        result.is_err(),
        "enable_allowlist must fail without admin auth"
    );
}

#[test]
fn set_allowlisted_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, CreditTokenContract);
    let c = CreditTokenContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    c.initialize(&admin);

    env.set_auths(&[]);
    let result = c.try_set_allowlisted(&Address::generate(&env), &true);
    assert!(
        result.is_err(),
        "set_allowlisted must fail without admin auth"
    );
}

#[test]
fn allowlist_gates_transfer() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);

    c.mint(&user, &1000);
    c.set_allowlisted(&user, &true);
    c.enable_allowlist(&true);

    // Recipient not allowlisted -> transfer rejected.
    let result = c.try_transfer(&user, &recipient, &100);
    assert_eq!(result, Err(Ok(TokenError::Unauthorized)));

    // Allowlist the recipient -> transfer succeeds.
    c.set_allowlisted(&recipient, &true);
    c.transfer(&user, &recipient, &100);
    assert_eq!(c.balance(&recipient), 100);
}

#[test]
fn allowlist_gates_transfer_from() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    c.mint(&user, &1000);
    c.approve(&user, &spender, &500);
    c.set_allowlisted(&user, &true);
    c.enable_allowlist(&true);

    // Recipient not allowlisted -> transfer_from rejected.
    let result = c.try_transfer_from(&spender, &user, &recipient, &100);
    assert_eq!(result, Err(Ok(TokenError::Unauthorized)));

    c.set_allowlisted(&recipient, &true);
    c.transfer_from(&spender, &user, &recipient, &100);
    assert_eq!(c.balance(&recipient), 100);
}

#[test]
fn allowlist_gates_mint() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.set_allowlisted(&user, &true);
    c.enable_allowlist(&true);

    c.mint(&user, &100);
    assert_eq!(c.balance(&user), 100);

    // New (non-allowlisted) recipient cannot receive minted credits.
    let stranger = Address::generate(&env);
    let result = c.try_mint(&stranger, &100);
    assert_eq!(result, Err(Ok(TokenError::Unauthorized)));
}

#[test]
fn allowlist_gates_burn() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &1000);
    c.set_allowlisted(&user, &true);
    c.enable_allowlist(&true);

    c.burn(&user, &100);
    assert_eq!(c.balance(&user), 900);

    // A non-allowlisted holder cannot even receive credits, so burn of such
    // an address is rejected by the gate.
    let stranger = Address::generate(&env);
    let mint_result = c.try_mint(&stranger, &1000);
    assert_eq!(mint_result, Err(Ok(TokenError::Unauthorized)));

    // Once allowlisted, mint succeeds and burning is permitted.
    c.set_allowlisted(&stranger, &true);
    c.mint(&stranger, &1000);
    c.burn(&stranger, &100);
    assert_eq!(c.balance(&stranger), 900);
}

// ---- event tests ----

fn last_event_topics(
    env: &Env,
    expected_len: u32,
) -> (soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val) {
    let events = env.events().all();
    let (_, topics, data) = events.last().unwrap();
    assert_eq!(topics.len(), expected_len);
    (topics, data)
}

/// Extract typed topics from a Vec<Val> event topic list.
fn topics3(env: &Env, topics: &soroban_sdk::Vec<soroban_sdk::Val>) -> (Symbol, Address, Address) {
    (
        topics.get(0).unwrap().try_into_val(env).unwrap(),
        topics.get(1).unwrap().try_into_val(env).unwrap(),
        topics.get(2).unwrap().try_into_val(env).unwrap(),
    )
}

#[test]
fn transfer_emits_event() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);

    c.transfer(&user, &recipient, &100);

    let (topics, data) = last_event_topics(&env, 3);
    let (transfer_sym, from, to) = topics3(&env, &topics);
    assert_eq!(transfer_sym, Symbol::new(&env, "transfer"));
    assert_eq!(from, user);
    assert_eq!(to, recipient);
    let (amount,): (i128,) = data.try_into_val(&env).unwrap();
    assert_eq!(amount, 100);
}

#[test]
fn transfer_from_emits_event() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    c.mint(&user, &1000);
    c.approve(&user, &spender, &500);

    c.transfer_from(&spender, &user, &recipient, &100);

    let (topics, data) = last_event_topics(&env, 3);
    let (_transfer_sym, from, to) = topics3(&env, &topics);
    assert_eq!(from, user);
    assert_eq!(to, recipient);
    let (amount,): (i128,) = data.try_into_val(&env).unwrap();
    assert_eq!(amount, 100);
}

#[test]
fn mint_emits_event() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);

    c.mint(&user, &1000);

    let (topics, data) = last_event_topics(&env, 3);
    let (_mint_sym, _admin_addr, to) = topics3(&env, &topics);
    assert_eq!(to, user);
    let (amount,): (i128,) = data.try_into_val(&env).unwrap();
    assert_eq!(amount, 1000);
}

#[test]
fn burn_emits_event() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    c.mint(&user, &1000);

    c.burn(&user, &400);

    let (topics, data) = last_event_topics(&env, 3);
    let (_burn_sym, _admin_addr, from) = topics3(&env, &topics);
    assert_eq!(from, user);
    let (amount,): (i128,) = data.try_into_val(&env).unwrap();
    assert_eq!(amount, 400);
}

#[test]
fn approve_emits_event() {
    let (env, _admin, user, contract_id) = setup();
    let c = client(&env, &contract_id);
    let spender = Address::generate(&env);

    c.approve(&user, &spender, &500);

    let (topics, data) = last_event_topics(&env, 3);
    let (_approve_sym, from, to) = topics3(&env, &topics);
    assert_eq!(from, user);
    assert_eq!(to, spender);
    let (amount,): (i128,) = data.try_into_val(&env).unwrap();
    assert_eq!(amount, 500);
}
