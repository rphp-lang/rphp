fn detect_long_tail_trace_guard(
    op_array: &OpArray,
    guard_ip: usize,
    increment_ip: usize,
    total_slots: u32,
) -> Option<QuickLongTraceGuard> {
    // The conditional jump must skip at least one cold instruction and land
    // directly on the loop increment. The cold range itself stays canonical
    // and may contain arbitrary PHP behavior.
    if guard_ip.checked_add(2)? >= increment_ip {
        return None;
    }
    let comparison = *op_array.instructions.get(guard_ip)?;
    let kind = match comparison.opcode {
        OpCode::IsIdentical => ScalarLongConditionKind::Equal,
        OpCode::IsNotIdentical => ScalarLongConditionKind::NotEqual,
        _ => return None,
    };
    let operand = |op_type, value| match op_type {
        OpType::Cv => {
            (u32::from(value) < op_array.num_cvs).then_some(QuickLongOperand::Slot(value))
        }
        OpType::Const => Some(QuickLongOperand::Const(long_literal(op_array, value)?)),
        _ => None,
    };
    let lhs = operand(comparison.op1_type, comparison.op1)?;
    let rhs = operand(comparison.op2_type, comparison.op2)?;
    if comparison.result_type != OpType::Tmp || u32::from(comparison.result) >= total_slots {
        return None;
    }
    let branch = *op_array.instructions.get(guard_ip + 1)?;
    if branch.op1_type != OpType::Tmp
        || branch.op1 != comparison.result
        || branch.op2_type != OpType::Unused
        || branch.op2 as usize != increment_ip
    {
        return None;
    }
    let expected = match branch.opcode {
        OpCode::JmpZ => false,
        OpCode::JmpNZ => true,
        _ => return None,
    };
    Some(QuickLongTraceGuard {
        kind,
        lhs,
        rhs,
        expected,
        condition_tmp: Some(comparison.result),
        resume_ip: guard_ip,
    })
}

fn instruction_writes_cv(instruction: crate::vm::instruction::Instruction, cv: u16) -> bool {
    if instruction.result_type == OpType::Cv && instruction.result == cv {
        return true;
    }
    if instruction.opcode == OpCode::ForeachNext {
        let value_cv = instruction.extended_value as u16;
        let encoded_key = (instruction.extended_value >> 16) as u16;
        return value_cv == cv || (encoded_key != 0 && encoded_key - 1 == cv);
    }
    matches!(
        instruction.opcode,
        OpCode::AssignCv
            | OpCode::AssignConcat
            | OpCode::PreInc
            | OpCode::PreDec
            | OpCode::PostInc
            | OpCode::PostDec
            | OpCode::BindGlobal
            | OpCode::BindStatic
    ) && instruction.op1_type == OpType::Cv
        && instruction.op1 == cv
}

fn preheader_string_literal_cv(op_array: &OpArray, header_ip: usize, cv: u16) -> bool {
    op_array.instructions[..header_ip]
        .iter()
        .rev()
        .copied()
        .find(|instruction| instruction_writes_cv(*instruction, cv))
        .is_some_and(|instruction| {
            instruction.opcode == OpCode::AssignCv
                && instruction.op1_type == OpType::Cv
                && instruction.op2_type == OpType::Const
                && instruction.result_type == OpType::Unused
                && op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .is_some()
        })
}

fn cv_unmodified_in_region(instructions: &[crate::vm::instruction::Instruction], cv: u16) -> bool {
    instructions
        .iter()
        .copied()
        .all(|instruction| !instruction_writes_cv(instruction, cv))
}

/// Recognize the shared loop-invariant associative JSON producer. Consumers
/// own path/use validation, but input stability and the canonical materialized
/// destination are identical for Long, Double and String regions.
fn detect_json_typed_invariant_source(
    op_array: &OpArray,
    region: &[crate::vm::instruction::Instruction],
    producer_ip: usize,
) -> Option<QuickTypedInvariantSource> {
    let producer = *op_array.instructions.get(producer_ip)?;
    if producer.opcode != OpCode::DirectInternalCall2
        || crate::builtin_metadata::DirectInternalKind::from_id(producer.extended_value)
            != Some(crate::builtin_metadata::DirectInternalKind::JsonDecode)
        || !matches!(producer.op1_type, OpType::Cv | OpType::Const)
        || producer.op2_type != OpType::Const
        || !matches!(producer.result_type, OpType::Tmp | OpType::Var)
        || op_array.literals.get(producer.op2 as usize)?.value_type()
            != crate::value::ValueType::True
    {
        return None;
    }
    let input = match producer.op1_type {
        OpType::Cv if cv_unmodified_in_region(region, producer.op1) => {
            QuickInvariantInput::StringSlot(producer.op1)
        }
        OpType::Const => {
            op_array.literals.get(producer.op1 as usize)?.as_str()?;
            QuickInvariantInput::StringLiteral(producer.op1)
        }
        _ => return None,
    };
    let assignment = *op_array.instructions.get(producer_ip + 1)?;
    if assignment.opcode != OpCode::AssignCv
        || assignment.op1_type != OpType::Cv
        || assignment.op2_type != producer.result_type
        || assignment.op2 != producer.result
        || assignment.result_type != OpType::Unused
    {
        return None;
    }
    Some(QuickTypedInvariantSource {
        producer: QuickTypedInvariantProducer::JsonDecodeAssociative { input },
        destination: assignment.op1,
        projections: Vec::new(),
        long_output_mask: 0,
        double_output_mask: 0,
        string_output_mask: 0,
    })
}

