#![no_std]

elrond_wasm::imports!();

mod math;
mod model;
mod price_aggregator;
mod tokens;

use model::*;

#[elrond_wasm::contract]
pub trait SavingsAccount:
    math::MathModule + price_aggregator::PriceAggregatorModule + tokens::TokensModule
{
    #[allow(clippy::too_many_arguments)]
    #[init]
    fn init(
        &self,
        stablecoin_token_id: TokenIdentifier,
        liquid_staking_token_id: TokenIdentifier,
        price_aggregator_address: Address,
        base_borrow_rate: Self::BigUint,
        borrow_rate_under_opt_factor: Self::BigUint,
        borrow_rate_over_opt_factor: Self::BigUint,
        optimal_utilisation: Self::BigUint,
        reserve_factor: Self::BigUint,
    ) -> SCResult<()> {
        require!(
            stablecoin_token_id.is_valid_esdt_identifier(),
            "Invalid stablecoin token ID"
        );
        require!(
            liquid_staking_token_id.is_valid_esdt_identifier(),
            "Invalid liquid staking token ID"
        );
        require!(
            self.blockchain()
                .is_smart_contract(&price_aggregator_address),
            "Invalid price aggregator address"
        );

        self.stablecoin_token_id().set(&stablecoin_token_id);
        self.liquid_staking_token_id().set(&liquid_staking_token_id);
        self.price_aggregator_address()
            .set(&price_aggregator_address);

        let pool_params = PoolParams {
            base_borrow_rate,
            borrow_rate_under_opt_factor,
            borrow_rate_over_opt_factor,
            optimal_utilisation,
            reserve_factor,
        };
        self.pool_params().set(&pool_params);

        Ok(())
    }

    // endpoints

    #[payable("*")]
    #[endpoint]
    fn deposit(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only deposit stablecoins"
        );

        let caller = self.blockchain().get_caller();
        let current_timestamp = self.blockchain().get_block_timestamp();

        let lend_token_id = self.lend_token_id().get();
        let sft_nonce = self.create_tokens(&lend_token_id, &payment_amount, &())?;

        self.reserves(&stablecoin_token_id)
            .update(|reserves| *reserves += &payment_amount);
        self.lend_metadata(sft_nonce).set(&LendMetadata {
            timestamp: current_timestamp,
            amount_in_circulation: payment_amount.clone(),
        });

        self.send()
            .direct(&caller, &lend_token_id, sft_nonce, &payment_amount, &[]);

        Ok(())
    }

    #[payable("*")]
    #[endpoint]
    fn borrow(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_nonce] payment_nonce: u64,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_borrow_token_issued()?;

        let caller = self.blockchain().get_caller();
        require!(
            !self.blockchain().is_smart_contract(&caller),
            "Caller may not be a smart contract"
        );

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        require!(
            payment_token == liquid_staking_token_id,
            "May only use liquid staking position as collateral"
        );

        let stablecoin_token_id = self.stablecoin_token_id().get();
        let stablecoin_reserves = self.reserves(&stablecoin_token_id).get();
        let borrowed_amount = self.borrowed_amount().get();

        let pool_params = self.pool_params().get();
        let current_utilisation =
            self.compute_capital_utilisation(&borrowed_amount, &stablecoin_reserves);
        let borrow_rate = self.compute_borrow_rate(
            &pool_params.base_borrow_rate,
            &pool_params.borrow_rate_under_opt_factor,
            &pool_params.borrow_rate_over_opt_factor,
            &pool_params.optimal_utilisation,
            &current_utilisation,
        );

        let egld_price_in_stablecoin = self.get_egld_price_in_stablecoin()?;
        let staking_position_value =
            self.compute_staking_position_value(&egld_price_in_stablecoin, &payment_amount);
        let borrow_value = self.compute_borrow_amount(&borrow_rate, &staking_position_value);

        require!(borrow_value > 0, "Deposit amount too low");

        let sc_stablecoin_balance = self.blockchain().get_sc_balance(&stablecoin_token_id, 0);
        require!(
            borrow_value <= sc_stablecoin_balance,
            "Not have enough funds to lend"
        );

        let borrow_token_id = self.borrow_token_id().get();
        let borrow_token_nonce = self.create_tokens(&borrow_token_id, &payment_amount, &())?;

        self.borrow_metadata(borrow_token_nonce)
            .set(&BorrowMetadata {
                amount_in_circulation: payment_amount.clone(),
                liquid_staking_token_nonce: payment_nonce,
                timestamp: self.blockchain().get_block_timestamp(),
            });

        self.send().direct(
            &caller,
            &borrow_token_id,
            borrow_token_nonce,
            &payment_amount,
            &[],
        );
        self.send()
            .direct(&caller, &stablecoin_token_id, 0, &borrow_value, &[]);

        Ok(())
    }

    // private

    fn get_egld_price_in_stablecoin(&self) -> SCResult<Self::BigUint> {
        let stablecoin_token_id = self.stablecoin_token_id().get();
        let opt_price = self.get_price_for_pair(TokenIdentifier::egld(), stablecoin_token_id);

        opt_price.ok_or("Failed to get EGLD price").into()
    }

    // storage

    #[view(getStablecoinTokenId)]
    #[storage_mapper("stablecoinTokenId")]
    fn stablecoin_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[view(getLiquidStakingTokenId)]
    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[storage_mapper("poolParams")]
    fn pool_params(&self) -> SingleValueMapper<Self::Storage, PoolParams<Self::BigUint>>;

    #[view(getBorowedAmount)]
    #[storage_mapper("borrowedAmount")]
    fn borrowed_amount(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getReserves)]
    #[storage_mapper("reserves")]
    fn reserves(
        &self,
        token_id: &TokenIdentifier,
    ) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("lendMetadata")]
    fn lend_metadata(
        &self,
        sft_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, LendMetadata<Self::BigUint>>;

    #[storage_mapper("borrowMetadata")]
    fn borrow_metadata(
        &self,
        sft_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, BorrowMetadata<Self::BigUint>>;
}
