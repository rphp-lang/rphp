fn is_existing_array_long_replacement(
    ops: &[QuickLongOp],
    array: u16,
    index: QuickArrayIndex,
    value: u16,
) -> bool {
    let [.., fetch, arithmetic] = ops else {
        return false;
    };
    let QuickLongOp::FetchArrayLong {
        array: fetch_array,
        index: fetch_index,
        result: fetch_result,
        ..
    } = *fetch
    else {
        return false;
    };
    let (arithmetic_result, consumes_fetch) = match *arithmetic {
        QuickLongOp::Add {
            lhs, rhs, result, ..
        } => (result, lhs == fetch_result || rhs == fetch_result),
        QuickLongOp::Binary {
            lhs, rhs, result, ..
        } => (
            result,
            lhs == QuickLongOperand::Slot(fetch_result)
                || rhs == QuickLongOperand::Slot(fetch_result),
        ),
        _ => return false,
    };
    fetch_array == array
        && fetch_index == index
        && arithmetic_result == value
        && consumes_fetch
}

fn detect_array_update_fusions(ops: &[QuickLongOp]) -> Vec<Option<QuickArrayUpdateFusion>> {
    let mut fusions = vec![None; ops.len()];
    for index in 0..ops.len().saturating_sub(2) {
        let QuickLongOp::FetchArrayLong {
            array,
            index: array_index,
            result: fetch_result,
            next_target: fetch_next,
            ..
        } = ops[index]
        else {
            continue;
        };
        if fetch_next.op_index() != Some(index + 1)
            || ops.iter().any(|operation| {
                matches!(operation, QuickLongOp::ArrayPushLong { array: pushed, .. } if *pushed == array)
                    || matches!(operation, QuickLongOp::SetArrayLong { array: set, .. } if *set == array)
            })
        {
            continue;
        }

        let (kind, lhs, rhs, result, arithmetic_next, arithmetic_resume_ip) = match ops[index + 1] {
            QuickLongOp::Add {
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => (
                ScalarLongOpKind::Add,
                QuickLongOperand::Slot(lhs),
                QuickLongOperand::Slot(rhs),
                result,
                next_target,
                resume_ip,
            ),
            QuickLongOp::Binary {
                kind,
                lhs,
                rhs,
                result,
                next_target,
                resume_ip,
            } => (kind, lhs, rhs, result, next_target, resume_ip),
            _ => continue,
        };
        if arithmetic_next.op_index() != Some(index + 2)
            || !matches!(lhs, QuickLongOperand::Slot(slot) if slot == fetch_result)
                && !matches!(rhs, QuickLongOperand::Slot(slot) if slot == fetch_result)
        {
            continue;
        }

        let QuickLongOp::StoreArrayLong {
            array: store_array,
            index: store_index,
            value,
            next_target,
            ..
        } = ops[index + 2]
        else {
            continue;
        };
        if store_array != array || store_index != array_index || value != result {
            continue;
        }
        fusions[index] = Some(QuickArrayUpdateFusion {
            kind,
            lhs,
            rhs,
            result,
            next_target,
            arithmetic_resume_ip,
        });
    }
    fusions
}

pub const QUICK_STRAIGHT_ARRAY_MAX_ADDS: usize = 4;

fn instruction_mentions_operand(
    instruction: &crate::vm::instruction::Instruction,
    op_type: OpType,
    slot: u16,
) -> bool {
    (instruction.op1_type == op_type && instruction.op1 == slot)
        || (instruction.op2_type == op_type && instruction.op2 == slot)
        || (instruction.result_type == op_type && instruction.result == slot)
}

fn object_array_add_consumer(
    fetch: crate::vm::instruction::Instruction,
    add: crate::vm::instruction::Instruction,
    assign: crate::vm::instruction::Instruction,
) -> Option<u16> {
    if !matches!(
        add.opcode,
        OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
    ) || !matches!(add.result_type, OpType::Tmp | OpType::Var)
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != add.result_type
        || assign.op2 != add.result
        || assign.result_type != OpType::Unused
    {
        return None;
    }
    let accumulator = if add.op1_type == OpType::Cv
        && add.op2_type == fetch.result_type
        && add.op2 == fetch.result
    {
        add.op1
    } else if add.op2_type == OpType::Cv
        && add.op1_type == fetch.result_type
        && add.op1 == fetch.result
    {
        add.op2
    } else {
        return None;
    };
    (assign.op1 == accumulator).then_some(accumulator)
}

/// Prove an immediate scalar-consumer span for a method's small associative
/// array result. The assigned array CV must have no other syntactic use in the
/// function, which makes non-materialization unobservable for the admitted
/// Long-only ObjectArrayFunctionPlan result.
pub fn detect_object_array_consumer_span(op_array: &OpArray, init_ip: usize) -> Option<usize> {
    let initializer = *op_array.instructions.get(init_ip)?;
    if initializer.opcode != OpCode::InitMethodCall {
        return None;
    }
    let do_fcall_ip = init_ip
        .checked_add(1)?
        .checked_add(initializer.extended_value as usize)?;
    let do_fcall = *op_array.instructions.get(do_fcall_ip)?;
    let assign_ip = do_fcall_ip + 1;
    let assign = *op_array.instructions.get(assign_ip)?;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != do_fcall.result_type
        || assign.op2 != do_fcall.result
        || assign.result_type != OpType::Unused
    {
        return None;
    }

    let array_cv = assign.op1;
    let mut fetch_ips = [usize::MAX; QUICK_STRAIGHT_ARRAY_MAX_ADDS];
    let mut fetch_count = 0usize;
    let mut add_count = 0usize;
    let mut cursor = assign_ip + 1;
    while fetch_count < fetch_ips.len() {
        let Some(fetch) = op_array.instructions.get(cursor).copied() else {
            break;
        };
        if fetch.opcode != OpCode::FetchDimR
            || fetch.op1_type != OpType::Cv
            || fetch.op1 != array_cv
            || fetch.op2_type != OpType::Const
            || !matches!(fetch.result_type, OpType::Tmp | OpType::Var)
            || op_array
                .literals
                .get(fetch.op2 as usize)
                .and_then(Value::as_str)
                .is_none()
        {
            break;
        }
        fetch_ips[fetch_count] = cursor;
        fetch_count += 1;

        let add = op_array.instructions.get(cursor + 1).copied();
        let assign = op_array.instructions.get(cursor + 2).copied();
        if let (Some(add), Some(assign)) = (add, assign)
            && object_array_add_consumer(fetch, add, assign).is_some()
        {
            add_count += 1;
            cursor += 3;
            continue;
        }

        // One final fetch may feed arbitrary canonical scalar bytecode. The
        // fast path materializes that Long TMP and resumes immediately after
        // the fetch.
        cursor += 1;
        break;
    }
    if add_count == 0 || fetch_count == 0 || cursor >= op_array.instructions.len() {
        return None;
    }

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if ip == assign_ip || fetch_ips[..fetch_count].contains(&ip) {
            continue;
        }
        if instruction_mentions_operand(instruction, OpType::Cv, array_cv) {
            return None;
        }
        if ip != do_fcall_ip
            && ip != assign_ip
            && instruction_mentions_operand(instruction, do_fcall.result_type, do_fcall.result)
        {
            return None;
        }
    }

    Some(cursor)
}

