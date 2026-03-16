use std::sync::atomic::Ordering;

use crate::value::{Value, PhpArray, PhpObject, ArrayKey, ValueType};
use crate::runtime::ExecutorGlobals;
use super::opcode::OpCode;
use super::instruction::OpType;
use super::frame::{ExecuteData, CALL_FRAME_SLOTS};
use super::function::{Function, FunctionCommon, FunctionType, UserFunction};

/// VM error — replaces panic! in all runtime paths
#[derive(Debug)]
pub enum VmError {
    Fatal(String),
    UnimplementedOpcode(OpCode),
}

/// Write a new Value into a slot, properly dropping the old value first.
/// SAFETY: `ptr` must point to a valid, initialized Value.
#[inline]
unsafe fn write_val(ptr: *mut Value, val: Value) {
    std::ptr::drop_in_place(ptr);
    ptr.write(val);
}

/// Check if an exception value matches a catch clause's type list.
/// "Exception" catches everything (including non-object throws like strings/ints).
/// For objects, checks the class hierarchy.
fn exception_matches_catch(thrown: &Value, types: &[String], eg: &ExecutorGlobals) -> bool {
    if types.is_empty() {
        return true; // no type constraint = catch all
    }
    for type_name in types {
        // "Exception" is the universal catch-all in our simplified model
        if type_name.eq_ignore_ascii_case("Exception") || type_name.eq_ignore_ascii_case("Throwable") {
            return true;
        }
        // For object exceptions, check class hierarchy
        if let Some(obj) = thrown.as_object() {
            if eg.class_is_a(&obj.class_name, type_name) {
                return true;
            }
        }
    }
    false
}

/// Drop all CV and TMP slot values in a frame before popping it.
/// Only drops heap-allocated types (String, Array, Object).
/// Reference/Undef/Null/Bool/Long/Double are no-op drops — skip them entirely.
/// SAFETY: frame must be a valid ExecuteData pointer with initialized slots.
#[inline]
unsafe fn cleanup_frame_slots(frame: *mut ExecuteData) {
    let num_slots = (*frame).num_cvs as usize + (*frame).num_temps as usize;
    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
    for i in 0..num_slots {
        let ptr = base.add(i);
        match (*ptr).value_type() {
            ValueType::String | ValueType::Array | ValueType::Object => {
                std::ptr::drop_in_place(ptr);
            }
            _ => {} // Undef, Null, Bool, Long, Double, Reference — no-op
        }
    }
}

/// Execute a top-level script.
/// Result of throw_in_frame: either the exception was handled (new frame + op_array)
/// or it was not and should propagate via eg.exception.
enum ThrowResult<'a> {
    Handled(*mut ExecuteData, &'a crate::compiler::OpArray),
    Unhandled(Value),
}

/// Walk frames starting from `frame` looking for a try/catch handler for `thrown`.
/// On success: unwinds frames and returns the handler frame + op_array.
/// On failure: returns Unhandled with the original exception value.
fn throw_in_frame<'a>(
    eg: &mut ExecutorGlobals,
    mut frame: *mut ExecuteData,
    thrown: Value,
) -> ThrowResult<'a> {
    let mut search_frame = frame;
    loop {
        let sf_op_array = unsafe { (*search_frame).op_array() };
        let current_ip = unsafe {
            (*search_frame).opline.offset_from(sf_op_array.instructions.as_ptr()) as u32
        };

        let mut matched_entry: Option<&crate::compiler::compile::TryEntry> = None;
        for entry in &sf_op_array.try_entries {
            if current_ip >= entry.try_start && current_ip < entry.try_end {
                matched_entry = Some(entry);
                break;
            }
        }

        if let Some(entry) = matched_entry {
            let matched_catch = entry.catches.iter().find(|c| {
                exception_matches_catch(&thrown, &c.types, eg)
            });

            if let Some(catch) = matched_catch {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                }
                let base_ptr = sf_op_array.instructions.as_ptr();
                let catch_cv_ptr = unsafe { (*search_frame).get_op_mut(catch.catch_cv, OpType::Cv) };
                unsafe { write_val(catch_cv_ptr, thrown.clone()) };
                unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return ThrowResult::Handled(frame, new_op_array);
            } else if entry.finally_start != 0xFFFFFFFF {
                while frame != search_frame {
                    let prev = unsafe { (*frame).prev_execute_data };
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                }
                let base_ptr = sf_op_array.instructions.as_ptr();
                eg.exception = Some(thrown.clone());
                unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                let new_op_array = unsafe { (*frame).op_array() };
                return ThrowResult::Handled(frame, new_op_array);
            }
        }

        let prev = unsafe { (*search_frame).prev_execute_data };
        if prev.is_null() {
            break;
        }
        search_frame = prev;
    }

    ThrowResult::Unhandled(thrown)
}

pub fn execute(eg: &mut ExecutorGlobals, main_func: &UserFunction) -> Result<Value, VmError> {
    let func_ptr = &main_func.common as *const FunctionCommon;
    let frame = eg.vm_stack.push_call_frame(func_ptr, 0);

    let mut return_value = Value::null();
    unsafe {
        (*frame).return_value = &mut return_value;
        (*frame).opline = main_func.op_array.instructions.as_ptr();
        (*frame).prev_execute_data = eg.current_execute_data.get();
    }
    eg.current_execute_data.set(frame);

    execute_ex(eg, frame)?;

    eg.current_execute_data.set(unsafe { (*frame).prev_execute_data });
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    // Check for uncaught exception that propagated through execute_ex
    if let Some(exc) = eg.exception.take() {
        let msg = exc.echo_to_string();
        return Err(VmError::Fatal(format!("Uncaught Exception: {}", msg)));
    }

    Ok(return_value)
}

