use soroban_sdk::{contracttype, Address, Symbol};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Keyed by market_id. Represents a prediction market instance.
    Market(u64),
    /// Keyed by (market_id, predictor). Represents a user's prediction in a given market.
    Prediction(u64, Address),
    /// Keyed by user address. Represents an individual user's profile or state.
    User(Address),
    /// Keyed by season_id. Stores the leaderboard rankings per season.
    Leaderboard(u64),
    /// Keyed by season number. Represents a season's metadata and schedule.
    Season(u32),
    /// Keyed by code symbol. Maps an invite code to its underlying metadata.
    InviteCode(Symbol),
    /// Singleton. Holds global configuration for the platform.
    Config,
    /// Global counter. Tracks the total number of markets created.
    MarketCount,
    /// Global counter. Tracks the total number of seasons.
    SeasonCount,
    /// Emergency pause flag. Used to halt sensitive operations across the platform.
    Paused,
}
