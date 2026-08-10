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
        if let Some(expected) = generic_contract
            .and_then(|contract| contract.value_parameters.get(index))
            .and_then(Option::as_ref)
            && !eg.generic_metadata.value_matches_resolved_type(
                value,
                expected,
                |actual, bound| eg.class_is_a(actual, bound),
            )
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
        if cache.property_flags() != 3 {
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            {
                let declaration = cache.generic_property_declaration()?;
                let instruction = callee
                    .op_array
                    .instructions
                    .get(assignment.cache_ip as usize)?;
                let property = callee
                    .op_array
                    .literals
                    .get(instruction.op2 as usize)?
                    .as_str()?;
                let argument = &*arguments[assignment.argument as usize];
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
    let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let name = class_name.as_str().unwrap_or("");
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let ic = &op_array.cache[ip];

    // Literal object creation is monomorphic in ordinary PHP code. After the
    // first canonical name lookup, use the stable numeric class index instead
    // of hashing the same class name on every allocation.
    let class_def = if ic.class_id != 0 {
        eg.class_by_id(ic.class_id)
    } else {
        eg.class_table.get(name).map(Box::as_ref)
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
    let (class_id, property_layout, property_values) =
        if let Some(class_def) = class_def {
            (
                class_def.class_id,
                class_def.property_layout.clone(),
                class_def.property_defaults.as_ref().to_vec(),
            )
        } else {
            (0, std::rc::Rc::new(crate::value::ObjectLayout::empty()), Vec::new())
        };
    let obj = if class_id == 0 {
        PhpObject::dynamic(name.to_string(), class_id, std::collections::HashMap::new())
    } else {
        PhpObject::with_layout(class_id, property_layout, property_values)
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
                        .is_some_and(|owner| owner.eq_ignore_ascii_case(name))
            })
    }) {
        let object = unsafe { &*result_ptr };
        eg.bind_reified_object(object, binding);
    }
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if let Some(class) = eg.class_by_id(class_id) {
        let object = unsafe { &*result_ptr };
        for (property, default, _, _) in &class.properties {
            if let Some(default) = default {
                eg.check_generic_property_value(object, &class.name, property, default)
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

#[inline(always)]
fn finish_cached_fetch_obj_r(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    property_ptr: *const Value,
) -> CachedFetchObjResult {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    if let Some(strlen) = op_array.instructions.get(ip + 1) {
        let consumes_fetch = matches!(strlen.opcode, OpCode::Strlen | OpCode::Strlen_String)
            && matches!(opline.result_type, OpType::Tmp | OpType::Var)
            && strlen.op1_type == opline.result_type
            && strlen.op1 == opline.result
            && matches!(strlen.result_type, OpType::Tmp | OpType::Var);
        let property = unsafe { &*property_ptr };
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
fn op_fetch_obj_r_slow(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    if obj_val.value_type() != ValueType::Object {
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
                        let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                        return Err(VmError::Fatal(format!(
                            "Cannot access {} property {}::${}",
                            vis_str, defining_class, name
                        )));
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

        let (found_val, dynamic_position) = if cache_dynamic_std_class {
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
            unsafe { frame_slot_set(frame, result_ptr, val) };
        } else {
            // Property not found — try __get magic method
            if let Some(result) = call_magic_method(eg, obj_val, "__get", &[Value::string(name)])? {
                unsafe { frame_slot_set(frame, result_ptr, result) };
            } else {
                unsafe { frame_slot_set(frame, result_ptr, Value::null()) };
            }
        }
    }
    Ok(())
}

#[inline(never)]
fn op_assign_obj_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let prop_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
    let cloned = val.clone();
    let name = prop_name.as_str().unwrap_or("").to_string();
    let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let obj = unsafe { &*obj_ptr };

    if let Some(mut php_obj) = obj.as_object_mut() {
        let caller_class = get_caller_class(frame, eg);

        // Same receiver-in-scope guard as FetchObjR — only allow
        // private bypass when the receiver is in the caller's hierarchy.
        let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
            eg.class_is_a(&php_obj.class_name, cc)
        });
        let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };

        // Visibility check — use declaring class, not receiver class
        let mut prop_is_public = true;
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
                        let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                        return Err(VmError::Fatal(format!(
                            "Cannot access {} property {}::${}",
                            vis_str, defining_class, name
                        )));
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
                    // First initialization — only allowed from declaring class scope
                    let in_declaring_scope = caller_class.as_ref().map_or(false, |cc| {
                        cc.eq_ignore_ascii_case(&php_obj.class_name)
                    });
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
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        let generic_declaration = eg
            .check_generic_property_value(obj, &php_obj.class_name, &name, val)
            .map_err(VmError::Fatal)?;
        // Resolve storage key (mangled for private properties)
        let key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &name, effective_caller);

        // Cache: if public, not enum, not readonly, key == name → mark for write fast path.
        if prop_is_public && prop_is_writable && key == name && php_obj.class_id != 0 {
            let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
            let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
            if let Some(slot) = php_obj.property_slot(&key) {
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                if let Some(declaration) = generic_declaration {
                    #[cfg(all(
                        feature = "php-generics-erased",
                        not(feature = "php-generics-reified")
                    ))]
                    if eg
                        .generic_metadata
                        .property_erases_to_mixed(declaration, &name)
                    {
                        // An unbounded parameter erases to `mixed`; after the
                        // declaration is proven once, its write is identical
                        // to the ordinary property fast path.
                        ic_mut.set_property(php_obj.class_id, slot, 3);
                    } else {
                        ic_mut.set_generic_property(declaration, php_obj.class_id, slot);
                    }
                    #[cfg(feature = "php-generics-reified")]
                    ic_mut.set_generic_property(declaration, php_obj.class_id, slot);
                } else {
                    ic_mut.set_property(php_obj.class_id, slot, 3);
                }
                #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                ic_mut.set_property(php_obj.class_id, slot, 3);
            }
        }

        let prop_exists = php_obj.contains_property(&key);
        if prop_exists {
            php_obj.set_property(&key, cloned);
        } else {
            drop(php_obj); // Release borrow before potential magic method call
            // Property not found — try __set magic method
            if call_magic_method(eg, obj, "__set", &[Value::string(name.clone()), cloned.clone()])?.is_none() {
                // No __set — fall back to direct insert
                if let Some(mut php_obj) = obj.as_object_mut() {
                    php_obj.set_property(&key, cloned);
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
            if common.plan.borrow_this {
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
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
    let cached = op_array.cache[ip].func;
    let func_ptr = if !cached.is_null() {
        cached
    } else {
        let class_name = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
        let method_name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let class = class_name.as_str().unwrap_or("");
        let method = method_name.as_str().unwrap_or("");

        let full_name = format!("{}::{}", class, method);
        let resolved = match eg.find_function(&full_name) {
            Some(ptr) => ptr,
            None => {
                let err = make_error_value("Error", &format!("Call to undefined method {}::{}()", class, method));
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

        // Visibility check on first resolve
        if let Some((vis, defining_class)) = eg.find_method_visibility(class, method) {
            if vis != Visibility::Public {
                let caller_class = get_caller_class(frame, eg);
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

        // Cache for subsequent calls
        unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = resolved; }
        resolved
    };

    let num_args = opline.extended_value;
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
    let signature = unsafe { &(*resolved.func_ptr).sig };
    let public_end = signature.this_offset + explicit_args;
    let capture_end = signature.num_args + resolved.use_vars.len() as u32;
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

    // push_call_frame leaves the whole requested argument prefix
    // uninitialized. Optional declared parameters between the supplied
    // arguments and closure captures must remain readable Undef slots.
    for index in public_end..signature.num_args {
        let destination = unsafe { (*call).cv_mut(index) } as *mut Value;
        unsafe { destination.write(Value::undef()) };
    }

    for (index, value) in resolved.prepend_args.into_iter().enumerate() {
        let destination = unsafe { (*call).cv_mut(index as u32) } as *mut Value;
        unsafe { frame_slot_init(call, destination, value) };
    }

    let capture_offset = signature.num_args;
    for (index, value) in resolved.use_vars.into_iter().enumerate() {
        let destination = unsafe { (*call).cv_mut(capture_offset + index as u32) } as *mut Value;
        unsafe { frame_slot_init(call, destination, value) };
    }
}

#[inline(never)]
fn op_init_dynamic_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let callable = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    if let Some(closure) = callable.as_closure() {
        // Fast path: Closure value — direct function pointer, no string lookup.
        let func_ptr = closure.func;
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

        // Copy captured use_vars into CV slots after declared params
        let func = unsafe { &*func_ptr };
        let use_var_offset = func.sig.num_args;
        let n_captures = closure.captures.len();
        if n_captures > 0 {
            if !closure.has_heap_captures {
                // Scalar-only fast path: all captures are Long/Double/Bool/Null.
                // Raw memcpy — no clone overhead, no needs_cleanup checks.
                unsafe {
                    let src = closure.captures.as_ptr();
                    let dst = (*call).cv_mut(use_var_offset) as *mut Value;
                    std::ptr::copy_nonoverlapping(src, dst, n_captures);
                }
                // No heap flag needed — all scalars.
            } else {
                // General path: at least one heap capture, clone each.
                for (i, captured) in closure.captures.iter().enumerate() {
                    let cv_slot = unsafe { (*call).cv_mut(use_var_offset + i as u32) };
                    unsafe { frame_slot_init(call, cv_slot as *mut Value, captured.clone()) };
                }
            }
        }
    } else if let Some(arr) = callable.as_array() {
        // Legacy array callable: [class_or_object, method_name]
        let arr_len = arr.len();
        if arr_len == 0 {
            return Err(VmError::Fatal("Array is not callable".into()));
        }
        let func_name = arr.get_value_at(0)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                VmError::Fatal("Closure descriptor must start with function name".into())
            })?;

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

        // Copy captured use_vars into CV slots after params
        let func = unsafe { &*func_ptr };
        let use_var_offset = func.sig.num_args;
        for i in 1..arr_len {
            let captured_val = arr.get_value_at(i).unwrap().clone();
            let cv_slot = unsafe { (*call).cv_mut(use_var_offset + (i as u32 - 1)) };
            unsafe { frame_slot_set(call, cv_slot as *mut Value, captured_val) };
        }
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
    Ok(())
}
