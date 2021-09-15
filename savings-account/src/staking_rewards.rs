elrond_wasm::imports!();

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

#[elrond_wasm::module]
pub trait StakingRewardsModule: crate::tokens::TokensModule {
    // endpoints

    // TODO: Ongoing operation pattern
    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) -> SCResult<()> {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.last_staking_rewards_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );

        // let delegation_sc_address = self.delegation_sc_address().get();

        // TODO:
        // Claim staking rewards
        // Async call to delegation SC
        // update Staking position SFT nonces and last_claim_epoch

        Ok(())
    }

    #[endpoint(convertStakingTokenToStablecoin)]
    fn convert_staking_token_to_stablecoin(&self) -> SCResult<AsyncCall<Self::SendApi>> {
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

        Ok(self
            .dex_proxy(dex_sc_address)
            .swap_tokens_fixed_input(
                staking_token_id,
                staking_token_balance,
                stablecoin_token_id,
                Self::BigUint::zero(),
                OptionalArg::Some(b"convert_staking_token_to_stablecoin_callback"[..].into()),
            )
            .async_call())
    }

    // callbacks

    #[callback]
    fn claim_staking_rewards_callback(&self, #[call_result] result: AsyncCallResult<()>) {
        match result {
            AsyncCallResult::Ok(()) => {}
            AsyncCallResult::Err(_) => {}
        }
    }

    // Technically, this is not a callback, but its use is simply updating storage after DEX Swap
    #[payable("*")]
    #[endpoint]
    fn convert_staking_token_to_stablecoin_callback(
        &self,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        let caller = self.blockchain().get_caller();
        let dex_swap_sc_address = self.dex_swap_sc_address().get();
        require!(
            caller == dex_swap_sc_address,
            "Only the DEX Swap SC may call this function"
        );

        let current_epoch = self.blockchain().get_block_epoch();
        self.last_staking_token_convert_epoch().set(&current_epoch);
        self.stablecoin_reserves()
            .update(|stablecoin_reserves| *stablecoin_reserves += payment_amount);

        Ok(())
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

    #[storage_mapper("stakingPositions")]
    fn staking_positions(&self) -> SafeSetMapper<Self::Storage, u64>;

    #[view(getLastStakingRewardsClaimEpoch)]
    #[storage_mapper("lastStakingRewardsClaimEpoch")]
    fn last_staking_rewards_claim_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastStakingTokenConvertEpoch)]
    #[storage_mapper("lastStakingTokenConvertEpoch")]
    fn last_staking_token_convert_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getLastCalculateRewardsEpoch)]
    #[storage_mapper("lastCalculateRewardsEpoch")]
    fn last_calculate_rewards_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[view(getUnclaimedRewards)]
    #[storage_mapper("unclaimedRewards")]
    fn unclaimed_rewards(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;
}
