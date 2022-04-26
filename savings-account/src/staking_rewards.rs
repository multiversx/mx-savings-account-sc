elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use crate::ongoing_operation::{
    LoopOp, OngoingOperationType, ANOTHER_ONGOING_OP_ERR_MSG, CALLBACK_IN_PROGRESS_ERR_MSG,
    NR_ROUNDS_WAIT_FOR_CALLBACK,
};

const RECEIVE_STAKING_REWARDS_FUNC_NAME: &[u8] = b"receiveStakingRewards";
const STAKING_REWARDS_CLAIM_GAS_PER_TOKEN: u64 = 10_000_000;

mod dex_proxy {
    elrond_wasm::imports!();

    #[elrond_wasm::proxy]
    pub trait Dex {
        #[payable("*")]
        #[endpoint(swapTokensFixedInput)]
        fn swap_tokens_fixed_input(
            &self,
            #[payment_token] token_in: TokenIdentifier,
            #[payment_amount] amount_in: BigUint,
            token_out: TokenIdentifier,
            amount_out_min: BigUint,
        ) -> EsdtTokenPayment<Self::Api>;
    }
}

mod delegation_proxy {
    elrond_wasm::imports!();

    #[elrond_wasm::proxy]
    pub trait Delegation {
        #[payable("*")]
        #[endpoint(claimRewards)]
        fn claim_rewards(
            &self,
            #[payment_multi] payments: ManagedVec<EsdtTokenPayment<Self::Api>>,
            #[var_args] opt_receive_funds_func: OptionalValue<ManagedBuffer>,
        );
    }
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct StakingPosition {
    pub prev_pos_id: u64,
    pub next_pos_id: u64,
    pub liquid_staking_nonce: u64,
}

#[elrond_wasm::module]
pub trait StakingRewardsModule:
    crate::math::MathModule
    + crate::ongoing_operation::OngoingOperationModule
    + crate::tokens::TokensModule
{
    // endpoints

    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );

        let current_round = self.blockchain().get_block_round();
        let mut pos_id = match self.load_operation() {
            OngoingOperationType::None => {
                let first_pos_id = self.get_first_staking_position_id();
                require!(first_pos_id != 0, "No staking positions available");

                first_pos_id
            }
            OngoingOperationType::ClaimStakingRewards {
                pos_id,
                async_call_fire_round,
                callback_executed,
            } => {
                let round_diff = current_round - async_call_fire_round;
                require!(
                    callback_executed || round_diff >= NR_ROUNDS_WAIT_FOR_CALLBACK,
                    CALLBACK_IN_PROGRESS_ERR_MSG
                );

                let staking_pos = self.staking_position(pos_id).get();
                staking_pos.next_pos_id
            }
            _ => sc_panic!(ANOTHER_ONGOING_OP_ERR_MSG),
        };

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let mut transfers = ManagedVec::new();
        let mut callback_pos_ids = ManagedVec::new();

        let _ = self.run_while_it_has_gas(
            || {
                let current_staking_pos = self.staking_position(pos_id).get();
                let sft_nonce = current_staking_pos.liquid_staking_nonce;

                transfers.push(EsdtTokenPayment {
                    token_identifier: liquid_staking_token_id.clone(),
                    token_nonce: sft_nonce,
                    amount: self
                        .blockchain()
                        .get_sc_balance(&liquid_staking_token_id, sft_nonce),
                    token_type: EsdtTokenType::SemiFungible,
                });
                callback_pos_ids.push(pos_id);

                if current_staking_pos.next_pos_id == 0 {
                    return LoopOp::Break;
                }

                pos_id = current_staking_pos.next_pos_id;

                LoopOp::Continue
            },
            Some(STAKING_REWARDS_CLAIM_GAS_PER_TOKEN),
        );

        let cb_ids_len = callback_pos_ids.len();
        if cb_ids_len > 0 {
            let last_pos_id = callback_pos_ids.get(cb_ids_len - 1);
            self.save_progress(&OngoingOperationType::ClaimStakingRewards {
                pos_id: last_pos_id,
                async_call_fire_round: current_round,
                callback_executed: false,
            });
        }

