elrond_wasm::imports!();

const BASE_PRECISION: u32 = 1_000_000_000;
const EGLD_PRECISION: u64 = 1_000_000_000_000_000_000;
const SECONDS_IN_YEAR: u32 = 31_556_926;

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

    fn compute_deposit_rate(
        &self,
        u_current: &Self::BigUint,
        borrow_rate: &Self::BigUint,
        reserve_factor: &Self::BigUint,
    ) -> Self::BigUint {
        let bp = Self::BigUint::from(BASE_PRECISION);
        let loan_ratio = u_current * borrow_rate;
        let deposit_rate = u_current * &loan_ratio * (&bp - reserve_factor);

        deposit_rate / (&bp * &bp * bp)
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
        egld_price_in_stablecoin: &Self::BigUint,
        staking_position_value: &Self::BigUint,
    ) -> Self::BigUint {
        (egld_price_in_stablecoin * staking_position_value) / EGLD_PRECISION.into()
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
        borrow_timestamp: u64,
        borrow_rate: &Self::BigUint,
    ) -> Self::BigUint {
        let current_time = self.blockchain().get_block_timestamp();
        let time_diff = current_time - borrow_timestamp;

        let bp = Self::BigUint::from(BASE_PRECISION);
        let secs_year = Self::BigUint::from(SECONDS_IN_YEAR);
        let time_unit_percentage = (&time_diff.into() * &bp) / secs_year;
        let debt_percetange = &(&time_unit_percentage * borrow_rate) / &bp;

        (&debt_percetange * amount) / bp
    }

    fn compute_withdrawal_amount(
        &self,
        amount: &Self::BigUint,
        deposit_timestamp: u64,
        deposit_rate: &Self::BigUint,
    ) -> Self::BigUint {
        let current_time = self.blockchain().get_block_timestamp();
        let time_diff = current_time - deposit_timestamp;

        let bp = Self::BigUint::from(BASE_PRECISION);
        let secs_year = Self::BigUint::from(SECONDS_IN_YEAR);
        let percentage = (&time_diff.into() * deposit_rate) / secs_year;

        amount + &((&percentage * amount) / bp)
    }
}
