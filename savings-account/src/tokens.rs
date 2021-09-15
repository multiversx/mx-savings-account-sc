elrond_wasm::imports!();

const TICKER_SEPARATOR: u8 = b'-';
const LEND_TOKEN_TICKER: &[u8] = b"LEND";
const BORROW_TOKEN_TICKER: &[u8] = b"BORROW";
const REQUIRED_LOCAL_ROLES: &[EsdtLocalRole] = &[
    EsdtLocalRole::NftCreate,
    EsdtLocalRole::NftAddQuantity,
    EsdtLocalRole::NftBurn,
];

#[elrond_wasm::module]
pub trait TokensModule {
    // endpoints - owner-only

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueLendToken)]
    fn issue_lend_token(
        &self,
        #[payment_amount] payment_amount: Self::BigUint,
        token_name: BoxedBytes,
    ) -> AsyncCall<Self::SendApi> {
        self.issue_token(payment_amount, token_name, LEND_TOKEN_TICKER.into())
    }

    #[only_owner]
    #[payable("EGLD")]
    #[endpoint(issueBorrowToken)]
    fn issue_borrow_token(
        &self,
        #[payment_amount] payment_amount: Self::BigUint,
        token_name: BoxedBytes,
    ) -> AsyncCall<Self::SendApi> {
        self.issue_token(payment_amount, token_name, BORROW_TOKEN_TICKER.into())
    }

    #[only_owner]
    #[endpoint(setLendTokenRoles)]
    fn set_lend_token_roles(&self) -> SCResult<AsyncCall<Self::SendApi>> {
        self.require_lend_token_issued()?;

        let lend_token_id = self.lend_token_id().get();
        Ok(self.set_roles(lend_token_id))
    }

    #[only_owner]
    #[endpoint(setBorrowTokenRoles)]
    fn set_borrow_token_roles(&self) -> SCResult<AsyncCall<Self::SendApi>> {
        self.require_borrow_token_issued()?;

        let borrow_token_id = self.borrow_token_id().get();
        Ok(self.set_roles(borrow_token_id))
    }

    // private

    fn issue_token(
        &self,
        issue_cost: Self::BigUint,
        token_name: BoxedBytes,
        token_ticker: BoxedBytes,
    ) -> AsyncCall<Self::SendApi> {
        ESDTSystemSmartContractProxy::new_proxy_obj(self.send())
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
            .with_callback(self.callbacks().issue_callback())
    }

    fn set_roles(&self, token_id: TokenIdentifier) -> AsyncCall<Self::SendApi> {
        ESDTSystemSmartContractProxy::new_proxy_obj(self.send())
            .set_special_roles(
                &self.blockchain().get_sc_address(),
                &token_id,
                REQUIRED_LOCAL_ROLES,
            )
            .async_call()
    }

    fn get_token_ticker(&self, token_id: &TokenIdentifier) -> BoxedBytes {
        for (i, char) in token_id.as_esdt_identifier().iter().enumerate() {
            if *char == TICKER_SEPARATOR {
                return token_id.as_esdt_identifier()[..i].into();
            }
        }

        token_id.as_name().into()
    }

    fn create_tokens(&self, token_id: &TokenIdentifier, amount: &Self::BigUint) -> SCResult<u64> {
        self.require_local_roles_set(token_id)?;

        let sft_nonce = self.send().esdt_nft_create(
            token_id,
            amount,
            &BoxedBytes::empty(),
            &Self::BigUint::zero(),
            &BoxedBytes::empty(),
            &(),
            &[],
        );

        Ok(sft_nonce)
    }

    fn burn_tokens(
        &self,
        token_id: &TokenIdentifier,
        nonce: u64,
        amount: &Self::BigUint,
    ) -> SCResult<()> {
        self.require_local_roles_set(token_id)?;

        self.send().esdt_local_burn(token_id, nonce, amount);

        Ok(())
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

    fn require_local_roles_set(&self, token_id: &TokenIdentifier) -> SCResult<()> {
        let roles = self.blockchain().get_esdt_local_roles(token_id);
        for required_role in REQUIRED_LOCAL_ROLES {
            if !roles.contains(required_role) {
                return sc_error!("ESDT local roles not set");
            }
        }

        Ok(())
    }

    // callbacks

    #[callback]
    fn issue_callback(
        &self,
        #[call_result] result: AsyncCallResult<TokenIdentifier>,
    ) -> OptionalResult<AsyncCall<Self::SendApi>> {
        match result {
            AsyncCallResult::Ok(token_id) => {
                let ticker = self.get_token_ticker(&token_id);
                match ticker.as_slice() {
                    LEND_TOKEN_TICKER => {
                        self.lend_token_id().set(&token_id);
                    }
                    BORROW_TOKEN_TICKER => {
                        self.borrow_token_id().set(&token_id);
                    }
                    _ => return OptionalResult::None,
                }

                OptionalResult::Some(self.set_roles(token_id))
            }
            AsyncCallResult::Err(_) => {
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
    fn stablecoin_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[view(getLiquidStakingTokenId)]
    #[storage_mapper("liquidStakingTokenId")]
    fn liquid_staking_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[view(getStakedTokenId)]
    #[storage_mapper("stakedTokenId")]
    fn staked_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[view(getLendTokenId)]
    #[storage_mapper("lendTokenId")]
    fn lend_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    #[view(getBorrowTokenId)]
    #[storage_mapper("borrowTokenId")]
    fn borrow_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;
}
