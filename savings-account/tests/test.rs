mod savings_account_interactions;
mod savings_account_setup;

use elrond_wasm_debug::{managed_biguint, rust_biguint, DebugApi};
use savings_account::common_storage::CommonStorageModule;
use savings_account::model::LendMetadata;
use savings_account::staking_positions_mapper::StakingPosition;
use savings_account::staking_rewards::StakingRewardsModule;
use savings_account_setup::*;

#[test]
fn init_test() {
    let _ = SavingsAccountSetup::new(savings_account::contract_obj);
}

#[test]
fn lend_test() {
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    sa_setup.default_lenders();
}

#[test]
fn borrow_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);

    sa_setup.default_lenders();
    sa_setup.default_borrows();
}

#[test]
fn calculate_rewards_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);

    sa_setup.default_lenders();
    sa_setup.default_borrows();

    // check balance before
    sa_setup.b_mock.check_esdt_balance(
        sa_setup.sa_wrapper.address_ref(),
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(75_000),
    );
    let liq_staking_token_balance = rust_biguint!(STAKE_PER_POSITION) * DECIMALS;
    for i in 1..4u64 {
        sa_setup.b_mock.check_nft_balance(
            sa_setup.sa_wrapper.address_ref(),
            LIQUID_STAKING_TOKEN_ID,
            i,
            &liq_staking_token_balance,
            Some(&elrond_wasm::elrond_codec::Empty),
        );
    }

    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();

    // check balance after, received 10K stablecoins and new LIQ staking tokens
    sa_setup.b_mock.check_esdt_balance(
        sa_setup.sa_wrapper.address_ref(),
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(85_000),
    );
    for i in 5..8u64 {
        sa_setup.b_mock.check_nft_balance(
            sa_setup.sa_wrapper.address_ref(),
            LIQUID_STAKING_TOKEN_ID,
            i,
            &liq_staking_token_balance,
            Some(&elrond_wasm::elrond_codec::Empty),
        );
    }

    // check staking positions consistency - liquid_staking_nonce changed
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            // check staking positions mapper
            let mapper = sc.staking_positions();
            assert_eq!(
                mapper.get_staking_position(1),
                StakingPosition {
                    liquid_staking_nonce: 5,
                    prev_pos_id: 0,
                    next_pos_id: 2,
                }
            );
            assert_eq!(
                mapper.get_staking_position(2),
                StakingPosition {
                    liquid_staking_nonce: 6,
                    prev_pos_id: 1,
                    next_pos_id: 3,
                }
            );
            assert_eq!(
                mapper.get_staking_position(3),
                StakingPosition {
                    liquid_staking_nonce: 7,
                    prev_pos_id: 2,
                    next_pos_id: 4,
                }
            );
            assert_eq!(
                mapper.get_staking_position(4),
                StakingPosition {
                    liquid_staking_nonce: 8,
                    prev_pos_id: 3,
                    next_pos_id: 0,
                }
            );
        })
        .assert_ok();
}

#[test]
fn claim_rewards_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let first_lender = sa_setup.first_lender_address.clone();
    let second_lender = sa_setup.second_lender_address.clone();

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();

    sa_setup.b_mock.set_block_epoch(50);

    // 100,000 out of total 150,000 => ~66% of 12,250
    assert_eq!(sa_setup.call_get_penaly_amount(100_000), 8_166);

    // 50,000 out of total 150,000 => ~33% of 12,250
    assert_eq!(sa_setup.call_get_penaly_amount(50_000), 4_083);

    // (50 - 20) * 0.5% * 100,000 - 6,125 = 15,000 - 8,166 = 6,834
    let first_lender_rewards = 6_834;
    assert_eq!(
        sa_setup.call_get_lender_claimable_rewards(20, 100_000),
        first_lender_rewards
    );

    // (50 - 21) * 0.5% * 50,000 - 4,083 = 7,250 - 4,083 = 3,167
    let second_lender_rewards = 3_167;
    assert_eq!(
        sa_setup.call_get_lender_claimable_rewards(21, 50_000),
        second_lender_rewards
    );

    // lender 1 try claim without penalty
    sa_setup
        .call_lender_claim_rewards(&first_lender, 1, 100_000, 3, first_lender_rewards, true)
        .assert_user_error("Rewards have penalty");

    // lender 1 claim ok
    sa_setup
        .call_lender_claim_rewards(&first_lender, 1, 100_000, 3, first_lender_rewards, false)
        .assert_ok();

    sa_setup.b_mock.check_nft_balance(
        &first_lender,
        LEND_TOKEN_ID,
        3,
        &rust_biguint!(100_000),
        Some(&LendMetadata { lend_epoch: 50 }),
    );
    sa_setup.b_mock.check_esdt_balance(
        &first_lender,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(first_lender_rewards),
    );

    // lender 2 claim ok
    sa_setup
        .call_lender_claim_rewards(&second_lender, 2, 50_000, 3, second_lender_rewards, false)
        .assert_ok();

    // lender 1 and 2 now have same lend token nonce (because of same lend_epoch)
    // also, lender 2 initially has 50_000 stablecoins, as they only lent half
    sa_setup.b_mock.check_nft_balance(
        &second_lender,
        LEND_TOKEN_ID,
        3,
        &rust_biguint!(50_000),
        Some(&LendMetadata { lend_epoch: 50 }),
    );
    sa_setup.b_mock.check_esdt_balance(
        &second_lender,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(50_000 + second_lender_rewards),
    );

    // check Savings Account internal state
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            assert_eq!(sc.last_rewards_update_epoch().get(), 50);
            assert_eq!(sc.stablecoin_reserves().get(), managed_biguint!(0));
            assert_eq!(
                sc.total_missed_rewards_by_claim_since_last_calculation()
                    .get(),
                managed_biguint!(8_166 + 4_083)
            );
        })
        .assert_ok();
}