fn fixed_invariant_path_element(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
) -> Option<QuickInvariantPathElement> {
    if op_type != OpType::Const {
        return None;
    }
    let value = op_array.literals.get(operand as usize)?;
    Some(if value.as_str().is_some() {
        QuickInvariantPathElement::StringLiteral(operand)
    } else {
        QuickInvariantPathElement::Integer(value.as_long()?)
    })
}

/// Complete planning state for fixed projections rooted in one invariant JSON
/// producer. Keeping path ownership, reachability and derived String metadata
/// together prevents standalone and deferred-argument consumers from drifting.
struct InvariantJsonProjectionState {
    paths: Vec<Option<Vec<QuickInvariantPathElement>>>,
    fetch_mask: u64,
    parent_mask: u64,
    string_source_mask: u64,
    string_length_paths: Vec<Option<Vec<QuickInvariantPathElement>>>,
}

impl InvariantJsonProjectionState {
    fn new(total_slots: u32) -> Self {
        Self {
            paths: vec![None; total_slots as usize],
            fetch_mask: 0,
            parent_mask: 0,
            string_source_mask: 0,
            string_length_paths: vec![None; total_slots as usize],
        }
    }

    fn start(&mut self, destination: u16) -> Option<()> {
        self.paths
            .get_mut(destination as usize)?
            .replace(Vec::new());
        Some(())
    }

    fn tracks(&self, slot: u16) -> bool {
        self.paths
            .get(slot as usize)
            .and_then(|path| path.as_ref())
            .is_some()
    }

    /// Extend a fixed projection rooted in the invariant JSON producer.
    /// `false` means that the fetch belongs to an ordinary PHP array and must
    /// be handled by the caller's canonical array-planning path.
    fn extend_fetch(
        &mut self,
        op_array: &OpArray,
        instruction: crate::vm::instruction::Instruction,
        array: u16,
        total_slots: u32,
    ) -> Option<bool> {
        let Some(mut path) = self
            .paths
            .get(array as usize)
            .and_then(|path| path.as_ref())
            .cloned()
        else {
            return Some(false);
        };
        let element = fixed_invariant_path_element(
            op_array,
            instruction.op2_type,
            instruction.op2,
        )?;
        if path.len() == 8 {
            return None;
        }
        path.push(element);
        self.paths
            .get_mut(instruction.result as usize)?
            .replace(path);
        add_mask_slot(&mut self.fetch_mask, instruction.result, total_slots)?;
        add_mask_slot(&mut self.parent_mask, array, total_slots)?;
        Some(true)
    }

    fn derive_string_length(
        &mut self,
        instruction: crate::vm::instruction::Instruction,
        total_slots: u32,
    ) -> Option<()> {
        let path = self
            .paths
            .get(instruction.op1 as usize)
            .and_then(|path| path.as_ref())?
            .clone();
        if path.is_empty()
            || self
                .string_length_paths
                .get(instruction.result as usize)?
                .is_some()
        {
            return None;
        }
        add_mask_slot(
            &mut self.string_source_mask,
            instruction.op1,
            total_slots,
        )?;
        self.string_length_paths
            .get_mut(instruction.result as usize)?
            .replace(path);
        Some(())
    }

    fn retain_projections(
        &self,
        source: &mut QuickTypedInvariantSource,
        long_input_mask: u64,
    ) -> Option<()> {
        if self.fetch_mask == 0
            || self.fetch_mask
                & !(self.parent_mask | long_input_mask | self.string_source_mask)
                != 0
        {
            return None;
        }
        let mut outputs = self.fetch_mask & long_input_mask;
        while outputs != 0 {
            let result = outputs.trailing_zeros() as u16;
            outputs &= outputs - 1;
            let path = self.paths.get(result as usize)?.as_ref()?.clone();
            if path.is_empty() {
                return None;
            }
            source.long_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.into_boxed_slice(),
                result,
                kind: QuickInvariantValueKind::Long,
            });
        }
        let mut string_sources = self.string_source_mask;
        while string_sources != 0 {
            let result = string_sources.trailing_zeros() as u16;
            string_sources &= string_sources - 1;
            let path = self.paths.get(result as usize)?.as_ref()?.clone();
            source.string_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.into_boxed_slice(),
                result,
                kind: QuickInvariantValueKind::String,
            });
        }
        for (result, path) in self.string_length_paths.iter().enumerate() {
            let Some(path) = path else {
                continue;
            };
            source.long_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.clone().into_boxed_slice(),
                result: result as u16,
                kind: QuickInvariantValueKind::StringLength,
            });
        }
        (!source.projections.is_empty()).then_some(())
    }
}

