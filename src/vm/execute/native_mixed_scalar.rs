// Kept in the execute module through include! so this structural split does not change visibility or code generation.

impl NativeMixedBuildState {
    fn scalar_method_source(
        source: ScalarLongSource,
        call: &QuickTypedMethodCall,
        temporaries: &[Option<QuickLongOperand>; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        temporary_count: usize,
    ) -> Option<QuickLongOperand> {
        match source {
            ScalarLongSource::Input(index) if index < u16::from(call.argument_count) => {
                call.arguments.get(index as usize).copied()
            }
            ScalarLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
            ScalarLongSource::Temporary(index) if (index as usize) < temporary_count => {
                temporaries.get(index as usize).copied().flatten()
            }
            _ => None,
        }
    }

    fn lower_scalar_method_operation(
        &mut self,
        plan: &ScalarLongFunctionPlan,
        call: &QuickTypedMethodCall,
        temporaries: &mut [Option<QuickLongOperand>;
            NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        index: usize,
    ) -> Option<()> {
        let operation = *plan.program.operations.get(index)?;
        let lhs = Self::scalar_method_source(operation.lhs, call, temporaries, index)?;
        let rhs = Self::scalar_method_source(operation.rhs, call, temporaries, index)?;
        let result = self.allocate_slot()?;
        self.append(
            NativeStraightLongOperation::Binary {
                kind: operation.kind,
                lhs,
                rhs,
                result,
            },
            call.resume_ip,
        )?;
        temporaries[index] = Some(QuickLongOperand::Slot(result));
        Some(())
    }

    fn scalar_method_condition_operand(
        operand: ScalarLongConditionOperand,
        call: &QuickTypedMethodCall,
        temporaries: &[Option<QuickLongOperand>; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
        temporary_count: usize,
    ) -> Option<NativeStraightLongConditionOperand> {
        match operand {
            ScalarLongConditionOperand::Source(source) => {
                Some(NativeStraightLongConditionOperand::Source(
                    Self::scalar_method_source(source, call, temporaries, temporary_count)?,
                ))
            }
            ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => {
                Some(NativeStraightLongConditionOperand::BitwiseAnd {
                    lhs: Self::scalar_method_source(
                        lhs,
                        call,
                        temporaries,
                        temporary_count,
                    )?,
                    rhs: Self::scalar_method_source(
                        rhs,
                        call,
                        temporaries,
                        temporary_count,
                    )?,
                })
            }
        }
    }

    fn lower_scalar_method(
        &mut self,
        target: *const FunctionCommon,
        plan: &ScalarLongFunctionPlan,
        call: &QuickTypedMethodCall,
        result_slot: u16,
    ) -> Option<()> {
        if plan.public_args != call.argument_count
            || plan.program.output_count != 1
            || plan.program.operations.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
        {
            return None;
        }
        let mut temporaries = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        let output = if let Some(select) = plan.select {
            let shared_end = select.shared_operation_count as usize;
            let true_end = shared_end.checked_add(select.when_true_operation_count as usize)?;
            if true_end > plan.program.operations.len() {
                return None;
            }
            for index in 0..shared_end {
                self.lower_scalar_method_operation(plan, call, &mut temporaries, index)?;
            }
            let lhs = Self::scalar_method_condition_operand(
                select.lhs,
                call,
                &temporaries,
                shared_end,
            )?;
            let rhs = Self::scalar_method_condition_operand(
                select.rhs,
                call,
                &temporaries,
                shared_end,
            )?;
            let branch = self.append(
                NativeStraightLongOperation::BranchUnless {
                    kind: select.kind,
                    lhs,
                    rhs,
                    false_target: 0,
                },
                call.resume_ip,
            )?;
            for index in shared_end..true_end {
                self.lower_scalar_method_operation(plan, call, &mut temporaries, index)?;
            }
            let result = self.allocate_slot()?;
            let when_true = Self::scalar_method_source(
                select.when_true,
                call,
                &temporaries,
                true_end,
            )?;
            self.append(
                NativeStraightLongOperation::Move {
                    source: when_true,
                    result,
                },
                call.resume_ip,
            )?;
            let jump = self.append(
                NativeStraightLongOperation::Jump { target: 0 },
                call.resume_ip,
            )?;
            self.patch_branch_target(branch, u8::try_from(self.operation_count).ok()?)?;
            for index in true_end..plan.program.operations.len() {
                let operation = plan.program.operations[index];
                let available = |source| match source {
                    ScalarLongSource::Temporary(temporary) => {
                        let temporary = temporary as usize;
                        temporary < shared_end || (temporary >= true_end && temporary < index)
                    }
                    ScalarLongSource::Input(_) | ScalarLongSource::Constant(_) => true,
                };
                if !available(operation.lhs) || !available(operation.rhs) {
                    return None;
                }
                self.lower_scalar_method_operation(plan, call, &mut temporaries, index)?;
            }
            if matches!(select.when_false, ScalarLongSource::Temporary(temporary)
                if (temporary as usize) >= shared_end && (temporary as usize) < true_end)
            {
                return None;
            }
            let when_false = Self::scalar_method_source(
                select.when_false,
                call,
                &temporaries,
                plan.program.operations.len(),
            )?;
            self.append(
                NativeStraightLongOperation::Move {
                    source: when_false,
                    result,
                },
                call.resume_ip,
            )?;
            self.patch_jump_target(jump, u8::try_from(self.operation_count).ok()?)?;
            QuickLongOperand::Slot(result)
        } else {
            for index in 0..plan.program.operations.len() {
                self.lower_scalar_method_operation(plan, call, &mut temporaries, index)?;
            }
            Self::scalar_method_source(
                plan.program.outputs[0],
                call,
                &temporaries,
                plan.program.operations.len(),
            )?
        };
        let completion = self.append(
            NativeStraightLongOperation::Move {
                source: output,
                result: result_slot,
            },
            call.resume_ip,
        )?;
        self.record_call(target, completion)
    }

}
