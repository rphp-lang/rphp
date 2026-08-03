// Included in the execute module so cold opcode helpers keep private access to
// the canonical frame machinery without adding abstractions to hot dispatch.

#[inline(never)]
fn op_call_user_func_array<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let callback_raw =
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let callback = if callback_raw.is_reference() {
        unsafe { &*callback_raw.as_ref_ptr() }
    } else {
        callback_raw
    };
    let args_raw = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let args = if args_raw.is_reference() {
        unsafe { &*args_raw.as_ref_ptr() }
    } else {
        args_raw
    };

    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache_slot =
        unsafe { op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache };
    let caller_class = get_caller_class(frame, eg);
    let result = crate::stdlib::invoke_call_user_func_array(
        callback,
        args,
        eg,
        caller_class.as_deref(),
        Some(cache_slot),
    )?;

    if let Some(exc) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exc) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    if opline.result_type != OpType::Unused {
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, result) };
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_fetch_static_prop(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let class_name_val =
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let prop_name_val =
        unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    let cls = class_name_val.as_str().unwrap_or("");
    let prop = prop_name_val.as_str().unwrap_or("");
    let mut found = false;
    if let Some(class_def) = eg.class_table.get(cls) {
        for (pname, default, _vis, _declaring) in &class_def.properties {
            if pname == prop {
                if let Some(val) = default {
                    unsafe { slot_set(result_ptr, val.clone()) };
                    found = true;
                }
                break;
            }
        }
    }
    if !found {
        unsafe { slot_set(result_ptr, Value::null()) };
    }
}

#[inline(never)]
fn op_instanceof(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let class_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let target = class_name.as_str().unwrap_or("");
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    let is_instance = obj_val
        .as_object()
        .is_some_and(|object| eg.class_is_a(&object.class_name, target));
    unsafe { slot_set(result_ptr, Value::bool(is_instance)) };
}

#[inline(never)]
fn op_fetch_const(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    if opline.extended_value == 1 {
        let name_val =
            unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let value_val =
            unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let name = name_val.as_str().unwrap_or("").to_string();
        eg.define_constant(&name, value_val.clone())
            .map_err(VmError::Fatal)?;
    } else {
        let name_val =
            unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let name = name_val.as_str().unwrap_or("");
        let value = eg
            .find_constant(name)
            .ok_or_else(|| VmError::Fatal(format!("Undefined constant \"{}\"", name)))?;
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, value) };
    }
    Ok(())
}

#[inline(never)]
fn op_bind_default_param(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> bool {
    let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
    if unsafe { (*cv_ptr).is_undef() } {
        return false;
    }
    unsafe {
        (*frame).opline = op_array.instructions.as_ptr().add(opline.op2 as usize);
    }
    true
}

#[inline(never)]
fn op_bind_global(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let name = name_val.as_str().unwrap_or("").to_string();
    if let Some(val) = eg.globals.get(&name) {
        let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
        unsafe { slot_set(cv_ptr, val.clone()) };
    }
}

#[inline(never)]
fn op_bind_static(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let var_name = name_val.as_str().unwrap_or("").to_string();
    let func_name = op_array.literals[opline.extended_value as usize]
        .as_str()
        .unwrap_or("")
        .to_string();
    let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };

    if let Some(value) = eg
        .static_vars
        .get(&func_name)
        .and_then(|statics| statics.get(&var_name))
    {
        unsafe { slot_set(cv_ptr, value.clone()) };
        return;
    }

    if opline.result_type != OpType::Unused {
        let default_val =
            unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
        unsafe { slot_set(cv_ptr, default_val.clone()) };
    } else {
        unsafe { slot_set(cv_ptr, Value::null()) };
    }
}

#[inline(never)]
fn op_create_closure(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cached = op_array.cache[ip].func;
    let func_ptr = if !cached.is_null() {
        cached
    } else {
        let name_val =
            unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let name = name_val
            .as_str()
            .expect("CreateClosure: op1 must be a function name string");
        let ptr = eg
            .find_function(name)
            .unwrap_or_else(|| panic!("CreateClosure: closure function {} not found", name));
        unsafe {
            (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func =
                ptr;
        }
        ptr
    };
    let closure = PhpClosure {
        func: func_ptr,
        captures: Vec::with_capacity(opline.extended_value as usize),
        has_heap_captures: false,
    };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    unsafe { frame_tmp_set(frame, result_ptr, Value::closure(closure)) };
}

#[inline(never)]
fn op_closure_use_var(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let value = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let cloned_value = value.clone();
    let closure_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
    let closure = unsafe { &mut *closure_ptr }
        .as_closure_mut()
        .expect("ClosureUseVar: op1 must be a closure");
    if cloned_value.needs_cleanup() {
        closure.has_heap_captures = true;
    }
    closure.captures.push(cloned_value);
}
