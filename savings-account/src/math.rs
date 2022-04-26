elrond_wasm::imports!();

const BASE_PRECISION: u32 = 1_000_000_000; // Could be reduced maybe? Since we're working with epochs instead of seconds
const DEFAULT_DECIMALS: u64 = 1_000_000_000_000_000_000; // most tokens have 10^18 decimals. TODO: Add as configurable value
const EPOCHS_IN_YEAR: u32 = 365;

#[elrond_wasm::module]
pub trait MathModule {
    fn compute_borrow_rate(
        &self,
        r_base: &BigUint,
        r_slope1: &BigUint,
        r_slope2: &BigUint,
        u_optimal: &BigUint,
        u_current: &BigUint,
    ) -> BigUint {
        if u_current < u_optimal {
            let utilisation_ratio = &(u_current * r_slope1) / u_optimal;

            r_base + &utilisation_ratio
        } else {
            let bp = BigUint::from(BASE_PRECISION);
            let denominator = &bp - u_optimal;
            let numerator = &(u_current - u_optimal) * r_slope2;

            (r_base + r_slope1) + numerator / denominator
        }
    }

    fn compute_capital_utilisation(
        &self,
        borrowed_amount: &BigUint,
        total_pool_reserves: &BigUint,
    ) -> BigUint {
        &(borrowed_amount * BASE_PRECISION) / total_pool_reserves
    }

    fn compute_staking_position_value(
        &self,
        staked_token_value_in_dollars: &BigUint,
        staked_amount: &BigUint,
    ) -> BigUint {
        (staked_token_value_in_dollars * staked_amount) / DEFAULT_DECIMALS
    }

    fn compute_borrow_amount(&self, borrow_rate: &BigUint, deposit_value: &BigUint) -> BigUint {
        borrow_rate * deposit_value / BASE_PRECISION
    }

    fn compute_debt(&self, amount: &BigUint, borrow_epoch: u64, borrow_rate: &BigUint) -> BigUint {
        let current_epoch = self.blockchain().get_block_epoch();
        let epoch_diff = current_epoch - borrow_epoch;

        let bp = BigUint::from(BASE_PRECISION);
        let time_unit_percentage = (&epoch_diff.into() * &bp) / EPOCHS_IN_YEAR;
        let debt_percetange = &(&time_unit_percentage * borrow_rate) / &bp;

        (&debt_percetange * amount) / bp
    }

    fn compute_reward_amount(
        &self,
        amount: &BigUint,
        lend_epoch: u64,
        last_calculate_rewards_epoch: u64,
        reward_percentage_per_epoch: &BigUint,
    ) -> BigUint {
        if lend_epoch >= last_calculate_rewards_epoch {
            return BigUint::zero();
        }

        let epoch_diff = last_calculate_rewards_epoch - lend_epoch;
        let percentage = &epoch_diff.into() * reward_percentage_per_epoch;

        (&percentage * amount) / BASE_PRECISION
    }
}
