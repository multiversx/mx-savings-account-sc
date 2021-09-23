#![no_std]

use savings_account::multi_transfer::{EsdtTokenPayment, MultiTransferAsync};

elrond_wasm::imports!();

#[elrond_wasm::contract]
pub trait DelegationMock: savings_account::multi_transfer::MultiTransferModule {
    #[init]
    fn init(&self, liquid_staking_token_id: TokenIdentifier) -> SCResult<()> {
        require!(
            liquid_staking_token_id.is_valid_esdt_identifier(),
            "Invalid liquid staking token ID"
        );

        self.liquid_staking_token_id().set(&liquid_staking_token_id);

        Ok(())
    }

    #[payable("EGLD")]
    #[endpoint]
    fn stake(&self, #[payment_amount] payment_amount: Self::BigUint) -> SCResult<()> {
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

        Ok(())
    }

    #[payable("*")]
    #[endpoint(claimRewards)]
    fn claim_rewards(
        &self,
        #[var_args] opt_receive_funds_func: OptionalArg<BoxedBytes>,
    ) -> SCResult<
        MultiResult2<Vec<EsdtTokenPayment<Self::BigUint>>, MultiTransferAsync<Self::SendApi>>,
    > {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let transfers = self.get_all_esdt_transfers();

        let mut new_tokens = Vec::new();
        let mut total_amount = Self::BigUint::zero();
        for transfer in transfers {
            require!(
                transfer.token_name == liquid_staking_token_id,
                "Invalid token"
            );

            self.send().esdt_local_burn(
                &liquid_staking_token_id,
                transfer.token_nonce,
                &transfer.amount,
            );
            let new_nonce =
                self.create_liquid_staking_sft(&liquid_staking_token_id, &transfer.amount);

            total_amount += &transfer.amount;
            new_tokens.push(EsdtTokenPayment {
                token_name: transfer.token_name,
                token_nonce: new_nonce,
                amount: transfer.amount,
                token_type: EsdtTokenType::SemiFungible,
            })
        }

        let rewards_amount = total_amount / 10u64.into();
        let caller = self.blockchain().get_caller();

        match opt_receive_funds_func {
            OptionalArg::None => {
                self.send()
                    .direct(&caller, &TokenIdentifier::egld(), 0, &rewards_amount, &[])
            }
            OptionalArg::Some(func_name) => self.send().direct_egld_execute(
                &caller,
                &rewards_amount,
                self.blockchain().get_gas_left() / 4,
                func_name.as_slice(),
                &ArgBuffer::new(),
            )?,
        }

        let async_call = MultiTransferAsync::new(self.send(), caller, &[], new_tokens.clone());
        Ok((new_tokens, async_call).into())
    }

    fn create_liquid_staking_sft(&self, token_id: &TokenIdentifier, amount: &Self::BigUint) -> u64 {
        self.send().esdt_nft_create(
            token_id,
            amount,
            &BoxedBytes::empty(),
            &Self::BigUint::zero(),
            &BoxedBytes::empty(),
            &(),
            &[BoxedBytes::empty()],
        )
    }

    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;
}
