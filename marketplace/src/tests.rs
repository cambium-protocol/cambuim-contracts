use super::*;
use cambium_credit_token::{CreditTokenContract, CreditTokenContractClient};
use cambium_shared::Error;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol};

fn setup() -> (Env, MarketplaceContractClient<'static>) {
    let env = Env::default();
    let (_, client) = deploy_marketplace(&env);
    (env, client)
}

/// Deploy + initialize the marketplace contract, returning its address and a
/// client bound to it. The address is needed to assert on escrow balances.
fn deploy_marketplace(env: &Env) -> (Address, MarketplaceContractClient<'static>) {
    let contract_id = env.register_contract(None, MarketplaceContract);
    let client = MarketplaceContractClient::new(env, &contract_id);
    env.mock_all_auths();
    client.initialize();
    let client: MarketplaceContractClient<'static> = unsafe { core::mem::transmute(client) };
    (contract_id, client)
}

fn sample_pool_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

/// Build a `PoolConfig` concisely from the args tests use at each call site.
fn pool_config(
    env: &Env,
    credit_token: &Address,
    paired_token: &Address,
    paired_asset: &str,
    initial_credit: i128,
    initial_paired: i128,
    fee_bps: u32,
) -> PoolConfig {
    PoolConfig {
        credit_token: credit_token.clone(),
        paired_token: paired_token.clone(),
        paired_asset: Symbol::new(env, paired_asset),
        initial_credit,
        initial_paired,
        fee_bps,
    }
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    let token_client = CreditTokenContractClient::new(env, token);
    token_client.mint(to, &amount);
}

/// Deploy a real credit-token as both the credit and the paired asset, and a
/// marketplace, for exercising real escrow transfers.
/// Returns (env, marketplace client, credit_token_id, paired_token_id).
fn order_setup() -> (Env, MarketplaceContractClient<'static>, Address, Address) {
    let (env, client) = setup();

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    // Fund an LP so the pool's initial liquidity can be escrowed for real.
    let lp = Address::generate(&env);
    mint(&env, &credit_token_id, &lp, 10_000);
    mint(&env, &paired_token_id, &lp, 50_000);

    // Pool for credits against the paired asset.
    client.create_pool(
        &lp,
        &sample_pool_id(&env),
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "USDC",
            1000,
            5000,
            0,
        ),
    );

    (env, client, credit_token_id, paired_token_id)
}

/// AMM fixture: a pool escrowed by `lp` with 1000 credits / 5000 paired at a
/// 0 bp fee, plus a funded trader.
struct Amm {
    env: Env,
    client: MarketplaceContractClient<'static>,
    marketplace_id: Address,
    credit_token_id: Address,
    paired_token_id: Address,
    pool_id: BytesN<32>,
    lp: Address,
    trader: Address,
}

fn amm_setup() -> Amm {
    let env = Env::default();
    let (marketplace_id, client) = deploy_marketplace(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let lp = Address::generate(&env);
    mint(&env, &credit_token_id, &lp, 10_000);
    mint(&env, &paired_token_id, &lp, 50_000);

    let pool_id = sample_pool_id(&env);
    client.create_pool(
        &lp,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "USDC",
            1000,
            5000,
            0,
        ),
    );

    let trader = Address::generate(&env);
    mint(&env, &credit_token_id, &trader, 10_000);

    Amm {
        env,
        client,
        marketplace_id,
        credit_token_id,
        paired_token_id,
        pool_id,
        lp,
        trader,
    }
}

fn credit_balance(amm: &Amm, addr: &Address) -> i128 {
    CreditTokenContractClient::new(&amm.env, &amm.credit_token_id).balance(addr)
}

fn paired_balance(amm: &Amm, addr: &Address) -> i128 {
    CreditTokenContractClient::new(&amm.env, &amm.paired_token_id).balance(addr)
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
fn create_pool_succeeds_and_escrows_liquidity() {
    let env = Env::default();
    let (marketplace, client) = deploy_marketplace(&env);
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env);
    mint(&env, &credit_token_id, &creator, 2000);
    mint(&env, &paired_token_id, &creator, 10_000);

    let pool = client.create_pool(
        &creator,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "XLM",
            1000,
            5000,
            30,
        ),
    );

    assert_eq!(pool.id, pool_id);
    assert_eq!(pool.credit_token, credit_token_id);
    assert_eq!(pool.paired_token, paired_token_id);
    assert_eq!(pool.paired_asset, Symbol::new(&env, "XLM"));
    assert_eq!(pool.credit_reserves, 1000);
    assert_eq!(pool.paired_reserves, 5000);
    assert_eq!(pool.fee_bps, 30);

    // Liquidity actually moved: the creator funded the escrow.
    assert_eq!(credit_client.balance(&creator), 1000);
    assert_eq!(paired_client.balance(&creator), 5000);
    assert_eq!(credit_client.balance(&marketplace), 1000);
    assert_eq!(paired_client.balance(&marketplace), 5000);
}

