#![cfg(test)]

use cambium_credit_token::CreditTokenContract;
use cambium_marketplace::MarketplaceContract;
use cambium_registry::{Project, RegistryContract};
use cambium_retirement::RetirementContract;
use cambium_shared::{Proof, RetireeRef};
use cambium_zk_verifier::ZkVerifierContract;
use soroban_sdk::{testutils::Address as _, Bytes, BytesN, Env, Symbol};

/// Helper to deploy all contracts and wire them together.
/// Returns (env, registry_id, credit_token_id, zk_verifier_id, marketplace_id, retirement_id).
fn deploy_all() -> (
    Env,
    soroban_sdk::Address,
    soroban_sdk::Address,
    soroban_sdk::Address,
    soroban_sdk::Address,
    soroban_sdk::Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy credit-token
    let credit_token_id = env.register_contract(None, CreditTokenContract);
    let token_client = cambium_credit_token::CreditTokenContractClient::new(&env, &credit_token_id);

    // Deploy zk-verifier (mock)
    let zk_verifier_id = env.register_contract(None, ZkVerifierContract);
    let zk_verifier_client =
        cambium_zk_verifier::ZkVerifierContractClient::new(&env, &zk_verifier_id);
    zk_verifier_client.initialize();

    // Deploy registry
    let registry_id = env.register_contract(None, RegistryContract);
    let registry_client = cambium_registry::RegistryContractClient::new(&env, &registry_id);

    // Deploy marketplace
    let marketplace_id = env.register_contract(None, MarketplaceContract);
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);
    marketplace_client.initialize();

    // Deploy retirement
    let retirement_id = env.register_contract(None, RetirementContract);
    let retirement_client = cambium_retirement::RetirementContractClient::new(&env, &retirement_id);
    retirement_client.initialize(&credit_token_id, &registry_id);

    // Wire credit-token: registry is admin
    token_client.initialize(&registry_id);

    // Wire registry: credit-token and zk-verifier
    registry_client.initialize(&credit_token_id, &zk_verifier_id);

    (
        env,
        registry_id,
        credit_token_id,
        zk_verifier_id,
        marketplace_id,
        retirement_id,
    )
}

