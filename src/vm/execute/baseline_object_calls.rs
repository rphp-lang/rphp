// Kept in the execute module through include! so this structural split does not change visibility or code generation.

unsafe fn try_execute_property_init_constructor(
    eg: &ExecutorGlobals,
    caller: *mut ExecuteData,
    caller_op_array: &crate::compiler::OpArray,
    sends: *const Instruction,
    object: &Value,
    callee: &UserFunction,
    plan: &PropertyInitMethodPlan,
    generic_contract: Option<&crate::generics::GenericMethodContract>,
    reified_protocol: bool,
) -> Option<*const Instruction> {
    let common = &callee.common;
    if common.sig.public_arity() != plan.public_args as u32
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.ref_args != 0
        || common.sig.is_variadic
        || plan.assignments.len() > 8
        || object.value_type() != ValueType::Object
        || object.is_reference()
    {
        return None;
    }

    let declaring_class = eg.declaring_class_of(&callee.common as *const FunctionCommon);
    let mut arguments = [std::ptr::null(); 8];
    for index in 0..plan.public_args as usize {
        let send = &*sends.add(index);
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as u32 != common.sig.param_cv_index(index as u32)
        {
            return None;
        }
        let value = match send.op1_type {
            OpType::Cv | OpType::Tmp | OpType::Var | OpType::Const => {
                &*(*caller).get_op_ptr(
                    send.op1 as u32,
                    send.op1_type,
                    caller_op_array,
                )
            }
            OpType::Unused => return None,
        };
        if value.is_reference() {
            return None;
        }
        let hint = common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        if !check_type_hint(
            value,
            hint,
            eg,
            caller_op_array.strict_types,
            declaring_class,
        ) {
            return None;
        }
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some((contract, expected)) = generic_contract.and_then(|contract| {
            contract
                .value_parameters
                .get(index)
                .and_then(Option::as_ref)
                .map(|expected| (contract, expected))
        })
            && !eg.value_matches_generic_method_contract(value, expected, contract)
        {
            return None;
        }
        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
        let _ = generic_contract;
        arguments[index] = value as *const Value;
    }

    let mut do_fcall_ptr = sends.add(plan.public_args as usize);
    if reified_protocol {
        if (*do_fcall_ptr).opcode != OpCode::CheckReifiedArgs {
            return None;
        }
        do_fcall_ptr = do_fcall_ptr.add(1);
    }
    let do_fcall = &*do_fcall_ptr;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
        || (reified_protocol && (*do_fcall_ptr.add(1)).opcode != OpCode::CheckReifiedReturn)
    {
        return None;
    }
    let class_id = object.object_class_id_unchecked();
    if class_id == 0 {
        return None;
    }
    let mut property_slots = [0usize; 8];
    for (index, assignment) in plan.assignments.iter().copied().enumerate() {
        let cache = callee.op_array.cache.get(assignment.cache_ip as usize)?;
        if cache.class_id != class_id {
            return None;
        }
        let argument = &*arguments[assignment.argument as usize];
        let called_class = object.object_class_name_unchecked();
        if !instance_property_cache_accepts_exact_non_generic_write(
            cache,
            argument,
            eg,
            called_class,
        ) {
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            {
                let declaration = if cache.property_flags() == 2 {
                    cache
                        .typed_instance_property_definition()?
                        .generic_declaration?
                } else {
                    cache.generic_property_declaration()?
                };
                let instruction = callee
                    .op_array
                    .instructions
                    .get(assignment.cache_ip as usize)?;
                let property = callee
                    .op_array
                    .literals
                    .get(instruction.op2 as usize)?
                    .as_str()?;
                if eg
                    .check_cached_generic_property_value(
                        object,
                        property,
                        argument,
                        declaration,
                    )
                    .is_err()
                {
                    return None;
                }
            }
            #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
            {
                return None;
            }
        }
        property_slots[index] = cache.property_slot();
    }

    for (index, assignment) in plan.assignments.iter().copied().enumerate() {
        let argument = &*arguments[assignment.argument as usize];
        object.object_set_property_slot_unchecked(property_slots[index], argument.clone());
    }
    if matches!(do_fcall.result_type, OpType::Tmp | OpType::Var) {
        let result_ptr = (caller as *mut Value)
            .add(CALL_FRAME_SLOTS + do_fcall.result as usize);
        frame_tmp_set(caller, result_ptr, Value::null());
    }
    record_scalar_call(common);
    (*caller).opline = do_fcall_ptr.add(1);
    Some(do_fcall_ptr)
}

#[inline(never)]
fn op_new_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: The compiler guarantees that this live frame owns NewObj's
    // operand/result slots and that opline addresses op_array's stable storage.
    let (class_operand, ip, result_ptr) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    let raw_name = class_operand.as_str().unwrap_or("");
    let ic = &op_array.cache[ip];
    let dynamic_static_scope = opline._pad & NEW_FLAG_DYNAMIC_STATIC_SCOPE != 0;
    let dynamic_class_name = opline._pad & NEW_FLAG_DYNAMIC_CLASS_NAME != 0;
    let literal_cache_hit = !dynamic_static_scope
        && !dynamic_class_name
        && opline.op1_type == OpType::Const
        && ic.class_id != 0;
    if literal_cache_hit {
        stats::inc_newobj_literal_cache_hit();
    } else if !dynamic_static_scope && !dynamic_class_name && opline.op1_type == OpType::Const {
        stats::inc_newobj_literal_cache_miss();
    }

    let owned_name: String;
    let name = if literal_cache_hit {
        // Const operands live in the immutable OpArray, so the warmed cache
        // can borrow its spelling without allocating or risking re-entry.
        raw_name
    } else {
        // Cold literal, late-static and runtime class expressions still own
        // their evaluated name before autoload can re-enter the VM.
        stats::inc_newobj_class_name_materialization();
        owned_name = if dynamic_static_scope {
            resolve_static_call_class(eg, frame, raw_name, true).ok_or_else(|| {
                VmError::Fatal(format!("Cannot access {raw_name} when no class scope is active"))
            })?
        } else if dynamic_class_name && class_operand.value_type() == ValueType::Object {
            class_operand
                .as_object()
                .expect("object class operand must remain live")
                .class_name
                .to_string()
        } else if dynamic_class_name && class_operand.value_type() != ValueType::String {
            let error = make_error_value("Error", "Class name must be a valid object or a string");
            attach_throwable_origin(&error, eg, frame, op_array, ip);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        } else {
            raw_name.to_string()
        };
        owned_name.as_str()
    };

    if !literal_cache_hit {
        stats::inc_newobj_class_hash_lookup();
    }
    if (dynamic_static_scope || dynamic_class_name || ic.class_id == 0)
        && eg.find_class(&name).is_none()
        && let Some(class_def) = eg.take_pending_anonymous_class(&name)
    {
        let dependencies = class_def
            .parent
            .iter()
            .chain(class_def.uses.iter())
            .chain(class_def.implements.iter())
            .cloned()
            .collect::<Vec<_>>();
        for dependency in dependencies {
            stats::inc_newobj_class_hash_lookup();
            if eg.find_class(&dependency).is_none()
                && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?
            {
                if let Some(exception) = eg.exception.take() {
                    return Ok(match throw_in_frame(eg, frame, exception) {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                }
                return Err(VmError::Fatal(format!(
                    "Class dependency \"{dependency}\" not found"
                )));
            }
        }
        if let Err(error) = eg.register_class(class_def) {
            let line = op_array.source_line(ip).unwrap_or(0);
            return Err(VmError::Fatal(format!(
                "{error} in {} on line {line}",
                op_array.name
            )));
        }
    }

    if !literal_cache_hit
        && {
            stats::inc_newobj_class_hash_lookup();
            eg.find_class(&name).is_none()
        }
    {
        let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &name)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        if !loaded {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Class \"{name}\" not found"),
            ));
        }
    }
    // Literal object creation is monomorphic in ordinary PHP code. After the
    // first canonical name lookup, use the stable numeric class index instead
    // of hashing the same class name on every allocation.
    let class_def = if literal_cache_hit {
        eg.class_by_id(ic.class_id)
    } else {
        stats::inc_newobj_class_hash_lookup();
        eg.find_class(&name)
    };
    op_new_obj_resolved(
        eg,
        frame,
        op_array,
        opline,
        ip,
        result_ptr,
        name,
        class_def.map(|class| class.class_id),
    )
}

#[cold]
fn new_object_validation_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    ip: usize,
    message: &str,
) -> ColdResult<'a> {
    let error = make_error_value("Error", message);
    attach_throwable_origin(&error, eg, frame, op_array, ip);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }
}

#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_newobj"))]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_newobj"))]
fn op_new_obj_resolved<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    ip: usize,
    result_ptr: *mut Value,
    name: &str,
    class_id: Option<u32>,
) -> Result<ColdResult<'a>, VmError> {
    let ic = &op_array.cache[ip];
    let class_def = class_id.and_then(|class_id| eg.class_by_id(class_id));

    // Reject instantiation through ordinary catchable Error objects. These
    // validation failures originate at NewObj and therefore retain its source
    // location and trace when they escape the current frame.
    if name == "Generator" {
        return Ok(new_object_validation_error(
            eg,
            frame,
            op_array,
            ip,
            "The \"Generator\" class is reserved for internal use and cannot be manually instantiated",
        ));
    }
    if let Some(class_def) = class_def {
        if class_def.is_interface {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Cannot instantiate interface {}", class_def.name),
            ));
        }
        if class_def.is_abstract {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Cannot instantiate abstract class {}", class_def.name),
            ));
        }
        if class_def.is_enum {
            let err = make_error_value("Error", &format!(
                "Cannot instantiate enum {}",
                class_def.name
            ));
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
            }
        }
    }

    // Create compact declared-property slots from the class layout.
    let (class_id, obj) = if let Some(class_def) = class_def {
        let class_id = class_def.class_id;
        (
            class_id,
            PhpObject::with_layout_from_defaults(
                class_id,
                class_def.property_layout.clone(),
                class_def.property_defaults.as_ref(),
            ),
        )
    } else {
        (
            0,
            PhpObject::dynamic(name.to_string(), 0, std::collections::HashMap::new()),
        )
    };
    // SAFETY: NewObj writes a compiler-owned TMP/VAR in this live frame for the first time. Stack
    // reuse intentionally leaves dead scalar bytes uninitialized, so dropping
    // the old bytes here is invalid; the tracked TMP writer treats an unset
    // bitmap bit as no live value and records the new object's ownership. The
    // active frame remains live for the complete opcode dispatch.
    let object = unsafe {
        frame_tmp_set(frame, result_ptr, Value::object(obj));
        &*result_ptr
    };
    if eg.class_is_a(name, "Throwable") {
        attach_throwable_origin(object, eg, frame, op_array, ip);
    }
    #[cfg(feature = "php-generics-reified")]
    if let Some(binding) = eg.reified_bindings.last().copied().filter(|binding| {
        eg.generic_metadata
            .declaration(*binding)
            .is_some_and(|declaration| {
                declaration.kind == crate::generics::GenericDeclarationKind::Class
                    && eg
                        .generic_metadata
                        .symbol(declaration.owner)
                        .is_some_and(|owner| owner.eq_ignore_ascii_case(&name))
            })
    }) {
        let object = unsafe { &*result_ptr };
        eg.bind_reified_object(object, binding);
    }
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if let Some(class) = eg.class_by_id(class_id) {
        let object = unsafe { &*result_ptr };
        for property in &class.properties {
            if let Some(default) = &property.default {
                eg.check_generic_property_value(object, &class.name, &property.name, default)
                    .map_err(VmError::Fatal)?;
            }
        }
    }

    // Constructor lookup is invariant for this literal `new ClassName` site.
    // Cache both hits and misses under the stable class ID so repeated object
    // allocation does not format, lowercase, allocate and hash the same method
    // name every time. A changed/re-registered class gets a different ID and
    // therefore resolves again.
    let num_args = opline.extended_value;
    let func_ptr = if class_id != 0 && ic.class_id == class_id {
        ic.func
    } else {
        let construct_name = format!("{}::__construct", name);
        let resolved = eg.find_function(&construct_name).unwrap_or(std::ptr::null());
        if class_id != 0 {
            let ic_mut = unsafe {
                &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache)
            };
            ic_mut.set_constructor(resolved, class_id);
        }
        resolved
    };
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let generic_constructor_contract = if func_ptr.is_null() {
        None
    } else {
        let object = unsafe { &*result_ptr };
        eg.generic_instance_method_contract(object, "__construct")
    };
    #[cfg(feature = "php-generics-reified")]
    let reified_construction = {
        let object = unsafe { &*result_ptr };
        eg.reified_object_binding(object).is_some()
    };
    #[cfg(not(feature = "php-generics-reified"))]
    let reified_construction = false;
    if opline._pad & NEW_FLAG_UNPACKED_ARGUMENTS != 0 {
        if !func_ptr.is_null() {
            // SAFETY: result_ptr is the just-initialized object result slot and
            // op2 is the compiler-owned argument list consumed synchronously
            // before either operand or the active frame can be released.
            let (object, arguments) = unsafe {
                (
                    &*result_ptr,
                    &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
                )
            };
            let resolved = crate::stdlib::ResolvedCallback {
                func_ptr,
                prepend_args: vec![object.clone()],
                use_vars: Vec::new(),
                called_scope_class_id: class_id,
                bound_this: None,
                closure_static_vars: None,
                is_magic_call: false,
            };
            let source_file = if op_array.source_file.is_empty() {
                op_array.name.as_str()
            } else {
                op_array.source_file.as_str()
            };
            let _ = crate::stdlib::invoke_resolved_source_unpacked_call(
                resolved,
                arguments,
                eg,
                source_file,
                op_array.strict_types,
            )?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }
        return Ok(ColdResult::Done);
    }
    if !func_ptr.is_null() {
        let common = unsafe { &*func_ptr };
        if common.fn_type == FunctionType::User {
            let user = unsafe { &*(func_ptr as *const UserFunction) };
            if let Some(plan) = user.property_init_plan.as_deref() {
                let object = unsafe { &*result_ptr };
                if unsafe {
                    try_execute_property_init_constructor(
                        eg,
                        frame,
                        op_array,
                        (opline as *const Instruction).add(1),
                        object,
                        user,
                        plan,
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        generic_constructor_contract.as_deref(),
                        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                        None,
                        reified_construction,
                    )
                }
                .is_some()
                {
                    return Ok(ColdResult::Continue);
                }
            }
        }
        // +1 for $this at CV 0; SendVal writes args to CV 1..N
        let pending_call = unsafe { (*frame).call };
        let call = eg.vm_stack.push_call_frame(
            func_ptr,
            num_args + 1,
            num_args,
            frame,
            pending_call,
        );
        unsafe {
            (*frame).call = call;
            // Write $this directly — cleanup handles it separately.
            let obj_ref = &*result_ptr;
            frame_set_this(call, obj_ref.clone());
        }
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(contract) = generic_constructor_contract {
            eg.push_pending_generic_member_call(call as usize, contract);
        }
    } else {
        // No constructor — skip the call protocol through DoFcall. Explicit
        // reified construction may insert a boundary check between SendVals
        // and DoFcall plus a return/object-binding check after it, so locate
        // the terminator instead of relying on a fixed instruction count.
        // Arg expressions were compiled before NewObj so side effects
        // have already executed; we just discard the values.
        let base_ptr = op_array.instructions.as_ptr();
        let current_ip = unsafe { (*frame).opline.offset_from(base_ptr) } as usize;
        let do_fcall_ip = op_array.instructions[current_ip + 1..]
            .iter()
            .position(|instruction| instruction.opcode == OpCode::DoFcall)
            .map(|offset| current_ip + 1 + offset)
            .ok_or_else(|| VmError::Fatal("new expression is missing DoFcall".into()))?;
        unsafe { (*frame).opline = base_ptr.add(do_fcall_ip + 1) };
        return Ok(ColdResult::Continue);
    }
    Ok(ColdResult::Done)
}

