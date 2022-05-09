mod savings_account_interactions;
mod savings_account_setup;

use elrond_wasm_debug::DebugApi;
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

    sa_setup.call_claim_staking_rewards().assert_ok();
    sa_setup.call_convert_staking_token().assert_ok();
}

/*
#[test]
fn test_rewards_penalty() {
    let mut sa_setup = setup_savings_account(savings_account::contract_obj);
    let b_wrapper = &mut sa_setup.blockchain_wrapper;

    b_wrapper.set_block_epoch(20);

    b_wrapper
        .execute_esdt_transfer(
            &sa_setup.first_lender_address,
            &sa_setup.sa_wrapper,
            STABLECOIN_TOKEN_ID,
            0,
            &rust_biguint!(100_000),
            |sc| {
                sc.lend();
            },
        )
        .assert_ok();

    b_wrapper
        .execute_esdt_transfer(
            &sa_setup.second_lender_address,
            &sa_setup.sa_wrapper,
            STABLECOIN_TOKEN_ID,
            0,
            &rust_biguint!(100_000),
            |sc| {
                sc.lend();
            },
        )
        .assert_ok();

    b_wrapper.set_block_epoch(25);

    // set state required for calculate rewards to be callable
    b_wrapper
        .execute_tx(
            &sa_setup.owner_address,
            &sa_setup.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                sc.last_staking_rewards_claim_epoch().set(&25);
                sc.last_staking_token_convert_epoch().set(&25);
                sc.stablecoin_reserves().set(&managed_biguint!(500));
            },
        )
        .assert_ok();

    b_wrapper
        .execute_tx(
            &sa_setup.owner_address,
            &sa_setup.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                let op_status = sc.calculate_total_lender_rewards();
                assert_eq!(op_status, OperationCompletionStatus::Completed);
            },
        )
        .assert_ok();

    // expected rewards is 0.5% * 100_000 * (5 epochs) = 2.5% * 100_000 = 2_500 per lender
    // i.e. 5_000 total rewards
    b_wrapper
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            let unclaimed_rewards = sc.unclaimed_rewards().get();
            assert_eq!(unclaimed_rewards, managed_biguint!(500));

            let missing_rewards = sc.missing_rewards().get();
            assert_eq!(missing_rewards, managed_biguint!(4_500));

            let penalty_per_lender = sc.penalty_amount_per_lender().get();
            assert_eq!(penalty_per_lender, managed_biguint!(2_250));
        })
        .assert_ok();

    // lender 1 claim, should claim 2_500 - 2_250 = 250
    b_wrapper
        .execute_esdt_transfer(
            &sa_setup.first_lender_address,
            &sa_setup.sa_wrapper,
            LEND_TOKEN_ID,
            1,
            &rust_biguint!(100_000),
            |sc| {
                sc.lender_claim_rewards(OptionalValue::None);
            },
        )
        .assert_ok();

    b_wrapper.check_esdt_balance(
        &sa_setup.first_lender_address,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(250),
    );

    // assume lender2 waited and claimed rewards later, when penalty was lifted
    b_wrapper.set_block_epoch(26);

    b_wrapper
        .execute_tx(
            &sa_setup.owner_address,
            &sa_setup.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                sc.last_staking_rewards_claim_epoch().set(&26);
                sc.last_staking_token_convert_epoch().set(&26);
                sc.stablecoin_reserves().set(&managed_biguint!(10_000));
            },
        )
        .assert_ok();

    b_wrapper
        .execute_tx(
            &sa_setup.owner_address,
            &sa_setup.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                let op_status = sc.calculate_total_lender_rewards();
                assert_eq!(op_status, OperationCompletionStatus::Completed);
            },
        )
        .assert_ok();

    b_wrapper
        .execute_query(&sa_setup.sa_wrapper, |sc| {
            let unclaimed_rewards = sc.unclaimed_rewards().get();
            assert_eq!(unclaimed_rewards, managed_biguint!(3_500));

            let missing_rewards = sc.missing_rewards().get();
            assert_eq!(missing_rewards, managed_biguint!(0));

            let penalty_per_lender = sc.penalty_amount_per_lender().get();
            assert_eq!(penalty_per_lender, managed_biguint!(0));
        })
        .assert_ok();

    // lender 2 claim, will get the full amount of 3_000
    b_wrapper
        .execute_esdt_transfer(
            &sa_setup.second_lender_address,
            &sa_setup.sa_wrapper,
            LEND_TOKEN_ID,
            2,
            &rust_biguint!(100_000),
            |sc| {
                sc.lender_claim_rewards(OptionalValue::None);
            },
        )
        .assert_ok();

    b_wrapper.check_esdt_balance(
        &sa_setup.second_lender_address,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(3_000),
    );
}
*/