        if !transfers.is_empty() {
            self.delegation_proxy(self.delegation_sc_address().get())
                .claim_rewards(
                    transfers,
                    OptionalValue::Some(RECEIVE_STAKING_REWARDS_FUNC_NAME.into()),
                )
                .async_call()
                .with_callback(
                    <Self as StakingRewardsModule>::callbacks(self)
                        .claim_staking_rewards_callback(callback_pos_ids),
                )
                .call_and_exit();
        }
    }

    #[payable("*")]
    #[callback]
    fn claim_staking_rewards_callback(
        &self,
        pos_ids: ManagedVec<u64>,
        #[payment_multi] new_liquid_staking_tokens: ManagedVec<EsdtTokenPayment<Self::Api>>,
        #[call_result] result: ManagedAsyncCallResult<MultiValueEncoded<u64>>,
    ) -> OperationCompletionStatus {
        match result {
            // "result" contains nonces created by "ESDTNFTCreate calls on callee contract"
            // we don't need them, as we already have them in payment call data
            ManagedAsyncCallResult::Ok(_) => {
                let last_pos_id = match self.load_operation() {
                    OngoingOperationType::ClaimStakingRewards {
                        pos_id,
                        async_call_fire_round,
                        callback_executed: _,
                    } => {
                        self.save_progress(&OngoingOperationType::ClaimStakingRewards {
                            pos_id,
                            async_call_fire_round,
                            callback_executed: true,
                        });

                        pos_id
                    }
                    _ => sc_panic!("Invalid operation in callback"),
                };

                require!(
                    new_liquid_staking_tokens.len() == pos_ids.len(),
                    "Invalid old and new liquid staking position lengths"
                );

                // update liquid staking token nonces
                // needed to know which liquid staking SFT to return on repay
                for (pos_id, new_token) in pos_ids.iter().zip(new_liquid_staking_tokens.iter()) {
                    self.staking_position(pos_id)
                        .update(|pos| pos.liquid_staking_nonce = new_token.token_nonce);
                }

                let last_valid_id = self.last_valid_staking_position_id().get();
                if last_pos_id == last_valid_id {
                    let current_epoch = self.blockchain().get_block_epoch();
                    self.last_staking_rewards_claim_epoch().set(&current_epoch);
                    self.clear_operation();

                    OperationCompletionStatus::Completed
                } else {
                    OperationCompletionStatus::InterruptedBeforeOutOfGas
                }
            }
            ManagedAsyncCallResult::Err(_) => sc_panic!("Async call failed"),
        }
    }

    #[payable("*")]
    #[endpoint(receiveStakingRewards)]
    fn receive_staking_rewards(&self) {
        let caller = self.blockchain().get_caller();
        let delegation_sc_address = self.delegation_sc_address().get();
        require!(
            caller == delegation_sc_address,
            "Only the Delegation SC may call this function"
        );
    }

    // TODO: Convert EGLD to WrapedEgld first (DEX does not convert EGLD directly)
    #[endpoint(convertStakingTokenToStablecoin)]
    fn convert_staking_token_to_stablecoin(&self) {
        self.require_no_ongoing_operation();

        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            last_claim_epoch == current_epoch,
            "Must claim rewards for this epoch first"
        );

        let last_staking_token_convert_epoch = self.last_staking_token_convert_epoch().get();
        require!(
            current_epoch > last_staking_token_convert_epoch,
            "Already converted to stablecoins this epoch"
        );

        let dex_sc_address = self.dex_swap_sc_address().get();

        let staking_token_id = self.staked_token_id().get();
        let staking_token_balance = self.blockchain().get_sc_balance(&staking_token_id, 0);
        let stablecoin_token_id = self.stablecoin_token_id().get();

        let received_payment: EsdtTokenPayment<Self::Api> = self
            .dex_proxy(dex_sc_address)
            .swap_tokens_fixed_input(
                staking_token_id,
                staking_token_balance,
                stablecoin_token_id.clone(),
                1u32.into(),
            )
            .execute_on_dest_context();

        require!(
            received_payment.token_identifier == stablecoin_token_id,
            "Invalid token received from PAIR swap"
        );

        self.stablecoin_reserves()
            .update(|stablecoin_reserves| *stablecoin_reserves += received_payment.amount);
        self.last_staking_token_convert_epoch().set(current_epoch);
    }

    #[endpoint(calculateTotalLenderRewards)]
    fn calculate_total_lender_rewards(&self) -> OperationCompletionStatus {
        let reward_percentage_per_epoch = self.lender_rewards_percentage_per_epoch().get();
        let last_calculate_rewards_epoch = self.last_calculate_rewards_epoch().get();

        let last_staking_token_convert_epoch = self.last_staking_token_convert_epoch().get();
        let current_epoch = self.blockchain().get_block_epoch();

        require!(
            last_staking_token_convert_epoch == current_epoch,
            "Must claim staking rewards and convert to stablecoin for this epoch first"
        );
        require!(
            last_calculate_rewards_epoch < current_epoch,
            "Already calculated rewards this epoch"
        );

        let last_lend_nonce = self.last_valid_lend_nonce().get();
        require!(last_lend_nonce > 0, "No lenders");

        let (
            mut prev_lend_nonce,
            mut current_lend_nonce,
            mut total_rewards,
            mut nr_lenders_with_rewards,
        ) = match self.load_operation() {
            OngoingOperationType::None => {
                (0u64, self.get_first_lend_nonce(), BigUint::zero(), 0u64)
            }
            OngoingOperationType::CalculateTotalLenderRewards {
                prev_lend_nonce,
                current_lend_nonce,
                total_rewards,
                nr_lenders_with_rewards,
            } => (
                prev_lend_nonce,
                current_lend_nonce,
                total_rewards,
                nr_lenders_with_rewards,
            ),
            _ => sc_panic!(ANOTHER_ONGOING_OP_ERR_MSG),
        };
        let current_epoch = self.blockchain().get_block_epoch();

        let run_result = self.run_while_it_has_gas(
            || {
                let next_lend_nonce = self.lend_nonces_list(current_lend_nonce).get();

                if self.lend_metadata(current_lend_nonce).is_empty() {
                    self.remove_lend_nonce(prev_lend_nonce, current_lend_nonce);
                } else {
                    let metadata = self.lend_metadata(current_lend_nonce).get();
                    let reward_amount = self.compute_reward_amount(
                        &metadata.amount_in_circulation,
                        metadata.lend_epoch,
                        current_epoch,
                        &reward_percentage_per_epoch,
                    );
                    if reward_amount > 0u32 {
                        nr_lenders_with_rewards += 1;
                        total_rewards += reward_amount;
                    }

                    prev_lend_nonce = current_lend_nonce;
                }

                current_lend_nonce = next_lend_nonce;
                if current_lend_nonce == 0 {
                    LoopOp::Break
                } else {
                    LoopOp::Continue
                }
            },
            None,
        );

        let mut missing_rewards = self.missing_rewards().get();
        total_rewards -= &missing_rewards;

        match run_result {
            OperationCompletionStatus::Completed => {
                if nr_lenders_with_rewards == 0 {
                    return run_result;
                }

                let prev_unclaimed_rewards = self.unclaimed_rewards().get();
                let extra_unclaimed = &total_rewards - &prev_unclaimed_rewards;
                let stablecoin_reserves = self.stablecoin_reserves().get();

                let mut leftover_reserves: BigUint;
                if extra_unclaimed > stablecoin_reserves {
                    let extra_missing_rewards = &extra_unclaimed - &stablecoin_reserves;
                    total_rewards -= &extra_missing_rewards;
                    missing_rewards += extra_missing_rewards;

                    leftover_reserves = BigUint::zero();
                } else {
                    leftover_reserves = stablecoin_reserves - extra_unclaimed;
                    if leftover_reserves >= missing_rewards {
                        total_rewards += &missing_rewards;
                        leftover_reserves -= &missing_rewards;
                        missing_rewards = BigUint::zero();
                    } else {
                        total_rewards += &leftover_reserves;
                        missing_rewards -= &leftover_reserves;
                        leftover_reserves = BigUint::zero();
                    }
                }

                // round up
                let penalty_per_lender =
                    (&missing_rewards + (nr_lenders_with_rewards - 1)) / nr_lenders_with_rewards;
                self.penalty_amount_per_lender().set(&penalty_per_lender);
                self.missing_rewards().set(&missing_rewards);

                let current_epoch = self.blockchain().get_block_epoch();
                self.last_calculate_rewards_epoch().set(&current_epoch);
                self.unclaimed_rewards().set(&total_rewards);
                self.stablecoin_reserves().set(&leftover_reserves);
            }
            OperationCompletionStatus::InterruptedBeforeOutOfGas => {
                self.save_progress(&OngoingOperationType::CalculateTotalLenderRewards {
                    prev_lend_nonce,
                    current_lend_nonce,
                    total_rewards,
                    nr_lenders_with_rewards,
                });
            }
        };

        run_result
    }

    // private

    fn get_first_staking_position_id(&self) -> u64 {
        self.staking_position(0).get().next_pos_id
    }

    fn get_first_staking_position(&self) -> Option<StakingPosition> {
        let first_id = self.get_first_staking_position_id();
        if first_id != 0 {
            Some(self.staking_position(first_id).get())
        } else {
            None
        }
    }

    fn add_staking_position(&self, liquid_staking_nonce: u64) -> u64 {
        let existing_id = self
            .staking_position_nonce_to_id(liquid_staking_nonce)
            .get();
        if existing_id != 0 {
            return existing_id;
        }

        let prev_last_id = self.last_valid_staking_position_id().get();
        let new_last_id = prev_last_id + 1;

        self.staking_position(prev_last_id)
            .update(|last_pos| last_pos.next_pos_id = new_last_id);
        self.staking_position(new_last_id).set(&StakingPosition {
            next_pos_id: 0,
            prev_pos_id: prev_last_id,
            liquid_staking_nonce,
        });

        self.staking_position_nonce_to_id(liquid_staking_nonce)
            .set(&new_last_id);
        self.last_valid_staking_position_id().set(&new_last_id);

        new_last_id
    }

    fn remove_staking_position(&self, pos_id: u64) {
        if pos_id == 0 {
            return;
        }

        let pos = self.staking_position(pos_id).get();

        // re-connect nodes
        self.staking_position(pos.prev_pos_id)
            .update(|prev_pos| prev_pos.next_pos_id = pos.next_pos_id);

        if pos.next_pos_id != 0 {
            self.staking_position(pos.next_pos_id)
                .update(|next_pos| next_pos.prev_pos_id = pos.prev_pos_id);
        }

        let last_valid_pos_id = self.last_valid_staking_position_id().get();
        if pos_id == last_valid_pos_id {
            self.last_valid_staking_position_id().set(&pos.prev_pos_id)
        }

        self.staking_position(pos_id).clear();
    }

    // proxies

    #[proxy]
    fn dex_proxy(&self, address: ManagedAddress) -> dex_proxy::Proxy<Self::Api>;

    #[proxy]
    fn delegation_proxy(&self, address: ManagedAddress) -> delegation_proxy::Proxy<Self::Api>;

    // storage

    #[view(getDelegationScAddress)]
    #[storage_mapper("delegationScAddress")]
    fn delegation_sc_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[view(getDexSwapScAddress)]
    #[storage_mapper("dexSwapScAddress")]
    fn dex_swap_sc_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[storage_mapper("stakingPosition")]
    fn staking_position(&self, pos_id: u64) -> SingleValueMapper<StakingPosition>;

    #[storage_mapper("stakingPositionNonceToId")]
    fn staking_position_nonce_to_id(&self, liquid_staking_nonce: u64) -> SingleValueMapper<u64>;

    #[storage_mapper("lastValidStakingPositionId")]
    fn last_valid_staking_position_id(&self) -> SingleValueMapper<u64>;

    #[view(getLastStakingRewardsClaimEpoch)]
    #[storage_mapper("lastStakingRewardsClaimEpoch")]
    fn last_staking_rewards_claim_epoch(&self) -> SingleValueMapper<u64>;

    #[view(getLastStakingTokenConvertEpoch)]
    #[storage_mapper("lastStakingTokenConvertEpoch")]
    fn last_staking_token_convert_epoch(&self) -> SingleValueMapper<u64>;

    #[view(getLastCalculateRewardsEpoch)]
    #[storage_mapper("lastCalculateRewardsEpoch")]
    fn last_calculate_rewards_epoch(&self) -> SingleValueMapper<u64>;

    #[view(getLenderRewardsPercentagePerEpoch)]
    #[storage_mapper("lenderRewardsPercentagePerEpoch")]
    fn lender_rewards_percentage_per_epoch(&self) -> SingleValueMapper<BigUint>;

    #[view(getUnclaimedRewards)]
    #[storage_mapper("unclaimedRewards")]
    fn unclaimed_rewards(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("missingRewards")]
    fn missing_rewards(&self) -> SingleValueMapper<BigUint>;

    #[view(getPenaltyAmountPerLender)]
    #[storage_mapper("penaltyAmountPerLender")]
    fn penalty_amount_per_lender(&self) -> SingleValueMapper<BigUint>;

    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<BigUint>;
}
