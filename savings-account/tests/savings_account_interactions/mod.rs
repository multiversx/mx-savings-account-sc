use crate::savings_account_setup::{
    SavingsAccountSetup, BORROW_TOKEN_ID, DECIMALS, LEND_TOKEN_ID, LIQUID_STAKING_TOKEN_ID,
    NR_STAKING_POSITIONS, STABLECOIN_TOKEN_ID,
};
use elrond_wasm::{elrond_codec::multi_types::OptionalValue, types::Address};
use elrond_wasm_debug::tx_mock::TxInputESDT;
use elrond_wasm_debug::{
    managed_biguint, managed_token_id, rust_biguint, tx_mock::TxResult, DebugApi,
};
use savings_account::common_storage::CommonStorageModule;
use savings_account::model::{BorrowMetadata, LendMetadata};
use savings_account::staking_positions_mapper::StakingPosition;
use savings_account::staking_rewards::StakingRewardsModule;
use savings_account::SavingsAccount;

impl<SavingsAccountObjBuilder> SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    pub fn call_lend(
        &mut self,
        lender: &Address,
        amount: u64,
        expected_lend_nonce: u64,
    ) -> TxResult {
        self.b_mock.execute_esdt_transfer(
            lender,
            &self.sa_wrapper,
            STABLECOIN_TOKEN_ID,
            0,
            &rust_biguint!(amount),
            |sc| {
                let lend_tokens = sc.lend();
                assert_eq!(
                    lend_tokens.token_identifier,
                    managed_token_id!(LEND_TOKEN_ID)
                );
                assert_eq!(lend_tokens.token_nonce, expected_lend_nonce);
                assert_eq!(lend_tokens.amount, managed_biguint!(amount));
            },
        )
    }

    pub fn call_lender_claim_rewards(
        &mut self,
        lender: &Address,
        lend_token_nonce: u64,
        lend_token_amount: u64,
        expected_new_lend_nonce: u64,
        expected_rewards_amount: u64,
        reject_penalty: bool,
    ) -> TxResult {
        self.b_mock.execute_esdt_transfer(
            lender,
            &self.sa_wrapper,
            LEND_TOKEN_ID,
            lend_token_nonce,
            &rust_biguint!(lend_token_amount),
            |sc| {
                let (new_lend_tokens, rewards) = sc
                    .lender_claim_rewards(OptionalValue::Some(reject_penalty))
                    .into_tuple();

                assert_eq!(
                    new_lend_tokens.token_identifier,
                    managed_token_id!(LEND_TOKEN_ID)
                );
                assert_eq!(new_lend_tokens.token_nonce, expected_new_lend_nonce);
                assert_eq!(new_lend_tokens.amount, managed_biguint!(lend_token_amount));

                assert_eq!(
                    rewards.token_identifier,
                    managed_token_id!(STABLECOIN_TOKEN_ID)
                );
                assert_eq!(rewards.token_nonce, 0);
                assert_eq!(rewards.amount, managed_biguint!(expected_rewards_amount));
            },
        )
    }

    pub fn call_withdraw(
        &mut self,
        lender: &Address,
        lend_token_nonce: u64,
        lend_token_amount: u64,
        expected_withdraw_amount: u64,
    ) -> TxResult {
        self.b_mock.execute_esdt_transfer(
            lender,
            &self.sa_wrapper,
            LEND_TOKEN_ID,
            lend_token_nonce,
            &rust_biguint!(lend_token_amount),
            |sc| {
                let stablecoin_out = sc.withdraw(OptionalValue::None);
                assert_eq!(
                    stablecoin_out.amount,
                    managed_biguint!(expected_withdraw_amount)
                );
            },
        )
    }

    pub fn call_borrow(
        &mut self,
        borrower: &Address,
        liq_staking_nonce: u64,
        liq_staking_amount: &num_bigint::BigUint,
        expected_borrow_nonce: u64,
        expected_stablecoin_amount: u64,
    ) -> TxResult {
        self.b_mock.execute_esdt_transfer(
            borrower,
            &self.sa_wrapper,
            LIQUID_STAKING_TOKEN_ID,
            liq_staking_nonce,
            liq_staking_amount,
            |sc| {
                let (borrow_tokens, stablecoins) = sc.borrow().into_tuple();

                assert_eq!(
                    borrow_tokens.token_identifier,
                    managed_token_id!(BORROW_TOKEN_ID)
                );
                assert_eq!(borrow_tokens.token_nonce, expected_borrow_nonce);
                assert_eq!(
                    borrow_tokens.amount,
                    elrond_wasm::types::BigUint::from_bytes_be(&liq_staking_amount.to_bytes_be())
                );

                assert_eq!(
                    stablecoins.token_identifier,
                    managed_token_id!(STABLECOIN_TOKEN_ID)
                );
                assert_eq!(stablecoins.token_nonce, 0);
                assert_eq!(
                    stablecoins.amount,
                    managed_biguint!(expected_stablecoin_amount)
                );
            },
        )
    }

    pub fn call_repay(
        &mut self,
        borrower: &Address,
        borrow_token_nonce: u64,
        borrow_token_amount: &num_bigint::BigUint,
        stablecoin_amount: u64,
        expected_liq_staking_token_nonce: u64,
        expected_leftover_stablecoins: u64,
    ) -> TxResult {
        let transfers = vec![
            TxInputESDT {
                token_identifier: BORROW_TOKEN_ID.to_vec(),
                nonce: borrow_token_nonce,
                value: borrow_token_amount.clone(),
            },
            TxInputESDT {
                token_identifier: STABLECOIN_TOKEN_ID.to_vec(),
                nonce: 0,
                value: rust_biguint!(stablecoin_amount),
            },
        ];
        self.b_mock
            .execute_esdt_multi_transfer(borrower, &self.sa_wrapper, &transfers, |sc| {
                let (liq_staking_tokens, leftover_stablecoins) = sc.repay().into_tuple();

                assert_eq!(
                    liq_staking_tokens.token_identifier,
                    managed_token_id!(LIQUID_STAKING_TOKEN_ID)
                );
                assert_eq!(
                    liq_staking_tokens.token_nonce,
                    expected_liq_staking_token_nonce
                );
                assert_eq!(
                    liq_staking_tokens.amount,
                    elrond_wasm::types::BigUint::from_bytes_be(&borrow_token_amount.to_bytes_be())
                );

                assert_eq!(
                    leftover_stablecoins.token_identifier,
                    managed_token_id!(STABLECOIN_TOKEN_ID)
                );
                assert_eq!(leftover_stablecoins.token_nonce, 0);
                assert_eq!(
                    leftover_stablecoins.amount,
                    managed_biguint!(expected_leftover_stablecoins)
                );
            })
    }

    pub fn call_claim_staking_rewards(&mut self) -> TxResult {
        self.b_mock.execute_tx(
            &self.owner_address,
            &self.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                sc.claim_staking_rewards();
            },
        )
    }

    pub fn call_convert_staking_token(&mut self) -> TxResult {
        self.b_mock.execute_tx(
            &self.owner_address,
            &self.sa_wrapper,
            &rust_biguint!(0),
            |sc| {
                sc.convert_staking_token_to_stablecoin();
            },
        )
    }

    pub fn call_get_penaly_amount(&mut self, lend_amount: u64) -> u64 {
        let mut penalty = 0;
        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
                let penalty_biguint = sc.get_penalty_amount_view(managed_biguint!(lend_amount));
                penalty = penalty_biguint.to_u64().unwrap();
            })
            .assert_ok();

        penalty
    }

    pub fn call_get_lender_claimable_rewards(&mut self, lend_epoch: u64, lend_amount: u64) -> u64 {
        let mut rewards = 0;
        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
                let rewards_biguint =
                    sc.get_lender_claimable_rewards_view(lend_epoch, managed_biguint!(lend_amount));
                rewards = rewards_biguint.to_u64().unwrap();
            })
            .assert_ok();

        rewards
    }
}

