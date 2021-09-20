#![no_std]

use savings_account::multi_transfer::EsdtTokenPayment;

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
    fn stake(&self) {}

    #[payable("*")]
    #[endpoint(claimRewards)]
    fn claim_rewards(&self) -> SCResult<()> {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let transfers = self.get_all_esdt_transfers();
        let mut new_tokens = Vec::new();
        for transfer in transfers {
            require!(
                transfer.token_name == liquid_staking_token_id,
                "Invalid token"
            );

            let new_nonce = self.send().esdt_nft_create(
                &liquid_staking_token_id,
                &transfer.amount,
                &BoxedBytes::empty(),
                &Self::BigUint::zero(),
                &BoxedBytes::empty(),
                &(),
                &[BoxedBytes::empty()],
            );
            new_tokens.push(EsdtTokenPayment {
                token_name: transfer.token_name,
                token_nonce: new_nonce,
                amount: transfer.amount,
                token_type: EsdtTokenType::SemiFungible,
            })
        }

        let caller = self.blockchain().get_caller();
        self.multi_transfer_via_async_call(
            &caller,
            &new_tokens,
            &BoxedBytes::empty(),
            &[],
            &BoxedBytes::empty(),
            &[],
        );
    }

    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;
}
