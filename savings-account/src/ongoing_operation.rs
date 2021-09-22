elrond_wasm::imports!();
elrond_wasm::derive_imports!();

const MIN_GAS_TO_SAVE_PROGRESS: u64 = 100_000_000;
pub const ANOTHER_ONGOING_OP_ERR_MSG: &[u8] = b"Another ongoing operation is in progress";
pub const CALLBACK_IN_PROGRESS_ERR_MSG: &[u8] = b"Callback not executed yet";

#[derive(TopDecode, TopEncode, TypeAbi, PartialEq)]
pub enum OngoingOperationType {
    None,
    CalculateTotalLenderRewards {
        lend_nonce: u64,
    },
    ClaimStakingRewards {
        pos_id: u64,
        callback_executed: bool,
    },
    ConvertStakingTokenToStablecoin,
}

pub enum LoopOp {
    Continue,
    Save(OngoingOperationType),
    Break,
}

impl LoopOp {
    fn is_break(&self) -> bool {
        return matches!(self, LoopOp::Break);
    }
}

#[elrond_wasm::module]
pub trait OngoingOperationModule {
    fn run_while_it_has_gas<Process>(
        &self,
        mut process: Process,
        opt_additional_gas_reserve_per_iteration: Option<u64>,
    ) -> SCResult<OperationCompletionStatus>
    where
        Process: FnMut() -> LoopOp,
    {
        let gas_before = self.blockchain().get_gas_left();

        let mut loop_op = process();

        let gas_after = self.blockchain().get_gas_left();
        let gas_per_iteration = gas_before - gas_after;

        let additional_gas_reserve_per_iteration =
            opt_additional_gas_reserve_per_iteration.unwrap_or_default();
        let mut total_reserve_needed = additional_gas_reserve_per_iteration;

        loop {
            if loop_op.is_break() {
                break;
            }

            total_reserve_needed += additional_gas_reserve_per_iteration;
            if !self.can_continue_operation(gas_per_iteration, total_reserve_needed) {
                return Ok(OperationCompletionStatus::InterruptedBeforeOutOfGas);
            }

            loop_op = process();
        }

        self.clear_operation();

        Ok(OperationCompletionStatus::Completed)
    }

    fn can_continue_operation(&self, operation_cost: u64, extra_reserve_needed: u64) -> bool {
        let gas_left = self.blockchain().get_gas_left();

        gas_left > MIN_GAS_TO_SAVE_PROGRESS + extra_reserve_needed + operation_cost
    }

    fn load_operation(&self) -> OngoingOperationType {
        self.current_ongoing_operation().get()
    }

    fn save_progress(&self, operation: &OngoingOperationType) {
        self.current_ongoing_operation().set(operation);
    }

    fn clear_operation(&self) {
        self.current_ongoing_operation().clear();
    }

    fn require_no_ongoing_operation(&self) -> SCResult<()> {
        require!(
            self.current_ongoing_operation().get() == OngoingOperationType::None,
            "Ongoing operation in progress"
        );
        Ok(())
    }

    #[storage_mapper("currentOngoingOperation")]
    fn current_ongoing_operation(&self) -> SingleValueMapper<Self::Storage, OngoingOperationType>;
}
