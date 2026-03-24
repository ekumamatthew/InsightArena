use soroban_sdk::{contracttype, symbol_short, Address, Env, String, Symbol, Vec};

use crate::config::{self, PERSISTENT_BUMP, PERSISTENT_THRESHOLD};
use crate::errors::InsightArenaError;
use crate::storage_types::{DataKey, Market};

// ── Params struct ─────────────────────────────────────────────────────────────
// Soroban limits contract functions to 10 parameters. Bundling the market
// creation fields into a single `#[contracttype]` struct keeps the ABI legal
// while preserving full type-safety for every individual field.

#[contracttype]
#[derive(Clone, Debug)]
pub struct CreateMarketParams {
    pub title: String,
    pub description: String,
    pub category: Symbol,
    pub outcomes: Vec<Symbol>,
    pub end_time: u64,
    pub resolution_time: u64,
    pub creator_fee_bps: u32,
    pub min_stake: i128,
    pub max_stake: i128,
    pub is_public: bool,
}

// ── TTL helpers ───────────────────────────────────────────────────────────────

fn bump_market(env: &Env, market_id: u64) {
    env.storage().persistent().extend_ttl(
        &DataKey::Market(market_id),
        PERSISTENT_THRESHOLD,
        PERSISTENT_BUMP,
    );
}

fn bump_counter(env: &Env) {
    env.storage().persistent().extend_ttl(
        &DataKey::MarketCount,
        PERSISTENT_THRESHOLD,
        PERSISTENT_BUMP,
    );
}

// ── Counter helpers ───────────────────────────────────────────────────────────

fn load_market_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::MarketCount)
        .unwrap_or(0u64)
}

fn next_market_id(env: &Env) -> Result<u64, InsightArenaError> {
    let count = load_market_count(env);
    let next = count.checked_add(1).ok_or(InsightArenaError::Overflow)?;
    env.storage().persistent().set(&DataKey::MarketCount, &next);
    bump_counter(env);
    Ok(next)
}

// ── Event emission ────────────────────────────────────────────────────────────

fn emit_market_created(env: &Env, market_id: u64, creator: &Address, end_time: u64) {
    env.events().publish(
        (symbol_short!("mkt"), symbol_short!("created")),
        (market_id, creator.clone(), end_time),
    );
}

// ── Entry-point logic ─────────────────────────────────────────────────────────

/// Create a new prediction market and return its auto-assigned `market_id`.
///
/// Validation order:
/// 1. Platform not paused
/// 2. Creator authorisation via `require_auth()`
/// 3. `end_time` must be strictly after the current ledger timestamp
/// 4. `resolution_time` must be >= `end_time`
/// 5. At least two distinct outcomes required
/// 6. `creator_fee_bps` must not exceed the platform cap
/// 7. `min_stake` >= platform minimum; `max_stake` >= `min_stake`
pub fn create_market(
    env: &Env,
    creator: Address,
    params: CreateMarketParams,
) -> Result<u64, InsightArenaError> {
    // ── Guard 1: platform not paused ─────────────────────────────────────────
    config::ensure_not_paused(env)?;

    // ── Guard 2: creator authorisation ───────────────────────────────────────
    creator.require_auth();

    // ── Guard 3: end_time must be in the future ───────────────────────────────
    let now = env.ledger().timestamp();
    if params.end_time <= now {
        return Err(InsightArenaError::InvalidTimeRange);
    }

    // ── Guard 4: resolution_time must be at or after end_time ────────────────
    if params.resolution_time < params.end_time {
        return Err(InsightArenaError::InvalidTimeRange);
    }

    // ── Guard 5: at least two outcomes required ───────────────────────────────
    if params.outcomes.len() < 2 {
        return Err(InsightArenaError::InvalidInput);
    }

    // ── Load config for fee and stake floor checks ────────────────────────────
    let cfg = config::get_config(env)?;

    // ── Guard 6: creator fee must not exceed the platform cap ─────────────────
    if params.creator_fee_bps > cfg.max_creator_fee_bps {
        return Err(InsightArenaError::InvalidFee);
    }

    // ── Guard 7: stake bounds ─────────────────────────────────────────────────
    if params.min_stake < cfg.min_stake_xlm {
        return Err(InsightArenaError::StakeTooLow);
    }
    if params.max_stake < params.min_stake {
        return Err(InsightArenaError::InvalidInput);
    }

    // ── Atomically assign a new market ID ────────────────────────────────────
    let market_id = next_market_id(env)?;

    // ── Construct and persist the market ─────────────────────────────────────
    let market = Market::new(
        market_id,
        creator.clone(),
        params.title,
        params.description,
        params.category,
        params.outcomes,
        now, // start_time = creation ledger timestamp
        params.end_time,
        params.resolution_time,
        params.is_public,
        params.creator_fee_bps,
        params.min_stake,
        params.max_stake,
    );

    env.storage()
        .persistent()
        .set(&DataKey::Market(market_id), &market);
    bump_market(env, market_id);

    // ── Emit MarketCreated event ──────────────────────────────────────────────
    emit_market_created(env, market_id, &creator, params.end_time);

    Ok(market_id)
}

