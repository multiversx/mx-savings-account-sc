#![no_std]

use multiversx_sc::codec::Empty;

multiversx_sc::imports!();

#[multiversx_sc::contract]
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
    fn stake(&self) {
        let payment_amount = self.call_value().egld_value().clone_value();
        require!(payment_amount > 0, "Must pay more than 0 EGLD");

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let sft_nonce = self.create_liquid_staking_sft(&liquid_staking_token_id, &payment_amount);

        let caller = self.blockchain().get_caller();
        self.send().direct_esdt(
            &caller,
            &liquid_staking_token_id,
            sft_nonce,
            &payment_amount,
        );
    }

    #[payable("*")]
    #[endpoint(claimRewards)]
    fn claim_rewards(&self) -> MultiValue2<BigUint, ManagedVec<EsdtTokenPayment<Self::Api>>> {
        let payments: ManagedVec<EsdtTokenPayment<Self::Api>> =
            self.call_value().all_esdt_transfers().clone_value();
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
            new_tokens.push(EsdtTokenPayment::new(
                payment.token_identifier,
                new_nonce,
                payment.amount,
            ))
        }

        let rewards_amount = total_amount / 10u32;
        let caller = self.blockchain().get_caller();
        self.send().direct_egld(&caller, &rewards_amount);
        self.send().direct_multi(&caller, &new_tokens);

        (rewards_amount, new_tokens).into()
    }

    fn create_liquid_staking_sft(&self, token_id: &TokenIdentifier, amount: &BigUint) -> u64 {
        self.send().esdt_nft_create(
            token_id,
            amount,
            &ManagedBuffer::new(),
            &BigUint::zero(),
            &ManagedBuffer::new(),
            &Empty,
            &ManagedVec::new(),
        )
    }

    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<TokenIdentifier>;
}
