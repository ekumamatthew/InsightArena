#![no_std]

pub mod storage_types;
pub use crate::storage_types::DataKey;

use soroban_sdk::{contract, contractimpl};

#[contract]
pub struct InsightArenaContract;

#[contractimpl]
impl InsightArenaContract {
    // Contract modules (market, prediction, user, leaderboard, season, invite)
    // will be implemented here using the canonical DataKey enum.
}