/// Execute the common declared-public-property read without leaving the main
/// dispatch loop. The inline cache proves both the receiver class and stable
/// property slot; misses retain the complete visibility, magic-method and
/// dynamic-property behavior in `op_fetch_obj_r_slow`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CachedFetchObjResult {
    Miss,
    Complete,
    CompleteAndSkipNext,
}

#[inline]
fn take_magic_exception<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Option<ColdResult<'a>> {
    eg.exception.take().map(|exception| match throw_in_frame(eg, frame, exception) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

#[inline(always)]
fn finish_cached_fetch_obj_r(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    property_ptr: *const Value,
) -> CachedFetchObjResult {
    // SAFETY: callers obtain `property_ptr` from a guarded object layout and
    // perform no mutable property operation before this helper returns.
    let property = unsafe { &*property_ptr };
    // A class cache is shared by every instance. Another object of the same
    // class may still hold the typed-property undef sentinel even after this
    // site was warmed by an initialized instance; let the cold path produce
    // the catchable PHP Error with declaration metadata.
    if property.is_undef() {
        return CachedFetchObjResult::Miss;
    }
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    if let Some(strlen) = op_array.instructions.get(ip + 1) {
        let consumes_fetch = matches!(strlen.opcode, OpCode::Strlen | OpCode::Strlen_String)
            && matches!(opline.result_type, OpType::Tmp | OpType::Var)
            && strlen.op1_type == opline.result_type
            && strlen.op1 == opline.result
            && matches!(strlen.result_type, OpType::Tmp | OpType::Var);
        if consumes_fetch && property.value_type() == ValueType::String {
            let length = unsafe { property.as_str().unwrap_unchecked().len() as i64 };
            let result_ptr = unsafe {
                (*frame).get_op_mut(strlen.result as u32, strlen.result_type)
            };
            unsafe { frame_tmp_set_long(frame, result_ptr, length) };
            return CachedFetchObjResult::CompleteAndSkipNext;
        }
    }

    let result_ptr = unsafe {
        (*frame).get_op_mut(opline.result as u32, opline.result_type)
    };
    unsafe { frame_slot_set(frame, result_ptr, (*property_ptr).clone()) };
    CachedFetchObjResult::Complete
}

#[inline(always)]
fn try_cached_fetch_obj_r(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> CachedFetchObjResult {
    let obj_val = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    }
    .dereferenced();
    if obj_val.value_type() != ValueType::Object {
        return CachedFetchObjResult::Miss;
    }

    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = &op_array.cache[ip];
    if cache.is_dynamic_property_read() {
        if unsafe { obj_val.object_property_layout_ptr_unchecked() }
            != cache.dynamic_property_layout()
        {
            return CachedFetchObjResult::Miss;
        }
        let prop_name = unsafe {
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
        };
        let Some(name) = prop_name.as_str() else {
            return CachedFetchObjResult::Miss;
        };
        let mut property_ptr = cache.dynamic_property_position().map_or(
            std::ptr::null(),
            |position| unsafe {
                obj_val.object_dynamic_property_at_unchecked(name, position)
            },
        );
        if property_ptr.is_null() {
            property_ptr = unsafe { obj_val.object_dynamic_property_unchecked(name) };
        }
        if property_ptr.is_null() {
            return CachedFetchObjResult::Miss;
        }
        return finish_cached_fetch_obj_r(frame, op_array, opline, property_ptr);
    }

    let object_class_id = unsafe { obj_val.object_class_id_unchecked() };
    if cache.property_flags() & 1 == 0
        || cache.class_id != object_class_id
        || object_class_id == 0
    {
        return CachedFetchObjResult::Miss;
    }

    let property_ptr = unsafe {
        obj_val.object_property_slot_unchecked(cache.property_slot())
    };
    finish_cached_fetch_obj_r(frame, op_array, opline, property_ptr)
}

#[cold]
#[inline(never)]
fn scalar_property_write_fetch_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    name: &str,
    receiver_type: &str,
    flags: u16,
) -> ColdResult<'a> {
    let action = if flags & FETCH_OBJ_COMPOUND != 0 {
        "assign"
    } else if flags & FETCH_OBJ_INCDEC != 0 {
        "increment/decrement"
    } else {
        "modify"
    };
    object_property_throw(
        eg,
        frame,
        "Error",
        format!("Attempt to {action} property \"{name}\" on {receiver_type}"),
    )
}

