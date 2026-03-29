use crate::errors::InsightArenaError;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Minimum liquidity to prevent division by zero and manipulation.
pub const MIN_LIQUIDITY: i128 = 1000;

/// Default trading fee in basis points (0.3% = 30 bps).
pub const DEFAULT_FEE_BPS: u32 = 30;

// ── AMM Math Functions ────────────────────────────────────────────────────────

/// Calculate output amount for a swap using constant product formula.
///
/// Formula: amount_out = (amount_in * reserve_out) / (reserve_in + amount_in)
/// Then apply trading fee: amount_out_with_fee = amount_out * (1 - fee_bps/10000)
pub fn calculate_swap_output(
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, InsightArenaError> {
    if amount_in <= 0 || reserve_in <= 0 || reserve_out <= 0 {
        return Err(InsightArenaError::InvalidInput);
    }

    let numerator = amount_in
        .checked_mul(reserve_out)
        .ok_or(InsightArenaError::Overflow)?;

    let denominator = reserve_in
        .checked_add(amount_in)
        .ok_or(InsightArenaError::Overflow)?;

    let amount_out = numerator
        .checked_div(denominator)
        .ok_or(InsightArenaError::Overflow)?;

    let fee_multiplier = 10_000i128
        .checked_sub(fee_bps as i128)
        .ok_or(InsightArenaError::Overflow)?;

    let amount_out_with_fee = amount_out
        .checked_mul(fee_multiplier)
        .ok_or(InsightArenaError::Overflow)?
        .checked_div(10_000)
        .ok_or(InsightArenaError::Overflow)?;

    Ok(amount_out_with_fee)
}

// ── Helper Functions ──────────────────────────────────────────────────────────

// TODO: Add helper functions

// ── Liquidity Management ──────────────────────────────────────────────────────

/// Calculate LP tokens to mint for a deposit
pub fn calculate_lp_tokens(
    deposit_amount: i128,
    total_liquidity: i128,
    total_lp_supply: i128,
) -> Result<i128, InsightArenaError> {
    if deposit_amount <= 0 {
        return Err(InsightArenaError::InvalidInput);
    }

    // First deposit: mint tokens equal to deposit
    if total_lp_supply == 0 || total_liquidity == 0 {
        return Ok(deposit_amount);
    }

    // Subsequent deposits: mint proportionally
    let lp_tokens = deposit_amount
        .checked_mul(total_lp_supply)
        .ok_or(InsightArenaError::Overflow)?
        .checked_div(total_liquidity)
        .ok_or(InsightArenaError::Overflow)?;

    Ok(lp_tokens)
}

// TODO: add_liquidity
// TODO: remove_liquidity

// ── Trading Functions ─────────────────────────────────────────────────────────

// TODO: swap_outcome
// TODO: get_outcome_price

// ── Analytics ─────────────────────────────────────────────────────────────────

// TODO: get_pool_stats
// TODO: get_lp_position

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::InsightArenaError;

    #[test]
    fn test_calculate_swap_output_zero_input_fails() {
        // Should return InvalidInput error
        let result = calculate_swap_output(0, 1000, 1000, 30);
        assert_eq!(result, Err(InsightArenaError::InvalidInput));
    }

    #[test]
    fn test_calculate_swap_output_zero_reserve_fails() {
        // Should return InvalidInput error
        let result_in = calculate_swap_output(100, 0, 1000, 30);
        assert_eq!(result_in, Err(InsightArenaError::InvalidInput));

        let result_out = calculate_swap_output(100, 1000, 0, 30);
        assert_eq!(result_out, Err(InsightArenaError::InvalidInput));
    }

    #[test]
    fn test_calculate_swap_output_overflow_protection() {
        // Try: i128::MAX → Should return Overflow error
        let result = calculate_swap_output(i128::MAX, 1000, 1000, 30);
        assert_eq!(result, Err(InsightArenaError::Overflow));
    }

    #[test]
    fn test_calculate_lp_tokens_first_deposit() {
        // Deposit: 1000, Liquidity: 0, Supply: 0 → Expected: 1000
        assert_eq!(calculate_lp_tokens(1000, 0, 0), Ok(1000));
    }

    #[test]
    fn test_calculate_lp_tokens_second_deposit_equal() {
        // Deposit: 1000, Liquidity: 1000, Supply: 1000 → Expected: 1000
        assert_eq!(calculate_lp_tokens(1000, 1000, 1000), Ok(1000));
    }

    #[test]
    fn test_calculate_lp_tokens_second_deposit_half() {
        // Deposit: 500, Liquidity: 1000, Supply: 1000 → Expected: 500
        assert_eq!(calculate_lp_tokens(500, 1000, 1000), Ok(500));
    }

    #[test]
    fn test_calculate_lp_tokens_second_deposit_double() {
        // Deposit: 2000, Liquidity: 1000, Supply: 1000 → Expected: 2000
        assert_eq!(calculate_lp_tokens(2000, 1000, 1000), Ok(2000));
    }
}
