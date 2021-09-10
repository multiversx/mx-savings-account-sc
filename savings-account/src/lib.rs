#![no_std]

elrond_wasm::imports!();

mod math;
mod model;
mod multi_transfer;
mod tokens;

use model::*;
use price_aggregator_proxy::*;

#[elrond_wasm::contract]
pub trait SavingsAccount:
    math::MathModule
    + multi_transfer::MultiTransferModule
    + price_aggregator_proxy::PriceAggregatorModule
    + tokens::TokensModule
{
    #[allow(clippy::too_many_arguments)]
    #[init]
    fn init(
        &self,
        stablecoin_token_id: TokenIdentifier,
        liquid_staking_token_id: TokenIdentifier,
        staked_token_id: TokenIdentifier,
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
            staked_token_id.is_egld() || staked_token_id.is_valid_esdt_identifier(),
            "Invalid staked token ID"
        );
        require!(
            self.blockchain()
                .is_smart_contract(&price_aggregator_address),
            "Invalid price aggregator address"
        );

        self.stablecoin_token_id().set(&stablecoin_token_id);
        self.liquid_staking_token_id().set(&liquid_staking_token_id);
        self.staked_token_id().set(&staked_token_id);
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

        let current_epoch = self.blockchain().get_block_epoch();
        self.staking_rewards_last_claim_epoch().set(&current_epoch);

        Ok(())
    }

    // endpoints

    #[payable("*")]
    #[endpoint]
    fn lend(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only lend stablecoins"
        );

        let lend_token_id = self.lend_token_id().get();
        let sft_nonce = self.create_tokens(&lend_token_id, &payment_amount)?;

        self.lend_metadata(sft_nonce).set(&LendMetadata {
            last_claim_epoch: self.blockchain().get_block_epoch(),
            amount_in_circulation: payment_amount.clone(),
        });

        self.lended_amount()
            .update(|lended_amount| *lended_amount += &payment_amount);

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
        let stablecoin_reserves = self.stablecoin_reserves().get();
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

        let staked_token_value_in_dollars = self.get_staked_token_value_in_dollars()?;
        let staking_position_value =
            self.compute_staking_position_value(&staked_token_value_in_dollars, &payment_amount);
        let borrow_value = self.compute_borrow_amount(&borrow_rate, &staking_position_value);

        require!(borrow_value > 0, "Deposit amount too low");

        let sc_stablecoin_balance = self.blockchain().get_sc_balance(&stablecoin_token_id, 0);
        require!(
            borrow_value <= sc_stablecoin_balance,
            "Not have enough funds to lend"
        );

        let borrow_token_id = self.borrow_token_id().get();
        let borrow_token_nonce = self.create_tokens(&borrow_token_id, &payment_amount)?;

        self.borrow_metadata(borrow_token_nonce)
            .set(&BorrowMetadata {
                amount_in_circulation: payment_amount.clone(),
                liquid_staking_token_nonce: payment_nonce,
                borrow_epoch: self.blockchain().get_block_epoch(),
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
        self.update_borrow_metadata(
            &mut borrow_metadata,
            borrow_token_nonce,
            &borrow_token_amount,
        );

        let borrowed_amount = self.borrowed_amount().get();
        let stablecoin_reserves = self.stablecoin_reserves().get();
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

        let staked_token_value_in_dollars = self.get_staked_token_value_in_dollars()?;
        let staking_position_value = self
            .compute_staking_position_value(&staked_token_value_in_dollars, borrow_token_amount);

        let debt = self.compute_debt(
            &staking_position_value,
            borrow_metadata.borrow_epoch,
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
        self.update_lend_metadata(&mut lend_metadata, payment_nonce, &payment_amount);

        let borrowed_amount = self.borrowed_amount().get();
        let stablecoin_reserves = self.stablecoin_reserves().get();
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

        /* TODO: Calculate withdraw_amount + rewards
        let withdraw_amount =
            self.compute_withdrawal_amount(&payment_amount, lend_metadata.last_claim_epoch, &deposit_rate);
        */
        let withdraw_amount = Self::BigUint::zero();
        let sc_stablecoin_balance = self.blockchain().get_sc_balance(&stablecoin_token_id, 0);

        if withdraw_amount > sc_stablecoin_balance {
            /* TODO:
                Try convert EGLD to X worth of stablecoins (where X =  withdraw_amount - sc_balance)
                (EGLD gained through claiming staking rewards)
                Through DEX contracts?
                Throw an error if even that wouldn't be enough to cover for the costs

                ???
            */
        }

        self.burn_tokens(&lend_token_id, payment_nonce, &payment_amount)?;

        let caller = self.blockchain().get_caller();
        self.send()
            .direct(&caller, &stablecoin_token_id, 0, &withdraw_amount, &[]);

        Ok(())
    }

    // TODO: Rename? Not to be confused with the other claim function
    #[endpoint(claimStakingRewards)]
    fn claim_staking_rewards(&self) -> SCResult<OperationCompletionStatus> {
        let current_epoch = self.blockchain().get_block_epoch();
        let last_claim_epoch = self.staking_rewards_last_claim_epoch().get();
        require!(
            current_epoch > last_claim_epoch,
            "Already claimed this epoch"
        );
        // TODO:
        // Claim staking rewards
        // iterate over staking_positions mapper:
        // pop_front, push_back (if balance == 0 for that nonce, remove the entry)

        // Async call to delegation SC
        // update Staking position SFT nonces and reserve amounts in callback

        Ok(OperationCompletionStatus::Completed)
    }

    #[payable("*")]
    #[endpoint(lenderClaimRewards)]
    fn lender_claim_rewards(
        &self,
        #[payment_token] payment_token: TokenIdentifier,
        #[payment_nonce] payment_nonce: u64,
        #[payment_amount] payment_amount: Self::BigUint,
    ) -> SCResult<()> {
        self.require_lend_token_issued()?;

        let lend_token_id = self.lend_token_id().get();
        require!(
            payment_token == lend_token_id,
            "May only pay with lend tokens"
        );

        let mut lend_metadata = self.lend_metadata(payment_nonce).get();
        self.update_lend_metadata(&mut lend_metadata, payment_nonce, &payment_amount);

        // diff between current epoch and last time claimStakingRewards was called
        let lender_rewards_percentage_per_epoch = self.lender_rewards_percentage_per_epoch().get();
        let reward_amount = self.compute_reward_amount(
            &payment_amount,
            lend_metadata.last_claim_epoch,
            &lender_rewards_percentage_per_epoch,
        );

        // TODO: claim staking rewards, convert to stablecoins and send rewards

        // burn old sfts
        self.burn_tokens(&lend_token_id, payment_nonce, &payment_amount)?;

        // create sfts with updated timestamps
        let current_epoch = self.blockchain().get_block_epoch();
        let sft_nonce = self.create_tokens(&lend_token_id, &payment_amount)?;
        self.lend_metadata(sft_nonce).set(&LendMetadata {
            last_claim_epoch: current_epoch,
            amount_in_circulation: payment_amount.clone(),
        });

        let caller = self.blockchain().get_caller();
        self.send()
            .direct(&caller, &lend_token_id, sft_nonce, &payment_amount, &[]);

        Ok(())
    }

    // private

    fn get_staked_token_value_in_dollars(&self) -> SCResult<Self::BigUint> {
        let staked_token_id = self.staked_token_id().get();
        let staked_token_ticker = self.get_token_ticker(&staked_token_id);
        let opt_price = self.get_price_for_pair(staked_token_ticker, DOLLAR_TICKER.into());

        opt_price.ok_or("Failed to get staked token price").into()
    }

    fn get_staking_amount_for_position(&self, sft_nonce: u64) -> Self::BigUint {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();

        self.blockchain()
            .get_sc_balance(&liquid_staking_token_id, sft_nonce)
    }

    fn update_lend_metadata(
        &self,
        lend_metadata: &mut LendMetadata<Self::BigUint>,
        lend_nonce: u64,
        payment_amount: &Self::BigUint,
    ) {
        lend_metadata.amount_in_circulation -= payment_amount;

        if lend_metadata.amount_in_circulation == 0 {
            self.lend_metadata(lend_nonce).clear();
        } else {
            self.lend_metadata(lend_nonce).set(&lend_metadata);
        }
    }

    fn update_borrow_metadata(
        &self,
        borrow_metadata: &mut BorrowMetadata<Self::BigUint>,
        borrow_nonce: u64,
        payment_amount: &Self::BigUint,
    ) {
        borrow_metadata.amount_in_circulation -= payment_amount;

        if borrow_metadata.amount_in_circulation == 0 {
            self.borrow_metadata(borrow_nonce).clear();
        } else {
            self.borrow_metadata(borrow_nonce).set(&borrow_metadata);
        }
    }

    fn calculate_total_rewards(&self) -> Self::BigUint {
        // TODO: Use something like a SetMapper or a custom mapper that will hold valid nonces
        // There's no point in iterating over all the nonces and checking for empty over and over
        let last_lend_nonce = self.blockchain().get_current_esdt_nft_nonce(
            &self.blockchain().get_sc_address(),
            &self.lend_token_id().get(),
        );
        let reward_percentage_per_epoch = self.lender_rewards_percentage_per_epoch().get();

        let mut total_rewards = Self::BigUint::zero();
        for i in 1..=last_lend_nonce {
            if self.lend_metadata(i).is_empty() {
                continue;
            }

            let metadata = self.lend_metadata(i).get();
            let reward_amount = self.compute_reward_amount(
                &metadata.amount_in_circulation,
                metadata.last_claim_epoch,
                &reward_percentage_per_epoch,
            );

            total_rewards += reward_amount;
        }

        // TODO: Maybe store this value in storage? Something like "totalClaimableRewards"

        total_rewards
    }

    // storage

    #[storage_mapper("poolParams")]
    fn pool_params(&self) -> SingleValueMapper<Self::Storage, PoolParams<Self::BigUint>>;

    #[view(getLenderRewardsPercentagePerEpoch)]
    #[storage_mapper("lenderRewardsPercentagePerEpoch")]
    fn lender_rewards_percentage_per_epoch(
        &self,
    ) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getLoadToValuePercentage)]
    #[storage_mapper("loadToValuePercentage")]
    fn loan_to_value_percentage(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[view(getLendedAmount)]
    #[storage_mapper("lendedAmount")]
    fn lended_amount(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

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

    #[view(getStakingRewardsLastClaimEpoch)]
    #[storage_mapper("stakingRewardsLastClaimEpoch")]
    fn staking_rewards_last_claim_epoch(&self) -> SingleValueMapper<Self::Storage, u64>;

    // TODO: Update after claimStakingRewards and converting staking token to stablecoins
    #[view(getStablecoinReserves)]
    #[storage_mapper("stablecoinReserves")]
    fn stablecoin_reserves(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    // TODO:
    // ----------- Ongoing operation logic -----------------------
    //
}
