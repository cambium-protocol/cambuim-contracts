// Constant-product AMM pool logic.
// Formula: x * y = k
//
// For a swap of dx tokens into a pool with reserves (x, y) and a fee of
// `fee_bps` basis points applied to the input:
//   net_in  = dx - dx * fee_bps / 10000
//   dy      = (y * net_in) / (x + net_in)
//
// This keeps the product k = x * y non-decreasing for the pool (before fees)
// while routing the fee to liquidity providers.

use crate::{Error, Pool};

/// Calculate the output amount for a swap in a constant-product AMM pool.
///
/// Returns the amount of paired asset the swapper receives for `amount_in`
/// credit tokens, or an error if the calculation overflows or produces zero.
#[allow(dead_code)]
pub fn calculate_swap_output(pool: &Pool, amount_in: i128) -> Result<i128, Error> {
    if amount_in <= 0 {
        return Err(Error::NonPositiveAmount);
    }

    let fee = amount_in
        .checked_mul(pool.fee_bps as i128)
        .ok_or(Error::Overflow)?
        .checked_div(10000)
        .ok_or(Error::Overflow)?;
    let net_in = amount_in.checked_sub(fee).ok_or(Error::Overflow)?.max(0);

    let numerator = pool
        .paired_reserves
        .checked_mul(net_in)
        .ok_or(Error::Overflow)?;

    let denominator = pool
        .credit_reserves
        .checked_add(net_in)
        .ok_or(Error::Overflow)?;

    let amount_out = numerator.checked_div(denominator).ok_or(Error::Overflow)?;

    if amount_out <= 0 {
        return Err(Error::NonPositiveAmount);
    }

    Ok(amount_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env, Symbol};

    fn make_pool(env: &Env, credit: i128, paired: i128, fee_bps: u32) -> Pool {
        Pool {
            id: BytesN::from_array(env, &[1u8; 32]),
            credit_token: soroban_sdk::Address::generate(env),
            paired_token: soroban_sdk::Address::generate(env),
            paired_asset: Symbol::new(env, "XLM"),
            credit_reserves: credit,
            paired_reserves: paired,
            fee_bps,
        }
    }

    #[test]
    fn calculate_swap_output_basic() {
        let env = Env::default();
        let pool = make_pool(&env, 1000, 5000, 0);
        let result = calculate_swap_output(&pool, 100).unwrap();
        assert_eq!(result, 454);
    }

    #[test]
    fn calculate_swap_output_applies_fee() {
        let env = Env::default();
        // 300 bps (3%) fee: net_in = 100 - 3 = 97
        // (5000 * 97) / (1000 + 97) = 485000 / 1097 = 442
        let pool = make_pool(&env, 1000, 5000, 300);
        let result = calculate_swap_output(&pool, 100).unwrap();
        assert_eq!(result, 442);
    }

    #[test]
    fn calculate_swap_output_zero_fails() {
        let env = Env::default();
        let pool = make_pool(&env, 1000, 5000, 0);
        assert_eq!(
            calculate_swap_output(&pool, 0),
            Err(Error::NonPositiveAmount)
        );
    }

    #[test]
    fn calculate_swap_output_negative_fails() {
        let env = Env::default();
        let pool = make_pool(&env, 1000, 5000, 0);
        assert_eq!(
            calculate_swap_output(&pool, -100),
            Err(Error::NonPositiveAmount)
        );
    }
}
