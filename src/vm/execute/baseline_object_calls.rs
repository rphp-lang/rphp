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
    let (ip, result_ptr, raw_name) = unsafe {
        (
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
            (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
                .as_str()
                .unwrap_or(""),
        )
    };
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
        } else {
            raw_name.to_string()
        };
        owned_name.as_str()
    };

    if !literal_cache_hit {
        stats::inc_newobj_class_hash_lookup();
    }
    if !literal_cache_hit
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
        eg.register_class(class_def).map_err(VmError::Fatal)?;
    }

    if (dynamic_static_scope || dynamic_class_name || ic.class_id == 0)
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
            return Err(VmError::Fatal(format!("Uncaught Error: Class \"{name}\" not found")));
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

    // Reject instantiation of interfaces, abstract classes, and internal-only classes
    if name == "Generator" {
        return Err(VmError::Fatal(
            "The \"Generator\" class is reserved for internal use and cannot be manually instantiated".into()
        ));
    }
    if let Some(class_def) = class_def {
        if class_def.is_interface {
            return Err(VmError::Fatal(format!(
                "Cannot instantiate interface {}",
                name
            )));
        }
        if class_def.is_abstract {
            return Err(VmError::Fatal(format!(
                "Cannot instantiate abstract class {}",
                name
            )));
        }
        if class_def.is_enum {
            let err = make_error_value("Error", &format!(
                "Cannot instantiate enum {}",
                name
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
    // NewObj writes a compiler-owned TMP/VAR for the first time. Stack reuse
    // intentionally leaves dead scalar bytes uninitialized, so dropping the
    // old bytes here is invalid; the tracked TMP writer treats an unset bitmap
    // bit as no live value and records the new object's ownership.
    unsafe { frame_tmp_set(frame, result_ptr, Value::object(obj)) };
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
    };
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
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
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
        return Err(VmError::Fatal("Attempt to read property on non-object".into()));
    }

    let name = prop_name.as_str().unwrap_or("");
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
        let key = crate::runtime::resolve_property_key(eg, &obj.class_name, name, effective_caller);

        // Determine if property is public (for caching)
        let mut is_public = true;
        let mut property_accessible = true;
        // Visibility check
        if let Some((vis, defining_class)) = eg.find_property_visibility(&obj.class_name, name) {
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
                    if let Some((Visibility::Private, ref dc)) = eg.find_property_visibility(cc, name) {
                        dc.eq_ignore_ascii_case(cc)
                    } else {
                        false
                    }
                });
                if !own_private && !caller_has_own {
                    if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
                        let has_getter = eg
                            .find_function(&format!(
                                "{}::__get",
                                obj.class_name.to_ascii_lowercase()
                            ))
                            .is_some();
                        if opline._pad & FETCH_OBJ_SILENT == 0 && !has_getter {
                            let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                            return Err(VmError::Fatal(format!(
                                "Cannot access {} property {}::${}",
                                vis_str, defining_class, name
                            )));
                        }
                        property_accessible = false;
                    }
                }
            }
        }

        // Cache only declared public properties. Dynamic properties have no
        // stable slot and remain on the cold lookup path.
        let cache_dynamic_std_class =
            is_public && key == name && obj.is_dynamic_std_class();
        if is_public && key == name && obj.class_id != 0 {
            if let Some(slot) = obj.property_slot(&key) {
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
                let mut flags: u32 = 1; // read-safe
                let writable = eg.class_table.get(obj.class_name.as_ref()).is_none_or(|cd| {
                    !cd.is_enum && !cd.readonly_props.iter().any(|prop| prop == name)
                });
                if writable {
                    flags |= 2;
                }
                ic_mut.set_property(obj.class_id, slot, flags);
            }
        }

        let declared_slot = property_accessible
            .then(|| obj.property_slot(&key))
            .flatten();
        let definition = declared_slot.and_then(|slot| {
            eg.instance_property_definition(obj.class_id, slot)
        });
        let (found_val, dynamic_position) = if !property_accessible {
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
        if let Some(val) = found_val {
            if val.is_undef() && definition.is_some_and(|definition| definition.is_typed()) {
                if opline._pad & FETCH_OBJ_SILENT != 0 {
                    set_result(Value::null());
                    return Ok(ColdResult::Done);
                }
                let definition = definition.unwrap();
                let error = make_error_value(
                    "Error",
                    &format!(
                        "Typed property {}::${} must not be accessed before initialization",
                        definition.type_scope, definition.name
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
                let magic_set = call_guarded_property_magic_method(
                    eg,
                    obj_val,
                    name,
                    PROPERTY_GUARD_ISSET,
                    "__isset",
                    &[Value::string(name)],
                )?;
                if let Some(result) = take_magic_exception(eg, frame) {
                    return Ok(result);
                }
                if !magic_set.is_some_and(|value| value.is_truthy()) {
                    set_result(Value::null());
                    return Ok(ColdResult::Done);
                }
            }
            // Property not found (or accepted by __isset) — try __get.
            if name.starts_with('\0')
                && property_guard_active(obj_val, name, PROPERTY_GUARD_GET)
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
                obj_val,
                name,
                PROPERTY_GUARD_GET,
                "__get",
                &[Value::string(name)],
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
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
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
    let property_state = if accessible {
        let key = crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            name,
            effective_caller,
        );
        object_ref
            .get_property(&key)
            .map(|value| !value.is_undef() && value.value_type() != ValueType::Null)
    } else {
        None
    };
    drop(object_ref);

    let set = match property_state {
        Some(set) => set,
        None => call_guarded_property_magic_method(
            eg,
            object,
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
    let name = property
        .as_str()
        .ok_or_else(|| VmError::Fatal("Property name must be a string".into()))?;
    let caller_class = get_caller_class(frame, eg);
    let object_ref = object.as_object().unwrap();
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
    let key = crate::runtime::resolve_property_key(
        eg,
        &object_ref.class_name,
        name,
        effective_caller,
    );
    let removed = accessible && object_ref.contains_property(&key);
    drop(object_ref);

    if removed {
        object.as_object_mut().unwrap().unset_property(&key);
        return Ok(ColdResult::Done);
    }
    let _ = call_guarded_property_magic_method(
        eg,
        object,
        name,
        PROPERTY_GUARD_UNSET,
        "__unset",
        &[Value::string(name)],
    )?;
    if let Some(result) = take_magic_exception(eg, frame) {
        return Ok(result);
    }
    Ok(ColdResult::Done)
}

include!("instance_property_cache.rs");

fn op_bind_obj_prop_ref(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    // SAFETY: all three operand slots are compiler-owned by the active frame.
    // The receiver is cloned before its CV can be replaced, and the owned
    // reference cell keeps the property target stable across object growth.
    unsafe {
        let receiver = (&*(*frame).get_op_ptr(
            opline.op1 as u32,
            opline.op1_type,
            op_array,
        ))
            .clone();
        let name = (&*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        ))
            .as_str()
            .ok_or_else(|| VmError::Fatal("Property name must be a string".into()))?
            .to_string();
        let object = receiver
            .as_object()
            .ok_or_else(|| VmError::Fatal("Attempt to bind property on non-object".into()))?;
        let class_name = object.class_name.to_string();
        drop(object);

        let caller_class = get_caller_class(frame, eg);
        let receiver_in_scope = caller_class
            .as_ref()
            .is_some_and(|caller| eg.class_is_a(&class_name, caller));
        let effective_caller = receiver_in_scope
            .then_some(caller_class.as_deref())
            .flatten();
        if let Some((visibility, defining_class)) =
            eg.find_property_visibility(&class_name, &name)
            && visibility != Visibility::Public
            && !eg.check_visibility(effective_caller, &defining_class, visibility)
        {
            let visibility = match visibility {
                Visibility::Protected => "protected",
                Visibility::Private => "private",
                Visibility::Public => "public",
            };
            return Err(VmError::Fatal(format!(
                "Cannot access {visibility} property {defining_class}::${name}"
            )));
        }
        if eg
            .find_class(&class_name)
            .is_some_and(|class| class.readonly_props.contains(&name))
        {
            return Err(VmError::Fatal(format!(
                "Cannot acquire reference to readonly property {class_name}::${name}"
            )));
        }

        let key = crate::runtime::resolve_property_key(
            eg,
            &class_name,
            &name,
            effective_caller,
        );
        let mut object = receiver.as_object_mut().unwrap();
        let binding = if let Some(property) = object.get_property_mut(&key) {
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
            object.set_property(&key, binding.clone_owned_reference_alias());
            binding
        };
        drop(object);

        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
        frame_slot_set(frame, destination, binding);
    }
    Ok(())
}

fn op_bind_array_dim_ref(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    // SAFETY: the compiler emits mutable array/CV operands owned by this live
    // frame. Promoting the element to an Rc-backed cell makes both aliases
    // independent of subsequent array storage reallocations.
    unsafe {
        let index = &*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        );
        let key = value_to_array_key(index)?;
        let array_ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
        if matches!((*array_ptr).value_type(), ValueType::Null | ValueType::Undef) {
            slot_set(array_ptr, Value::array(PhpArray::new()));
        }
        let array = (&mut *array_ptr)
            .as_array_mut()
            .ok_or_else(|| VmError::Fatal("Cannot acquire reference to non-array offset".into()))?;
        if array.get_key_mut(&key).is_none() {
            array.set(key.clone(), Value::null());
        }
        let element = array.get_key_mut(&key).unwrap();
        let binding = if element.is_owned_reference() {
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
        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
        frame_slot_set(frame, destination, binding);
    }
    Ok(())
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
        let obj = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        (prop_name, val, obj)
    };
    let mut assigned = val.clone();
    let name = prop_name.as_str().unwrap_or("").to_string();

    if let Some(php_obj) = obj.as_object_mut() {
        let caller_class = get_caller_class(frame, eg);

        // Same receiver-in-scope guard as FetchObjR — only allow
        // private bypass when the receiver is in the caller's hierarchy.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&php_obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Visibility check — use declaring class, not receiver class
        let mut prop_is_public = true;
        let mut property_accessible = true;
        if let Some((vis, defining_class)) = eg.find_property_visibility(&php_obj.class_name, &name) {
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
                if !own_private && !caller_has_own {
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
                            return Err(VmError::Fatal(format!(
                                "Cannot access {} property {}::${}",
                                vis_str, defining_class, name
                            )));
                        }
                    }
                }
            }
        }
        // Enum guard: enum cases are sealed — no property writes allowed
        // Track writability for cache population — enum/readonly are not cacheable for writes.
        let mut prop_is_writable = true;
        if let Some(class_def) = eg.class_table.get(php_obj.class_name.as_ref()) {
            if class_def.is_enum {
                let err = make_error_value("Error", &format!(
                    "Cannot modify readonly property {}::${}",
                    php_obj.class_name, name
                ));
                drop(php_obj);
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
        // Readonly property check
        if let Some(class_def) = eg.class_table.get(php_obj.class_name.as_ref()) {
            if class_def.readonly_props.contains(&name) {
                prop_is_writable = false;
                let key_check = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);
                let already_init = php_obj.get_property(&key_check)
                    .map_or(false, |v| !v.is_undef());
                if already_init {
                    // Already initialized — always error
                    let err = make_error_value("Error", &format!(
                        "Cannot modify readonly property {}::${}",
                        php_obj.class_name, name
                    ));
                    drop(php_obj);
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                    }
                } else {
                    // PHP 8.4+ readonly writes are protected(set): first
                    // initialization is available to the receiver's class
                    // family, including a parent constructor on a child object.
                    let in_declaring_scope = receiver_in_scope;
                    if !in_declaring_scope {
                        let err = make_error_value("Error", &format!(
                            "Cannot initialize readonly property {}::${} from {}",
                            php_obj.class_name, name,
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
        let key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);
        let declared_slot = property_accessible
            .then(|| php_obj.property_slot(&key))
            .flatten();
        let definition = declared_slot.and_then(|slot| {
            eg.instance_property_definition(php_obj.class_id, slot)
        });
        let object_class_id = php_obj.class_id;
        let object_class_name = php_obj.class_name.clone();
        let prop_exists = property_accessible && php_obj.contains_property(&key);
        drop(php_obj);
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

        if prop_exists {
            if let Some(mut php_obj) = obj.as_object_mut() {
                let property = php_obj
                    .get_property_mut(&key)
                    .expect("existing property must remain addressable during assignment");
                assignment_slot_set(property, assigned);
            }
        } else {
            // Property not found — try __set magic method
            let guarded = property_guard_active(obj, &name, PROPERTY_GUARD_SET);
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
                obj,
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
                // No __set — fall back to direct insert
                if let Some(mut php_obj) = obj.as_object_mut() {
                    php_obj.set_property(&key, assigned);
                }
            }
        }
    } else {
        return Err(VmError::Fatal("Attempt to assign property on non-object".into()));
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
        let (func_ptr, has_generic_contract) = if !ic.func.is_null()
            && ic.class_id == obj_class_id
            && obj_class_id != 0
        {
            drop(obj); // release borrow — class_name not needed on cache hit
            (
                ic.func,
                cfg!(any(feature = "php-generics-erased", feature = "php-generics-reified"))
                    && ic.method_has_generic_contract(),
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
            let resolved = match eg.find_function(&full_name) {
                Some(ptr) => ptr,
                None => {
                    let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", dispatch_class, method));
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                        ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
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
            if obj_class_id != 0 {
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
            (resolved, resolved_has_generic_contract)
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
        let scalar_plan_eligible = !has_active_generic_contract
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
        let call = if deferred {
            eg.pending_call_stack.push_deferred_scalar_call(
                func_ptr,
                num_args + 1,
                num_args,
                frame,
                pending_call,
            )
        } else {
            eg.vm_stack.push_call_frame(
                func_ptr,
                num_args + 1,
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
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(contract) = generic_contract {
            eg.push_pending_generic_member_call(call as usize, contract);
        }
    } else {
        let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let method = method_name.as_str().unwrap_or("");
        let err = make_error_value("Error", &format!("Call to member function {}() on non-object", method));
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
fn op_init_static_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Inline cache: static calls have constant class+method — cache resolved func_ptr.
    // Visibility is checked on first resolve only (same instruction = same caller context).
    let dynamic_scope = opline._pad & CALL_FLAG_DYNAMIC_STATIC_SCOPE != 0;
    let class_name = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    let method_name = unsafe {
        &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
    };
    let raw_class = class_name.as_str().unwrap_or("").to_string();
    let method = method_name.as_str().unwrap_or("").to_string();
    let class = resolve_static_call_class(eg, frame, &raw_class, dynamic_scope)
        .unwrap_or_else(|| raw_class.clone());
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
    let cached = op_array.cache[ip].func;
    let func_ptr = if !cached.is_null() {
        cached
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

        let full_name = format!("{}::{}", class, method);
        let resolved = match eg.find_function(&full_name) {
            Some(ptr) => ptr,
            None => {
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
        };

        // Visibility check on first resolve for each dynamic class.
        if let Some((vis, defining_class)) = eg.find_method_visibility(&class, &method) {
            if vis != Visibility::Public {
                let caller_class = if dynamic_scope {
                    resolve_static_call_class(eg, frame, "self", true)
                } else {
                    get_caller_class(frame, eg)
                };
                if !eg.check_visibility(caller_class.as_deref(), &defining_class, vis) {
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
        if !dynamic_scope {
            unsafe {
                let cache = &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache);
                cache.func = resolved;
            }
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
    // +1 for $this at CV 0 (compiler allocates $this even for static calls)
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
        let target_is_instance = eg
            .find_method_info(&class, &method)
            .is_some_and(|(_, is_static, _)| !is_static)
            || (common.sig.this_offset == 1
                && matches!(raw_class.to_ascii_lowercase().as_str(), "self" | "parent"));
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
            let error = make_error_value("TypeError", &format!(
                "call_user_func(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found or not callable",
                description,
            ));
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    };

    init_resolved_user_call(eg, frame, opline.extended_value, resolved);
    Ok(ColdResult::Done)
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
    resolved: crate::stdlib::ResolvedCallback,
    defer_method_receiver: bool,
) {
    let called_scope_class_id = resolved.called_scope_class_id;
    let bound_this = resolved.bound_this;
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
    let callable = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    if callable.value_type() == ValueType::Array {
        let class_name = callable.as_array().and_then(|array| {
            if array.len() != 2 || array.get_value_at(1)?.as_str().is_none() {
                return None;
            }
            array.get_value_at(0)?.as_str().map(str::to_string)
        });
        if let Some(class_name) = class_name
            && eg.find_class(&class_name).is_none()
        {
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
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
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        }
        let resolved = resolve_user_call_at_opline(eg, frame, op_array, opline)
            .ok_or_else(|| VmError::Fatal("Array is not callable".into()))?;
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
        let func_ptr = eg.find_function(func_name).ok_or_else(|| {
            VmError::Fatal(format!("Call to undefined function {}()", func_name))
        })?;

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
            None => return Err(VmError::Fatal(format!("Call to undefined method {}::__invoke()", class_name))),
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
        return Err(VmError::Fatal(format!("Value of type {:?} is not callable", callable.value_type())));
    }
    Ok(ColdResult::Done)
}
