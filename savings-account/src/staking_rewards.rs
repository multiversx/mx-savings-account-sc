elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use crate::{
    math::DEFAULT_DECIMALS,
    ongoing_operation::{
        LoopOp, OngoingOperationType, CALLBACK_IN_PROGRESS_ERR_MSG, NR_ROUNDS_WAIT_FOR_CALLBACK,
    },
    staking_positions_mapper::StakingPositionsMapper,
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
            opt_receive_funds_func: OptionalValue<ManagedBuffer>,
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
    + crate::common_storage::CommonStorageModule
{
    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );

        let staking_positions_mapper = self.staking_positions();
        let current_round = self.blockchain().get_block_round();
        let mut pos_id = match self.load_operation() {
            OngoingOperationType::None => {
                let first_pos_id = staking_positions_mapper.get_first_staking_position_id();
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

                let staking_pos = staking_positions_mapper.get_staking_position(pos_id);
                staking_pos.next_pos_id
            }
        };

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let mut transfers = ManagedVec::new();
        let mut callback_pos_ids = ManagedVec::new();

        let _ = self.run_while_it_has_gas(
            || {
                let current_staking_pos = staking_positions_mapper.get_staking_position(pos_id);
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
                    OptionalValue::Some(ManagedBuffer::new_from_bytes(
                        RECEIVE_STAKING_REWARDS_FUNC_NAME,
                    )),
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

                let mut staking_positions_mapper = self.staking_positions();

                // update liquid staking token nonces
                // needed to know which liquid staking SFT to return on repay
                for (pos_id, new_token) in pos_ids.iter().zip(new_liquid_staking_tokens.iter()) {
                    staking_positions_mapper.update_staking_position(pos_id, |pos| {
                        pos.liquid_staking_nonce = new_token.token_nonce
                    });
                }

                let last_valid_id = staking_positions_mapper.get_last_valid_staking_pos_id();
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
                1u32,
            )
            .execute_on_dest_context();

        require!(
            received_payment.token_identifier == stablecoin_token_id,
            "Invalid token received from PAIR swap"
        );

        self.stablecoin_reserves()
            .update(|stablecoin_reserves| *stablecoin_reserves += received_payment.amount);
        self.last_staking_token_convert_epoch().set(current_epoch);

        self.update_global_lender_rewards();
    }

    fn update_global_lender_rewards(&self) {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_rewards_update_epoch = self.last_rewards_update_epoch().get();
        let total_lent_amount = self.lent_amount().get();
        let extra_rewards_needed = if last_rewards_update_epoch < current_epoch {
            self.total_missed_rewards_by_claim_since_last_calculation()
                .clear();

            let reward_percentage_per_epoch = self.lender_rewards_percentage_per_epoch().get();
            self.compute_reward_amount(
                &total_lent_amount,
                last_rewards_update_epoch,
                current_epoch,
                &reward_percentage_per_epoch,
            )
        } else {
            BigUint::zero()
        };

        let mut stablecoin_reserves = self.stablecoin_reserves().get();
        let mut missing_rewards = self.missing_rewards().get();
        if extra_rewards_needed > stablecoin_reserves {
            let extra_missing_rewards = &extra_rewards_needed - &stablecoin_reserves;
            missing_rewards += extra_missing_rewards;
            stablecoin_reserves = BigUint::zero();
        } else {
            stablecoin_reserves -= extra_rewards_needed;

            if stablecoin_reserves >= missing_rewards {
                stablecoin_reserves -= &missing_rewards;
                missing_rewards = BigUint::zero();
            } else {
                missing_rewards -= &stablecoin_reserves;
                stablecoin_reserves = BigUint::zero();
            }
        }

        // rounded up
        let penalty_per_lend_token = if missing_rewards == 0 || total_lent_amount == 0 {
            BigUint::zero()
        } else {
            let missed_by_claim = self
                .total_missed_rewards_by_claim_since_last_calculation()
                .get();
            let total_missing_rewards = &missing_rewards + &missed_by_claim;

            (total_missing_rewards * DEFAULT_DECIMALS + &total_lent_amount - 1u32)
                / total_lent_amount
        };

        self.penalty_per_lend_token().set(&penalty_per_lend_token);
        self.missing_rewards().set(&missing_rewards);
        self.stablecoin_reserves().set(&stablecoin_reserves);
        self.last_rewards_update_epoch().set(current_epoch);
    }

    #[proxy]
    fn dex_proxy(&self, address: ManagedAddress) -> dex_proxy::Proxy<Self::Api>;

    #[proxy]
    fn delegation_proxy(&self, address: ManagedAddress) -> delegation_proxy::Proxy<Self::Api>;

    #[view(getDelegationScAddress)]
    #[storage_mapper("delegationScAddress")]
    fn delegation_sc_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[view(getDexSwapScAddress)]
    #[storage_mapper("dexSwapScAddress")]
    fn dex_swap_sc_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[storage_mapper("stakingPosition")]
    fn staking_positions(&self) -> StakingPositionsMapper<Self::Api>;

    #[view(getLastStakingRewardsClaimEpoch)]
    #[storage_mapper("lastStakingRewardsClaimEpoch")]
    fn last_staking_rewards_claim_epoch(&self) -> SingleValueMapper<u64>;

    #[view(getLastStakingTokenConvertEpoch)]
    #[storage_mapper("lastStakingTokenConvertEpoch")]
    fn last_staking_token_convert_epoch(&self) -> SingleValueMapper<u64>;

    #[storage_mapper("lastRewardsUpdateEpoch")]
    fn last_rewards_update_epoch(&self) -> SingleValueMapper<u64>;

    #[view(getLenderRewardsPercentagePerEpoch)]
    #[storage_mapper("lenderRewardsPercentagePerEpoch")]
    fn lender_rewards_percentage_per_epoch(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("missingRewards")]
    fn missing_rewards(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("penaltyPerLendToken")]
    fn penalty_per_lend_token(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("totalMissedRewardsByClaimSinceLastCalculation")]
    fn total_missed_rewards_by_claim_since_last_calculation(&self) -> SingleValueMapper<BigUint>;
}