#[inline(never)]
fn op_fetch_obj_r_slow<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and compiler-emitted operands and
    // result slot; none of these borrows escape this non-reentrant opcode.
    let (obj_val, prop_name, result_ptr) = unsafe {
        (
            (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
                .dereferenced(),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    let set_result = |value| {
        // SAFETY: `result_ptr` is the live compiler-emitted slot proven above;
        // each invocation transfers exactly one owned Value into it.
        unsafe { frame_slot_set(frame, result_ptr, value) };
    };

    if obj_val.value_type() != ValueType::Object {
        if opline._pad & FETCH_OBJ_SILENT != 0 {
            set_result(Value::null());
            return Ok(ColdResult::Done);
        }
        let name = prop_name
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| prop_name.echo_to_string());
        let write_flags =
            opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND);
        if write_flags != 0 {
            return Ok(scalar_property_write_fetch_throw(
                eg,
                frame,
                &name,
                obj_val.type_name(),
                write_flags,
            ));
        }
        report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            &format!(
                "Attempt to read property \"{name}\" on {}",
                obj_val.type_name()
            ),
            opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
        )?;
        if let Some(result) = take_magic_exception(eg, frame) {
            return Ok(result);
        }
        set_result(Value::null());
        return Ok(ColdResult::Done);
    }

    // Property-name conversion and magic accessors may rebind the CV/global
    // slots that supplied either operand. Keep opcode-local owners before any
    // re-entrant call so later phases still address the original values.
    let receiver = obj_val.clone();
    let obj_val = &receiver;
    let property_name_owner = prop_name.dereferenced().clone();

    let name = if property_name_owner.value_type() == ValueType::Object {
        let class_name = property_name_owner
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string());
        let rendered = call_magic_method(eg, &property_name_owner, "__tostring", &[])?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let Some(rendered) = rendered else {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Object of class {class_name} could not be converted to string"),
            ));
        };
        let Some(rendered) = rendered.as_str() else {
            return Ok(object_property_throw(
                eg,
                frame,
                "TypeError",
                format!("{class_name}::__toString(): Return value must be of type string"),
            ));
        };
        rendered.to_string()
    } else {
        if property_name_owner.value_type() == ValueType::Array {
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                "Array to string conversion",
                opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
        }
        property_name_owner
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| property_name_owner.echo_to_string())
    };
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };

    if let Some(obj) = obj_val.as_object() {

        // ── Full resolution (cache miss or private/protected) ──
        let caller_class = get_caller_class(frame, eg);

        // Private property early binding is only valid when the receiver
        // is in the same inheritance hierarchy as the caller.  When
        // accessing an unrelated object, the caller's private property
        // must NOT leak — use target-only key resolution.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Resolve storage key (mangled for private properties)
        let mut key = crate::runtime::resolve_property_key(eg, &obj.class_name, &name, effective_caller);

        // Determine if property is public (for caching)
        let mut is_public = true;
        let mut property_accessible = true;
        let mut force_dynamic = false;
        // Visibility check
        if let Some((vis, defining_class)) = eg.find_property_visibility(&obj.class_name, &name) {
            if vis != Visibility::Public {
                is_public = false;
                // Skip check if the caller owns the defining class AND
                // the receiver is in that scope (same hierarchy).
                let own_private = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    vis == Visibility::Private && defining_class.eq_ignore_ascii_case(cc)
                });
                // Also skip if caller's class declares its own private
                // with same name AND the receiver is in scope.
                let caller_has_own = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    if let Some((Visibility::Private, ref dc)) = eg.find_property_visibility(cc, &name) {
                        dc.eq_ignore_ascii_case(cc)
                    } else {
                        false
                    }
                });
                // A parent's private property is not a member of the child
                // scope. Unless the declaring parent is the caller, reading
                // it through a child object follows the ordinary undefined
                // property path (including __get and its warning) rather than
                // reporting an inaccessible declaration.
                let hidden_parent_private = vis == Visibility::Private
                    && !defining_class.eq_ignore_ascii_case(&obj.class_name)
                    && !own_private;
                if hidden_parent_private {
                    key = name.to_string();
                    force_dynamic = true;
                } else if !own_private && !caller_has_own {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let has_getter = eg
                            .find_function(&format!(
                                "{}::__get",
                                obj.class_name.to_ascii_lowercase()
                            ))
                            .is_some();
                        if opline._pad & FETCH_OBJ_SILENT == 0 && !has_getter {
                            let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                            let message = format!(
                                "Cannot access {} property {}::${}",
                                vis_str, defining_class, name
                            );
                            drop(obj);
                            return Ok(object_property_throw(eg, frame, "Error", message));
                        }
                        property_accessible = false;
                    }
                }
            }
        }

        // Lazy objects keep every triggering property at the undef sentinel,
        // so their fast cache naturally lands here. Initialize only after
        // visibility/key resolution, then continue the same access against
        // the ghost or the real proxy instance without repeating name side
        // effects.
        let declared_property = obj.property_slot(&key).is_some();
        let dynamic_property = obj.get_dynamic_property_with_position(&key).is_some();
        let class_name = obj.class_name.clone();
        drop(obj);
        let has_magic_get = eg
            .find_function(&format!("{}::__get", class_name.to_ascii_lowercase()))
            .is_some();
        let has_magic_isset = eg
            .find_function(&format!(
                "{}::__isset",
                class_name.to_ascii_lowercase()
            ))
            .is_some();
        let magic_get_can_handle = !declared_property
            && !dynamic_property
            && ((has_magic_get
                && !property_guard_active(eg, obj_val, &name, PROPERTY_GUARD_GET))
                || (opline._pad & FETCH_OBJ_SILENT != 0
                    && has_magic_isset
                    && !property_guard_active(eg, obj_val, &name, PROPERTY_GUARD_ISSET)));
        let must_initialize = (property_accessible || force_dynamic)
            && !dynamic_property
            && !magic_get_can_handle
            && eg.lazy_property_requires_initialization(obj_val, &key);
        let initialized_target = if must_initialize {
            Some(crate::stdlib::reflection::initialize_lazy_object(
                eg, obj_val,
            )?)
        } else {
            eg.lazy_proxy_instance(obj_val)
        };
        if let Some(result) = take_magic_exception(eg, frame) {
            return Ok(result);
        }
        let magic_receiver = obj_val;
        let obj_val = initialized_target.as_ref().unwrap_or(obj_val);
        let obj = obj_val
            .as_object()
            .expect("lazy initialization must preserve an object receiver");

        // Cache only declared public properties. Dynamic properties have no
        // stable slot and remain on the cold lookup path.
        let cache_dynamic_std_class =
            is_public && key == name && obj.is_dynamic_std_class();
        if is_public && key == name && obj.class_id != 0 {
            if let Some(slot) = obj.property_slot(&key) {
                let has_get_hook = eg
                    .instance_property_definition(obj.class_id, slot)
                    .is_some_and(|definition| definition.has_get_hook);
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
                let mut flags: u32 = 1; // read-safe
                let writable = eg.class_table.get(obj.class_name.as_ref()).is_none_or(|cd| {
                    !cd.is_enum
                        && !cd.readonly_props.iter().any(|prop| prop == &name)
                        && eg
                            .find_property_set_visibility(&obj.class_name, &name)
                            .is_none_or(|(visibility, _)| visibility == Visibility::Public)
                });
                if writable {
                    flags |= 2;
                }
                if !has_get_hook {
                    ic_mut.set_property(obj.class_id, slot, flags);
                }
            }
        }

        let declared_slot = (!force_dynamic && property_accessible)
            .then(|| obj.property_slot(&key))
            .flatten();
        let definition = declared_slot.and_then(|slot| {
            eg.instance_property_definition(obj.class_id, slot)
        });
        let has_get_hook = definition.is_some_and(|definition| definition.has_get_hook);
        let write_only_property = definition.is_some_and(|definition| {
            definition.has_set_hook
                && !definition.has_get_hook
                && !definition.set_hook_is_backed
        });
        let typed_property = definition
            .filter(|definition| definition.is_typed())
            .map(|definition| (definition.type_scope.clone(), definition.name.clone()));
        let (found_val, dynamic_position) = if force_dynamic {
            match obj.get_dynamic_property_with_position(&key) {
                Some((value, position)) => (Some(value.clone()), position),
                None => (None, None),
            }
        } else if !property_accessible {
            (None, None)
        } else if cache_dynamic_std_class {
            match obj.get_dynamic_property_with_position(&key) {
                Some((value, position)) => (Some(value.clone()), position),
                None => (None, None),
            }
        } else {
            (obj.get_property(&key).cloned(), None)
        };
        if cache_dynamic_std_class && found_val.is_some() {
            let ic_mut = unsafe {
                &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache)
            };
            ic_mut.set_dynamic_property_read(obj.property_layout_ptr(), dynamic_position);
        }
        drop(obj); // Release borrow before potential magic method call
        if write_only_property
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET)
        {
            let class_name = obj_val
                .as_object()
                .map(|object| object.class_name.to_string())
                .unwrap_or_else(|| "object".to_string());
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Property {class_name}::${name} is write-only"),
            ));
        }
        if has_get_hook
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_GET)
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET)
        {
            let hook_name = format!("${name}::get");
            let hook_value = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_GET,
                &hook_name,
                &[],
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if let Some(value) = hook_value {
                if opline._pad & FETCH_OBJ_MODIFY != 0
                    && !value.is_reference()
                    && value.dereferenced().value_type() != ValueType::Object
                {
                    let class_name = obj_val
                        .as_object()
                        .map(|object| object.class_name.to_string())
                        .unwrap_or_else(|| "object".to_string());
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "Error",
                        format!("Indirect modification of {class_name}::${name} is not allowed"),
                    ));
                }
                set_result(value);
                return Ok(ColdResult::Done);
            }
        }
        if let Some(val) = found_val {
            if val.is_undef() && typed_property.is_some() {
                if opline._pad & FETCH_OBJ_SILENT != 0 {
                    set_result(Value::null());
                    return Ok(ColdResult::Done);
                }
                let (type_scope, property_name) = typed_property.as_ref().unwrap();
                let error = make_error_value(
                    "Error",
                    &format!(
                        "Typed property {}::${} must not be accessed before initialization",
                        type_scope, property_name
                    ),
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            set_result(val);
        } else {
            // An intermediate property in `isset($object->a->b)` first asks
            // `__isset(a)` and invokes `__get(a)` only when it returns true.
            if opline._pad & FETCH_OBJ_SILENT != 0 {
                let directly_isset_guarded = magic_receiver
                    .as_object()
                    .is_some_and(|object| {
                        object.property_guard_active(&name, PROPERTY_GUARD_ISSET)
                    });
                let isset_guarded = directly_isset_guarded
                    || eg.lazy_proxy_related_property_guard_active(
                        magic_receiver,
                        &name,
                        PROPERTY_GUARD_ISSET,
                    );
                let magic_set = if isset_guarded {
                    None
                } else {
                    call_guarded_property_magic_method(
                        eg,
                        magic_receiver,
                        &name,
                        PROPERTY_GUARD_ISSET,
                        "__isset",
                        &[Value::string(name.clone())],
                    )?
                };
                if let Some(result) = take_magic_exception(eg, frame) {
                    return Ok(result);
                }
                let guarded_lazy_get = directly_isset_guarded
                    && has_magic_get
                    && eg.lazy_object_state(magic_receiver).is_some();
                if (!isset_guarded && !magic_set.is_some_and(|value| value.is_truthy()))
                    || (isset_guarded && !guarded_lazy_get)
                {
                    set_result(Value::null());
                    return Ok(ColdResult::Done);
                }
            }
            // Property not found (or accepted by __isset) — try __get.
            if name.starts_with('\0')
                && property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_GET)
            {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    "Cannot access property starting with \"\\0\"".into(),
                ));
            }
            let magic_value = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_GET,
                "__get",
                &[Value::string(name.clone())],
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if let Some(result) = magic_value {
                set_result(result);
            } else if name.starts_with('\0') {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    "Cannot access property starting with \"\\0\"".into(),
                ));
            } else {
                let class_name = obj_val
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| "object".to_string());
                let write_flags =
                    opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND);
                if write_flags != 0 {
                    if eg
                        .class_table
                        .get(class_name.as_str())
                        .is_some_and(|class_def| class_def.is_readonly)
                    {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "Error",
                            format!("Cannot create dynamic property {class_name}::${name}"),
                        ));
                    }
                    let dynamic_properties_allowed = obj_val.as_object().is_some_and(|object| {
                        object.is_dynamic_std_class()
                            || eg
                                .class_table
                                .get(object.class_name.as_ref())
                                .is_some_and(|class_def| class_def.allow_dynamic_properties)
                    });
                    if !dynamic_properties_allowed {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Creation of dynamic property {class_name}::${name} is deprecated"
                            ),
                        )?;
                        if let Some(result) = take_magic_exception(eg, frame) {
                            return Ok(result);
                        }
                    }
                    // Publish the missing member before the read-side warning.
                    // The following reference/write opcode then observes the
                    // same first-creation boundary and cannot diagnose twice.
                    if let Some(mut object) = obj_val.as_object_mut() {
                        object.set_property(&name, Value::null());
                    }
                }
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!("Undefined property: {class_name}::${name}"),
                    opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
                )?;
                if let Some(result) = take_magic_exception(eg, frame) {
                    return Ok(result);
                }
                set_result(Value::null());
            }
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_isset_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and compiler-emitted operands and
    // result slot; none of these borrows escape this non-reentrant opcode.
    let (object, property, result_ptr) = unsafe {
        (
            (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
                .dereferenced(),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    let set_result = |value| {
        // SAFETY: `result_ptr` is the live compiler-emitted TMP proven above.
        unsafe { frame_tmp_set_bool(frame, result_ptr, value) };
    };

    if object.value_type() != ValueType::Object {
        set_result(false);
        return Ok(ColdResult::Done);
    }
    let lazy_receiver_owner = eg.lazy_object_state(object).map(|_| object.clone());
    let object = lazy_receiver_owner.as_ref().unwrap_or(object);
    let name = property.as_str().unwrap_or("");
    let caller_class = get_caller_class(frame, eg);
    let object_ref = object.as_object().expect("object tag must expose object storage");
    let receiver_in_scope = caller_class
        .as_ref()
        .is_some_and(|caller| eg.class_is_a(&object_ref.class_name, caller));
    let effective_caller = receiver_in_scope
        .then_some(caller_class.as_deref())
        .flatten();
    let accessible = eg
        .find_property_visibility(&object_ref.class_name, name)
        .is_none_or(|(visibility, defining_class)| {
            visibility == Visibility::Public
                || eg.check_visibility(effective_caller, &defining_class, visibility)
        });
    let hidden_parent_private = eg
        .find_property_visibility(&object_ref.class_name, name)
        .is_some_and(|(visibility, defining_class)| {
            visibility == Visibility::Private
                && !defining_class.eq_ignore_ascii_case(&object_ref.class_name)
                && !eg.check_visibility(effective_caller, &defining_class, visibility)
        });
    let write_only_property = if accessible && !hidden_parent_private {
        let key = crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            name,
            effective_caller,
        );
        object_ref
            .property_slot(&key)
            .and_then(|slot| eg.instance_property_definition(object_ref.class_id, slot))
            .is_some_and(|definition| {
                definition.has_set_hook
                    && !definition.has_get_hook
                    && !definition.set_hook_is_backed
            })
    } else {
        false
    };
    let object_class_name = object_ref.class_name.clone();
    let key = if hidden_parent_private {
        name.to_string()
    } else {
        crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            name,
            effective_caller,
        )
    };
    let has_get_hook = accessible
        && !hidden_parent_private
        && object_ref
            .property_slot(&key)
            .and_then(|slot| eg.instance_property_definition(object_ref.class_id, slot))
            .is_some_and(|definition| definition.has_get_hook);
    let declared_property = object_ref.property_slot(&key).is_some();
    drop(object_ref);
    let initialized_target = if accessible
        && !hidden_parent_private
        && declared_property
        && eg.lazy_property_requires_initialization(object, &key)
    {
        Some(crate::stdlib::reflection::initialize_lazy_object(
            eg, object,
        )?)
    } else {
        eg.lazy_proxy_instance(object)
    };
    if let Some(result) = take_magic_exception(eg, frame) {
        return Ok(result);
    }
    let magic_receiver = object;
    let object = initialized_target.as_ref().unwrap_or(object);
    if has_get_hook
        && !property_guard_active(eg, magic_receiver, name, PROPERTY_GUARD_GET)
        && !property_guard_active(eg, magic_receiver, name, PROPERTY_GUARD_SET)
    {
        let hook_name = format!("${name}::get");
        let value = call_guarded_property_magic_method(
            eg,
            magic_receiver,
            name,
            PROPERTY_GUARD_GET,
            &hook_name,
            &[],
        )?;
        if let Some(result) = take_magic_exception(eg, frame) {
            return Ok(result);
        }
        set_result(value.is_some_and(|value| {
            !value.is_undef() && value.value_type() != ValueType::Null
        }));
        return Ok(ColdResult::Done);
    }
    let object_ref = object
        .as_object()
        .expect("lazy initialization must preserve an object receiver");
    let property_state = if hidden_parent_private {
        object_ref
            .get_dynamic_property_with_position(name)
            .map(|(value, _)| !value.is_undef() && value.value_type() != ValueType::Null)
    } else if accessible {
        object_ref
            .get_property(&key)
            .map(|value| !value.is_undef() && value.value_type() != ValueType::Null)
    } else {
        None
    };
    drop(object_ref);

    if write_only_property
        && !property_guard_active(eg, magic_receiver, name, PROPERTY_GUARD_SET)
    {
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!("Property {object_class_name}::${name} is write-only"),
        ));
    }

    let set = match property_state {
        Some(set) => set,
        None => call_guarded_property_magic_method(
            eg,
            magic_receiver,
            name,
            PROPERTY_GUARD_ISSET,
            "__isset",
            &[Value::string(name)],
        )?
        .is_some_and(|value| value.is_truthy()),
    };
    if let Some(result) = take_magic_exception(eg, frame) {
        return Ok(result);
    }
    set_result(set);
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_unset_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: the compiler validated both operands against this op array, and
    // dispatch keeps `frame` live for the entire opcode.
    let (object, property) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
        )
    };
    if object.value_type() != ValueType::Object {
        return Ok(ColdResult::Done);
    }
    let lazy_receiver_owner = eg.lazy_object_state(object).map(|_| object.clone());
    let object = lazy_receiver_owner.as_ref().unwrap_or(object);
    let name = property
        .as_str()
        .ok_or_else(|| VmError::Fatal("Property name must be a string".into()))?
        .to_string();
    let caller_class = get_caller_class(frame, eg);
    let object_ref = object.as_object().unwrap();
    let receiver_in_scope = caller_class
        .as_ref()
        .is_some_and(|caller| eg.class_is_a(&object_ref.class_name, caller));
    let effective_caller = receiver_in_scope
        .then_some(caller_class.as_deref())
        .flatten();
    let accessible = eg
        .find_property_set_visibility(&object_ref.class_name, &name)
        .is_none_or(|(visibility, defining_class)| {
            visibility == Visibility::Public
                || eg.check_visibility(effective_caller, &defining_class, visibility)
        });
    let hidden_parent_private = eg
        .find_property_visibility(&object_ref.class_name, &name)
        .is_some_and(|(visibility, defining_class)| {
            visibility == Visibility::Private
                && !defining_class.eq_ignore_ascii_case(&object_ref.class_name)
                && !eg.check_visibility(effective_caller, &defining_class, visibility)
        });
    let key = if hidden_parent_private {
        name.clone()
    } else {
        crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            &name,
            effective_caller,
        )
    };
    if !accessible
        && eg.property_has_asymmetric_set_visibility(&object_ref.class_name, &name)
        && object_ref
            .get_property(&key)
            .is_some_and(|value| !value.is_undef())
    {
        let (visibility, defining_class) = eg
            .find_property_set_visibility(&object_ref.class_name, &name)
            .expect("asymmetric property has write visibility");
        let visibility = match visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => "public",
        };
        let message = format!(
            "Cannot unset {visibility}(set) property {defining_class}::${name} from {}",
            caller_class
                .as_deref()
                .map_or_else(|| "global scope".to_string(), |scope| format!("scope {scope}")),
        );
        drop(object_ref);
        return Ok(object_property_throw(eg, frame, "Error", message));
    }
    let hooked_property = accessible
        && !hidden_parent_private
        && object_ref
            .property_slot(&key)
            .and_then(|slot| eg.instance_property_definition(object_ref.class_id, slot))
            .is_some_and(|definition| definition.has_get_hook || definition.has_set_hook);
    if hooked_property {
        let class_name = object_ref.class_name.clone();
        drop(object_ref);
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!("Cannot unset hooked property {class_name}::${name}"),
        ));
    }
    let lazy_declared_property = accessible
        && !hidden_parent_private
        && object_ref.property_slot(&key).is_some();
    let lazy_dynamic_property = object_ref
        .get_dynamic_property_with_position(&key)
        .is_some();
    let lazy_declared_undefined = lazy_declared_property
        && object_ref
            .get_property(&key)
            .is_some_and(Value::is_undef);
    let lazy_class_name = object_ref.class_name.clone();
    drop(object_ref);
    let magic_unset_can_handle = !lazy_declared_property
        && !lazy_dynamic_property
        && !property_guard_active(eg, object, &name, PROPERTY_GUARD_UNSET)
        && eg
            .find_function(&format!(
                "{}::__unset",
                lazy_class_name.to_ascii_lowercase()
            ))
            .is_some();
    let lazy_undefined = (lazy_declared_undefined
        && eg.lazy_property_requires_initialization(object, &key))
        || (accessible
            && !hidden_parent_private
            && !lazy_declared_property
            && !lazy_dynamic_property
            && !magic_unset_can_handle
            && eg.is_uninitialized_lazy_object(object));
    let initialized_target = if lazy_undefined {
        Some(crate::stdlib::reflection::initialize_lazy_object(
            eg, object,
        )?)
    } else {
        eg.lazy_proxy_instance(object)
    };
    if let Some(result) = take_magic_exception(eg, frame) {
        return Ok(result);
    }
    let magic_receiver = object;
    let object = initialized_target.as_ref().unwrap_or(object);
    let object_ref = object
        .as_object()
        .expect("lazy initialization must preserve an object receiver");
    let removed = if hidden_parent_private {
        object_ref.get_dynamic_property_with_position(&key).is_some()
    } else {
        accessible && object_ref.contains_property(&key)
    };
    drop(object_ref);

    if removed {
        if hidden_parent_private {
            object.as_object_mut().unwrap().remove_dynamic_property(&key);
        } else {
            object.as_object_mut().unwrap().unset_property(&key);
            eg.mark_initializing_lazy_property_written(object, &key);
        }
        return Ok(ColdResult::Done);
    }
    let _ = call_guarded_property_magic_method(
        eg,
        magic_receiver,
        &name,
        PROPERTY_GUARD_UNSET,
        "__unset",
        &[Value::string(&name)],
    )?;
    if let Some(result) = take_magic_exception(eg, frame) {
        return Ok(result);
    }
    Ok(ColdResult::Done)
}

