// Kept in the execute module through include! so this structural split does not change visibility or code generation.

impl NativeMixedBuildState {
    fn property_binding_slot(
        &mut self,
        op_index: usize,
        receiver: *const Value,
        object_slot: usize,
        property_index: usize,
    ) -> Option<u16> {
        for index in 0..self.property_binding_count {
            if self.property_binding_receivers[index] == receiver
                && self.property_binding_object_slots[index] == object_slot
            {
                return Some(u16::from(self.property_binding_slots[index]));
            }
        }
        if self.property_binding_count == NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES {
            return None;
        }
        let shadow_slot = self.allocate_slot()?;
        let index = self.property_binding_count;
        self.property_binding_op_indices[index] = u8::try_from(op_index).ok()?;
        self.property_binding_property_indices[index] = u8::try_from(property_index).ok()?;
        self.property_binding_slots[index] = u8::try_from(shadow_slot).ok()?;
        self.property_binding_receivers[index] = receiver;
        self.property_binding_object_slots[index] = object_slot;
        self.property_binding_count += 1;
        Some(shadow_slot)
    }

    fn property_method_source(
        source: LongPlanSource,
        call: &QuickTypedMethodCall,
    ) -> Option<QuickLongOperand> {
        match source {
            LongPlanSource::Argument(index) if index < call.argument_count => {
                call.arguments.get(index as usize).copied()
            }
            LongPlanSource::Constant(value) => Some(QuickLongOperand::Const(value)),
            LongPlanSource::Argument(_) => None,
        }
    }

    fn lower_property_getter(
        &mut self,
        op_index: usize,
        receiver: *const Value,
        target: *const FunctionCommon,
        property_slot: usize,
        call: &QuickTypedMethodCall,
        result_slot: u16,
    ) -> Option<()> {
        if call.argument_count != 0 {
            return None;
        }
        let shadow_slot = self.property_binding_slot(op_index, receiver, property_slot, 0)?;
        let completion = self.append(
            NativeStraightLongOperation::Move {
                source: QuickLongOperand::Slot(shadow_slot),
                result: result_slot,
            },
            call.resume_ip,
        )?;
        self.record_call(target, completion)
    }

    fn lower_property_method(
        &mut self,
        op_index: usize,
        receiver: *const Value,
        target: *const FunctionCommon,
        plan: &LongPropertyMethodPlan,
        property_slots: &[usize; 8],
        property_count: u8,
        call: &QuickTypedMethodCall,
    ) -> Option<()> {
        if plan.public_args != call.argument_count
            || property_count == 0
            || property_count as usize != plan.properties.len()
            || property_count > 8
            || plan.operations.is_empty()
        {
            return None;
        }
        let mut values = [None; 8];
        let mut bindings = [u16::MAX; 8];
        for index in 0..property_count as usize {
            let slot = self.property_binding_slot(
                op_index,
                receiver,
                property_slots[index],
                index,
            )?;
            bindings[index] = slot;
            values[index] = Some(QuickLongOperand::Slot(slot));
        }
        let mut written = 0u8;
        for operation in plan.operations.iter().copied() {
            let (property, value) = match operation {
                LongPropertyOp::Add { property, rhs }
                | LongPropertyOp::Sub { property, rhs } => {
                    let current = values.get(property as usize).copied().flatten()?;
                    let rhs = Self::property_method_source(rhs, call)?;
                    let result = self.allocate_slot()?;
                    self.append(
                        NativeStraightLongOperation::Binary {
                            kind: if matches!(operation, LongPropertyOp::Add { .. }) {
                                ScalarLongOpKind::Add
                            } else {
                                ScalarLongOpKind::Subtract
                            },
                            lhs: current,
                            rhs,
                            result,
                        },
                        call.resume_ip,
                    )?;
                    (property, QuickLongOperand::Slot(result))
                }
                LongPropertyOp::Set { property, value } => {
                    (property, Self::property_method_source(value, call)?)
                }
                LongPropertyOp::Min {
                    property,
                    candidate,
                }
                | LongPropertyOp::Max {
                    property,
                    candidate,
                } => {
                    let current = values.get(property as usize).copied().flatten()?;
                    let candidate = Self::property_method_source(candidate, call)?;
                    let result = self.allocate_slot()?;
                    self.append(
                        NativeStraightLongOperation::Move {
                            source: current,
                            result,
                        },
                        call.resume_ip,
                    )?;
                    let (lhs, rhs) = if matches!(operation, LongPropertyOp::Min { .. }) {
                        (candidate, current)
                    } else {
                        (current, candidate)
                    };
                    let branch = self.append(
                        NativeStraightLongOperation::BranchUnless {
                            kind: ScalarLongConditionKind::LessThan,
                            lhs: NativeStraightLongConditionOperand::Source(lhs),
                            rhs: NativeStraightLongConditionOperand::Source(rhs),
                            false_target: 0,
                        },
                        call.resume_ip,
                    )?;
                    self.append(
                        NativeStraightLongOperation::Move {
                            source: candidate,
                            result,
                        },
                        call.resume_ip,
                    )?;
                    self.patch_branch_target(
                        branch,
                        u8::try_from(self.operation_count).ok()?,
                    )?;
                    (property, QuickLongOperand::Slot(result))
                }
            };
            *values.get_mut(property as usize)? = Some(value);
            written |= 1u8.checked_shl(u32::from(property))?;
        }
        let mut completion = None;
        for index in 0..property_count as usize {
            if written & (1u8 << index) == 0 {
                continue;
            }
            completion = Some(self.append(
                NativeStraightLongOperation::Move {
                    source: values[index]?,
                    result: bindings[index],
                },
                call.resume_ip,
            )?);
        }
        self.record_call(target, completion?)
    }
}
