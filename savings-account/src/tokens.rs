elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use crate::model::{BorrowMetadata, LendMetadata};

const LEND_TOKEN_TICKER: &[u8] = b"LEND";
const BORROW_TOKEN_TICKER: &[u8] = b"BORROW";
const REQUIRED_ROLES: EsdtLocalRoleFlags = EsdtLocalRoleFlags::from_bits_truncate(
    EsdtLocalRoleFlags::NFT_CREATE.bits()
        | EsdtLocalRoleFlags::NFT_ADD_QUANTITY.bits()
        | EsdtLocalRoleFlags::NFT_BURN.bits(),
);

// only for readability in storage mappers
pub type CurrentLendNonce = u64;
pub type NextLendNonce = u64;

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

    fn create_and_send_lend_tokens(&self, to: &ManagedAddress, amount: &BigUint) -> u64 {
        let lend_token_id = self.lend_token_id().get();
        let lend_nonce = self.create_tokens(&lend_token_id, amount);
        self.insert_lend_nonce(lend_nonce);

        self.send()
            .direct(to, &lend_token_id, lend_nonce, amount, &[]);

        lend_nonce
    }

    fn create_and_send_borrow_tokens(&self, to: &ManagedAddress, amount: &BigUint) -> u64 {
        let borrow_token_id = self.borrow_token_id().get();
        let borrow_nonce = self.create_tokens(&borrow_token_id, amount);

        self.send()
            .direct(to, &borrow_token_id, borrow_nonce, amount, &[]);

        borrow_nonce
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

    fn send_stablecoins(&self, to: &ManagedAddress, amount: &BigUint) {
        let stablecoin_token_id = self.stablecoin_token_id().get();
        self.send().direct(to, &stablecoin_token_id, 0, amount, &[]);
    }

    fn send_liquid_staking_tokens(&self, to: &ManagedAddress, token_nonce: u64, amount: &BigUint) {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        self.send()
            .direct(to, &liquid_staking_token_id, token_nonce, amount, &[]);
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

    fn get_first_lend_nonce(&self) -> u64 {
        self.lend_nonces_list(0u64).get()
    }

    fn insert_lend_nonce(&self, new_lend_nonce: u64) {
        let last_valid_lend_nonce = self.last_valid_lend_nonce().get();
        self.lend_nonces_list(last_valid_lend_nonce)
            .set(&new_lend_nonce);

        self.last_valid_lend_nonce().set(&new_lend_nonce);
    }

    fn remove_lend_nonce(&self, prev_lend_nonce: u64, lend_nonce_to_remove: u64) {
        let last_valid_nonce = self.last_valid_lend_nonce().get();
        if lend_nonce_to_remove == last_valid_nonce {
            self.last_valid_lend_nonce().set(&prev_lend_nonce);
        }

        // connect prev to next in the list
        let next_lend_nonce = self.lend_nonces_list(lend_nonce_to_remove).get();
        self.lend_nonces_list(prev_lend_nonce).set(&next_lend_nonce);

        self.lend_nonces_list(lend_nonce_to_remove).clear();
    }

    // callbacks

    #[callback]
    fn issue_callback(
        &self,
        token_ticker: ManagedBuffer,
        #[call_result] result: ManagedAsyncCallResult<TokenIdentifier>,
    ) {
        match result {
            ManagedAsyncCallResult::Ok(token_id) => {
                if token_ticker == ManagedBuffer::new_from_bytes(LEND_TOKEN_TICKER) {
                    self.lend_token_id().set(&token_id);
                } else if token_ticker == ManagedBuffer::new_from_bytes(BORROW_TOKEN_TICKER) {
                    self.borrow_token_id().set(&token_id);
                } else {
                    self.issue_callback_refund();
                }
            }
            ManagedAsyncCallResult::Err(_) => {
                self.issue_callback_refund();
            }
        }
    }

    fn issue_callback_refund(&self) {
        let caller = self.blockchain().get_owner_address();
        let (returned_tokens, token_id) = self.call_value().payment_token_pair();

        if token_id.is_egld() && returned_tokens > 0 {
            self.send()
                .direct(&caller, &token_id, 0, &returned_tokens, &[]);
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

    #[storage_mapper("lendMetadata")]
    fn lend_metadata(&self, sft_nonce: u64) -> SingleValueMapper<LendMetadata<Self::Api>>;

    // Each item stores the next nonce. First item is stored at index 0, and the last item has index 0 as next
    // Example: 0 -> 1, 1 -> 3, 3 -> 4, 4 -> 0 (list containing nonces 1, 3, 4)
    #[storage_mapper("lendNoncesList")]
    fn lend_nonces_list(&self, lend_nonce: CurrentLendNonce) -> SingleValueMapper<NextLendNonce>;

    #[storage_mapper("lastValidLendNonce")]
    fn last_valid_lend_nonce(&self) -> SingleValueMapper<CurrentLendNonce>;

    #[view(getBorrowTokenId)]
    #[storage_mapper("borrowTokenId")]
    fn borrow_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[storage_mapper("borrowMetadata")]
    fn borrow_metadata(&self, sft_nonce: u64) -> SingleValueMapper<BorrowMetadata<Self::Api>>;
}