include!("instance_property_cache.rs");

fn op_bind_obj_prop_ref<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: all three operand slots are compiler-owned by the active frame.
    // The receiver is cloned before its CV can be replaced, and the owned
    // reference cell keeps the property target stable across object growth.
    unsafe {
        let instruction_index =
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        let receiver = (&*(*frame).get_op_ptr(
            opline.op1 as u32,
            opline.op1_type,
            op_array,
        ))
            .clone();
        let name_value = &*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        );
        let name = name_value.echo_to_string();
        let Some(object) = receiver.as_object() else {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!(
                    "Attempt to modify property \"{name}\" on {}",
                    receiver.dereferenced().type_name()
                ),
            ));
        };
        let class_name = object.class_name.to_string();
        drop(object);

        let caller_class = get_caller_class(frame, eg);
        let receiver_in_scope = caller_class
            .as_ref()
            .is_some_and(|caller| eg.class_is_a(&class_name, caller));
        let effective_caller = receiver_in_scope
            .then_some(caller_class.as_deref())
            .flatten();
        let mut force_dynamic = false;
        if let Some((visibility, defining_class)) =
            eg.find_property_set_visibility(&class_name, &name)
            && visibility != Visibility::Public
            && !eg.check_visibility(effective_caller, &defining_class, visibility)
        {
            if visibility == Visibility::Private
                && !eg.property_has_asymmetric_set_visibility(&class_name, &name)
                && !defining_class.eq_ignore_ascii_case(&class_name)
            {
                force_dynamic = true;
            } else {
                let visibility = match visibility {
                    Visibility::Protected => "protected",
                    Visibility::Private => "private",
                    Visibility::Public => "public",
                };
                let message = if eg.property_has_asymmetric_set_visibility(&class_name, &name) {
                    format!(
                        "Cannot indirectly modify {visibility}(set) property {defining_class}::${name} from {}",
                        caller_class
                            .as_deref()
                            .map_or_else(|| "global scope".to_string(), |scope| format!("scope {scope}")),
                    )
                } else {
                    format!("Cannot access {visibility} property {defining_class}::${name}")
                };
                return Ok(object_property_throw_at(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Error",
                    message,
                ));
            }
        }
        if eg
            .find_class(&class_name)
            .is_some_and(|class| class.readonly_props.contains(&name))
        {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                format!("Cannot acquire reference to readonly property {class_name}::${name}"),
            ));
        }

        let key = if force_dynamic {
            name.clone()
        } else {
            crate::runtime::resolve_property_key(
                eg,
                &class_name,
                &name,
                effective_caller,
            )
        };
        let (lazy_declared_property, lazy_dynamic_property) = receiver
            .as_object()
            .map(|object| {
                (
                    object.property_slot(&key).is_some(),
                    object.get_dynamic_property_with_position(&key).is_some(),
                )
            })
            .unwrap_or((false, false));
        let lazy_magic_get_directly_guarded = receiver
            .as_object()
            .is_some_and(|object| object.property_guard_active(&name, PROPERTY_GUARD_GET));
        let lazy_magic_get_related_guarded = !lazy_magic_get_directly_guarded
            && eg.lazy_proxy_related_property_guard_active(
                &receiver,
                &name,
                PROPERTY_GUARD_GET,
            );
        let lazy_magic_get_can_handle = !lazy_declared_property
            && !lazy_dynamic_property
            && !lazy_magic_get_directly_guarded
            && !lazy_magic_get_related_guarded
            && eg
                .find_function(&format!("{}::__get", class_name.to_ascii_lowercase()))
                .is_some();
        let receiver = if !lazy_magic_get_can_handle
            && eg.lazy_property_requires_initialization(&receiver, &key)
        {
            crate::stdlib::reflection::initialize_lazy_object(eg, &receiver)?
        } else {
            eg.lazy_proxy_instance(&receiver).unwrap_or(receiver)
        };
        if let Some(result) = take_magic_exception(eg, frame) {
            return Ok(result);
        }
        let (declared_slot, definition, owner) = {
            let object = receiver.as_object().unwrap();
            let slot = (!force_dynamic).then(|| object.property_slot(&key)).flatten();
            let definition = slot
                .and_then(|slot| eg.instance_property_definition(object.class_id, slot))
                .map(|definition| definition as *const crate::compiler::compile::PropertyDefinition);
            let owner = slot.map(|slot| object.instance_property_reference_owner(slot));
            (slot, definition, owner)
        };
        let definition = definition.map(|definition| &*definition);

        if let Some(definition) = definition
            && definition.has_get_hook
            && !property_guard_active(eg, &receiver, &name, PROPERTY_GUARD_GET)
            && !property_guard_active(eg, &receiver, &name, PROPERTY_GUARD_SET)
        {
            if opline._pad & OBJ_PROP_REFERENCE_BIND != 0 {
                return Ok(object_property_throw_at(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Error",
                    "Cannot assign by reference to overloaded object".to_string(),
                ));
            }
            let hook_name = format!("${name}::get");
            let returned = call_guarded_property_magic_method(
                eg,
                &receiver,
                &name,
                PROPERTY_GUARD_GET,
                &hook_name,
                &[],
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if let Some(returned) = returned {
                let mut binding = if returned.is_owned_reference() {
                    returned.clone_owned_reference_alias()
                } else if returned.is_reference() {
                    Value::reference(returned.as_ref_ptr())
                } else {
                    let message = if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                        format!("Indirect modification of {class_name}::${name} is not allowed")
                    } else {
                        format!("Cannot create reference to property {class_name}::${name}")
                    };
                    return Ok(object_property_throw_at(
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                        "Error",
                        message,
                    ));
                };
                if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                    binding.mark_internal_reference_alias();
                }
                let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
                frame_slot_set(frame, destination, binding);
                return Ok(ColdResult::Done);
            }
        }

        let missing_property = declared_slot.is_none()
            && receiver
                .as_object()
                .is_some_and(|object| {
                    object.get_dynamic_property_with_position(&key).is_none()
                });
        if missing_property && lazy_magic_get_related_guarded {
            let receiver_class = receiver
                .as_object()
                .map(|object| object.class_name.to_string())
                .unwrap_or(class_name.clone());
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &format!("Undefined property: {receiver_class}::${name}"),
                false,
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            let mut binding = Value::owned_reference(Value::null());
            if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                binding.mark_internal_reference_alias();
            }
            let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
            frame_slot_set(frame, destination, binding);
            return Ok(ColdResult::Done);
        }
        if missing_property
            && !property_guard_active(eg, &receiver, &name, PROPERTY_GUARD_GET)
        {
            let returned = call_guarded_property_magic_method(
                eg,
                &receiver,
                &name,
                PROPERTY_GUARD_GET,
                "__get",
                &[Value::string(name.clone())],
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if let Some(returned) = returned {
                let mut binding = if returned.is_owned_reference() {
                    returned.clone_owned_reference_alias()
                } else if returned.is_reference() {
                    Value::reference(returned.as_ref_ptr())
                } else {
                    Value::owned_reference(returned)
                };
                if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                    binding.mark_internal_reference_alias();
                }
                let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
                frame_slot_set(frame, destination, binding);
                return Ok(ColdResult::Done);
            }
        }

        if opline._pad & OBJ_PROP_REFERENCE_BIND == 0
            && let Some(definition) = definition
            && definition.is_typed()
            && declared_slot.is_some_and(|slot| {
                receiver
                    .as_object()
                    .and_then(|object| object.get_property_slot(slot).map(Value::is_undef))
                    .unwrap_or(false)
            })
            && !property_type_matches_exact(
                &Value::null(),
                &definition.type_hint,
                eg,
                &definition.type_scope,
                &class_name,
            )
        {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                format!(
                    "Cannot access uninitialized non-nullable property {}::${} by reference",
                    definition.declaring_class, definition.name
                ),
            ));
        }

        let (creates_dynamic_property, dynamic_properties_allowed) = {
            let object = receiver.as_object().unwrap();
            let exists = if force_dynamic {
                object.get_dynamic_property_with_position(&key).is_some()
            } else {
                object.contains_property(&key)
            };
            (
                definition.is_none() && !exists,
                object.is_dynamic_std_class()
                    || eg
                        .class_table
                        .get(object.class_name.as_ref())
                        .is_some_and(|class_def| class_def.allow_dynamic_properties),
            )
        };
        let has_magic_get = eg
            .find_function(&format!("{}::__get", class_name.to_ascii_lowercase()))
            .is_some();
        if creates_dynamic_property
            && !has_magic_get
            && eg
                .class_table
                .get(class_name.as_str())
                .is_some_and(|class_def| class_def.is_readonly)
        {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                format!("Cannot create dynamic property {class_name}::${name}"),
            ));
        }
        if creates_dynamic_property
            && !dynamic_properties_allowed
            && !has_magic_get
            && opline._pad & REFERENCE_RESULT_INTERNAL == 0
        {
            report_php_deprecation(
                eg,
                frame,
                op_array,
                opline,
                &format!("Creation of dynamic property {class_name}::${name} is deprecated"),
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
        }

        if opline._pad & OBJ_PROP_REFERENCE_BIND != 0 {
            let source = (*frame).cv_mut(opline.result as u32) as *mut Value;
            let binding = materialize_reference_alias(frame, source);
            let constraints = binding.reference_property_constraints();
            let current = (&*binding.as_ref_ptr()).clone();
            let prepared = if let Some(definition) = definition {
                match prepare_typed_property_reference_attachment(
                    current,
                    definition,
                    &constraints,
                    eg,
                    op_array.strict_types,
                    &class_name,
                ) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(object_property_throw_at(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            "TypeError",
                            message,
                        ));
                    }
                }
            } else {
                current
            };
            let target = binding.as_ref_ptr();
            std::ptr::drop_in_place(target);
            target.write(prepared);
            let property_binding = binding.clone_owned_reference_alias();
            let destructor = {
                let object = receiver.as_object().unwrap();
                let previous = if force_dynamic {
                    object
                        .get_dynamic_property_with_position(&key)
                        .map(|(value, _)| value)
                } else {
                    object.get_property(&key)
                };
                previous.and_then(|value| {
                    (!value.owned_reference_is_aliased())
                        .then(|| prepare_replaced_value_destructor(eg, value))
                        .flatten()
                })
            };
            let mut object = receiver.as_object_mut().unwrap();
            if force_dynamic {
                object.set_dynamic_property(&key, property_binding.clone_owned_reference_alias());
            } else {
                object.set_property(&key, property_binding.clone_owned_reference_alias());
            }
            drop(object);
            run_prepared_value_destructor(eg, destructor)?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if let (Some(definition), Some(owner)) = (definition, owner)
                && definition.is_typed()
            {
                property_binding.add_reference_property_constraint(
                    crate::value::ReferencePropertyConstraint {
                        owner,
                        declaring_class: definition.declaring_class.clone(),
                        property: definition.name.clone(),
                        type_scope: definition.type_scope.clone(),
                        called_class: class_name.clone(),
                        type_hint: definition.type_hint.clone(),
                    },
                );
            }
            return Ok(ColdResult::Done);
        }

        let mut object = receiver.as_object_mut().unwrap();
        let property = if force_dynamic {
            object.get_dynamic_property_mut(&key)
        } else {
            object.get_property_mut(&key)
        };
        let mut binding = if let Some(property) = property {
            if property.is_owned_reference() {
                property.clone_owned_reference_alias()
            } else {
                let current = std::mem::replace(property, Value::undef());
                let current = if current.is_reference() {
                    current.dereferenced().clone()
                } else {
                    current
                };
                let binding = Value::owned_reference(current);
                *property = binding.clone_owned_reference_alias();
                binding
            }
        } else {
            let binding = Value::owned_reference(Value::null());
            if force_dynamic {
                object.set_dynamic_property(&key, binding.clone_owned_reference_alias());
            } else {
                object.set_property(&key, binding.clone_owned_reference_alias());
            }
            binding
        };
        drop(object);

        if let Some(definition) = definition
            && definition.is_typed()
        {
            let current = (&*binding.as_ref_ptr()).clone();
            if current.is_undef() {
                let target = binding.as_ref_ptr();
                std::ptr::drop_in_place(target);
                target.write(Value::null());
            }
            binding.add_reference_property_constraint(
                crate::value::ReferencePropertyConstraint {
                    owner: owner.expect("declared typed property must retain its slot"),
                    declaring_class: definition.declaring_class.clone(),
                    property: definition.name.clone(),
                    type_scope: definition.type_scope.clone(),
                    called_class: class_name.clone(),
                    type_hint: definition.type_hint.clone(),
                },
            );
        }

        if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
            binding.mark_internal_reference_alias();
        }

        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
        frame_slot_set(frame, destination, binding);
    }
    Ok(ColdResult::Done)
}

