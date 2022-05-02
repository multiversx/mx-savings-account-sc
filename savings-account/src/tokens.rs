elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use crate::model::{BorrowMetadata, LendMetadata};

const LEND_TOKEN_TICKER: &[u8] = b"LEND";
const BORROW_TOKEN_TICKER: &[u8] = b"BORROW";
static TOKEN_ALREADY_ISSUED_ERR_MSG: &[u8] = b"Token already issued";

// only for readability in storage mappers
pub type CurrentLendNonce = u64;
pub type NextLendNonce = u64;

#[elrond_wasm::module]
pub trait TokensModule {
    // endpoints - owner-only

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueLendToken)]
    fn issue_lend_token(&self, token_name: ManagedBuffer, num_decimals: usize) {
        require!(self.lend_token().is_empty(), TOKEN_ALREADY_ISSUED_ERR_MSG);

        let payment_amount = self.call_value().egld_value();
        self.issue_token(
            payment_amount,
            token_name,
            LEND_TOKEN_TICKER.into(),
            num_decimals,
        )
        .call_and_exit();
    }

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueBorrowToken)]
    fn issue_borrow_token(&self, token_name: ManagedBuffer, num_decimals: usize) {
        require!(self.borrow_token().is_empty(), TOKEN_ALREADY_ISSUED_ERR_MSG);

        let payment_amount = self.call_value().egld_value();
        self.issue_token(
            payment_amount,
            token_name,
            BORROW_TOKEN_TICKER.into(),
            num_decimals,
        )
        .call_and_exit();
    }

    // private

    fn issue_token(
        &self,
        issue_cost: BigUint,
        token_name: ManagedBuffer,
        token_ticker: ManagedBuffer,
        num_decimals: usize,
    ) -> AsyncCall {
        ESDTSystemSmartContractProxy::new_proxy_obj()
            .issue_and_set_all_roles(
                issue_cost,
                token_name,
                token_ticker.clone(),
                EsdtTokenType::Meta,
                num_decimals,
            )
            .async_call()
            .with_callback(self.callbacks().issue_callback(token_ticker))
    }

    fn send_stablecoins(&self, to: &ManagedAddress, amount: &BigUint) {
        let stablecoin_token_id = self.stablecoin_token_id().get();
        self.send().direct(to, &stablecoin_token_id, 0, amount, &[]);
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
                    self.lend_token().set_token_id(&token_id);
                } else if token_ticker == ManagedBuffer::new_from_bytes(BORROW_TOKEN_TICKER) {
                    self.borrow_token().set_token_id(&token_id);
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
    fn lend_token(&self) -> NonFungibleTokenMapper<Self::Api>;

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
    fn borrow_token(&self) -> NonFungibleTokenMapper<Self::Api>;

    #[storage_mapper("borrowMetadata")]
    fn borrow_metadata(&self, sft_nonce: u64) -> SingleValueMapper<BorrowMetadata<Self::Api>>;
}