#[test]
fn create_pool_duplicate_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env);
    mint(&env, &credit_token_id, &creator, 2000);
    mint(&env, &paired_token_id, &creator, 10_000);

    client.create_pool(
        &creator,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "XLM",
            1000,
            5000,
            0,
        ),
    );
    let result = client.try_create_pool(
        &creator,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "XLM",
            1000,
            5000,
            0,
        ),
    );
    assert_eq!(result, Err(Ok(Error::AlreadyRegistered)));
}

#[test]
fn create_pool_zero_credit_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env);

    let result = client.try_create_pool(
        &creator,
        &pool_id,
        &pool_config(&env, &credit_token_id, &paired_token_id, "XLM", 0, 5000, 0),
    );
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn create_pool_negative_paired_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env);

    let result = client.try_create_pool(
        &creator,
        &pool_id,
        &pool_config(&env, &credit_token_id, &paired_token_id, "XLM", 1000, -1, 0),
    );
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn create_pool_invalid_fee_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env);
    mint(&env, &credit_token_id, &creator, 2000);
    mint(&env, &paired_token_id, &creator, 10_000);

    let result = client.try_create_pool(
        &creator,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "XLM",
            1000,
            5000,
            10_001,
        ),
    );
    assert_eq!(result, Err(Ok(Error::InvalidConfig)));
}

#[test]
fn create_pool_insufficient_liquidity_fails() {
    let (env, client) = setup();
    let pool_id = sample_pool_id(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let creator = Address::generate(&env); // never funded

    let result = client.try_create_pool(
        &creator,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "XLM",
            1000,
            5000,
            0,
        ),
    );
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
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
fn swap_succeeds_and_settles_tokens() {
    let amm = amm_setup();
    let marketplace = &amm.marketplace_id;

    // Swap 100 credit tokens for USDC.
    // Expected output: (5000 * 100) / (1000 + 100) = 500000 / 1100 = 454
    let amount_out = amm.client.swap(&amm.trader, &amm.pool_id, &100, &0);
    assert_eq!(amount_out, 454);

    // Real tokens moved: trader paid 100 credits, received 454 paired; the
    // marketplace escrow holds the swapped-in credits and paid out paired.
    assert_eq!(credit_balance(&amm, &amm.trader), 9900);
    assert_eq!(paired_balance(&amm, &amm.trader), 454);
    assert_eq!(credit_balance(&amm, marketplace), 1100);
    assert_eq!(paired_balance(&amm, marketplace), 4546);

    // Pool reserves updated to match the escrow.
    let pool = amm.client.get_pool(&amm.pool_id);
    assert_eq!(pool.credit_reserves, 1100);
    assert_eq!(pool.paired_reserves, 4546);
}

#[test]
fn swap_with_min_amount_out_succeeds() {
    let amm = amm_setup();

    // Swap with slippage protection.
    let amount_out = amm.client.swap(&amm.trader, &amm.pool_id, &100, &450);
    assert_eq!(amount_out, 454);
}

#[test]
fn swap_slippage_protection_fails() {
    let amm = amm_setup();

    // Try to swap with unrealistic min_amount_out.
    let result = amm.client.try_swap(&amm.trader, &amm.pool_id, &100, &500);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));

    // No tokens moved on a rejected swap.
    assert_eq!(credit_balance(&amm, &amm.trader), 10_000);
    assert_eq!(paired_balance(&amm, &amm.trader), 0);
}

