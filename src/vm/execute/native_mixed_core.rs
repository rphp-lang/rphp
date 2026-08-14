// Kept in the execute module through include! so this structural split does not change visibility or code generation.

struct NativeMixedBuildState {
    operations: [NativeStraightLongOperation; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    operation_resume_ips: [usize; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    operation_count: usize,
    used_slots: u64,
    string_literals: [u16; NATIVE_FINITE_STRING_LIMIT],
    string_lengths: [i64; NATIVE_FINITE_STRING_LIMIT],
    string_token_count: usize,
    context_array_slots: [u16; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    context_tokens: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    context_kinds: [NativeMixedContextKind; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    context_count: usize,
    property_binding_op_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_property_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_slots: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_receivers: [*const Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_object_slots: [usize; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_count: usize,
    call_targets: [*const FunctionCommon; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    call_completion_operations: [u8; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    call_count: usize,
}

impl NativeMixedBuildState {
    fn append(
        &mut self,
        operation: NativeStraightLongOperation,
        resume_ip: usize,
    ) -> Option<u8> {
        if self.operation_count == NATIVE_STRAIGHT_LONG_MAX_OPERATIONS {
            return None;
        }
        let index = self.operation_count;
        self.operations[index] = operation;
        self.operation_resume_ips[index] = resume_ip;
        self.operation_count += 1;
        Some(index as u8)
    }

    fn allocate_slot(&mut self) -> Option<u16> {
        for slot in (0..64u16).rev() {
            let bit = 1u64 << slot;
            if self.used_slots & bit == 0 {
                self.used_slots |= bit;
                return Some(slot);
            }
        }
        None
    }

    fn token_for_literal(&self, literal: u16) -> Option<u8> {
        self.string_literals[..self.string_token_count]
            .iter()
            .position(|candidate| *candidate == literal)
            .and_then(|index| u8::try_from(index).ok())
    }

    fn token_for_callee_literal(
        &self,
        caller: &crate::compiler::OpArray,
        callee: &UserFunction,
        literal: u16,
    ) -> Option<Option<u8>> {
        let expected = callee.op_array.literals.get(literal as usize)?.as_str()?;
        for (token, caller_literal) in self.string_literals[..self.string_token_count]
            .iter()
            .copied()
            .enumerate()
        {
            let candidate = caller
                .literals
                .get(caller_literal as usize)?
                .as_str()?;
            if candidate == expected {
                return Some(Some(u8::try_from(token).ok()?));
            }
        }
        Some(None)
    }

    fn object_long_source(
        user: &UserFunction,
        source: ObjectLongSource,
        call: &QuickObjectLongMethodCall,
    ) -> Option<QuickLongOperand> {
        match source {
            ObjectLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
            ObjectLongSource::Slot(slot) => {
                for index in 0..call.argument_count {
                    if user.common.sig.param_cv_index(u32::from(index)) != u32::from(slot) {
                        continue;
                    }
                    return match call.arguments[index as usize] {
                        QuickObjectLongArgument::Long(source) => Some(source),
                        QuickObjectLongArgument::StringSlot(_) => None,
                    };
                }
                None
            }
        }
    }

    fn patch_branch_target(&mut self, operation: u8, false_target: u8) -> Option<()> {
        let NativeStraightLongOperation::BranchUnless { kind, lhs, rhs, .. } =
            self.operations[operation as usize]
        else {
            return None;
        };
        self.operations[operation as usize] = NativeStraightLongOperation::BranchUnless {
            kind,
            lhs,
            rhs,
            false_target,
        };
        Some(())
    }

    fn patch_jump_target(&mut self, operation: u8, target: u8) -> Option<()> {
        if !matches!(
            self.operations[operation as usize],
            NativeStraightLongOperation::Jump { .. }
        ) {
            return None;
        }
        self.operations[operation as usize] = NativeStraightLongOperation::Jump { target };
        Some(())
    }

    fn record_call(&mut self, target: *const FunctionCommon, completion: u8) -> Option<()> {
        if self.call_count == NATIVE_QUICK_LONG_MAX_CALL_TARGETS {
            return None;
        }
        self.call_targets[self.call_count] = target;
        self.call_completion_operations[self.call_count] = completion;
        self.call_count += 1;
        Some(())
    }

    fn lower_object_method(
        &mut self,
        caller: &crate::compiler::OpArray,
        target: *const FunctionCommon,
        user: &UserFunction,
        plan: &ObjectLongFunctionPlan,
        call: &QuickObjectLongMethodCall,
        result_slot: u16,
    ) -> Option<()> {
        if self.call_count == NATIVE_QUICK_LONG_MAX_CALL_TARGETS
            || plan.public_args != call.argument_count
        {
            return None;
        }

        if let Some(score) = plan.weighted_string_score.as_deref() {
            if score.string_argument >= call.argument_count {
                return None;
            }
            let weighted = Self::object_long_source(user, score.weighted_input, call)?;
            let additive = Self::object_long_source(user, score.additive_input, call)?;
            let QuickObjectLongArgument::StringSlot(string_slot) =
                *call.arguments.get(score.string_argument as usize)?
            else {
                return None;
            };
            let value = self.allocate_slot()?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: weighted,
                    rhs: QuickLongOperand::Const(score.multiplier),
                    result: value,
                },
                call.resume_ip,
            )?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::Add,
                    lhs: QuickLongOperand::Slot(value),
                    rhs: additive,
                    result: value,
                },
                call.resume_ip,
            )?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::IntDivide,
                    lhs: QuickLongOperand::Slot(value),
                    rhs: QuickLongOperand::Const(score.divisor),
                    result: value,
                },
                call.resume_ip,
            )?;
            let string_length = self.allocate_slot()?;
            self.append(
                NativeStraightLongOperation::StringLength {
                    source: string_slot,
                    lengths: self.string_lengths,
                    token_count: self.string_token_count as u8,
                    result: string_length,
                },
                call.resume_ip,
            )?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::Add,
                    lhs: QuickLongOperand::Slot(value),
                    rhs: QuickLongOperand::Slot(string_length),
                    result: value,
                },
                call.resume_ip,
            )?;

            let mut adjustment_tokens = 0u64;
            for adjustment in score.string_adjustments.iter().copied() {
                let Some(token) =
                    self.token_for_callee_literal(caller, user, adjustment.literal)?
                else {
                    continue;
                };
                let bit = 1u64 << token;
                if adjustment_tokens & bit != 0 {
                    return None;
                }
                adjustment_tokens |= bit;
                let branch = self.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(string_slot),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Const(i64::from(token)),
                        ),
                        false_target: 0,
                    },
                    call.resume_ip,
                )?;
                self.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(value),
                        rhs: QuickLongOperand::Const(adjustment.addend),
                        result: value,
                    },
                    call.resume_ip,
                )?;
                self.patch_branch_target(branch, u8::try_from(self.operation_count).ok()?)?;
            }

            for adjustment in score.conditional_adjustments.iter().copied() {
                let lhs = Self::object_long_source(user, adjustment.lhs, call)?;
                let rhs = Self::object_long_source(user, adjustment.rhs, call)?;
                let branch = self.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: adjustment.kind,
                        lhs: NativeStraightLongConditionOperand::Source(lhs),
                        rhs: NativeStraightLongConditionOperand::Source(rhs),
                        false_target: 0,
                    },
                    call.resume_ip,
                )?;
                self.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs: QuickLongOperand::Slot(value),
                        rhs: QuickLongOperand::Const(adjustment.addend),
                        result: value,
                    },
                    call.resume_ip,
                )?;
                self.patch_branch_target(branch, u8::try_from(self.operation_count).ok()?)?;
            }

            let completion = self.append(
                NativeStraightLongOperation::Move {
                    source: QuickLongOperand::Slot(value),
                    result: result_slot,
                },
                call.resume_ip,
            )?;
            return self.record_call(target, completion);
        }

        if let Some(select) = plan.string_intdiv_select.as_deref() {
            if select.string_argument >= call.argument_count {
                return None;
            }
            let input = Self::object_long_source(user, select.input, call)?;
            let QuickObjectLongArgument::StringSlot(string_slot) =
                call.arguments[select.string_argument as usize]
            else {
                return None;
            };
            let work = self.allocate_slot()?;
            let selected = self.allocate_slot()?;
            let mut selected_tokens = 0u64;
            let mut completion_jumps = Vec::with_capacity(select.cases.len());
            for case in select.cases.iter() {
                let Some(token) = self.token_for_callee_literal(caller, user, case.literal)? else {
                    continue;
                };
                let bit = 1u64 << token;
                if selected_tokens & bit != 0 {
                    return None;
                }
                selected_tokens |= bit;
                let branch = self.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(string_slot),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Const(i64::from(token)),
                        ),
                        false_target: 0,
                    },
                    call.resume_ip,
                )?;
                self.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Multiply,
                        lhs: input,
                        rhs: QuickLongOperand::Const(case.arm.multiplier),
                        result: work,
                    },
                    call.resume_ip,
                )?;
                self.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::IntDivide,
                        lhs: QuickLongOperand::Slot(work),
                        rhs: QuickLongOperand::Const(case.arm.divisor),
                        result: selected,
                    },
                    call.resume_ip,
                )?;
                completion_jumps.push(self.append(
                    NativeStraightLongOperation::Jump { target: 0 },
                    call.resume_ip,
                )?);
                self.patch_branch_target(branch, u8::try_from(self.operation_count).ok()?)?;
            }
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: input,
                    rhs: QuickLongOperand::Const(select.default_arm.multiplier),
                    result: work,
                },
                call.resume_ip,
            )?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::IntDivide,
                    lhs: QuickLongOperand::Slot(work),
                    rhs: QuickLongOperand::Const(select.default_arm.divisor),
                    result: selected,
                },
                call.resume_ip,
            )?;
            let completion_target = u8::try_from(self.operation_count).ok()?;
            let completion = self.append(
                NativeStraightLongOperation::Move {
                    source: QuickLongOperand::Slot(selected),
                    result: result_slot,
                },
                call.resume_ip,
            )?;
            for jump in completion_jumps {
                self.patch_jump_target(jump, completion_target)?;
            }
            return self.record_call(target, completion);
        }

        if let Some(select) = plan.modulo_any_select.as_deref() {
            let selected = self.allocate_slot()?;
            let remainder = self.allocate_slot()?;
            let mut match_jumps = Vec::with_capacity(select.terms.len());
            for term in select.terms.iter().copied() {
                let input = Self::object_long_source(user, term.input, call)?;
                self.append(
                    NativeStraightLongOperation::Binary {
                        kind: ScalarLongOpKind::Modulo,
                        lhs: input,
                        rhs: QuickLongOperand::Const(term.divisor),
                        result: remainder,
                    },
                    call.resume_ip,
                )?;
                let branch = self.append(
                    NativeStraightLongOperation::BranchUnless {
                        kind: ScalarLongConditionKind::Equal,
                        lhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Slot(remainder),
                        ),
                        rhs: NativeStraightLongConditionOperand::Source(
                            QuickLongOperand::Const(term.expected),
                        ),
                        false_target: 0,
                    },
                    call.resume_ip,
                )?;
                self.append(
                    NativeStraightLongOperation::Move {
                        source: QuickLongOperand::Const(select.when_match),
                        result: selected,
                    },
                    call.resume_ip,
                )?;
                match_jumps.push(self.append(
                    NativeStraightLongOperation::Jump { target: 0 },
                    call.resume_ip,
                )?);
                self.patch_branch_target(branch, u8::try_from(self.operation_count).ok()?)?;
            }
            self.append(
                NativeStraightLongOperation::Move {
                    source: QuickLongOperand::Const(select.when_miss),
                    result: selected,
                },
                call.resume_ip,
            )?;
            let completion_target = u8::try_from(self.operation_count).ok()?;
            let completion = self.append(
                NativeStraightLongOperation::Move {
                    source: QuickLongOperand::Slot(selected),
                    result: result_slot,
                },
                call.resume_ip,
            )?;
            for jump in match_jumps {
                self.patch_jump_target(jump, completion_target)?;
            }
            return self.record_call(target, completion);
        }

        None
    }
}