/// Prove the caller-side escape shape for a constructor-initialized object
/// passed directly into an ObjectArray consumer call. Runtime supplies the
/// class, constructor-plan and declared-property guards that are unavailable
/// while this caller is compiled.
pub fn detect_virtual_object_array_pipeline_span(
    op_array: &OpArray,
    new_ip: usize,
) -> Option<usize> {
    let new_object = *op_array.instructions.get(new_ip)?;
    if new_object.opcode != OpCode::NewObj
        || new_object.op1_type != OpType::Const
        || !matches!(new_object.result_type, OpType::Tmp | OpType::Var)
        || new_object.extended_value == 0
        || new_object.extended_value > 8
        || op_array
            .literals
            .get(new_object.op1 as usize)
            .and_then(Value::as_str)
            .is_none()
    {
        return None;
    }
    let constructor_do_ip = new_ip + 1 + new_object.extended_value as usize;
    let constructor_do = *op_array.instructions.get(constructor_do_ip)?;
    let object_assign_ip = constructor_do_ip + 1;
    let object_assign = *op_array.instructions.get(object_assign_ip)?;
    if constructor_do.opcode != OpCode::DoFcall
        || object_assign.opcode != OpCode::AssignCv
        || object_assign.op1_type != OpType::Cv
        || object_assign.op2_type != new_object.result_type
        || object_assign.op2 != new_object.result
        || object_assign.result_type != OpType::Unused
    {
        return None;
    }

    for index in 0..new_object.extended_value as usize {
        let send = *op_array.instructions.get(new_ip + 1 + index)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            return None;
        }
    }

    let method_ip = object_assign_ip + 1;
    let method = *op_array.instructions.get(method_ip)?;
    if method.opcode != OpCode::InitMethodCall
        || method._pad & crate::vm::instruction::CALL_FLAG_OBJECT_ARRAY_CONSUMERS == 0
        || detect_object_array_consumer_span(op_array, method_ip).is_none()
    {
        return None;
    }
    let mut virtual_argument_sends = 0usize;
    let mut virtual_send_ip = usize::MAX;
    for index in 0..method.extended_value as usize {
        let send_ip = method_ip + 1 + index;
        let send = *op_array.instructions.get(send_ip)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            return None;
        }
        if send.op1_type == OpType::Cv && send.op1 == object_assign.op1 {
            virtual_argument_sends += 1;
            virtual_send_ip = send_ip;
        }
    }
    if virtual_argument_sends != 1 {
        return None;
    }

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if ip != new_ip
            && ip != object_assign_ip
            && instruction_mentions_operand(instruction, new_object.result_type, new_object.result)
        {
            return None;
        }
        if ip != object_assign_ip
            && ip != virtual_send_ip
            && instruction_mentions_operand(instruction, OpType::Cv, object_assign.op1)
        {
            return None;
        }
        if matches!(constructor_do.result_type, OpType::Tmp | OpType::Var)
            && ip != constructor_do_ip
            && instruction_mentions_operand(
                instruction,
                constructor_do.result_type,
                constructor_do.result,
            )
        {
            return None;
        }
    }

    detect_object_array_consumer_span(op_array, method_ip)
}

