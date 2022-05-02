elrond_wasm::imports!();

#[elrond_wasm::module]
pub trait CommonStorageModule {
    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("nrLenders")]
    fn nr_lenders(&self) -> SingleValueMapper<u64>;

    #[view(getLendedAmount)]
    #[storage_mapper("lendedAmount")]
    fn lended_amount(&self) -> SingleValueMapper<BigUint>;

    #[view(getBorowedAmount)]
    #[storage_mapper("borrowedAmount")]
    fn borrowed_amount(&self) -> SingleValueMapper<BigUint>;
}
