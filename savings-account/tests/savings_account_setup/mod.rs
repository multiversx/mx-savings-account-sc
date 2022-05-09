use delegation_mock::DelegationMock;
use elrond_wasm::elrond_codec::Empty;
use elrond_wasm::storage::mappers::StorageTokenWrapper;
use elrond_wasm::types::{Address, EsdtLocalRole, ManagedBuffer, ManagedVec, TokenIdentifier};
use elrond_wasm_debug::{
    managed_address, managed_biguint, managed_buffer, managed_token_id, rust_biguint,
    testing_framework::*, DebugApi,
};
use price_aggregator::PriceAggregator;
use savings_account::common_storage::CommonStorageModule;
use savings_account::staking_rewards::StakingRewardsModule;
use savings_account::tokens::TokensModule;
use savings_account::*;

pub static STABLECOIN_TOKEN_ID: &[u8] = b"STABLE-123456";
pub static LIQUID_STAKING_TOKEN_ID: &[u8] = b"LIQ-123456";
pub static STAKED_TOKEN_ID: &[u8] = b"";
pub static STAKED_TOKEN_TICKER: &[u8] = b"EGLD";
pub const LOAN_TO_VALUE_PERCENTAGE: u64 = 750_000_000; // 75%
pub const LENDER_REWARDS_PERCENTAGE_PER_EPOCH: u64 = 5_000_000; // 0.5%
pub const BASE_BORROW_RATE: u64 = 500_000_000; // 50%
pub const BORROW_RATE_UNDER_OPTIMAL_FACTOR: u64 = 100_000_000; // 10%
pub const BORROW_RATE_OVER_OPTIMAL_FACTOR: u64 = 100_000_000; // 10%
pub const OPTIMAL_UTILISATION: u64 = 750_000_000; // 75%

pub static LEND_TOKEN_ID: &[u8] = b"LEND-123456";
pub static BORROW_TOKEN_ID: &[u8] = b"BORROW-123456";
pub static NFT_ROLES: &[EsdtLocalRole] = &[
    EsdtLocalRole::NftCreate,
    EsdtLocalRole::NftAddQuantity,
    EsdtLocalRole::NftBurn,
];

pub const STAKE_PER_POSITION: u64 = 250;
pub const DECIMALS: u64 = 1_000_000_000_000_000_000;
pub const NR_STAKING_POSITIONS: u32 = 4;

pub struct SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    pub b_mock: BlockchainStateWrapper,
    pub owner_address: Address,
    pub first_lender_address: Address,
    pub second_lender_address: Address,
    pub borrower_address: Address,
    pub sa_wrapper:
        ContractObjWrapper<savings_account::ContractObj<DebugApi>, SavingsAccountObjBuilder>,
}

impl<SavingsAccountObjBuilder> SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    pub fn new(sa_builder: SavingsAccountObjBuilder) -> Self {
        let rust_zero = rust_biguint!(0u64);
        let mut b_mock = BlockchainStateWrapper::new();
        let owner_address = b_mock.create_user_account(&rust_zero);
        let first_lender_address = b_mock.create_user_account(&rust_zero);
        let second_lender_address = b_mock.create_user_account(&rust_zero);
        let borrower_address = b_mock.create_user_account(&rust_zero);

        b_mock.set_block_epoch(10);

        let delegation_address =
            Self::init_delegation_mock(&mut b_mock, &owner_address, &borrower_address);
        let dex_address = Self::init_dex_mock(&mut b_mock, &owner_address);
        let price_aggregator_address = Self::init_price_aggregator(&mut b_mock, &owner_address);
        let sa_wrapper = b_mock.create_sc_account(
            &rust_zero,
            Some(&owner_address),
            sa_builder,
            "savings_account.wasm",
        );

        b_mock.set_esdt_balance(
            &first_lender_address,
            STABLECOIN_TOKEN_ID,
            &rust_biguint!(100_000),
        );
        b_mock.set_esdt_balance(
            &second_lender_address,
            STABLECOIN_TOKEN_ID,
            &rust_biguint!(100_000),
        );

