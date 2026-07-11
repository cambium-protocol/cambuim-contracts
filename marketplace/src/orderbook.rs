// Limit order book logic (deferred from Day 4).
//
// This module will implement a limit order book for larger trades where
// price slippage from the AMM would be unacceptable.
//
// Planned features:
// - Place limit orders at specific prices
// - Match orders when prices cross
// - Cancel open orders
// - Support both buy and sell sides
//
// See README Roadmap for timeline.

use crate::Error;

/// Order ID type (placeholder).
#[allow(dead_code)]
pub type OrderId = soroban_sdk::BytesN<32>;

/// Place a limit order (not yet implemented).
#[allow(dead_code)]
pub fn place_limit_order(
    _env: &soroban_sdk::Env,
    _side: crate::OrderSide,
    _amount: i128,
    _price: i128,
) -> Result<(), Error> {
    Err(Error::NotYetImplemented)
}
