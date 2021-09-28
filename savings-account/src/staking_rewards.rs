elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use crate::{
    multi_transfer::MultiTransferAsync,
    ongoing_operation::{
        LoopOp, OngoingOperationType, ANOTHER_ONGOING_OP_ERR_MSG, CALLBACK_IN_PROGRESS_ERR_MSG,
    },
};

const DELEGATION_CLAIM_REWARDS_ENDPOINT: &[u8] = b"claimRewards";
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
            #[payment_amount] amount_in: Self::BigUint,
            token_out: TokenIdentifier,
            amount_out_min: Self::BigUint,
            #[var_args] opt_accept_funds_func: OptionalArg<BoxedBytes>,
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
    + crate::multi_transfer::MultiTransferModule
    + crate::ongoing_operation::OngoingOperationModule
    + crate::tokens::TokensModule
{
    // endpoints

    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) -> SCResult<OptionalResult<MultiTransferAsync<Self::SendApi>>> {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );

        let mut pos_id = match self.load_operation() {
            OngoingOperationType::None => {
                let first_pos_id = self.get_first_staking_position_id();
                require!(first_pos_id != 0, "No staking positions available");

                first_pos_id
            }
            OngoingOperationType::ClaimStakingRewards {
                pos_id,
                callback_executed,
            } => {
                require!(callback_executed, CALLBACK_IN_PROGRESS_ERR_MSG);

                let staking_pos = self.staking_position(pos_id).get();
                staking_pos.next_pos_id
            }
            _ => return sc_error!(ANOTHER_ONGOING_OP_ERR_MSG),
        };

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let mut transfers = Vec::new();
        let mut callback_pos_ids = Vec::new();

        let _ = self.run_while_it_has_gas(
            || {
                let current_staking_pos = self.staking_position(pos_id).get();
                let sft_nonce = current_staking_pos.liquid_staking_nonce;

                transfers.push(crate::multi_transfer::EsdtTokenPayment {
                    token_name: liquid_staking_token_id.clone(),
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
        )?;

        let last_pos_id = callback_pos_ids[callback_pos_ids.len() - 1];
        self.save_progress(&OngoingOperationType::ClaimStakingRewards {
            pos_id: last_pos_id,
            callback_executed: false,
        });

        if !transfers.is_empty() {
            // TODO: Use SC proxy instead of manual call
            let mut async_call = MultiTransferAsync::new(
                self.send(),
                self.delegation_sc_address().get(),
                DELEGATION_CLAIM_REWARDS_ENDPOINT,
                transfers,
            );
            async_call.add_endpoint_arg(&RECEIVE_STAKING_REWARDS_FUNC_NAME);

            async_call.add_callback(b"claim_staking_rewards_callback");
            for cb_pos in callback_pos_ids {
                async_call.add_callback_arg(&cb_pos);
            }

            Ok(OptionalResult::Some(async_call))
        } else {
            Ok(OptionalResult::None)
        }
    }

    #[payable("*")]
    #[callback]
    fn claim_staking_rewards_callback(
        &self,
        #[call_result] result: AsyncCallResult<VarArgs<u64>>,
        pos_ids: VarArgs<u64>,
    ) -> SCResult<OperationCompletionStatus> {
        match result {
            // "result" contains nonces created by "ESDTNFTCreate calls on callee contract"
            // we don't need them, as we already have them in payment call data
            AsyncCallResult::Ok(_) => {
                let last_pos_id = match self.load_operation() {
                    OngoingOperationType::ClaimStakingRewards {
                        pos_id,
                        callback_executed: _,
                    } => {
                        self.save_progress(&OngoingOperationType::ClaimStakingRewards {
                            pos_id,
                            callback_executed: true,
                        });

                        pos_id
                    }
                    _ => return sc_error!("Invalid operation in callback"),
                };

                let new_liquid_staking_tokens = self.get_all_esdt_transfers();
                require!(
                    new_liquid_staking_tokens.len() == pos_ids.len(),
                    "Invalid old and new liquid staking position lengths"
                );

                // update liquid staking token nonces
                // needed to know which liquid staking SFT to return on repay
                for (pos_id, new_token) in pos_ids
                    .into_vec()
                    .iter()
                    .zip(new_liquid_staking_tokens.iter())
                {
                    self.staking_position(*pos_id)
                        .update(|pos| pos.liquid_staking_nonce = new_token.token_nonce);
                }

                let last_valid_id = self.last_valid_staking_position_id().get();
                if last_pos_id == last_valid_id {
                    let current_epoch = self.blockchain().get_block_epoch();
                    self.last_staking_rewards_claim_epoch().set(&current_epoch);
                    self.clear_operation();

                    Ok(OperationCompletionStatus::Completed)
                } else {
                    Ok(OperationCompletionStatus::InterruptedBeforeOutOfGas)
                }
            }
            AsyncCallResult::Err(err) => Err(SCError::from(err.err_msg)),
        }
    }

    #[payable("*")]
    #[endpoint(receiveStakingRewards)]
    fn receive_staking_rewards(&self) -> SCResult<()> {
        let caller = self.blockchain().get_caller();
        let delegation_sc_address = self.delegation_sc_address().get();
        require!(
            caller == delegation_sc_address,
            "Only the Delegation SC may call this function"
        );
        Ok(())
    }

    // TODO: Convert EGLD to WrapedEgld first (DEX does not convert EGLD directly)
    #[endpoint(convertStakingTokenToStablecoin)]
    fn convert_staking_token_to_stablecoin(&self) -> SCResult<AsyncCall<Self::SendApi>> {
        self.require_no_ongoing_operation()?;

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

        self.save_progress(&OngoingOperationType::ConvertStakingTokenToStablecoin);

        Ok(self
            .dex_proxy(dex_sc_address)
            .swap_tokens_fixed_input(
                staking_token_id,
                staking_token_balance,
                stablecoin_token_id,
                Self::BigUint::zero(),
                OptionalArg::Some(b"receive_stablecoin_after_convert"[..].into()),
            )
            .async_call()
            .with_callback(
                <Self as StakingRewardsModule>::callbacks(&self)
                    .convert_staking_token_to_stablecoin_callback(),
            ))
    }

    // name is intentionally left without camel case to decrease the chance of accidental calls by users
    // we do not check the caller here to save gas, as the DEX SC only allocates very little gas for this call
    #[payable("*")]
    #[endpoint]
    fn receive_stablecoin_after_convert(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only receive stablecoins"
        );

        self.stablecoin_reserves()
            .update(|stablecoin_reserves| *stablecoin_reserves += payment_amount);

        Ok(())
    }

    #[callback]
    fn convert_staking_token_to_stablecoin_callback(
        &self,
        #[call_result] result: AsyncCallResult<VarArgs<BoxedBytes>>,
    ) {
        match result {
            AsyncCallResult::Ok(_) => {
                let current_epoch = self.blockchain().get_block_epoch();
                self.last_staking_token_convert_epoch().set(&current_epoch);
            }
            AsyncCallResult::Err(_) => {}
        }

        self.clear_operation();
    }

    #[endpoint(calculateTotalLenderRewards)]
    fn calculate_total_lender_rewards(&self) -> SCResult<OperationCompletionStatus> {
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

        let (mut current_lend_nonce, mut total_rewards) = match self.load_operation() {
            OngoingOperationType::None => (1u64, Self::BigUint::zero()),
            OngoingOperationType::CalculateTotalLenderRewards {
                lend_nonce,
                total_rewards_be_bytes,
            } => (
                lend_nonce,
                Self::BigUint::from_bytes_be(total_rewards_be_bytes.as_slice()),
            ),
            _ => return sc_error!(ANOTHER_ONGOING_OP_ERR_MSG),
        };
        let last_lend_nonce = self.blockchain().get_current_esdt_nft_nonce(
            &self.blockchain().get_sc_address(),
            &self.lend_token_id().get(),
        );
        let current_epoch = self.blockchain().get_block_epoch();

        let run_result = self.run_while_it_has_gas(
            || {
                // TODO: Use something like a SetMapper or a custom mapper that will hold valid nonces
                // There's no point in iterating over all the nonces and checking for empty over and over
                if !self.lend_metadata(current_lend_nonce).is_empty() {
                    let metadata = self.lend_metadata(current_lend_nonce).get();
                    let reward_amount = self.compute_reward_amount(
                        &metadata.amount_in_circulation,
                        metadata.lend_epoch,
                        current_epoch,
                        &reward_percentage_per_epoch,
                    );

                    total_rewards += reward_amount;
                }

                current_lend_nonce += 1;
                if current_lend_nonce > last_lend_nonce {
                    LoopOp::Break
                } else {
                    LoopOp::Continue
                }
            },
            None,
        )?;

        match run_result {
            OperationCompletionStatus::Completed => {
                let prev_unclaimed_rewards = self.unclaimed_rewards().get();
                let extra_unclaimed = &total_rewards - &prev_unclaimed_rewards;

                // TODO: Maybe calculate by how much it's lower?
                // For example, if 1000 is needed, but only 900 is available, that's 10% less
                // So store this "10%" in storage and decrease everyone's rewards by 10% on lenderClaim?
                let stablecoin_reserves = self.stablecoin_reserves().get();
                require!(
                    stablecoin_reserves >= extra_unclaimed,
                    "Total rewards exceed reserves"
                );

                let current_epoch = self.blockchain().get_block_epoch();
                self.last_calculate_rewards_epoch().set(&current_epoch);
                self.unclaimed_rewards().set(&total_rewards);

                let leftover_reserves = stablecoin_reserves - extra_unclaimed;
                self.stablecoin_reserves().set(&leftover_reserves);
            }
            OperationCompletionStatus::InterruptedBeforeOutOfGas => {
                self.save_progress(&OngoingOperationType::CalculateTotalLenderRewards {
                    lend_nonce: current_lend_nonce,
                    total_rewards_be_bytes: total_rewards.to_bytes_be().into(),
                });
            }
        };

        Ok(run_result)
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
    fn dex_proxy(&self, address: Address) -> dex_proxy::Proxy<Self::SendApi>;

    // storage

    #[view(getDelegationScAddress)]
    #[storage_mapper("delegationScAddress")]
    fn delegation_sc_address(&self) -> SingleValueMapper<Self::Storage, Address>;

    #[view(getDexSwapScAddress)]
    #[storage_mapper("dexSwapScAddress")]
    fn dex_swap_sc_address(&self) -> SingleValueMapper<Self::Storage, Address>;

    #[storage_mapper("stakingPosition")]
    fn staking_position(&self, pos_id: u64) -> SingleValueMapper<Self::Storage, StakingPosition>;

    #[storage_mapper("stakingPositionNonceToId")]
    fn staking_position_nonce_to_id(
        &self,
        liquid_staking_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("lastValidStakingPositionId")]
    fn last_valid_staking_position_id(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastStakingRewardsClaimEpoch)]
    #[storage_mapper("lastStakingRewardsClaimEpoch")]
    fn last_staking_rewards_claim_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastStakingTokenConvertEpoch)]
    #[storage_mapper("lastStakingTokenConvertEpoch")]
    fn last_staking_token_convert_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastCalculateRewardsEpoch)]
    #[storage_mapper("lastCalculateRewardsEpoch")]
    fn last_calculate_rewards_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLenderRewardsPercentagePerEpoch)]
    #[storage_mapper("lenderRewardsPercentagePerEpoch")]
    fn lender_rewards_percentage_per_epoch(
        &self,
    ) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getUnclaimedRewards)]
    #[storage_mapper("unclaimedRewards")]
    fn unclaimed_rewards(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;
}
