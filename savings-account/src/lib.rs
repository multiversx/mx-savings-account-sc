#![no_std]

elrond_wasm::imports!();

mod borrow;
mod pool_params;
mod price_aggregator;

use pool_params::*;

#[elrond_wasm::contract]
pub trait SavingsAccount: borrow::BorrowModule + price_aggregator::PriceAggregatorModule {
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
}
