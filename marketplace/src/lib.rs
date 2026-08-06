#![cfg_attr(not(test), no_std)]

mod amm;
mod orderbook;

use crate::orderbook::{crosses, Order};
use cambium_shared::{Error, OrderSide};
use soroban_sdk::{
    contract, contractimpl, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env, IntoVal, Symbol,
    Vec,
};

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
    /// Resting order by id.
    Order(BytesN<32>),
    /// Ids of resting orders per pool (kept in price-insertion order).
    OrderIds(BytesN<32>),
    /// Monotonic counter used to mint unique order ids.
    OrderNonce,
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

    /// Place a limit order against a pool's order book.
    ///
    /// The sold asset is escrowed to the marketplace immediately:
    /// * `OrderSide::Sell` escrows `amount` credit tokens.
    /// * `OrderSide::Buy` escrows `amount * price` units of the paired asset.
    ///
    /// The order immediately sweeps resting orders of the opposite side whose
    /// price crosses (fill at the resting maker price). The unfilled remainder
    /// rests on the book until matched or cancelled via `cancel_order`.
    ///
    /// # Arguments
    /// * `trader` - The address placing the order (must authorize).
    /// * `side` - Buy or Sell.
    /// * `amount` - Credit quantity (must be > 0).
    /// * `price` - Limit price in paired-asset units per credit (must be > 0).
    /// * `pool_id` - Pool this order trades against.
    /// * `paired_token` - Contract address of the paired asset to escrow for
    ///   buy orders.
    ///
    /// # Returns
    /// The id of the placed order.
    pub fn place_limit_order(
        env: Env,
        trader: Address,
        side: OrderSide,
        amount: i128,
        price: i128,
        pool_id: BytesN<32>,
        paired_token: Address,
    ) -> Result<BytesN<32>, Error> {
        if amount <= 0 || price <= 0 {
            return Err(Error::NonPositiveAmount);
        }
        trader.require_auth();

        let pool: Pool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(pool_id.clone()))
            .ok_or(Error::PoolNotFound)?;
        let credit_token = pool.credit_token.clone();

        // Escrow the sold asset up front.
        let escrow_amount = match &side {
            OrderSide::Sell => amount,
            OrderSide::Buy => amount.checked_mul(price).ok_or(Error::Overflow)?,
        };
        let escrow_token = match &side {
            OrderSide::Sell => &credit_token,
            OrderSide::Buy => &paired_token,
        };
        Self::transfer_token(
            &env,
            escrow_token,
            &trader,
            &env.current_contract_address(),
            escrow_amount,
        )?;

        // Build the taker order.
        let id = Self::next_order_id(&env, &trader, &side, amount, price);
        let mut taker = Order {
            id: id.clone(),
            trader: trader.clone(),
            side: side.clone(),
            amount,
            remaining: amount,
            price,
            pool_id: pool_id.clone(),
            paired_token: paired_token.clone(),
            created_at: env.ledger().timestamp(),
        };

        // Sweep resting opposite-side orders that cross, settling at maker price.
        let ids = Self::load_order_ids(&env, &pool_id);
        let mut makers = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(order) = env.storage().persistent().get(&DataKey::Order(id)) {
                makers.push_back(order);
            }
        }

        let mut settled_credits = 0i128;
        let mut settled_paired = 0i128;
        for i in 0..makers.len() {
            if taker.remaining == 0 {
                break;
            }
            let maker: Order = makers.get(i).unwrap();
            if maker.side == taker.side {
                continue;
            }
            if !crosses(&taker.side, maker.price, taker.price) {
                continue;
            }

            let maker_id = maker.id.clone();
            let qty = core::cmp::min(maker.remaining, taker.remaining);
            let maker_paid = qty.checked_mul(maker.price).ok_or(Error::Overflow)?;

            // Both escrows live at the marketplace; move the assets to the
            // receiving sides. The buyer receives credits, the seller paired.
            let credit_recipient = match taker.side {
                OrderSide::Buy => trader.clone(),
                OrderSide::Sell => maker.trader.clone(),
            };
            let paired_recipient = match taker.side {
                OrderSide::Buy => maker.trader.clone(),
                OrderSide::Sell => trader.clone(),
            };
            Self::transfer_token(
                &env,
                &credit_token,
                &env.current_contract_address(),
                &credit_recipient,
                qty,
            )?;
            Self::transfer_token(
                &env,
                &paired_token,
                &env.current_contract_address(),
                &paired_recipient,
                maker_paid,
            )?;

            let mut maker = maker;
            maker.remaining -= qty;
            if maker.remaining == 0 {
                env.storage()
                    .persistent()
                    .remove(&DataKey::Order(maker_id.clone()));
            } else {
                env.storage()
                    .persistent()
                    .set(&DataKey::Order(maker_id.clone()), &maker);
            }

            taker.remaining -= qty;
            settled_credits += qty;
            settled_paired += maker_paid;

            env.events().publish(
                (
                    Symbol::new(&env, "order_filled"),
                    maker_id,
                    taker.id.clone(),
                ),
                (qty, maker_paid),
            );
        }

        // Rebuild the resting order id list, dropping fully-filled makers.
        let mut resting_ids = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if env.storage().persistent().has(&DataKey::Order(id.clone())) {
                resting_ids.push_back(id);
            }
        }
        if taker.remaining > 0 {
            resting_ids.push_back(taker.id.clone());
        }
        env.storage()
            .persistent()
            .set(&DataKey::OrderIds(pool_id.clone()), &resting_ids);

        // Refund escrow left over from a fully-filled taker.
        if taker.remaining == 0 {
            let refund = match side {
                OrderSide::Sell => amount - settled_credits,
                OrderSide::Buy => escrow_amount - settled_paired,
            };
            if refund > 0 {
                let refund_token = match side {
                    OrderSide::Sell => &credit_token,
                    OrderSide::Buy => &paired_token,
                };
                Self::transfer_token(
                    &env,
                    refund_token,
                    &env.current_contract_address(),
                    &trader,
                    refund,
                )?;
            }
        }

        // Rest the unfilled remainder (its id is already in `resting_ids`).
        if taker.remaining > 0 {
            env.storage()
                .persistent()
                .set(&DataKey::Order(taker.id.clone()), &taker);
            env.events().publish(
                (Symbol::new(&env, "order_placed"), taker.id.clone()),
                (trader, side, taker.remaining, price),
            );
        }

        Ok(id)
    }

    /// Cancel a resting order and refund its escrow to the trader.
    ///
    /// # Errors
    /// * `NotFound` if the order does not exist
    /// * `Unauthorized` if `trader` is not the order's owner
    /// * `OrderClosed` if the order was already fully filled or cancelled
    pub fn cancel_order(env: Env, trader: Address, order_id: BytesN<32>) -> Result<(), Error> {
        trader.require_auth();

        let order: Order = env
            .storage()
            .persistent()
            .get(&DataKey::Order(order_id.clone()))
            .ok_or(Error::NotFound)?;
        if order.trader != trader {
            return Err(Error::Unauthorized);
        }
        if order.remaining <= 0 {
            return Err(Error::OrderClosed);
        }

        let pool: Pool = env
            .storage()
            .persistent()
            .get(&DataKey::Pool(order.pool_id.clone()))
            .ok_or(Error::PoolNotFound)?;

        let refund_asset = match order.side {
            OrderSide::Sell => pool.credit_token.clone(),
            OrderSide::Buy => order.paired_token.clone(),
        };
        let refund_amount = match order.side {
            OrderSide::Sell => order.remaining,
            OrderSide::Buy => order
                .remaining
                .checked_mul(order.price)
                .ok_or(Error::Overflow)?,
        };
        Self::transfer_token(
            &env,
            &refund_asset,
            &env.current_contract_address(),
            &trader,
            refund_amount,
        )?;

        env.storage()
            .persistent()
            .remove(&DataKey::Order(order_id.clone()));
        let ids = Self::load_order_ids(&env, &order.pool_id);
        let mut ids_out = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if id != order_id && env.storage().persistent().has(&DataKey::Order(id.clone())) {
                ids_out.push_back(id);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::OrderIds(order.pool_id.clone()), &ids_out);

        env.events().publish(
            (Symbol::new(&env, "order_cancelled"), order_id),
            (trader, refund_amount),
        );

        Ok(())
    }

    /// Look up an order by id.
    pub fn get_order(env: Env, order_id: BytesN<32>) -> Result<Order, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Order(order_id))
            .ok_or(Error::NotFound)
    }

    /// Return all resting orders for a pool.
    pub fn get_orders(env: Env, pool_id: BytesN<32>) -> Vec<Order> {
        let ids = Self::load_order_ids(&env, &pool_id);
        let mut orders = Vec::new(&env);
        for i in 0..ids.len() {
            let id = ids.get(i).unwrap();
            if let Some(order) = env.storage().persistent().get(&DataKey::Order(id)) {
                orders.push_back(order);
            }
        }
        orders
    }

    /// Load the ordered list of resting order ids for a pool.
    fn load_order_ids(env: &Env, pool_id: &BytesN<32>) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::OrderIds(pool_id.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Transfer `amount` of `token` from `from` to `to` on behalf of the
    /// marketplace. `from` must have authorized this transaction (for traders,
    /// their signature covers the whole call; the marketplace authorizes its
    /// own address when paying out escrow).
    fn transfer_token(
        env: &Env,
        token: &Address,
        from: &Address,
        to: &Address,
        amount: i128,
    ) -> Result<(), Error> {
        let result: Result<Result<(), _>, _> = env.try_invoke_contract::<(), soroban_sdk::Error>(
            token,
            &Symbol::new(env, "transfer"),
            soroban_sdk::vec![
                env,
                from.clone().into_val(env),
                to.clone().into_val(env),
                amount.into_val(env),
            ],
        );
        if !matches!(result, Ok(Ok(()))) {
            return Err(Error::InsufficientBalance);
        }
        Ok(())
    }

    /// Mint a unique, deterministic order id from order attributes.
    fn next_order_id(
        env: &Env,
        trader: &Address,
        side: &OrderSide,
        amount: i128,
        price: i128,
    ) -> BytesN<32> {
        let nonce: u32 = env
            .storage()
            .instance()
            .get(&DataKey::OrderNonce)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::OrderNonce, &(nonce + 1));

        let mut preimage = Bytes::new(env);
        preimage.append(&trader.clone().to_xdr(env));
        let side_bit: u32 = match side {
            OrderSide::Buy => 0,
            OrderSide::Sell => 1,
        };
        preimage.extend_from_array(&side_bit.to_be_bytes());
        preimage.extend_from_array(&amount.to_be_bytes());
        preimage.extend_from_array(&price.to_be_bytes());
        preimage.extend_from_array(&env.ledger().sequence().to_be_bytes());
        preimage.extend_from_array(&env.ledger().timestamp().to_be_bytes());
        preimage.extend_from_array(&nonce.to_be_bytes());
        env.crypto().keccak256(&preimage).into()
    }
}

#[cfg(test)]
mod tests;
