// Kept in the execute module through include! so this structural split does not change visibility or code generation.

impl NativeMixedBuildState {
    fn virtual_property_argument(
        resolved: &QuickResolvedVirtualPipeline,
        owner: &UserFunction,
        object: ObjectLongObjectSource,
        cache_ip: u16,
    ) -> Option<u8> {
        if object != ObjectLongObjectSource::Argument(0) {
            return None;
        }
        let cache = owner.op_array.cache.get(cache_ip as usize)?;
        if cache.class_id != resolved.class_id || cache.property_flags() & 1 == 0 {
            return None;
        }
        let property_slot = cache.property_slot();
        resolved.property_slots[..resolved.property_count as usize]
            .iter()
            .position(|candidate| *candidate == property_slot)
            .map(|index| resolved.property_arguments[index])
    }

    fn virtual_long_argument(
        resolved: &QuickResolvedVirtualPipeline,
        constructor_arguments: &[QuickVirtualValueSource; 8],
        owner: &UserFunction,
        object: ObjectLongObjectSource,
        cache_ip: u16,
    ) -> Option<QuickLongOperand> {
        let argument = Self::virtual_property_argument(resolved, owner, object, cache_ip)?;
        match *constructor_arguments.get(argument as usize)? {
            QuickVirtualValueSource::Long(source) => Some(source),
            QuickVirtualValueSource::StringLiteral(_)
            | QuickVirtualValueSource::StringSlot(_) => None,
        }
    }

    fn virtual_string_slot(
        &mut self,
        resolved: &QuickResolvedVirtualPipeline,
        constructor_arguments: &[QuickVirtualValueSource; 8],
        owner: &UserFunction,
        object: ObjectLongObjectSource,
        cache_ip: u16,
        resume_ip: usize,
    ) -> Option<u16> {
        let argument = Self::virtual_property_argument(resolved, owner, object, cache_ip)?;
        match *constructor_arguments.get(argument as usize)? {
            QuickVirtualValueSource::StringSlot(slot) => Some(slot),
            QuickVirtualValueSource::StringLiteral(literal) => {
                let slot = self.allocate_slot()?;
                self.append(
                    NativeStraightLongOperation::StringToken {
                        token: self.token_for_literal(literal)?,
                        result: slot,
                    },
                    resume_ip,
                )?;
                Some(slot)
            }
            QuickVirtualValueSource::Long(_) => None,
        }
    }

    fn native_local_destination(
        &mut self,
        destinations: &mut [Option<u16>; 64],
        sources: &mut [Option<QuickLongOperand>; 64],
        destination: u16,
    ) -> Option<u16> {
        let index = destination as usize;
        if index >= destinations.len() {
            return None;
        }
        let slot = match destinations[index] {
            Some(slot) => slot,
            None => {
                let slot = self.allocate_slot()?;
                destinations[index] = Some(slot);
                slot
            }
        };
        sources[index] = Some(QuickLongOperand::Slot(slot));
        Some(slot)
    }

    fn object_array_long_source(
        resolved: &QuickResolvedVirtualPipeline,
        constructor_arguments: &[QuickVirtualValueSource; 8],
        owner: &UserFunction,
        sources: &[Option<QuickLongOperand>; 64],
        source: ObjectArraySource,
    ) -> Option<QuickLongOperand> {
        match source {
            ObjectArraySource::LongSlot(slot) => sources.get(slot as usize).copied().flatten(),
            ObjectArraySource::Literal(literal) => owner
                .op_array
                .literals
                .get(literal as usize)
                .filter(|value| value.value_type() == ValueType::Long)
                .map(|value| QuickLongOperand::Const(unsafe { value.raw_long() })),
            ObjectArraySource::Property { object, cache_ip } => Self::virtual_long_argument(
                resolved,
                constructor_arguments,
                owner,
                object,
                cache_ip,
            ),
            ObjectArraySource::Receiver | ObjectArraySource::Argument(_) => None,
        }
    }

