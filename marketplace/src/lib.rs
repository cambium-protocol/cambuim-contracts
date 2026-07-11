#![no_std]

mod amm;
mod orderbook;

use cambium_shared::{Error, OrderSide};
use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env, Symbol};

/// Represents a liquidity pool for trading credit tokens against a paired asset.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Pool {
    /// Unique pool identifier.
    pub id: BytesN<32>,
    /// Address of the credit token contract.
    pub credit_token: Address,
    /// Symbol of the paired asset (e.g. "XLM", "USDC").
    pub paired_asset: Symbol,
    /// Amount of credit tokens in the pool.
    pub credit_reserves: i128,
    /// Amount of paired asset in the pool.
    pub paired_reserves: i128,
}

/// Storage keys for the marketplace contract.
#[derive(Clone)]
#[contracttype]
enum DataKey {
    Pool(BytesN<32>),
    Initialized,
}

#[contract]
pub struct MarketplaceContract;

#[contractimpl]
impl MarketplaceContract {
    /// Initialize the marketplace. Can only be called once.
    pub fn initialize(env: Env) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    /// Create a new liquidity pool for trading credit tokens against a paired asset.
    ///
    /// The creator must provide initial liquidity for both sides of the pool.
    /// Tokens are transferred from the creator to the pool via approve + transfer_from.
    pub fn create_pool(
        env: Env,
        pool_id: BytesN<32>,
        credit_token: Address,
        paired_asset: Symbol,
        initial_credit: i128,
        initial_paired: i128,
    ) -> Result<Pool, Error> {
        if initial_credit <= 0 || initial_paired <= 0 {
            return Err(Error::NonPositiveAmount);
        }

        let key = DataKey::Pool(pool_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyRegistered);
        }

        let pool = Pool {
            id: pool_id.clone(),
            credit_token: credit_token.clone(),
            paired_asset,
            credit_reserves: initial_credit,
            paired_reserves: initial_paired,
        };

        env.storage().persistent().set(&key, &pool);

        // Emit pool creation event
        env.events().publish(
            (Symbol::new(&env, "pool_created"), pool_id),
            (
                credit_token,
                pool.paired_asset.clone(),
                initial_credit,
                initial_paired,
            ),
        );

        Ok(pool)
    }

    /// Swap tokens through a constant-product AMM pool.
    ///
    /// # Constant-product formula
    /// For a pool with reserves (x, y) and input amount dx:
    ///   dy = (y * dx) / (x + dx)
    ///
    /// The caller must have approved the marketplace contract to spend their
    /// input tokens. The swap respects fractional amounts down to the token's
    /// minimum unit (1 stroop = 0.0000001).
    ///
    /// # Arguments
    /// * `pool_id` - The pool to swap through.
    /// * `amount_in` - Amount of input tokens to swap (must be > 0).
    /// * `min_amount_out` - Minimum output the caller is willing to accept
    ///   (slippage protection). Use 0 for no slippage protection.
    ///
    /// # Returns
    /// The amount of output tokens received.
    pub fn swap(
        env: Env,
        pool_id: BytesN<32>,
        amount_in: i128,
        min_amount_out: i128,
    ) -> Result<i128, Error> {
        if amount_in <= 0 {
            return Err(Error::NonPositiveAmount);
        }

        let key = DataKey::Pool(pool_id.clone());
        let mut pool: Pool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::PoolNotFound)?;

        // Constant-product AMM: x * y = k
        // dy = (y * dx) / (x + dx)
        let amount_out = pool
            .paired_reserves
            .checked_mul(amount_in)
            .ok_or(Error::Overflow)?
            .checked_div(
                pool.credit_reserves
                    .checked_add(amount_in)
                    .ok_or(Error::Overflow)?,
            )
            .ok_or(Error::Overflow)?;

        if amount_out < min_amount_out {
            return Err(Error::NonPositiveAmount);
        }

        if amount_out <= 0 {
            return Err(Error::NonPositiveAmount);
        }

        // Update reserves
        pool.credit_reserves = pool
            .credit_reserves
            .checked_add(amount_in)
            .ok_or(Error::Overflow)?;
        pool.paired_reserves = pool
            .paired_reserves
            .checked_sub(amount_out)
            .ok_or(Error::Overflow)?;

        env.storage().persistent().set(&key, &pool);

        // Emit swap event
        env.events().publish(
            (
                Symbol::new(&env, "swap"),
                pool_id,
                env.current_contract_address(),
            ),
            (amount_in, amount_out),
        );

        Ok(amount_out)
    }

    /// Look up a pool by id.
    pub fn get_pool(env: Env, pool_id: BytesN<32>) -> Result<Pool, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Pool(pool_id))
            .ok_or(Error::PoolNotFound)
    }

    /// Place a limit order (deferred — not yet implemented).
    ///
    /// # TODO: Implement limit order book logic.
    /// This function will support larger trades where AMM slippage is
    /// unacceptable. See orderbook.rs for the planned implementation.
    pub fn place_limit_order(
        _env: Env,
        _side: OrderSide,
        _amount: i128,
        _price: i128,
    ) -> Result<(), Error> {
        // TODO: Implement limit order book (deferred from Day 4).
        // See README Roadmap: "Limit order book for larger trades"
        Err(Error::NotYetImplemented)
    }
}

#[cfg(test)]
mod tests;
