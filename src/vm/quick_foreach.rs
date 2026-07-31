//! Guarded value-only foreach execution.
//!
//! Kept outside the main executor so this specialized loop does not perturb
//! code generation for the existing indexed-array kernels.

use std::sync::atomic::Ordering;

use crate::compiler::OpArray;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};

use super::execute::{QuickLoopOutcome, VmError, handle_interrupt, quick_loop_slot_has_heap};
use super::frame::{CALL_FRAME_SLOTS, ExecuteData};
use super::quick::QuickForeachLongAccumulateLoop;
use super::stats;

#[derive(Clone, Copy)]
struct QuickForeachLongArray {
    first_value: *const u8,
    stride: usize,
}

impl QuickForeachLongArray {
    #[inline]
    fn from_array(array: &PhpArray) -> Option<Self> {
        match array.packed_values() {
            Some(values) => values.first().map(|value| Self {
                first_value: (value as *const Value).cast(),
                stride: std::mem::size_of::<Value>(),
            }),
            None => array
                .ordered_hash_value_layout()
                .map(|(first_value, stride)| Self {
                    first_value,
                    stride,
                }),
        }
    }

    #[inline(always)]
    unsafe fn long_at_position(self, position: usize) -> Option<i64> {
        let value = &*(self.first_value.add(position * self.stride) as *const Value);
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }
}

#[inline(never)]
pub(super) unsafe fn run_quick_foreach_long_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &OpArray,
    plan: QuickForeachLongAccumulateLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs || (*frame).num_cvs + (*frame).num_temps > 64 {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let array_ptr = slot_base.add(plan.array_tmp as usize);
    let position_ptr = slot_base.add(plan.position_tmp as usize);
    let value_ptr = slot_base.add(plan.value_cv as usize);
    let done_ptr = slot_base.add(plan.done_tmp as usize);
    let accumulator_ptr = slot_base.add(plan.accumulator_cv as usize);
    let sum_ptr = slot_base.add(plan.sum_tmp as usize);

    if quick_loop_slot_has_heap(frame, plan.position_tmp)
        || quick_loop_slot_has_heap(frame, plan.value_cv)
        || quick_loop_slot_has_heap(frame, plan.done_tmp)
        || quick_loop_slot_has_heap(frame, plan.accumulator_cv)
        || quick_loop_slot_has_heap(frame, plan.sum_tmp)
        || (*array_ptr).as_array().is_none()
        || (*position_ptr).value_type() != ValueType::Long
        || (*value_ptr).value_type() != ValueType::Long
        || (*done_ptr).value_type() != ValueType::True
        || (*accumulator_ptr).value_type() != ValueType::Long
        || (*sum_ptr).value_type() != ValueType::Long
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let Some(mut position) = usize::try_from((*position_ptr).raw_long()).ok() else {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    };
    let array = (*array_ptr).as_array().unwrap_unchecked();
    let len = array.len();
    let Some(quick_array) = QuickForeachLongArray::from_array(array) else {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    };
    let mut accumulator = (*accumulator_ptr).raw_long();
    let mut last_value = (*value_ptr).raw_long();
    let mut completed_iteration = false;
    let mut iterations = 0u64;

    while position < len {
        let Some(value) = quick_array.long_at_position(position) else {
            Value::write_long(position_ptr, position as i64);
            Value::write_long(accumulator_ptr, accumulator);
            Value::write_bool(done_ptr, true);
            Value::write_long(sum_ptr, accumulator);
            if completed_iteration {
                Value::write_long(value_ptr, last_value);
            }
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        };

        let next_position = position + 1;
        let Some(next_accumulator) = accumulator.checked_add(value) else {
            Value::write_long(position_ptr, next_position as i64);
            Value::write_long(value_ptr, value);
            Value::write_bool(done_ptr, true);
            Value::write_long(accumulator_ptr, accumulator);
            (*frame).opline = op_array.instructions.as_ptr().add(plan.sum_ip);
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        };

        position = next_position;
        accumulator = next_accumulator;
        last_value = value;
        completed_iteration = true;
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            Value::write_long(position_ptr, position as i64);
            Value::write_long(value_ptr, last_value);
            Value::write_bool(done_ptr, true);
            Value::write_long(accumulator_ptr, accumulator);
            Value::write_long(sum_ptr, accumulator);
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            handle_interrupt(eg)?;
        }
    }

    Value::write_long(position_ptr, position as i64);
    Value::write_bool(done_ptr, false);
    Value::write_long(accumulator_ptr, accumulator);
    Value::write_long(sum_ptr, accumulator);
    if completed_iteration {
        Value::write_long(value_ptr, last_value);
    }
    (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}