    unsafe fn virtual_nested_call(
        resolved: &QuickResolvedVirtualPipeline,
        call: &ObjectArrayLongCall,
    ) -> Option<(*const FunctionCommon, *const UserFunction, *const ObjectLongFunctionPlan)> {
        let ObjectArraySource::Property {
            object: ObjectLongObjectSource::Receiver,
            cache_ip: receiver_cache_ip,
        } = call.receiver
        else {
            return None;
        };
        let owner = &*resolved.method_user;
        let receiver = &*resolved.method_receiver;
        let receiver_cache = owner.op_array.cache.get(receiver_cache_ip as usize)?;
        if receiver.value_type() != ValueType::Object
            || receiver.is_reference()
            || receiver_cache.property_flags() & 1 == 0
        {
            return None;
        }
        let receiver_class_id = receiver.object_class_id_unchecked();
        if receiver_class_id == 0 || receiver_cache.class_id != receiver_class_id {
            return None;
        }
        let nested_receiver = receiver.object_property_slot_unchecked(
            receiver_cache.property_slot(),
        );
        if nested_receiver.is_null()
            || (*nested_receiver).value_type() != ValueType::Object
            || (*nested_receiver).is_reference()
        {
            return None;
        }
        let nested_class_id = (*nested_receiver).object_class_id_unchecked();
        let cache = owner.op_array.cache.get(call.cache_ip as usize)?;
        let initializer = owner.op_array.instructions.get(call.cache_ip as usize)?;
        if nested_class_id == 0
            || cache.class_id != nested_class_id
            || cache.func.is_null()
            || !method_return_dispatch_contract_matches(initializer, &*cache.func)
        {
            return None;
        }
        let common = &*cache.func;
        if common.fn_type != FunctionType::User
            || common.sig.public_arity() != call.arguments.len() as u32
            || common.sig.required_num_args != call.arguments.len() as u32
            || common.sig.ref_args != 0
            || common.sig.is_variadic
            || !common.plan.call.is_compact_user_call()
            || common.plan.ret != ReturnStrategy::Fast
        {
            return None;
        }
        let user = &*(cache.func as *const UserFunction);
        let plan = user.object_long_plan.as_deref()?;
        if plan.public_args as usize != call.arguments.len() {
            return None;
        }
        Some((cache.func, user, plan))
    }