/// Call a PHP function by FunctionCommon pointer with given arguments.
/// Used by stdlib functions like array_map/array_filter for callback invocation.
pub fn call_function(
    eg: &mut ExecutorGlobals,
    func_ptr: *const FunctionCommon,
    args: &[Value],
) -> Result<Value, VmError> {
    let saved_execute_data = eg.current_execute_data.get();
    let frame = eg.vm_stack.push_call_frame(func_ptr, args.len() as u32);
    let mut return_value = Value::null();

    unsafe {
        (*frame).return_value = &mut return_value;
        // prev=null so Return exits execute_ex instead of continuing in caller
        (*frame).prev_execute_data = std::ptr::null_mut();
        (*frame).num_args = args.len() as u32;
    }

    // Write args into CV slots
    for (i, arg) in args.iter().enumerate() {
        let slot = unsafe { (*frame).cv_mut(i as u32) };
        unsafe { std::ptr::drop_in_place(slot as *mut Value) };
        unsafe { (slot as *mut Value).write(arg.clone()) };
    }

    let func = unsafe { Function::from_common_ptr(func_ptr) };
    match func.fn_type() {
        FunctionType::User => {
            let user = unsafe { func.as_user() };
            unsafe { (*frame).opline = user.op_array.instructions.as_ptr() };
            eg.current_execute_data.set(frame);
            execute_ex(eg, frame)?;
            // Exception from callback stays in eg.exception for DoFcall to handle.
            // Bail early so caller stops iterating, but don't convert to Err.
            if eg.exception.is_some() {
                eg.current_execute_data.set(saved_execute_data);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                return Ok(Value::null());
            }
        }
        FunctionType::Internal => {
            let internal = unsafe { func.as_internal() };
            unsafe { std::ptr::drop_in_place(&mut return_value as *mut Value) };
            if let Err(e) = (internal.handler)(frame, &mut return_value, eg) {
                eg.current_execute_data.set(saved_execute_data);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                return Err(e);
            }
            // Exception from callback stays in eg.exception for DoFcall to handle.
            if eg.exception.is_some() {
                eg.current_execute_data.set(saved_execute_data);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                return Ok(Value::null());
            }
        }
        FunctionType::Undef => {
            eg.current_execute_data.set(saved_execute_data);
            return Err(VmError::Fatal("Call to undefined function".into()));
        }
    }

    eg.current_execute_data.set(saved_execute_data);
    unsafe { cleanup_frame_slots(frame) };
    eg.vm_stack.pop_call_frame(frame);

    Ok(return_value)
}