fn op_bind_array_dim_ref<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: the compiler emits mutable array/CV operands owned by this live
    // frame. Promoting the element to an Rc-backed cell makes both aliases
    // independent of subsequent array storage reallocations.
    unsafe {
        let index = &*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        );
        let key = match value_to_array_key(index) {
            Ok(key) => key,
            Err(_) => {
                let instruction_index = (opline as *const Instruction)
                    .offset_from(op_array.instructions.as_ptr())
                    as usize;
                return Ok(match throw_illegal_offset_type(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Illegal offset type",
                ) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        };
        let array_ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
        let raw_type = (*array_ptr).dereferenced().value_type();
        if raw_type == ValueType::String {
            let error = make_error_value("Error", "Cannot create references to/from string offsets");
            let instruction_index = (opline as *const Instruction)
                .offset_from(op_array.instructions.as_ptr()) as usize;
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        if matches!(raw_type, ValueType::Object | ValueType::Closure) {
            let receiver = (*array_ptr).dereferenced().clone();
            let returned = crate::stdlib::call_object_protocol_method(
                eg,
                &receiver,
                "ArrayAccess",
                "offsetGet",
                std::slice::from_ref(index),
            )?;
            let Some(returned) = returned else {
                let instruction_index = (opline as *const Instruction)
                    .offset_from(op_array.instructions.as_ptr())
                    as usize;
                return Ok(match throw_object_as_array(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    &receiver,
                ) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            };
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            let mut binding = if returned.is_owned_reference() {
                returned.clone_owned_reference_alias()
            } else if returned.is_reference() {
                Value::reference(returned.as_ref_ptr())
            } else {
                let class_name = receiver
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| "object".to_string());
                report_php_notice(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!(
                        "Indirect modification of overloaded element of {class_name} has no effect"
                    ),
                )?;
                Value::owned_reference(returned)
            };
            if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                binding.mark_internal_reference_alias();
            }
            let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
            frame_slot_set(frame, destination, binding);
            return Ok(ColdResult::Done);
        }
        if opline._pad & REFERENCE_SOURCE_MAY_BE_NONREFERENCEABLE != 0
            && !(*array_ptr).is_reference()
        {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                "Attempting to set reference to non referenceable value",
            )?;
        }
        let mutable_source = if (*array_ptr).is_reference() {
            &mut *(*array_ptr).as_ref_ptr()
        } else {
            &mut *array_ptr
        };
        if matches!(mutable_source.value_type(), ValueType::Null | ValueType::Undef) {
            slot_set(mutable_source, Value::array(PhpArray::new()));
        }
        let array = mutable_source
            .as_array_mut()
            .ok_or_else(|| VmError::Fatal("Cannot acquire reference to non-array offset".into()))?;
        if array.get_key_mut(&key).is_none() {
            array.set(key.clone(), Value::null());
        }
        let element = array.get_key_mut(&key).unwrap();
        let mut binding = if element.is_owned_reference() {
            element.clone_owned_reference_alias()
        } else {
            let current = std::mem::replace(element, Value::undef());
            let current = if current.is_reference() {
                current.dereferenced().clone()
            } else {
                current
            };
            let binding = Value::owned_reference(current);
            *element = binding.clone_owned_reference_alias();
            binding
        };
        if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
            binding.mark_internal_reference_alias();
        }
        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
        frame_slot_set(frame, destination, binding);
    }
    Ok(ColdResult::Done)
}

