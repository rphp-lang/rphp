/// Build a small typed program for a closed scalar loop.
///
/// This deliberately supports only side-effect-free long operations and
/// forward branches inside the body. Unsupported instructions leave the
/// original backedge untouched.
#[derive(Clone, Copy)]
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
struct VirtualClosureAlias {
    source: u16,
    destination: u16,
    resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
struct PendingIndirectScalarCall {
    guard: ScalarLongCallGuard,
    callable_slot: u16,
    outer_argument_count: u8,
    next_argument: u8,
    arguments: [QuickLongOperand; 8],
    resume_ip: usize,
}

/// Admit one dead local alias used solely as the first argument of the
/// immediately following named call. Eliding the Rc copy is safe because the
/// destination has no other read or write anywhere in the function; every
/// side exit resumes at the original assignment before the call protocol.
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
fn virtual_closure_alias(
    op_array: &OpArray,
    region: &[crate::vm::instruction::Instruction],
    relative_ip: usize,
    absolute_ip: usize,
) -> Option<VirtualClosureAlias> {
    let assignment = *region.get(relative_ip)?;
    let initializer = *region.get(relative_ip + 1)?;
    let send = *region.get(relative_ip + 2)?;
    if assignment.opcode != OpCode::AssignCv
        || assignment.op1_type != OpType::Cv
        || assignment.op2_type != OpType::Cv
        || assignment.result_type != OpType::Unused
        || assignment.op1 == assignment.op2
        || initializer.opcode != OpCode::InitFcall
        || send.opcode != OpCode::SendVal
        || send.op1_type != OpType::Cv
        || send.op1 != assignment.op1
        || send.op2 != 0
        || !cv_unmodified_in_region(region, assignment.op2)
    {
        return None;
    }

    let destination = assignment.op1;
    let mut allowed_assignment = false;
    let mut allowed_send = false;
    for instruction in &op_array.instructions {
        if instruction.op1_type == OpType::Cv && instruction.op1 == destination {
            if instruction.opcode == OpCode::AssignCv
                && instruction.op2_type == OpType::Cv
                && instruction.op2 == assignment.op2
                && !allowed_assignment
            {
                allowed_assignment = true;
            } else if instruction.opcode == OpCode::SendVal
                && instruction.op2 == 0
                && !allowed_send
            {
                allowed_send = true;
            } else {
                return None;
            }
        }
        if instruction.op2_type == OpType::Cv && instruction.op2 == destination {
            return None;
        }
        if instruction.result_type == OpType::Cv && instruction.result == destination {
            return None;
        }
    }
    (allowed_assignment && allowed_send).then_some(VirtualClosureAlias {
        source: assignment.op2,
        destination,
        resume_ip: absolute_ip,
    })
}

pub fn detect_long_ops_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongOpsLoop> {
    detect_long_ops_region_inner(op_array, header_ip, backedge_ip, true)
}

/// Build a typed, straight-line application region between semantic events.
/// Calls, returns, visible mutation, and control-flow edges remain baseline
/// boundaries; the returned plan shares the exact side-exit contract and
/// executor with closed quick loops.
pub fn detect_long_ops_region(
    op_array: &OpArray,
    entry_ip: usize,
    end_ip: usize,
) -> Option<QuickLongOpsLoop> {
    detect_long_ops_region_inner(op_array, entry_ip, end_ip, false)
}

fn detect_long_ops_region_inner(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
    closed_loop: bool,
) -> Option<QuickLongOpsLoop> {
    if header_ip >= backedge_ip
        || backedge_ip >= op_array.instructions.len()
        || backedge_ip - header_ip >= u16::MAX as usize
    {
        return None;
    }

    if closed_loop {
        let backedge = op_array.instructions[backedge_ip];
        if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
            || backedge.op1 as usize != header_ip
        {
            return None;
        }
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64 {
        return None;
    }

    let region_len = backedge_ip - header_ip + 1;
    let mut ip_to_op = vec![u16::MAX; region_len];
    let mut ops = Vec::new();
    let mut op_ips = Vec::new();
    let mut long_input_mask = 0u64;
    let mut long_output_mask = 0u64;
    let mut bool_output_mask = 0u64;
    let mut array_input_mask = 0u64;
    let mut array_output_mask = 0u64;
    let mut structural_array_output_mask = 0u64;
    let mut set_array_output_mask = 0u64;
    let mut string_input_mask = 0u64;
    let mut string_output_mask = 0u64;
    let mut string_append_mask = 0u64;
    let mut object_input_mask = 0u64;
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    let mut closure_input_mask = 0u64;
    let mut typed_invariant_source = None;
    let mut json_projections = InvariantJsonProjectionState::new(total_slots);
    let mut has_add = false;
    let mut has_assign = false;
    let mut has_object_call = false;
    let mut has_array_push = false;
    let mut has_string_append = false;
    let mut has_post_inc = false;
    let mut ip = header_ip;

    // String array-key state is selected only when every visible assignment to
    // that key is either a string literal or an immutable CV whose last
    // preheader definition is a string literal. Runtime guards still verify all
    // selected CVs; ambiguous integer-key programs keep their established path.
    let region = &op_array.instructions[header_ip..=backedge_ip];
    let mut array_index_cv_mask = 0u64;
    for instruction in region {
        if instruction.opcode == OpCode::FetchDimR && instruction.op2_type == OpType::Cv {
            add_mask_slot(&mut array_index_cv_mask, instruction.op2, total_slots)?;
        }
    }

    // A non-escaping virtual constructor may also retain a string CV even
    // though it is never used as an array key in the caller. Admit only CVs
    // whose complete visible assignment set below proves immutable strings.
    let mut virtual_string_candidate_mask = 0u64;
    for (relative_ip, instruction) in region.iter().copied().enumerate() {
        if instruction.opcode != OpCode::NewObj
            || instruction._pad & crate::vm::instruction::NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE
                == 0
        {
            continue;
        }
        let new_ip = header_ip + relative_ip;
        for index in 0..instruction.extended_value as usize {
            let send = *op_array.instructions.get(new_ip + 1 + index)?;
            if send.op1_type == OpType::Cv {
                add_mask_slot(&mut virtual_string_candidate_mask, send.op1, total_slots)?;
            }
        }
    }

    let mut string_key_assignment_mask = 0u64;
    let mut candidates = array_index_cv_mask | virtual_string_candidate_mask;
    while candidates != 0 {
        let key = candidates.trailing_zeros() as u16;
        candidates &= candidates - 1;
        let mut has_assignment = false;
        let mut valid = true;
        for instruction in region.iter().filter(|instruction| {
            instruction.opcode == OpCode::AssignCv
                && instruction.op1_type == OpType::Cv
                && instruction.op1 == key
                && instruction.result_type == OpType::Unused
        }) {
            has_assignment = true;
            valid &= match instruction.op2_type {
                OpType::Const => op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .is_some(),
                OpType::Cv => {
                    preheader_string_literal_cv(op_array, header_ip, instruction.op2).is_some()
                        && cv_unmodified_in_region(region, instruction.op2)
                }
                _ => false,
            };
        }
        if has_assignment && valid {
            add_mask_slot(&mut string_key_assignment_mask, key, total_slots)?;
        }
    }

    let mut string_source_input_mask = 0u64;
    let mut string_cache_literals = [u16::MAX; QUICK_STRING_FETCH_CACHE_LIMIT];
    let mut string_cache_literal_count = 0usize;
    let mut finite_string_literals = [u16::MAX; QUICK_STRING_FETCH_CACHE_LIMIT];
    let mut finite_string_literal_count = 0usize;
    let mut finite_string_literal_overflow = false;
    for instruction in region.iter().filter(|instruction| {
        instruction.opcode == OpCode::AssignCv
            && instruction.op1_type == OpType::Cv
            && string_key_assignment_mask & (1u64 << instruction.op1) != 0
    }) {
        let literal = match instruction.op2_type {
            OpType::Cv => {
                add_mask_slot(&mut string_source_input_mask, instruction.op2, total_slots)?;
                preheader_string_literal_cv(op_array, header_ip, instruction.op2)?
            }
            OpType::Const
                if !string_cache_literals[..string_cache_literal_count]
                    .contains(&instruction.op2)
                    && string_cache_literal_count < string_cache_literals.len() =>
            {
                string_cache_literals[string_cache_literal_count] = instruction.op2;
                string_cache_literal_count += 1;
                instruction.op2
            }
            OpType::Const => instruction.op2,
            _ => continue,
        };
        if finite_string_literals[..finite_string_literal_count].contains(&literal) {
            continue;
        }
        if finite_string_literal_count == finite_string_literals.len() {
            finite_string_literal_overflow = true;
            continue;
        }
        finite_string_literals[finite_string_literal_count] = literal;
        finite_string_literal_count += 1;
    }
    string_input_mask |= string_source_input_mask;
    let string_cache_capacity = (string_source_input_mask.count_ones() as usize
        + string_cache_literal_count)
        .min(QUICK_STRING_FETCH_CACHE_LIMIT);

    let mut passthrough_ips = Vec::new();
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    let mut closure_alias = None;
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    let mut pending_indirect_call: Option<PendingIndirectScalarCall> = None;
    while ip <= backedge_ip {
        let instruction = op_array.instructions[ip];
        if instruction.opcode == OpCode::ReleaseTemps {
            if instruction.op1_type != OpType::Tmp
                || instruction.op2_type != OpType::Tmp
                || instruction.result_type != OpType::Unused
                || instruction.op1 > instruction.op2
                || u32::from(instruction.op2) > total_slots
            {
                return None;
            }
            passthrough_ips.push(ip);
            ip += 1;
            continue;
        }
        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        if pending_indirect_call.is_none()
            && closure_alias.is_none()
            && instruction.opcode == OpCode::AssignCv
            && let Some(alias) = virtual_closure_alias(op_array, region, ip - header_ip, ip)
        {
            add_mask_slot(&mut closure_input_mask, alias.source, total_slots)?;
            closure_alias = Some(alias);
            passthrough_ips.push(ip);
            ip += 1;
            continue;
        }
        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        if pending_indirect_call.is_none()
            && instruction.opcode == OpCode::InitFcall
            && let Some(alias) = closure_alias.take()
        {
            let outer_argument_count = u8::try_from(instruction.op1).ok()?;
            let first_send = *op_array.instructions.get(ip + 1)?;
            if outer_argument_count == 0
                || outer_argument_count > 8
                || instruction.op2_type != OpType::Const
                || first_send.opcode != OpCode::SendVal
                || first_send.op1_type != OpType::Cv
                || first_send.op1 != alias.destination
                || first_send.op2 != 0
            {
                return None;
            }
            pending_indirect_call = Some(PendingIndirectScalarCall {
                guard: ScalarLongCallGuard::FunctionCache {
                    cache_ip: u32::try_from(ip).ok()?,
                },
                callable_slot: alias.source,
                outer_argument_count,
                next_argument: 1,
                arguments: [QuickLongOperand::Const(0); 8],
                resume_ip: alias.resume_ip,
            });
            passthrough_ips.push(ip);
            passthrough_ips.push(ip + 1);
            ip += 2;
            continue;
        }
        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        if let Some(mut pending) = pending_indirect_call
            && matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx)
        {
            if pending.next_argument >= pending.outer_argument_count
                || instruction.op2 != u16::from(pending.next_argument)
            {
                return None;
            }
            let argument = match instruction.op1_type {
                OpType::Cv | OpType::Tmp | OpType::Var => {
                    add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                    QuickLongOperand::Slot(instruction.op1)
                }
                OpType::Const => QuickLongOperand::Const(long_literal(op_array, instruction.op1)?),
                OpType::Unused => return None,
            };
            pending.arguments[usize::from(pending.next_argument - 1)] = argument;
            pending.next_argument += 1;
            pending_indirect_call = Some(pending);
            passthrough_ips.push(ip);
            ip += 1;
            continue;
        }
        let op = match instruction.opcode {
            OpCode::IsSmaller => {
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Cv
                    || instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: QuickLongOperand::Slot(instruction.op2),
                        },
                        condition_tmp: Some(instruction.result),
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: QuickLongOperand::Slot(instruction.op2),
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::IsSmaller_CvConst => {
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: Some(instruction.result),
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::JmpZ_Lt_CvConst => {
                let dead_branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Unused
                    || dead_branch.opcode != OpCode::JmpZ
                    || dead_branch.op1_type != OpType::Tmp
                    || dead_branch.op2_type != OpType::Unused
                    || dead_branch.op2 != instruction.result
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, instruction.result as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: None,
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: None,
                        false_target: QuickLongTarget::unresolved(instruction.result as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::Mod => {
                let value = long_slot(instruction.op1_type, instruction.op1)?;
                if instruction.op2_type != OpType::Const || instruction.result_type != OpType::Tmp {
                    return None;
                }
                let divisor = long_literal(op_array, instruction.op2)?;
                if divisor == 0 {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, value, total_slots)?;
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::ModConst {
                    value,
                    divisor,
                    result: instruction.result,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::DirectInternalCall2
                if crate::builtin_metadata::DirectInternalKind::from_id(
                    instruction.extended_value,
                ) == Some(crate::builtin_metadata::DirectInternalKind::JsonDecode) =>
            {
                let skipped_by_prior_edge = ops.iter().skip(1).any(|operation| match *operation {
                    QuickLongOp::BranchUnlessLt { false_target, .. }
                    | QuickLongOp::BranchUnlessEq { false_target, .. }
                    | QuickLongOp::BranchUnlessLe { false_target, .. } => false_target
                        .unresolved_ip()
                        .is_some_and(|target| target > ip),
                    QuickLongOp::TraceGuard { .. } | QuickLongOp::Jump { .. } => true,
                    _ => false,
                });
                if typed_invariant_source.is_some() || skipped_by_prior_edge {
                    return None;
                }
                let source = detect_json_typed_invariant_source(op_array, region, ip)?;
                // The invariant-source prelude owns validation of its JSON
                // input, including the String/reference guard. Do not classify
                // that source as a string-token input unless another operation
                // in the region consumes the same CV; native mixed regions
                // would otherwise try to map arbitrary JSON to a finite token.
                json_projections.start(source.destination)?;
                typed_invariant_source = Some(source);
                has_assign = true;
                let resume_ip = ip;
                ip += 2;
                QuickLongOp::JsonProjectionStep {
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::FetchDimR => {
                let array = long_slot(instruction.op1_type, instruction.op1)?;
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                if json_projections.extend_fetch(op_array, instruction, array, total_slots)? {
                    let resume_ip = ip;
                    ip += 1;
                    QuickLongOp::JsonProjectionStep {
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    let index = match instruction.op2_type {
                        OpType::Cv | OpType::Tmp => {
                            if instruction.op2_type == OpType::Cv
                                && string_key_assignment_mask & (1u64 << instruction.op2) != 0
                            {
                                add_mask_slot(
                                    &mut string_input_mask,
                                    instruction.op2,
                                    total_slots,
                                )?;
                                QuickArrayIndex::ValueSlot(instruction.op2)
                            } else {
                                add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                                QuickArrayIndex::Long(QuickLongOperand::Slot(instruction.op2))
                            }
                        }
                        OpType::Const => array_literal_index(op_array, instruction.op2)?,
                        _ => return None,
                    };
                    add_mask_slot(&mut array_input_mask, array, total_slots)?;
                    add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                    let destination = op_array
                        .instructions
                        .get(ip + 1)
                        .copied()
                        .and_then(long_assign)
                        .and_then(|(destination, source)| {
                            (source == instruction.result).then_some(destination)
                        });
                    let resume_ip = ip;
                    if let Some(destination) = destination {
                        add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                        has_assign = true;
                        ip += 2;
                    } else {
                        ip += 1;
                    }
                    QuickLongOp::FetchArrayLong {
                        array,
                        index,
                        result: instruction.result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::Strlen | OpCode::Strlen_String => {
                if !matches!(instruction.op1_type, OpType::Tmp | OpType::Var)
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                json_projections.derive_string_length(instruction, total_slots)?;
                add_mask_slot(&mut long_input_mask, instruction.result, total_slots)?;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::JsonProjectionStep {
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::AssignDim => {
                if json_projections.tracks(instruction.op1) {
                    // Reusing one decoded array is observable once the loop
                    // mutates it; canonical execution creates a fresh array on
                    // every iteration. Keep all such roots on the baseline.
                    return None;
                }
                if instruction.op1_type != OpType::Cv
                    || !matches!(
                        instruction.result_type,
                        OpType::Cv | OpType::Tmp | OpType::Var
                    )
                {
                    return None;
                }
                let array = instruction.op1;
                let index = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp => {
                        if instruction.op2_type == OpType::Cv
                            && string_key_assignment_mask & (1u64 << instruction.op2) != 0
                        {
                            add_mask_slot(&mut string_input_mask, instruction.op2, total_slots)?;
                            QuickArrayIndex::ValueSlot(instruction.op2)
                        } else {
                            add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                            QuickArrayIndex::Long(QuickLongOperand::Slot(instruction.op2))
                        }
                    }
                    OpType::Const => array_literal_index(op_array, instruction.op2)?,
                    _ => return None,
                };

                add_mask_slot(&mut array_output_mask, array, total_slots)?;
                add_mask_slot(&mut long_input_mask, instruction.result, total_slots)?;
                has_assign = true;
                let resume_ip = ip;
                ip += 1;

                // A preceding guarded read plus arithmetic proves replacement
                // of an existing entry, so its borrowed array view remains
                // stable. All other typed integer assignments use canonical
                // `set_int`; they may resize and are kept disjoint from reads
                // of the same array after the full region has been inspected.
                if is_existing_array_long_replacement(
                    &ops,
                    array,
                    index,
                    instruction.result,
                ) {
                    add_mask_slot(&mut array_input_mask, array, total_slots)?;
                    QuickLongOp::StoreArrayLong {
                        array,
                        index,
                        value: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    let QuickArrayIndex::Long(index) = index else {
                        return None;
                    };
                    add_mask_slot(&mut structural_array_output_mask, array, total_slots)?;
                    add_mask_slot(&mut set_array_output_mask, array, total_slots)?;
                    QuickLongOp::SetArrayLong {
                        array,
                        index,
                        value: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::ArrayPushOp => {
                if json_projections.tracks(instruction.op1) {
                    return None;
                }
                if instruction.op1_type != OpType::Cv || instruction.result_type != OpType::Unused {
                    return None;
                }
                let value = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp | OpType::Var => {
                        add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                        QuickLongOperand::Slot(instruction.op2)
                    }
                    OpType::Const => {
                        QuickLongOperand::Const(long_literal(op_array, instruction.op2)?)
                    }
                    OpType::Unused => return None,
                };
                add_mask_slot(&mut array_output_mask, instruction.op1, total_slots)?;
                add_mask_slot(
                    &mut structural_array_output_mask,
                    instruction.op1,
                    total_slots,
                )?;
                has_array_push = true;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::ArrayPushLong {
                    array: instruction.op1,
                    value,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::IsEqual | OpCode::IsEqual_CvConst => {
                let lhs = long_slot(instruction.op1_type, instruction.op1)?;
                let rhs = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp => QuickLongOperand::Slot(instruction.op2),
                    OpType::Const => {
                        QuickLongOperand::Const(long_literal(op_array, instruction.op2)?)
                    }
                    _ => return None,
                };
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                if let QuickLongOperand::Slot(rhs) = rhs {
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                }
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;

                if let Some((add_lhs, add_rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, add_lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, add_rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Eq { lhs, rhs },
                        condition_tmp: Some(instruction.result),
                        lhs: add_lhs,
                        rhs: add_rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessEq {
                        lhs,
                        rhs,
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::JmpZ_Eq_CvConst => {
                let dead_branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Unused
                    || dead_branch.opcode != OpCode::JmpZ
                    || dead_branch.op1_type != OpType::Tmp
                    || dead_branch.op2_type != OpType::Unused
                    || dead_branch.op2 != instruction.result
                {
                    return None;
                }
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;

                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, instruction.result as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Eq {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: None,
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessEq {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: None,
                        false_target: QuickLongTarget::unresolved(instruction.result as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::IsSmallerOrEqual => {
                let lhs = quick_long_operand(op_array, instruction.op1_type, instruction.op1)?;
                let rhs = quick_long_operand(op_array, instruction.op2_type, instruction.op2)?;
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                let op = QuickLongOp::BranchUnlessLe {
                    lhs,
                    rhs,
                    condition_tmp: Some(instruction.result),
                    false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                    next_target: QuickLongTarget::unresolved(ip + 2)?,
                    resume_ip: ip,
                };
                ip += 2;
                op
            }
            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                if !closed_loop {
                    return None;
                }
                let lhs = quick_long_operand(op_array, instruction.op1_type, instruction.op1)?;
                let rhs = quick_long_operand(op_array, instruction.op2_type, instruction.op2)?;
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                let expected = match branch.opcode {
                    OpCode::JmpZ => false,
                    OpCode::JmpNZ => true,
                    _ => return None,
                };
                let target_ip = branch.op2 as usize;
                // The selected edge must skip at least one cold instruction
                // and rejoin this closed loop before its baseline backedge.
                if target_ip <= ip + 2 || target_ip >= backedge_ip {
                    return None;
                }
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                let resume_ip = ip;
                ip = target_ip;
                QuickLongOp::TraceGuard {
                    kind: match instruction.opcode {
                        OpCode::IsIdentical => ScalarLongConditionKind::Equal,
                        OpCode::IsNotIdentical => ScalarLongConditionKind::NotEqual,
                        _ => unreachable!(),
                    },
                    lhs,
                    rhs,
                    expected,
                    condition_tmp: Some(instruction.result),
                    next_target: QuickLongTarget::unresolved(target_ip)?,
                    resume_ip,
                }
            }
            OpCode::Add
                if long_slot(instruction.op1_type, instruction.op1).is_none()
                    || long_slot(instruction.op2_type, instruction.op2).is_none() =>
            {
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let lhs = quick_long_operand(op_array, instruction.op1_type, instruction.op1)?;
                let rhs = quick_long_operand(op_array, instruction.op2_type, instruction.op2)?;
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                has_add = true;
                let resume_ip = ip;
                let destination = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                    .and_then(|(destination, source)| {
                        (source == instruction.result).then_some(destination)
                    });
                if let Some(destination) = destination {
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    ip += 2;
                    QuickLongOp::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs,
                        rhs,
                        result: instruction.result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs,
                        rhs,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::Sub
            | OpCode::Sub_CvConst
            | OpCode::Sub_TmpTmp
            | OpCode::Mul
            | OpCode::BitwiseAnd
            | OpCode::BitwiseAnd_LongLong
            | OpCode::BitwiseOr
            | OpCode::BitwiseOr_LongLong
            | OpCode::BitwiseXor
            | OpCode::BitwiseXor_LongLong => {
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let lhs = quick_long_operand(op_array, instruction.op1_type, instruction.op1)?;
                let rhs = quick_long_operand(op_array, instruction.op2_type, instruction.op2)?;
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let kind = match instruction.opcode {
                    OpCode::Mul => ScalarLongOpKind::Multiply,
                    OpCode::BitwiseAnd | OpCode::BitwiseAnd_LongLong => {
                        ScalarLongOpKind::BitwiseAnd
                    }
                    OpCode::BitwiseOr | OpCode::BitwiseOr_LongLong => ScalarLongOpKind::BitwiseOr,
                    OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => {
                        ScalarLongOpKind::BitwiseXor
                    }
                    _ => ScalarLongOpKind::Subtract,
                };
                let resume_ip = ip;
                let destination = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                    .and_then(|(destination, source)| {
                        (source == instruction.result).then_some(destination)
                    });
                if let Some(destination) = destination {
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    ip += 2;
                    QuickLongOp::BinaryAssign {
                        kind,
                        lhs,
                        rhs,
                        result: instruction.result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Binary {
                        kind,
                        lhs,
                        rhs,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::ShiftLeft | OpCode::ShiftRight => {
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let lhs = quick_long_operand(op_array, instruction.op1_type, instruction.op1)?;
                let rhs = quick_long_operand(op_array, instruction.op2_type, instruction.op2)?;
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::Shift {
                    left: instruction.opcode == OpCode::ShiftLeft,
                    lhs,
                    rhs,
                    result: instruction.result,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                let (lhs, rhs, result) = long_add(instruction)?;
                add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                add_mask_slot(&mut long_output_mask, result, total_slots)?;
                has_add = true;

                if let (
                    Some((second_lhs, second_rhs, second_result)),
                    Some((destination, source)),
                ) = (
                    op_array
                        .instructions
                        .get(ip + 1)
                        .copied()
                        .and_then(long_add),
                    op_array
                        .instructions
                        .get(ip + 2)
                        .copied()
                        .and_then(long_assign),
                ) {
                    if source != second_result {
                        return None;
                    }
                    for input in [second_lhs, second_rhs] {
                        if input != result {
                            add_mask_slot(&mut long_input_mask, input, total_slots)?;
                        }
                    }
                    add_mask_slot(&mut long_output_mask, second_result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    let first_resume_ip = ip;
                    ip += 3;
                    QuickLongOp::AddAddAssign {
                        first_lhs: lhs,
                        first_rhs: rhs,
                        first_result: result,
                        second_lhs,
                        second_rhs,
                        second_result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        first_resume_ip,
                        second_resume_ip: first_resume_ip + 1,
                    }
                } else if let Some((destination, source)) = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                {
                    if source != result {
                        return None;
                    }
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    let add_resume_ip = ip;
                    ip += 2;
                    QuickLongOp::AddAssign {
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        add_resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Add {
                        lhs,
                        rhs,
                        result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip: ip - 1,
                    }
                }
            }
            OpCode::NewObj => {
                if instruction._pad & crate::vm::instruction::NEW_FLAG_VIRTUAL_DECLARED_READS != 0 {
                    let next_ip = detect_virtual_declared_object_read_span(op_array, ip)?;
                    if next_ip <= ip || next_ip > backedge_ip {
                        return None;
                    }
                    let constructor_do_ip = ip + 1;
                    let object_assign_ip = constructor_do_ip + 1;
                    let (first_read_ip, _) = after_optional_assignment_release(
                        op_array,
                        object_assign_ip,
                        instruction.result_type,
                        instruction.result,
                    )?;
                    let read_count = next_ip.checked_sub(first_read_ip)?;
                    if read_count == 0 || read_count > 8 {
                        return None;
                    }
                    let mut reads = [QuickVirtualDeclaredPropertyRead::EMPTY; 8];
                    let mut output_mask = 0u64;
                    for (index, read) in reads.iter_mut().enumerate().take(read_count) {
                        let fetch = *op_array.instructions.get(first_read_ip + index)?;
                        add_mask_slot(&mut long_output_mask, fetch.result, total_slots)?;
                        output_mask |= 1u64.checked_shl(u32::from(fetch.result))?;
                        *read = QuickVirtualDeclaredPropertyRead {
                            property_literal: fetch.op2,
                            result: fetch.result,
                        };
                    }
                    has_object_call = true;
                    let resume_ip = ip;
                    ip = next_ip;
                    QuickLongOp::VirtualDeclaredObjectReads {
                        class_literal: instruction.op1,
                        reads,
                        read_count: read_count as u8,
                        output_mask,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    if instruction._pad
                        & crate::vm::instruction::NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE
                        == 0
                    {
                        return None;
                    }
                    let next_ip = detect_virtual_object_array_pipeline_span(op_array, ip)?;
                    if next_ip <= ip || next_ip > backedge_ip {
                        return None;
                    }

                    let mut constructor_arguments =
                        [QuickVirtualValueSource::Long(QuickLongOperand::Const(0)); 8];
                    for (index, argument) in constructor_arguments
                        .iter_mut()
                        .enumerate()
                        .take(instruction.extended_value as usize)
                    {
                        let send = *op_array.instructions.get(ip + 1 + index)?;
                        *argument = match send.op1_type {
                            OpType::Cv | OpType::Tmp => {
                                let bit = 1u64.checked_shl(u32::from(send.op1))?;
                                if string_key_assignment_mask & bit != 0 {
                                    add_mask_slot(&mut string_input_mask, send.op1, total_slots)?;
                                    QuickVirtualValueSource::StringSlot(send.op1)
                                } else {
                                    add_mask_slot(&mut long_input_mask, send.op1, total_slots)?;
                                    QuickVirtualValueSource::Long(QuickLongOperand::Slot(send.op1))
                                }
                            }
                            OpType::Const => {
                                let value = op_array.literals.get(send.op1 as usize)?;
                                if let Some(value) = value.as_long() {
                                    QuickVirtualValueSource::Long(QuickLongOperand::Const(value))
                                } else if value.as_str().is_some() {
                                    QuickVirtualValueSource::StringLiteral(send.op1)
                                } else {
                                    return None;
                                }
                            }
                            _ => return None,
                        };
                    }

                    let constructor_do_ip = ip + 1 + instruction.extended_value as usize;
                    let object_assign_ip = constructor_do_ip + 1;
                    let (method_ip, _) = after_optional_assignment_release(
                        op_array,
                        object_assign_ip,
                        instruction.result_type,
                        instruction.result,
                    )?;
                    let method = *op_array.instructions.get(method_ip)?;
                    if method.opcode != OpCode::InitMethodCall
                        || method.op1_type != OpType::Cv
                        || method.extended_value != 1
                    {
                        return None;
                    }
                    add_mask_slot(&mut object_input_mask, method.op1, total_slots)?;

                    let method_do_ip = method_ip + 1 + method.extended_value as usize;
                    let result_assign_ip = method_do_ip + 1;
                    let method_do = *op_array.instructions.get(method_do_ip)?;
                    let (mut cursor, _) = after_optional_assignment_release(
                        op_array,
                        result_assign_ip,
                        method_do.result_type,
                        method_do.result,
                    )?;
                    let mut output_mask = 0u64;
                    let mut consumer_count = 0usize;
                    let mut consumers = [QuickObjectArrayConsumer::EMPTY; 4];
                    let mut trailing_key_literal = None;
                    let mut trailing_result = 0;
                    while cursor < next_ip {
                        let fetch = *op_array.instructions.get(cursor)?;
                        if fetch.opcode == OpCode::ReleaseTemps && cursor + 1 == next_ip {
                            cursor = next_ip;
                            break;
                        }
                        if fetch.opcode != OpCode::FetchDimR {
                            return None;
                        }
                        let add = op_array.instructions.get(cursor + 1).copied();
                        let assign = op_array.instructions.get(cursor + 2).copied();
                        if let (Some(add), Some(assign)) = (add, assign)
                            && let Some(accumulator) = object_array_add_consumer(fetch, add, assign)
                        {
                            add_mask_slot(&mut long_input_mask, accumulator, total_slots)?;
                            add_mask_slot(&mut long_output_mask, accumulator, total_slots)?;
                            output_mask |= 1u64 << accumulator;
                            *consumers.get_mut(consumer_count)? = QuickObjectArrayConsumer {
                                key_literal: fetch.op2,
                                accumulator,
                            };
                            consumer_count += 1;
                            cursor += 3;
                            if cursor < next_ip
                                && op_array.instructions.get(cursor).is_some_and(|instruction| {
                                    instruction.opcode == OpCode::ReleaseTemps
                                })
                            {
                                cursor += 1;
                            }
                        } else {
                            add_mask_slot(&mut long_output_mask, fetch.result, total_slots)?;
                            output_mask |= 1u64 << fetch.result;
                            trailing_key_literal = Some(fetch.op2);
                            trailing_result = fetch.result;
                            cursor += 1;
                        }
                    }
                    if cursor != next_ip || consumer_count == 0 {
                        return None;
                    }
                    has_object_call = true;
                    let resume_ip = ip;
                    ip = next_ip;
                    QuickLongOp::VirtualObjectArrayPipeline {
                        constructor_arguments,
                        argument_count: instruction.extended_value as u8,
                        consumers,
                        consumer_count: consumer_count as u8,
                        trailing_key_literal,
                        trailing_result,
                        output_mask,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::FetchObjR => {
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    || op_array
                        .literals
                        .get(instruction.op2 as usize)
                        .and_then(Value::as_str)
                        .is_none()
                    || !cv_unmodified_in_region(region, instruction.op1)
                {
                    return None;
                }
                add_mask_slot(&mut object_input_mask, instruction.op1, total_slots)?;
                has_object_call = true;
                let cache_ip = u32::try_from(ip).ok()?;
                let resume_ip = ip;
                let strlen = op_array.instructions.get(ip + 1).copied();
                if let Some(strlen) = strlen
                    && matches!(strlen.opcode, OpCode::Strlen | OpCode::Strlen_String)
                    && strlen.op1_type == instruction.result_type
                    && strlen.op1 == instruction.result
                    && matches!(strlen.result_type, OpType::Tmp | OpType::Var)
                {
                    add_mask_slot(&mut long_output_mask, strlen.result, total_slots)?;
                    ip += 2;
                    QuickLongOp::ObjectPropertyStringLength {
                        object: instruction.op1,
                        cache_ip,
                        result: strlen.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                    ip += 1;
                    QuickLongOp::ObjectPropertyLong {
                        object: instruction.op1,
                        cache_ip,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::InitMethodCall => {
                if instruction.op1_type != OpType::Cv {
                    return None;
                }
                let cache_ip = u32::try_from(ip).ok()?;
                let outer_guard = ScalarLongCallGuard::MethodCache {
                    cache_ip,
                    receiver_slot: instruction.op1,
                };
                add_mask_slot(&mut object_input_mask, instruction.op1, total_slots)?;

                let nested = if instruction.extended_value == 1 {
                    let inner_init = *op_array.instructions.get(ip + 1)?;
                    let inner_do = *op_array.instructions.get(ip + 2)?;
                    let send = *op_array.instructions.get(ip + 3)?;
                    let outer_do = *op_array.instructions.get(ip + 4)?;
                    if inner_init.opcode == OpCode::InitMethodCall
                        && inner_init.op1_type == OpType::Cv
                        && inner_init.extended_value == 0
                        && inner_do.opcode == OpCode::DoFcall
                        && matches!(inner_do.result_type, OpType::Tmp | OpType::Var)
                        && matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                        && send.op1_type == inner_do.result_type
                        && send.op1 == inner_do.result
                        && send.op2 == 1
                        && outer_do.opcode == OpCode::DoFcall
                        && outer_do.result_type == OpType::Unused
                    {
                        Some((inner_init, inner_do, outer_do))
                    } else {
                        None
                    }
                } else {
                    None
                };

                has_object_call = true;
                if let Some((inner_init, _inner_do, _outer_do)) = nested {
                    add_mask_slot(&mut object_input_mask, inner_init.op1, total_slots)?;
                    let resume_ip = ip;
                    let inner_guard = ScalarLongCallGuard::MethodCache {
                        cache_ip: u32::try_from(ip + 1).ok()?,
                        receiver_slot: inner_init.op1,
                    };
                    ip += 5;
                    QuickLongOp::ComposedPropertyCall {
                        outer_guard,
                        inner_guard,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    let argument_count = usize::try_from(instruction.extended_value).ok()?;
                    if argument_count > 8 {
                        return None;
                    }
                    let mut arguments = [QuickLongOperand::Const(0); 8];
                    let mut object_long_arguments =
                        [QuickObjectLongArgument::Long(QuickLongOperand::Const(0)); 8];
                    let mut has_string_argument = false;
                    let mut cursor = ip + 1;
                    for argument_index in 0..argument_count {
                        let send = json_projections.deferred_argument_send(
                            op_array,
                            &mut cursor,
                            total_slots,
                        )?;
                        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                            || send.op2 as usize != argument_index + 1
                        {
                            return None;
                        }
                        if send.op1_type == OpType::Cv
                            && string_key_assignment_mask & (1u64 << send.op1) != 0
                        {
                            add_mask_slot(&mut string_input_mask, send.op1, total_slots)?;
                            object_long_arguments[argument_index] =
                                QuickObjectLongArgument::StringSlot(send.op1);
                            has_string_argument = true;
                            cursor += 1;
                            continue;
                        }
                        let argument = match send.op1_type {
                            OpType::Cv | OpType::Tmp | OpType::Var => {
                                add_mask_slot(&mut long_input_mask, send.op1, total_slots)?;
                                QuickLongOperand::Slot(send.op1)
                            }
                            OpType::Const => {
                                QuickLongOperand::Const(long_literal(op_array, send.op1)?)
                            }
                            OpType::Unused => return None,
                        };
                        arguments[argument_index] = argument;
                        object_long_arguments[argument_index] =
                            QuickObjectLongArgument::Long(argument);
                        cursor += 1;
                    }
                    let do_fcall = *op_array.instructions.get(cursor)?;
                    if do_fcall.opcode != OpCode::DoFcall {
                        return None;
                    }
                    let destination = matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                        .then(|| {
                            op_array
                                .instructions
                                .get(cursor + 1)
                                .copied()
                                .and_then(long_assign)
                                .and_then(|(destination, source)| {
                                    (source == do_fcall.result).then_some(destination)
                                })
                        })
                        .flatten();
                    let resume_ip = ip;
                    ip = cursor + 1;
                    if destination.is_some() {
                        has_assign = true;
                        ip += 1;
                    }
                    // A consumed DoFcall TMP is dead after the skipped
                    // canonical Assign. Write the quick result directly into
                    // that CV so the hot opcode stays compact.
                    let call_result = destination.unwrap_or(do_fcall.result);
                    let call = QuickTypedMethodCall {
                        guard: outer_guard,
                        arguments,
                        argument_count: argument_count as u8,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    };
                    if has_string_argument
                        && argument_count != 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(&mut long_output_mask, call_result, total_slots)?;
                        QuickLongOp::ObjectLongMethodCall {
                            call: QuickObjectLongMethodCall {
                                guard: outer_guard,
                                arguments: object_long_arguments,
                                argument_count: argument_count as u8,
                                next_target: call.next_target,
                                resume_ip,
                            },
                            result: call_result,
                        }
                    } else if do_fcall.result_type == OpType::Unused {
                        QuickLongOp::PropertyMethodCall { call }
                    } else if argument_count == 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(&mut long_output_mask, call_result, total_slots)?;
                        QuickLongOp::PropertyGetterCall {
                            call,
                            result: call_result,
                        }
                    } else if argument_count != 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(&mut long_output_mask, call_result, total_slots)?;
                        QuickLongOp::ScalarMethodCall {
                            call,
                            result: call_result,
                        }
                    } else {
                        return None;
                    }
                }
            }
            #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
            OpCode::DoFcall => {
                let pending = pending_indirect_call.take()?;
                if pending.next_argument != pending.outer_argument_count
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                has_object_call = true;
                let operation_ip = ip;
                ip += 1;
                QuickLongOp::IndirectScalarFunctionCall {
                    call: QuickIndirectScalarCall {
                        leaf: QuickTypedMethodCall {
                            guard: pending.guard,
                            arguments: pending.arguments,
                            argument_count: pending.outer_argument_count - 1,
                            next_target: QuickLongTarget::unresolved(ip)?,
                            resume_ip: pending.resume_ip,
                        },
                        callable_slot: pending.callable_slot,
                        outer_argument_count: pending.outer_argument_count,
                        operation_ip,
                    },
                    result: instruction.result,
                }
            }
            OpCode::AssignConcat => {
                if instruction.op1_type != OpType::Cv || instruction.result_type != OpType::Unused {
                    return None;
                }
                let source = match instruction.op2_type {
                    OpType::Const => {
                        op_array.literals.get(instruction.op2 as usize)?.as_str()?;
                        QuickStringAppendSource::Literal(instruction.op2)
                    }
                    OpType::Cv
                        if instruction.op2 != instruction.op1
                            && cv_unmodified_in_region(region, instruction.op2) =>
                    {
                        add_mask_slot(&mut string_input_mask, instruction.op2, total_slots)?;
                        QuickStringAppendSource::Slot(instruction.op2)
                    }
                    _ => return None,
                };
                add_mask_slot(&mut string_append_mask, instruction.op1, total_slots)?;
                has_string_append = true;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::StringAppend {
                    destination: instruction.op1,
                    source,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::AssignCv => {
                if instruction.op1_type != OpType::Cv || instruction.result_type != OpType::Unused {
                    return None;
                }
                if string_key_assignment_mask & (1u64 << instruction.op1) != 0 {
                    add_mask_slot(&mut string_output_mask, instruction.op1, total_slots)?;
                    has_assign = true;
                    ip += 1;
                    match instruction.op2_type {
                        OpType::Const => QuickLongOp::AssignStringLiteral {
                            destination: instruction.op1,
                            literal: instruction.op2,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        },
                        OpType::Cv => {
                            add_mask_slot(&mut string_input_mask, instruction.op2, total_slots)?;
                            QuickLongOp::AssignStringSlot {
                                destination: instruction.op1,
                                source: instruction.op2,
                                next_target: QuickLongTarget::unresolved(ip)?,
                            }
                        }
                        _ => return None,
                    }
                } else {
                    add_mask_slot(&mut long_output_mask, instruction.op1, total_slots)?;
                    has_assign = true;
                    ip += 1;
                    if instruction.op2_type == OpType::Const {
                        QuickLongOp::AssignLongLiteral {
                            destination: instruction.op1,
                            value: long_literal(op_array, instruction.op2)?,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        }
                    } else {
                        let source = long_slot(instruction.op2_type, instruction.op2)?;
                        add_mask_slot(&mut long_input_mask, source, total_slots)?;
                        QuickLongOp::Assign {
                            destination: instruction.op1,
                            source,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        }
                    }
                }
            }
            OpCode::PostInc | OpCode::PreInc => {
                if instruction.op1_type != OpType::Cv {
                    return None;
                }
                let result = match instruction.result_type {
                    OpType::Unused => None,
                    OpType::Tmp if instruction.opcode == OpCode::PostInc => {
                        Some(instruction.result)
                    }
                    _ => return None,
                };
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut long_output_mask, instruction.op1, total_slots)?;
                if let Some(result) = result {
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                }
                has_post_inc = true;
                let resume_ip = ip;
                if closed_loop && ip + 1 == backedge_ip {
                    let jump = op_array.instructions[backedge_ip];
                    if !matches!(jump.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                        || jump.op1 as usize != header_ip
                    {
                        return None;
                    }
                    ip += 2;
                    QuickLongOp::PostIncJump {
                        value: instruction.op1,
                        result,
                        target: QuickLongTarget::unresolved(header_ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::PostInc {
                        value: instruction.op1,
                        result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::Jmp | OpCode::QuickLongLoopJmp => {
                let target_ip = instruction.op1 as usize;
                if closed_loop && ip == backedge_ip {
                    if target_ip != header_ip {
                        return None;
                    }
                } else if target_ip <= ip {
                    return None;
                }
                ip += 1;
                QuickLongOp::Jump {
                    target: QuickLongTarget::unresolved(target_ip)?,
                }
            }
            _ => return None,
        };

        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        let mut op = op;
        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        if let Some(pending) = pending_indirect_call {
            match &mut op {
                QuickLongOp::Binary { resume_ip, .. } => *resume_ip = pending.resume_ip,
                _ => return None,
            }
        }

        let op_ip = match op {
            QuickLongOp::BranchUnlessLt { resume_ip, .. }
            | QuickLongOp::BranchUnlessEq { resume_ip, .. }
            | QuickLongOp::BranchUnlessLe { resume_ip, .. }
            | QuickLongOp::TraceGuard { resume_ip, .. }
            | QuickLongOp::ModConst { resume_ip, .. }
            | QuickLongOp::JsonProjectionStep { resume_ip, .. }
            | QuickLongOp::FetchArrayLong { resume_ip, .. }
            | QuickLongOp::ObjectPropertyLong { resume_ip, .. }
            | QuickLongOp::ObjectPropertyStringLength { resume_ip, .. }
            | QuickLongOp::StoreArrayLong { resume_ip, .. }
            | QuickLongOp::SetArrayLong { resume_ip, .. }
            | QuickLongOp::ArrayPushLong { resume_ip, .. }
            | QuickLongOp::StringAppend { resume_ip, .. }
            | QuickLongOp::Add { resume_ip, .. }
            | QuickLongOp::Binary { resume_ip, .. }
            | QuickLongOp::Shift { resume_ip, .. }
            | QuickLongOp::BinaryAssign { resume_ip, .. }
            | QuickLongOp::ComposedPropertyCall { resume_ip, .. }
            | QuickLongOp::VirtualObjectArrayPipeline { resume_ip, .. }
            | QuickLongOp::VirtualDeclaredObjectReads { resume_ip, .. }
            | QuickLongOp::PostInc { resume_ip, .. }
            | QuickLongOp::PostIncJump { resume_ip, .. }
            | QuickLongOp::PostIncLoopLt { resume_ip, .. } => resume_ip,
            QuickLongOp::PropertyMethodCall { call }
            | QuickLongOp::PropertyGetterCall { call, .. }
            | QuickLongOp::ScalarMethodCall { call, .. } => call.resume_ip,
            #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
            QuickLongOp::IndirectScalarFunctionCall { call, .. } => call.operation_ip,
            QuickLongOp::ObjectLongMethodCall { call, .. } => call.resume_ip,
            QuickLongOp::AddAssign { add_resume_ip, .. } => add_resume_ip,
            QuickLongOp::ConditionalAddAssign {
                condition_resume_ip,
                ..
            } => condition_resume_ip,
            QuickLongOp::AddAddAssign {
                first_resume_ip, ..
            } => first_resume_ip,
            QuickLongOp::Assign { .. } => ip - 1,
            QuickLongOp::AssignLongLiteral { .. } => ip - 1,
            QuickLongOp::AssignStringLiteral { .. } => ip - 1,
            QuickLongOp::AssignStringSlot { .. } => ip - 1,
            QuickLongOp::Jump { .. } => ip - 1,
        };
        let relative = op_ip - header_ip;
        if ip_to_op[relative] != u16::MAX || ops.len() >= u16::MAX as usize {
            return None;
        }
        let op_index = ops.len() as u16;
        ip_to_op[relative] = op_index;
        for passthrough_ip in passthrough_ips.drain(..) {
            ip_to_op[passthrough_ip - header_ip] = op_index;
        }
        op_ips.push(u32::try_from(op_ip).ok()?);
        ops.push(op);
    }

    if !passthrough_ips.is_empty() {
        return None;
    }
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    if closure_alias.is_some() || pending_indirect_call.is_some() {
        return None;
    }

    if closed_loop
        && let (
            Some(QuickLongOp::BranchUnlessLt {
                lhs,
                rhs,
                condition_tmp,
                false_target,
                next_target,
                ..
            }),
            Some(QuickLongOp::PostIncJump {
                value,
                result,
                target,
                resume_ip,
            }),
        ) = (ops.first().copied(), ops.last().copied())
    {
        if target.unresolved_ip()? == header_ip {
            *ops.last_mut()? = QuickLongOp::PostIncLoopLt {
                value,
                result,
                condition_lhs: lhs,
                condition_rhs: rhs,
                condition_tmp,
                body_target: next_target,
                exit_target: false_target,
                resume_ip,
            };
        }
    }

    let has_internal_branch = ops.iter().skip(1).any(|op| {
        matches!(
            op,
            QuickLongOp::BranchUnlessLt { .. }
                | QuickLongOp::BranchUnlessEq { .. }
                | QuickLongOp::BranchUnlessLe { .. }
                | QuickLongOp::TraceGuard { .. }
        )
    });
    if closed_loop {
        if !(has_add
            || has_assign
            || has_internal_branch
            || has_object_call
            || has_array_push
            || has_string_append)
            || !has_post_inc
            || !matches!(
                ops.first(),
                Some(QuickLongOp::BranchUnlessLt { false_target, .. })
                    if matches!(false_target.unresolved_ip(), Some(ip) if ip > backedge_ip)
            )
            || !(matches!(
                ops.last(),
                Some(QuickLongOp::Jump { target } | QuickLongOp::PostIncJump { target, .. })
                    if target.unresolved_ip() == Some(header_ip)
            ) || matches!(ops.last(), Some(QuickLongOp::PostIncLoopLt { .. })))
        {
            return None;
        }
    } else {
        if ops.len() < 2 || !(has_add || has_assign) {
            return None;
        }
        long_input_mask = straight_long_region_inputs(&ops)?;
    }

    let entry_op = ip_to_op.first().copied()?;
    if entry_op == u16::MAX {
        return None;
    }
    for op in &mut ops {
        op.resolve_targets(
            header_ip,
            backedge_ip,
            op_array.instructions.len(),
            &ip_to_op,
        )?;
    }

    // Compiler temporaries are single-definition values inside this OpArray.
    // If this region produces a TMP, every valid use is dominated by that
    // definition and the stale frame representation is not an entry input.
    // Keep CV read/write overlap intact because CVs carry loop state.
    let cv_mask = if op_array.num_cvs == 64 {
        u64::MAX
    } else {
        (1u64 << op_array.num_cvs) - 1
    };
    long_input_mask &= !(long_output_mask & !cv_mask);

    if let Some(source) = typed_invariant_source.as_mut() {
        json_projections.retain_projections(source, long_input_mask)?;
    }

    let long_mask = long_input_mask | long_output_mask;
    if long_mask & bool_output_mask != 0
        || set_array_output_mask & array_input_mask != 0
        || array_input_mask & (long_mask | bool_output_mask) != 0
        || array_output_mask & (long_mask | bool_output_mask) != 0
        || string_input_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_append_mask)
            != 0
        || string_output_mask & !string_input_mask != 0
        || string_append_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_output_mask)
            != 0
        || object_input_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_input_mask
                | string_append_mask)
            != 0
    {
        return None;
    }
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    if closure_input_mask
        & (long_mask
            | bool_output_mask
            | array_input_mask
            | array_output_mask
            | string_input_mask
            | string_append_mask
            | object_input_mask)
        != 0
    {
        return None;
    }
    let involved_mask = long_input_mask
        | long_output_mask
        | bool_output_mask
        | array_input_mask
        | array_output_mask
        | string_input_mask
        | string_output_mask
        | string_append_mask
        | object_input_mask;
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    let involved_mask = involved_mask | closure_input_mask;

    let array_update_fusions = detect_array_update_fusions(&ops);
    let mut plan = QuickLongOpsLoop {
        header_ip,
        backedge_ip,
        ops,
        array_update_fusions,
        entry_op,
        op_ips,
        long_input_mask,
        long_output_mask,
        bool_output_mask,
        array_input_mask,
        array_output_mask,
        structural_array_output_mask,
        string_input_mask,
        string_output_mask,
        string_append_mask,
        finite_string_literals,
        finite_string_literal_count: finite_string_literal_count as u8,
        finite_string_literal_overflow,
        object_input_mask,
        #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
        closure_input_mask,
        typed_invariant_source,
        string_cache_capacity: string_cache_capacity as u8,
        involved_mask,
        straight_array_kernel: None,
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        native_jit: crate::jit::QuickLongOpsJitCache::new(),
    };
    plan.straight_array_kernel = detect_straight_array_region_kernel(&plan);
    Some(plan)
}