/// Inner execute loop — equivalent to zend_execute_ex.
fn execute_ex(eg: &mut ExecutorGlobals, initial_frame: *mut ExecuteData) -> Result<(), VmError> {
    let mut frame = initial_frame;

    loop {
        let opline = unsafe { &*(*frame).opline };
        let mut op_array = unsafe { (*frame).op_array() };

        // Check for pending return or exception after finally block ends
        let frame_pending = unsafe { (*frame).pending_return_after_finally };
        let check_finally = frame_pending || eg.exception.is_some();
        if check_finally {
            let current_ip = unsafe {
                (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
            };
            let at_finally_end = op_array.try_entries.iter().any(|e| {
                e.finally_start != 0xFFFFFFFF && current_ip == e.finally_end
            });
            if at_finally_end {
                if frame_pending {
                    unsafe { (*frame).pending_return_after_finally = false; }
                    // Deferred return — pop frame now (return value already written)
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    eg.vm_stack.pop_call_frame(frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                } else {
                    // Real exception — re-enter throw/unwind to find outer handler
                    let pending = eg.exception.take().unwrap();
                    // Start from current frame (outer try/catch may be in same frame)
                    let mut search_frame = frame;
                    let mut found = false;
                    loop {
                        let sf_op_array = unsafe { (*search_frame).op_array() };
                        let sf_ip = unsafe {
                            (*search_frame).opline.offset_from(sf_op_array.instructions.as_ptr()) as u32
                        };
                        for entry in &sf_op_array.try_entries {
                            // Skip the entry whose finally we just finished
                            if entry.finally_start != 0xFFFFFFFF && sf_ip == entry.finally_end {
                                continue;
                            }
                            if sf_ip >= entry.try_start && sf_ip < entry.try_end {
                                // Unwind frames between current and search_frame
                                while frame != search_frame {
                                    let prev = unsafe { (*frame).prev_execute_data };
                                    eg.current_execute_data.set(prev);
                                    unsafe { cleanup_frame_slots(frame) };
                                    eg.vm_stack.pop_call_frame(frame);
                                    frame = prev;
                                }
                                let base_ptr = sf_op_array.instructions.as_ptr();
                                let matched_catch = entry.catches.iter().find(|c| {
                                    exception_matches_catch(&pending, &c.types, eg)
                                });
                                if let Some(catch) = matched_catch {
                                    let catch_cv_ptr = unsafe { (*search_frame).get_op_mut(catch.catch_cv, OpType::Cv) };
                                    unsafe { write_val(catch_cv_ptr, pending.clone()) };
                                    unsafe { (*frame).opline = base_ptr.add(catch.catch_start as usize) };
                                } else if entry.finally_start != 0xFFFFFFFF {
                                    eg.exception = Some(pending.clone());
                                    unsafe { (*frame).opline = base_ptr.add(entry.finally_start as usize) };
                                }
                                found = true;
                                break;
                            }
                        }
                        if found { break; }
                        let prev = unsafe { (*search_frame).prev_execute_data };
                        if prev.is_null() { break; }
                        search_frame = prev;
                    }
                    if found {
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    // Propagate via eg.exception for re-entry boundary crossing
                    eg.exception = Some(pending);
                    return Ok(());
                }
            }
        }

        match opline.opcode {
            OpCode::AssignCv => {
                // ASSIGN_CV op1=CV(dest), op2=value, result=optional copy
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let cloned = val.clone();
                let dest = unsafe { (*frame).get_op_mut(opline.op1, opline.op1_type) };
                unsafe { write_val(dest, cloned.clone()) };
                // If result is used, write a copy there too
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                    unsafe { write_val(result_ptr, cloned) };
                }
            }

            OpCode::Echo => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let output = val.echo_to_string();
                eg.write_output(output.as_bytes());
            }

            OpCode::Add => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { write_val(result_ptr, Value::long(sum)) },
                        None => unsafe {
                            write_val(result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { write_val(result_ptr, Value::double(d1 + d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for +".into()));
                }
            }

            OpCode::Sub => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { write_val(result_ptr, Value::long(diff)) },
                        None => unsafe {
                            write_val(result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { write_val(result_ptr, Value::double(d1 - d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for -".into()));
                }
            }

            OpCode::Mul => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match l1.checked_mul(l2) {
                        Some(prod) => unsafe { write_val(result_ptr, Value::long(prod)) },
                        None => unsafe {
                            write_val(result_ptr, Value::double(l1 as f64 * l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    unsafe { write_val(result_ptr, Value::double(d1 * d2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for *".into()));
                }
            }

            OpCode::Div => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    if d2 == 0.0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    // PHP: if both are long and divisible, result is long
                    if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                        if l2 != 0 && l1 % l2 == 0 {
                            unsafe { write_val(result_ptr, Value::long(l1 / l2)) };
                        } else {
                            unsafe { write_val(result_ptr, Value::double(d1 / d2)) };
                        }
                    } else {
                        unsafe { write_val(result_ptr, Value::double(d1 / d2)) };
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for /".into()));
                }
            }

            OpCode::Mod => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    if l2 == 0 {
                        return Err(VmError::Fatal("Division by zero".into()));
                    }
                    unsafe { write_val(result_ptr, Value::long(l1 % l2)) };
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for %".into()));
                }
            }

            OpCode::Concat => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                let s1 = op1.echo_to_string();
                let s2 = op2.echo_to_string();
                let concatenated = format!("{}{}", s1, s2);
                unsafe { write_val(result_ptr, Value::string(concatenated)) };
            }

            OpCode::IsEqual | OpCode::IsNotEqual | OpCode::IsSmaller | OpCode::IsSmallerOrEqual => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match opline.opcode {
                        OpCode::IsEqual => l1 == l2,
                        OpCode::IsNotEqual => l1 != l2,
                        OpCode::IsSmaller => l1 < l2,
                        OpCode::IsSmallerOrEqual => l1 <= l2,
                        _ => unreachable!(),
                    }
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    match opline.opcode {
                        OpCode::IsEqual => s1 == s2,
                        OpCode::IsNotEqual => s1 != s2,
                        OpCode::IsSmaller => s1 < s2,
                        OpCode::IsSmallerOrEqual => s1 <= s2,
                        _ => unreachable!(),
                    }
                } else if let (Some(d1), Some(d2)) = (op1.to_double(), op2.to_double()) {
                    match opline.opcode {
                        OpCode::IsEqual => d1 == d2,
                        OpCode::IsNotEqual => d1 != d2,
                        OpCode::IsSmaller => d1 < d2,
                        OpCode::IsSmallerOrEqual => d1 <= d2,
                        _ => unreachable!(),
                    }
                } else {
                    return Err(VmError::Fatal("Unsupported operand types for comparison".into()));
                };

                unsafe { write_val(result_ptr, Value::bool(result)) };
            }

            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                let identical = values_identical(op1, op2);

                let result = match opline.opcode {
                    OpCode::IsIdentical => identical,
                    _ => !identical,
                };
                unsafe { write_val(result_ptr, Value::bool(result)) };
            }

            OpCode::Isset => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                let is_set = val.value_type() != ValueType::Undef && val.value_type() != ValueType::Null;
                unsafe { write_val(result_ptr, Value::bool(is_set)) };
            }

            OpCode::Cast => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                let casted = match opline.extended_value {
                    0 => Value::long(val.to_long_val()),    // (int)
                    1 => Value::double(val.to_float_val()), // (float)
                    2 => Value::string(val.echo_to_string()), // (string)
                    3 => Value::bool(val.is_truthy()),      // (bool)
                    4 => {                                   // (array)
                        match val.value_type() {
                            ValueType::Array => val.clone(),
                            ValueType::Null | ValueType::Undef => Value::array(PhpArray::new()),
                            _ => {
                                let mut arr = PhpArray::new();
                                arr.push(val.clone());
                                Value::array(arr)
                            }
                        }
                    }
                    _ => val.clone(),
                };
                unsafe { write_val(result_ptr, casted) };
            }

            OpCode::BoolNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                let negated = !val.is_truthy();
                unsafe { write_val(result_ptr, Value::bool(negated)) };
            }

            OpCode::Jmp => {
                // op1 = absolute instruction index to jump to
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }
                continue; // skip normal advance
            }

            OpCode::JmpZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                if !val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
                    continue;
                }
            }

            OpCode::JmpNZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                if val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
                    continue;
                }
            }

            OpCode::InitFcall => {
                // op1 = num_args (extended_value in PHP, we use op1 for now)
                // op2 = CONST index pointing to function name string
                let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let name = name_val.as_str().unwrap_or_else(|| {
                    panic!("INIT_FCALL: op2 must be a string");
                });
                let func_ptr = eg.find_function(name).ok_or_else(|| {
                    VmError::Fatal(format!("Call to undefined function {}()", name))
                })?;

                let num_args = opline.op1;
                let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                unsafe {
                    (*call).prev_execute_data = frame;
                    // Save previous pending call in callee's call field (nested call chain)
                    (*call).call = (*frame).call;
                    // Link as pending call on current frame
                    (*frame).call = call;
                }
            }

            OpCode::SendVal => {
                // Send value to pending call frame
                // op1 = value to send, op2 = argument number (0-based)
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let cloned = val.clone();
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                // Arguments are stored in CV slots of the callee frame
                let arg_slot = unsafe { (*call).cv_mut(opline.op2) };
                *arg_slot = cloned;
            }

            OpCode::SendRef => {
                // Send reference to caller's CV into callee frame
                // op1 = CV index in caller, op1_type must be CV
                // op2 = argument number in callee (0-based)
                debug_assert!(opline.op1_type == OpType::Cv);
                let caller_cv_ptr = unsafe {
                    let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                    let raw_ptr = base.add(opline.op1 as usize);
                    // If caller's CV is itself a reference, forward the target
                    if (*raw_ptr).is_reference() {
                        (*raw_ptr).as_ref_ptr()
                    } else {
                        raw_ptr
                    }
                };
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let arg_slot = unsafe { (*call).cv_mut(opline.op2) };
                // Drop old undef in the slot, then write Reference
                unsafe { std::ptr::drop_in_place(arg_slot as *mut Value) };
                unsafe { (arg_slot as *mut Value).write(Value::reference(caller_cv_ptr)) };
            }

            OpCode::SendVarEx => {
                // Runtime-checked send: by-ref if callee expects it AND op1 is CV, else by-val
                // op2 = CV slot in callee, extended_value = parameter index for ref_args check
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let param_idx = opline.extended_value;
                let ref_args = unsafe { (*(*call).func).ref_args };
                let is_ref = param_idx < 64 && (ref_args & (1u64 << param_idx)) != 0;

                if is_ref && opline.op1_type == OpType::Cv {
                    // Same logic as SendRef
                    let caller_cv_ptr = unsafe {
                        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                        let raw_ptr = base.add(opline.op1 as usize);
                        if (*raw_ptr).is_reference() {
                            (*raw_ptr).as_ref_ptr()
                        } else {
                            raw_ptr
                        }
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2) };
                    unsafe { std::ptr::drop_in_place(arg_slot as *mut Value) };
                    unsafe { (arg_slot as *mut Value).write(Value::reference(caller_cv_ptr)) };
                } else {
                    // Same logic as SendVal
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2) };
                    *arg_slot = cloned;
                }
            }

            OpCode::DoFcall => {
                // Execute the pending call
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                // Restore previous pending call from the chain
                unsafe { (*frame).call = (*call).call };

                // Set up return value in result slot if used
                let return_value_ptr = if opline.result_type != OpType::Unused {
                    unsafe { (*frame).get_op_mut(opline.result, opline.result_type) }
                } else {
                    std::ptr::null_mut()
                };
                unsafe { (*call).return_value = return_value_ptr };

                // Validate argument count
                let func_common = unsafe { &*(*call).func };
                let num_args = unsafe { (*call).num_args };
                if num_args < func_common.required_num_args {
                    return Err(VmError::Fatal(format!(
                        "Too few arguments, {} passed and exactly {} expected",
                        num_args, func_common.required_num_args
                    )));
                }
                if !func_common.is_variadic && num_args > func_common.num_args {
                    return Err(VmError::Fatal(format!(
                        "Too many arguments, {} passed and at most {} expected",
                        num_args, func_common.num_args
                    )));
                }

                // Pack extra arguments into variadic parameter array
                if func_common.is_variadic {
                    let extra_count = num_args.saturating_sub(func_common.num_args);
                    let mut variadic_arr = crate::value::PhpArray::new();
                    let cv_start = func_common.variadic_cv_index;
                    for i in 0..extra_count {
                        let arg = unsafe { (*call).cv(cv_start + i) }.clone();
                        variadic_arr.push(arg);
                    }
                    // Overwrite the variadic CV slot with the packed array
                    let variadic_slot = unsafe { (*call).cv_mut(cv_start) };
                    *variadic_slot = crate::value::Value::array(variadic_arr);
                }

                // Direct fn_type check — avoids Function wrapper overhead
                let call_fn_type = unsafe { (*(*call).func).fn_type };
                match call_fn_type {
                    FunctionType::User => {
                        let user = unsafe { &*((*call).func as *const UserFunction) };
                        unsafe {
                            (*call).opline = user.op_array.instructions.as_ptr();
                            (*frame).opline = (*frame).opline.add(1);
                        }
                        eg.current_execute_data.set(call);
                        frame = call;
                        continue;
                    }
                    FunctionType::Internal => {
                        let internal = unsafe {
                            &*((*call).func as *const super::function::InternalFunction)
                        };
                        if !return_value_ptr.is_null() {
                            unsafe { std::ptr::drop_in_place(return_value_ptr) };
                        }
                        let handler_result = (internal.handler)(call, return_value_ptr, eg);
                        unsafe { cleanup_frame_slots(call) };
                        eg.vm_stack.pop_call_frame(call);
                        // 1) eg.exception set (real PHP throw from callback) → catchable
                        if let Some(exc) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exc) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                        // 2) Handler returned Err (hard fatal) → not catchable
                        if let Err(e) = handler_result {
                            return Err(e);
                        }
                    }
                    FunctionType::Undef => {
                        return Err(VmError::Fatal("Call to undefined function".into()));
                    }
                }
            }

            OpCode::PreInc => {
                // ++$var: increment CV in place, result = new value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1) // PHP: null++ = 1
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                    unsafe { write_val(result_ptr, new_val.clone()) };
                }
                unsafe { write_val(cv_ptr, new_val) };
            }

            OpCode::PreDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, new_val.clone()) };
                    }
                    unsafe { write_val(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, new_val.clone()) };
                    }
                    unsafe { write_val(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::PostInc => {
                // $var++: increment CV in place, result = old value
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let old_val = old.clone();
                let new_val = if let Some(n) = old.as_long() {
                    match n.checked_add(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 + 1.0),
                    }
                } else if let Some(d) = old.to_double() {
                    Value::double(d + 1.0)
                } else {
                    Value::long(1)
                };
                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                    unsafe { write_val(result_ptr, old_val) };
                }
                unsafe { write_val(cv_ptr, new_val) };
            }

            OpCode::PostDec => {
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1, OpType::Cv) };
                let old = unsafe { &*cv_ptr };
                let old_val = old.clone();
                if let Some(n) = old.as_long() {
                    let new_val = match n.checked_sub(1) {
                        Some(v) => Value::long(v),
                        None => Value::double(n as f64 - 1.0),
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, old_val) };
                    }
                    unsafe { write_val(cv_ptr, new_val) };
                } else if let Some(d) = old.to_double() {
                    let new_val = Value::double(d - 1.0);
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, old_val) };
                    }
                    unsafe { write_val(cv_ptr, new_val) };
                } else {
                    // PHP: null-- has no effect, value stays null
                    if opline.result_type != OpType::Unused {
                        let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                        unsafe { write_val(result_ptr, Value::null()) };
                    }
                }
            }

            OpCode::InitArray => {
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                unsafe { write_val(result_ptr, Value::array(PhpArray::new())) };
            }

            OpCode::AddArrayElement => {
                // op1 = array TMP, op2 = value, result = key (or Unused for auto-key)
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                let php_arr = arr.as_array_mut().ok_or_else(|| {
                    VmError::Fatal("AddArrayElement: operand is not an array".into())
                })?;
                if opline.result_type != OpType::Unused {
                    let key_val = unsafe { &*(*frame).get_op_ptr(opline.result, opline.result_type, op_array) };
                    let key = value_to_array_key(key_val)?;
                    php_arr.set(key, cloned_val);
                } else {
                    php_arr.push(cloned_val);
                }
            }

            OpCode::FetchDimR => {
                // result = op1[op2]
                let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let Some(arr) = arr_val.as_array() {
                    let key = value_to_array_key(idx_val)?;
                    let fetched = match &key {
                        ArrayKey::Int(k) => arr.get_int(*k),
                        ArrayKey::String(k) => arr.get_str(k),
                    };
                    let val = fetched.cloned().unwrap_or(Value::null());
                    unsafe { write_val(result_ptr, val) };
                } else if let Some(s) = arr_val.as_str() {
                    // String offset access: $s[0] — PHP strings are byte-oriented
                    let bytes = s.as_bytes();
                    if let Some(idx) = idx_val.as_long() {
                        let pos = if idx >= 0 {
                            idx as usize
                        } else {
                            let len = bytes.len() as i64;
                            let p = len + idx;
                            if p >= 0 { p as usize } else { usize::MAX }
                        };
                        let val = if pos < bytes.len() {
                            // Single byte as a string
                            Value::string(String::from(bytes[pos] as char))
                        } else {
                            Value::string("")
                        };
                        unsafe { write_val(result_ptr, val) };
                    } else {
                        unsafe { write_val(result_ptr, Value::null()) };
                    }
                } else {
                    unsafe { write_val(result_ptr, Value::null()) };
                }
            }

            OpCode::AssignDim => {
                // op1[op2] = result (value source encoded in result/result_type)
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let val = unsafe { &*(*frame).get_op_ptr(opline.result, opline.result_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { write_val(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().set(key, cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.set(key, cloned_val);
                } else {
                    return Err(VmError::Fatal("Cannot use a scalar value as an array".into()));
                }
            }

            OpCode::ArrayPushOp => {
                // op1[] = op2
                let val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let cloned_val = val.clone();
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { write_val(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().push(cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    php_arr.push(cloned_val);
                } else {
                    return Err(VmError::Fatal("[] operator not supported for non-array".into()));
                }
            }

            OpCode::UnsetDim => {
                // Remove key op2 from array op1
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let key = value_to_array_key(idx_val)?;
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                match arr.value_type() {
                    ValueType::Array => {
                        arr.as_array_mut().unwrap().remove(&key);
                    }
                    ValueType::Undef | ValueType::Null => {
                        // PHP silently ignores unset on undef/null
                    }
                    _ => {
                        return Err(VmError::Fatal(
                            "Cannot unset offset in a non-array variable".into(),
                        ));
                    }
                }
            }

            OpCode::ForeachInit => {
                // Copy array from op1 to result TMP, set position TMP (extended_value) to 0
                // If array is empty or not an array, jump to op2
                let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let is_empty = match arr_val.as_array() {
                    Some(arr) => arr.is_empty(),
                    None => {
                        // PHP: foreach() argument must be of type array|object
                        eg.write_output(b"\nWarning: foreach() argument must be of type array|object, ");
                        let type_name = match arr_val.value_type() {
                            ValueType::Null => "null",
                            ValueType::True | ValueType::False => "bool",
                            ValueType::Long => "int",
                            ValueType::Double => "float",
                            ValueType::String => "string",
                            _ => "unknown",
                        };
                        eg.write_output(type_name.as_bytes());
                        eg.write_output(b" given\n");
                        true
                    }
                };
                if is_empty {
                    // Jump to after-loop
                    let target = opline.op2 as usize;
                    let base_ptr = op_array.instructions.as_ptr();
                    unsafe { (*frame).opline = base_ptr.add(target) };
                    continue;
                }
                // Copy array to result TMP
                let cloned = arr_val.clone();
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                unsafe { write_val(result_ptr, cloned) };
                // Set position TMP to 0
                let pos_ptr = unsafe { (*frame).get_op_mut(opline.extended_value, OpType::Tmp) };
                unsafe { write_val(pos_ptr, Value::long(0)) };
            }

            OpCode::ForeachNext => {
                // op1 = array copy TMP, op2 = position TMP
                // result = has_more TMP (bool: true if entry fetched, false if done)
                // extended_value: low 16 bits = value_cv, high 16 bits = key_cv + 1 (0 = no key)
                let val_cv = (opline.extended_value & 0xFFFF) as u32;
                let key_encoded = (opline.extended_value >> 16) as u32;

                let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let pos_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let pos = pos_val.as_long().unwrap_or(0) as usize;

                let has_more = if let Some(arr) = arr_val.as_array() {
                    let entries = arr.entries();
                    if pos < entries.len() {
                        let (ref key, ref val) = entries[pos];
                        // Assign value CV
                        let val_ptr = unsafe { (*frame).get_op_mut(val_cv, OpType::Cv) };
                        unsafe { write_val(val_ptr, val.clone()) };
                        // Assign key CV if requested
                        if key_encoded > 0 {
                            let key_cv = key_encoded - 1;
                            let key_val = match key {
                                ArrayKey::Int(k) => Value::long(*k),
                                ArrayKey::String(k) => Value::string(k.clone()),
                            };
                            let key_ptr = unsafe { (*frame).get_op_mut(key_cv, OpType::Cv) };
                            unsafe { write_val(key_ptr, key_val) };
                        }
                        // Increment position
                        let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2, opline.op2_type) };
                        unsafe { write_val(pos_ptr, Value::long((pos + 1) as i64)) };
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                unsafe { write_val(result_ptr, Value::bool(has_more)) };
            }

            OpCode::Throw => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let thrown = val.clone();

                match throw_in_frame(eg, frame, thrown) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ThrowResult::Unhandled(exc) => {
                        // Propagate via eg.exception through re-entry boundaries
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                }
            }

            OpCode::NewObj => {
                let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let name = class_name.as_str().unwrap_or("");
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                // Create new object with default properties from class definition
                let mut props = std::collections::HashMap::new();
                if let Some(class_def) = eg.class_table.get(name) {
                    for (prop_name, default_val) in &class_def.properties {
                        let val = default_val.as_ref()
                            .map(|v| v.clone())
                            .unwrap_or(Value::null());
                        props.insert(prop_name.clone(), val);
                    }
                }

                let obj = PhpObject {
                    class_name: name.to_string(),
                    properties: props,
                };
                unsafe { write_val(result_ptr, Value::object(obj)) };

                // Check for __construct — set up call frame if it exists
                let num_args = opline.extended_value;
                let construct_name = format!("{}::__construct", name);
                if let Some(func_ptr) = eg.find_function(&construct_name) {
                    let call = eg.vm_stack.push_call_frame(func_ptr, num_args); // num_args from caller (excl $this)
                    unsafe {
                        (*call).prev_execute_data = frame;
                        (*call).call = (*frame).call;
                        (*frame).call = call;
                        // Set $this as CV 0
                        let this_ptr = (*call).cv_mut(0);
                        let obj_ref = &*result_ptr;
                        *this_ptr = obj_ref.clone();
                    }
                } else {
                    // No constructor — skip num_args SendVals + 1 DoFcall.
                    // Arg expressions were compiled before NewObj so side effects
                    // have already executed; we just discard the values.
                    let skip = num_args + 1; // SendVals + DoFcall
                    let base_ptr = op_array.instructions.as_ptr();
                    let current_ip = unsafe { (*frame).opline.offset_from(base_ptr) } as usize;
                    unsafe { (*frame).opline = base_ptr.add(current_ip + 1 + skip as usize) };
                    continue;
                }
            }

            OpCode::FetchObjR => {
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                if let Some(obj) = obj_val.as_object() {
                    let name = prop_name.as_str().unwrap_or("");
                    let val = obj.properties.get(name).cloned().unwrap_or(Value::null());
                    unsafe { write_val(result_ptr, val) };
                } else {
                    return Err(VmError::Fatal("Attempt to read property on non-object".into()));
                }
            }

            OpCode::AssignObjProp => {
                let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let val = unsafe { &*(*frame).get_op_ptr(opline.result, opline.result_type, op_array) };
                let cloned = val.clone();
                let name = prop_name.as_str().unwrap_or("").to_string();
                let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let obj = unsafe { &*obj_ptr };

                if let Some(mut php_obj) = obj.as_object_mut() {
                    php_obj.properties.insert(name, cloned);
                } else {
                    return Err(VmError::Fatal("Attempt to assign property on non-object".into()));
                }
            }

            OpCode::InitMethodCall => {
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let method = method_name.as_str().unwrap_or("");

                if let Some(obj) = obj_val.as_object() {
                    let full_name = format!("{}::{}", obj.class_name, method);
                    let func_ptr = eg.find_function(&full_name).ok_or_else(|| {
                        VmError::Fatal(format!("Call to undefined method {}::{}()", obj.class_name, method))
                    })?;

                    let num_args = opline.extended_value;
                    let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                    unsafe {
                        (*call).prev_execute_data = frame;
                        (*call).call = (*frame).call;
                        (*frame).call = call;
                        // Set $this as CV 0
                        let this_ptr = (*call).cv_mut(0);
                        *this_ptr = obj_val.clone();
                    }
                } else {
                    return Err(VmError::Fatal(format!("Call to member function {}() on non-object", method)));
                }
            }

            OpCode::InitStaticCall => {
                let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let class = class_name.as_str().unwrap_or("");
                let method = method_name.as_str().unwrap_or("");

                let full_name = format!("{}::{}", class, method);
                let func_ptr = eg.find_function(&full_name).ok_or_else(|| {
                    VmError::Fatal(format!("Call to undefined method {}::{}()", class, method))
                })?;

                let num_args = opline.extended_value;
                let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                unsafe {
                    (*call).prev_execute_data = frame;
                    (*call).call = (*frame).call;
                    (*frame).call = call;
                }
            }

            OpCode::InitDynamicCall => {
                let callable = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };

                if let Some(arr) = callable.as_array() {
                    // Closure call: array is [function_name, use_val1, use_val2, ...]
                    let entries = arr.entries();
                    if entries.is_empty() {
                        return Err(VmError::Fatal("Array is not callable".into()));
                    }
                    let func_name = entries[0].1.as_str().ok_or_else(|| {
                        VmError::Fatal("Closure descriptor must start with function name".into())
                    })?;

                    let func_ptr = eg.find_function(func_name).ok_or_else(|| {
                        VmError::Fatal(format!("Call to undefined function {}()", func_name))
                    })?;

                    let num_args = opline.extended_value;
                    let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                    unsafe {
                        (*call).prev_execute_data = frame;
                        (*call).call = (*frame).call;
                        (*frame).call = call;
                    }

                    // Copy captured use_vars into CV slots after params
                    // Params are CV 0..num_args-1, use_vars are CV num_args..
                    let func = unsafe { &*func_ptr };
                    let use_var_offset = func.num_args;
                    for i in 1..entries.len() {
                        let captured_val = entries[i].1.clone();
                        let cv_slot = unsafe { (*call).cv_mut(use_var_offset + (i as u32 - 1)) };
                        *cv_slot = captured_val;
                    }
                } else if let Some(func_name) = callable.as_str() {
                    // Simple string function call: $func = "my_func"; $func()
                    let func_ptr = eg.find_function(func_name).ok_or_else(|| {
                        VmError::Fatal(format!("Call to undefined function {}()", func_name))
                    })?;

                    let num_args = opline.extended_value;
                    let call = eg.vm_stack.push_call_frame(func_ptr, num_args);
                    unsafe {
                        (*call).prev_execute_data = frame;
                        (*call).call = (*frame).call;
                        (*frame).call = call;
                    }
                } else {
                    return Err(VmError::Fatal(format!("Value of type {:?} is not callable", callable.value_type())));
                }
            }

            OpCode::FetchStaticProp => {
                // For now, static properties are stored as class-level state
                // Simple implementation: look up in class table
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                unsafe { write_val(result_ptr, Value::null()) }; // TODO: implement properly
            }

            OpCode::Instanceof => {
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                let class_name = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                let target = class_name.as_str().unwrap_or("");
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };

                let is_instance = if let Some(obj) = obj_val.as_object() {
                    eg.class_is_a(&obj.class_name, target)
                } else {
                    false
                };
                unsafe { write_val(result_ptr, Value::bool(is_instance)) };
            }

            OpCode::FetchConst => {
                if opline.extended_value == 1 {
                    // Define mode: const FOO = value;
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                    let value_val = unsafe { &*(*frame).get_op_ptr(opline.op2, opline.op2_type, op_array) };
                    let name = name_val.as_str().unwrap_or("").to_string();
                    let value = value_val.clone();
                    eg.define_constant(&name, value).map_err(|e| VmError::Fatal(e))?;
                } else {
                    // Read mode: fetch constant value
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array) };
                    let name = name_val.as_str().unwrap_or("");
                    let value = eg.find_constant(name).ok_or_else(|| {
                        VmError::Fatal(format!("Undefined constant \"{}\"", name))
                    })?;
                    let result_ptr = unsafe { (*frame).get_op_mut(opline.result, opline.result_type) };
                    unsafe { write_val(result_ptr, value) };
                }
            }

            OpCode::BindDefaultParam => {
                // If CV slot is NOT undef (arg was passed), skip default init
                let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1, OpType::Cv) };
                let is_undef = unsafe { (*cv_ptr).is_undef() };
                if !is_undef {
                    // Jump past the default expr computation + AssignCv
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions.as_ptr().add(target);
                    }
                    continue;
                }
                // Otherwise fall through — next instructions compute and assign default
            }

            OpCode::Return => {
                // Check if we're inside a try region with a finally block
                let current_ip = unsafe {
                    (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
                };
                let mut need_finally: Option<u32> = None;
                for entry in &op_array.try_entries {
                    if current_ip >= entry.try_start && current_ip < entry.finally_end
                        && entry.finally_start != 0xFFFFFFFF
                        // Don't re-enter finally if we're already inside it
                        && current_ip < entry.finally_start
                    {
                        need_finally = Some(entry.finally_start);
                        break;
                    }
                }

                if let Some(finally_ip) = need_finally {
                    // Write return value now (so it's available after finally)
                    if opline.op1_type != OpType::Unused {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array)
                        };
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            unsafe { write_val(return_target, retval.clone()) };
                        }
                    }
                    // Jump to finally; after finally ends, the pending return
                    // will be detected by the finally_end check
                    eg.exception = None; // no exception, just deferred return
                    let base_ptr = op_array.instructions.as_ptr();
                    unsafe { (*frame).opline = base_ptr.add(finally_ip as usize) };
                    // Mark that we need to return after finally completes (per-frame)
                    unsafe { (*frame).pending_return_after_finally = true; }
                    continue;
                }

                // If returning from inside a finally block while an exception
                // is pending, the return suppresses the exception (PHP semantics).
                if eg.exception.is_some() {
                    eg.exception = None;
                }

                if opline.op1_type != OpType::Unused {
                    let retval = unsafe {
                        &*(*frame).get_op_ptr(opline.op1, opline.op1_type, op_array)
                    };
                    let return_target = unsafe { (*frame).return_value };
                    if !return_target.is_null() {
                        unsafe { write_val(return_target, retval.clone()) };
                    }
                }

                let prev = unsafe { (*frame).prev_execute_data };
                if prev.is_null() {
                    return Ok(());
                }

                eg.current_execute_data.set(prev);
                unsafe { cleanup_frame_slots(frame) };
                eg.vm_stack.pop_call_frame(frame);
                frame = prev;
                continue;
            }

            // All opcodes handled — new opcodes must be added above
        }

        // VM interrupt check
        if eg.vm_interrupt.load(Ordering::Relaxed) {
            handle_interrupt(eg)?;
        }

        // Advance to next instruction
        unsafe { (*frame).opline = (*frame).opline.add(1); }
    }
}