    fn lower_virtual_object_method(
        &mut self,
        resolved: &QuickResolvedVirtualPipeline,
        constructor_arguments: &[QuickVirtualValueSource; 8],
        target: *const FunctionCommon,
        user: &UserFunction,
        plan: &ObjectLongFunctionPlan,
        result_slot: u16,
        resume_ip: usize,
    ) -> Option<()> {
        if plan.operations.is_empty()
            || plan.operations.len() > 64
            || plan.public_args != 1
            || plan.object_argument_mask != 1
            || plan.long_argument_mask != 0
            || plan.string_argument_mask != 0
        {
            return None;
        }
        let mut sources = [None; 64];
        let mut destinations = [None; 64];
        let mut conditions = [None; 64];
        let mut plan_to_native = [u8::MAX; 65];
        let mut pending_branches = Vec::new();
        let mut pending_jumps = Vec::new();
        let mut ip = 0usize;
        let mut completion = None;
        while ip < plan.operations.len() {
            plan_to_native[ip] = u8::try_from(self.operation_count).ok()?;
            match plan.operations[ip] {
                ObjectLongOp::Noop => {}
                ObjectLongOp::Assign {
                    destination,
                    source,
                } => {
                    let source = match source {
                        ObjectLongSource::Constant(value) => QuickLongOperand::Const(value),
                        ObjectLongSource::Slot(slot) => {
                            sources.get(slot as usize).copied().flatten()?
                        }
                    };
                    let destination = self.native_local_destination(
                        &mut destinations,
                        &mut sources,
                        destination,
                    )?;
                    self.append(
                        NativeStraightLongOperation::Move {
                            source,
                            result: destination,
                        },
                        resume_ip,
                    )?;
                }
                ObjectLongOp::FetchProperty {
                    object,
                    cache_ip,
                    destination,
                } => {
                    let source = Self::virtual_long_argument(
                        resolved,
                        constructor_arguments,
                        user,
                        object,
                        cache_ip,
                    )?;
                    *sources.get_mut(destination as usize)? = Some(source);
                }
                ObjectLongOp::Arithmetic {
                    kind,
                    lhs,
                    rhs,
                    destination,
                } => {
                    let resolve = |source| match source {
                        ObjectLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
                        ObjectLongSource::Slot(slot) => {
                            sources.get(slot as usize).copied().flatten()
                        }
                    };
                    let lhs = resolve(lhs)?;
                    let rhs = resolve(rhs)?;
                    let mut native_destination = destination;
                    let mut skip_assign = false;
                    if let Some(ObjectLongOp::Assign {
                        destination: assigned,
                        source: ObjectLongSource::Slot(source),
                    }) = plan.operations.get(ip + 1)
                        && *source == destination
                        && !plan.operations.iter().any(|operation| match operation {
                            ObjectLongOp::JumpIfFalse { target, .. }
                            | ObjectLongOp::JumpIfTrue { target, .. }
                            | ObjectLongOp::Jump { target } => usize::from(*target) == ip + 1,
                            _ => false,
                        })
                    {
                        native_destination = *assigned;
                        skip_assign = true;
                    }
                    let result = self.native_local_destination(
                        &mut destinations,
                        &mut sources,
                        native_destination,
                    )?;
                    self.append(
                        NativeStraightLongOperation::Binary {
                            kind,
                            lhs,
                            rhs,
                            result,
                        },
                        resume_ip,
                    )?;
                    if skip_assign {
                        *sources.get_mut(destination as usize)? =
                            Some(QuickLongOperand::Slot(result));
                        plan_to_native[ip + 1] = u8::try_from(self.operation_count).ok()?;
                        ip += 1;
                    }
                }
                ObjectLongOp::Compare {
                    kind,
                    lhs,
                    rhs,
                    destination,
                } => {
                    let resolve = |source| match source {
                        ObjectLongSource::Constant(value) => Some(QuickLongOperand::Const(value)),
                        ObjectLongSource::Slot(slot) => {
                            sources.get(slot as usize).copied().flatten()
                        }
                    };
                    *conditions.get_mut(destination as usize)? = Some((
                        kind,
                        resolve(lhs)?,
                        resolve(rhs)?,
                    ));
                }
                ObjectLongOp::JumpIfFalse { condition, target } => {
                    let ObjectLongSource::Slot(condition) = condition else {
                        return None;
                    };
                    let (kind, lhs, rhs) = conditions
                        .get(condition as usize)
                        .copied()
                        .flatten()?;
                    let branch = self.append(
                        NativeStraightLongOperation::BranchUnless {
                            kind,
                            lhs: NativeStraightLongConditionOperand::Source(lhs),
                            rhs: NativeStraightLongConditionOperand::Source(rhs),
                            false_target: 0,
                        },
                        resume_ip,
                    )?;
                    pending_branches.push((branch, target));
                }
                ObjectLongOp::Jump { target } => {
                    let jump = self.append(
                        NativeStraightLongOperation::Jump { target: 0 },
                        resume_ip,
                    )?;
                    pending_jumps.push((jump, target));
                }
                ObjectLongOp::Return { value } => {
                    let value = match value {
                        ObjectLongSource::Constant(value) => QuickLongOperand::Const(value),
                        ObjectLongSource::Slot(slot) => {
                            sources.get(slot as usize).copied().flatten()?
                        }
                    };
                    completion = Some(self.append(
                        NativeStraightLongOperation::Move {
                            source: value,
                            result: result_slot,
                        },
                        resume_ip,
                    )?);
                    ip += 1;
                    while matches!(plan.operations.get(ip), Some(ObjectLongOp::Bail)) {
                        plan_to_native[ip] = u8::try_from(self.operation_count).ok()?;
                        ip += 1;
                    }
                    if ip != plan.operations.len() {
                        return None;
                    }
                    break;
                }
                ObjectLongOp::JumpIfTrue { .. }
                | ObjectLongOp::StringLiteralBranch { .. }
                | ObjectLongOp::StringLength { .. }
                | ObjectLongOp::IntDiv { .. }
                | ObjectLongOp::Bail => return None,
            }
            ip += 1;
        }
        plan_to_native[plan.operations.len()] = u8::try_from(self.operation_count).ok()?;
        for (branch, target) in pending_branches {
            let target = *plan_to_native.get(target as usize)?;
            if target == u8::MAX {
                return None;
            }
            self.patch_branch_target(branch, target)?;
        }
        for (jump, target) in pending_jumps {
            let target = *plan_to_native.get(target as usize)?;
            if target == u8::MAX {
                return None;
            }
            self.patch_jump_target(jump, target)?;
        }
        self.record_call(target, completion?)
    }