#[test]
fn swap_zero_amount_fails() {
    let amm = amm_setup();
    let result = amm.client.try_swap(&amm.trader, &amm.pool_id, &0, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_negative_amount_fails() {
    let amm = amm_setup();
    let result = amm.client.try_swap(&amm.trader, &amm.pool_id, &-100, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_pool_not_found() {
    let amm = amm_setup();
    let fake_pool = BytesN::from_array(&amm.env, &[99u8; 32]);

    let result = amm.client.try_swap(&amm.trader, &fake_pool, &100, &0);
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn swap_fractional_amounts() {
    let amm = amm_setup();
    let small_pool = BytesN::from_array(&amm.env, &[2u8; 32]);
    amm.client.create_pool(
        &amm.lp,
        &small_pool,
        &pool_config(
            &amm.env,
            &amm.credit_token_id,
            &amm.paired_token_id,
            "XLM",
            1000,
            1000,
            0,
        ),
    );

    // Swap 1 unit — returns 0 with integer division (1000/1001 = 0).
    let result = amm.client.try_swap(&amm.trader, &small_pool, &1, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn swap_multiple_swaps() {
    let amm = amm_setup();

    // First swap
    let amount_out1 = amm.client.swap(&amm.trader, &amm.pool_id, &100, &0);
    assert_eq!(amount_out1, 454);

    // Second swap — pool reserves have changed
    let amount_out2 = amm.client.swap(&amm.trader, &amm.pool_id, &100, &0);
    // New reserves: credit=1100, paired=4546
    // (4546 * 100) / (1100 + 100) = 454600 / 1200 = 378
    assert_eq!(amount_out2, 378);

    assert_eq!(credit_balance(&amm, &amm.trader), 9800);
    assert_eq!(paired_balance(&amm, &amm.trader), 832);
}

#[test]
fn swap_applies_fee_to_pool_balance() {
    let env = Env::default();
    let (marketplace, client) = deploy_marketplace(&env);

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let lp = Address::generate(&env);
    mint(&env, &credit_token_id, &lp, 10_000);
    mint(&env, &paired_token_id, &lp, 50_000);

    let pool_id = sample_pool_id(&env);
    client.create_pool(
        &lp,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "USDC",
            1000,
            5000,
            300,
        ),
    );

    let trader = Address::generate(&env);
    mint(&env, &credit_token_id, &trader, 10_000);

    // 300 bps (3%) fee: net_in = 97, out = (5000*97)/(1000+97) = 442.
    let amount_out = client.swap(&trader, &pool_id, &100, &0);
    assert_eq!(amount_out, 442);

    // The fee stays in the pool: 3 credits of the input are retained.
    assert_eq!(credit_client.balance(&marketplace), 1100);
    assert_eq!(paired_client.balance(&marketplace), 4558);
    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.credit_reserves, 1100);
    assert_eq!(pool.paired_reserves, 4558);
}

#[test]
fn swap_insufficient_credits_fails() {
    let (env, client) = setup();

    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    credit_client.initialize(&Address::generate(&env));

    let paired_token_id = env.register_contract(None, CreditTokenContract);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    paired_client.initialize(&Address::generate(&env));

    let lp = Address::generate(&env);
    mint(&env, &credit_token_id, &lp, 1000);
    mint(&env, &paired_token_id, &lp, 5000);

    let pool_id = sample_pool_id(&env);
    client.create_pool(
        &lp,
        &pool_id,
        &pool_config(
            &env,
            &credit_token_id,
            &paired_token_id,
            "USDC",
            1000,
            5000,
            0,
        ),
    );

    let trader = Address::generate(&env); // never funded
    let result = client.try_swap(&trader, &pool_id, &100, &0);
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
}

// ---- add_liquidity / remove_liquidity tests ----

#[test]
fn add_liquidity_increases_reserves_and_escrow() {
    let amm = amm_setup();
    let provider = Address::generate(&amm.env);
    mint(&amm.env, &amm.credit_token_id, &provider, 1000);
    mint(&amm.env, &amm.paired_token_id, &provider, 5000);

    amm.client
        .add_liquidity(&provider, &amm.pool_id, &100, &500);

    let pool = amm.client.get_pool(&amm.pool_id);
    assert_eq!(pool.credit_reserves, 1100);
    assert_eq!(pool.paired_reserves, 5500);
    assert_eq!(credit_balance(&amm, &amm.marketplace_id), 1100);
    assert_eq!(paired_balance(&amm, &amm.marketplace_id), 5500);
}

#[test]
fn add_liquidity_requires_pool() {
    let amm = amm_setup();
    let provider = Address::generate(&amm.env);
    let fake_pool = BytesN::from_array(&amm.env, &[99u8; 32]);
    let result = amm
        .client
        .try_add_liquidity(&provider, &fake_pool, &100, &500);
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn add_liquidity_nonpositive_fails() {
    let amm = amm_setup();
    let provider = Address::generate(&amm.env);
    let result = amm
        .client
        .try_add_liquidity(&provider, &amm.pool_id, &0, &500);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
    let result = amm
        .client
        .try_add_liquidity(&provider, &amm.pool_id, &100, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn remove_liquidity_refunds_at_deposit_ratio() {
    let amm = amm_setup();

    // The LP contributed 1000 credit / 5000 paired, so removing 200 credits
    // refunds 200 * 5000/1000 = 1000 paired.
    let (credit_refunded, paired_refunded) =
        amm.client.remove_liquidity(&amm.lp, &amm.pool_id, &200);
    assert_eq!(credit_refunded, 200);
    assert_eq!(paired_refunded, 1000);

    let pool = amm.client.get_pool(&amm.pool_id);
    assert_eq!(pool.credit_reserves, 800);
    assert_eq!(pool.paired_reserves, 4000);

    // The LP has its principal back (it had 9000 credits / 45000 paired left
    // after the initial 1000/5000 escrow).
    assert_eq!(credit_balance(&amm, &amm.lp), 9200);
    assert_eq!(paired_balance(&amm, &amm.lp), 46_000);
}

#[test]
fn remove_liquidity_beyond_contribution_fails() {
    let amm = amm_setup();

    // The LP only contributed 1000 credits.
    let result = amm
        .client
        .try_remove_liquidity(&amm.lp, &amm.pool_id, &2000);
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));

    // A provider with no contribution cannot withdraw.
    let outsider = Address::generate(&amm.env);
    let result = amm
        .client
        .try_remove_liquidity(&outsider, &amm.pool_id, &100);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn remove_liquidity_nonpositive_fails() {
    let amm = amm_setup();
    let result = amm.client.try_remove_liquidity(&amm.lp, &amm.pool_id, &0);
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

#[test]
fn remove_liquidity_full_withdrawal_clears_contribution() {
    let amm = amm_setup();

    let (credit_refunded, paired_refunded) =
        amm.client.remove_liquidity(&amm.lp, &amm.pool_id, &1000);
    assert_eq!(credit_refunded, 1000);
    assert_eq!(paired_refunded, 5000);

    // Contribution fully withdrawn; a second withdrawal is NotFound.
    let result = amm.client.try_remove_liquidity(&amm.lp, &amm.pool_id, &100);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ---- place_limit_order tests ----

#[test]
fn place_sell_order_escrows_credits() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);

    let order_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    // Escrow moved to the marketplace; order rests on the book.
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    assert_eq!(credit_client.balance(&seller), 900);

    let order = client.get_order(&order_id);
    assert_eq!(order.side, OrderSide::Sell);
    assert_eq!(order.amount, 100);
    assert_eq!(order.remaining, 100);
    assert_eq!(order.price, 10);

    let orders = client.get_orders(&pool_id);
    assert_eq!(orders.len(), 1);
    assert_eq!(orders.get(0).unwrap().id, order_id);
}

#[test]
fn place_buy_order_escrows_paired() {
    let (env, client, _credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let buyer = Address::generate(&env);
    mint(&env, &paired_token_id, &buyer, 10000);

    let order_id = client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    // Buyer escrowed 100 * 10 = 1000 paired units.
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    assert_eq!(paired_client.balance(&buyer), 9000);

    let order = client.get_order(&order_id);
    assert_eq!(order.side, OrderSide::Buy);
    assert_eq!(order.remaining, 100);
}

#[test]
fn sell_then_buy_fills_and_settles() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);
    mint(&env, &paired_token_id, &buyer, 10000);

    // Seller rests 100 credits at 10.
    let _sell_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    // Buyer takes at 10: fully fills, no order rests.
    let _buy_id = client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);

    // Seller paid 100 credits, received 1000 paired.
    assert_eq!(credit_client.balance(&seller), 900);
    assert_eq!(paired_client.balance(&seller), 1000);
    // Buyer paid 1000 paired, received 100 credits.
    assert_eq!(credit_client.balance(&buyer), 100);
    assert_eq!(paired_client.balance(&buyer), 9000);

    // Book is empty.
    assert_eq!(client.get_orders(&pool_id).len(), 0);
}

#[test]
fn partial_fill_rests_maker_remainder() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);
    mint(&env, &paired_token_id, &buyer, 10000);

    let sell_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    // Buyer only takes 60 of the 100 offered: buy order fully fills.
    let buy_id = client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &60,
        &10,
        &pool_id,
        &paired_token_id,
    );

    // Seller's order rests with 40 remaining; the buy order is gone.
    let sell_order = client.get_order(&sell_id);
    assert_eq!(sell_order.remaining, 40);
    assert_eq!(client.try_get_order(&buy_id), Err(Ok(Error::NotFound)));

    // Only the resting sell order remains on the book.
    assert_eq!(client.get_orders(&pool_id).len(), 1);
}

#[test]
fn buy_fills_at_maker_price_and_refunds_overpayment() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);
    mint(&env, &paired_token_id, &buyer, 10000);

    // Seller rests at 10.
    let _sell_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    // Buyer crosses with a limit of 12; fill settles at the maker price (10),
    // so the buyer's over-escrow of 200 paired units is refunded.
    let _buy_id = client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &100,
        &12,
        &pool_id,
        &paired_token_id,
    );

    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    // Escrowed 1200, paid 1000 to the seller at the maker price, refunded 200.
    assert_eq!(paired_client.balance(&buyer), 8800 + 200);
    assert_eq!(paired_client.balance(&seller), 1000);
    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    assert_eq!(credit_client.balance(&buyer), 100);
}

