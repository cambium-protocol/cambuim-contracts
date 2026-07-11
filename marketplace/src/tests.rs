use super::*;
use cambium_shared::Error;
use soroban_sdk::{testutils::Address as _, BytesN, Env, Symbol};

fn setup() -> (Env, MarketplaceContractClient<'static>) {
    let env = Env::default();
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    client.initialize();
    let client: MarketplaceContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client)
}

fn sample_pool_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

// ---- initialize tests ----

#[test]
fn initialize_sets_initialized() {
    let (env, client) = setup();
    // Verify we can call functions that depend on initialization
    let pool_id = sample_pool_id(&env);
    let result = client.try_get_pool(&pool_id);
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn initialize_panics_on_double_init() {
    let env = Env::default();
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(&env, &contract_id);
    env.mock_all_auths();
    client.initialize();

    let result = client.try_initialize();
    assert!(result.is_err(), "double-init must panic");
}

// ---- create_pool tests ----

#[test]
fn create_pool_succeeds() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    let pool = client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    assert_eq!(pool.id, pool_id);
    assert_eq!(pool.credit_token, credit_token);
    assert_eq!(pool.paired_asset, paired_asset);
    assert_eq!(pool.credit_reserves, 1000);
    assert_eq!(pool.paired_reserves, 5000);
}

#[test]
fn create_pool_duplicate_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);
    let result = client.try_create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn create_pool_zero_credit_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    let result = client.try_create_pool(&pool_id, &credit_token, &paired_asset, &0, &5000);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn create_pool_negative_paired_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    let result = client.try_create_pool(&pool_id, &credit_token, &paired_asset, &1000, &-1);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

// ---- get_pool tests ----

#[test]
fn get_pool_not_found() {
    let (env, client) = setup();
    let pool_id = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_get_pool(&pool_id);
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

// ---- swap tests ----

#[test]
fn swap_succeeds() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    // Swap 100 credit tokens for XLM
    // Expected output: (5000 * 100) / (1000 + 100) = 500000 / 1100 ≈ 454.54...
    // With integer division: 500000 / 1100 = 454
    let amount_out = client.swap(&pool_id, &100, &0);
    assert_eq!(amount_out, 454);

    // Verify pool reserves updated
    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.credit_reserves, 1100);
    assert_eq!(pool.paired_reserves, 4546);
}

#[test]
fn swap_with_min_amount_out_succeeds() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    // Swap with slippage protection
    let amount_out = client.swap(&pool_id, &100, &450);
    assert_eq!(amount_out, 454);
}

#[test]
fn swap_slippage_protection_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    // Try to swap with unrealistic min_amount_out
    let result = client.try_swap(&pool_id, &100, &500);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_zero_amount_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    let result = client.try_swap(&pool_id, &0, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_negative_amount_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    let result = client.try_swap(&pool_id, &-100, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_pool_not_found() {
    let (env, client) = setup();
    let pool_id = BytesN::from_array(&env, &[99u8; 32]);

    let result = client.try_swap(&pool_id, &100, &0);
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn swap_fractional_amounts() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    // Pool with very small reserves to test fractional amounts
    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &1000);

    // Swap 1 unit — should return 0 with integer division (1000/1001 = 0)
    // This is expected behavior for constant-product AMM with integer arithmetic
    let result = client.try_swap(&pool_id, &1, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_multiple_swaps() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);
    let credit_token = Address::generate(&env);
    let paired_asset = Symbol::new(&env, "XLM");

    client.create_pool(&pool_id, &credit_token, &paired_asset, &1000, &5000);

    // First swap
    let amount_out1 = client.swap(&pool_id, &100, &0);
    assert_eq!(amount_out1, 454);

    // Second swap — pool reserves have changed
    let amount_out2 = client.swap(&pool_id, &100, &0);
    // New reserves: credit=1100, paired=4546
    // (4546 * 100) / (1100 + 100) = 454600 / 1200 = 378
    assert_eq!(amount_out2, 378);
}

// ---- place_limit_order tests ----

#[test]
fn place_limit_order_not_yet_implemented() {
    let (_env, client) = setup();
    let result = client.try_place_limit_order(&OrderSide::Buy, &100, &10);
    assert_eq!(result, Err(Ok(Error::NotYetImplemented)));
}
