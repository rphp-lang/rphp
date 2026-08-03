// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// Read a string-keyed array entry through a validated per-opcode position
/// hint. Layout changes merely miss the positional check and refresh through
/// the canonical string index; the key is compared before every hinted read.
#[inline(always)]
unsafe fn cached_string_array_value<'a>(
    op_array: &crate::compiler::OpArray,
    cache_ip: usize,
    array: &'a PhpArray,
    key: &str,
) -> Option<&'a Value> {
    let cache = op_array.cache.get_unchecked(cache_ip);
    if let Some(position) = cache.string_array_position() {
        if let Some(value) = array.get_positioned_str(key, position) {
            return Some(value);
        }
    }

    let (position, value) = array.get_str_with_position(key)?;
    let cache = &mut *(op_array.cache.as_ptr().add(cache_ip)
        as *mut crate::vm::instruction::InlineCache);
    cache.set_string_array_position(position);
    Some(value)
}

#[inline(always)]
#[cfg(feature = "quick-loops")]
unsafe fn quick_straight_array_fetch(
    array: QuickLongArray,
    index: QuickArrayIndex,
    op_array: &crate::compiler::OpArray,
    cache_ip: usize,
) -> Option<i64> {
    match index {
        QuickArrayIndex::Long(QuickLongOperand::Const(index)) => {
            array.long_at_int(index)
        }
        QuickArrayIndex::StringLiteral(literal) => {
            let QuickLongArray::Hash { array } = array else {
                return None;
            };
            let key = op_array
                .literals
                .get_unchecked(literal as usize)
                .as_str()
                .unwrap_unchecked();
            let value = cached_string_array_value(op_array, cache_ip, &*array, key)?;
            (value.value_type() == ValueType::Long).then(|| value.raw_long())
        }
        QuickArrayIndex::Long(QuickLongOperand::Slot(_))
        | QuickArrayIndex::ValueSlot(_) => None,
    }
}

#[inline(never)]
#[cfg(feature = "quick-loops")]
unsafe fn run_quick_straight_array_region(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    kernel: crate::vm::quick::QuickStraightArrayRegionKernel,
) -> QuickLoopOutcome {
    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let mut input_mask = plan.long_input_mask;
    while input_mask != 0 {
        let slot = input_mask.trailing_zeros() as usize;
        input_mask &= input_mask - 1;
        if (*slot_base.add(slot)).value_type() != ValueType::Long {
            stats::inc_quick_loop_guard_failed();
            return QuickLoopOutcome::GuardFailed;
        }
    }

    let Some(array) = (*slot_base.add(kernel.array as usize)).as_array() else {
        stats::inc_quick_loop_guard_failed();
        return QuickLoopOutcome::GuardFailed;
    };
    let array = QuickLongArray::from_array(array);

    for add in kernel.adds.iter().take(kernel.add_count as usize) {
        let Some(fetched) = quick_straight_array_fetch(
            array,
            add.index,
            op_array,
            add.fetch_resume_ip,
        ) else {
            (*frame).opline = op_array
                .instructions
                .as_ptr()
                .add(add.fetch_resume_ip);
            stats::inc_quick_loop_deoptimized(0);
            return QuickLoopOutcome::Deoptimized;
        };
        Value::write_long(slot_base.add(add.fetch_result as usize), fetched);

        let accumulator = (*slot_base.add(add.accumulator as usize)).raw_long();
        let Some(sum) = accumulator.checked_add(fetched) else {
            (*frame).opline = op_array
                .instructions
                .as_ptr()
                .add(add.add_resume_ip);
            stats::inc_quick_loop_deoptimized(0);
            return QuickLoopOutcome::Deoptimized;
        };
        Value::write_long(slot_base.add(add.add_result as usize), sum);
        Value::write_long(slot_base.add(add.accumulator as usize), sum);
    }

    if let Some(fetch) = kernel.trailing_fetch {
        let Some(fetched) = quick_straight_array_fetch(
            array,
            fetch.index,
            op_array,
            fetch.resume_ip,
        ) else {
            (*frame).opline = op_array.instructions.as_ptr().add(fetch.resume_ip);
            stats::inc_quick_loop_deoptimized(0);
            return QuickLoopOutcome::Deoptimized;
        };
        Value::write_long(slot_base.add(fetch.result as usize), fetched);
    }

    (*frame).opline = op_array
        .instructions
        .as_ptr()
        .add(kernel.exit_target.exit_ip().unwrap_unchecked());
    stats::inc_quick_loop_completed(0);
    QuickLoopOutcome::Completed
}