#[test]
fn non_crossing_orders_rest() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);
    mint(&env, &paired_token_id, &buyer, 10000);

    // Seller asks 12, buyer bids 10 — no crossing, both rest.
    client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &12,
        &pool_id,
        &paired_token_id,
    );
    client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );

    assert_eq!(client.get_orders(&pool_id).len(), 2);
}

#[test]
fn place_limit_order_without_pool_fails() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let trader = Address::generate(&env);
    mint(&env, &credit_token_id, &trader, 1000);
    let fake_pool = BytesN::from_array(&env, &[99u8; 32]);

    let result = client.try_place_limit_order(
        &trader,
        &OrderSide::Sell,
        &100,
        &10,
        &fake_pool,
        &paired_token_id,
    );
    assert_eq!(result, Err(Ok(Error::PoolNotFound)));
}

#[test]
fn place_limit_order_insufficient_escrow_fails() {
    let (env, client, _credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let trader = Address::generate(&env); // never funded

    let result = client.try_place_limit_order(
        &trader,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
}

#[test]
fn place_limit_order_nonpositive_fails() {
    let (env, client, _credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let trader = Address::generate(&env);

    let result = client.try_place_limit_order(
        &trader,
        &OrderSide::Sell,
        &0,
        &10,
        &pool_id,
        &paired_token_id,
    );
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));

    let result = client.try_place_limit_order(
        &trader,
        &OrderSide::Sell,
        &100,
        &0,
        &pool_id,
        &paired_token_id,
    );
    assert_eq!(result, Err(Ok(Error::NonPositiveAmount)));
}

// ---- cancel_order tests ----

#[test]
fn cancel_sell_refunds_credits() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);

    let order_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    client.cancel_order(&seller, &order_id);

    let credit_client = CreditTokenContractClient::new(&env, &credit_token_id);
    assert_eq!(credit_client.balance(&seller), 1000);
    assert_eq!(client.get_orders(&pool_id).len(), 0);
    assert_eq!(client.try_get_order(&order_id), Err(Ok(Error::NotFound)));
}

