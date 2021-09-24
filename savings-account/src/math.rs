elrond_wasm::imports!();

const BASE_PRECISION: u32 = 1_000_000_000; // Could be reduced maybe? Since we're working with epochs instead of seconds
const DEFAULT_DECIMALS: u64 = 1_000_000_000_000_000_000; // most tokens have 10^18 decimals. TODO: Add as configurable value
const EPOCHS_IN_YEAR: u32 = 355;

#[elrond_wasm::module]
pub trait MathModule {
    fn compute_borrow_rate(
        &self,
        r_base: &Self::BigUint,
        r_slope1: &Self::BigUint,
        r_slope2: &Self::BigUint,
        u_optimal: &Self::BigUint,
        u_current: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);

        if u_current < u_optimal {
            let utilisation_ratio = &(u_current * r_slope1) / u_optimal;

            r_base + &utilisation_ratio
        } else {
            let denominator = &bp - u_optimal;
            let numerator = &(u_current - u_optimal) * r_slope2;

            (r_base + r_slope1) + numerator / denominator
        }
    }

    fn compute_capital_utilisation(
        &self,
        borrowed_amount: &Self::BigUint,
        total_pool_reserves: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        &(borrowed_amount * &bp) / total_pool_reserves
    }

    fn compute_staking_position_value(
        &self,
        staked_token_value_in_dollars: &Self::BigUint,
        staked_amount: &Self::BigUint,
    ) -> Self::BigUint {
        (staked_token_value_in_dollars * staked_amount) / DEFAULT_DECIMALS.into()
    }

    fn compute_borrow_amount(
        &self,
        borrow_rate: &Self::BigUint,
        deposit_value: &Self::BigUint,
    ) -> Self::BigUint {
        borrow_rate * deposit_value / BASE_PRECISION.into()
    }

    fn compute_debt(
        &self,
        amount: &Self::BigUint,
        borrow_epoch: u64,
        borrow_rate: &Self::BigUint,
    ) -> Self::BigUint {
        let current_epoch = self.blockchain().get_block_epoch();
        let epoch_diff = current_epoch - borrow_epoch;

        let bp = Self::BigUint::from(BASE_PRECISION);
        let epochs_year = Self::BigUint::from(EPOCHS_IN_YEAR);
        let time_unit_percentage = (&epoch_diff.into() * &bp) / epochs_year;
        let debt_percetange = &(&time_unit_percentage * borrow_rate) / &bp;

        (&debt_percetange * amount) / bp
    }

    fn compute_reward_amount(
        &self,
        amount: &Self::BigUint,
        lend_epoch: u64,
        last_calculate_rewards_epoch: u64,
        reward_percentage_per_epoch: &Self::BigUint,
    ) -> Self::BigUint {
        if lend_epoch >= last_calculate_rewards_epoch {
            return Self::BigUint::zero();
        }

        let epoch_diff = last_calculate_rewards_epoch - lend_epoch;
        let bp = Self::BigUint::from(BASE_PRECISION);
        let percentage = &epoch_diff.into() * reward_percentage_per_epoch;

        (&percentage * amount) / bp
    }
}