/// Prove `new LiteralClass() -> dead local -> declared read(s)` without using
/// class metadata that is unavailable while the caller is compiled. Runtime
/// still requires warmed negative-constructor and public-property caches, an
/// exact declared class, scalar defaults and no destructor before eliding the
/// canonical owner allocation.
pub fn detect_virtual_declared_object_read_span(
    op_array: &OpArray,
    new_ip: usize,
) -> Option<usize> {
    let new_object = *op_array.instructions.get(new_ip)?;
    if new_object.opcode != OpCode::NewObj
        || new_object.op1_type != OpType::Const
        || !matches!(new_object.result_type, OpType::Tmp | OpType::Var)
        || new_object.extended_value != 0
        || new_object._pad
            & (crate::vm::instruction::NEW_FLAG_DYNAMIC_STATIC_SCOPE
                | crate::vm::instruction::NEW_FLAG_DYNAMIC_CLASS_NAME)
            != 0
        || op_array
            .literals
            .get(new_object.op1 as usize)
            .and_then(Value::as_str)
            .is_none()
    {
        return None;
    }

    let constructor_do_ip = new_ip + 1;
    let constructor_do = *op_array.instructions.get(constructor_do_ip)?;
    let object_assign_ip = constructor_do_ip + 1;
    let object_assign = *op_array.instructions.get(object_assign_ip)?;
    if constructor_do.opcode != OpCode::DoFcall
        || object_assign.opcode != OpCode::AssignCv
        || object_assign.op1_type != OpType::Cv
        || object_assign.op2_type != new_object.result_type
        || object_assign.op2 != new_object.result
        || object_assign.result_type != OpType::Unused
    {
        return None;
    }

    let mut cursor = object_assign_ip + 1;
    let mut read_count = 0usize;
    while read_count < 8 {
        let Some(read) = op_array.instructions.get(cursor).copied() else {
            break;
        };
        if read.opcode != OpCode::FetchObjR
            || read.op1_type != OpType::Cv
            || read.op1 != object_assign.op1
            || read.op2_type != OpType::Const
            || !matches!(read.result_type, OpType::Tmp | OpType::Var)
            || read._pad & crate::vm::instruction::FETCH_OBJ_SILENT != 0
            || op_array
                .literals
                .get(read.op2 as usize)
                .and_then(Value::as_str)
                .is_none()
        {
            break;
        }
        read_count += 1;
        cursor += 1;
    }
    if read_count == 0 {
        return None;
    }

    // The assigned CV is the identity/escape boundary. Requiring its complete
    // OpArray use set to be exactly this span also excludes references,
    // comparisons, post-loop observation, dynamic-property access and calls.
    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if ip != new_ip
            && ip != object_assign_ip
            && instruction_mentions_operand(instruction, new_object.result_type, new_object.result)
        {
            return None;
        }
        let is_admitted_read = (object_assign_ip + 1..cursor).contains(&ip);
        if ip != object_assign_ip
            && !is_admitted_read
            && instruction_mentions_operand(instruction, OpType::Cv, object_assign.op1)
        {
            return None;
        }
        if matches!(constructor_do.result_type, OpType::Tmp | OpType::Var)
            && ip != constructor_do_ip
            && instruction_mentions_operand(
                instruction,
                constructor_do.result_type,
                constructor_do.result,
            )
        {
            return None;
        }
    }

    Some(cursor)
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayFetchAdd {
    pub index: QuickArrayIndex,
    pub fetch_result: u16,
    pub accumulator: u16,
    pub add_result: u16,
    pub fetch_resume_ip: usize,
    pub add_resume_ip: usize,
}

