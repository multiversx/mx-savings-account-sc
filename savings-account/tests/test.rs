use elrond_wasm::elrond_codec::multi_types::OptionalValue;
use elrond_wasm::storage::mappers::StorageTokenWrapper;
use elrond_wasm::types::{Address, EsdtLocalRole, ManagedBuffer, OperationCompletionStatus};
use elrond_wasm_debug::{
    managed_address, managed_biguint, managed_token_id, rust_biguint, testing_framework::*,
    DebugApi,
};
use savings_account::staking_rewards::StakingRewardsModule;
use savings_account::tokens::TokensModule;
use savings_account::*;

const DUMMY_WASM_PATH: &'static str = "";
const STABLECOIN_TOKEN_ID: &[u8] = b"STABLE-123456";
const LIQUID_STAKING_TOKEN_ID: &[u8] = b"LIQ-123456";
const STAKED_TOKEN_ID: &[u8] = b"";
const STAKED_TOKEN_TICKER: &[u8] = b"EGLD";
const LOAN_TO_VALUE_PERCENTAGE: u64 = 750_000_000; // 75%
const LENDER_REWARDS_PERCENTAGE_PER_EPOCH: u64 = 5_000_000; // 0.5%
const BASE_BORROW_RATE: u64 = 500_000_000; // 50%
const BORROW_RATE_UNDER_OPTIMAL_FACTOR: u64 = 100_000_000; // 10%
const BORROW_RATE_OVER_OPTIMAL_FACTOR: u64 = 100_000_000; // 10%
const OPTIMAL_UTILISATION: u64 = 750_000_000; // 75%

const LEND_TOKEN_ID: &[u8] = b"LEND-123456";
const BORROW_TOKEN_ID: &[u8] = b"BORROW-123456";

struct SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    pub blockchain_wrapper: BlockchainStateWrapper,
    pub owner_address: Address,
    pub first_lender_address: Address,
    pub second_lender_address: Address,
    pub _first_borrower_address: Address,
    pub _second_borrower_address: Address,
    pub sa_wrapper:
        ContractObjWrapper<savings_account::ContractObj<DebugApi>, SavingsAccountObjBuilder>,
}

fn setup_savings_account<SavingsAccountObjBuilder>(
    sa_builder: SavingsAccountObjBuilder,
) -> SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    let rust_zero = rust_biguint!(0u64);
    let mut blockchain_wrapper = BlockchainStateWrapper::new();
    let owner_address = blockchain_wrapper.create_user_account(&rust_zero);
    let first_lender_address = blockchain_wrapper.create_user_account(&rust_zero);
    let second_lender_address = blockchain_wrapper.create_user_account(&rust_zero);
    let first_borrower_address = blockchain_wrapper.create_user_account(&rust_zero);
    let second_borrower_address = blockchain_wrapper.create_user_account(&rust_zero);

    blockchain_wrapper.set_block_epoch(10);

    // they use the SavingsAccount SC builder, as we only really need their addresses
    // Async calls don't work yet, so we can't use the other two contracts
    let delegation_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        sa_builder,
        DUMMY_WASM_PATH,
    );
    let dex_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        sa_builder,
        DUMMY_WASM_PATH,
    );

    let price_aggregator_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        price_aggregator::contract_obj,
        DUMMY_WASM_PATH,
    );
    let sa_wrapper = blockchain_wrapper.create_sc_account(
        &rust_zero,
        Some(&owner_address),
        sa_builder,
        DUMMY_WASM_PATH,
    );

    blockchain_wrapper.set_esdt_balance(
        &first_lender_address,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(100_000),
    );
    blockchain_wrapper.set_esdt_balance(
        &second_lender_address,
        STABLECOIN_TOKEN_ID,
        &rust_biguint!(100_000),
    );

    let nft_balance = rust_biguint!(250) * rust_biguint!(1_000_000_000_000_000_000);
    blockchain_wrapper.set_nft_balance(
        &first_borrower_address,
        LIQUID_STAKING_TOKEN_ID,
        1,
        &nft_balance,
        &(),
    );
    blockchain_wrapper.set_nft_balance(
        &second_borrower_address,
        LIQUID_STAKING_TOKEN_ID,
        2,
        &nft_balance,
        &(),
    );

    blockchain_wrapper
        .execute_tx(&owner_address, &sa_wrapper, &rust_zero, |sc| {
            sc.init(
                managed_token_id!(STABLECOIN_TOKEN_ID),
                managed_token_id!(LIQUID_STAKING_TOKEN_ID),
                managed_token_id!(STAKED_TOKEN_ID),
                ManagedBuffer::new_from_bytes(STAKED_TOKEN_TICKER),
                managed_address!(delegation_wrapper.address_ref()),
                managed_address!(dex_wrapper.address_ref()),
                managed_address!(price_aggregator_wrapper.address_ref()),
                managed_biguint!(LOAN_TO_VALUE_PERCENTAGE),
                managed_biguint!(LENDER_REWARDS_PERCENTAGE_PER_EPOCH),
                managed_biguint!(BASE_BORROW_RATE),
                managed_biguint!(BORROW_RATE_UNDER_OPTIMAL_FACTOR),
                managed_biguint!(BORROW_RATE_OVER_OPTIMAL_FACTOR),
                managed_biguint!(OPTIMAL_UTILISATION),
            );

            sc.lend_token()
                .set_token_id(&managed_token_id!(LEND_TOKEN_ID));
            sc.borrow_token()
                .set_token_id(&managed_token_id!(BORROW_TOKEN_ID));
        })
        .assert_ok();

    let roles = [
        EsdtLocalRole::NftCreate,
        EsdtLocalRole::NftAddQuantity,
        EsdtLocalRole::NftBurn,
    ];
    blockchain_wrapper.set_esdt_local_roles(sa_wrapper.address_ref(), LEND_TOKEN_ID, &roles[..]);
    blockchain_wrapper.set_esdt_local_roles(sa_wrapper.address_ref(), BORROW_TOKEN_ID, &roles[..]);

    SavingsAccountSetup {
        blockchain_wrapper,
        owner_address,
        first_lender_address,
        second_lender_address,
        _first_borrower_address: first_borrower_address,
        _second_borrower_address: second_borrower_address,
        sa_wrapper,
    }
}

#[test]
fn init_test() {
    let _ = setup_savings_account(savings_account::contract_obj);
}

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
