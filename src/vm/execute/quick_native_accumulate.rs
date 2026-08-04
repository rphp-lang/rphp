// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
// Native regions are non-blocking and bounded to at most 48 operations per
// iteration. A 21-run matrix across scalar, call, conditional, straight and
// application-shaped regions reaches its performance plateau here while the
// slowest measured interval between VM interrupt checks remains below 9 us.
const NATIVE_LONG_SAFEPOINT_INTERVAL: u64 = 1024;

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_INDUCTION_SLOT: u16 = 0;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_BOUND_SLOT: u16 = 1;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_ACCUMULATOR_SLOT: u16 = 2;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_FIRST_DYNAMIC_SLOT: u16 = 3;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_TERM_SLOT: u16 = 61;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_SUM_RESULT_SLOT: u16 = 62;
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_CALL_POST_RESULT_SLOT: u16 = 63;

#[derive(Clone, Copy)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
struct NativeQuickLongCallAccumulateKernel {
    config: NativeStraightLongLoopConfig,
    targets: [*const FunctionCommon; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    target_identities: [usize; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    target_count: u8,
    sum_operation_index: u8,
    trace_guard_operation_index: Option<u8>,
    call_resume_ip: usize,
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
struct NativeQuickLongCallTreeBuilder<'a> {
    op_array: &'a crate::compiler::OpArray,
    frame: *mut ExecuteData,
    slot_base: *mut Value,
    induction_cv: u16,
    accumulator_cv: u16,
    operations: [NativeStraightLongOperation; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    operation_count: usize,
    slots: [i64; 64],
    caller_cv_slots: [u16; 64],
    next_dynamic_slot: u16,
    targets: [*const FunctionCommon; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    target_identities: [usize; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    target_count: usize,
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
impl<'a> NativeQuickLongCallTreeBuilder<'a> {
    unsafe fn new(
        op_array: &'a crate::compiler::OpArray,
        frame: *mut ExecuteData,
        induction_cv: u16,
        accumulator_cv: u16,
        induction: i64,
        accumulator: i64,
        bound: i64,
    ) -> Self {
        let mut slots = [0i64; 64];
        slots[NATIVE_CALL_INDUCTION_SLOT as usize] = induction;
        slots[NATIVE_CALL_BOUND_SLOT as usize] = bound;
        slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize] = accumulator;
        Self {
            op_array,
            frame,
            slot_base: (frame as *mut Value).add(CALL_FRAME_SLOTS),
            induction_cv,
            accumulator_cv,
            operations: [
                NativeStraightLongOperation::Unused;
                NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
            ],
            operation_count: 0,
            slots,
            caller_cv_slots: [u16::MAX; 64],
            next_dynamic_slot: NATIVE_CALL_FIRST_DYNAMIC_SLOT,
            targets: [std::ptr::null(); NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
            target_identities: [0; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
            target_count: 0,
        }
    }

    fn allocate_dynamic_slot(&mut self) -> Option<u16> {
        if self.next_dynamic_slot >= NATIVE_CALL_TERM_SLOT {
            return None;
        }
        let slot = self.next_dynamic_slot;
        self.next_dynamic_slot += 1;
        Some(slot)
    }

    unsafe fn caller_operand(
        &mut self,
        op_type: OpType,
        operand: u16,
    ) -> Option<QuickLongOperand> {
        match op_type {
            OpType::Cv if operand == self.induction_cv => {
                Some(QuickLongOperand::Slot(NATIVE_CALL_INDUCTION_SLOT))
            }
            OpType::Cv if operand == self.accumulator_cv => {
                Some(QuickLongOperand::Slot(NATIVE_CALL_ACCUMULATOR_SLOT))
            }
            OpType::Cv => {
                let index = operand as usize;
                if index >= self.caller_cv_slots.len() {
                    return None;
                }
                let cached_slot = self.caller_cv_slots[index];
                if cached_slot != u16::MAX {
                    return Some(QuickLongOperand::Slot(cached_slot));
                }
                let value = &*self.slot_base.add(index);
                if value.value_type() != ValueType::Long || value.is_reference() {
                    return None;
                }
                let slot = self.allocate_dynamic_slot()?;
                self.slots[slot as usize] = value.raw_long();
                self.caller_cv_slots[index] = slot;
                Some(QuickLongOperand::Slot(slot))
            }
            OpType::Const => Some(QuickLongOperand::Const(
                self.op_array.literals.get(operand as usize)?.as_long()?,
            )),
            OpType::Tmp | OpType::Var | OpType::Unused => None,
        }
    }

    unsafe fn caller_condition_operand(
        &mut self,
        operand: QuickLongOperand,
    ) -> Option<NativeStraightLongConditionOperand> {
        Some(NativeStraightLongConditionOperand::Source(match operand {
            QuickLongOperand::Const(value) => QuickLongOperand::Const(value),
            QuickLongOperand::Slot(slot) => self.caller_operand(OpType::Cv, slot)?,
        }))
    }

    fn append_binary(
        &mut self,
        kind: ScalarLongOpKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
    ) -> Option<QuickLongOperand> {
        let result = self.allocate_dynamic_slot()?;
        self.append_operation(NativeStraightLongOperation::Binary {
            kind,
            lhs,
            rhs,
            result,
        })?;
        Some(QuickLongOperand::Slot(result))
    }

    fn append_operation(
        &mut self,
        operation: NativeStraightLongOperation,
    ) -> Option<usize> {
        if self.operation_count + 2 >= NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
            return None;
        }
        let index = self.operation_count;
        self.operations[index] = operation;
        self.operation_count += 1;
        Some(index)
    }

    fn scalar_source(
        source: ScalarLongSource,
        arguments: &[QuickLongOperand; 8],
        argument_count: u8,
        temporaries: &[QuickLongOperand; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        temporary_count: usize,
    ) -> Option<QuickLongOperand> {
        match source {
            ScalarLongSource::Input(index) if index < u16::from(argument_count) => {
                Some(arguments[index as usize])
            }
            ScalarLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
            ScalarLongSource::Temporary(index) if (index as usize) < temporary_count => {
                Some(temporaries[index as usize])
            }
            _ => None,
        }
    }

    fn lower_plan_operation(
        &mut self,
        plan: &ScalarLongFunctionPlan,
        arguments: &[QuickLongOperand; 8],
        argument_count: u8,
        temporaries: &mut [QuickLongOperand; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        index: usize,
    ) -> Option<()> {
        let operation = *plan.program.operations.get(index)?;
        let lhs = Self::scalar_source(
            operation.lhs,
            arguments,
            argument_count,
            temporaries,
            index,
        )?;
        let rhs = Self::scalar_source(
            operation.rhs,
            arguments,
            argument_count,
            temporaries,
            index,
        )?;
        temporaries[index] = self.append_binary(operation.kind, lhs, rhs)?;
        Some(())
    }

    fn lower_plan(
        &mut self,
        plan: &ScalarLongFunctionPlan,
        arguments: &[QuickLongOperand; 8],
        argument_count: u8,
    ) -> Option<QuickLongOperand> {
        if plan.public_args != argument_count
            || plan.program.output_count != 1
            || plan.program.operations.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
        {
            return None;
        }
        let mut temporaries = [
            QuickLongOperand::Const(0);
            NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
        ];
        let Some(select) = plan.select else {
            for index in 0..plan.program.operations.len() {
                self.lower_plan_operation(
                    plan,
                    arguments,
                    argument_count,
                    &mut temporaries,
                    index,
                )?;
            }
            return Self::scalar_source(
                plan.program.outputs[0],
                arguments,
                argument_count,
                &temporaries,
                plan.program.operations.len(),
            );
        };

        let shared_end = select.shared_operation_count as usize;
        let true_end = shared_end.checked_add(select.when_true_operation_count as usize)?;
        if true_end > plan.program.operations.len() {
            return None;
        }
        for index in 0..shared_end {
            self.lower_plan_operation(
                plan,
                arguments,
                argument_count,
                &mut temporaries,
                index,
            )?;
        }
        let lower_condition_operand = |operand| match operand {
            ScalarLongConditionOperand::Source(source) => {
                Some(NativeStraightLongConditionOperand::Source(Self::scalar_source(
                    source,
                    arguments,
                    argument_count,
                    &temporaries,
                    shared_end,
                )?))
            }
            ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => {
                Some(NativeStraightLongConditionOperand::BitwiseAnd {
                    lhs: Self::scalar_source(
                        lhs,
                        arguments,
                        argument_count,
                        &temporaries,
                        shared_end,
                    )?,
                    rhs: Self::scalar_source(
                        rhs,
                        arguments,
                        argument_count,
                        &temporaries,
                        shared_end,
                    )?,
                })
            }
        };
        let condition_lhs = lower_condition_operand(select.lhs)?;
        let condition_rhs = lower_condition_operand(select.rhs)?;
        let branch_index = self.append_operation(NativeStraightLongOperation::BranchUnless {
            kind: select.kind,
            lhs: condition_lhs,
            rhs: condition_rhs,
            false_target: 0,
        })?;

        for index in shared_end..true_end {
            self.lower_plan_operation(
                plan,
                arguments,
                argument_count,
                &mut temporaries,
                index,
            )?;
        }
        let select_result = self.allocate_dynamic_slot()?;
        let when_true = Self::scalar_source(
            select.when_true,
            arguments,
            argument_count,
            &temporaries,
            true_end,
        )?;
        self.append_operation(NativeStraightLongOperation::Move {
            source: when_true,
            result: select_result,
        })?;
        let jump_index = self.append_operation(NativeStraightLongOperation::Jump { target: 0 })?;

        let false_target = u8::try_from(self.operation_count).ok()?;
        self.operations[branch_index] = NativeStraightLongOperation::BranchUnless {
            kind: select.kind,
            lhs: condition_lhs,
            rhs: condition_rhs,
            false_target,
        };
        for index in true_end..plan.program.operations.len() {
            let operation = plan.program.operations[index];
            let false_source_is_available = |source: ScalarLongSource| match source {
                ScalarLongSource::Temporary(temporary) => {
                    let temporary = temporary as usize;
                    temporary < shared_end || (temporary >= true_end && temporary < index)
                }
                ScalarLongSource::Input(_) | ScalarLongSource::Constant(_) => true,
            };
            if !false_source_is_available(operation.lhs)
                || !false_source_is_available(operation.rhs)
            {
                return None;
            }
            self.lower_plan_operation(
                plan,
                arguments,
                argument_count,
                &mut temporaries,
                index,
            )?;
        }
        if let ScalarLongSource::Temporary(temporary) = select.when_false {
            let temporary = temporary as usize;
            if temporary >= shared_end && temporary < true_end {
                return None;
            }
        }
        let when_false = Self::scalar_source(
            select.when_false,
            arguments,
            argument_count,
            &temporaries,
            plan.program.operations.len(),
        )?;
        self.append_operation(NativeStraightLongOperation::Move {
            source: when_false,
            result: select_result,
        })?;
        let join_target = u8::try_from(self.operation_count).ok()?;
        self.operations[jump_index] = NativeStraightLongOperation::Jump {
            target: join_target,
        };
        Some(QuickLongOperand::Slot(select_result))
    }

    unsafe fn lower_call(
        &mut self,
        initializer_ip: usize,
        expected_target: Option<*const FunctionCommon>,
        expected_plan: Option<*const ScalarLongFunctionPlan>,
        depth: usize,
    ) -> Option<(QuickLongOperand, usize)> {
        if depth >= NATIVE_QUICK_LONG_MAX_CALL_TARGETS
            || self.target_count >= NATIVE_QUICK_LONG_MAX_CALL_TARGETS
        {
            return None;
        }
        let initializer_ptr = self.op_array.instructions.as_ptr().add(initializer_ip);
        let (target, plan_ptr) = composed_scalar_callee(
            self.frame,
            self.op_array,
            initializer_ptr,
        )?;
        if expected_target.is_some_and(|expected| expected != target)
            || expected_plan.is_some_and(|expected| expected != plan_ptr)
        {
            return None;
        }
        let common = &*target;
        let plan = &*plan_ptr;
        let argument_count = usize::try_from(common.sig.public_arity()).ok()?;
        if argument_count > 8 || plan.public_args as usize != argument_count {
            return None;
        }

        let mut arguments = [QuickLongOperand::Const(0); 8];
        let mut cursor = initializer_ip + 1;
        for argument_index in 0..argument_count {
            let destination =
                u16::try_from(common.sig.param_cv_index(argument_index as u32)).ok()?;
            let instruction = *self.op_array.instructions.get(cursor)?;
            if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
                if instruction.op2 != destination {
                    return None;
                }
                arguments[argument_index] =
                    self.caller_operand(instruction.op1_type, instruction.op1)?;
                cursor += 1;
                continue;
            }

            let arithmetic_kind = match instruction.opcode {
                OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => {
                    Some(ScalarLongOpKind::Add)
                }
                OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                    Some(ScalarLongOpKind::Subtract)
                }
                OpCode::Mul => Some(ScalarLongOpKind::Multiply),
                OpCode::Mod | OpCode::Mod_LongLong => Some(ScalarLongOpKind::Modulo),
                OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => {
                    Some(ScalarLongOpKind::BitwiseXor)
                }
                _ => None,
            };
            if let Some(kind) = arithmetic_kind {
                if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                    return None;
                }
                let lhs = self.caller_operand(instruction.op1_type, instruction.op1)?;
                let rhs = self.caller_operand(instruction.op2_type, instruction.op2)?;
                let value = self.append_binary(kind, lhs, rhs)?;
                let send = *self.op_array.instructions.get(cursor + 1)?;
                if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                    || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                    || send.op1 != instruction.result
                    || send.op2 != destination
                {
                    return None;
                }
                arguments[argument_index] = value;
                cursor += 2;
                continue;
            }

            if !matches!(instruction.opcode, OpCode::InitFcall | OpCode::InitMethodCall) {
                return None;
            }
            let (nested_result, nested_do_fcall_ip) =
                self.lower_call(cursor, None, None, depth + 1)?;
            let nested_do_fcall = *self.op_array.instructions.get(nested_do_fcall_ip)?;
            let send = *self.op_array.instructions.get(nested_do_fcall_ip + 1)?;
            if !matches!(nested_do_fcall.result_type, OpType::Tmp | OpType::Var)
                || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != nested_do_fcall.result
                || send.op2 != destination
            {
                return None;
            }
            arguments[argument_index] = nested_result;
            cursor = nested_do_fcall_ip + 2;
        }

        let do_fcall = *self.op_array.instructions.get(cursor)?;
        if do_fcall.opcode != OpCode::DoFcall
            || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
        {
            return None;
        }
        let result = self.lower_plan(plan, &arguments, argument_count as u8)?;
        if self.target_count == NATIVE_QUICK_LONG_MAX_CALL_TARGETS {
            return None;
        }
        self.targets[self.target_count] = target;
        self.target_identities[self.target_count] = target as usize;
        self.target_count += 1;
        Some((result, cursor))
    }
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
unsafe fn native_quick_long_call_accumulate_kernel(
    op_array: &crate::compiler::OpArray,
    frame: *mut ExecuteData,
    plan: &QuickLongAccumulateLoop,
    target: *const FunctionCommon,
    call_plan: &ScalarLongFunctionPlan,
    induction: i64,
    accumulator: i64,
    bound: i64,
) -> Option<(NativeQuickLongCallAccumulateKernel, [i64; 64])> {
    let (guard, do_fcall_ip, argument_count) = match &plan.term {
        QuickLongTerm::ScalarFunctionCall {
            guard,
            do_fcall_ip,
            argument_count,
            ..
        }
        | QuickLongTerm::ScalarCallTree {
            guard,
            do_fcall_ip,
            argument_count,
            ..
        } => (*guard, *do_fcall_ip, *argument_count),
        _ => return None,
    };
    if target.is_null() || call_plan.public_args != argument_count {
        return None;
    }
    let initializer_ip = guard.cache_ip();
    let mut builder = NativeQuickLongCallTreeBuilder::new(
        op_array,
        frame,
        plan.induction_cv,
        plan.accumulator_cv,
        induction,
        accumulator,
        bound,
    );
    let (term, actual_do_fcall_ip) = builder.lower_call(
        initializer_ip,
        Some(target),
        Some(call_plan as *const ScalarLongFunctionPlan),
        0,
    )?;
    if actual_do_fcall_ip != do_fcall_ip {
        return None;
    }
    let trailing_operation_count = if plan.tail_guard.is_some() {
        3
    } else {
        2
    };
    if builder.operation_count + trailing_operation_count > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
        return None;
    }
    builder.operations[builder.operation_count] = NativeStraightLongOperation::Move {
        source: term,
        result: NATIVE_CALL_TERM_SLOT,
    };
    builder.operation_count += 1;
    let sum_operation_index = builder.operation_count as u8;
    builder.operations[builder.operation_count] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(NATIVE_CALL_ACCUMULATOR_SLOT),
        rhs: QuickLongOperand::Slot(NATIVE_CALL_TERM_SLOT),
        result: NATIVE_CALL_SUM_RESULT_SLOT,
        destination: NATIVE_CALL_ACCUMULATOR_SLOT,
    };
    let mut operation_count = builder.operation_count + 1;
    let trace_guard_operation_index = if let Some(trace_guard) = plan.tail_guard {
        let lhs = builder.caller_condition_operand(trace_guard.lhs)?;
        let rhs = builder.caller_condition_operand(trace_guard.rhs)?;
        let index = u8::try_from(operation_count).ok()?;
        builder.operations[operation_count] = NativeStraightLongOperation::Guard {
            kind: trace_guard.kind,
            lhs,
            rhs,
            expected: trace_guard.expected,
        };
        operation_count += 1;
        Some(index)
    } else {
        None
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: NATIVE_CALL_INDUCTION_SLOT,
        bound: QuickLongOperand::Slot(NATIVE_CALL_BOUND_SLOT),
        operations: builder.operations,
        operation_count: operation_count as u8,
        post_result: Some(NATIVE_CALL_POST_RESULT_SLOT),
    };
    Some((
        NativeQuickLongCallAccumulateKernel {
            config,
            targets: builder.targets,
            target_identities: builder.target_identities,
            target_count: builder.target_count as u8,
            sum_operation_index,
            trace_guard_operation_index,
            call_resume_ip: initializer_ip,
        },
        builder.slots,
    ))
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "x86_64",
    target_os = "linux"
))]
unsafe fn run_native_long_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongAccumulateLoop,
    induction_ptr: *mut Value,
    accumulator_ptr: *mut Value,
    condition_ptr: Option<*mut Value>,
    term_ptr: Option<*mut Value>,
    sum_ptr: *mut Value,
    increment_ptr: Option<*mut Value>,
    induction: i64,
    accumulator: i64,
    bound: i64,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    if plan.tail_guard.is_some() {
        return Ok(None);
    }

    const INDUCTION_SLOT: u16 = 0;
    const ACCUMULATOR_SLOT: u16 = 1;
    const SUM_SLOT: u16 = 2;
    const BOUND_SLOT: u16 = 3;
    const TERM_SLOT: u16 = 4;
    const ADDEND_SLOT: u16 = 5;
    let mut operations =
        [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let (sum_operation_index, term_resume_ip, addend) = match plan.term {
        QuickLongTerm::Induction => (0usize, None, None),
        QuickLongTerm::InductionPlusConst {
            addend, term_ip, ..
        } => {
            operations[0] = NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(INDUCTION_SLOT),
                rhs: QuickLongOperand::Const(addend),
                result: TERM_SLOT,
            };
            (1, Some(term_ip), None)
        }
        QuickLongTerm::InductionPlusCv {
            addend_cv,
            term_ip,
            ..
        } => {
            operations[0] = NativeStraightLongOperation::Binary {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(INDUCTION_SLOT),
                rhs: QuickLongOperand::Slot(ADDEND_SLOT),
                result: TERM_SLOT,
            };
            let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
            (1, Some(term_ip), Some((*slot_base.add(addend_cv as usize)).raw_long()))
        }
        _ => return Ok(None),
    };
    let sum_rhs = if sum_operation_index == 0 {
        QuickLongOperand::Slot(INDUCTION_SLOT)
    } else {
        QuickLongOperand::Slot(TERM_SLOT)
    };
    operations[sum_operation_index] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(ACCUMULATOR_SLOT),
        rhs: sum_rhs,
        result: SUM_SLOT,
        destination: ACCUMULATOR_SLOT,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: INDUCTION_SLOT,
        bound: match plan.bound {
            QuickLongBound::Cv(_) => QuickLongOperand::Slot(BOUND_SLOT),
            QuickLongBound::Const(bound) => QuickLongOperand::Const(bound),
        },
        operations,
        operation_count: (sum_operation_index + 1) as u8,
        post_result: None,
    };
    let mut slots = [0i64; 64];
    slots[INDUCTION_SLOT as usize] = induction;
    slots[ACCUMULATOR_SLOT as usize] = accumulator;
    slots[SUM_SLOT as usize] = (*sum_ptr).raw_long();
    slots[BOUND_SLOT as usize] = bound;
    if let Some(ptr) = term_ptr {
        slots[TERM_SLOT as usize] = (*ptr).raw_long();
    }
    if let Some(addend) = addend {
        slots[ADDEND_SLOT as usize] = addend;
    }