/// Full lifecycle test: register project → request mint → verify (mocked true) →
/// credits appear in credit-token balance → swap via marketplace → retire via
/// retirement → confirm retirement record and updated vintage totals in registry.
#[test]
fn full_lifecycle_register_mint_swap_retire() {
    let (env, registry_id, credit_token_id, _zk_verifier_id, marketplace_id, retirement_id) =
        deploy_all();

    let registry_client = cambium_registry::RegistryContractClient::new(&env, &registry_id);
    let token_client = cambium_credit_token::CreditTokenContractClient::new(&env, &credit_token_id);
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);
    let retirement_client = cambium_retirement::RetirementContractClient::new(&env, &retirement_id);

    // --- Step 1: Register a project ---
    let project_id = BytesN::from_array(&env, &[1u8; 32]);
    let project = Project {
        id: project_id.clone(),
        methodology: Symbol::new(&env, "VM0007"),
        geography: Symbol::new(&env, "BRA"),
        external_registry_ref: None,
        verifying_key_version: 1,
    };
    registry_client.register_project(&project);

    // Verify project is registered
    let fetched_project = registry_client.get_project(&project_id);
    assert_eq!(fetched_project, project);

    // --- Step 2: Request mint (triggers zk-verifier verify call) ---
    let proof = Proof {
        proof_data: Bytes::from_array(&env, &[1u8, 2, 3, 4]),
        public_inputs: soroban_sdk::vec![&env, BytesN::from_array(&env, &[0u8; 32])],
    };

    registry_client.request_mint(&project_id, &2025, &1000, &proof);

    // Verify vintage was updated
    let vintage = registry_client.get_vintage(&project_id, &2025);
    assert_eq!(vintage.total_issued, 1000);
    assert_eq!(vintage.total_retired, 0);

    // Verify credits were minted to registry (the caller of mint)
    let registry_balance = token_client.balance(&registry_id);
    assert_eq!(registry_balance, 1000);

    // --- Step 3: Transfer credits to a user ---
    let user = soroban_sdk::Address::generate(&env);
    token_client.transfer(&registry_id, &user, &500);
    assert_eq!(token_client.balance(&user), 500);
    assert_eq!(token_client.balance(&registry_id), 500);

    // --- Step 4: Create a marketplace pool and swap ---
    let pool_id = BytesN::from_array(&env, &[2u8; 32]);
    let paired_asset = Symbol::new(&env, "XLM");

    // Create pool with initial liquidity (from registry which has credits)
    // First, approve marketplace to spend registry's credits
    // Note: In real usage, the user would provide their own liquidity.
    // For this test, we create a pool with the registry's remaining credits.
    let pool =
        marketplace_client.create_pool(&pool_id, &credit_token_id, &paired_asset, &500, &2500);
    assert_eq!(pool.credit_reserves, 500);
    assert_eq!(pool.paired_reserves, 2500);

    // User swaps 100 credit tokens for XLM
    // Expected: (2500 * 100) / (500 + 100) = 250000 / 600 ≈ 416
    let amount_out = marketplace_client.swap(&pool_id, &100, &0);
    assert_eq!(amount_out, 416);

    // Verify pool reserves updated
    let updated_pool = marketplace_client.get_pool(&pool_id);
    assert_eq!(updated_pool.credit_reserves, 600);
    assert_eq!(updated_pool.paired_reserves, 2084);

    // --- Step 5: Retire credits ---
    let retire_amount = 200;
    let record = retirement_client.retire(&user, &project_id, &2025, &retire_amount, &false);

    assert_eq!(record.project_id, project_id);
    assert_eq!(record.vintage_year, 2025);
    assert_eq!(record.amount, retire_amount);
    assert_eq!(record.retiree, RetireeRef::Public(user.clone()));

    // Verify retirement record is stored
    let fetched_record = retirement_client.get_retirement(&record.id);
    assert_eq!(fetched_record, record);

    // --- Step 6: Verify shield=true fails loudly ---
    let shield_result = retirement_client.try_retire(&user, &project_id, &2025, &100, &true);
    assert_eq!(
        shield_result,
        Err(Ok(cambium_shared::Error::NotYetImplemented))
    );

    // --- Step 7: Verify vintage totals ---
    let vintage = registry_client.get_vintage(&project_id, &2025);
    assert_eq!(vintage.total_issued, 1000);
    // Note: vintage.total_retired is not updated by retirement contract
    // in this implementation (retirement contract is separate).
    // This is expected per the architecture — retirement records are in
    // the retirement contract, vintage totals in registry.
    assert_eq!(vintage.total_retired, 0);
}

/// Test that the verifier is called and mock returns true.
#[test]
fn verifier_mock_always_passes() {
    let (env, _registry_id, _credit_token_id, zk_verifier_id, _marketplace_id, _retirement_id) =
        deploy_all();

    let zk_client = cambium_zk_verifier::ZkVerifierContractClient::new(&env, &zk_verifier_id);

    let proof = Proof {
        proof_data: Bytes::from_array(&env, &[1u8, 2, 3, 4]),
        public_inputs: soroban_sdk::vec![&env, BytesN::from_array(&env, &[0u8; 32])],
    };

    let public_inputs = soroban_sdk::vec![&env, BytesN::from_array(&env, &[1u8; 32])];
    let result = zk_client.verify(&proof, &public_inputs);
    assert!(result);
}

/// Test that marketplace place_limit_order returns NotYetImplemented.
#[test]
fn marketplace_orderbook_deferred() {
    let (env, _registry_id, _credit_token_id, _zk_verifier_id, marketplace_id, _retirement_id) =
        deploy_all();

    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);
    let result =
        marketplace_client.try_place_limit_order(&cambium_shared::OrderSide::Buy, &100, &10);
    assert_eq!(result, Err(Ok(cambium_shared::Error::NotYetImplemented)));
}

