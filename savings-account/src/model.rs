elrond_wasm::imports!();
elrond_wasm::derive_imports!();

pub type LendResultType<M> = EsdtTokenPayment<M>;
pub type BorrowResultType<M> = MultiValue2<EsdtTokenPayment<M>, EsdtTokenPayment<M>>;
pub type RepayResultType<M> = MultiValue2<EsdtTokenPayment<M>, EsdtTokenPayment<M>>;
pub type WithdrawResultType<M> = EsdtTokenPayment<M>;
pub type ClaimRewardsResultType<M> = MultiValue2<EsdtTokenPayment<M>, EsdtTokenPayment<M>>;

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct PoolParams<M: ManagedTypeApi> {
    pub base_borrow_rate: BigUint<M>,
    pub borrow_rate_under_opt_factor: BigUint<M>,
    pub borrow_rate_over_opt_factor: BigUint<M>,
    pub optimal_utilisation: BigUint<M>,
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct LendMetadata {
    pub lend_epoch: u64,
}

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct BorrowMetadata<M: ManagedTypeApi> {
    pub staking_position_id: u64,
    pub borrow_epoch: u64,
    pub staked_token_value_in_dollars_at_borrow: BigUint<M>,
}
