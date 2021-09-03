#![no_std]

elrond_wasm::imports!();

mod math;
mod pool_params;
mod price_aggregator;
mod tokens;

use pool_params::*;

#[elrond_wasm::contract]
pub trait SavingsAccount:
    math::MathModule + price_aggregator::PriceAggregatorModule + tokens::TokensModule
{
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
        #[payment_amount] _payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let lend_token_id = self.lend_token_id().get();
        self.require_local_roles_set(&lend_token_id)?;

        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only deposit stablecoins"
        );

        // let current_timestamp = self.blockchain().get_block_timestamp();

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

    #[view(getPoolReserves)]
    #[storage_mapper("poolReserves")]
    fn pool_reserves(
        &self,
        token_id: &TokenIdentifier,
    ) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("lendTimestamp")]
    fn lend_timestamp(&self, sft_nonce: u64) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("borrowTimestamp")]
    fn borrow_timestamp(&self, sft_nonce: u64) -> SingleValueMapper<Self::Storage, u64>;
}
