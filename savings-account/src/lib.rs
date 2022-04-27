#![no_std]

elrond_wasm::imports!();

pub mod common_storage;
mod math;
mod model;
mod ongoing_operation;
mod price_aggregator_proxy;
pub mod staking_rewards;
pub mod tokens;

use model::*;
use price_aggregator_proxy::*;

use crate::staking_rewards::StakingPosition;

static REPAY_INVALID_PAYMENTS_ERR_MSG: &[u8] =
    b"Must send exactly 2 types of tokens: Borrow SFTs and Stablecoins";

#[elrond_wasm::contract]
pub trait SavingsAccount:
    math::MathModule
    + ongoing_operation::OngoingOperationModule
    + price_aggregator_proxy::PriceAggregatorModule
    + staking_rewards::StakingRewardsModule
    + tokens::TokensModule
    + common_storage::CommonStorageModule
{
    #[allow(clippy::too_many_arguments)]
    #[init]
    fn init(
        &self,
        stablecoin_token_id: TokenIdentifier,
        liquid_staking_token_id: TokenIdentifier,
        staked_token_id: TokenIdentifier,
        staked_token_ticker: ManagedBuffer,
        delegation_sc_address: ManagedAddress,
        dex_swap_sc_address: ManagedAddress,
        price_aggregator_address: ManagedAddress,
        loan_to_value_percentage: BigUint,
        lender_rewards_percentage_per_epoch: BigUint,
        base_borrow_rate: BigUint,
        borrow_rate_under_opt_factor: BigUint,
        borrow_rate_over_opt_factor: BigUint,
        optimal_utilisation: BigUint,
    ) {
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
            self.blockchain().is_smart_contract(&delegation_sc_address),
            "Invalid Delegation SC address"
        );
        require!(
            self.blockchain().is_smart_contract(&dex_swap_sc_address),
            "Invalid DEX Swap SC address"
        );
        require!(
            self.blockchain()
                .is_smart_contract(&price_aggregator_address),
            "Invalid Price Aggregator SC address"
        );

        self.stablecoin_token_id().set(&stablecoin_token_id);
        self.liquid_staking_token_id().set(&liquid_staking_token_id);
        self.staked_token_id().set(&staked_token_id);
        self.staked_token_ticker().set(&staked_token_ticker);

        self.delegation_sc_address().set(&delegation_sc_address);
        self.dex_swap_sc_address().set(&dex_swap_sc_address);
        self.price_aggregator_address()
            .set(&price_aggregator_address);

        self.loan_to_value_percentage()
            .set(&loan_to_value_percentage);
        self.lender_rewards_percentage_per_epoch()
            .set(&lender_rewards_percentage_per_epoch);

        let pool_params = PoolParams {
            base_borrow_rate,
            borrow_rate_under_opt_factor,
            borrow_rate_over_opt_factor,
            optimal_utilisation,
        };
        self.pool_params().set(&pool_params);

        let current_epoch = self.blockchain().get_block_epoch();
        self.last_staking_rewards_claim_epoch().set(&current_epoch);

        // init staking position list
        self.staking_position(0).set_if_empty(&StakingPosition {
            liquid_staking_nonce: 0,
            next_pos_id: 0,
            prev_pos_id: 0,
        });
    }

    #[payable("*")]
    #[endpoint]
    fn lend(&self) {
        self.require_no_ongoing_operation();

        self.update_global_lender_rewards();

        let (payment_amount, payment_token) = self.call_value().payment_token_pair();
        let stablecoin_token_id = self.stablecoin_token_id().get();
        require!(
            payment_token == stablecoin_token_id,
            "May only lend stablecoins"
        );

        let caller = self.blockchain().get_caller();
        let new_lend_tokens = self
            .lend_token()
            .nft_create_and_send(&caller, payment_amount, &());

        self.lended_amount()
            .update(|lended_amount| *lended_amount += &new_lend_tokens.amount);
        self.nr_lenders().update(|nr_lenders| *nr_lenders += 1);

        self.lend_metadata(new_lend_tokens.token_nonce)
            .set(&LendMetadata {
                lend_epoch: self.blockchain().get_block_epoch(),
                amount_in_circulation: new_lend_tokens.amount,
            });
    }

    #[payable("*")]
    #[endpoint]
    fn borrow(&self) {
        self.require_no_ongoing_operation();

        let payment: EsdtTokenPayment<Self::Api> = self.call_value().payment();
        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        require!(
            payment.token_identifier == liquid_staking_token_id,
            "May only use liquid staking position as collateral"
        );

        let staked_token_value_in_dollars = self.get_staked_token_value_in_dollars();
        let staking_position_value =
            self.compute_staking_position_value(&staked_token_value_in_dollars, &payment.amount);

        let loan_to_value_percentage = self.loan_to_value_percentage().get();
        let borrow_value =
            self.compute_borrow_amount(&loan_to_value_percentage, &staking_position_value);

        require!(borrow_value > 0, "Deposit amount too low");

        let caller = self.blockchain().get_caller();
        let borrow_tokens =
            self.borrow_token()
                .nft_create_and_send(&caller, payment.amount.clone(), &());
        let staking_pos_id = self.add_staking_position(payment.token_nonce);

        let lended_amount = self.lended_amount().get();
        self.borrowed_amount().update(|total_borrowed| {
            *total_borrowed += &borrow_value;
            require!(
                *total_borrowed <= lended_amount,
                "Not have enough funds to lend"
            );
        });

        self.borrow_metadata(borrow_tokens.token_nonce)
            .set(&BorrowMetadata {
                staking_position_id: staking_pos_id,
                borrow_epoch: self.blockchain().get_block_epoch(),
                staked_token_value_in_dollars_at_borrow: staked_token_value_in_dollars,
                amount_in_circulation: payment.amount,
            });

        self.send_stablecoins(&caller, &borrow_value);
    }

    #[payable("*")]
    #[endpoint]
    fn repay(&self) {
        self.require_no_ongoing_operation();

        let payments = self.call_value().all_esdt_transfers();
        require!(payments.len() == 2, REPAY_INVALID_PAYMENTS_ERR_MSG);

        let first_payment = payments.get(0);
        let second_payment = payments.get(1);

        let stablecoin_token_id = self.stablecoin_token_id().get();
        let borrow_token_id = self.borrow_token().get_token_id();
        require!(
            first_payment.token_identifier == stablecoin_token_id,
            "First transfer must be the Stablecoins"
        );
        require!(
            second_payment.token_identifier == borrow_token_id,
            "Second transfer must be the Borrow Tokens"
        );

        let stablecoin_amount = &first_payment.amount;
        let borrow_token_amount = &second_payment.amount;
        let borrow_token_nonce = second_payment.token_nonce;

        let mut borrow_metadata = self.borrow_metadata(borrow_token_nonce).get();
        self.update_borrow_metadata(
            &mut borrow_metadata,
            borrow_token_nonce,
            borrow_token_amount,
        );

        let borrowed_amount = self.borrowed_amount().get();
        let lended_amount = self.lended_amount().get();
        let current_utilisation =
            self.compute_capital_utilisation(&borrowed_amount, &lended_amount);

        let pool_params = self.pool_params().get();
        let borrow_rate = self.compute_borrow_rate(
            &pool_params.base_borrow_rate,
            &pool_params.borrow_rate_under_opt_factor,
            &pool_params.borrow_rate_over_opt_factor,
            &pool_params.optimal_utilisation,
            &current_utilisation,
        );

        let staked_token_value_in_dollars = self.get_staked_token_value_in_dollars();
        let staking_position_current_value = self
            .compute_staking_position_value(&staked_token_value_in_dollars, borrow_token_amount);

        let debt = self.compute_debt(
            &staking_position_current_value,
            borrow_metadata.borrow_epoch,
            &borrow_rate,
        );
        let total_stablecoins_needed = &staking_position_current_value + &debt;
        require!(
            stablecoin_amount >= &total_stablecoins_needed,
            "Not enough stablecoins paid to cover the debt"
        );

        // even if the value of the staked token changed between borrow and repay time,
        // we still need to map the repaid value to the initial value at borrow time,
        // this is done to keep the borrowed_amount valid
        let borrow_amount_repaid = self.compute_staking_position_value(
            &borrow_metadata.staked_token_value_in_dollars_at_borrow,
            borrow_token_amount,
        );
        self.borrowed_amount()
            .update(|borrowed_amount| *borrowed_amount -= &borrow_amount_repaid);

        // the "debt" and any additional value paid is added to the reserves
        if total_stablecoins_needed > borrow_amount_repaid {
            let extra_reserves = &total_stablecoins_needed - &borrow_amount_repaid;
            self.stablecoin_reserves()
                .update(|stablecoin_reserves| *stablecoin_reserves += extra_reserves);
        }

        self.borrow_token()
            .nft_burn(borrow_token_nonce, borrow_token_amount);

        let caller = self.blockchain().get_caller();
        let extra_stablecoins_paid = stablecoin_amount - &total_stablecoins_needed;
        if extra_stablecoins_paid > 0u32 {
            self.send_stablecoins(&caller, &extra_stablecoins_paid);
        }

        let liquid_staking_token_id = self.liquid_staking_token_id().get();
        let liquid_staking_nonce = self
            .staking_position(borrow_metadata.staking_position_id)
            .get()
            .liquid_staking_nonce;

        let liquid_staking_tokens_for_nonce = self
            .blockchain()
            .get_sc_balance(&liquid_staking_token_id, liquid_staking_nonce);

        // no tokens left after transfer, so we clear the entry
        if &liquid_staking_tokens_for_nonce == borrow_token_amount {
            self.remove_staking_position(borrow_metadata.staking_position_id);
        }

        self.send().direct(
            &caller,
            &liquid_staking_token_id,
            liquid_staking_nonce,
            borrow_token_amount,
            &[],
        );
    }

    #[payable("*")]
    #[endpoint]
    fn withdraw(&self) {
        self.require_no_ongoing_operation();

        self.update_global_lender_rewards();

        let payment: EsdtTokenPayment<Self::Api> = self.call_value().payment();
        let lend_token_mapper = self.lend_token();
        lend_token_mapper.require_same_token(&payment.token_identifier);

        let mut lend_metadata = self.lend_metadata(payment.token_nonce).get();

        let lended_amount = self.lended_amount().get();
        let borrowed_amount = self.borrowed_amount().get();
        let leftover_lend_amount = lended_amount - borrowed_amount;
        require!(
            payment.amount <= leftover_lend_amount,
            "Cannot withdraw, not enough funds"
        );

        self.lended_amount()
            .update(|amount| *amount -= &payment.amount);

        lend_token_mapper.nft_burn(payment.token_nonce, &payment.amount);

        // TODO: Account for lender penalty on rewards
        let rewards_amount =
            self.get_lender_claimable_rewards(payment.token_nonce, &payment.amount);

        self.update_lend_metadata(&mut lend_metadata, payment.token_nonce, &payment.amount);

        let total_withdraw_amount = payment.amount + rewards_amount;
        let caller = self.blockchain().get_caller();
        self.send_stablecoins(&caller, &total_withdraw_amount);
    }

    #[payable("*")]
    #[endpoint(lenderClaimRewards)]
    fn lender_claim_rewards(&self, #[var_args] opt_reject_if_penalty: OptionalValue<bool>) {
        self.require_no_ongoing_operation();

        self.update_global_lender_rewards();

        let payment: EsdtTokenPayment<Self::Api> = self.call_value().payment();
        let lend_token_mapper = self.lend_token();
        lend_token_mapper.require_same_token(&payment.token_identifier);

        let mut lend_metadata = self.lend_metadata(payment.token_nonce).get();
        let last_valid_claim_epoch = self.last_rewards_update_epoch().get();
        require!(
            lend_metadata.lend_epoch < last_valid_claim_epoch,
            "No rewards to claim"
        );

        // burn old sfts
        lend_token_mapper.nft_burn(payment.token_nonce, &payment.amount);

        // create and send new sfts, with updated metadata
        let caller = self.blockchain().get_caller();
        let new_lend_tokens =
            lend_token_mapper.nft_create_and_send(&caller, payment.amount.clone(), &());

        self.lend_metadata(new_lend_tokens.token_nonce)
            .set(&LendMetadata {
                lend_epoch: last_valid_claim_epoch,
                amount_in_circulation: payment.amount.clone(),
            });

        // send rewards
        let mut rewards_amount =
            self.get_lender_claimable_rewards(payment.token_nonce, &payment.amount);
        let penalty_amount = self.penalty_amount_per_lender().get();
        if penalty_amount > 0u32 {
            let reject = match opt_reject_if_penalty {
                OptionalValue::Some(r) => r,
                OptionalValue::None => false,
            };
            require!(!reject, "Rewards have penalty");

            require!(rewards_amount > penalty_amount, "No rewards to claim");
            rewards_amount -= &penalty_amount;

            self.missing_rewards().update(|missing_rewards| {
                // since we round up for penalty, this is possible if everyone claims
                if *missing_rewards < penalty_amount {
                    *missing_rewards = BigUint::zero();
                } else {
                    *missing_rewards -= penalty_amount;
                }
            })
        }

        self.update_lend_metadata(&mut lend_metadata, payment.token_nonce, &payment.amount);

        self.send_stablecoins(&caller, &rewards_amount);
    }

    #[view(getLenderClaimableRewards)]
    fn get_lender_claimable_rewards_view(&self, sft_nonce: u64, sft_amount: BigUint) -> BigUint {
        self.update_global_lender_rewards();

        let rewards = self.get_lender_claimable_rewards(sft_nonce, &sft_amount);
        let penalty = self.penalty_amount_per_lender().get();

        if rewards > penalty {
            rewards - penalty
        } else {
            BigUint::zero()
        }
    }

    fn get_lender_claimable_rewards(&self, sft_nonce: u64, sft_amount: &BigUint) -> BigUint {
        if self.lend_metadata(sft_nonce).is_empty() {
            return BigUint::zero();
        }

        let lend_metadata = self.lend_metadata(sft_nonce).get();
        let last_valid_claim_epoch = self.last_rewards_update_epoch().get();
        let lender_rewards_percentage_per_epoch = self.lender_rewards_percentage_per_epoch().get();

        self.compute_reward_amount(
            sft_amount,
            lend_metadata.lend_epoch,
            last_valid_claim_epoch,
            &lender_rewards_percentage_per_epoch,
        )
    }

    fn get_staked_token_value_in_dollars(&self) -> BigUint {
        let staked_token_ticker = self.staked_token_ticker().get();
        let opt_price = self.get_price_for_pair(staked_token_ticker, DOLLAR_TICKER.into());

        opt_price.unwrap_or_else(|| sc_panic!("Failed to get staked token price"))
    }

    fn get_staking_amount_for_position(&self, sft_nonce: u64) -> BigUint {
        let liquid_staking_token_id = self.liquid_staking_token_id().get();

        self.blockchain()
            .get_sc_balance(&liquid_staking_token_id, sft_nonce)
    }

    fn update_lend_metadata(
        &self,
        lend_metadata: &mut LendMetadata<Self::Api>,
        lend_nonce: u64,
        payment_amount: &BigUint,
    ) {
        lend_metadata.amount_in_circulation -= payment_amount;

        if lend_metadata.amount_in_circulation == 0 {
            self.lend_metadata(lend_nonce).clear();
            self.nr_lenders().update(|nr_lenders| *nr_lenders -= 1);
        } else {
            self.lend_metadata(lend_nonce).set(lend_metadata);
        }
    }

    fn update_borrow_metadata(
        &self,
        borrow_metadata: &mut BorrowMetadata<Self::Api>,
        borrow_nonce: u64,
        payment_amount: &BigUint,
    ) {
        borrow_metadata.amount_in_circulation -= payment_amount;

        if borrow_metadata.amount_in_circulation == 0 {
            self.borrow_metadata(borrow_nonce).clear();
        } else {
            self.borrow_metadata(borrow_nonce).set(borrow_metadata);
        }
    }

    #[storage_mapper("poolParams")]
    fn pool_params(&self) -> SingleValueMapper<PoolParams<Self::Api>>;

    #[view(getLoadToValuePercentage)]
    #[storage_mapper("loadToValuePercentage")]
    fn loan_to_value_percentage(&self) -> SingleValueMapper<BigUint>;
}