fn op_assign_obj_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: all operands belong to the active compiler-sized frame. A
    // Reference target remains live through this non-reentrant assignment.
    let (prop_name, val, obj) = unsafe {
        let prop_name =
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let val = &*(*frame).get_op_ptr(
            opline.result as u32,
            opline.result_type,
            op_array,
        );
        let val = if val.is_reference() {
            &*val.as_ref_ptr()
        } else {
            val
        };
        let obj = (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
            .dereferenced();
        (prop_name, val, obj)
    };
    let mut assigned = if opline._pad & ASSIGN_PROP_MOVE_SOURCE != 0
        && matches!(opline.result_type, OpType::Tmp | OpType::Var)
    {
        // The statement compiler proved this source has no later consumer.
        // `val` is no longer used after this transfer.
        unsafe {
            let source = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            if (&*source).is_reference() {
                val.clone()
            } else {
                frame_tmp_take!(frame, source)
            }
        }
    } else {
        val.clone()
    };
    let name = prop_name.echo_to_string();
    let lazy_receiver_owner = eg.lazy_object_state(obj).map(|_| obj.clone());
    let obj = lazy_receiver_owner.as_ref().unwrap_or(obj);

    if let Some(php_obj) = obj.as_object_mut() {
        let caller_class = get_caller_class(frame, eg);
        let object_display_class_name = if php_obj.class_name.starts_with("class@anonymous#") {
            std::rc::Rc::<str>::from("class@anonymous")
        } else {
            php_obj.class_name.clone()
        };

        // Same receiver-in-scope guard as FetchObjR — only allow
        // private bypass when the receiver is in the caller's hierarchy.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&php_obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Visibility check — use declaring class, not receiver class
        let mut prop_is_public = true;
        let mut property_accessible = true;
        let mut force_dynamic = false;
        if let Some((vis, defining_class)) = eg.find_property_set_visibility(&php_obj.class_name, &name) {
            if vis != Visibility::Public {
                prop_is_public = false;
                let own_private = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    vis == Visibility::Private && defining_class.eq_ignore_ascii_case(cc)
                });
                let caller_has_own = receiver_in_scope && caller_class.as_ref().map_or(false, |cc| {
                    if let Some((Visibility::Private, ref dc)) = eg.find_property_visibility(cc, &name) {
                        dc.eq_ignore_ascii_case(cc)
                    } else {
                        false
                    }
                });
                let hidden_parent_private = vis == Visibility::Private
                    && !eg.property_has_asymmetric_set_visibility(&php_obj.class_name, &name)
                    && !defining_class.eq_ignore_ascii_case(&php_obj.class_name)
                    && !own_private;
                if hidden_parent_private {
                    property_accessible = false;
                    force_dynamic = true;
                } else if !own_private && !caller_has_own {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let has_setter = eg
                            .find_function(&format!(
                                "{}::__set",
                                php_obj.class_name.to_ascii_lowercase()
                            ))
                            .is_some();
                        if has_setter {
                            prop_is_public = false;
                            property_accessible = false;
                        } else {
                            let vis_str = match vis {
                                Visibility::Protected => "protected",
                                Visibility::Private => "private",
                                _ => "public",
                            };
                            let message = if eg.property_has_asymmetric_set_visibility(
                                &php_obj.class_name,
                                &name,
                            ) {
                                let action = if opline._pad & ASSIGN_OBJ_MODIFY != 0
                                    && assigned.value_type() == ValueType::Array
                                {
                                    "indirectly modify"
                                } else {
                                    "modify"
                                };
                                format!(
                                    "Cannot {action} {vis_str}(set) property {defining_class}::${name} from {}",
                                    caller_class.as_deref().map_or_else(
                                        || "global scope".to_string(),
                                        |scope| format!("scope {scope}"),
                                    ),
                                )
                            } else {
                                format!("Cannot access {vis_str} property {defining_class}::${name}")
                            };
                            drop(php_obj);
                            return Ok(object_property_throw(eg, frame, "Error", message));
                        }
                    }
                }
            }
        }
        let lazy_key = crate::runtime::resolve_property_key(
            eg,
            &php_obj.class_name,
            &name,
            effective_caller,
        );
        let lazy_declared_property = php_obj.property_slot(&lazy_key).is_some();
        let lazy_dynamic_property = php_obj
            .get_dynamic_property_with_position(&lazy_key)
            .is_some();
        let lazy_class_name = php_obj.class_name.clone();
        drop(php_obj);
        let magic_set_can_handle = !lazy_declared_property
            && !lazy_dynamic_property
            && !property_guard_active(eg, obj, &name, PROPERTY_GUARD_SET)
            && eg
                .find_function(&format!(
                    "{}::__set",
                    lazy_class_name.to_ascii_lowercase()
                ))
                .is_some();
        let must_initialize = (property_accessible || force_dynamic)
            && !lazy_dynamic_property
            && !magic_set_can_handle
            && eg.lazy_property_requires_initialization(obj, &lazy_key);
        let initialized_target = if must_initialize {
            Some(crate::stdlib::reflection::initialize_lazy_object(
                eg, obj,
            )?)
        } else {
            eg.lazy_proxy_instance(obj)
        };
        if let Some(result) = take_magic_exception(eg, frame) {
            return Ok(result);
        }
        let magic_receiver = obj;
        let obj = initialized_target.as_ref().unwrap_or(obj);
        let php_obj = obj
            .as_object_mut()
            .expect("lazy initialization must preserve an object receiver");
        // Enum guard: enum cases are sealed — no property writes allowed
        // Track writability for cache population — enum/readonly are not cacheable for writes.
        let mut prop_is_writable = true;
        if let Some(class_def) = eg.class_table.get(php_obj.class_name.as_ref()) {
            if class_def.is_enum {
                let err = make_error_value("Error", &format!(
                    "Cannot modify readonly property {}::${}",
                    object_display_class_name, name
                ));
                drop(php_obj);
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
        // Readonly property check
        let mut consume_clone_reinitialization = false;
        if let Some(class_def) = eg.class_table.get(php_obj.class_name.as_ref()) {
            if class_def.readonly_props.contains(&name) {
                prop_is_writable = false;
                let key_check = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);
                let already_init = php_obj.get_property(&key_check)
                    .map_or(false, |v| !v.is_undef());
                if already_init {
                    if opline._pad & ASSIGN_OBJ_MODIFY != 0
                        && assigned.value_type() == ValueType::Array
                    {
                        let err = make_error_value("Error", &format!(
                            "Cannot indirectly modify readonly property {}::${}",
                            object_display_class_name, name
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    } else if readonly_clone_reinitialization_allowed(eg, obj, &name) {
                        consume_clone_reinitialization = true;
                    } else if opline._pad & ASSIGN_OBJ_CLONE_WITH != 0
                        && consume_readonly_clone_with_update(eg, frame, obj, &name)
                    {
                        // The pre-update snapshot grants this one direct write.
                    } else {
                        let err = make_error_value("Error", &format!(
                            "Cannot modify readonly property {}::${}",
                            object_display_class_name, name
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    }
                } else {
                    // PHP 8.4+ readonly writes are protected(set): first
                    // initialization is available to the receiver's class
                    // family, including a parent constructor on a child object.
                    let in_declaring_scope = receiver_in_scope;
                    if !in_declaring_scope {
                        let err = make_error_value("Error", &format!(
                            "Cannot initialize readonly property {}::${} from {}",
                            object_display_class_name, name,
                            caller_class.as_deref().map_or("global scope".to_string(), |c| format!("scope {}", c))
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    }
                }
            }
        }
        // Resolve storage key (mangled for private properties)
        let key = if force_dynamic {
            name.clone()
        } else {
            crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller)
        };
        let declared_slot = property_accessible
            .then(|| php_obj.property_slot(&key))
            .flatten();
        let definition = declared_slot
            .and_then(|slot| eg.instance_property_definition(php_obj.class_id, slot));
        let getter_only_property = definition.is_some_and(|definition| {
            definition.has_get_hook
                && !definition.has_set_hook
                && !definition.get_hook_is_backed
        });
        let has_set_hook = definition.is_some_and(|definition| definition.has_set_hook);
        let property_constraints = if force_dynamic {
            php_obj
                .get_dynamic_property_with_position(&key)
                .map(|(property, _)| property.reference_property_constraints())
                .unwrap_or_default()
        } else {
            php_obj
                .get_property(&key)
                .map(Value::reference_property_constraints)
                .unwrap_or_default()
        };
        let object_class_id = php_obj.class_id;
        let object_class_name = php_obj.class_name.clone();
        let dynamic_properties_allowed = php_obj.is_dynamic_std_class()
            || eg
                .class_table
                .get(php_obj.class_name.as_ref())
                .is_some_and(|class_def| class_def.allow_dynamic_properties);
        let magic_get_handles_indirect_writeback = opline._pad & ASSIGN_OBJ_MODIFY != 0
            && eg
                .find_function(&format!(
                    "{}::__get",
                    php_obj.class_name.to_ascii_lowercase()
                ))
                .is_some();
        let readonly_class = eg
            .class_table
            .get(php_obj.class_name.as_ref())
            .is_some_and(|class_def| class_def.is_readonly);
        let prop_exists = if force_dynamic {
            php_obj.get_dynamic_property_with_position(&key).is_some()
        } else {
            property_accessible && php_obj.contains_property(&key)
        };
        drop(php_obj);
        if has_set_hook
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET)
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_GET)
        {
            let hook_name = format!("${name}::set");
            let hook_value = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_SET,
                &hook_name,
                std::slice::from_ref(&assigned),
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if hook_value.is_some() {
                return Ok(ColdResult::Done);
            }
        }
        if getter_only_property {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Property {object_display_class_name}::${name} is read-only"),
            ));
        }
        if !prop_exists && object_class_name.as_ref() == "Generator" {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Cannot create dynamic property Generator::${name}"),
            ));
        }
        // A setter may execute arbitrary user code. Reacquire the stable
        // class-table definition after that call; inline caches must never
        // retain pointers to task-local snapshots.
        let definition = declared_slot
            .and_then(|slot| eg.instance_property_definition(object_class_id, slot));
        if let Some(definition_ref) = definition {
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            if let Some(declaration) = definition_ref.generic_declaration
                && let Err(message) = eg.check_cached_generic_property_value(
                    obj,
                    &definition_ref.name,
                    &assigned,
                    declaration,
                )
            {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "TypeError",
                    message,
                ));
            }
            if definition_ref.is_typed() && definition_ref.generic_declaration.is_none() {
                assigned = match prepare_property_assignment(
                    assigned,
                    definition_ref,
                    eg,
                    op_array.strict_types,
                    &object_class_name,
                ) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "TypeError",
                            message,
                        ));
                    }
                };
            }
        }
        assigned = match prepare_reference_assignment(
            assigned,
            &property_constraints,
            eg,
            op_array.strict_types,
        ) {
            Ok(value) => value,
            Err(message) => {
                return Ok(object_property_throw(eg, frame, "TypeError", message));
            }
        };

        // Cache: if public, not enum, not readonly, key == name → mark for write fast path.
        if prop_is_public && prop_is_writable && key == name && object_class_id != 0 {
            let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
            let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
            if let Some(slot) = declared_slot {
                if let Some(definition) = definition
                    && definition.is_typed()
                {
                    ic_mut.set_typed_instance_property(definition, object_class_id, slot);
                } else {
                    ic_mut.set_property(object_class_id, slot, 3);
                }
            }
        }

        // A failed type/reference check must not consume PHP's one successful
        // readonly reinitialization opportunity during `__clone`.
        if consume_clone_reinitialization {
            consume_readonly_clone_reinitialization(eg, obj, &name);
        }

        if prop_exists {
            let destructor = {
                let object = obj.as_object().unwrap();
                let property = if force_dynamic {
                    object
                        .get_dynamic_property_with_position(&key)
                        .map(|(value, _)| value)
                } else {
                    object.get_property(&key)
                };
                property.and_then(|value| prepare_replaced_value_destructor(eg, value))
            };
            if let Some(mut php_obj) = obj.as_object_mut() {
                let property = if force_dynamic {
                    php_obj.get_dynamic_property_mut(&key)
                } else {
                    php_obj.get_property_mut(&key)
                }
                    .expect("existing property must remain addressable during assignment");
                assignment_slot_set(property, assigned);
            }
            run_prepared_value_destructor(eg, destructor)?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
        } else {
            // Property not found — try __set magic method
            let guarded = property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET);
            if name.starts_with('\0') && guarded {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    "Cannot access property starting with \"\\0\"".into(),
                ));
            }
            let magic = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_SET,
                "__set",
                &[Value::string(name.clone()), assigned.clone()],
            )?;
            if let Some(result) = take_magic_exception(eg, frame) {
                return Ok(result);
            }
            if guarded || magic.is_none() {
                if name.starts_with('\0') {
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "Error",
                        "Cannot access property starting with \"\\0\"".into(),
                    ));
                }
                if readonly_class && !magic_get_handles_indirect_writeback {
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "Error",
                        format!(
                            "Cannot create dynamic property {object_display_class_name}::${name}"
                        ),
                    ));
                }
                if !dynamic_properties_allowed && !magic_get_handles_indirect_writeback {
                    report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &format!(
                            "Creation of dynamic property {object_display_class_name}::${name} is deprecated"
                        ),
                    )?;
                    if let Some(result) = take_magic_exception(eg, frame) {
                        return Ok(result);
                    }
                }
                // No __set — fall back to direct insert
                if let Some(mut php_obj) = obj.as_object_mut() {
                    if force_dynamic {
                        php_obj.set_dynamic_property(&key, assigned);
                    } else {
                        php_obj.set_property(&key, assigned);
                    }
                }
            }
        }
        eg.mark_initializing_lazy_property_written(obj, &key);
    } else {
        let action = if opline._pad & ASSIGN_OBJ_MODIFY != 0 {
            "modify"
        } else {
            "assign"
        };
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Attempt to {action} property \"{name}\" on {}",
                obj.type_name()
            ),
        ));
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_method_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    if obj_val.value_type() == ValueType::Closure {
        let method = unsafe {
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
        }
        .as_str()
        .unwrap_or("");
        let Some(func_ptr) = eg.find_function(&format!("Closure::{method}")) else {
            let error = make_error_value(
                "Error",
                &format!("Call to undefined method Closure::{method}()"),
            );
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        };
        let num_args = opline.extended_value;
        let pending_call = unsafe { (*frame).call };
        let call = eg.vm_stack.push_call_frame(
            func_ptr,
            num_args + 1,
            num_args,
            frame,
            pending_call,
        );
        // SAFETY: the new method frame owns CV 0, and cloning the compact
        // Closure handle preserves its boxed payload through the call.
        unsafe {
            (*frame).call = call;
            frame_set_this(call, obj_val.clone());
        }
        return Ok(ColdResult::Done);
    }

    if let Some(obj) = obj_val.as_object() {
        let obj_class_id = obj.class_id;

        // Inline cache: if same class_id as last time, reuse resolved func_ptr
        // — avoids class_name.clone() and full method resolution on cache hit.
        let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
        let ic = &op_array.cache[ip];
        let (func_ptr, has_generic_contract, magic_method) = if !ic.func.is_null()
            && ic.class_id == obj_class_id
            && obj_class_id != 0
        {
            drop(obj); // release borrow — class_name not needed on cache hit
            (
                ic.func,
                cfg!(any(feature = "php-generics-erased", feature = "php-generics-reified"))
                    && ic.method_has_generic_contract(),
                None,
            )
        } else {
            let target_class_name = obj.class_name.clone();
            drop(obj); // release borrow before lookup
            let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
            let method = method_name.as_str().unwrap_or("");
            let caller_class = get_caller_class(frame, eg);

            let dispatch_class = if let Some(ref cc) = caller_class {
                if let Some((Visibility::Private, ref defining)) = eg.find_method_visibility(cc, method) {
                    if defining.eq_ignore_ascii_case(cc)
                        && eg.class_is_a(&target_class_name, cc)
                    {
                        cc.clone()
                    } else {
                        target_class_name.to_string()
                    }
                } else {
                    target_class_name.to_string()
                }
            } else {
                target_class_name.to_string()
            };

            let full_name = format!("{}::{}", dispatch_class, method);
            let (resolved, magic_method) = match eg.find_function(&full_name) {
                Some(ptr) => (ptr, None),
                None => {
                    let magic = eg
                        .find_method_info(&target_class_name, "__call")
                        .filter(|(visibility, is_static, _)| {
                            *visibility == Visibility::Public && !*is_static
                        })
                        .and_then(|(_, _, defining)| {
                            eg.find_function(&format!("{defining}::__call"))
                        });
                    if let Some(magic) = magic {
                        (magic, Some(Value::string(method)))
                    } else {
                        let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", dispatch_class, method));
                        match throw_in_frame(eg, frame, err) {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    }
                }
            };
            let resolved_has_generic_contract = cfg!(any(
                feature = "php-generics-erased",
                feature = "php-generics-reified"
            ))
                && eg
                    .generic_metadata
                    .has_instance_method_contract(&target_class_name, method);
            let linked_generic_long_contract = cfg!(any(
                feature = "php-generics-erased",
                feature = "php-generics-reified"
            ))
                && eg
                    .generic_metadata
                    .linked_instance_method_contract_admits_exact_long(
                        &target_class_name,
                        method,
                        opline.extended_value,
                    );

            // Visibility check
            if let Some((vis, defining_class)) = eg.find_method_visibility(&dispatch_class, method) {
                if vis != Visibility::Public {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let vis_str = match vis {
                            Visibility::Protected => "protected",
                            Visibility::Private => "private",
                            _ => "public",
                        };
                        return Err(VmError::Fatal(format!(
                            "Call to {} method {}::{}() from scope {}",
                            vis_str, defining_class, method,
                            caller_class.as_deref().unwrap_or("global")
                        )));
                    }
                }
            }

            // Cache the resolution (don't cache if class_id is 0 = unknown)
            if obj_class_id != 0 && magic_method.is_none() {
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
                let common = unsafe { &*resolved };
                let (fusion_eligible, long_property_plan, property_getter_plan) = if common.fn_type == FunctionType::User
                    && common.supports_scalar_long_plan()
                {
                    let user = unsafe { &*(resolved as *const UserFunction) };
                    (
                        user.op_array.instructions.len() <= FAST_SCALAR_METHOD_FUSION_MAX_OPS,
                        user.long_property_plan.is_some(),
                        user.property_getter_plan.is_some(),
                    )
                } else {
                    (false, false, false)
                };
                ic_mut.set_method(
                    resolved,
                    obj_class_id,
                    fusion_eligible,
                    long_property_plan,
                    property_getter_plan,
                    resolved_has_generic_contract,
                    linked_generic_long_contract,
                );
            }
            (resolved, resolved_has_generic_contract, magic_method)
        };
        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
        let _ = has_generic_contract;

        let num_args = opline.extended_value;
        let pending_call = unsafe { (*frame).call };
        let common = unsafe { &*func_ptr };
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        let generic_contract = if has_generic_contract {
            let method_name = unsafe {
                &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
            };
            eg.generic_instance_method_contract(obj_val, method_name.as_str().unwrap_or(""))
        } else {
            None
        };
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        let has_active_generic_contract = generic_contract.is_some();
        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
        let has_active_generic_contract = false;
        if !method_return_dispatch_contract_matches(opline, common) {
            return Err(VmError::Fatal(
                "Resolved method signature is incompatible with the statically declared receiver contract"
                    .into(),
            ));
        }
        let scalar_plan_eligible = magic_method.is_none()
            && !has_active_generic_contract
            && common.fn_type == FunctionType::User
            && num_args == common.sig.public_arity()
            && {
                let user = unsafe { &*(func_ptr as *const UserFunction) };
                (common.supports_scalar_long_plan()
                    && (user.scalar_long_plan.is_some()
                        || user.composed_scalar_long_plan.is_some()
                        || user.long_property_plan.is_some()))
                    || (common.supports_scalar_double_plan()
                        && user.scalar_double_plan.is_some())
            };
        let deferred = should_defer_scalar_call(opline, scalar_plan_eligible);
        let storage_slots = if magic_method.is_some() {
            (num_args + 1).max(3)
        } else {
            num_args + 1
        };
        let call = if deferred {
            eg.pending_call_stack.push_deferred_scalar_call(
                func_ptr,
                storage_slots,
                num_args,
                frame,
                pending_call,
            )
        } else {
            eg.vm_stack.push_call_frame(
                func_ptr,
                storage_slots,
                num_args,
                frame,
                pending_call,
            )
        };
        unsafe {
            (*frame).call = call;
            if common.plan.borrow_this() {
                frame_set_borrowed_this(call, obj_val as *const Value);
            } else {
                frame_set_this(call, obj_val.clone());
            }
        }
        if let Some(method) = magic_method {
            push_pending_magic_call(eg, call as usize, method);
        }
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(contract) = generic_contract {
            eg.push_pending_generic_member_call(call as usize, contract);
        }
    } else {
        let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let method = method_name.as_str().unwrap_or("");
        let err = make_error_value(
            "Error",
            &format!(
                "Call to a member function {method}() on {}",
                obj_val.dereferenced().type_name()
            ),
        );
        let instruction_index = (opline as *const Instruction as usize
            - op_array.instructions.as_ptr() as usize)
            / std::mem::size_of::<Instruction>();
        attach_throwable_origin(&err, eg, frame, op_array, instruction_index);
        match throw_in_frame(eg, frame, err) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                return Ok(ColdResult::NewFrame(new_frame, new_op_array));
            }
            ThrowResult::Unhandled(thrown) => {
                return Ok(ColdResult::Unhandled(thrown));
            }
        }
    }
    Ok(ColdResult::Done)
}
#[inline(never)]
fn class_callback_requires_instance(
    eg: &ExecutorGlobals,
    class: &str,
    method: &str,
) -> bool {
    if let Some((_, is_static, _)) = eg.find_method_info(class, method) {
        return !is_static;
    }
    if eg.find_method_info(class, "__callStatic").is_some() {
        return false;
    }
    eg.find_method_info(class, "__call")
        .is_some_and(|(_, is_static, _)| !is_static)
}

