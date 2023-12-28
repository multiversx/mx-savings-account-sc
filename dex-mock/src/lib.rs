#![no_std]


multiversx_sc::imports!();
multiversx_sc::derive_imports!();

const EGLD_DECIMALS: u64 = 1_000_000_000_000_000_000;

#[multiversx_sc::contract]
pub trait DexMock {
    #[init]
    fn init(&self) {}

    #[payable("*")]
    #[endpoint]
    fn deposit(&self) {}

    #[payable("*")]
    #[endpoint(swapTokensFixedInput)]
    fn swap_tokens_fixed_input(
        &self,
        #[payment_token] _token_in: EgldOrEsdtTokenIdentifier,
        #[payment_amount] amount_in: BigUint,
        token_out: EgldOrEsdtTokenIdentifier,
        _amount_out_min: BigUint,
        opt_accept_funds_func: OptionalValue<ManagedBuffer>,
    ) -> EgldOrEsdtTokenPayment<Self::Api> {
        let caller = self.blockchain().get_caller();
        let amount_out = amount_in * 100u64 / EGLD_DECIMALS;
        let func = match opt_accept_funds_func {
            OptionalValue::Some(f) => f,
            OptionalValue::None => ManagedBuffer::default(),
        };

        let _ = self.send().direct_with_gas_limit(
            &caller,
            &token_out.clone(),
            0,
            &amount_out,
            self.blockchain().get_gas_left(),
            func,
            &[],
        );

        EgldOrEsdtTokenPayment::new(token_out, 0, amount_out)
    }
}