    let cache = plan.native_jit();
    let remaining_range_proven = cache
        .prove_straight_remaining_range(&config, &slots)
        .is_some();
    let Some(program) = cache.prepare_straight_program(&config) else {
        return Ok(None);
    };
    let interrupt_flag = eg.vm_interrupt.as_ptr() as *const bool;
    let mut iterations = 0u64;
    let mut entered_native = false;

    loop {
        let before = slots;
        let native_result = if remaining_range_proven {
            let Some(result) = cache.dispatch_prepared_proven_straight_remaining(
                program,
                &config,
                &mut slots,
                interrupt_flag,
                NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            ) else {
                return Ok(None);
            };
            result
        } else {
            cache.dispatch_prepared_straight_chunk(
                program,
                &mut slots,
                NATIVE_LONG_SAFEPOINT_INTERVAL,
            )
        };
        if !entered_native {
            cache.record_region_entry();
            entered_native = true;
        }

        let completed_in_chunk = (slots[INDUCTION_SLOT as usize] as u64)
            .wrapping_sub(before[INDUCTION_SLOT as usize] as u64);
        let result = match native_result {
            Ok(result) => result,
            Err(_) => {
                slots = before;
                Value::write_long(induction_ptr, slots[INDUCTION_SLOT as usize]);
                Value::write_long(accumulator_ptr, slots[ACCUMULATOR_SLOT as usize]);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if iterations != 0 {
                    let last_induction = slots[INDUCTION_SLOT as usize] - 1;
                    let last_term = if sum_operation_index == 0 {
                        last_induction
                    } else {
                        slots[TERM_SLOT as usize]
                    };
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, slots[ACCUMULATOR_SLOT as usize]);
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => slots[INDUCTION_SLOT as usize],
                                QuickIncrementKind::Post => last_induction,
                            },
                        );
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };
        iterations = iterations.saturating_add(completed_in_chunk);

