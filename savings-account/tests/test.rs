mod savings_account_interactions;
mod savings_account_setup;

use elrond_wasm_debug::{managed_biguint, rust_biguint, DebugApi};
use savings_account::common_storage::CommonStorageModule;
use savings_account::model::BorrowMetadata;
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

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
    sa_setup.default_claim_rewards();
}

#[test]
fn withdraw_before_claim_rewards_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let first_lender = sa_setup.first_lender_address.clone();
    let second_lender = sa_setup.second_lender_address.clone();

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();

    sa_setup.b_mock.set_block_epoch(50);

    // withdraw the initial 50,000 lent + 3,167 as rewards (calculate in previous test)
    sa_setup
        .call_withdraw(&second_lender, 2, 50_000, 53_167)
        .assert_ok();

    sa_setup.b_mock.check_esdt_balance(
        &second_lender,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(50_000 + 53_167),
    );

    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            assert_eq!(sc.lent_amount().get(), managed_biguint!(100_000));
            assert_eq!(sc.borrowed_amount().get(), managed_biguint!(75_000));
        })
        .assert_ok();

    // first lender try withdraw, not enough lent_amount left
    sa_setup
        .call_withdraw(&first_lender, 1, 100_000, 0)
        .assert_user_error("Cannot withdraw, not enough funds");
}

#[test]
fn withdraw_after_claim_rewards_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let second_lender = sa_setup.second_lender_address.clone();

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();

    sa_setup.b_mock.set_block_epoch(50);

    sa_setup
        .call_lender_claim_rewards(&second_lender, 2, 50_000, 3, 3_167, false)
        .assert_ok();
    sa_setup
        .call_withdraw(&second_lender, 3, 50_000, 50_000)
        .assert_ok();

    sa_setup.b_mock.check_esdt_balance(
        &second_lender,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(50_000 + 53_167),
    );
}

#[test]
fn withdraw_partial_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let second_lender = sa_setup.second_lender_address.clone();

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();

    sa_setup.b_mock.set_block_epoch(50);

    // intial 25_000 + ~(3_167 / 2)
    sa_setup
        .call_withdraw(&second_lender, 2, 25_000, 26_584)
        .assert_ok();

    // since there is less total lent amount now, the penalty amount per lend token increases,
    // so less is withdrawn
    sa_setup
        .call_withdraw(&second_lender, 2, 25_000, 26_175)
        .assert_ok();
}

#[test]
fn repay_full_first_pos_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let borrower = sa_setup.borrower_address.clone();
    let borrow_token_amount = rust_biguint!(STAKE_PER_POSITION) * DECIMALS;

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
    sa_setup.default_claim_rewards();

    // one year after borrow
    sa_setup.b_mock.set_block_epoch(390);

    sa_setup
        .b_mock
        .check_esdt_balance(&borrower, STABLECOIN_TOKEN_ID, &rust_biguint!(75_000));

    // repay - too few stablecoins
    sa_setup
        .call_repay(&borrower, 1, &borrow_token_amount, 10_000, 5, 0)
        .assert_user_error("Not enough stablecoins paid to cover the debt");

    // repay - ok
    // borrow rate = 50% + 2/3 * 10% = 56,66%, which leads to 25,000 * 56,66% = 14,165 debt,
    // so ~39,165 as total amount needed
    sa_setup
        .call_repay(&borrower, 1, &borrow_token_amount, 75_000, 5, 35_834)
        .assert_ok();

    sa_setup.b_mock.check_nft_balance(
        &borrower,
        LIQUID_STAKING_TOKEN_ID,
        5,
        &borrow_token_amount,
        Some(&elrond_wasm::elrond_codec::Empty),
    );
    sa_setup
        .b_mock
        .check_esdt_balance(&borrower, STABLECOIN_TOKEN_ID, &rust_biguint!(35_834));

    // check staking positions list consistency
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            // check staking positions mapper
            let mapper = sc.staking_positions();
            assert_eq!(
                mapper.get_staking_position(2),
                StakingPosition {
                    liquid_staking_nonce: 6,
                    prev_pos_id: 0,
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

    // position with ID 1 is now invalid
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            let _ = sc.staking_positions().get_staking_position(1);
        })
        .assert_user_error("Invalid staking position ID");
}

#[test]
fn repay_other_pos_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let borrower = sa_setup.borrower_address.clone();
    let borrow_token_amount = rust_biguint!(STAKE_PER_POSITION) * DECIMALS;

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
    sa_setup.default_claim_rewards();

    // one year after borrow
    sa_setup.b_mock.set_block_epoch(390);

    sa_setup
        .call_repay(&borrower, 3, &borrow_token_amount, 75_000, 7, 35_834)
        .assert_ok();

    // check staking positions list consistency
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
                    next_pos_id: 4,
                }
            );
            assert_eq!(
                mapper.get_staking_position(4),
                StakingPosition {
                    liquid_staking_nonce: 8,
                    prev_pos_id: 2,
                    next_pos_id: 0,
                }
            );
        })
        .assert_ok();

    // position with ID 3 is now invalid
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            let _ = sc.staking_positions().get_staking_position(3);
        })
        .assert_user_error("Invalid staking position ID");
}

#[test]
fn repay_last_pos_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let borrower = sa_setup.borrower_address.clone();
    let borrow_token_amount = rust_biguint!(STAKE_PER_POSITION) * DECIMALS;

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
    sa_setup.default_claim_rewards();

    // one year after borrow
    sa_setup.b_mock.set_block_epoch(390);

    sa_setup
        .call_repay(&borrower, 4, &borrow_token_amount, 75_000, 8, 35_834)
        .assert_ok();

    // check staking positions list consistency
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
                    next_pos_id: 0,
                }
            );
        })
        .assert_ok();

    // position with ID 4 is now invalid
    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            let _ = sc.staking_positions().get_staking_position(4);
        })
        .assert_user_error("Invalid staking position ID");
}

#[test]
fn repay_partial_test() {
    let _ = DebugApi::dummy();
    let mut sa_setup = SavingsAccountSetup::new(savings_account::contract_obj);
    let borrower = sa_setup.borrower_address.clone();
    let borrow_token_amount = rust_biguint!(150) * DECIMALS;

    sa_setup.default_lenders();
    sa_setup.default_borrows();
    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
    sa_setup.default_claim_rewards();

    // one year after borrow
    sa_setup.b_mock.set_block_epoch(390);

    // repay 150 out of the total 250
    sa_setup
        .call_repay(&borrower, 1, &borrow_token_amount, 75_000, 5, 51_501)
        .assert_ok();

    sa_setup.b_mock.check_nft_balance(
        &borrower,
        BORROW_TOKEN_ID,
        1,
        &(rust_biguint!(100) * DECIMALS),
        Some(&BorrowMetadata::<DebugApi> {
            borrow_epoch: 25,
            staking_position_id: 1,
            staked_token_value_in_dollars_at_borrow: managed_biguint!(100),
        }),
    );
    sa_setup.b_mock.check_nft_balance(
        &borrower,
        LIQUID_STAKING_TOKEN_ID,
        5,
        &(rust_biguint!(150) * DECIMALS),
        Option::<&elrond_wasm::elrond_codec::Empty>::None,
    );
    sa_setup
        .b_mock
        .check_esdt_balance(&borrower, STABLECOIN_TOKEN_ID, &rust_biguint!(51_501));

    sa_setup
        .b_mock
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            assert_eq!(sc.lent_amount().get(), managed_biguint!(150_000));
            assert_eq!(sc.borrowed_amount().get(), managed_biguint!(60_000));
        })
        .assert_ok();
}
