#![no_std]

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

const EGLD_DECIMALS: u64 = 1_000_000_000_000_000_000;

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, TypeAbi, Clone)]
pub struct FftTokenAmountPair<M: ManagedTypeApi> {
    pub token_id: TokenIdentifier<M>,
    pub amount: BigUint<M>,
}

#[elrond_wasm::contract]
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
        #[payment_token] _token_in: TokenIdentifier,
        #[payment_amount] amount_in: BigUint,
        token_out: TokenIdentifier,
        _amount_out_min: BigUint,
        #[var_args] opt_accept_funds_func: OptionalArg<ManagedBuffer>,
    ) -> FftTokenAmountPair<Self::Api> {
        let caller = self.blockchain().get_caller();
        let amount_out = amount_in * 100u64 / EGLD_DECIMALS;
        let func = match opt_accept_funds_func {
            OptionalArg::Some(f) => f,
            OptionalArg::None => ManagedBuffer::default(),
        };

        let _ = self.raw_vm_api().direct_esdt_execute(
            &caller,
            &token_out,
            &amount_out,
            self.blockchain().get_gas_left(),
            &func,
            &ManagedArgBuffer::new_empty(),
        );

        FftTokenAmountPair {
            token_id: token_out,
            amount: amount_out,
        }
    }
}