#[test]
fn cancel_buy_refunds_paired() {
    let (env, client, _credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let buyer = Address::generate(&env);
    mint(&env, &paired_token_id, &buyer, 10000);

    let order_id = client.place_limit_order(
        &buyer,
        &OrderSide::Buy,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    client.cancel_order(&buyer, &order_id);

    let paired_client = CreditTokenContractClient::new(&env, &paired_token_id);
    assert_eq!(paired_client.balance(&buyer), 10000);
}

#[test]
fn cancel_others_order_fails() {
    let (env, client, _credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    let attacker = Address::generate(&env);
    mint(&env, &paired_token_id, &seller, 10000);

    let order_id = client.place_limit_order(
        &seller,
        &OrderSide::Buy,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    let result = client.try_cancel_order(&attacker, &order_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn cancel_missing_order_fails() {
    let (env, client, _credit_token_id, _paired_token_id) = order_setup();
    let trader = Address::generate(&env);
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    let result = client.try_cancel_order(&trader, &missing);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn cancel_closed_order_fails() {
    let (env, client, credit_token_id, paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    let seller = Address::generate(&env);
    mint(&env, &credit_token_id, &seller, 1000);

    let order_id = client.place_limit_order(
        &seller,
        &OrderSide::Sell,
        &100,
        &10,
        &pool_id,
        &paired_token_id,
    );
    client.cancel_order(&seller, &order_id);
    // A cancelled order is removed from storage, so re-cancelling reports it
    // as missing rather than OrderClosed.
    let result = client.try_cancel_order(&seller, &order_id);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

// ---- get_order / get_orders tests ----

#[test]
fn get_order_not_found() {
    let (_env, client) = setup();
    let missing = BytesN::from_array(&_env, &[99u8; 32]);
    let result = client.try_get_order(&missing);
    assert_eq!(result, Err(Ok(Error::NotFound)));
}

#[test]
fn get_orders_empty_pool() {
    let (env, client, _credit_token_id, _paired_token_id) = order_setup();
    let pool_id = sample_pool_id(&env);
    assert_eq!(client.get_orders(&pool_id).len(), 0);
}
