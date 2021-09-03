elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, TypeAbi)]
pub struct PoolParams<BigUint: BigUintApi> {
    pub base_borrow_rate: BigUint,
    pub borrow_rate_under_opt_factor: BigUint,
    pub borrow_rate_over_opt_factor: BigUint,
    pub optimal_utilisation: BigUint,
    pub reserve_factor: BigUint,
}
