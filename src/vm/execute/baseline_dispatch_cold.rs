// Included in the execute module so cold opcode helpers keep private access to
// the canonical frame machinery without adding abstractions to hot dispatch.

#[inline(never)]
fn op_check_generic_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = unsafe {
        &mut *(op_array.cache.as_ptr().add(ip)
            as *mut crate::vm::instruction::InlineCache)
    };

    let mut kind = crate::generics::GenericDeclarationKind::from_tag(opline._pad)
        .ok_or_else(|| VmError::Fatal("Invalid generic declaration kind".into()))?;
    let raw_owner = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    let owner_value = if raw_owner.is_reference() {
        unsafe { &*raw_owner.as_ref_ptr() }
    } else {
        raw_owner
    };

    if let Some(declaration) = cache.generic_declaration() {
        let cache_hit = if kind == crate::generics::GenericDeclarationKind::Method
            && opline.op2_type == OpType::Const
        {
            owner_value
                .as_object()
                .is_some_and(|object| object.class_id == cache.class_id)
        } else if opline.op1_type == OpType::Const {
            true
        } else {
            owner_value
                .as_closure()
                .is_some_and(|closure| closure.func == cache.func)
        };
        if cache_hit {
            let binding = crate::generics::ReifiedBinding {
                declaration,
                use_site: opline.extended_value,
            };
            #[cfg(feature = "php-generics-reified")]
            eg.reified_bindings.push(binding);
            #[cfg(not(feature = "php-generics-reified"))]
            let _ = binding;
            return Ok(());
        }
    }

    let mut cacheable = opline.op1_type == OpType::Const;
    let mut receiver_class_id = 0;
    let mut callable = std::ptr::null();

    let mut owner = if kind == crate::generics::GenericDeclarationKind::Method
        && opline.op2_type == OpType::Const
    {
        let object = owner_value.as_object().ok_or_else(|| {
            VmError::Fatal("Generic method arguments require an object receiver".into())
        })?;
        let method = op_array.literals[opline.op2 as usize]
            .as_str()
            .unwrap_or("");
        cacheable = true;
        receiver_class_id = object.class_id;
        format!("{}::{}", object.class_name, method)
    } else if let Some(name) = owner_value.as_str() {
        name.to_string()
    } else if let Some(closure) = owner_value.as_closure() {
        let common = unsafe { &*closure.func };
        if common.fn_type != FunctionType::User {
            return Err(VmError::Fatal(
                "Generic arguments are not supported for this callable".into(),
            ));
        }
        kind = crate::generics::GenericDeclarationKind::Closure;
        cacheable = true;
        callable = closure.func;
        let user = unsafe { &*(closure.func as *const UserFunction) };
        user.op_array.name.clone()
    } else {
        return Err(VmError::Fatal(
            "Generic arguments require a named function, method, class, or closure".into(),
        ));
    };

    // Resolve inherited method metadata without storing anything on objects.
    if kind == crate::generics::GenericDeclarationKind::Method
        && eg.generic_metadata.find(kind, &owner).is_none()
    {
        if let Some((class_name, method)) = owner.split_once("::") {
            let mut current = class_name;
            while let Some(class) = eg.class_table.get(current) {
                let Some(parent) = class.parent.as_deref() else {
                    break;
                };
                let candidate = format!("{}::{}", parent, method);
                if eg.generic_metadata.find(kind, &candidate).is_some() {
                    owner = candidate;
                    break;
                }
                current = parent;
            }
        }
    }

    // Unqualified namespaced function calls retain PHP's global fallback.
    if kind == crate::generics::GenericDeclarationKind::Function
        && eg.generic_metadata.find(kind, &owner).is_none()
        && opline.op2_type == OpType::Const
    {
        if let Some(fallback) = op_array.literals[opline.op2 as usize].as_str() {
            owner = fallback.to_string();
        }
    }

    let binding = eg
        .generic_metadata
        .resolve_binding(kind, &owner, opline.extended_value, |actual, bound| {
            eg.class_is_a(actual, bound)
        })
        .map_err(VmError::Fatal)?;

    if cacheable {
        cache.set_generic_declaration(binding.declaration, receiver_class_id, callable);
    }

    #[cfg(feature = "php-generics-reified")]
    eg.reified_bindings.push(binding);

    #[cfg(not(feature = "php-generics-reified"))]
    let _ = binding;

    Ok(())
}

#[inline(never)]
fn op_check_reified_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<(), VmError> {
    #[cfg(not(feature = "php-generics-reified"))]
    {
        let _ = (eg, frame);
        return Err(VmError::Fatal(
            "Reified generic check emitted without php-generics-reified".into(),
        ));
    }

    #[cfg(feature = "php-generics-reified")]
    {
        let binding = *eg
            .reified_bindings
            .last()
            .ok_or_else(|| VmError::Fatal("Missing reified generic binding".into()))?;
        let declaration = eg
            .generic_metadata
            .declaration(binding)
            .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
        let call = unsafe { (*frame).call };
        if call.is_null() {
            return Err(VmError::Fatal(
                "Reified generic call has no pending call frame".into(),
            ));
        }
        let common = unsafe { &*(*call).func };
        for (index, expected) in declaration.value_parameters.iter().enumerate() {
            let Some(expected) = expected else {
                continue;
            };
            if index >= unsafe { (*call).num_args as usize } {
                continue;
            }
            let slot = common.sig.this_offset as usize + index;
            if slot >= unsafe { (*call).num_cvs as usize } {
                break;
            }
            let value = unsafe { (*call).cv(slot as u32) };
            if value.is_undef() {
                continue;
            }
            if !eg.generic_metadata.value_matches_binding(
                value,
                expected,
                binding,
                |actual, bound| eg.class_is_a(actual, bound),
            ) {
                let owner = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                return Err(VmError::Fatal(format!(
                    "Argument #{} passed to {} does not match its reified generic type",
                    index + 1,
                    owner
                )));
            }
        }
        Ok(())
    }
}

#[inline(never)]
fn op_check_reified_return(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    #[cfg(not(feature = "php-generics-reified"))]
    {
        let _ = (eg, frame, op_array, opline);
        return Err(VmError::Fatal(
            "Reified generic return check emitted without php-generics-reified".into(),
        ));
    }

    #[cfg(feature = "php-generics-reified")]
    {
        let binding = *eg
            .reified_bindings
            .last()
            .ok_or_else(|| VmError::Fatal("Missing reified generic binding".into()))?;
        let declaration = eg
            .generic_metadata
            .declaration(binding)
            .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
        if let Some(expected) = declaration.return_type.as_ref() {
            let value = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            if !eg.generic_metadata.value_matches_binding(
                value,
                expected,
                binding,
                |actual, bound| eg.class_is_a(actual, bound),
            ) {
                let owner = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                return Err(VmError::Fatal(format!(
                    "Return value of {} does not match its reified generic type",
                    owner
                )));
            }
        }
        eg.reified_bindings.pop();
        Ok(())
    }
}

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
