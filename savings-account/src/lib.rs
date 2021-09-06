#![no_std]

elrond_wasm::imports!();

mod math;
mod model;
mod multi_transfer;
mod price_aggregator;
mod tokens;

use model::*;

#[elrond_wasm::contract]
pub trait SavingsAccount:
    math::MathModule
    + multi_transfer::MultiTransferModule
    + price_aggregator::PriceAggregatorModule
    + tokens::TokensModule
{
    #[allow(clippy::too_many_arguments)]
    #[init]
    fn init(
        &self,
        stablecoin_token_id: TokenIdentifier,
        liquid_staking_token_id: TokenIdentifier,
        price_aggregator_address: Address,
        base_borrow_rate: Self::BigUint,
        borrow_rate_under_opt_factor: Self::BigUint,
        borrow_rate_over_opt_factor: Self::BigUint,
        optimal_utilisation: Self::BigUint,
        reserve_factor: Self::BigUint,
    ) -> SCResult<()> {
        require!(
            stablecoin_token_id.is_valid_esdt_identifier(),
            "Invalid stablecoin token ID"
        );
        require!(
            liquid_staking_token_id.is_valid_esdt_identifier(),
            "Invalid liquid staking token ID"
        );
        require!(
            self.blockchain()
                .is_smart_contract(&price_aggregator_address),
            "Invalid price aggregator address"
        );

        self.stablecoin_token_id().set(&stablecoin_token_id);
        self.liquid_staking_token_id().set(&liquid_staking_token_id);
        self.price_aggregator_address()
            .set(&price_aggregator_address);

        let pool_params = PoolParams {
            base_borrow_rate,
            borrow_rate_under_opt_factor,
            borrow_rate_over_opt_factor,
            optimal_utilisation,
            reserve_factor,
        };
        self.pool_params().set(&pool_params);

        Ok(())
    }

    // endpoints

    #[payable("*")]
    #[endpoint]
    fn deposit(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only deposit stablecoins"
        );

        let lend_token_id = self.lend_token_id().get();
        let sft_nonce = self.create_tokens(&lend_token_id, &payment_amount, &())?;

        self.lend_metadata(sft_nonce).set(&LendMetadata {
            timestamp: self.blockchain().get_block_timestamp(),
            amount_in_circulation: payment_amount.clone(),
        });

        let caller = self.blockchain().get_caller();
        self.send()
            .direct(&caller, &lend_token_id, sft_nonce, &payment_amount, &[]);

        Ok(())
    }

    #[payable("*")]
    #[endpoint]
    fn borrow(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_nonce] payment_nonce: u64,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_borrow_token_issued()?;

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        require!(
            payment_token == liquid_staking_token_id,
            "May only use liquid staking position as collateral"
        );

        let stablecoin_token_id = self.stablecoin_token_id().get();
        let stablecoin_reserves = self.get_reserves(&stablecoin_token_id);
        let borrowed_amount = self.borrowed_amount().get();

        let pool_params = self.pool_params().get();
        let current_utilisation =
            self.compute_capital_utilisation(&borrowed_amount, &stablecoin_reserves);
        let borrow_rate = self.compute_borrow_rate(
            &pool_params.base_borrow_rate,
            &pool_params.borrow_rate_under_opt_factor,
            &pool_params.borrow_rate_over_opt_factor,
            &pool_params.optimal_utilisation,
            &current_utilisation,
        );

        let egld_price_in_stablecoin = self.get_egld_price_in_stablecoin()?;
        let staking_position_value =
            self.compute_staking_position_value(&egld_price_in_stablecoin, &payment_amount);
        let borrow_value = self.compute_borrow_amount(&borrow_rate, &staking_position_value);

        require!(borrow_value > 0, "Deposit amount too low");

        let sc_stablecoin_balance = self.blockchain().get_sc_balance(&stablecoin_token_id, 0);
        require!(
            borrow_value <= sc_stablecoin_balance,
            "Not have enough funds to lend"
        );

        let borrow_token_id = self.borrow_token_id().get();
        let borrow_token_nonce = self.create_tokens(&borrow_token_id, &payment_amount, &())?;

        self.borrow_metadata(borrow_token_nonce)
            .set(&BorrowMetadata {
                amount_in_circulation: payment_amount.clone(),
                liquid_staking_token_nonce: payment_nonce,
                timestamp: self.blockchain().get_block_timestamp(),
            });
        self.borrowed_amount()
            .update(|total| *total += &borrow_value);

        self.staking_positions().push_back(payment_nonce);

        let caller = self.blockchain().get_caller();
        self.send().direct(
            &caller,
            &borrow_token_id,
            borrow_token_nonce,
            &payment_amount,
            &[],
        );
        self.send()
            .direct(&caller, &stablecoin_token_id, 0, &borrow_value, &[]);

        Ok(())
    }

    #[payable("*")]
    #[endpoint]
    fn repay(&self) -> SCResult<()> {
        self.require_borrow_token_issued()?;

        let transfers = self.get_all_esdt_transfers();
        require!(
            transfers.len() == 2,
            "Must send exactly 2 types of tokens: Borrow SFTs and Stablecoins"
        );

        let stablecoin_token_id = self.stablecoin_token_id().get();
        let borrow_token_id = self.borrow_token_id().get();
        require!(
            transfers[0].token_name == stablecoin_token_id,
            "First transfer must be the Stablecoins"
        );
        require!(
            transfers[1].token_name == borrow_token_id,
            "Second transfer token must be the Borrow Tokens"
        );

        let stablecoin_amount = &transfers[0].amount;
        let borrow_token_amount = &transfers[1].amount;
        let borrow_token_nonce = transfers[1].token_nonce;

        let mut borrow_metadata = self.borrow_metadata(borrow_token_nonce).get();
        borrow_metadata.amount_in_circulation -= borrow_token_amount;

        if borrow_metadata.amount_in_circulation == 0 {
            self.borrow_metadata(borrow_token_nonce).clear();
        } else {
            self.borrow_metadata(borrow_token_nonce)
                .set(&borrow_metadata);
        }

        let borrowed_amount = self.borrowed_amount().get();
        let stablecoin_reserves = self.get_reserves(&stablecoin_token_id);
        let current_utilisation =
            self.compute_capital_utilisation(&borrowed_amount, &stablecoin_reserves);

        let pool_params = self.pool_params().get();
        let borrow_rate = self.compute_borrow_rate(
            &pool_params.base_borrow_rate,
            &pool_params.borrow_rate_under_opt_factor,
            &pool_params.borrow_rate_over_opt_factor,
            &pool_params.optimal_utilisation,
            &current_utilisation,
        );

        let egld_price_in_stablecoins = self.get_egld_price_in_stablecoin()?;
        let staking_position_value =
            self.compute_staking_position_value(&egld_price_in_stablecoins, borrow_token_amount);

        let debt = self.compute_debt(
            &staking_position_value,
            borrow_metadata.timestamp,
            &borrow_rate,
        );
        let total_stablecoins_needed = staking_position_value + debt;
        require!(
            stablecoin_amount >= &total_stablecoins_needed,
            "Not enough stablecoins paid to cover the debt"
        );

        self.borrowed_amount()
            .update(|borrowed_amount| *borrowed_amount -= &total_stablecoins_needed);

        self.burn_tokens(&borrow_token_id, borrow_token_nonce, borrow_token_amount)?;

        let caller = self.blockchain().get_caller();
        let extra_stablecoins_paid = stablecoin_amount - &total_stablecoins_needed;
        if extra_stablecoins_paid > 0 {
            self.send().direct(
                &caller,
                &stablecoin_token_id,
                0,
                &extra_stablecoins_paid,
                &[],
            );
        }

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        self.send().direct(
            &caller,
            &liquid_staking_token_id,
            borrow_metadata.liquid_staking_token_nonce,
            borrow_token_amount,
            &[],
        );

        Ok(())
    }

    #[payable("*")]
    #[endpoint]
    fn withdraw(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_nonce] payment_nonce: u64,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let stablecoin_token_id = self.stablecoin_token_id().get();
        let lend_token_id = self.lend_token_id().get();
        require!(
            payment_token == lend_token_id,
            "May only pay with lend tokens"
        );

        let mut lend_metadata = self.lend_metadata(payment_nonce).get();
        lend_metadata.amount_in_circulation -= &payment_amount;

        if lend_metadata.amount_in_circulation == 0 {
            self.lend_metadata(payment_nonce).clear();
        } else {
            self.lend_metadata(payment_nonce).set(&lend_metadata);
        }

        let borrowed_amount = self.borrowed_amount().get();
        let stablecoin_reserves = self.get_reserves(&stablecoin_token_id);
        let current_utilisation =
            self.compute_capital_utilisation(&borrowed_amount, &stablecoin_reserves);

        let pool_params = self.pool_params().get();
        let borrow_rate = self.compute_borrow_rate(
            &pool_params.base_borrow_rate,
            &pool_params.borrow_rate_under_opt_factor,
            &pool_params.borrow_rate_over_opt_factor,
            &pool_params.optimal_utilisation,
            &current_utilisation,
        );
        let deposit_rate = self.compute_deposit_rate(
            &current_utilisation,
            &borrow_rate,
            &pool_params.reserve_factor,
        );

        let withdraw_amount =
            self.compute_withdrawal_amount(&payment_amount, lend_metadata.timestamp, &deposit_rate);
        let sc_stablecoin_balance = self.blockchain().get_sc_balance(&stablecoin_token_id, 0);

        if withdraw_amount > sc_stablecoin_balance {
            /* TODO:
                Try convert EGLD to X worth of stablecoins (where X =  withdraw_amount - sc_balance)
                (EGLD gained through claiming staking rewards)
                Through DEX contracts?
                Throw an error if even that wouldn't be enough to cover for the costs
            */
        }

        self.burn_tokens(&lend_token_id, payment_nonce, &payment_amount)?;

        let caller = self.blockchain().get_caller();
        self.send()
            .direct(&caller, &stablecoin_token_id, 0, &withdraw_amount, &[]);

        Ok(())
    }

    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) -> SCResult<OperationCompletionStatus> {
        // TODO:
        // Claim staking rewards
        // iterate over staking_positions mapper:
        // pop_front, push_back (if balance == 0 for that nonce, remove the entry)

        Ok(OperationCompletionStatus::Completed)
    }

    // private

    fn get_egld_price_in_stablecoin(&self) -> SCResult<Self::BigUint> {
        let stablecoin_token_id = self.stablecoin_token_id().get();
        let opt_price = self.get_price_for_pair(TokenIdentifier::egld(), stablecoin_token_id);

        opt_price.ok_or("Failed to get EGLD price").into()
    }

    fn get_reserves(&self, token_id: &TokenIdentifier) -> Self::BigUint {
        self.blockchain().get_sc_balance(token_id, 0)
    }

    fn get_staking_amount_for_position(&self, sft_nonce: u64) -> Self::BigUint {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();

        self.blockchain()
            .get_sc_balance(&liquid_staking_token_id, sft_nonce)
    }

    // storage

    #[storage_mapper("poolParams")]
    fn pool_params(&self) -> SingleValueMapper<Self::Storage, PoolParams<Self::BigUint>>;

    #[view(getBorowedAmount)]
    #[storage_mapper("borrowedAmount")]
    fn borrowed_amount(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("lendMetadata")]
    fn lend_metadata(
        &self,
        sft_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, LendMetadata<Self::BigUint>>;

    #[storage_mapper("borrowMetadata")]
    fn borrow_metadata(
        &self,
        sft_nonce: u64,
    ) -> SingleValueMapper<Self::Storage, BorrowMetadata<Self::BigUint>>;

    // SFT nonces for the available staking positions
    // Using LinkedListMapper to be able iterate and claim staking rewards,
    // while also being able to split in multiple SC calls
    #[storage_mapper("stakingPositions")]
    fn staking_positions(&self) -> LinkedListMapper<Self::Storage, u64>;
}
