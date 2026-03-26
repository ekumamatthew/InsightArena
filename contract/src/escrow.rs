use soroban_sdk::{token, Address, Env};

use crate::config::{self, PERSISTENT_BUMP, PERSISTENT_THRESHOLD};
use crate::errors::InsightArenaError;
use crate::storage_types::DataKey;

fn bump_treasury(env: &Env) {
    env.storage().persistent().extend_ttl(
        &DataKey::Treasury,
        PERSISTENT_THRESHOLD,
        PERSISTENT_BUMP,
    );
}

/// Transfer `amount` stroops from `predictor` into the contract's escrow.
///
/// The contract address becomes the custodian of the staked XLM; funds are held
/// until the market is resolved (payout) or cancelled (refund).
///
/// # Errors
/// - `InvalidInput` when `amount <= 0`.
/// - Propagates any error returned by [`config::get_config`].
///
/// Token transfer panics are handled by the Soroban runtime and surface as
/// contract failures.
pub fn lock_stake(env: &Env, from: &Address, amount: i128) -> Result<(), InsightArenaError> {
    if amount <= 0 {
        return Err(InsightArenaError::InvalidInput);
    }

    from.require_auth();

    let cfg = config::get_config(env)?;
    token::Client::new(env, &cfg.xlm_token).transfer(
        from,
        &env.current_contract_address(),
        &amount,
    );
    Ok(())
}

/// Transfer `amount` stroops from the contract's own escrow balance to `recipient`.
///
/// The contract address is the implicit custodian of all staked XLM; when a
/// market is cancelled every predictor's stake is returned here.
///
/// # Errors
/// Propagates any error returned by [`config::get_config`].  Token transfer
/// panics are handled by the Soroban runtime and surface as contract failures.
pub fn refund(env: &Env, recipient: &Address, amount: i128) -> Result<(), InsightArenaError> {
    let cfg = config::get_config(env)?;
    token::Client::new(env, &cfg.xlm_token).transfer(
        &env.current_contract_address(),
        recipient,
        &amount,
    );
    Ok(())
}

/// Release a winner payout from contract escrow to `predictor`.
///
/// This is semantically distinct from `refund` (used for market cancellation),
/// but uses the same escrow transfer path from contract balance to recipient.
pub fn release_payout(env: &Env, to: &Address, amount: i128) -> Result<(), InsightArenaError> {
    if amount <= 0 {
        return Err(InsightArenaError::InvalidInput);
    }

    let cfg = config::get_config(env)?;
    let client = token::Client::new(env, &cfg.xlm_token);
    let contract = env.current_contract_address();

    if client.balance(&contract) < amount {
        return Err(InsightArenaError::EscrowEmpty);
    }

    client.transfer(&contract, to, &amount);
    Ok(())
}

pub(crate) fn add_to_treasury_balance(env: &Env, amount: i128) {
    if amount <= 0 {
        return;
    }

    let current_balance: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Treasury)
        .unwrap_or(0);

    let next_balance = current_balance
        .checked_add(amount)
        .expect("treasury balance overflow");

    env.storage().persistent().set(&DataKey::Treasury, &next_balance);
    bump_treasury(env);
}