        match result.outcome {
            NativeStraightLongLoopOutcome::Completed => {
                let induction = slots[INDUCTION_SLOT as usize];
                let accumulator = slots[ACCUMULATOR_SLOT as usize];
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, false);
                }
                if iterations != 0 {
                    let last_induction = induction - 1;
                    let last_term = if sum_operation_index == 0 {
                        last_induction
                    } else {
                        slots[TERM_SLOT as usize]
                    };
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, accumulator);
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => induction,
                                QuickIncrementKind::Post => last_induction,
                            },
                        );
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            NativeStraightLongLoopOutcome::ChunkExhausted => {
                debug_assert_eq!(completed_in_chunk, NATIVE_LONG_SAFEPOINT_INTERVAL);
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    let induction = slots[INDUCTION_SLOT as usize];
                    let accumulator = slots[ACCUMULATOR_SLOT as usize];
                    let last_induction = induction - 1;
                    let last_term = if sum_operation_index == 0 {
                        last_induction
                    } else {
                        slots[TERM_SLOT as usize]
                    };
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(accumulator_ptr, accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, accumulator);
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => induction,
                                QuickIncrementKind::Post => last_induction,
                            },
                        );
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                    handle_interrupt(eg)?;
                }
            }
            NativeStraightLongLoopOutcome::OperationSideExit => {
                let failed_operation = result
                    .failed_operation
                    .expect("x86 operation side exit carries its operation index");
                let induction = slots[INDUCTION_SLOT as usize];
                let accumulator = slots[ACCUMULATOR_SLOT as usize];
                Value::write_long(induction_ptr, induction);
                Value::write_long(accumulator_ptr, accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr
                    && (iterations != 0
                        || usize::from(failed_operation) >= sum_operation_index)
                {
                    let last_induction = induction - 1;
                    let term = if sum_operation_index == 0 {
                        last_induction
                    } else {
                        slots[TERM_SLOT as usize]
                    };
                    Value::write_long(ptr, term);
                }
                if iterations != 0 {
                    let last_induction = induction - 1;
                    Value::write_long(sum_ptr, accumulator);
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => induction,
                                QuickIncrementKind::Post => last_induction,
                            },
                        );
                    }
                }
                let resume_ip = if usize::from(failed_operation) < sum_operation_index {
                    term_resume_ip.expect("x86 term operation has a resume point")
                } else {
                    plan.sum_ip
                };
                (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                Value::write_long(induction_ptr, slots[INDUCTION_SLOT as usize]);
                Value::write_long(accumulator_ptr, slots[ACCUMULATOR_SLOT as usize]);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
unsafe fn run_native_long_call_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongAccumulateLoop,
    target: *const FunctionCommon,
    call_plan: &ScalarLongFunctionPlan,
    induction_ptr: *mut Value,
    accumulator_ptr: *mut Value,
    condition_ptr: Option<*mut Value>,
    tail_condition_ptr: Option<*mut Value>,
    term_ptr: Option<*mut Value>,
    sum_ptr: *mut Value,
    increment_ptr: Option<*mut Value>,
    induction: i64,
    accumulator: i64,
    bound: i64,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    let Some((kernel, mut slots)) = native_quick_long_call_accumulate_kernel(
        op_array,
        frame,
        plan,
        target,
        call_plan,
        induction,
        accumulator,
        bound,
    ) else {
        return Ok(None);
    };
    let cache = plan.native_jit();
    let Some(program) = cache.prepare_call_program(
        kernel.target_identities,
        kernel.target_count,
        kernel.config,
    ) else {
        return Ok(None);
    };
    let mut native_iterations = 0u64;
    let mut entered_native = false;

    loop {
        let before_induction = slots[NATIVE_CALL_INDUCTION_SLOT as usize];
        let before_accumulator = slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize];
        let before_term = slots[NATIVE_CALL_TERM_SLOT as usize];
        let before_post = slots[NATIVE_CALL_POST_RESULT_SLOT as usize];
        let native_result = cache.dispatch_prepared_call_chunk(
            program,
            &mut slots,
            NATIVE_LONG_SAFEPOINT_INTERVAL,
        );
        if !entered_native {
            cache.record_region_entry();
            entered_native = true;
        }
        let mut result = match native_result {
            Ok(result) => result,
            Err(_) => {
                Value::write_long(induction_ptr, before_induction);
                Value::write_long(accumulator_ptr, before_accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if native_iterations != 0 {
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, before_term);
                    }
                    Value::write_long(sum_ptr, before_accumulator);
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => before_induction,
                                QuickIncrementKind::Post => before_post,
                            },
                        );
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                stats::inc_quick_loop_deoptimized(native_iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };

        let induction = slots[NATIVE_CALL_INDUCTION_SLOT as usize];
        let completed_in_chunk =
            (induction as u64).wrapping_sub(before_induction as u64);
        native_iterations = native_iterations.saturating_add(completed_in_chunk);
        if result.outcome == NativeStraightLongLoopOutcome::ChunkExhausted
            && induction >= bound
        {
            result.outcome = NativeStraightLongLoopOutcome::Completed;
        }

        let successful_calls = match result.outcome {
            NativeStraightLongLoopOutcome::Completed
            | NativeStraightLongLoopOutcome::ChunkExhausted => completed_in_chunk,
            NativeStraightLongLoopOutcome::OperationSideExit
                if result
                    .failed_operation
                    .is_some_and(|failed| failed >= kernel.sum_operation_index) =>
            {
                completed_in_chunk.saturating_add(1)
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                completed_in_chunk.saturating_add(1)
            }
            NativeStraightLongLoopOutcome::OperationSideExit => completed_in_chunk,
        };
        for called in kernel
            .targets
            .iter()
            .copied()
            .take(kernel.target_count as usize)
        {
            record_scalar_calls_bulk(&*called, successful_calls);
        }

        match result.outcome {
            NativeStraightLongLoopOutcome::Completed => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(
                    accumulator_ptr,
                    slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                );
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, false);
                }
                if native_iterations != 0 {
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, slots[NATIVE_CALL_TERM_SLOT as usize]);
                    }
                    Value::write_long(
                        sum_ptr,
                        slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                    );
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => induction,
                                QuickIncrementKind::Post => {
                                    slots[NATIVE_CALL_POST_RESULT_SLOT as usize]
                                }
                            },
                        );
                    }
                    if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                        Value::write_bool(ptr, guard.expected);
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
                stats::inc_quick_loop_completed(native_iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            NativeStraightLongLoopOutcome::ChunkExhausted => {
                debug_assert_eq!(completed_in_chunk, NATIVE_LONG_SAFEPOINT_INTERVAL);
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    Value::write_long(induction_ptr, induction);
                    Value::write_long(
                        accumulator_ptr,
                        slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                    );
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, slots[NATIVE_CALL_TERM_SLOT as usize]);
                    }
                    Value::write_long(
                        sum_ptr,
                        slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                    );
                    if let Some(ptr) = increment_ptr {
                        Value::write_long(
                            ptr,
                            match plan.increment_kind {
                                QuickIncrementKind::Pre => induction,
                                QuickIncrementKind::Post => {
                                    slots[NATIVE_CALL_POST_RESULT_SLOT as usize]
                                }
                            },
                        );
                    }
                    if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                        Value::write_bool(ptr, guard.expected);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                    handle_interrupt(eg)?;
                }
            }
            NativeStraightLongLoopOutcome::OperationSideExit => {
                let failed_operation = result
                    .failed_operation
                    .expect("call operation side exit carries its operation index");
                Value::write_long(induction_ptr, induction);
                Value::write_long(
                    accumulator_ptr,
                    slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                );
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                let resume_ip = if failed_operation < kernel.sum_operation_index {
                    kernel.call_resume_ip
                } else if failed_operation == kernel.sum_operation_index {
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, slots[NATIVE_CALL_TERM_SLOT as usize]);
                    }
                    plan.sum_ip
                } else {
                    debug_assert_eq!(Some(failed_operation), kernel.trace_guard_operation_index);
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, slots[NATIVE_CALL_TERM_SLOT as usize]);
                    }
                    Value::write_long(
                        sum_ptr,
                        slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                    );
                    plan.tail_guard
                        .expect("native trace-guard side exit has a resume point")
                        .resume_ip
                };
                (*frame).opline = op_array.instructions.as_ptr().add(resume_ip);
                stats::inc_quick_loop_deoptimized(native_iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            NativeStraightLongLoopOutcome::IncrementOverflow => {
                Value::write_long(induction_ptr, induction);
                Value::write_long(
                    accumulator_ptr,
                    slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                );
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    Value::write_long(ptr, slots[NATIVE_CALL_TERM_SLOT as usize]);
                }
                Value::write_long(
                    sum_ptr,
                    slots[NATIVE_CALL_ACCUMULATOR_SLOT as usize],
                );
                if let (Some(guard), Some(ptr)) = (plan.tail_guard, tail_condition_ptr) {
                    Value::write_bool(ptr, guard.expected);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(native_iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        }
    }
}