// InitStaticCall uses the low bit of a cached FunctionCommon pointer to retain
// the resolved method's staticness without growing the per-instruction cache.
const _: () = assert!(std::mem::align_of::<FunctionCommon>() >= 2);

fn throw_non_static_callback_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    class: &str,
    method: &str,
) -> ColdResult<'a> {
    let error = make_error_value(
        "Error",
        &format!("Non-static method {class}::{method}() cannot be called statically"),
    );
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(error) => ColdResult::Unhandled(error),
    }
}

fn op_init_static_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Inline cache: static calls have constant class+method — cache resolved func_ptr.
    // Visibility is checked on first resolve only (same instruction = same caller context).
    let dynamic_scope = opline._pad & CALL_FLAG_DYNAMIC_STATIC_SCOPE != 0;
    let (class_name, method_name, ip) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
        )
    };
    let raw_class = class_name.as_str().unwrap_or("").to_string();
    let method = method_name.as_str().unwrap_or("").to_string();
    let resolved_class = resolve_static_call_class(eg, frame, &raw_class, dynamic_scope);
    if raw_class.eq_ignore_ascii_case("parent") && resolved_class.is_none() {
        let error = make_error_value(
            "Error",
            "Cannot use \"parent\" when current class scope has no parent",
        );
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    let class = resolved_class.unwrap_or_else(|| raw_class.clone());
    let num_args = opline.extended_value;
    let cached = op_array.cache[ip].func;
    let (func_ptr, method_is_non_static, magic_method) = if !cached.is_null() {
        let tagged = cached as usize;
        if tagged & 1 == 0 {
            // Keep the overwhelmingly common static-method cache hit as the
            // original pointer without an unconditional mask operation.
            (cached, false, None)
        } else {
            ((tagged & !1usize) as *const FunctionCommon, true, None)
        }
    } else {
        if eg.find_class(&class).is_none() {
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &class)?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            if !loaded {
                let error = make_error_value("Error", &format!("Class \"{class}\" not found"));
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }

        // PHP exposes parent::$prop::get()/set() for ordinary backed
        // properties as implicit, internal-like accessors. Keep their exact
        // arity and property diagnostics here on the cold static-call path;
        // explicit user hooks retain normal user-function surplus-argument
        // semantics.
        if raw_class.eq_ignore_ascii_case("parent")
            && let Some((property, accessor)) = method
                .strip_prefix('$')
                .and_then(|name| name.rsplit_once("::"))
            && matches!(accessor, "get" | "set")
        {
            let definition = eg
                .find_class(&class)
                .and_then(|definition| definition.properties.iter().find(|p| p.name == property));
            let Some(definition) = definition else {
                let error = make_error_value(
                    "Error",
                    &format!("Undefined property {}::${property}", class),
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            };
            let caller_class = get_caller_class(frame, eg);
            let visibility = if accessor == "set" {
                definition.set_visibility.unwrap_or(definition.visibility)
            } else {
                definition.visibility
            };
            if !eg.check_visibility(
                caller_class.as_deref(),
                &definition.declaring_class,
                visibility,
            ) {
                let visibility_name = match visibility {
                    Visibility::Private => "private",
                    Visibility::Protected => "protected",
                    Visibility::Public => "public",
                };
                let error = make_error_value(
                    "Error",
                    &format!(
                        "Cannot access {visibility_name} property {}::${property}",
                        definition.declaring_class
                    ),
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            let implicit = if accessor == "get" {
                !definition.has_get_hook
            } else {
                !definition.has_set_hook
            };
            let expected = u32::from(accessor == "set");
            if implicit && num_args != expected {
                let noun = if expected == 1 { "argument" } else { "arguments" };
                let error = make_error_value(
                    "ArgumentCountError",
                    &format!(
                        "{}::${property}::{accessor}() expects exactly {expected} {noun}, {num_args} given",
                        class
                    ),
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }

        let full_name = format!("{}::{}", class, method);
        let (resolved, magic_method) = match eg.find_function(&full_name) {
            Some(ptr) => (ptr, None),
            None => {
                if class_callback_requires_instance(eg, &class, &method) {
                    return Ok(throw_non_static_callback_error(
                        eg, frame, op_array, ip, &class, &method,
                    ));
                }
                let magic = eg
                    .find_method_info(&class, "__callStatic")
                    .filter(|(visibility, is_static, _)| {
                        *visibility == Visibility::Public && *is_static
                    })
                    .and_then(|(_, _, defining)| {
                        eg.find_function(&format!("{defining}::__callStatic"))
                    });
                if let Some(magic) = magic {
                    (magic, Some(Value::string(&method)))
                } else {
                    let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", raw_class, method));
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                        }
                        ThrowResult::Unhandled(thrown) => {
                            return Ok(ColdResult::Unhandled(thrown));
                        }
                    }
                }
            }
        };
        let method_info = eg.find_method_info(&class, &method);
        let method_is_non_static = method_info
            .as_ref()
            .is_some_and(|(_, is_static, _)| !is_static);

        // Visibility check on first resolve for each dynamic class.
        if let Some((vis, _, defining_class)) = method_info.as_ref() {
            if *vis != Visibility::Public {
                let caller_class = if dynamic_scope {
                    resolve_static_call_class(eg, frame, "self", true)
                } else {
                    get_caller_class(frame, eg)
                };
                if !eg.check_visibility(caller_class.as_deref(), defining_class, *vis) {
                    let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                    return Err(VmError::Fatal(format!(
                        "Call to {} method {}::{}() from scope {}",
                        vis_str, defining_class, method,
                        caller_class.as_deref().unwrap_or("global")
                    )));
                }
            }
        }

        // Shared trait op arrays can be entered through different consuming
        // classes. Leaving their call cache empty keeps ordinary static calls'
        // exact one-load hot path and makes the exceptional scope explicit.
        if !dynamic_scope && magic_method.is_none() {
            unsafe {
                let cache = &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache);
                // FunctionCommon is pointer-aligned, so InitStaticCall owns
                // the otherwise-zero low bit as its non-static marker. This
                // keeps the warmed scalar-call path at one cache load.
                cache.func = ((resolved as usize) | usize::from(method_is_non_static))
                    as *const FunctionCommon;
            }
        }
        (resolved, method_is_non_static, magic_method)
    };

    let common = unsafe { &*func_ptr };
    let target_is_instance = method_is_non_static
        || (common.sig.this_offset == 1
            && matches!(raw_class.to_ascii_lowercase().as_str(), "self" | "parent"));
    if method_is_non_static {
        let compatible_this = unsafe {
            get_caller_class(frame, eg).is_some()
                && (*frame).num_cvs != 0
                && (*frame)
                    .cv(0)
                    .as_object()
                    .is_some_and(|receiver| eg.class_is_a(&receiver.class_name, &class))
        };
        if !compatible_this {
            return Ok(throw_non_static_callback_error(
                eg, frame, op_array, ip, &class, &method,
            ));
        }
    }
    if magic_method.is_none()
        && common.fn_type == FunctionType::User
        && num_args == common.sig.public_arity()
    {
        let user = unsafe { &*(func_ptr as *const UserFunction) };
        if let Some(plan) = user.scalar_long_plan.as_deref()
            && let Some((result, do_fcall_ptr)) = unsafe {
                try_execute_direct_scalar_long_call(
                    frame,
                    op_array,
                    (opline as *const Instruction).add(1),
                    common,
                    plan,
                )
            }
        {
            stats::inc_do_fcall_fast();
            stats::inc_return_fast();
            let count = common.call_count.get();
            if count < u32::MAX {
                common.call_count.set(count + 1);
            }
            unsafe { complete_direct_scalar_long_call(frame, do_fcall_ptr, result) };
            return Ok(ColdResult::Continue);
        }
    }
    // +1 for $this at CV 0 (compiler allocates $this even for static calls)
    let pending_call = unsafe { (*frame).call };
    let storage_slots = if magic_method.is_some() {
        (num_args + 1).max(3)
    } else {
        num_args + 1
    };
    let call = eg.vm_stack.push_call_frame(
        func_ptr,
        storage_slots,
        num_args,
        frame,
        pending_call,
    );
    unsafe {
        (*frame).call = call;
        let mut initialized_receiver = false;
        if target_is_instance && (*frame).num_cvs != 0 {
            let receiver = (*frame).cv(0);
            if receiver.value_type() == ValueType::Object {
                if common.plan.borrow_this() {
                    frame_set_borrowed_this(call, receiver as *const Value);
                } else {
                    frame_set_this(call, receiver.clone());
                }
                initialized_receiver = true;
            }
        }
        if !initialized_receiver {
            // Static method frames retain the class-method CV layout, whose
            // first slot is reserved for `$this`. No receiver is written for
            // a genuine static call, but wide-frame cleanup scans every CV
            // instead of using the compact ownership bitmap. Publish Undef so
            // stale stack bytes can never be mistaken for an owned PHP value.
            frame_slot_init(call, (*call).cv_mut(0) as *mut Value, Value::undef());
        }
    }
    let called_scope_class_id = if raw_class.eq_ignore_ascii_case("self")
        || raw_class.eq_ignore_ascii_case("parent")
    {
        let forwarding = late_static_call_class_id(eg, frame);
        if forwarding != 0 {
            forwarding
        } else {
            eg.class_id_of(&class)
        }
    } else {
        eg.class_id_of(&class)
    };
    if called_scope_class_id != 0 {
        publish_late_static_call_class_id(eg, call, called_scope_class_id);
    }
    if let Some(method) = magic_method {
        push_pending_magic_call(eg, call as usize, method);
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_late_static_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    #[cfg(target_arch = "x86_64")]
    let class_id = {
        if opline.result_type == OpType::Const {
            let validated = &op_array.cache[opline.result as usize];
            if validated.class_id != 0 {
                validated.class_id
            } else {
                late_static_call_class_id(eg, frame)
            }
        } else {
            late_static_call_class_id(eg, frame)
        }
    };
    #[cfg(not(target_arch = "x86_64"))]
    let class_id = late_static_call_class_id(eg, frame);
    let cache = &op_array.cache[ip];
    let func_ptr = if cache.class_id == class_id && !cache.func.is_null() {
        cache.func
    } else {
        let Some(class_definition) = eg.class_by_id(class_id) else {
            return Err(VmError::Fatal(
                "Cannot access \"static\" when no class scope is active".into(),
            ));
        };
        let method_name = unsafe {
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
        };
        let class = class_definition.name.clone();
        let method = method_name.as_str().unwrap_or("");
        let full_name = format!("{}::{}", class, method);
        let resolved = match eg.find_function(&full_name) {
            Some(pointer) => pointer,
            None => {
                let error = make_error_value(
                    "Error",
                    &format!("Call to undefined method {}::{}()", class, method),
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        };

        if let Some((visibility, defining_class)) = eg.find_method_visibility(&class, method) {
            if visibility != Visibility::Public {
                let caller_class = if opline._pad & CALL_FLAG_DYNAMIC_STATIC_SCOPE != 0 {
                    resolve_static_call_class(eg, frame, "self", true)
                } else {
                    get_caller_class(frame, eg)
                };
                if !eg.check_visibility(caller_class.as_deref(), &defining_class, visibility) {
                    let visibility = match visibility {
                        Visibility::Protected => "protected",
                        Visibility::Private => "private",
                        Visibility::Public => "public",
                    };
                    return Err(VmError::Fatal(format!(
                        "Call to {} method {}::{}() from scope {}",
                        visibility,
                        defining_class,
                        method,
                        caller_class.as_deref().unwrap_or("global")
                    )));
                }
            }
        }

        unsafe {
            let cache = &mut *(op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache);
            cache.class_id = class_id;
            cache.func = resolved;
        }
        resolved
    };

    let num_args = opline.extended_value;
    let common = unsafe { &*func_ptr };
    if common.fn_type == FunctionType::User && num_args == common.sig.public_arity() {
        let user = unsafe { &*(func_ptr as *const UserFunction) };
        if let Some(plan) = user.scalar_long_plan.as_deref()
            && let Some((result, do_fcall_ptr)) = unsafe {
                try_execute_direct_scalar_long_call(
                    frame,
                    op_array,
                    (opline as *const Instruction).add(1),
                    common,
                    plan,
                )
            }
        {
            stats::inc_do_fcall_fast();
            stats::inc_return_fast();
            let count = common.call_count.get();
            if count < u32::MAX {
                common.call_count.set(count + 1);
            }
            unsafe { complete_direct_scalar_long_call(frame, do_fcall_ptr, result) };
            return Ok(ColdResult::Continue);
        }
    }

    let pending_call = unsafe { (*frame).call };
    let call = eg.vm_stack.push_call_frame(
        func_ptr,
        num_args + 1,
        num_args,
        frame,
        pending_call,
    );
    unsafe {
        (*frame).call = call;
        // Late-static method calls use the same hidden class-method slot as
        // ordinary static calls. A genuine static target has no receiver to
        // publish there, so initialize it before SendVal fills CV 1..N. This
        // is required by wide frames, whose cleanup scans all CVs.
        frame_slot_init(call, (*call).cv_mut(0) as *mut Value, Value::undef());
    }
    if class_id != 0 {
        publish_late_static_call_class_id(eg, call, class_id);
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_init_user_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let resolved = match resolve_user_call_at_opline(eg, frame, op_array, opline) {
        Some(resolved) => resolved,
        None => {
            let callback_raw = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            let callback = if callback_raw.is_reference() {
                unsafe { &*callback_raw.as_ref_ptr() }
            } else {
                callback_raw
            };
            let description = callback.echo_to_string();
            let message = if opline._pad == 1 {
                if callback.as_str().is_some() {
                    format!(
                        "call_user_func_array(): Argument #1 ($callback) must be a valid callback, function \"{description}\" not found or not callable"
                    )
                } else {
                    "call_user_func_array(): Argument #1 ($callback) must be a valid callback, no array or string given".to_string()
                }
            } else {
                format!(
                    "call_user_func(): Argument #1 ($callback) must be a valid callback, function \"{description}\" not found or not callable"
                )
            };
            let error = make_error_value("TypeError", &message);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    };

    if let Some(name) = crate::stdlib::scope_introspection_callback_name(&resolved) {
        let error = make_error_value("Error", &format!("Cannot call {name}() dynamically"));
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    }

    init_resolved_user_call(eg, frame, opline.extended_value, resolved);
    Ok(ColdResult::Done)
}

fn scope_introspection_function_name(name: &str) -> Option<&'static str> {
    let normalized = name.strip_prefix('\\').unwrap_or(name);
    if normalized.contains('\\') {
        return None;
    }
    match normalized.to_ascii_lowercase().as_str() {
        "extract" => Some("extract"),
        "compact" => Some("compact"),
        "get_defined_vars" => Some("get_defined_vars"),
        "func_get_args" => Some("func_get_args"),
        "func_get_arg" => Some("func_get_arg"),
        "func_num_args" => Some("func_num_args"),
        _ => None,
    }
}

#[inline]
fn resolve_user_call_at_opline(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Option<crate::stdlib::ResolvedCallback> {
    let callback_raw = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    let callback = if callback_raw.is_reference() {
        unsafe { &*callback_raw.as_ref_ptr() }
    } else {
        callback_raw
    };
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache_slot = unsafe {
        op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache
    };
    let caller_class = get_caller_class(frame, eg);
    crate::stdlib::resolve_callback_with_cache(
        callback,
        eg,
        caller_class.as_deref(),
        Some(cache_slot),
    )
}

#[inline]
fn init_resolved_user_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    explicit_args: u32,
    resolved: crate::stdlib::ResolvedCallback,
) {
    init_resolved_user_call_mode(eg, frame, explicit_args, resolved, false);
}

#[inline]
fn init_resolved_user_call_mode(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    explicit_args: u32,
    mut resolved: crate::stdlib::ResolvedCallback,
    defer_method_receiver: bool,
) {
    let magic_method = if resolved.is_magic_call {
        debug_assert_eq!(resolved.use_vars.len(), 1);
        resolved.use_vars.pop()
    } else {
        None
    };
    let called_scope_class_id = resolved.called_scope_class_id;
    let bound_this = resolved.bound_this;
    let closure_static_vars = resolved.closure_static_vars;
    let signature = unsafe { &(*resolved.func_ptr).sig };
    let public_end = signature.this_offset + explicit_args;
    let parameter_cv_count = signature.parameter_cv_count();
    let capture_end = parameter_cv_count + resolved.use_vars.len() as u32;
    let storage_slots = public_end.max(capture_end);
    let pending_call = unsafe { (*frame).call };
    let call = eg.vm_stack.push_call_frame(
        resolved.func_ptr,
        storage_slots,
        explicit_args,
        frame,
        pending_call,
    );
    unsafe {
        (*frame).call = call;
    }
    if called_scope_class_id != 0 {
        publish_late_static_call_class_id(eg, call, called_scope_class_id);
    }
    if let Some(storage) = closure_static_vars {
        eg.publish_closure_static_vars(call as usize, storage);
    }
    if let Some(method) = magic_method {
        // Late-static scope stays below this entry and the receiver marker is
        // pushed later, so DoFcall consumes receiver -> magic -> scope in order.
        push_pending_magic_call(eg, call as usize, method);
    }

    // push_call_frame leaves the whole requested argument prefix
    // uninitialized. Optional declared parameters between the supplied
    // arguments and closure captures must remain readable Undef slots.
    for index in public_end..parameter_cv_count {
        // SAFETY: `push_call_frame` allocated every declared CV, and this loop
        // initializes only the unsupplied optional-parameter suffix once.
        unsafe {
            let destination = (*call).cv_mut(index) as *mut Value;
            destination.write(Value::undef());
        }
    }

    if defer_method_receiver && !resolved.prepend_args.is_empty() {
        debug_assert_eq!(resolved.prepend_args.len(), 1);
        push_pending_invoke_this(eg, call as usize, resolved.prepend_args[0].clone());
    } else {
        for (index, value) in resolved.prepend_args.into_iter().enumerate() {
            // SAFETY: callback resolution bounds the hidden receiver prefix to
            // CVs reserved by the selected user function's signature.
            unsafe {
                let destination = (*call).cv_mut(index as u32) as *mut Value;
                frame_slot_init(call, destination, value);
            }
        }
    }

    if !resolved.use_vars.is_empty()
        && (signature.is_variadic || explicit_args > signature.public_arity())
    {
        // Variadic storage and tolerated extra user arguments can occupy the
        // CV range where a closure body expects its lexical captures. Keep
        // captures pending until DoFcall has snapshotted the supplied argument
        // list, then restore them at the declared parameter boundary.
        eg.pending_closure_captures
            .insert(call as usize, resolved.use_vars);
    } else {
        for (index, value) in resolved.use_vars.into_iter().enumerate() {
            // SAFETY: call frame sizing included all non-variadic captures
            // after parameter_cv_count, and each destination is written once.
            let destination = unsafe { (*call).cv_mut(parameter_cv_count + index as u32) }
                as *mut Value;
            unsafe { frame_slot_init(call, destination, value) };
        }
    }
    initialize_bound_this_frame(call, resolved.func_ptr, bound_this);
}

#[inline(never)]
fn op_init_dynamic_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let (callable, instruction_index) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
        )
    };

    if callable.value_type() == ValueType::Array {
        let callable_array = callable
            .as_array()
            .expect("array-tagged dynamic callable must expose array storage");
        if callable_array.len() != 2 {
            let error = make_error_value(
                "Error",
                "Array callback must have exactly two elements",
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let closure_receiver = callable_array
            .get_value_at(0)
            .is_some_and(|receiver| receiver.value_type() == ValueType::Closure);
        if !closure_receiver
            && callable_array
                .get_value_at(0)
                .is_some_and(|class| class.as_str().is_none() && class.as_object().is_none())
        {
            let error = make_error_value(
                "Error",
                "Class name must be a valid object or a string",
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        if callable_array.len() == 2
            && callable_array
                .get_value_at(1)
                .is_some_and(|method| method.as_str().is_none())
        {
            let error = make_error_value("Error", "Method name must be a string");
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let class_name = callable.as_array().and_then(|array| {
            if array.len() != 2 || array.get_value_at(1)?.as_str().is_none() {
                return None;
            }
            array.get_value_at(0)?.as_str().map(str::to_string)
        });
        if let Some(class_name) = class_name.as_deref()
            && matches!(class_name.to_ascii_lowercase().as_str(), "self" | "parent" | "static")
            && get_caller_class(frame, eg).is_none()
        {
            let error = make_error_value(
                "Error",
                &format!(
                    "Cannot access \"{}\" when no class scope is active",
                    class_name.to_ascii_lowercase()
                ),
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        if let Some(class_name) = class_name.as_deref()
            && eg.find_class(class_name).is_none()
        {
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, class_name)?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
            if !loaded {
                let error = make_error_value("Error", &format!("Class \"{class_name}\" not found"));
                attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        }
        if let Some(class_name) = class_name.as_deref()
            && let Some(method) = callable_array.get_value_at(1).and_then(Value::as_str)
            && class_callback_requires_instance(eg, class_name, method)
        {
            return Ok(throw_non_static_callback_error(
                eg,
                frame,
                op_array,
                instruction_index,
                class_name,
                method,
            ));
        }
        let Some(resolved) = resolve_user_call_at_opline(eg, frame, op_array, opline) else {
            let message = if closure_receiver {
                let method = callable_array
                    .get_value_at(1)
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!("Call to undefined method Closure::{method}()")
            } else if let Some(method) = callable_array
                .get_value_at(1)
                .and_then(Value::as_str)
                && let Some(class) = callable_array.get_value_at(0).and_then(|receiver| {
                    receiver
                        .as_object()
                        .map(|object| object.class_name.to_string())
                        .or_else(|| receiver.as_str().map(str::to_string))
                })
            {
                format!("Call to undefined method {class}::{method}()")
            } else {
                "Array is not callable".to_string()
            };
            let error = make_error_value("Error", &message);
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        };
        // Dynamic-call sends start at CV 0 because the compiler cannot know
        // that this callable is a method. Defer the hidden receiver until
        // DoFcall, which shifts the supplied positional prefix by one.
        init_resolved_user_call_mode(eg, frame, opline.extended_value, resolved, true);
        return Ok(ColdResult::Done);
    }

    if let Some(closure) = callable.as_closure() {
        let func_ptr = closure.func;
        let bound_this = closure.bound_this.clone();
        // Class-scoped anonymous closures carry visibility and `$this`
        // metadata, but only reflected/first-class method closures reserve a
        // hidden receiver CV in their signature.
        let mut resolved = crate::stdlib::ResolvedCallback {
            func_ptr,
            prepend_args: vec![],
            use_vars: closure.clone_captures(),
            called_scope_class_id: closure.called_scope_class_id,
            bound_this,
            closure_static_vars: closure.static_vars.clone(),
            is_magic_call: crate::stdlib::closure_is_magic_call(closure, eg),
        };
        let is_method = resolved.is_method();
        if is_method {
            resolved.prepend_args = vec![resolved.bound_this.clone().unwrap_or_else(Value::null)];
        }
        // Dynamic sends start at CV 0. A first-class method closure retains
        // the hidden receiver slot, so defer it until DoFcall shifts the
        // explicit argument prefix exactly like an array method callback.
        init_resolved_user_call_mode(eg, frame, opline.extended_value, resolved, is_method);
        return Ok(ColdResult::Done);
    } else if let Some(func_name) = callable.as_str() {
        // Simple string function call: $func = "my_func"; $func()
        if let Some((class_name, method)) = func_name.rsplit_once("::") {
            let class_name = class_name.trim_start_matches('\\');
            if class_callback_requires_instance(eg, class_name, method) {
                return Ok(throw_non_static_callback_error(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    class_name,
                    method,
                ));
            }
        }
        if let Some(normalized) = scope_introspection_function_name(func_name) {
            let error = make_error_value(
                "Error",
                &format!("Cannot call {normalized}() dynamically"),
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let Some(func_ptr) = eg.find_function(func_name) else {
            let error = make_error_value(
                "Error",
                &format!("Call to undefined function {}()", func_name),
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        };

        let num_args = opline.extended_value;
        let pending_call = unsafe { (*frame).call };
        let call = eg.vm_stack.push_call_frame(
            func_ptr,
            num_args,
            num_args,
            frame,
            pending_call,
        );
        unsafe {
            (*frame).call = call;
        }
    } else if callable.value_type() == ValueType::Object {
        // Object with __invoke: set up as method call to __invoke
        let obj = callable.as_object().unwrap();
        let class_name = obj.class_name.clone();
        drop(obj);
        let full_name = format!("{}::__invoke", class_name.to_lowercase());
        let func_ptr = match eg.find_function(&full_name) {
            Some(ptr) => ptr,
            None => {
                let error = make_error_value(
                    "Error",
                    &format!("Object of type {class_name} is not callable"),
                );
                attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        };

        let num_args = opline.extended_value;
        // +1 for $this at CV 0; but don't write $this yet because
        // SendVal will write args to CV 0..N-1 (compiler doesn't know
        // it's a method call). We'll shift args in DoFcall.
        let pending_call = unsafe { (*frame).call };
        let call = eg.vm_stack.push_call_frame(
            func_ptr,
            num_args + 1,
            num_args,
            frame,
            pending_call,
        );
        unsafe {
            (*frame).call = call;
        }
        // Keep the receiver attached to its pending call. Argument expressions
        // may execute nested calls before this frame reaches DoFcall.
        push_pending_invoke_this(eg, call as usize, callable.clone());
    } else {
        let error = make_error_value(
            "Error",
            &format!(
                "Value of type {} is not callable",
                callable.dereferenced().type_name()
            ),
        );
        attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    }
    Ok(ColdResult::Done)
}
