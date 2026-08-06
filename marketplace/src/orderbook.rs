// Limit order book for larger trades where AMM slippage is unacceptable.
//
// Orders escrow the sold asset up front (credit tokens for sells, the paired
// asset for buys) and rest until an opposite order crosses their price. New
// orders sweep resting orders of the opposite side in price-time priority,
// settling at the resting (maker) price.

use cambium_shared::OrderSide;
use soroban_sdk::{contracttype, Address, BytesN};

/// A resting limit order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Order {
    /// Unique order identifier.
    pub id: BytesN<32>,
    /// Address that placed the order and funded its escrow.
    pub trader: Address,
    /// Which side of the book this order rests on.
    pub side: OrderSide,
    /// Original credit quantity of the order.
    pub amount: i128,
    /// Unfilled credit quantity remaining (0 once filled or cancelled).
    pub remaining: i128,
    /// Limit price in paired-asset units per credit token.
    pub price: i128,
    /// Pool this order trades against (defines the credit token).
    pub pool_id: BytesN<32>,
    /// Contract address of the paired asset escrowed for buy orders.
    pub paired_token: Address,
    /// Ledger timestamp when the order was placed.
    pub created_at: u64,
}

/// A record of credits and paired asset exchanged in a fill.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Fill {
    /// Maker (resting) order id.
    pub maker_id: BytesN<32>,
    /// Taker (incoming) order id.
    pub taker_id: BytesN<32>,
    /// Credit tokens transferred from the seller to the buyer.
    pub credits: i128,
    /// Paired-asset units paid from the buyer to the seller.
    pub paired: i128,
}

/// Whether a taker order crosses the resting maker order's price.
pub fn crosses(taker: &OrderSide, maker_price: i128, taker_price: i128) -> bool {
    match taker {
        OrderSide::Buy => maker_price <= taker_price,
        OrderSide::Sell => maker_price >= taker_price,
    }
}
