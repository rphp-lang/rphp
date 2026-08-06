//! Guarded value-only foreach execution.
//!
//! Kept outside the main executor so this specialized loop does not perturb
//! code generation for the existing indexed-array kernels.

use std::sync::atomic::Ordering;

use crate::compiler::OpArray;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpArray, Value, ValueType};

use super::execute::{
    QuickLoopOutcome, VmError, frame_slot_set, frame_tmp_set_long, handle_interrupt,
    quick_loop_slot_has_heap,
};
use super::frame::{CALL_FRAME_SLOTS, ExecuteData};
use super::quick::{
    QuickForeachLongAccumulateLoop, QuickForeachObjectProjectionKind,
    QuickForeachObjectPropertyAccumulateLoop,
};
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
    unsafe fn value_at_position(self, position: usize) -> *const Value {
        self.first_value.add(position * self.stride).cast()
    }

    #[inline(always)]
    unsafe fn long_at_position(self, position: usize) -> Option<i64> {
        let value = &*self.value_at_position(position);
        (value.value_type() == ValueType::Long).then(|| value.raw_long())
    }
}

#[derive(Clone, Copy)]
enum QuickForeachObjectPropertyBinding<'a> {
    Declared {
        class_id: u32,
        slots: [usize; 2],
    },
    Dynamic {
        layout: *const ObjectLayout,
        names: [&'a str; 2],
        positions: [Option<usize>; 2],
    },
}

impl<'a> QuickForeachObjectPropertyBinding<'a> {
    fn from_plan(
        op_array: &'a OpArray,
        plan: QuickForeachObjectPropertyAccumulateLoop,
    ) -> Option<Self> {
        let first = plan.projections[0]?;
        let first_cache = op_array.cache.get(first.cache_ip)?;
        if first_cache.is_dynamic_property_read() {
            let layout = first_cache.dynamic_property_layout();
            let mut names = [""; 2];
            let mut positions = [None; 2];
            for (index, projection) in plan.projections.iter().flatten().enumerate() {
                let cache = op_array.cache.get(projection.cache_ip)?;
                let instruction = op_array.instructions.get(projection.cache_ip)?;
                if !cache.is_dynamic_property_read()
                    || cache.dynamic_property_layout() != layout
                {
                    return None;
                }
                names[index] = op_array
                    .literals
                    .get(instruction.op2 as usize)?
                    .as_str()?;
                positions[index] = cache.dynamic_property_position();
            }
            return Some(Self::Dynamic {
                layout,
                names,
                positions,
            });
        }

        let class_id = first_cache.class_id;
        if class_id == 0 || first_cache.property_flags() & 1 == 0 {
            return None;
        }
        let mut slots = [0; 2];
        for (index, projection) in plan.projections.iter().flatten().enumerate() {
            let cache = op_array.cache.get(projection.cache_ip)?;
            if cache.is_dynamic_property_read()
                || cache.class_id != class_id
                || cache.property_flags() & 1 == 0
            {
                return None;
            }
            slots[index] = cache.property_slot();
        }
        Some(Self::Declared { class_id, slots })
    }

    #[inline(always)]
    unsafe fn receiver_matches(self, receiver: &Value) -> bool {
        if receiver.value_type() != ValueType::Object || receiver.is_reference() {
            return false;
        }
        match self {
            Self::Declared { class_id, .. } => {
                receiver.object_class_id_unchecked() == class_id
            }
            Self::Dynamic { layout, .. } => {
                receiver.object_property_layout_ptr_unchecked() == layout
            }
        }
    }

    #[inline(always)]
    unsafe fn property_at(self, receiver: &Value, index: usize) -> *const Value {
        match self {
            Self::Declared { slots, .. } => {
                receiver.object_property_slot_unchecked(*slots.get_unchecked(index))
            }
            Self::Dynamic {
                names, positions, ..
            } => {
                let name = *names.get_unchecked(index);
                let mut property = (*positions.get_unchecked(index)).map_or(
                    std::ptr::null(),
                    |position| receiver.object_dynamic_property_at_unchecked(name, position),
                );
                if property.is_null() {
                    property = receiver.object_dynamic_property_unchecked(name);
                }
                property
            }
        }
    }
}