impl<SavingsAccountObjBuilder> SavingsAccountSetup<SavingsAccountObjBuilder>
where
    SavingsAccountObjBuilder: 'static + Copy + Fn() -> savings_account::ContractObj<DebugApi>,
{
    pub fn default_lenders(&mut self) {
        let first_lender = self.first_lender_address.clone();
        let second_lender = self.second_lender_address.clone();

        self.b_mock.set_block_epoch(20);

        // lender 1 - lend ok
        self.call_lend(&first_lender, 100_000, 1).assert_ok();
        self.b_mock.check_nft_balance(
            &first_lender,
            LEND_TOKEN_ID,
            1,
            &rust_biguint!(100_000),
            Some(&LendMetadata { lend_epoch: 20 }),
        );
        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
                let expected_lent_amount = managed_biguint!(100_000);
                let actual_lent_amount = sc.lent_amount().get();
                assert_eq!(expected_lent_amount, actual_lent_amount);
            })
            .assert_ok();

        // lender 1 try claim rewards
        self.call_lender_claim_rewards(&first_lender, 1, 100_000, 2, 0, true)
            .assert_user_error("No rewards to claim");

        // try claim staking rewards - no staking positions
        self.call_claim_staking_rewards()
            .assert_user_error("No staking positions available");

        self.b_mock.set_block_epoch(21);

        // lender 2 - lend ok
        self.call_lend(&second_lender, 50_000, 2).assert_ok();
        self.b_mock.check_nft_balance(
            &second_lender,
            LEND_TOKEN_ID,
            2,
            &rust_biguint!(50_000),
            Some(&LendMetadata { lend_epoch: 21 }),
        );
        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
                let expected_lent_amount = managed_biguint!(150_000);
                let actual_lent_amount = sc.lent_amount().get();
                assert_eq!(expected_lent_amount, actual_lent_amount);
            })
            .assert_ok();
    }

    pub fn default_borrows(&mut self) {
        let borrower = self.borrower_address.clone();
        let liq_staking_amount = rust_biguint!(250) * DECIMALS;
        let stablecoin_amount_per_borrow = 18_750;

        self.b_mock.set_block_epoch(25);

        for i in 1..=NR_STAKING_POSITIONS {
            self.call_borrow(
                &borrower,
                i as u64,
                &liq_staking_amount,
                i as u64,
                stablecoin_amount_per_borrow,
            )
            .assert_ok();
            self.b_mock.check_nft_balance(
                &borrower,
                BORROW_TOKEN_ID,
                i as u64,
                &liq_staking_amount,
                Some(&BorrowMetadata::<DebugApi> {
                    borrow_epoch: 25,
                    staked_token_value_in_dollars_at_borrow: managed_biguint!(100),
                    staking_position_id: i as u64,
                }),
            );

            self.b_mock
                .execute_query(&self.sa_wrapper, |sc| {
                    let expected_borrowed_amount =
                        managed_biguint!(stablecoin_amount_per_borrow * i as u64);
                    let actual_borrowed_amount = sc.borrowed_amount().get();
                    assert_eq!(expected_borrowed_amount, actual_borrowed_amount);
                })
                .assert_ok();
        }

        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
                // check staking positions mapper
                let mapper = sc.staking_positions();
                assert_eq!(
                    mapper.get_staking_position(1),
                    StakingPosition {
                        liquid_staking_nonce: 1,
                        prev_pos_id: 0,
                        next_pos_id: 2,
                    }
                );
                assert_eq!(
                    mapper.get_staking_position(2),
                    StakingPosition {
                        liquid_staking_nonce: 2,
                        prev_pos_id: 1,
                        next_pos_id: 3,
                    }
                );
                assert_eq!(
                    mapper.get_staking_position(3),
                    StakingPosition {
                        liquid_staking_nonce: 3,
                        prev_pos_id: 2,
                        next_pos_id: 4,
                    }
                );
                assert_eq!(
                    mapper.get_staking_position(4),
                    StakingPosition {
                        liquid_staking_nonce: 4,
                        prev_pos_id: 3,
                        next_pos_id: 0,
                    }
                );
            })
            .assert_ok();

        self.b_mock
            .check_esdt_balance(&borrower, STABLECOIN_TOKEN_ID, &rust_biguint!(4 * 18_750));
    }

    pub fn default_claim_rewards(&mut self) {
        let first_lender = self.first_lender_address.clone();
        let second_lender = self.second_lender_address.clone();

        self.b_mock.set_block_epoch(50);

        // 100,000 out of total 150,000 => ~66% of 12,250
        assert_eq!(self.call_get_penaly_amount(100_000), 8_166);

        // 50,000 out of total 150,000 => ~33% of 12,250
        assert_eq!(self.call_get_penaly_amount(50_000), 4_083);

        // (50 - 20) * 0.5% * 100,000 - 6,125 = 15,000 - 8,166 = 6,834
        let first_lender_rewards = 6_834;
        assert_eq!(
            self.call_get_lender_claimable_rewards(20, 100_000),
            first_lender_rewards
        );

        // (50 - 21) * 0.5% * 50,000 - 4,083 = 7,250 - 4,083 = 3,167
        let second_lender_rewards = 3_167;
        assert_eq!(
            self.call_get_lender_claimable_rewards(21, 50_000),
            second_lender_rewards
        );

        // lender 1 try claim without penalty
        self.call_lender_claim_rewards(&first_lender, 1, 100_000, 3, first_lender_rewards, true)
            .assert_user_error("Rewards have penalty");

        // lender 1 claim ok
        self.call_lender_claim_rewards(&first_lender, 1, 100_000, 3, first_lender_rewards, false)
            .assert_ok();

        self.b_mock.check_nft_balance(
            &first_lender,
            LEND_TOKEN_ID,
            3,
            &rust_biguint!(100_000),
            Some(&LendMetadata { lend_epoch: 50 }),
        );
        self.b_mock.check_esdt_balance(
            &first_lender,
            STABLECOIN_TOKEN_ID,
            &rust_biguint!(first_lender_rewards),
        );

        // lender 2 claim ok
        self.call_lender_claim_rewards(&second_lender, 2, 50_000, 3, second_lender_rewards, false)
            .assert_ok();

        // lender 1 and 2 now have same lend token nonce (because of same lend_epoch)
        // also, lender 2 initially has 50_000 stablecoins, as they only lent half
        self.b_mock.check_nft_balance(
            &second_lender,
            LEND_TOKEN_ID,
            3,
            &rust_biguint!(50_000),
            Some(&LendMetadata { lend_epoch: 50 }),
        );
        self.b_mock.check_esdt_balance(
            &second_lender,
            STABLECOIN_TOKEN_ID,
            &rust_biguint!(50_000 + second_lender_rewards),
        );

        // check Savings Account internal state
        self.b_mock
            .execute_query(&self.sa_wrapper, |sc| {
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
}
