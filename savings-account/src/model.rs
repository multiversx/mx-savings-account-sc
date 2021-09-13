elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct PoolParams<BigUint: BigUintApi> {
    pub base_borrow_rate: BigUint,
    pub borrow_rate_under_opt_factor: BigUint,
    pub borrow_rate_over_opt_factor: BigUint,
    pub optimal_utilisation: BigUint,
    pub reserve_factor: BigUint,
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct LendMetadata<BigUint: BigUintApi>
{
    pub lend_epoch: u64,
    pub amount_in_circulation: BigUint,
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct BorrowMetadata<BigUint: BigUintApi>
{
    pub liquid_staking_token_nonce: u64,
    pub borrow_epoch: u64,
    pub amount_in_circulation: BigUint,
}