    fn object_array_entry_source(
        caller: &crate::compiler::OpArray,
        key_literal: u16,
        owner: &UserFunction,
        plan: &ObjectArrayFunctionPlan,
    ) -> Option<ObjectArraySource> {
        let key = caller.literals.get(key_literal as usize)?.as_str()?;
        plan.entries.iter().rev().find_map(|entry| {
            owner
                .op_array
                .literals
                .get(entry.key_literal as usize)
                .and_then(Value::as_str)
                .filter(|candidate| *candidate == key)
                .map(|_| entry.value)
        })
    }

    unsafe fn lower_virtual_pipeline(
        &mut self,
        caller: &crate::compiler::OpArray,
        resolved: &QuickResolvedVirtualPipeline,
        constructor_arguments: &[QuickVirtualValueSource; 8],
        consumers: &[QuickObjectArrayConsumer; 4],
        consumer_count: u8,
        trailing_key_literal: Option<u16>,
        trailing_result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    ) -> Option<()> {
        let owner = &*resolved.method_user;
        let plan = &*resolved.method_plan;
        if plan.operations.is_empty()
            || plan.operations.len() > 64
            || plan.entries.is_empty()
            || plan.entries.len() > 4
            || consumer_count == 0
            || consumer_count > 4
        {
            return None;
        }
        let transaction_call_start = self.call_count;
        let mut sources = [None; 64];
        let mut destinations = [None; 64];
        for operation in plan.operations.iter() {
            match operation {
                ObjectArrayLongOp::Assign {
                    destination,
                    source,
                } => {
                    let source = Self::object_array_long_source(
                        resolved,
                        constructor_arguments,
                        owner,
                        &sources,
                        *source,
                    )?;
                    *sources.get_mut(*destination as usize)? = Some(source);
                }
                ObjectArrayLongOp::Arithmetic {
                    kind,
                    lhs,
                    rhs,
                    destination,
                } => {
                    let lhs = Self::object_array_long_source(
                        resolved,
                        constructor_arguments,
                        owner,
                        &sources,
                        *lhs,
                    )?;
                    let rhs = Self::object_array_long_source(
                        resolved,
                        constructor_arguments,
                        owner,
                        &sources,
                        *rhs,
                    )?;
                    let result = self.native_local_destination(
                        &mut destinations,
                        &mut sources,
                        *destination,
                    )?;
                    self.append(
                        NativeStraightLongOperation::Binary {
                            kind: *kind,
                            lhs,
                            rhs,
                            result,
                        },
                        resume_ip,
                    )?;
                }
                ObjectArrayLongOp::IntDiv {
                    lhs,
                    rhs,
                    destination,
                } => {
                    let lhs = Self::object_array_long_source(
                        resolved,
                        constructor_arguments,
                        owner,
                        &sources,
                        *lhs,
                    )?;
                    let rhs = Self::object_array_long_source(
                        resolved,
                        constructor_arguments,
                        owner,
                        &sources,
                        *rhs,
                    )?;
                    let result = self.native_local_destination(
                        &mut destinations,
                        &mut sources,
                        *destination,
                    )?;
                    self.append(
                        NativeStraightLongOperation::Binary {
                            kind: ScalarLongOpKind::IntDivide,
                            lhs,
                            rhs,
                            result,
                        },
                        resume_ip,
                    )?;
                }
                ObjectArrayLongOp::Call(call) => {
                    let (target, user, nested_plan) =
                        Self::virtual_nested_call(resolved, call)?;
                    let user = &*user;
                    let nested_plan = &*nested_plan;
                    let result = self.native_local_destination(
                        &mut destinations,
                        &mut sources,
                        call.destination,
                    )?;
                    if nested_plan.object_argument_mask != 0 {
                        if call.arguments.len() != 1
                            || call.arguments[0] != ObjectArraySource::Argument(0)
                        {
                            return None;
                        }
                        self.lower_virtual_object_method(
                            resolved,
                            constructor_arguments,
                            target,
                            user,
                            nested_plan,
                            result,
                            resume_ip,
                        )?;
                    } else {
                        if call.arguments.len() > 8 {
                            return None;
                        }
                        let mut arguments = [QuickObjectLongArgument::Long(
                            QuickLongOperand::Const(0),
                        ); 8];
                        for (index, source) in call.arguments.iter().copied().enumerate() {
                            let bit = 1u8 << index;
                            arguments[index] = if nested_plan.long_argument_mask & bit != 0 {
                                QuickObjectLongArgument::Long(Self::object_array_long_source(
                                    resolved,
                                    constructor_arguments,
                                    owner,
                                    &sources,
                                    source,
                                )?)
                            } else if nested_plan.string_argument_mask & bit != 0 {
                                let ObjectArraySource::Property { object, cache_ip } = source else {
                                    return None;
                                };
                                QuickObjectLongArgument::StringSlot(self.virtual_string_slot(
                                    resolved,
                                    constructor_arguments,
                                    owner,
                                    object,
                                    cache_ip,
                                    resume_ip,
                                )?)
                            } else {
                                return None;
                            };
                        }
                        let method_call = QuickObjectLongMethodCall {
                            guard: ScalarLongCallGuard::FunctionCache {
                                cache_ip: u32::from(call.cache_ip),
                            },
                            arguments,
                            argument_count: call.arguments.len() as u8,
                            next_target,
                            resume_ip,
                        };
                        self.lower_object_method(
                            caller,
                            target,
                            user,
                            nested_plan,
                            &method_call,
                            result,
                        )?;
                    }
                }
            }
        }

        let mut committed_destinations = [u16::MAX; 4];
        let mut committed_sources = [QuickLongOperand::Const(0); 4];
        let mut committed_count = 0usize;
        for consumer in consumers.iter().copied().take(consumer_count as usize) {
            let current = committed_destinations[..committed_count]
                .iter()
                .rposition(|destination| *destination == consumer.accumulator)
                .map(|index| committed_sources[index])
                .unwrap_or(QuickLongOperand::Slot(consumer.accumulator));
            let entry = Self::object_array_entry_source(
                caller,
                consumer.key_literal,
                owner,
                plan,
            )?;
            let value = Self::object_array_long_source(
                resolved,
                constructor_arguments,
                owner,
                &sources,
                entry,
            )?;
            let result = self.allocate_slot()?;
            self.append(
                NativeStraightLongOperation::Binary {
                    kind: ScalarLongOpKind::Add,
                    lhs: current,
                    rhs: value,
                    result,
                },
                resume_ip,
            )?;
            if let Some(index) = committed_destinations[..committed_count]
                .iter()
                .position(|destination| *destination == consumer.accumulator)
            {
                committed_sources[index] = QuickLongOperand::Slot(result);
            } else {
                *committed_destinations.get_mut(committed_count)? = consumer.accumulator;
                committed_sources[committed_count] = QuickLongOperand::Slot(result);
                committed_count += 1;
            }
        }
        let trailing = match trailing_key_literal {
            Some(key) => {
                let entry = Self::object_array_entry_source(caller, key, owner, plan)?;
                Some(Self::object_array_long_source(
                    resolved,
                    constructor_arguments,
                    owner,
                    &sources,
                    entry,
                )?)
            }
            None => None,
        };
        let mut final_completion = None;
        for index in 0..committed_count {
            final_completion = Some(self.append(
                NativeStraightLongOperation::Move {
                    source: committed_sources[index],
                    result: committed_destinations[index],
                },
                resume_ip,
            )?);
        }
        if let Some(source) = trailing {
            final_completion = Some(self.append(
                NativeStraightLongOperation::Move {
                    source,
                    result: trailing_result,
                },
                resume_ip,
            )?);
        }
        let final_completion = final_completion?;
        self.record_call(resolved.constructor_target, final_completion)?;
        self.record_call(resolved.method_target, final_completion)?;
        for index in transaction_call_start..self.call_count {
            self.call_completion_operations[index] = final_completion;
        }
        Some(())
    }
}