impl QuickStraightArrayFetchAdd {
    const EMPTY: Self = Self {
        index: QuickArrayIndex::Long(QuickLongOperand::Const(0)),
        fetch_result: 0,
        accumulator: 0,
        add_result: 0,
        fetch_resume_ip: 0,
        add_resume_ip: 0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayFetch {
    pub index: QuickArrayIndex,
    pub result: u16,
    pub resume_ip: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayRegionKernel {
    pub array: u16,
    pub adds: [QuickStraightArrayFetchAdd; QUICK_STRAIGHT_ARRAY_MAX_ADDS],
    pub add_count: u8,
    pub trailing_fetch: Option<QuickStraightArrayFetch>,
    pub exit_target: QuickLongTarget,
}

/// Select a measured superinstruction from the general typed graph. PHP names,
/// literal keys, and source-level workload identity never participate; this
/// only compresses adjacent FetchArrayLong/AddAssign operations over one
/// immutable result array.
fn detect_straight_array_region_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickStraightArrayRegionKernel> {
    if plan.entry_op != 0
        || plan.ops.len() < 2
        || plan.array_input_mask.count_ones() != 1
        || plan.array_output_mask != 0
        || plan.string_input_mask != 0
        || plan.string_output_mask != 0
        || plan.string_append_mask != 0
        || plan.object_input_mask != 0
    {
        return None;
    }

    let array = plan.array_input_mask.trailing_zeros() as u16;
    let mut adds = [QuickStraightArrayFetchAdd::EMPTY; QUICK_STRAIGHT_ARRAY_MAX_ADDS];
    let mut add_count = 0usize;
    let mut cursor = 0usize;
    let mut exit_target = None;

    while cursor + 1 < plan.ops.len() && add_count < adds.len() {
        let QuickLongOp::FetchArrayLong {
            array: fetch_array,
            index,
            result: fetch_result,
            destination: None,
            next_target: fetch_next,
            resume_ip: fetch_resume_ip,
        } = plan.ops[cursor]
        else {
            break;
        };
        if fetch_array != array
            || matches!(index, QuickArrayIndex::ValueSlot(_))
            || fetch_next.op_index() != Some(cursor + 1)
        {
            return None;
        }
        let QuickLongOp::AddAssign {
            lhs,
            rhs,
            result: add_result,
            destination,
            next_target,
            add_resume_ip,
        } = plan.ops[cursor + 1]
        else {
            break;
        };
        let accumulator = if lhs == destination && rhs == fetch_result {
            lhs
        } else if rhs == destination && lhs == fetch_result {
            rhs
        } else {
            return None;
        };
        adds[add_count] = QuickStraightArrayFetchAdd {
            index,
            fetch_result,
            accumulator,
            add_result,
            fetch_resume_ip,
            add_resume_ip,
        };
        add_count += 1;
        cursor += 2;
        if cursor < plan.ops.len() {
            if next_target.op_index() != Some(cursor) {
                return None;
            }
        } else {
            exit_target = Some(next_target);
        }
    }

    let trailing_fetch = if cursor < plan.ops.len() {
        if cursor + 1 != plan.ops.len() {
            return None;
        }
        let QuickLongOp::FetchArrayLong {
            array: fetch_array,
            index,
            result,
            destination: None,
            next_target,
            resume_ip,
        } = plan.ops[cursor]
        else {
            return None;
        };
        if fetch_array != array || matches!(index, QuickArrayIndex::ValueSlot(_)) {
            return None;
        }
        exit_target = Some(next_target);
        Some(QuickStraightArrayFetch {
            index,
            result,
            resume_ip,
        })
    } else {
        None
    };

    if add_count == 0 {
        return None;
    }
    let exit_target = exit_target?;
    exit_target.exit_ip()?;
    Some(QuickStraightArrayRegionKernel {
        array,
        adds,
        add_count: add_count as u8,
        trailing_fetch,
        exit_target,
    })
}

fn long_literal(op_array: &OpArray, index: u16) -> Option<i64> {
    op_array
        .literals
        .get(index as usize)
        .and_then(Value::as_long)
}

#[derive(Clone, Copy)]
struct ProvenQuickDoubleSource {
    source: QuickDoubleSource,
    is_double: bool,
}

fn quick_double_argument_source(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
    induction_cv: u16,
    produced_temporary_slots: &[u16; 8],
    produced_temporary_count: usize,
    input_slots: &mut [u16; 8],
    input_count: &mut usize,
    double_input_mask: &mut u64,
    total_slots: u32,
) -> Option<ProvenQuickDoubleSource> {
    match op_type {
        OpType::Cv if operand == induction_cv => Some(ProvenQuickDoubleSource {
            source: QuickDoubleSource::Induction,
            is_double: false,
        }),
        OpType::Cv => {
            add_mask_slot(double_input_mask, operand, total_slots)?;
            let input = if let Some(index) = input_slots[..*input_count]
                .iter()
                .position(|slot| *slot == operand)
            {
                index
            } else {
                if *input_count == input_slots.len() {
                    return None;
                }
                let index = *input_count;
                input_slots[index] = operand;
                *input_count += 1;
                index
            };
            Some(ProvenQuickDoubleSource {
                source: QuickDoubleSource::Input(input as u8),
                is_double: true,
            })
        }
        OpType::Const => {
            let value = op_array.literals.get(operand as usize)?;
            match value.value_type() {
                crate::value::ValueType::Double => Some(ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Constant(value.as_double()?),
                    is_double: true,
                }),
                crate::value::ValueType::Long => Some(ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Constant(value.as_long()? as f64),
                    is_double: false,
                }),
                _ => None,
            }
        }
        OpType::Tmp | OpType::Var => produced_temporary_slots[..produced_temporary_count]
            .iter()
            .position(|slot| *slot == operand)
            .map(|index| ProvenQuickDoubleSource {
                source: QuickDoubleSource::Temporary(index as u8),
                is_double: true,
            }),
        OpType::Unused => None,
    }
}