#[inline(never)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
unsafe fn run_native_long_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongAccumulateLoop,
    induction_ptr: *mut Value,
    accumulator_ptr: *mut Value,
    condition_ptr: Option<*mut Value>,
    term_ptr: Option<*mut Value>,
    sum_ptr: *mut Value,
    increment_ptr: Option<*mut Value>,
    induction: i64,
    accumulator: i64,
    bound: i64,
) -> Result<Option<QuickLoopOutcome>, VmError> {
    let mut state = NativeLongAccumulateState {
        induction,
        bound,
        accumulator,
    };
    let mut iterations = 0u64;
    let cache = plan.native_jit();
    let remaining_range_proven = cache.prove_remaining_range(plan, state);
    let interrupt_flag = eg.vm_interrupt.as_ptr() as *const bool;
    let mut entered_native = false;

    loop {
        let before = state;
        let native_result = if remaining_range_proven {
            cache.dispatch_proven_remaining(
                plan,
                &mut state,
                interrupt_flag,
                NATIVE_LONG_SAFEPOINT_INTERVAL as u16,
            )
        } else {
            cache.dispatch_chunk(
                plan,
                &mut state,
                NATIVE_LONG_SAFEPOINT_INTERVAL,
                false,
            )
        };
        let Some(native_result) = native_result else {
            return Ok(None);
        };
        if !entered_native {
            cache.record_region_entry();
            entered_native = true;
        }

        let completed_in_chunk = (state.induction as u64)
            .wrapping_sub(before.induction as u64);
        iterations = iterations.saturating_add(completed_in_chunk);

        let outcome = match native_result {
            Ok(outcome) => outcome,
            Err(_) => {
                // A malformed native return status is an internal backend
                // failure, not a PHP side exit. Discard this chunk, publish the
                // last known-good boundary, and resume the canonical header.
                state = before;
                Value::write_long(induction_ptr, state.induction);
                Value::write_long(accumulator_ptr, state.accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if iterations > completed_in_chunk {
                    let last_induction = state.induction - 1;
                    let last_term = native_long_accumulate_term(plan, last_induction)
                        .unwrap_unchecked();
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, state.accumulator);
                    if let Some(ptr) = increment_ptr {
                        let last_increment_result = match plan.increment_kind {
                            QuickIncrementKind::Pre => state.induction,
                            QuickIncrementKind::Post => last_term,
                        };
                        Value::write_long(ptr, last_increment_result);
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                stats::inc_quick_loop_deoptimized(
                    iterations.saturating_sub(completed_in_chunk),
                );
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
        };
        let outcome = if outcome == QuickLongAccumulateJitOutcome::ChunkExhausted
            && state.induction >= state.bound
        {
            QuickLongAccumulateJitOutcome::Completed
        } else {
            outcome
        };

        match outcome {
            QuickLongAccumulateJitOutcome::Completed => {
                Value::write_long(induction_ptr, state.induction);
                Value::write_long(accumulator_ptr, state.accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, false);
                }
                if iterations != 0 {
                    let last_induction = state.induction - 1;
                    let last_term = native_long_accumulate_term(plan, last_induction)
                        .unwrap_unchecked();
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, state.accumulator);
                    if let Some(ptr) = increment_ptr {
                        let last_increment_result = match plan.increment_kind {
                            QuickIncrementKind::Pre => state.induction,
                            QuickIncrementKind::Post => last_term,
                        };
                        Value::write_long(ptr, last_increment_result);
                    }
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
                stats::inc_quick_loop_completed(iterations);
                return Ok(Some(QuickLoopOutcome::Completed));
            }
            QuickLongAccumulateJitOutcome::ChunkExhausted => {
                debug_assert_ne!(completed_in_chunk, 0);
                debug_assert_eq!(
                    completed_in_chunk % NATIVE_LONG_SAFEPOINT_INTERVAL,
                    0
                );
                if eg.vm_interrupt.load(Ordering::Relaxed) {
                    Value::write_long(induction_ptr, state.induction);
                    Value::write_long(accumulator_ptr, state.accumulator);
                    if let Some(ptr) = condition_ptr {
                        Value::write_bool(ptr, true);
                    }
                    let last_induction = state.induction - 1;
                    let last_term = native_long_accumulate_term(plan, last_induction)
                        .unwrap_unchecked();
                    if let Some(ptr) = term_ptr {
                        Value::write_long(ptr, last_term);
                    }
                    Value::write_long(sum_ptr, state.accumulator);
                    if let Some(ptr) = increment_ptr {
                        let last_increment_result = match plan.increment_kind {
                            QuickIncrementKind::Pre => state.induction,
                            QuickIncrementKind::Post => last_term,
                        };
                        Value::write_long(ptr, last_increment_result);
                    }
                    (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
                    handle_interrupt(eg)?;
                }
            }
            QuickLongAccumulateJitOutcome::TermOverflow => {
                let term_ip = match plan.term {
                    QuickLongTerm::InductionPlusConst { term_ip, .. } => term_ip,
                    _ => unreachable!("term overflow requires a checked native term"),
                };
                Value::write_long(induction_ptr, state.induction);
                Value::write_long(accumulator_ptr, state.accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(term_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::SumOverflow => {
                Value::write_long(induction_ptr, state.induction);
                Value::write_long(accumulator_ptr, state.accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    let term = native_long_accumulate_term(plan, state.induction)
                        .unwrap_unchecked();
                    Value::write_long(ptr, term);
                }
                (*frame).opline = op_array.instructions.as_ptr().add(plan.sum_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::IncrementOverflow => {
                Value::write_long(induction_ptr, state.induction);
                Value::write_long(accumulator_ptr, state.accumulator);
                if let Some(ptr) = condition_ptr {
                    Value::write_bool(ptr, true);
                }
                if let Some(ptr) = term_ptr {
                    let term = native_long_accumulate_term(plan, state.induction)
                        .unwrap_unchecked();
                    Value::write_long(ptr, term);
                }
                Value::write_long(sum_ptr, state.accumulator);
                (*frame).opline = op_array.instructions.as_ptr().add(plan.increment_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(Some(QuickLoopOutcome::Deoptimized));
            }
            QuickLongAccumulateJitOutcome::ConditionSideExit => {
                unreachable!("the accumulate-only native program has no condition side exit")
            }
        }
    }
}

#[inline(always)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn native_long_accumulate_term(
    plan: &QuickLongAccumulateLoop,
    induction: i64,
) -> Option<i64> {
    match plan.term {
        QuickLongTerm::Induction => Some(induction),
        QuickLongTerm::InductionPlusConst { addend, .. } => induction.checked_add(addend),
        _ => None,
    }
}