/// Load a single market by ID. Returns `MarketNotFound` if absent.
pub fn get_market(env: &Env, market_id: u64) -> Result<Market, InsightArenaError> {
    let market = env
        .storage()
        .persistent()
        .get(&DataKey::Market(market_id))
        .ok_or(InsightArenaError::MarketNotFound)?;
    bump_market(env, market_id);
    Ok(market)
}

/// Return the total number of markets ever created (0 before any are made).
/// Extends the counter TTL on every call.
pub fn get_market_count(env: &Env) -> u64 {
    let count = load_market_count(env);
    // Only bump when the key actually exists — extend_ttl panics on missing keys.
    if env.storage().persistent().has(&DataKey::MarketCount) {
        bump_counter(env);
    }
    count
}

/// Return a paginated slice of markets in creation order.
///
/// - `start` is the 1-based market ID to begin from (inclusive).
/// - `limit` is capped at 50 to bound simulation cost.
/// - Markets that have been deleted from storage are silently skipped.
/// - Returns an empty `Vec` when `start` exceeds the current market count.
pub fn list_markets(env: &Env, start: u64, limit: u32) -> Vec<Market> {
    const MAX_LIMIT: u32 = 50;
    let effective_limit = if limit > MAX_LIMIT { MAX_LIMIT } else { limit };

    let total = get_market_count(env);
    let mut result: Vec<Market> = Vec::new(env);

    if start == 0 || start > total || effective_limit == 0 {
        return result;
    }

    let mut collected: u32 = 0;
    let mut id = start;

    while id <= total && collected < effective_limit {
        if let Some(market) = env
            .storage()
            .persistent()
            .get::<DataKey, Market>(&DataKey::Market(id))
        {
            bump_market(env, id);
            result.push_back(market);
            collected += 1;
        }
        id += 1;
    }

    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod market_tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{symbol_short, vec, Address, Env, String};

    use crate::{InsightArenaContract, InsightArenaContractClient, InsightArenaError};

    use super::CreateMarketParams;

    fn deploy(env: &Env) -> InsightArenaContractClient<'_> {
        let id = env.register(InsightArenaContract, ());
        let client = InsightArenaContractClient::new(env, &id);
        let admin = Address::generate(env);
        let oracle = Address::generate(env);
        env.mock_all_auths();
        client.initialize(&admin, &oracle, &200_u32);
        client
    }

    fn default_params(env: &Env) -> CreateMarketParams {
        let now = env.ledger().timestamp();
        CreateMarketParams {
            title: String::from_str(env, "Will it rain?"),
            description: String::from_str(env, "Daily weather market"),
            category: symbol_short!("weather"),
            outcomes: vec![env, symbol_short!("yes"), symbol_short!("no")],
            end_time: now + 1000,
            resolution_time: now + 2000,
            creator_fee_bps: 100,
            min_stake: 10_000_000,
            max_stake: 100_000_000,
            is_public: true,
        }
    }

    #[test]
    fn create_market_success_returns_incremented_id() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let id = client.create_market(&creator, &default_params(&env));
        assert_eq!(id, 1);

        let id2 = client.create_market(&creator, &default_params(&env));
        assert_eq!(id2, 2);
    }

    #[test]
    fn create_market_fails_end_time_in_past() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let mut p = default_params(&env);
        p.end_time = env.ledger().timestamp(); // not strictly after now

        let result = client.try_create_market(&creator, &p);
        assert!(matches!(
            result,
            Err(Ok(InsightArenaError::InvalidTimeRange))
        ));
    }

    #[test]
    fn create_market_fails_resolution_before_end() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let mut p = default_params(&env);
        p.resolution_time = p.end_time - 1;

        let result = client.try_create_market(&creator, &p);
        assert!(matches!(
            result,
            Err(Ok(InsightArenaError::InvalidTimeRange))
        ));
    }

    #[test]
    fn create_market_fails_single_outcome() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let mut p = default_params(&env);
        p.outcomes = vec![&env, symbol_short!("yes")];

        let result = client.try_create_market(&creator, &p);
        assert!(matches!(result, Err(Ok(InsightArenaError::InvalidInput))));
    }

    #[test]
    fn create_market_fails_fee_too_high() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let mut p = default_params(&env);
        p.creator_fee_bps = 501; // exceeds 500 bps cap

        let result = client.try_create_market(&creator, &p);
        assert!(matches!(result, Err(Ok(InsightArenaError::InvalidFee))));
    }

    #[test]
    fn create_market_fails_when_paused() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        client.set_paused(&true);
        let result = client.try_create_market(&creator, &default_params(&env));
        assert!(matches!(result, Err(Ok(InsightArenaError::Paused))));
    }

    #[test]
    fn create_market_fails_stake_too_low() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let mut p = default_params(&env);
        p.min_stake = 1; // below 10_000_000 stroops platform floor

        let result = client.try_create_market(&creator, &p);
        assert!(matches!(result, Err(Ok(InsightArenaError::StakeTooLow))));
    }

    // ── get_market ────────────────────────────────────────────────────────────

    #[test]
    fn get_market_returns_correct_market() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        let id = client.create_market(&creator, &default_params(&env));
        let market = client.get_market(&id);
        assert_eq!(market.market_id, id);
        assert_eq!(market.creator, creator);
    }

    #[test]
    fn get_market_returns_not_found_for_missing_id() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);

        let result = client.try_get_market(&99_u64);
        assert!(matches!(result, Err(Ok(InsightArenaError::MarketNotFound))));
    }

    // ── get_market_count ──────────────────────────────────────────────────────

    #[test]
    fn get_market_count_zero_before_any_market() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);

        assert_eq!(client.get_market_count(), 0);
    }

    #[test]
    fn get_market_count_increments_with_each_market() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        client.create_market(&creator, &default_params(&env));
        assert_eq!(client.get_market_count(), 1);

        client.create_market(&creator, &default_params(&env));
        assert_eq!(client.get_market_count(), 2);
    }

    // ── list_markets ──────────────────────────────────────────────────────────

    #[test]
    fn list_markets_empty_when_no_markets() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);

        let list = client.list_markets(&1_u64, &10_u32);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn list_markets_returns_all_when_within_limit() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        for _ in 0..3 {
            client.create_market(&creator, &default_params(&env));
        }

        let list = client.list_markets(&1_u64, &10_u32);
        assert_eq!(list.len(), 3);
        assert_eq!(list.get(0).unwrap().market_id, 1);
        assert_eq!(list.get(2).unwrap().market_id, 3);
    }

    #[test]
    fn list_markets_respects_pagination_start() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        for _ in 0..5 {
            client.create_market(&creator, &default_params(&env));
        }

        // Start from market ID 3, take up to 10
        let list = client.list_markets(&3_u64, &10_u32);
        assert_eq!(list.len(), 3); // IDs 3, 4, 5
        assert_eq!(list.get(0).unwrap().market_id, 3);
    }

    #[test]
    fn list_markets_caps_at_max_limit_50() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        for _ in 0..60 {
            client.create_market(&creator, &default_params(&env));
        }

        let list = client.list_markets(&1_u64, &100_u32); // ask for 100, should get 50
        assert_eq!(list.len(), 50);
    }

    #[test]
    fn list_markets_empty_when_start_out_of_bounds() {
        let env = Env::default();
        env.mock_all_auths();
        let client = deploy(&env);
        let creator = Address::generate(&env);

        client.create_market(&creator, &default_params(&env));

        // start > total count → empty
        let list = client.list_markets(&99_u64, &10_u32);
        assert_eq!(list.len(), 0);
    }
}
