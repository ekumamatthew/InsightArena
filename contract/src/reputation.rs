use soroban_sdk::{Address, Env};

use crate::config::{PERSISTENT_BUMP, PERSISTENT_THRESHOLD};
use crate::errors::InsightArenaError;
use crate::storage_types::{CreatorStats, DataKey};

// ── Storage helpers ───────────────────────────────────────────────────────────

fn load_stats(env: &Env, creator: &Address) -> CreatorStats {
    env.storage()
        .persistent()
        .get(&DataKey::CreatorStats(creator.clone()))
        .unwrap_or(CreatorStats {
            markets_created: 0,
            markets_resolved: 0,
            average_participant_count: 0,
            dispute_count: 0,
            reputation_score: 0,
        })
}

fn save_stats(env: &Env, creator: &Address, stats: &CreatorStats) {
    let key = DataKey::CreatorStats(creator.clone());
    env.storage().persistent().set(&key, stats);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_BUMP);
}

// ── Pure reputation formula ───────────────────────────────────────────────────

/// Compute reputation score from `CreatorStats`. No storage access.
///
/// Formula (integer arithmetic, overflow-safe):
///   score  = (markets_resolved / max(markets_created, 1)) * 600
///           + min(average_participant_count * 2, 200)
///           - min(dispute_count * 50, 200)
///   score  = clamp(score, 0, 1000)
pub fn calculate_creator_reputation(stats: &CreatorStats) -> u32 {
    let denominator = stats.markets_created.max(1) as u64;
    let resolution_ratio_600 = ((stats.markets_resolved as u64 * 600) / denominator) as u32;

    let participation_bonus = (stats.average_participant_count.saturating_mul(2)).min(200);

    let dispute_penalty = (stats.dispute_count.saturating_mul(50)).min(200);

    let score = resolution_ratio_600
        .saturating_add(participation_bonus)
        .saturating_sub(dispute_penalty);

    score.min(1000)
}

// ── Mutation hooks ────────────────────────────────────────────────────────────

/// Called after a market is successfully created.
pub fn on_market_created(env: &Env, creator: &Address) {
    let mut stats = load_stats(env, creator);
    stats.markets_created = stats.markets_created.saturating_add(1);
    stats.reputation_score = calculate_creator_reputation(&stats);
    save_stats(env, creator, &stats);
}

/// Called after a market is successfully resolved.
/// `participant_count` is the final participant count of the resolved market.
pub fn on_market_resolved(env: &Env, creator: &Address, participant_count: u32) {
    let mut stats = load_stats(env, creator);

    // Rolling average: new_avg = (old_avg * resolved + participant_count) / (resolved + 1)
    let new_resolved = stats.markets_resolved.saturating_add(1);
    let new_avg = ((stats.average_participant_count as u64)
        .saturating_mul(stats.markets_resolved as u64)
        .saturating_add(participant_count as u64))
        / (new_resolved as u64);

    stats.markets_resolved = new_resolved;
    stats.average_participant_count = new_avg as u32;
    stats.reputation_score = calculate_creator_reputation(&stats);
    save_stats(env, creator, &stats);
}

// ── View ──────────────────────────────────────────────────────────────────────

pub fn get_creator_stats(env: Env, creator: Address) -> Result<CreatorStats, InsightArenaError> {
    Ok(load_stats(&env, &creator))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// Tests have been moved to tests/reputation_tests.rs