        b_mock
            .execute_tx(&owner_address, &sa_wrapper, &rust_zero, |sc| {
                sc.init(
                    managed_token_id!(STABLECOIN_TOKEN_ID),
                    managed_token_id!(LIQUID_STAKING_TOKEN_ID),
                    managed_token_id!(STAKED_TOKEN_ID),
                    ManagedBuffer::new_from_bytes(STAKED_TOKEN_TICKER),
                    managed_address!(&delegation_address),
                    managed_address!(&dex_address),
                    managed_address!(&price_aggregator_address),
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

        b_mock.set_esdt_local_roles(sa_wrapper.address_ref(), LEND_TOKEN_ID, NFT_ROLES);
        b_mock.set_esdt_local_roles(sa_wrapper.address_ref(), BORROW_TOKEN_ID, NFT_ROLES);

        SavingsAccountSetup {
            b_mock,
            owner_address,
            first_lender_address,
            second_lender_address,
            borrower_address,
            sa_wrapper,
        }
    }

    fn init_delegation_mock(
        b_mock: &mut BlockchainStateWrapper,
        owner_address: &Address,
        staker: &Address,
    ) -> Address {
        let rust_zero = rust_biguint!(0);
        let delegation_wrapper = b_mock.create_sc_account(
            &rust_zero,
            Some(owner_address),
            delegation_mock::contract_obj,
            "delegation.wasm",
        );

        b_mock.set_esdt_local_roles(
            delegation_wrapper.address_ref(),
            LIQUID_STAKING_TOKEN_ID,
            NFT_ROLES,
        );

        b_mock
            .execute_tx(owner_address, &delegation_wrapper, &rust_zero, |sc| {
                sc.init(managed_token_id!(LIQUID_STAKING_TOKEN_ID));
            })
            .assert_ok();

        let stake_denominated = rust_biguint!(STAKE_PER_POSITION) * DECIMALS;
        b_mock.set_egld_balance(staker, &(&stake_denominated * NR_STAKING_POSITIONS));

        for i in 1..=NR_STAKING_POSITIONS {
            b_mock
                .execute_tx(staker, &delegation_wrapper, &stake_denominated, |sc| {
                    sc.stake();
                })
                .assert_ok();

            b_mock.check_nft_balance(
                staker,
                LIQUID_STAKING_TOKEN_ID,
                i as u64,
                &stake_denominated,
                Some(&Empty),
            );
        }

        delegation_wrapper.address_ref().clone()
    }

    fn init_dex_mock(b_mock: &mut BlockchainStateWrapper, owner_address: &Address) -> Address {
        let dex_wrapper = b_mock.create_sc_account(
            &rust_biguint!(0),
            Some(&owner_address),
            dex_mock::contract_obj,
            "dex.wasm",
        );
        let dex_address = dex_wrapper.address_ref().clone();
        b_mock.set_esdt_balance(
            &dex_address,
            STABLECOIN_TOKEN_ID,
            &(&rust_biguint!(1000) * DECIMALS),
        );

        dex_address
    }

    fn init_price_aggregator(
        b_mock: &mut BlockchainStateWrapper,
        owner_address: &Address,
    ) -> Address {
        let rust_zero = rust_biguint!(0);
        let price_aggregator_wrapper = b_mock.create_sc_account(
            &rust_zero,
            Some(&owner_address),
            price_aggregator::contract_obj,
            "price_aggregator.wasm",
        );
        let oracle = b_mock.create_user_account(&rust_zero);

        b_mock
            .execute_tx(owner_address, &price_aggregator_wrapper, &rust_zero, |sc| {
                sc.init(
                    TokenIdentifier::egld(),
                    ManagedVec::from_single_item(managed_address!(&oracle)),
                    1,
                    0,
                    managed_biguint!(0),
                );
            })
            .assert_ok();

        b_mock
            .execute_tx(&oracle, &price_aggregator_wrapper, &rust_zero, |sc| {
                sc.submit(
                    managed_buffer!(b"EGLD"),
                    managed_buffer!(b"USD"),
                    managed_biguint!(100),
                );
            })
            .assert_ok();

        price_aggregator_wrapper.address_ref().clone()
    }
}
