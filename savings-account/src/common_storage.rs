elrond_wasm::imports!();

#[elrond_wasm::module]
pub trait CommonStorageModule {
    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<BigUint>;

    #[view(getLentAmount)]
    #[storage_mapper("lentAmount")]
    fn lent_amount(&self) -> SingleValueMapper<BigUint>;

    #[view(getBorowedAmount)]
    #[storage_mapper("borrowedAmount")]
    fn borrowed_amount(&self) -> SingleValueMapper<BigUint>;
}