#[cfg(test)]
mod escrow_tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
    use soroban_sdk::{Address, Env};

    use crate::storage_types::DataKey;
    use crate::{InsightArenaContract, InsightArenaContractClient, InsightArenaError};

    use super::{add_to_treasury_balance, lock_stake, release_payout};

    fn register_token(env: &Env) -> Address {
        let token_admin = Address::generate(env);
        env.register_stellar_asset_contract_v2(token_admin)
            .address()
    }

    fn deploy<'a>(env: &'a Env, xlm_token: &Address) -> InsightArenaContractClient<'a> {
        let id = env.register(InsightArenaContract, ());
        let client = InsightArenaContractClient::new(env, &id);
        let admin = Address::generate(env);
        let oracle = Address::generate(env);
        client.initialize(&admin, &oracle, &200_u32, xlm_token);
        client
    }

    fn fund(env: &Env, xlm_token: &Address, recipient: &Address, amount: i128) {
        StellarAssetClient::new(env, xlm_token).mint(recipient, &amount);
    }

    fn treasury_balance(env: &Env, contract_id: &Address) -> i128 {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .get(&DataKey::Treasury)
                .unwrap_or(0_i128)
        })
    }

    #[test]
    fn test_lock_stake_happy_path() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let predictor = Address::generate(&env);
        let amount = 20_000_000_i128;

        fund(&env, &xlm_token, &predictor, amount);

        let token = TokenClient::new(&env, &xlm_token);
        assert_eq!(token.balance(&predictor), amount);
        assert_eq!(token.balance(&client.address), 0);

        let result = env.as_contract(&client.address, || lock_stake(&env, &predictor, amount));
        assert_eq!(result, Ok(()));

        assert_eq!(token.balance(&predictor), 0);
        assert_eq!(token.balance(&client.address), amount);
    }

    #[test]
    fn test_lock_stake_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let predictor = Address::generate(&env);

        let result = env.as_contract(&client.address, || lock_stake(&env, &predictor, 0));
        assert_eq!(result, Err(InsightArenaError::InvalidInput));
    }

    #[test]
    #[should_panic]
    fn test_lock_stake_unauthorized() {
        let env = Env::default();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let predictor = Address::generate(&env);
        let amount = 10_000_000_i128;

        fund(&env, &xlm_token, &predictor, amount);

        env.as_contract(&client.address, || {
            let _ = lock_stake(&env, &predictor, amount);
        });
    }

    #[test]
    #[should_panic]
    fn test_lock_stake_insufficient_user_funds() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let predictor = Address::generate(&env);

        env.as_contract(&client.address, || {
            let _ = lock_stake(&env, &predictor, 10_000_000_i128);
        });
    }

    #[test]
    fn test_release_payout_success() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let recipient = Address::generate(&env);
        let payout = 20_000_000_i128;

        fund(&env, &xlm_token, &client.address, payout);

        let token = TokenClient::new(&env, &xlm_token);
        assert_eq!(token.balance(&client.address), payout);
        assert_eq!(token.balance(&recipient), 0);

        let result = env.as_contract(&client.address, || release_payout(&env, &recipient, payout));
        assert_eq!(result, Ok(()));

        assert_eq!(token.balance(&client.address), 0);
        assert_eq!(token.balance(&recipient), payout);
    }

    #[test]
    fn test_release_payout_contract_insolvent() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let recipient = Address::generate(&env);

        let result = env.as_contract(&client.address, || {
            release_payout(&env, &recipient, 10_000_000_i128)
        });
        assert_eq!(result, Err(InsightArenaError::EscrowEmpty));
    }

    #[test]
    fn test_release_payout_zero_value() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);
        let recipient = Address::generate(&env);

        let result = env.as_contract(&client.address, || release_payout(&env, &recipient, 0));
        assert_eq!(result, Err(InsightArenaError::InvalidInput));
    }

    #[test]
    fn test_add_treasury_starts_at_zero() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);

        assert_eq!(treasury_balance(&env, &client.address), 0);

        env.as_contract(&client.address, || add_to_treasury_balance(&env, 1_000_000));

        assert_eq!(treasury_balance(&env, &client.address), 1_000_000);
    }

    #[test]
    fn test_add_treasury_multiple_increments() {
        let env = Env::default();
        env.mock_all_auths();
        let xlm_token = register_token(&env);
        let client = deploy(&env, &xlm_token);

        env.as_contract(&client.address, || {
            add_to_treasury_balance(&env, 1_000_000);
            add_to_treasury_balance(&env, 2_500_000);
            add_to_treasury_balance(&env, 500_000);
        });

        assert_eq!(treasury_balance(&env, &client.address), 4_000_000);
    }
}