/// Convert a Value to an ArrayKey.
fn value_to_array_key(val: &Value) -> Result<ArrayKey, VmError> {
    match val.value_type() {
        ValueType::Long => Ok(ArrayKey::Int(val.as_long().unwrap())),
        ValueType::String => {
            let s = val.as_str().unwrap();
            // PHP normalizes numeric-string keys to integers:
            // "1" → Int(1), "-3" → Int(-3), but "01" or "1a" stay as String
            if let Ok(n) = s.parse::<i64>() {
                // Only normalize if the parsed integer stringifies back identically
                // (rejects "01", "+1", " 1", etc.)
                if n.to_string() == s {
                    return Ok(ArrayKey::Int(n));
                }
            }
            Ok(ArrayKey::String(s.to_string()))
        }
        ValueType::Null => Ok(ArrayKey::String(String::new())),
        ValueType::True => Ok(ArrayKey::Int(1)),
        ValueType::False => Ok(ArrayKey::Int(0)),
        ValueType::Double => Ok(ArrayKey::Int(val.as_double().unwrap() as i64)),
        other => Err(VmError::Fatal(format!("Illegal offset type {:?}", other))),
    }
}

/// PHP === comparison: same type and same value (recursive for arrays).
fn values_identical(a: &Value, b: &Value) -> bool {
    if a.value_type() != b.value_type() {
        return false;
    }
    match a.value_type() {
        ValueType::Undef | ValueType::Null => true,
        ValueType::True | ValueType::False => true,
        ValueType::Long => a.as_long() == b.as_long(),
        ValueType::Double => a.as_double() == b.as_double(),
        ValueType::String => a.as_str() == b.as_str(),
        ValueType::Array => {
            let arr_a = a.as_array().unwrap();
            let arr_b = b.as_array().unwrap();
            if arr_a.len() != arr_b.len() {
                return false;
            }
            // Same keys in same order, each value ===
            let entries_a = arr_a.entries();
            let entries_b = arr_b.entries();
            for ((ka, va), (kb, vb)) in entries_a.iter().zip(entries_b.iter()) {
                if ka != kb || !values_identical(va, vb) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn handle_interrupt(eg: &ExecutorGlobals) -> Result<(), VmError> {
    eg.vm_interrupt.store(false, Ordering::Relaxed);

    if eg.timed_out.load(Ordering::Relaxed) {
        eg.timed_out.store(false, Ordering::Relaxed);
        return Err(VmError::Fatal("Maximum execution time exceeded".into()));
    }

    Ok(())
}
