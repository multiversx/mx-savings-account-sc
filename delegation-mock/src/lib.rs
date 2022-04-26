#![no_std]

elrond_wasm::imports!();

#[elrond_wasm::contract]
pub trait DelegationMock {
    #[init]
    fn init(&self, liquid_staking_token_id: TokenIdentifier) {
        require!(
            liquid_staking_token_id.is_valid_esdt_identifier(),
            "Invalid liquid staking token ID"
        );

        self.liquid_staking_token_id().set(&liquid_staking_token_id);
    }

    #[payable("EGLD")]
    #[endpoint]
    fn stake(&self, #[payment_amount] payment_amount: BigUint) {
        require!(payment_amount > 0, "Must pay more than 0 EGLD");

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let sft_nonce = self.create_liquid_staking_sft(&liquid_staking_token_id, &payment_amount);

        let caller = self.blockchain().get_caller();
        self.send().direct(
            &caller,
            &liquid_staking_token_id,
            sft_nonce,
            &payment_amount,
            &[],
        );
    }

    #[payable("*")]
    #[endpoint(claimRewards)]
    fn claim_rewards(
        &self,
        #[payment_multi] payments: ManagedVec<EsdtTokenPayment<Self::Api>>,
        #[var_args] opt_receive_funds_func: OptionalValue<ManagedBuffer>,
    ) {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();

        let mut new_tokens = ManagedVec::new();
        let mut total_amount = BigUint::zero();
        for payment in &payments {
            require!(
                payment.token_identifier == liquid_staking_token_id,
                "Invalid token"
            );

            self.send().esdt_local_burn(
                &liquid_staking_token_id,
                payment.token_nonce,
                &payment.amount,
            );
            let new_nonce =
                self.create_liquid_staking_sft(&liquid_staking_token_id, &payment.amount);

            total_amount += &payment.amount;
            new_tokens.push(EsdtTokenPayment {
                token_identifier: payment.token_identifier,
                token_nonce: new_nonce,
                amount: payment.amount,
                token_type: EsdtTokenType::SemiFungible,
            })
        }

        let rewards_amount = total_amount / 10u32;
        let caller = self.blockchain().get_caller();

        match opt_receive_funds_func {
            OptionalValue::None => {
                self.send()
                    .direct(&caller, &TokenIdentifier::egld(), 0, &rewards_amount, &[])
            }
            OptionalValue::Some(func_name) => {
                let _ = Self::Api::send_api_impl().direct_egld_execute(
                    &caller,
                    &rewards_amount,
                    self.blockchain().get_gas_left() / 4,
                    &func_name,
                    &ManagedArgBuffer::new_empty(),
                );
            }
        }

        self.send().transfer_multiple_esdt_via_async_call(
            &caller,
            &new_tokens,
            ManagedBuffer::new(),
        );
    }

    fn create_liquid_staking_sft(&self, token_id: &TokenIdentifier, amount: &BigUint) -> u64 {
        self.send().esdt_nft_create(
            token_id,
            amount,
            &ManagedBuffer::new(),
            &BigUint::zero(),
            &ManagedBuffer::new(),
            &(),
            &ManagedVec::new(),
        )
    }

    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<TokenIdentifier>;
}