fn long_add(instruction: crate::vm::instruction::Instruction) -> Option<(u16, u16, u16)> {
    if !matches!(
        instruction.opcode,
        OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
    ) || instruction.result_type != OpType::Tmp
    {
        return None;
    }
    Some((
        long_slot(instruction.op1_type, instruction.op1)?,
        long_slot(instruction.op2_type, instruction.op2)?,
        instruction.result,
    ))
}

fn long_assign(instruction: crate::vm::instruction::Instruction) -> Option<(u16, u16)> {
    if instruction.opcode != OpCode::AssignCv
        || instruction.op1_type != OpType::Cv
        || instruction.result_type != OpType::Unused
    {
        return None;
    }
    Some((
        instruction.op1,
        long_slot(instruction.op2_type, instruction.op2)?,
    ))
}

fn conditional_add_assign(
    op_array: &OpArray,
    add_ip: usize,
    false_ip: usize,
) -> Option<(u16, u16, u16, u16, usize)> {
    let (lhs, rhs, result) = long_add(*op_array.instructions.get(add_ip)?)?;
    let (destination, source) = long_assign(*op_array.instructions.get(add_ip + 1)?)?;
    let next_ip = add_ip + 2;
    (source == result && false_ip == next_ip).then_some((lhs, rhs, result, destination, next_ip))
}

/// Determine the Long values that must exist before a straight-line region
/// starts. Temporaries produced earlier in the same region are deliberately
/// excluded, so a region can activate in a fresh function frame rather than
/// depending on stale temporary-slot contents from an earlier execution.
fn straight_region_read(inputs: &mut u64, defined: u64, slot: u16) -> Option<()> {
    let bit = 1u64.checked_shl(u32::from(slot))?;
    if defined & bit == 0 {
        *inputs |= bit;
    }
    Some(())
}

fn straight_region_write(defined: &mut u64, slot: u16) -> Option<()> {
    *defined |= 1u64.checked_shl(u32::from(slot))?;
    Some(())
}

fn straight_region_read_operand(
    inputs: &mut u64,
    defined: u64,
    operand: QuickLongOperand,
) -> Option<()> {
    match operand {
        QuickLongOperand::Slot(slot) => straight_region_read(inputs, defined, slot),
        QuickLongOperand::Const(_) => Some(()),
    }
}

fn straight_long_region_inputs(ops: &[QuickLongOp]) -> Option<u64> {
    let mut inputs = 0u64;
    let mut defined = 0u64;

    for op in ops {
        match *op {
            QuickLongOp::ModConst { value, result, .. } => {
                straight_region_read(&mut inputs, defined, value)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::TraceGuard {
                lhs,
                rhs,
                condition_tmp,
                ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                if let Some(condition_tmp) = condition_tmp {
                    straight_region_write(&mut defined, condition_tmp)?;
                }
            }
            QuickLongOp::Binary {
                lhs, rhs, result, ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::BinaryAssign {
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::FetchArrayLong {
                index,
                result,
                destination,
                ..
            } => {
                if let QuickArrayIndex::Long(operand) = index {
                    straight_region_read_operand(&mut inputs, defined, operand)?;
                }
                straight_region_write(&mut defined, result)?;
                if let Some(destination) = destination {
                    straight_region_write(&mut defined, destination)?;
                }
            }
            QuickLongOp::Add {
                lhs, rhs, result, ..
            } => {
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                match condition {
                    QuickLongCondition::Lt { lhs, rhs } | QuickLongCondition::Eq { lhs, rhs } => {
                        straight_region_read(&mut inputs, defined, lhs)?;
                        straight_region_read_operand(&mut inputs, defined, rhs)?;
                    }
                }
                if let Some(condition_tmp) = condition_tmp {
                    straight_region_write(&mut defined, condition_tmp)?;
                }
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::AddAddAssign {
                first_lhs,
                first_rhs,
                first_result,
                second_lhs,
                second_rhs,
                second_result,
                destination,
                ..
            } => {
                straight_region_read(&mut inputs, defined, first_lhs)?;
                straight_region_read(&mut inputs, defined, first_rhs)?;
                straight_region_write(&mut defined, first_result)?;
                straight_region_read(&mut inputs, defined, second_lhs)?;
                straight_region_read(&mut inputs, defined, second_rhs)?;
                straight_region_write(&mut defined, second_result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::Assign {
                destination,
                source,
                ..
            } => {
                straight_region_read(&mut inputs, defined, source)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::AssignLongLiteral { destination, .. } => {
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::PostInc { value, result, .. } => {
                straight_region_read(&mut inputs, defined, value)?;
                if let Some(result) = result {
                    straight_region_write(&mut defined, result)?;
                }
                straight_region_write(&mut defined, value)?;
            }
            // The first application-region slice is intentionally bounded by
            // calls, observable mutation, and control-flow edges. Closed loops
            // continue to use the complete operation vocabulary.
            _ => return None,
        }
    }
    Some(inputs)
}
