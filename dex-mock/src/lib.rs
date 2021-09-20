#![no_std]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, TypeAbi, Clone)]
pub struct FftTokenAmountPair<BigUint: BigUintApi> {
    pub token_id: TokenIdentifier,
    pub amount: BigUint,
}

#[elrond_wasm::contract]
pub trait DexMock {
    #[init]
    fn init(&self) {}

    #[payable("*")]
    #[endpoint(swapTokensFixedInput)]
    fn swap_tokens_fixed_input(
        &self,
        #[payment_token] _token_in: TokenIdentifier,
        #[payment_amount] amount_in: Self::BigUint,
        token_out: TokenIdentifier,
        _amount_out_min: Self::BigUint,
        #[var_args] opt_accept_funds_func: OptionalArg<BoxedBytes>,
    ) -> FftTokenAmountPair<Self::BigUint> {
        let caller = self.blockchain().get_caller();
        let amount_out = amount_in * 500u64.into();

        let _ = self.send().direct_esdt_execute(
            &caller,
            &token_out,
            &amount_out,
            self.blockchain().get_gas_left(),
            &opt_accept_funds_func
                .into_option()
                .unwrap_or_else(BoxedBytes::empty)
                .as_slice(),
            &ArgBuffer::new(),
        );

        FftTokenAmountPair {
            token_id: token_out,
            amount: amount_out,
        }
    }
}