#[inline(always)]
unsafe fn publish_foreach_object_state(
    frame: *mut ExecuteData,
    slot_base: *mut Value,
    plan: QuickForeachObjectPropertyAccumulateLoop,
    position: usize,
    receiver: *const Value,
    accumulator: i64,
    projected: &[i64; 2],
    projected_count: usize,
    term: Option<i64>,
    done: bool,
) {
    Value::write_long(
        slot_base.add(plan.position_tmp as usize),
        position as i64,
    );
    Value::write_bool(slot_base.add(plan.done_tmp as usize), done);
    Value::write_long(
        slot_base.add(plan.accumulator_cv as usize),
        accumulator,
    );
    if !receiver.is_null() {
        frame_slot_set(
            frame,
            slot_base.add(plan.receiver_cv as usize),
            (*receiver).clone(),
        );
    }
    for index in 0..projected_count {
        let projection = plan.projections[index].unwrap_unchecked();
        frame_tmp_set_long(
            frame,
            slot_base.add(projection.result_tmp as usize),
            projected[index],
        );
    }
    if let (Some(term_tmp), Some(term)) = (plan.term_tmp, term) {
        frame_tmp_set_long(frame, slot_base.add(term_tmp as usize), term);
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

#[inline(never)]
pub(super) unsafe fn run_quick_foreach_object_property_accumulate_loop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &OpArray,
    plan: QuickForeachObjectPropertyAccumulateLoop,
) -> Result<QuickLoopOutcome, VmError> {
    if (*frame).num_cvs != op_array.num_cvs
        || (*frame).num_cvs + (*frame).num_temps > 64
        || plan.projection_count == 0
        || plan.projection_count > 2
    {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }

    let slot_base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    let array_ptr = slot_base.add(plan.array_tmp as usize);
    let position_ptr = slot_base.add(plan.position_tmp as usize);
    let done_ptr = slot_base.add(plan.done_tmp as usize);
    let accumulator_ptr = slot_base.add(plan.accumulator_cv as usize);
    let sum_ptr = slot_base.add(plan.sum_tmp as usize);

    if quick_loop_slot_has_heap(frame, plan.position_tmp)
        || quick_loop_slot_has_heap(frame, plan.done_tmp)
        || quick_loop_slot_has_heap(frame, plan.accumulator_cv)
        || (*array_ptr).as_array().is_none()
        || (*position_ptr).value_type() != ValueType::Long
        || (*done_ptr).value_type() != ValueType::True
        || (*accumulator_ptr).value_type() != ValueType::Long
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
    if position > len {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    }
    let Some(quick_array) = QuickForeachLongArray::from_array(array) else {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    };
    let Some(binding) = QuickForeachObjectPropertyBinding::from_plan(op_array, plan) else {
        stats::inc_quick_loop_guard_failed();
        return Ok(QuickLoopOutcome::GuardFailed);
    };

    let mut accumulator = (*accumulator_ptr).raw_long();
    let mut projected = [0i64; 2];
    let mut last_receiver = std::ptr::null();
    let mut iterations = 0u64;

    while position < len {
        let receiver = quick_array.value_at_position(position);
        let next_position = position + 1;
        if !binding.receiver_matches(&*receiver) {
            publish_foreach_object_state(
                frame,
                slot_base,
                plan,
                next_position,
                receiver,
                accumulator,
                &projected,
                0,
                None,
                true,
            );
            (*frame).opline = op_array
                .instructions
                .as_ptr()
                .add(plan.projections[0].unwrap_unchecked().cache_ip);
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        }

        for index in 0..plan.projection_count as usize {
            let projection = plan.projections[index].unwrap_unchecked();
            let property = binding.property_at(&*receiver, index);
            let projected_value = if property.is_null() || (*property).is_reference() {
                None
            } else {
                match projection.kind {
                    QuickForeachObjectProjectionKind::Long => {
                        ((*property).value_type() == ValueType::Long)
                            .then(|| (*property).raw_long())
                    }
                    QuickForeachObjectProjectionKind::StringLength => (*property)
                        .as_str()
                        .map(|value| value.len() as i64),
                }
            };
            let Some(projected_value) = projected_value else {
                publish_foreach_object_state(
                    frame,
                    slot_base,
                    plan,
                    next_position,
                    receiver,
                    accumulator,
                    &projected,
                    index,
                    None,
                    true,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(projection.cache_ip);
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            };
            projected[index] = projected_value;
        }

        let term = if plan.projection_count == 2 {
            let Some(term) = projected[0].checked_add(projected[1]) else {
                publish_foreach_object_state(
                    frame,
                    slot_base,
                    plan,
                    next_position,
                    receiver,
                    accumulator,
                    &projected,
                    2,
                    None,
                    true,
                );
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(plan.term_ip.unwrap_unchecked());
                stats::inc_quick_loop_deoptimized(iterations);
                return Ok(QuickLoopOutcome::Deoptimized);
            };
            term
        } else {
            projected[0]
        };
        let Some(next_accumulator) = accumulator.checked_add(term) else {
            publish_foreach_object_state(
                frame,
                slot_base,
                plan,
                next_position,
                receiver,
                accumulator,
                &projected,
                plan.projection_count as usize,
                Some(term),
                true,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.sum_ip);
            stats::inc_quick_loop_deoptimized(iterations);
            return Ok(QuickLoopOutcome::Deoptimized);
        };

        position = next_position;
        accumulator = next_accumulator;
        last_receiver = receiver;
        iterations += 1;

        if iterations & 31 == 0 && eg.vm_interrupt.load(Ordering::Relaxed) {
            publish_foreach_object_state(
                frame,
                slot_base,
                plan,
                position,
                last_receiver,
                accumulator,
                &projected,
                0,
                None,
                true,
            );
            (*frame).opline = op_array.instructions.as_ptr().add(plan.header_ip);
            handle_interrupt(eg)?;
        }
    }

    Value::write_long(position_ptr, position as i64);
    Value::write_bool(done_ptr, false);
    Value::write_long(accumulator_ptr, accumulator);
    frame_tmp_set_long(frame, sum_ptr, accumulator);
    if !last_receiver.is_null() {
        frame_slot_set(
            frame,
            slot_base.add(plan.receiver_cv as usize),
            (*last_receiver).clone(),
        );
    }
    (*frame).opline = op_array.instructions.as_ptr().add(plan.exit_ip);
    stats::inc_quick_loop_completed(iterations);
    Ok(QuickLoopOutcome::Completed)
}
