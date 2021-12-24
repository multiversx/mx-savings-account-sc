elrond_wasm::imports!();

use crate::model::{BorrowMetadata, LendMetadata};

const LEND_TOKEN_TICKER: &[u8] = b"LEND";
const BORROW_TOKEN_TICKER: &[u8] = b"BORROW";
const REQUIRED_ROLES: EsdtLocalRoleFlags = EsdtLocalRoleFlags::from_bits_truncate(
    EsdtLocalRoleFlags::NFT_CREATE.bits()
        | EsdtLocalRoleFlags::NFT_ADD_QUANTITY.bits()
        | EsdtLocalRoleFlags::NFT_BURN.bits(),
);

#[elrond_wasm::module]
pub trait TokensModule {
    // endpoints - owner-only

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueLendToken)]
    fn issue_lend_token(
        &self,
        #[payment_amount] payment_amount: BigUint,
        token_name: ManagedBuffer,
    ) -> SCResult<AsyncCall> {
        require!(self.lend_token_id().is_empty(), "Lend token already issued");
        Ok(self.issue_token(payment_amount, token_name, LEND_TOKEN_TICKER.into()))
    }

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueBorrowToken)]
    fn issue_borrow_token(
        &self,
        #[payment_amount] payment_amount: BigUint,
        token_name: ManagedBuffer,
    ) -> SCResult<AsyncCall> {
        require!(
            self.borrow_token_id().is_empty(),
            "Borrow token already issued"
        );
        Ok(self.issue_token(payment_amount, token_name, BORROW_TOKEN_TICKER.into()))
    }

    #[only_owner]
    #[endpoint(setLendTokenRoles)]
    fn set_lend_token_roles(&self) -> SCResult<AsyncCall> {
        self.require_lend_token_issued()?;

        let lend_token_id = self.lend_token_id().get();
        Ok(self.set_roles(lend_token_id))
    }

    #[only_owner]
    #[endpoint(setBorrowTokenRoles)]
    fn set_borrow_token_roles(&self) -> SCResult<AsyncCall> {
        self.require_borrow_token_issued()?;

        let borrow_token_id = self.borrow_token_id().get();
        Ok(self.set_roles(borrow_token_id))
    }

    // private

    fn issue_token(
        &self,
        issue_cost: BigUint,
        token_name: ManagedBuffer,
        token_ticker: ManagedBuffer,
    ) -> AsyncCall {
        ESDTSystemSmartContractProxy::new_proxy_obj(self.raw_vm_api())
            .issue_semi_fungible(
                issue_cost,
                &token_name,
                &token_ticker,
                SemiFungibleTokenProperties {
                    can_freeze: true,
                    can_wipe: true,
                    can_pause: true,
                    can_change_owner: false,
                    can_upgrade: false,
                    can_add_special_roles: true,
                },
            )
            .async_call()
            .with_callback(self.callbacks().issue_callback(token_ticker))
    }

    fn set_roles(&self, token_id: TokenIdentifier) -> AsyncCall {
        ESDTSystemSmartContractProxy::new_proxy_obj(self.raw_vm_api())
            .set_special_roles(
                &self.blockchain().get_sc_address(),
                &token_id,
                REQUIRED_ROLES.iter_roles().cloned(),
            )
            .async_call()
    }

    fn create_tokens(&self, token_id: &TokenIdentifier, amount: &BigUint) -> u64 {
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

    #[inline]
    fn burn_tokens(&self, token_id: &TokenIdentifier, nonce: u64, amount: &BigUint) {
        self.send().esdt_local_burn(token_id, nonce, amount);
    }

    fn require_lend_token_issued(&self) -> SCResult<()> {
        require!(!self.lend_token_id().is_empty(), "Lend token not issued");
        Ok(())
    }

    fn require_borrow_token_issued(&self) -> SCResult<()> {
        require!(
            !self.borrow_token_id().is_empty(),
            "Borrow token not issued"
        );
        Ok(())
    }

    // callbacks

    #[callback]
    fn issue_callback(
        &self,
        token_ticker: ManagedBuffer,
        #[call_result] result: ManagedAsyncCallResult<TokenIdentifier>,
    ) -> OptionalResult<AsyncCall> {
        match result {
            ManagedAsyncCallResult::Ok(token_id) => {
                if token_ticker == ManagedBuffer::new_from_bytes(LEND_TOKEN_TICKER) {
                    self.lend_token_id().set(&token_id);
                } else if token_ticker == ManagedBuffer::new_from_bytes(BORROW_TOKEN_TICKER) {
                    self.borrow_token_id().set(&token_id);
                } else {
                    return OptionalResult::None;
                }

                OptionalResult::Some(self.set_roles(token_id))
            }
            ManagedAsyncCallResult::Err(_) => {
                let caller = self.blockchain().get_owner_address();
                let (returned_tokens, token_id) = self.call_value().payment_token_pair();
                if token_id.is_egld() && returned_tokens > 0 {
                    self.send()
                        .direct(&caller, &token_id, 0, &returned_tokens, &[]);
                }

                OptionalResult::None
            }
        }
    }

    // storage

    #[view(getStablecoinTokenId)]
    #[storage_mapper("stablecoinTokenId")]
    fn stablecoin_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getLiquidStakingTokenId)]
    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getStakedTokenId)]
    #[storage_mapper("stakedTokenId")]
    fn staked_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[storage_mapper("stakedTokenTicker")]
    fn staked_token_ticker(&self) -> SingleValueMapper<ManagedBuffer>;

    #[view(getLendTokenId)]
    #[storage_mapper("lendTokenId")]
    fn lend_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getBorrowTokenId)]
    #[storage_mapper("borrowTokenId")]
    fn borrow_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[storage_mapper("lendMetadata")]
    fn lend_metadata(&self, sft_nonce: u64) -> SingleValueMapper<LendMetadata<Self::Api>>;

    #[storage_mapper("borrowMetadata")]
    fn borrow_metadata(&self, sft_nonce: u64) -> SingleValueMapper<BorrowMetadata<Self::Api>>;
}
