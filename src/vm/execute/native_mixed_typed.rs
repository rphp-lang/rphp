// Kept in the execute module through include! so this structural split does not change visibility or code generation.

impl NativeMixedBuildState {
    fn typed_long_source(
        source: ScalarLongSource,
        arguments: &[QuickObjectLongArgument; 8],
        argument_count: u8,
        temporaries: &[Option<QuickLongOperand>; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    ) -> Option<QuickLongOperand> {
        match source {
            ScalarLongSource::Input(index) if index < u16::from(argument_count) => {
                match arguments[index as usize] {
                    QuickObjectLongArgument::Long(source) => Some(source),
                    QuickObjectLongArgument::StringSlot(_) => None,
                }
            }
            ScalarLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
            ScalarLongSource::Temporary(index) => temporaries.get(index as usize).copied().flatten(),
            _ => None,
        }
    }

    fn typed_string_length_source(
        source: ScalarStringSource,
        arguments: &[QuickObjectLongArgument; 8],
        argument_count: u8,
        string_temporaries: &[Option<QuickLongOperand>; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    ) -> Option<Result<u16, QuickLongOperand>> {
        match source {
            ScalarStringSource::Input(index) if index < argument_count => {
                match arguments[index as usize] {
                    QuickObjectLongArgument::StringSlot(slot) => Some(Ok(slot)),
                    QuickObjectLongArgument::Long(_) => None,
                }
            }
            ScalarStringSource::Temporary(index) => string_temporaries
                .get(index as usize)
                .copied()
                .flatten()
                .map(Err),
            _ => None,
        }
    }

    fn lower_typed_method(
        &mut self,
        target: *const FunctionCommon,
        plan: &ComposedTypedLongFunctionPlan,
        call: &QuickObjectLongMethodCall,
        result_slot: u16,
    ) -> Option<()> {
        if self.call_count == NATIVE_QUICK_LONG_MAX_CALL_TARGETS
            || plan.public_args != call.argument_count
            || plan.program.output_count != 1
            || plan.program.operations.len() > NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
            || plan.program.operations.iter().any(|operation| matches!(
                operation,
                ComposedTypedLongOp::Call(_) | ComposedTypedLongOp::StringCall(_)
            ))
        {
            return None;
        }

        let mut temporaries = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        let mut string_temporaries = [None; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        for (index, operation) in plan.program.operations.iter().enumerate() {
            match operation {
                ComposedTypedLongOp::Arithmetic(operation) => {
                    let lhs = Self::typed_long_source(
                        operation.lhs,
                        &call.arguments,
                        call.argument_count,
                        &temporaries,
                    )?;
                    let rhs = Self::typed_long_source(
                        operation.rhs,
                        &call.arguments,
                        call.argument_count,
                        &temporaries,
                    )?;
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
                }
                ComposedTypedLongOp::StringLength(source) => {
                    let length = match Self::typed_string_length_source(
                        *source,
                        &call.arguments,
                        call.argument_count,
                        &string_temporaries,
                    )? {
                        Ok(source) => {
                            let result = self.allocate_slot()?;
                            self.append(
                                NativeStraightLongOperation::StringLength {
                                    source,
                                    lengths: self.string_lengths,
                                    token_count: self.string_token_count as u8,
                                    result,
                                },
                                call.resume_ip,
                            )?;
                            QuickLongOperand::Slot(result)
                        }
                        Err(length) => length,
                    };
                    temporaries[index] = Some(length);
                }
                ComposedTypedLongOp::StringConcatLiteral { value, literal_len } => {
                    let base = match Self::typed_string_length_source(
                        *value,
                        &call.arguments,
                        call.argument_count,
                        &string_temporaries,
                    )? {
                        Ok(source) => {
                            let result = self.allocate_slot()?;
                            self.append(
                                NativeStraightLongOperation::StringLength {
                                    source,
                                    lengths: self.string_lengths,
                                    token_count: self.string_token_count as u8,
                                    result,
                                },
                                call.resume_ip,
                            )?;
                            QuickLongOperand::Slot(result)
                        }
                        Err(length) => length,
                    };
                    let result = self.allocate_slot()?;
                    self.append(
                        NativeStraightLongOperation::Binary {
                            kind: ScalarLongOpKind::Add,
                            lhs: base,
                            rhs: QuickLongOperand::Const(i64::from(*literal_len)),
                            result,
                        },
                        call.resume_ip,
                    )?;
                    string_temporaries[index] = Some(QuickLongOperand::Slot(result));
                }
                ComposedTypedLongOp::Call(_) | ComposedTypedLongOp::StringCall(_) => {
                    return None;
                }
            }
        }

        let output = Self::typed_long_source(
            plan.program.outputs[0],
            &call.arguments,
            call.argument_count,
            &temporaries,
        )?;
        let completion = self.append(
            NativeStraightLongOperation::Move {
                source: output,
                result: result_slot,
            },
            call.resume_ip,
        )?;
        self.call_targets[self.call_count] = target;
        self.call_completion_operations[self.call_count] = completion;
        self.call_count += 1;
        Some(())
    }
}