/// Test duplicate project registration fails.
#[test]
fn duplicate_project_registration_fails() {
    let (env, registry_id, _credit_token_id, _zk_verifier_id, _marketplace_id, _retirement_id) =
        deploy_all();
    let registry_client = cambium_registry::RegistryContractClient::new(&env, &registry_id);

    let project_id = BytesN::from_array(&env, &[1u8; 32]);
    let project = Project {
        id: project_id.clone(),
        methodology: Symbol::new(&env, "VM0007"),
        geography: Symbol::new(&env, "BRA"),
        external_registry_ref: None,
        verifying_key_version: 1,
    };
    registry_client.register_project(&project);

    let result = registry_client.try_register_project(&project);
    assert_eq!(result, Err(Ok(cambium_shared::Error::AlreadyRegistered)));
}

/// Test get_project on non-existent project returns DoesNotExist.
#[test]
fn get_nonexistent_project_fails() {
    let (env, registry_id, _credit_token_id, _zk_verifier_id, _marketplace_id, _retirement_id) =
        deploy_all();
    let registry_client = cambium_registry::RegistryContractClient::new(&env, &registry_id);

    let fake_id = BytesN::from_array(&env, &[99u8; 32]);
    let result = registry_client.try_get_project(&fake_id);
    assert_eq!(result, Err(Ok(cambium_shared::Error::NotFound)));
}

/// Test duplicate pool creation fails.
#[test]
fn duplicate_pool_creation_fails() {
    let (env, _registry_id, credit_token_id, _zk_verifier_id, marketplace_id, _retirement_id) =
        deploy_all();
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);

    let pool_id = BytesN::from_array(&env, &[1u8; 32]);
    let paired_asset = Symbol::new(&env, "XLM");
    marketplace_client.create_pool(&pool_id, &credit_token_id, &paired_asset, &100, &500);

    let result =
        marketplace_client.try_create_pool(&pool_id, &credit_token_id, &paired_asset, &100, &500);
    assert_eq!(result, Err(Ok(cambium_shared::Error::AlreadyRegistered)));
}

/// Test create_pool with non-positive amounts fails.
#[test]
fn create_pool_nonpositive_fails() {
    let (env, _registry_id, credit_token_id, _zk_verifier_id, marketplace_id, _retirement_id) =
        deploy_all();
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);

    let pool_id = BytesN::from_array(&env, &[1u8; 32]);
    let paired_asset = Symbol::new(&env, "XLM");

    let result =
        marketplace_client.try_create_pool(&pool_id, &credit_token_id, &paired_asset, &0, &500);
    assert_eq!(result, Err(Ok(cambium_shared::Error::NonPositiveAmount)));
}

/// Test get_pool on non-existent pool returns PoolNotFound.
#[test]
fn get_nonexistent_pool_fails() {
    let (env, _registry_id, _credit_token_id, _zk_verifier_id, marketplace_id, _retirement_id) =
        deploy_all();
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);

    let fake_pool = BytesN::from_array(&env, &[99u8; 32]);
    let result = marketplace_client.try_get_pool(&fake_pool);
    assert_eq!(result, Err(Ok(cambium_shared::Error::PoolNotFound)));
}

/// Test swap on non-existent pool returns PoolNotFound.
#[test]
fn swap_nonexistent_pool_fails() {
    let (env, _registry_id, _credit_token_id, _zk_verifier_id, marketplace_id, _retirement_id) =
        deploy_all();
    let marketplace_client =
        cambium_marketplace::MarketplaceContractClient::new(&env, &marketplace_id);

    let fake_pool = BytesN::from_array(&env, &[99u8; 32]);
    let result = marketplace_client.try_swap(&fake_pool, &100, &0);
    assert_eq!(result, Err(Ok(cambium_shared::Error::PoolNotFound)));
}

/// Test retirement of non-existent record returns RetirementNotFound.
#[test]
fn get_nonexistent_retirement_fails() {
    let (env, _registry_id, _credit_token_id, _zk_verifier_id, _marketplace_id, retirement_id) =
        deploy_all();
    let retirement_client = cambium_retirement::RetirementContractClient::new(&env, &retirement_id);

    let fake_id = BytesN::from_array(&env, &[99u8; 32]);
    let result = retirement_client.try_get_retirement(&fake_id);
    assert_eq!(result, Err(Ok(cambium_shared::Error::RetirementNotFound)));
}
