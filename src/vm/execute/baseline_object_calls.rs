// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline]
fn internal_class_forbids_dynamic_properties(class_name: &str) -> bool {
    matches!(
        class_name,
        "Generator"
            | "WeakReference"
            | "WeakMap"
            | "InternalIterator"
            | "SensitiveParameterValue"
    )
}

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
            let Some(resolved) = resolve_static_call_class(eg, frame, raw_name, true) else {
                let error = make_error_value(
                    "Error",
                    &format!(
                        "Cannot access \"{}\" when no class scope is active",
                        raw_name.to_ascii_lowercase()
                    ),
                );
                attach_throwable_origin(&error, eg, frame, op_array, ip);
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            };
            resolved
        } else if dynamic_class_name && class_operand.value_type() == ValueType::Object {
            class_operand
                .as_object()
                .expect("object class operand must remain live")
                .class_name
                .to_string()
        } else if dynamic_class_name && class_operand.value_type() != ValueType::String {
            let error = make_error_value("Error", "Class name must be a valid object or a string");
            attach_throwable_origin(&error, eg, frame, op_array, ip);
            return Ok(match throw_in_frame(eg, frame, error)? {
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
        && eg.find_public_class(&name).is_none()
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
                    return Ok(match throw_in_frame(eg, frame, exception)? {
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
        for dependency in eg.method_variance_dependencies(&class_def) {
            stats::inc_newobj_class_hash_lookup();
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            if !loaded
                && let Some(error) = eg
                    .unavailable_method_variance_dependency_error(&class_def, &dependency)
            {
                return Err(VmError::Fatal(error));
            }
        }
        for dependency in crate::runtime::property_hook_setter_variance_dependencies(eg, &class_def)
        {
            stats::inc_newobj_class_hash_lookup();
            if eg.find_class(&dependency).is_some() {
                continue;
            }
            let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, &dependency)?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }
        let anonymous_parent_is_enum = class_def
            .parent
            .as_deref()
            .and_then(|parent| eg.find_class(parent))
            .is_some_and(|parent| parent.is_enum);
        if let Err(error) = eg.register_class(class_def) {
            if anonymous_parent_is_enum {
                return Err(VmError::CompileFatal(error));
            }
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
            eg.find_public_class(&name).is_none()
        }
    {
        let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &name)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
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
            )?);
        }
    }
    // Literal object creation is monomorphic in ordinary PHP code. After the
    // first canonical name lookup, use the stable numeric class index instead
    // of hashing the same class name on every allocation.
    let class_def = if literal_cache_hit {
        eg.class_by_id(ic.class_id)
    } else {
        stats::inc_newobj_class_hash_lookup();
        eg.find_public_class(&name)
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
) -> Result<ColdResult<'a>, VmError> {
    let error = make_error_value("Error", message);
    attach_throwable_origin(&error, eg, frame, op_array, ip);
    Ok(match throw_in_frame(eg, frame, error)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

#[cold]
fn attach_constant_expression_origin(
    throwable: &Value,
    definition: &crate::compiler::compile::DeferredPropertyDefault,
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    ip: usize,
) {
    let already_stamped = throwable.as_object().is_some_and(|object| {
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object
            .get_property("file")
            .and_then(Value::as_str)
            .is_some_and(|file| !file.is_empty())
            && object.contains_property(&trace_key)
    });
    if already_stamped || definition.source_file.is_empty() {
        return;
    }
    attach_throwable_origin(throwable, eg, frame, op_array, ip);
    let ignore_arguments = crate::stdlib::ini_default(eg, "zend.exception_ignore_args")
        .as_deref()
        .is_some_and(crate::stdlib::ini_boolean);
    let caller_trace = throwable
        .as_object()
        .and_then(|object| {
            let trace_key =
                crate::runtime::throwable_private_property_key(eg, &object, "trace");
            object.get_property(&trace_key).cloned()
        })
        .and_then(|trace| trace.as_array().cloned())
        .unwrap_or_else(PhpArray::new);
    let mut trace = PhpArray::new();
    let mut constant_expression = PhpArray::new();
    if !op_array.source_file.is_empty() {
        constant_expression.set_str(
            "file",
            Value::shared_string(op_array.source_file.clone()),
        );
    }
    if let Some(line) = op_array.source_line(ip) {
        constant_expression.set_str("line", Value::long(line as i64));
    }
    constant_expression.set_str("function", Value::string("[constant expression]"));
    if !ignore_arguments {
        constant_expression.set_str("args", Value::array(PhpArray::new()));
    }
    trace.push(Value::array(constant_expression));
    for (_, entry) in caller_trace.iter() {
        trace.push(entry.clone());
    }
    if let Some(mut object) = throwable.as_object_mut() {
        object.set_property("file", Value::string(definition.source_file.clone()));
        object.set_property("line", Value::long(definition.source_line as i64));
        let trace_key = crate::runtime::throwable_private_property_key(eg, &object, "trace");
        object.set_property(&trace_key, Value::array(trace));
    }
}

#[cold]
fn materialize_deferred_instance_defaults(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    ip: usize,
    class_id: u32,
) -> Result<Option<std::rc::Rc<[Value]>>, VmError> {
    let Some((entries, base_defaults, class_name)) = eg.class_by_id(class_id).and_then(|class| {
        class.deferred_instance_defaults.as_ref().map(|deferred| {
            (
                deferred.entries(),
                class.property_defaults.clone(),
                class.name.clone(),
            )
        })
    }) else {
        return Ok(None);
    };
    if let Some(resolved) = eg
        .class_by_id(class_id)
        .and_then(|class| class.deferred_instance_defaults.as_ref())
        .and_then(|deferred| deferred.resolved())
    {
        return Ok(Some(resolved));
    }

    let mut defaults = base_defaults.as_ref().to_vec();
    for deferred in entries.iter() {
        let Some(value) = crate::stdlib::reflection::evaluate_deferred_property_default_value(
            deferred,
            eg,
        )? else {
            let exception = eg
                .exception
                .as_ref()
                .expect("deferred property default failure sets an exception");
            attach_constant_expression_origin(
                exception,
                deferred,
                eg,
                frame,
                op_array,
                ip,
            );
            return Ok(None);
        };
        if eg.exception.is_some() {
            return Ok(None);
        }
        let definition = eg
            .class_by_id(class_id)
            .and_then(|class| class.properties.get(deferred.property_index))
            .ok_or_else(|| VmError::Fatal("Invalid deferred property default slot".into()))?;
        let value = match prepare_property_assignment(value, definition, eg, true, &class_name) {
            Ok(value) => value,
            Err(message) => {
                let exception = make_error_value("TypeError", &message);
                attach_constant_expression_origin(
                    &exception,
                    deferred,
                    eg,
                    frame,
                    op_array,
                    ip,
                );
                eg.exception = Some(exception);
                return Ok(None);
            }
        };
        let Some(slot) = defaults.get_mut(deferred.property_index) else {
            return Err(VmError::Fatal(
                "Invalid deferred property default template slot".into(),
            ));
        };
        *slot = value;
    }

    let resolved: std::rc::Rc<[Value]> = defaults.into();
    if let Some(deferred) = eg
        .class_by_id(class_id)
        .and_then(|class| class.deferred_instance_defaults.as_ref())
    {
        deferred.cache_resolved(resolved.clone());
    }
    Ok(Some(resolved))
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
        )?);
    }
    if name.eq_ignore_ascii_case("Closure") {
        return Ok(new_object_validation_error(
            eg,
            frame,
            op_array,
            ip,
            "Instantiation of class Closure is not allowed",
        )?);
    }
    if let Some(class_def) = class_def {
        if class_def.is_trait {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Cannot instantiate trait {}", class_def.name),
            )?);
        }
        if class_def.is_interface {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Cannot instantiate interface {}", class_def.name),
            )?);
        }
        if class_def.is_abstract {
            return Ok(new_object_validation_error(
                eg,
                frame,
                op_array,
                ip,
                &format!("Cannot instantiate abstract class {}", class_def.name),
            )?);
        }
        if class_def.is_enum {
            let err = make_error_value("Error", &format!(
                "Cannot instantiate enum {}",
                class_def.name
            ));
            match throw_in_frame(eg, frame, err)? {
                ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
            }
        }
    }

    if let Some(class_id) = class_id
        && eg.deferred_class_constants_require_activation(class_id)
        && !crate::stdlib::reflection::activate_deferred_class_constants(class_id, eg)?
    {
        let exception = eg
            .exception
            .take()
            .expect("deferred class-constant activation failure sets an exception");
        attach_constant_expression_trace(&exception, eg, frame, op_array, ip);
        attach_throwable_origin(&exception, eg, frame, op_array, ip);
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    let class_def = class_id.and_then(|class_id| eg.class_by_id(class_id));

    // Create compact declared-property slots from the class layout. Ordinary
    // classes clone the established immutable template directly. A class with
    // unresolved symbolic defaults materializes a request-local template on
    // first construction and publishes it only after every expression and
    // typed-property guard succeeds.
    let deferred_defaults = class_def
        .and_then(|class| class.deferred_instance_defaults.as_ref())
        .is_some_and(|defaults| defaults.has_runtime_entries());
    let deferred_defaults = if deferred_defaults {
        let class_id = class_def.expect("checked class definition").class_id;
        match materialize_deferred_instance_defaults(eg, frame, op_array, ip, class_id)? {
            Some(defaults) => Some(defaults),
            None => {
                let exception = eg
                    .exception
                    .take()
                    .expect("deferred property default failure sets an exception");
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }
    } else {
        None
    };
    let class_def = class_id.and_then(|class_id| eg.class_by_id(class_id));
    let (class_id, obj) = if let Some(class_def) = class_def {
        let class_id = class_def.class_id;
        let defaults = deferred_defaults
            .as_deref()
            .unwrap_or(class_def.property_defaults.as_ref());
        (
            class_id,
            PhpObject::with_layout_from_defaults(
                class_id,
                class_def.property_layout.clone(),
                defaults,
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
    let constructor_cache_hit = class_id != 0 && ic.class_id == class_id;
    let is_throwable = if constructor_cache_hit {
        ic.constructor_is_throwable()
    } else {
        eg.class_is_a(name, "Throwable")
    };
    if is_throwable {
        attach_new_throwable_origin(object, eg, frame, op_array, ip);
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
    let (func_ptr, constructor_has_destructor) = if constructor_cache_hit {
        (ic.func, ic.constructor_has_destructor())
    } else {
        let construct_name = format!("{}::__construct", name);
        let resolved = eg.find_function(&construct_name).unwrap_or(std::ptr::null());
        let has_destructor = !resolved.is_null()
            && eg.find_method_info(name, "__destruct").is_some();
        if class_id != 0 {
            let ic_mut = unsafe {
                &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache)
            };
            ic_mut.set_constructor(resolved, class_id, has_destructor, is_throwable);
        }
        (resolved, has_destructor)
    };
    if constructor_has_destructor {
        // A failed constructor permanently retires this allocation's own
        // destructor. Property values still follow their ordinary release
        // tree, because the marker suppresses only owner dispatch.
        unsafe { &*result_ptr }.suppress_unconstructed_object_destructor();
    }
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
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            if constructor_has_destructor {
                unsafe { &*result_ptr }.enable_constructed_object_destructor();
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
                    if constructor_has_destructor {
                        unsafe { &*result_ptr }.enable_constructed_object_destructor();
                    }
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
        if constructor_has_destructor {
            unsafe { (*call).set_original_constructor_call(true) };
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
) -> Result<Option<ColdResult<'a>>, VmError> {
    let Some(exception) = eg.exception.take() else {
        return Ok(None);
    };
    Ok(Some(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }))
}

enum ConvertedPropertyName<'a> {
    Name(String),
    Control(ColdResult<'a>),
}

#[inline(always)]
fn property_name_conversion_may_reenter(property: &Value) -> bool {
    matches!(
        property.dereferenced().value_type(),
        ValueType::Object | ValueType::Closure | ValueType::Array
    )
}

/// Convert a runtime property name through PHP's common object-member rule.
///
/// Dynamic property-name expressions may invoke `__toString()` or an error
/// handler for an array warning. Callers must therefore own the receiver and
/// any source value before entering this helper.
#[inline]
fn convert_object_property_name<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    property: &Value,
    suppress_warning: bool,
) -> Result<ConvertedPropertyName<'a>, VmError> {
    let property = property.dereferenced();
    if let Some(property) = property.as_str() {
        return Ok(ConvertedPropertyName::Name(property.to_string()));
    }
    let property = property.clone();
    if matches!(property.value_type(), ValueType::Object | ValueType::Closure) {
        let class_name = if property.value_type() == ValueType::Closure {
            "Closure".to_string()
        } else {
            property
                .as_object()
                .map(|object| {
                    object
                        .class_name
                        .strip_prefix("class@anonymous#")
                        .map_or_else(
                            || object.class_name.to_string(),
                            |_| "class@anonymous".to_string(),
                        )
                })
                .unwrap_or_else(|| "object".to_string())
        };
        let rendered = if property.value_type() == ValueType::Closure {
            None
        } else {
            call_magic_method(eg, &property, "__tostring", &[])?
        };
        if let Some(result) = take_magic_exception(eg, frame)? {
            return Ok(ConvertedPropertyName::Control(result));
        }
        let Some(rendered) = rendered else {
            return Ok(ConvertedPropertyName::Control(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Object of class {class_name} could not be converted to string"),
            )?));
        };
        let Some(rendered) = rendered.as_str() else {
            return Ok(ConvertedPropertyName::Control(object_property_throw(
                eg,
                frame,
                "TypeError",
                format!("{class_name}::__toString(): Return value must be of type string"),
            )?));
        };
        return Ok(ConvertedPropertyName::Name(rendered.to_string()));
    }

    if property.value_type() == ValueType::Array {
        report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            "Array to string conversion",
            suppress_warning,
        )?;
        if let Some(result) = take_magic_exception(eg, frame)? {
            return Ok(ConvertedPropertyName::Control(result));
        }
    }
    Ok(ConvertedPropertyName::Name(
        property
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| property.echo_to_string()),
    ))
}

#[inline(always)]
fn finish_cached_fetch_obj_r<const FUNC_ARG: bool>(
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
    let value = if FUNC_ARG && property.is_reference() {
        property.dereferenced().clone()
    } else {
        unsafe { (*property_ptr).clone() }
    };
    unsafe { frame_slot_set(frame, result_ptr, value) };
    CachedFetchObjResult::Complete
}

#[inline(always)]
fn finish_cached_restricted_obj_modify<const FUNC_ARG: bool>(
    frame: *mut ExecuteData,
    opline: &Instruction,
    property_ptr: *const Value,
) -> CachedFetchObjResult {
    // SAFETY: the guarded declared-property cache supplied this live slot and
    // no re-entrant operation occurs before the clone and frame-slot write.
    unsafe {
        let property = &*property_ptr;
        if property.is_undef()
            || !matches!(
                property.dereferenced().value_type(),
                ValueType::Object | ValueType::Closure
            )
        {
            return CachedFetchObjResult::Miss;
        }
        let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        let mut value = if FUNC_ARG && property.is_reference() {
            property.dereferenced().clone()
        } else {
            (*property_ptr).clone()
        };
        if value.is_reference() {
            value.mark_indirect_property_modification_reference();
        } else {
            value.mark_indirect_property_modification_result();
        }
        frame_slot_set(frame, result_ptr, value);
    }
    CachedFetchObjResult::Complete
}

#[inline(always)]
fn try_cached_fetch_obj_r<const RUNTIME_NAME: bool, const FUNC_ARG: bool>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> CachedFetchObjResult {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = &op_array.cache[ip];
    // SAFETY: both compiler operands belong to the live frame. A retained
    // dynamic-name cache key stays alive until replacement or op-array drop.
    let (obj_val, declared_name_matches) = unsafe {
        let object = (&*(*frame).get_op_ptr(
            opline.op1 as u32,
            opline.op1_type,
            op_array,
        ))
            .dereferenced();
        let name_matches = if !RUNTIME_NAME || cache.class_id == 0 {
            true
        } else {
            let requested = (&*(*frame).get_op_ptr(
                opline.op2 as u32,
                opline.op2_type,
                op_array,
            ))
                .dereferenced();
            let cached_name = cache.declared_property_name();
            requested.string_rc_ptr().is_some_and(|requested_name| {
                requested_name == cached_name
                    || (!cached_name.is_null()
                        && (&*cached_name).as_str()
                            == requested.as_str().unwrap_unchecked())
            })
        };
        (object, name_matches)
    };
    if obj_val.value_type() != ValueType::Object {
        return CachedFetchObjResult::Miss;
    }

    if opline._pad & FETCH_OBJ_CONSTANT_EXPRESSION != 0 {
        // SAFETY: the tag check above proves an Object value, and no
        // re-entrant operation or object mutation occurs before this read.
        let class_id = unsafe { obj_val.object_class_id_unchecked() };
        if class_id == 0 || !eg.class_by_id(class_id).is_some_and(|class| class.is_enum) {
            return CachedFetchObjResult::Miss;
        }
    }

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
        return finish_cached_fetch_obj_r::<FUNC_ARG>(frame, op_array, opline, property_ptr);
    }

    let object_class_id = unsafe { obj_val.object_class_id_unchecked() };
    if cache.property_flags() & 1 == 0
        || cache.class_id != object_class_id
        || object_class_id == 0
    {
        return CachedFetchObjResult::Miss;
    }
    if !declared_name_matches {
        return CachedFetchObjResult::Miss;
    }

    let property_ptr = unsafe {
        obj_val.object_property_slot_unchecked(cache.property_slot())
    };
    if opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC) != 0
        && cache.property_flags() & 2 == 0
    {
        if opline._pad & FETCH_OBJ_INCDEC != 0 {
            return CachedFetchObjResult::Miss;
        }
        if opline._pad & FETCH_OBJ_MODIFY != 0
            && opline._pad & FETCH_OBJ_COMPOUND == 0
        {
            return finish_cached_restricted_obj_modify::<FUNC_ARG>(
                frame,
                opline,
                property_ptr,
            );
        }
    }
    finish_cached_fetch_obj_r::<FUNC_ARG>(frame, op_array, opline, property_ptr)
}

#[cold]
#[inline(never)]
fn scalar_property_write_fetch_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    name: &str,
    receiver_type: &str,
    flags: u16,
) -> Result<ColdResult<'a>, VmError> {
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

#[inline(always)]
fn property_write_receiver_type(value: &Value) -> &'static str {
    match value.value_type() {
        ValueType::False => "false",
        ValueType::True => "true",
        _ => value.type_name(),
    }
}

#[inline]
fn property_fetch_write_capability_error(
    eg: &ExecutorGlobals,
    receiver: &Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    name: &str,
    caller_class: Option<&str>,
    indirect: bool,
) -> Option<String> {
    if definition.is_readonly {
        let action = if indirect { "indirectly modify" } else { "modify" };
        return Some(format!(
            "Cannot {action} readonly property {}::${name}",
            property_diagnostic_class_name(&definition.declaring_class),
        ));
    }
    let receiver_class = receiver.as_object()?.class_name.clone();
    if !eg.property_has_asymmetric_set_visibility(&receiver_class, name) {
        return None;
    }
    let (visibility, defining_class) =
        eg.find_property_set_visibility(&receiver_class, name)?;
    if visibility == Visibility::Public {
        return None;
    }
    if eg.check_instance_property_visibility(
        caller_class,
        &receiver_class,
        name,
        &defining_class,
        visibility,
    ) {
        return None;
    }
    let visibility = match visibility {
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Public => "public",
    };
    let action = if indirect { "indirectly modify" } else { "modify" };
    Some(format!(
        "Cannot {action} {visibility}(set) property {}::${name} from {}",
        property_diagnostic_class_name(&defining_class),
        caller_class.map_or_else(
            || "global scope".to_string(),
            |scope| format!("scope {scope}"),
        ),
    ))
}

#[inline]
fn detached_restricted_object_property_reference(
    eg: &ExecutorGlobals,
    receiver: &Value,
    name: &str,
    caller_class: Option<&str>,
) -> Option<Value> {
    let object = receiver.as_object()?;
    let (visibility, defining_class) =
        eg.find_property_visibility(&object.class_name, name)?;
    if visibility != Visibility::Public
        && !eg.check_instance_property_visibility(
            caller_class,
            &object.class_name,
            name,
            &defining_class,
            visibility,
        )
    {
        return None;
    }
    let key = crate::runtime::resolve_property_key(eg, &object.class_name, name, caller_class);
    let slot = object.property_slot(&key)?;
    let definition = eg.instance_property_definition(object.class_id, slot)?;
    if definition.has_get_hook || definition.has_set_hook {
        return None;
    }
    let current = object.get_property_slot(slot)?.dereferenced();
    matches!(
        current.value_type(),
        ValueType::Object | ValueType::Closure
    )
    .then(|| Value::owned_reference(current.clone()))
}

#[inline(never)]
fn op_fetch_obj_r_slow<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let suppressed = opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0;
    if suppressed {
        eg.begin_error_suppression(frame as usize);
    }
    let result = op_fetch_obj_r_slow_inner::<false>(eg, frame, op_array, opline);
    if suppressed {
        eg.end_error_suppression(frame as usize);
    }
    result
}

#[inline(never)]
fn op_fetch_obj_func_arg_slow<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_obj_r_slow_inner::<true>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_runtime_call_property_argument<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    if pending_call_argument_is_ref(
        frame,
        RuntimeCallArgument::Position(opline.extended_value),
    ) {
        return op_bind_obj_prop_ref(eg, frame, op_array, opline);
    }
    let cached = if opline.op2_type == OpType::Const {
        try_cached_fetch_obj_r::<false, true>(eg, frame, op_array, opline)
    } else {
        try_cached_fetch_obj_r::<true, true>(eg, frame, op_array, opline)
    };
    Ok(match cached {
        CachedFetchObjResult::Miss => {
            return op_fetch_obj_func_arg_slow(eg, frame, op_array, opline);
        }
        CachedFetchObjResult::Complete | CachedFetchObjResult::CompleteAndSkipNext => {
            ColdResult::Done
        }
    })
}

#[inline(never)]
fn op_fetch_obj_r_slow_inner<'a, const FUNC_ARG: bool>(
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
    let set_result = |value: Value| {
        // SAFETY: `result_ptr` is the live compiler-emitted slot proven above;
        // each invocation transfers exactly one owned Value into it.
        let value = if FUNC_ARG && value.is_reference() {
            value.dereferenced().clone()
        } else {
            value
        };
        unsafe { frame_slot_set(frame, result_ptr, value) };
    };

    if obj_val.value_type() != ValueType::Object {
        if opline._pad & FETCH_OBJ_SILENT != 0 {
            let mut result = Value::null();
            if opline._pad & FETCH_OBJ_UNSET != 0 {
                result.mark_indirect_property_modification_result();
            }
            set_result(result);
            return Ok(ColdResult::Done);
        }
        let name = prop_name
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| prop_name.echo_to_string());
        let write_flags =
            opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND);
        if obj_val.value_type() == ValueType::Closure {
            if write_flags != 0 {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    format!("Cannot create dynamic property Closure::${name}"),
                )?);
            }
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &format!("Undefined property: Closure::${name}"),
                opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            set_result(Value::null());
            return Ok(ColdResult::Done);
        }
        if write_flags != 0 {
            return Ok(scalar_property_write_fetch_throw(
                eg,
                frame,
                &name,
                property_write_receiver_type(obj_val),
                write_flags,
            )?);
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
        if let Some(result) = take_magic_exception(eg, frame)? {
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
    let name = match convert_object_property_name(
        eg,
        frame,
        op_array,
        opline,
        prop_name,
        opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
    )? {
        ConvertedPropertyName::Name(name) => name,
        ConvertedPropertyName::Control(result) => return Ok(result),
    };
    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };

    let write_flags = opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND);
    if write_flags != 0
        && let Some(missing_class) = obj_val
            .as_object()
            .and_then(|object| object.incomplete_class_name())
    {
        return Ok(object_property_throw_at(
            eg,
            frame,
            op_array,
            ip,
            "Error",
            format!(
                "The script tried to modify a property on an incomplete object. Please ensure that the class definition \"{missing_class}\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition"
            ),
        )?);
    }

    if opline._pad & FETCH_OBJ_CONSTANT_EXPRESSION != 0 {
        let is_enum = obj_val
            .as_object()
            .and_then(|object| eg.find_class(&object.class_name))
            .is_some_and(|class| class.is_enum);
        if !is_enum {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                ip,
                "Error",
                "Fetching properties on non-enums in constant expressions is not allowed"
                    .to_string(),
            )?);
        }
    }

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
                    if !eg.check_instance_property_visibility(
                        caller_class.as_deref(),
                        &obj.class_name,
                        &name,
                        &defining_class,
                        vis,
                    ) {
                        let has_getter = eg
                            .find_function(&format!(
                                "{}::__get",
                                obj.class_name.to_ascii_lowercase()
                            ))
                            .is_some();
                        // Once __get() has recursively re-entered this member,
                        // the overload guard no longer turns an inaccessible
                        // declaration into an undefined property. PHP reports
                        // the original visibility error at that boundary.
                        let getter_guarded = property_guard_active(
                            eg,
                            obj_val,
                            &name,
                            PROPERTY_GUARD_GET,
                        );
                        if opline._pad & FETCH_OBJ_SILENT == 0
                            && (!has_getter || getter_guarded)
                        {
                            let vis_str = match vis { Visibility::Protected => "protected", Visibility::Private => "private", _ => "public" };
                            let reported_class = if vis == Visibility::Protected {
                                obj.class_name.as_ref()
                            } else {
                                defining_class.as_str()
                            };
                            let message = format!(
                                "Cannot access {} property {}::${}",
                                vis_str, reported_class, name
                            );
                            drop(obj);
                            return Ok(object_property_throw(eg, frame, "Error", message)?);
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
        let explicitly_unset_declared = declared_property
            && obj
                .get_property(&key)
                .is_some_and(Value::is_explicitly_unset_property);
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
        let magic_get_can_handle = (!declared_property || explicitly_unset_declared)
            && !dynamic_property
            && ((has_magic_get
                && !property_guard_active(eg, obj_val, &name, PROPERTY_GUARD_GET))
                || (opline._pad & FETCH_OBJ_SILENT != 0
                    && has_magic_isset
                    && !property_guard_active(eg, obj_val, &name, PROPERTY_GUARD_ISSET)));
        let triggers_lazy_initialization = (property_accessible || force_dynamic)
            && !dynamic_property
            && !magic_get_can_handle;
        let initialized_target = if eg.lazy_object_state(obj_val).is_some() {
            Some(crate::stdlib::reflection::resolve_lazy_property_chain(
                eg,
                obj_val,
                &key,
                triggers_lazy_initialization,
            )?)
        } else {
            None
        };
        if let Some(result) = take_magic_exception(eg, frame)? {
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
                let dynamic_name = (opline.op2_type != OpType::Const)
                    .then(|| prop_name.dereferenced().string_rc_ptr())
                    .flatten();
                if !has_get_hook
                    && (opline.op2_type == OpType::Const || dynamic_name.is_some())
                {
                    // SAFETY: the opcode owns this cache entry for the op-array
                    // lifetime. Retaining a runtime string makes the pointer
                    // identity guard stable across CV replacement and COW.
                    unsafe {
                        let ic_mut = &mut *(op_array.cache.as_ptr().add(ip)
                            as *mut crate::vm::instruction::InlineCache);
                        let old_name = if opline.op2_type != OpType::Const
                            && ic_mut.class_id != 0
                            && ic_mut.property_flags() != 0
                        {
                            ic_mut.declared_property_name()
                        } else {
                            std::ptr::null()
                        };
                        if old_name != dynamic_name.unwrap_or(std::ptr::null()) {
                            if let Some(dynamic_name) = dynamic_name {
                                Value::retain_cached_string(dynamic_name);
                            }
                            if !old_name.is_null() {
                                Value::release_cached_string(old_name);
                            }
                        }
                        ic_mut.set_property(obj.class_id, slot, flags);
                        if let Some(dynamic_name) = dynamic_name {
                            ic_mut.set_declared_property_name(dynamic_name);
                        }
                    }
                }
            }
        }

        let declared_slot = (!force_dynamic && property_accessible)
            .then(|| obj.property_slot(&key))
            .flatten();
        let definition = declared_slot
            .and_then(|slot| eg.instance_property_definition(obj.class_id, slot))
            .cloned();
        let has_get_hook = definition
            .as_ref()
            .is_some_and(|definition| definition.has_get_hook);
        let get_hook_declaring_class = definition
            .as_ref()
            .filter(|definition| definition.has_get_hook)
            .map(|definition| definition.declaring_class.clone());
        let has_property_hook = definition
            .as_ref()
            .is_some_and(|definition| definition.has_get_hook || definition.has_set_hook);
        let write_only_property = definition
            .as_ref()
            .is_some_and(|definition| {
                definition.has_set_hook
                    && !definition.has_get_hook
                    && !definition.set_hook_is_backed
            });
        let typed_property_definition = definition
            .as_ref()
            .filter(|definition| definition.is_typed())
            .cloned();
        let typed_property = typed_property_definition
            .as_ref()
            .map(|definition| (definition.type_scope.clone(), definition.name.clone()));
        let explicitly_unset_declared = declared_slot.is_some()
            && obj
                .get_property(&key)
                .is_some_and(Value::is_explicitly_unset_property);
        let (mut found_val, dynamic_position) = if force_dynamic {
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
        let explicitly_unset_uses_missing_path = explicitly_unset_declared
            && (typed_property.is_none()
                || (has_magic_get
                    && !property_guard_active(
                        eg,
                        magic_receiver,
                        &name,
                        PROPERTY_GUARD_GET,
                    )));
        if explicitly_unset_uses_missing_path {
            found_val = None;
        }
        let current_incdec_value = || {
            obj_val
                .as_object()
                .and_then(|object| {
                    if force_dynamic || cache_dynamic_std_class {
                        object
                            .get_dynamic_property_with_position(&key)
                            .map(|(value, _)| value.clone())
                    } else if property_accessible {
                        object.get_property(&key).cloned()
                    } else {
                        None
                    }
                })
                .filter(|value| !value.is_undef())
                .unwrap_or_else(Value::null)
        };
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
            )?);
        }
        if has_get_hook
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_HOOK_GET)
        {
            let hook_name = format!("${name}::get");
            let hook_value = call_guarded_property_hook_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_HOOK_GET,
                get_hook_declaring_class
                    .as_deref()
                    .expect("get hook must retain its declaring class"),
                &hook_name,
                &[],
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if let Some(mut value) = hook_value {
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
                    )?);
                }
                if opline._pad & FETCH_OBJ_MODIFY != 0 {
                    if value.is_reference() {
                        value.mark_indirect_property_modification_reference();
                    } else if matches!(
                        value.dereferenced().value_type(),
                        ValueType::Object | ValueType::Closure
                    ) {
                        value.mark_indirect_property_modification_result();
                    }
                }
                set_result(value);
                return Ok(ColdResult::Done);
            }
        }
        if let Some(mut val) = found_val {
            if val.is_undef()
                && opline._pad & FETCH_OBJ_UNSET != 0
                && !has_get_hook
            {
                let mut result = Value::undef();
                result.mark_indirect_property_modification_result();
                set_result(result);
                return Ok(ColdResult::Done);
            }
            if has_property_hook
                && opline._pad & FETCH_OBJ_MODIFY != 0
                && opline._pad & (FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND) == 0
                && !val.is_reference()
            {
                if matches!(
                    val.dereferenced().value_type(),
                    ValueType::Object | ValueType::Closure
                ) {
                    val.mark_indirect_property_modification_result();
                } else {
                    let class_name = obj_val
                        .as_object()
                        .map(|object| object.class_name.to_string())
                        .unwrap_or_else(|| "object".to_string());
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "Error",
                        format!("Indirect modification of {class_name}::${name} is not allowed"),
                    )?);
                }
            }
            let indirect_modify = opline._pad & FETCH_OBJ_MODIFY != 0
                && opline._pad & (FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND) == 0;
            let identity_preserving_object = matches!(
                val.dereferenced().value_type(),
                ValueType::Object | ValueType::Closure
            );
            if indirect_modify
                && !identity_preserving_object
                && !(val.is_undef() && opline._pad & FETCH_OBJ_COMPOUND_RECEIVER != 0)
                && let Some(definition) = definition.as_ref()
                && let Some(message) = property_fetch_write_capability_error(
                    eg,
                    obj_val,
                    definition,
                    &name,
                    caller_class.as_deref(),
                    true,
                )
            {
                return Ok(object_property_throw(eg, frame, "Error", message)?);
            }
            if indirect_modify
                && identity_preserving_object
                && definition.as_ref().is_some_and(|definition| {
                    definition.is_readonly || definition.set_visibility.is_some()
                })
            {
                if val.is_reference() {
                    val.mark_indirect_property_modification_reference();
                } else {
                    val.mark_indirect_property_modification_result();
                }
            }
            if !val.is_undef()
                && opline._pad & FETCH_OBJ_REFERENCE_SOURCE != 0
                && !matches!(
                    val.dereferenced().value_type(),
                    ValueType::Object | ValueType::Closure
                )
                && obj_val
                    .as_object()
                    .and_then(|object| eg.class_table.get(object.class_name.as_ref()))
                    .is_some_and(|class| class.readonly_props.contains(&name))
                && !readonly_clone_reinitialization_allowed(eg, obj_val, &name)
            {
                let class_name = obj_val
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| "object".to_string());
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    if opline._pad & FETCH_OBJ_INCDEC != 0 {
                        format!("Cannot modify readonly property {class_name}::${name}")
                    } else {
                        format!(
                            "Cannot indirectly modify readonly property {class_name}::${name}"
                        )
                    },
                )?);
            }
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
                        property_diagnostic_class_name(type_scope),
                        property_name
                    ),
                );
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            if !val.is_undef()
                && opline._pad & FETCH_OBJ_INCDEC != 0
                && !readonly_clone_reinitialization_allowed(eg, obj_val, &name)
                && let Some(definition) = definition.as_ref()
                && let Some(message) = property_fetch_write_capability_error(
                    eg,
                    obj_val,
                    definition,
                    &name,
                    caller_class.as_deref(),
                    false,
                )
            {
                return Ok(object_property_throw(eg, frame, "Error", message)?);
            }
            if val.is_undef() && opline._pad & FETCH_OBJ_INCDEC != 0 {
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!("Undefined property: {class_name}::${name}"),
                    opline._pad & FETCH_OBJ_ERROR_SUPPRESS != 0,
                )?;
                if let Some(result) = take_magic_exception(eg, frame)? {
                    return Ok(result);
                }
                set_result(current_incdec_value());
                return Ok(ColdResult::Done);
            }
            set_result(val);
        } else {
            // An intermediate property in `isset($object->a->b)` first asks
            // `__isset(a)` and invokes `__get(a)` only when it returns true.
            // A compiler-silent modification fetch is different: the RHS has
            // already committed and PHP must fetch the overloaded l-value via
            // `__get`, without consulting `__isset`.
            if opline._pad & FETCH_OBJ_SILENT != 0 && write_flags != 0 && !has_magic_get {
                // Missing ordinary properties are auto-vivified by the later
                // canonical writeback. The silent l-value fetch must not emit
                // the read-side undefined-property warning (or eagerly create
                // the member), while an overloaded property still needs to
                // reach __get below.
                if opline._pad & FETCH_OBJ_UNSET != 0 {
                    if explicitly_unset_declared {
                        let mut result = Value::undef();
                        result.mark_indirect_property_modification_result();
                        set_result(result);
                        return Ok(ColdResult::Done);
                    }
                    if internal_class_forbids_dynamic_properties(&class_name)
                        || eg
                            .class_table
                            .get(class_name.as_ref())
                            .is_some_and(|class_def| class_def.is_readonly || class_def.is_enum)
                    {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "Error",
                            format!("Cannot create dynamic property {class_name}::${name}"),
                        )?);
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
                        if let Some(result) = take_magic_exception(eg, frame)? {
                            return Ok(result);
                        }
                    }
                    if let Some(mut object) = obj_val.as_object_mut() {
                        object.set_property(&name, Value::null());
                    }
                    let mut result = Value::null();
                    result.mark_indirect_property_modification_result();
                    set_result(result);
                } else {
                    set_result(Value::null());
                }
                return Ok(ColdResult::Done);
            }
            if opline._pad & FETCH_OBJ_SILENT != 0 && write_flags == 0 {
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
                if let Some(result) = take_magic_exception(eg, frame)? {
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
                if !has_magic_get {
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
                )?);
            }
            let magic_value = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_GET,
                "__get",
                &[Value::string(name.clone())],
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if let Some(mut result) = magic_value {
                if explicitly_unset_declared
                    && let Some(definition) = typed_property_definition.as_ref()
                {
                    let original = result.dereferenced().clone();
                    let original_type = property_assignment_type_name(&original).to_string();
                    let prepared = match prepare_property_assignment(
                        original,
                        definition,
                        eg,
                        op_array.strict_types,
                        &class_name,
                    ) {
                        Ok(prepared) => prepared,
                        Err(_) => {
                            return Ok(object_property_throw(
                                eg,
                                frame,
                                "TypeError",
                                format!(
                                    "Value of type {original_type} returned from {class_name}::__get() must be compatible with unset property {}::${} of type {}",
                                    property_diagnostic_class_name(&definition.declaring_class),
                                    definition.name,
                                    definition.type_hint.property_declaration_display_name(),
                                ),
                            )?);
                        }
                    };
                    if result.is_reference() {
                        result.assign_dereferenced(prepared);
                    } else {
                        result = prepared;
                    }
                }
                if opline._pad & FETCH_OBJ_MODIFY != 0 {
                    if result.is_reference() {
                        result.mark_indirect_property_modification_reference();
                    } else {
                        let indirect_container_write = opline._pad
                            & (FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND)
                            == 0;
                        let identity_preserving_object = matches!(
                            result.dereferenced().value_type(),
                            ValueType::Object | ValueType::Closure
                        );
                        if indirect_container_write
                            && !identity_preserving_object
                        {
                            let class_name = obj_val
                                .as_object()
                                .map(|object| object.class_name.to_string())
                                .unwrap_or_else(|| "object".to_string());
                            report_php_notice(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "Indirect modification of overloaded property {class_name}::${name} has no effect"
                                ),
                            )?;
                            if let Some(result) = take_magic_exception(eg, frame)? {
                                return Ok(result);
                            }
                        }
                        if indirect_container_write {
                            if identity_preserving_object {
                                result.mark_indirect_property_modification_result();
                            } else {
                                result = Value::owned_reference(result);
                                result.mark_indirect_property_modification_result();
                            }
                        }
                    }
                }
                set_result(result);
            } else if opline._pad & FETCH_OBJ_SILENT != 0 {
                // A recursively guarded __get() reached through a successful
                // __isset() remains a silent miss. The outer magic method can
                // then select its parent fallback without an intervening
                // undefined-property warning.
                set_result(Value::null());
            } else if name.starts_with('\0') {
                return Ok(object_property_throw(
                    eg,
                    frame,
                    "Error",
                    "Cannot access property starting with \"\\0\"".into(),
                )?);
            } else {
                let class_name = obj_val
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| "object".to_string());
                let write_flags =
                    opline._pad & (FETCH_OBJ_MODIFY | FETCH_OBJ_INCDEC | FETCH_OBJ_COMPOUND);
                if write_flags != 0 {
                    if internal_class_forbids_dynamic_properties(&class_name) {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "Error",
                            format!("Cannot create dynamic property {class_name}::${name}"),
                        )?);
                    }
                    if eg
                        .class_table
                        .get(class_name.as_str())
                        .is_some_and(|class_def| class_def.is_readonly || class_def.is_enum)
                    {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "Error",
                            format!("Cannot create dynamic property {class_name}::${name}"),
                        )?);
                    }
                    let dynamic_properties_allowed = obj_val.as_object().is_some_and(|object| {
                        object.is_dynamic_std_class()
                            || eg
                                .class_table
                                .get(object.class_name.as_ref())
                                .is_some_and(|class_def| class_def.allow_dynamic_properties)
                    });
                    if !dynamic_properties_allowed && !explicitly_unset_declared {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Creation of dynamic property {class_name}::${name} is deprecated"
                            ),
                        )?;
                        if let Some(result) = take_magic_exception(eg, frame)? {
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
                if let Some(result) = take_magic_exception(eg, frame)? {
                    return Ok(result);
                }
                let result = if opline._pad & FETCH_OBJ_INCDEC != 0 {
                    current_incdec_value()
                } else {
                    Value::null()
                };
                set_result(result);
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
    let receiver_owner =
        property_name_conversion_may_reenter(property).then(|| object.clone());
    let object = receiver_owner.as_ref().unwrap_or(object);
    let name = match convert_object_property_name(
        eg, frame, op_array, opline, property, false,
    )? {
        ConvertedPropertyName::Name(name) => name,
        ConvertedPropertyName::Control(result) => return Ok(result),
    };
    let lazy_receiver_owner = eg.lazy_object_state(object).map(|_| object.clone());
    let object = lazy_receiver_owner.as_ref().unwrap_or(object);
    let caller_class = get_caller_class(frame, eg);
    let object_ref = object.as_object().expect("object tag must expose object storage");
    let receiver_in_scope = caller_class
        .as_ref()
        .is_some_and(|caller| eg.class_is_a(&object_ref.class_name, caller));
    let effective_caller = receiver_in_scope
        .then_some(caller_class.as_deref())
        .flatten();
    let accessible = eg
        .find_property_visibility(&object_ref.class_name, &name)
        .is_none_or(|(visibility, defining_class)| {
            visibility == Visibility::Public
                || eg.check_instance_property_visibility(
                    caller_class.as_deref(),
                    &object_ref.class_name,
                    &name,
                    &defining_class,
                    visibility,
                )
        });
    let hidden_parent_private = eg
        .find_property_visibility(&object_ref.class_name, &name)
        .is_some_and(|(visibility, defining_class)| {
            visibility == Visibility::Private
                && !defining_class.eq_ignore_ascii_case(&object_ref.class_name)
                && !eg.check_visibility(effective_caller, &defining_class, visibility)
        });
    let write_only_property = if accessible && !hidden_parent_private {
        let key = crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            &name,
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
        name.clone()
    } else {
        crate::runtime::resolve_property_key(
            eg,
            &object_ref.class_name,
            &name,
            effective_caller,
        )
    };
    let get_hook_declaring_class = (accessible && !hidden_parent_private)
        .then_some(())
        .and_then(|()| {
            object_ref
            .property_slot(&key)
            .and_then(|slot| eg.instance_property_definition(object_ref.class_id, slot))
            .filter(|definition| definition.has_get_hook)
            .map(|definition| definition.declaring_class.clone())
        });
    let has_get_hook = get_hook_declaring_class.is_some();
    let declared_property = object_ref.property_slot(&key).is_some();
    let explicitly_unset_declared = declared_property
        && object_ref
            .get_property(&key)
            .is_some_and(Value::is_explicitly_unset_property);
    drop(object_ref);
    let initialized_target = if accessible
        && !hidden_parent_private
        && declared_property
        && !explicitly_unset_declared
        && eg.lazy_property_requires_initialization(object, &key)
    {
        Some(crate::stdlib::reflection::initialize_lazy_object(
            eg, object,
        )?)
    } else {
        eg.lazy_proxy_instance(object)
    };
    if let Some(result) = take_magic_exception(eg, frame)? {
        return Ok(result);
    }
    let magic_receiver = object;
    let object = initialized_target.as_ref().unwrap_or(object);
    if has_get_hook
        && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_HOOK_GET)
    {
        let hook_name = format!("${name}::get");
        let value = call_guarded_property_hook_method(
            eg,
            magic_receiver,
            &name,
            PROPERTY_GUARD_HOOK_GET,
            get_hook_declaring_class
                .as_deref()
                .expect("get hook must retain its declaring class"),
            &hook_name,
            &[],
        )?;
        if let Some(result) = take_magic_exception(eg, frame)? {
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
    let property_state = if explicitly_unset_declared {
        None
    } else if hidden_parent_private {
        object_ref
            .get_dynamic_property_with_position(&name)
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
        && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET)
    {
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!("Property {object_class_name}::${name} is write-only"),
        )?);
    }

    let set = match property_state {
        Some(set) => set,
        None => call_guarded_property_magic_method(
            eg,
            magic_receiver,
            &name,
            PROPERTY_GUARD_ISSET,
            "__isset",
            &[Value::string(&name)],
        )?
        .is_some_and(|value| value.is_truthy()),
    };
    if let Some(result) = take_magic_exception(eg, frame)? {
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
    let receiver_owner =
        property_name_conversion_may_reenter(property).then(|| object.clone());
    let object = receiver_owner.as_ref().unwrap_or(object);
    let name = match convert_object_property_name(
        eg, frame, op_array, opline, property, false,
    )? {
        ConvertedPropertyName::Name(name) => name,
        ConvertedPropertyName::Control(result) => return Ok(result),
    };
    let lazy_receiver_owner = eg.lazy_object_state(object).map(|_| object.clone());
    let object = lazy_receiver_owner.as_ref().unwrap_or(object);
    let caller_class = get_caller_class(frame, eg);
    let object_ref = object.as_object().unwrap();
    let receiver_in_scope = caller_class
        .as_ref()
        .is_some_and(|caller| eg.class_is_a(&object_ref.class_name, caller));
    let effective_caller = receiver_in_scope
        .then_some(caller_class.as_deref())
        .flatten();
    let caller_has_own = receiver_in_scope && caller_class.as_ref().is_some_and(|caller| {
        eg.find_property_visibility(caller, &name)
            .is_some_and(|(visibility, defining_class)| {
                visibility == Visibility::Private
                    && defining_class.eq_ignore_ascii_case(caller)
            })
    });
    let accessible = caller_has_own
        || eg
            .find_property_set_visibility(&object_ref.class_name, &name)
            .is_none_or(|(visibility, defining_class)| {
                visibility == Visibility::Public
                    || eg.check_instance_property_visibility(
                        caller_class.as_deref(),
                        &object_ref.class_name,
                        &name,
                        &defining_class,
                        visibility,
                    )
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
    let explicitly_unset_declared = object_ref.property_slot(&key).is_some()
        && object_ref
            .get_property(&key)
            .is_some_and(Value::is_explicitly_unset_property);
    let magic_unset_can_handle = explicitly_unset_declared
        && !property_guard_active(eg, object, &name, PROPERTY_GUARD_UNSET)
        && eg
            .find_function(&format!(
                "{}::__unset",
                object_ref.class_name.to_ascii_lowercase()
            ))
            .is_some();
    if eg
        .class_table
        .get(object_ref.class_name.as_ref())
        .is_some_and(|class| class.is_enum && class.readonly_props.contains(&name))
    {
        let class_name = object_ref.class_name.clone();
        drop(object_ref);
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!("Cannot unset readonly property {class_name}::${name}"),
        )?);
    }
    let readonly_property = eg
        .class_table
        .get(object_ref.class_name.as_ref())
        .is_some_and(|class| class.readonly_props.contains(&name));
    if readonly_property {
        let initialized = object_ref
            .get_property(&key)
            .is_some_and(|value| !value.is_undef());
        let clone_reinitialization =
            initialized && readonly_clone_reinitialization_allowed(eg, object, &name);
        if (initialized && !clone_reinitialization)
            || (!receiver_in_scope && !magic_unset_can_handle)
        {
            let defining_class = eg
                .find_property_visibility(&object_ref.class_name, &name)
                .map(|(_, defining_class)| defining_class)
                .unwrap_or_else(|| object_ref.class_name.to_string());
            let message = if initialized {
                format!("Cannot unset readonly property {defining_class}::${name}")
            } else {
                format!(
                    "Cannot unset protected(set) readonly property {defining_class}::${name} from global scope"
                )
            };
            drop(object_ref);
            return Ok(object_property_throw(eg, frame, "Error", message)?);
        }
    }
    if !accessible
        && eg.property_has_asymmetric_set_visibility(&object_ref.class_name, &name)
        && !magic_unset_can_handle
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
        return Ok(object_property_throw(eg, frame, "Error", message)?);
    }
    if !accessible
        && !hidden_parent_private
        && property_guard_active(eg, object, &name, PROPERTY_GUARD_UNSET)
        && let Some((visibility, defining_class)) =
            eg.find_property_visibility(&object_ref.class_name, &name)
    {
        let visibility = match visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => "public",
        };
        let message = format!(
            "Cannot access {visibility} property {defining_class}::${name}"
        );
        drop(object_ref);
        return Ok(object_property_throw(eg, frame, "Error", message)?);
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
        )?);
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
    let magic_unset_can_handle = (!lazy_declared_property || explicitly_unset_declared)
        && !lazy_dynamic_property
        && !property_guard_active(eg, object, &name, PROPERTY_GUARD_UNSET)
        && eg
            .find_function(&format!(
                "{}::__unset",
                lazy_class_name.to_ascii_lowercase()
            ))
            .is_some();
    let lazy_undefined = (lazy_declared_undefined
        && !explicitly_unset_declared
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
    if let Some(result) = take_magic_exception(eg, frame)? {
        return Ok(result);
    }
    let magic_receiver = object;
    let object = initialized_target.as_ref().unwrap_or(object);
    let object_ref = object
        .as_object()
        .expect("lazy initialization must preserve an object receiver");
    let explicitly_unset_declared = object_ref.property_slot(&key).is_some()
        && object_ref
            .get_property(&key)
            .is_some_and(Value::is_explicitly_unset_property);
    let removed = if hidden_parent_private {
        object_ref.get_dynamic_property_with_position(&key).is_some()
    } else {
        accessible
            && object_ref.contains_property(&key)
            && !explicitly_unset_declared
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
    if name.starts_with('\0')
        && property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_UNSET)
    {
        let instruction_index = (opline as *const Instruction as usize
            - op_array.instructions.as_ptr() as usize)
            / std::mem::size_of::<Instruction>();
        return Ok(object_property_throw_at(
            eg,
            frame,
            op_array,
            instruction_index,
            "Error",
            "Cannot access property starting with \"\\0\"".into(),
        )?);
    }
    let _ = call_guarded_property_magic_method(
        eg,
        magic_receiver,
        &name,
        PROPERTY_GUARD_UNSET,
        "__unset",
        &[Value::string(&name)],
    )?;
    if let Some(result) = take_magic_exception(eg, frame)? {
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
        let internal_result = opline._pad
            & (REFERENCE_RESULT_INTERNAL | OBJ_PROP_FUNC_ARG)
            != 0;
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
        let name = match convert_object_property_name(
            eg,
            frame,
            op_array,
            opline,
            name_value,
            false,
        )? {
            ConvertedPropertyName::Name(name) => name,
            ConvertedPropertyName::Control(result) => return Ok(result),
        };
        if opline._pad & OBJ_PROP_TEMPORARY_RECEIVER != 0 {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                "Cannot use temporary expression in write context".to_string(),
            )?);
        }
        let Some(object) = receiver.as_object() else {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!(
                    "Attempt to modify property \"{name}\" on {}",
                    property_write_receiver_type(receiver.dereferenced())
                ),
            )?);
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
        let caller_has_own = receiver_in_scope && caller_class.as_ref().is_some_and(|caller| {
            eg.find_property_visibility(caller, &name)
                .is_some_and(|(visibility, defining_class)| {
                    visibility == Visibility::Private
                        && defining_class.eq_ignore_ascii_case(caller)
                })
        });
        let mut force_dynamic = false;
        let mut property_accessible = true;
        if let Some((visibility, defining_class)) =
            eg.find_property_set_visibility(&class_name, &name)
            && visibility != Visibility::Public
            && !caller_has_own
            && !eg.check_instance_property_visibility(
                caller_class.as_deref(),
                &class_name,
                &name,
                &defining_class,
                visibility,
            )
        {
            if visibility == Visibility::Private
                && !eg.property_has_asymmetric_set_visibility(&class_name, &name)
                && !defining_class.eq_ignore_ascii_case(&class_name)
            {
                force_dynamic = true;
            } else {
                let has_magic_get = eg
                    .find_function(&format!("{}::__get", class_name.to_ascii_lowercase()))
                    .is_some();
                if !eg.property_has_asymmetric_set_visibility(&class_name, &name)
                    && has_magic_get
                    && !property_guard_active(eg, &receiver, &name, PROPERTY_GUARD_GET)
                {
                    property_accessible = false;
                } else {
                    if opline._pad & OBJ_PROP_REFERENCE_BIND == 0
                        && let Some(mut binding) =
                            detached_restricted_object_property_reference(
                                eg,
                                &receiver,
                                &name,
                                caller_class.as_deref(),
                            )
                    {
                        if internal_result {
                            binding.mark_internal_reference_alias();
                        }
                        let destination =
                            (*frame).cv_mut(opline.result as u32) as *mut Value;
                        frame_slot_set(frame, destination, binding);
                        return Ok(ColdResult::Done);
                    }
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
                    )?);
                }
            }
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
        if eg
            .find_class(&class_name)
            .is_some_and(|class| class.readonly_props.contains(&name))
        {
            if opline._pad & OBJ_PROP_REFERENCE_BIND == 0
                && let Some(mut binding) = detached_restricted_object_property_reference(
                    eg,
                    &receiver,
                    &name,
                    caller_class.as_deref(),
                )
            {
                if internal_result {
                    binding.mark_internal_reference_alias();
                }
                let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
                frame_slot_set(frame, destination, binding);
                return Ok(ColdResult::Done);
            }
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                format!("Cannot indirectly modify readonly property {class_name}::${name}"),
            )?);
        }
        let (lazy_declared_property, lazy_dynamic_property) = receiver
            .as_object()
            .map(|object| {
                (
                    property_accessible && object.property_slot(&key).is_some(),
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
        if let Some(result) = take_magic_exception(eg, frame)? {
            return Ok(result);
        }
        let (declared_slot, definition, owner) = {
            let object = receiver.as_object().unwrap();
            let slot = (!force_dynamic && property_accessible)
                .then(|| object.property_slot(&key))
                .flatten();
            let definition = slot
                .and_then(|slot| eg.instance_property_definition(object.class_id, slot))
                .map(|definition| definition as *const crate::compiler::compile::PropertyDefinition);
            let owner = slot.map(|slot| object.instance_property_reference_owner(slot));
            (slot, definition, owner)
        };
        let definition = definition.map(|definition| &*definition);
        let explicitly_unset_declared = declared_slot.is_some_and(|slot| {
            receiver
                .as_object()
                .is_some_and(|object| {
                    object
                        .get_property_slot(slot)
                        .is_some_and(Value::is_explicitly_unset_property)
                })
        });

        if let Some(definition) = definition
            && definition.has_get_hook
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && !property_guard_active(eg, &receiver, &name, PROPERTY_GUARD_HOOK_GET)
        {
            let hook_name = format!("${name}::get");
            let returned = call_guarded_property_hook_method(
                eg,
                &receiver,
                &name,
                PROPERTY_GUARD_HOOK_GET,
                &definition.declaring_class,
                &hook_name,
                &[],
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if opline._pad & OBJ_PROP_REFERENCE_BIND != 0 {
                return Ok(object_property_throw_at(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Error",
                    "Cannot assign by reference to overloaded object".to_string(),
                )?);
            }
            if let Some(returned) = returned {
                let mut binding = if returned.is_owned_reference() {
                    returned.clone_owned_reference_alias()
                } else if returned.is_reference() {
                    Value::reference(returned.as_ref_ptr())
                } else {
                    let message = if internal_result {
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
                    )?);
                };
                if internal_result {
                    binding.mark_internal_reference_alias();
                }
                let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
                frame_slot_set(frame, destination, binding);
                return Ok(ColdResult::Done);
            }
        }

        if let Some(definition) = definition
            && !definition.has_get_hook
            && definition.has_set_hook
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && let Some(mut returned) = receiver
                .as_object()
                .and_then(|object| object.get_property_slot(declared_slot?).cloned())
        {
            if !matches!(
                returned.dereferenced().value_type(),
                ValueType::Object | ValueType::Closure
            ) {
                return Ok(object_property_throw_at(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Error",
                    format!("Indirect modification of {class_name}::${name} is not allowed"),
                )?);
            }
            if !returned.is_reference() {
                returned = Value::owned_reference(returned);
            }
            if internal_result {
                returned.mark_internal_reference_alias();
            }
            let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
            frame_slot_set(frame, destination, returned);
            return Ok(ColdResult::Done);
        }

        let missing_property = explicitly_unset_declared
            || declared_slot.is_none()
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
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            let mut binding = Value::owned_reference(Value::null());
            if internal_result {
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
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if let Some(mut returned) = returned {
                if explicitly_unset_declared
                    && let Some(definition) = definition.filter(|definition| definition.is_typed())
                {
                    let original = returned.dereferenced().clone();
                    let original_type = property_assignment_type_name(&original).to_string();
                    let prepared = match prepare_property_assignment(
                        original,
                        definition,
                        eg,
                        op_array.strict_types,
                        &class_name,
                    ) {
                        Ok(prepared) => prepared,
                        Err(_) => {
                            return Ok(object_property_throw_at(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                "TypeError",
                                format!(
                                    "Value of type {original_type} returned from {class_name}::__get() must be compatible with unset property {}::${} of type {}",
                                    property_diagnostic_class_name(&definition.declaring_class),
                                    definition.name,
                                    definition.type_hint.property_declaration_display_name(),
                                ),
                            )?);
                        }
                    };
                    if returned.is_reference() {
                        returned.assign_dereferenced(prepared);
                    } else {
                        returned = prepared;
                    }
                }
                if opline._pad & OBJ_PROP_REFERENCE_BIND != 0 {
                    if !returned.is_reference()
                        && !matches!(
                            returned.dereferenced().value_type(),
                            ValueType::Object | ValueType::Closure
                        )
                    {
                        report_php_notice(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Indirect modification of overloaded property {class_name}::${name} has no effect"
                            ),
                        )?;
                        // Zend's target-side reference error takes precedence
                        // even when a user notice handler throws. The handler
                        // side effects remain visible, but its pending
                        // Throwable is replaced by the assignment Error below.
                        let _ = eg.exception.take();
                    }
                    return Ok(object_property_throw_at(
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                        "Error",
                        "Cannot assign by reference to overloaded object".to_string(),
                    )?);
                }
                if !returned.is_reference()
                    && !matches!(
                        returned.dereferenced().value_type(),
                        ValueType::Object | ValueType::Closure
                    )
                {
                    report_php_notice(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &format!(
                            "Indirect modification of overloaded property {class_name}::${name} has no effect"
                        ),
                    )?;
                    if let Some(result) = take_magic_exception(eg, frame)? {
                        return Ok(result);
                    }
                }
                let mut binding = if returned.is_owned_reference() {
                    returned.clone_owned_reference_alias()
                } else if returned.is_reference() {
                    Value::reference(returned.as_ref_ptr())
                } else {
                    Value::owned_reference(returned)
                };
                if internal_result {
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
                    property_diagnostic_class_name(&definition.declaring_class),
                    definition.name
                ),
            )?);
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
            && (internal_class_forbids_dynamic_properties(&class_name)
                || eg
                    .class_table
                    .get(class_name.as_str())
                    .is_some_and(|class_def| class_def.is_readonly || class_def.is_enum))
        {
            return Ok(object_property_throw_at(
                eg,
                frame,
                op_array,
                instruction_index,
                "Error",
                format!("Cannot create dynamic property {class_name}::${name}"),
            )?);
        }
        if creates_dynamic_property
            && !dynamic_properties_allowed
            && !has_magic_get
            && !internal_result
        {
            report_php_deprecation(
                eg,
                frame,
                op_array,
                opline,
                &format!("Creation of dynamic property {class_name}::${name} is deprecated"),
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
        }

        if opline._pad & OBJ_PROP_REFERENCE_BIND != 0 {
            let source = if opline.result_type == OpType::Cv {
                (*frame).cv_mut(opline.result as u32) as *mut Value
            } else {
                (*frame).get_op_mut(opline.result as u32, opline.result_type)
            };
            if opline.result_type != OpType::Cv && !(&*source).is_reference() {
                report_php_notice(
                    eg,
                    frame,
                    op_array,
                    opline,
                    "Only variables should be assigned by reference",
                )?;
                if let Some(result) = take_magic_exception(eg, frame)? {
                    return Ok(result);
                }
            }
            let binding = materialize_reference_alias(frame, source);
            if internal_result
                && (&*source).is_owned_reference()
            {
                (&mut *source).mark_internal_reference_alias();
            }
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
                        )?);
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
            if let Some(result) = take_magic_exception(eg, frame)? {
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

        if internal_result {
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
    let internal_result = opline._pad & REFERENCE_RESULT_INTERNAL != 0
        || opline._pad & FETCH_DIM_FUNC_ARG != 0;
    // SAFETY: the compiler emits mutable array/CV operands owned by this live
    // frame. Promoting the element to an Rc-backed cell makes both aliases
    // independent of subsequent array storage reallocations.
    unsafe {
        let index = &*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        );
        let array_ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
        let raw_type = (*array_ptr).dereferenced().value_type();
        if raw_type == ValueType::String {
            let (class_name, message) = if matches!(string_offset_key(index), StringOffsetKey::Invalid)
            {
                (
                    "TypeError",
                    format!(
                        "Cannot access offset of type {} on string",
                        index.diagnostic_type_name()
                    ),
                )
            } else {
                (
                    "Error",
                    "Cannot create references to/from string offsets".to_string(),
                )
            };
            let error = make_error_value(class_name, &message);
            let instruction_index = (opline as *const Instruction)
                .offset_from(op_array.instructions.as_ptr()) as usize;
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
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
                )? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            };
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
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
                if opline._pad & FETCH_DIM_FUNC_ARG == 0 {
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
                }
                Value::owned_reference(returned)
            };
            if internal_result {
                binding.mark_internal_reference_alias();
            }
            let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
            frame_slot_set(frame, destination, binding);
            return Ok(ColdResult::Done);
        }
        let key = match value_to_array_key(index) {
            Ok(key) => key,
            Err(ArrayKeyError::Illegal) => {
                let instruction_index = (opline as *const Instruction)
                    .offset_from(op_array.instructions.as_ptr())
                    as usize;
                return Ok(match throw_illegal_offset_type(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    &format!(
                        "Cannot access offset of type {} on array",
                        index.diagnostic_type_name()
                    ),
                )? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
            Err(error) => {
                let key = match error {
                    ArrayKeyError::DeprecatedNull => ArrayKey::String(String::new()),
                    ArrayKeyError::DeprecatedFloat(integer) | ArrayKeyError::Resource(integer) => {
                        ArrayKey::Int(integer)
                    }
                    ArrayKeyError::NonRepresentableFloat { integer, .. } => {
                        ArrayKey::Int(integer)
                    }
                    ArrayKeyError::Illegal => unreachable!(),
                };
                match error {
                    ArrayKeyError::Resource(resource) => report_php_warning(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &format!(
                            "Resource ID#{resource} used as offset, casting to integer ({resource})"
                        ),
                        false,
                    )?,
                    ArrayKeyError::DeprecatedNull => report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        "Using null as an array offset is deprecated, use an empty string instead",
                    )?,
                    ArrayKeyError::DeprecatedFloat(_) => report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            index.echo_to_string_with_precision(-1)
                        ),
                    )?,
                    ArrayKeyError::NonRepresentableFloat {
                        also_deprecated,
                        ..
                    } => {
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "The float {} is not representable as an int, cast occurred",
                                index.echo_to_string_with_precision(-1)
                            ),
                            false,
                        )?;
                        if eg.exception.is_none() && also_deprecated {
                            report_php_deprecation(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "Implicit conversion from float {} to int loses precision",
                                    index.echo_to_string_with_precision(-1)
                                ),
                            )?;
                        }
                    }
                    ArrayKeyError::Illegal => unreachable!(),
                }
                if let Some(exception) = eg.exception.take() {
                    return Ok(match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                    });
                }
                key
            }
        };
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
        let Some(array) = mutable_source.as_array_mut() else {
            let instruction_index = (opline as *const Instruction)
                .offset_from(op_array.instructions.as_ptr())
                as usize;
            return Ok(match throw_array_dimension_error(
                eg,
                frame,
                op_array,
                instruction_index,
                "Cannot use a scalar value as an array",
            )? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        };
        let key = array.prepare_string_key_for_write(key, index);
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
        if internal_result {
            binding.mark_internal_reference_alias();
        }
        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
        frame_slot_set(frame, destination, binding);
    }
    Ok(ColdResult::Done)
}

#[cold]
#[inline(never)]
fn op_assign_obj_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let suppressed = opline._pad & ASSIGN_OBJ_ERROR_SUPPRESS != 0;
    if suppressed {
        eg.begin_error_suppression(frame as usize);
    }
    let result = op_assign_obj_prop_inner(eg, frame, op_array, opline);
    if suppressed {
        eg.end_error_suppression(frame as usize);
    }
    result
}

fn op_assign_obj_prop_inner<'a>(
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
        let source = &*(*frame).get_op_ptr(
            opline.result as u32,
            opline.result_type,
            op_array,
        );
        // A by-reference property or magic getter already exposed the storage
        // mutated by the preceding read-modify operation. Its synthetic
        // property writeback must not invoke a setter or re-check write
        // visibility. Consume a compiler temporary now so it does not remain
        // observable as an additional PHP reference alias until frame exit.
        if opline._pad & ASSIGN_OBJ_MODIFY != 0
            && source.is_indirect_property_modification_result()
        {
            if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                let source = (*frame).get_op_mut(opline.result as u32, opline.result_type);
                frame_slot_set(frame, source, Value::undef());
            }
            return Ok(ColdResult::Done);
        }
        let val = if source.is_reference() {
            &*source.as_ref_ptr()
        } else {
            source
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
    let receiver_owner = property_name_conversion_may_reenter(prop_name).then(|| obj.clone());
    let obj = receiver_owner.as_ref().unwrap_or(obj);
    let name = match convert_object_property_name(
        eg, frame, op_array, opline, prop_name, false,
    )? {
        ConvertedPropertyName::Name(name) => name,
        ConvertedPropertyName::Control(result) => return Ok(result),
    };
    if obj.value_type() == ValueType::Closure {
        return Ok(object_property_throw(
            eg,
            frame,
            "Error",
            format!("Cannot create dynamic property Closure::${name}"),
        )?);
    }
    let lazy_receiver_owner = eg.lazy_object_state(obj).map(|_| obj.clone());
    let obj = lazy_receiver_owner.as_ref().unwrap_or(obj);

    if let Some(missing_class) = obj
        .as_object()
        .and_then(|object| object.incomplete_class_name())
    {
        let instruction_index = (opline as *const Instruction as usize
            - op_array.instructions.as_ptr() as usize)
            / std::mem::size_of::<Instruction>();
        return Ok(object_property_throw_at(
            eg,
            frame,
            op_array,
            instruction_index,
            "Error",
            format!(
                "The script tried to modify a property on an incomplete object. Please ensure that the class definition \"{missing_class}\" of the object you are trying to operate on was loaded _before_ unserialize() gets called or provide an autoloader to load the class definition"
            ),
        )?);
    }

    let setter_guarded = property_guard_active(eg, obj, &name, PROPERTY_GUARD_SET);
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
        let mut property_defining_class = None;
        if let Some((vis, defining_class)) = eg.find_property_set_visibility(&php_obj.class_name, &name) {
            property_defining_class = Some(defining_class.clone());
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
                    if !eg.check_instance_property_visibility(
                        caller_class.as_deref(),
                        &php_obj.class_name,
                        &name,
                        &defining_class,
                        vis,
                    ) {
                        let storage_key = crate::runtime::resolve_property_key(
                            eg,
                            &php_obj.class_name,
                            &name,
                            Some(&defining_class),
                        );
                        let readonly_state = php_obj
                            .property_slot(&storage_key)
                            .and_then(|slot| {
                                eg.instance_property_definition(php_obj.class_id, slot)
                                    .map(|definition| {
                                        (
                                            definition.is_readonly,
                                            php_obj
                                                .get_property_slot(slot)
                                                .is_some_and(|value| !value.is_undef()),
                                        )
                                    })
                            })
                            .unwrap_or((false, false));
                        if readonly_state == (true, true) {
                            let action = if opline._pad & ASSIGN_OBJ_MODIFY != 0
                                && assigned.value_type() == ValueType::Array
                            {
                                "indirectly modify"
                            } else {
                                "modify"
                            };
                            let message = format!(
                                "Cannot {action} readonly property {defining_class}::${name}"
                            );
                            drop(php_obj);
                            return Ok(object_property_throw(eg, frame, "Error", message)?);
                        }
                        let has_setter = eg
                            .find_function(&format!(
                                "{}::__set",
                                php_obj.class_name.to_ascii_lowercase()
                            ))
                            .is_some();
                        let asymmetric = eg.property_has_asymmetric_set_visibility(
                            &php_obj.class_name,
                            &name,
                        );
                        if readonly_state == (true, false)
                            && asymmetric
                            && vis == Visibility::Protected
                        {
                            let message = format!(
                                "Cannot modify protected(set) readonly property {defining_class}::${name} from {}",
                                caller_class.as_deref().map_or_else(
                                    || "global scope".to_string(),
                                    |scope| format!("scope {scope}"),
                                ),
                            );
                            drop(php_obj);
                            return Ok(object_property_throw(eg, frame, "Error", message)?);
                        }
                        let explicitly_unset_declared = php_obj
                            .get_property(&storage_key)
                            .is_some_and(Value::is_explicitly_unset_property);
                        if has_setter
                            && !setter_guarded
                            && (!asymmetric || explicitly_unset_declared)
                        {
                            prop_is_public = false;
                            property_accessible = false;
                        } else {
                            let vis_str = match vis {
                                Visibility::Protected => "protected",
                                Visibility::Private => "private",
                                _ => "public",
                            };
                            let message = if asymmetric {
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
                            let instruction_index = (opline as *const Instruction as usize
                                - op_array.instructions.as_ptr() as usize)
                                / std::mem::size_of::<Instruction>();
                            return Ok(object_property_throw_at(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                "Error",
                                message,
                            )?);
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
        let lazy_set_hook_declaring_class = property_accessible
            .then(|| php_obj.property_slot(&lazy_key))
            .flatten()
            .and_then(|slot| eg.instance_property_definition(php_obj.class_id, slot))
            .filter(|definition| definition.has_set_hook)
            .map(|definition| definition.declaring_class.clone());
        let lazy_declared_explicitly_unset = lazy_declared_property
            && php_obj
                .get_property(&lazy_key)
                .is_some_and(Value::is_explicitly_unset_property);
        let lazy_dynamic_property = php_obj
            .get_dynamic_property_with_position(&lazy_key)
            .is_some();
        let lazy_class_name = php_obj.class_name.clone();
        drop(php_obj);
        let magic_set_can_handle = (!lazy_declared_property
            || lazy_declared_explicitly_unset)
            && !lazy_dynamic_property
            && !property_guard_active(eg, obj, &name, PROPERTY_GUARD_SET)
            && eg
                .find_function(&format!(
                    "{}::__set",
                    lazy_class_name.to_ascii_lowercase()
                ))
                .is_some();
        if let Some(declaring_class) = lazy_set_hook_declaring_class.as_deref()
            && opline._pad & crate::vm::instruction::OBJ_PROP_HOOK_BYPASS == 0
            && !property_guard_active(eg, obj, &name, PROPERTY_GUARD_HOOK_SET)
            && eg.lazy_property_requires_initialization(obj, &lazy_key)
        {
            let hook_name = format!("${name}::set");
            let hook_value = call_guarded_property_hook_method(
                eg,
                obj,
                &name,
                PROPERTY_GUARD_HOOK_SET,
                declaring_class,
                &hook_name,
                std::slice::from_ref(&assigned),
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if hook_value.is_some() {
                return Ok(ColdResult::Done);
            }
        }
        let must_initialize = (property_accessible || force_dynamic)
            && !lazy_dynamic_property
            && !lazy_declared_explicitly_unset
            && !magic_set_can_handle
            && eg.lazy_property_requires_initialization(obj, &lazy_key);
        let initialized_target = if must_initialize {
            Some(crate::stdlib::reflection::initialize_lazy_object(
                eg, obj,
            )?)
        } else {
            eg.lazy_proxy_instance(obj)
        };
        if let Some(result) = take_magic_exception(eg, frame)? {
            return Ok(result);
        }
        let magic_receiver = obj;
        let obj = initialized_target.as_ref().unwrap_or(obj);
        let explicitly_unset_declared = !force_dynamic
            && obj.as_object().is_some_and(|object| {
                object.property_slot(&lazy_key).is_some()
                    && object
                        .get_property(&lazy_key)
                        .is_some_and(Value::is_explicitly_unset_property)
            });
        if explicitly_unset_declared
            && !setter_guarded
            && eg
                .find_function(&format!(
                    "{}::__set",
                    lazy_class_name.to_ascii_lowercase()
                ))
                .is_some()
        {
            let magic = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_SET,
                "__set",
                &[Value::string(name.clone()), assigned.clone()],
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if magic.is_some() {
                return Ok(ColdResult::Done);
            }
        }
        let php_obj = obj
            .as_object_mut()
            .expect("lazy initialization must preserve an object receiver");
        // Enum guard: enum cases are sealed — no property writes allowed
        // Track writability for cache population — enum/readonly are not cacheable for writes.
        let mut prop_is_writable = true;
        // A declared property explicitly removed by unset() is overloaded by
        // __set on its next assignment. Keep that pay-for-use class family on
        // the canonical handler instead of charging every ordinary cached
        // property write for a per-instance unset-state guard.
        if eg
            .find_function(&format!(
                "{}::__set",
                php_obj.class_name.to_ascii_lowercase()
            ))
            .is_some()
        {
            prop_is_writable = false;
        }
        if let Some(class_def) = eg.class_table.get(php_obj.class_name.as_ref()) {
            if class_def.is_enum {
                let message = if class_def.readonly_props.contains(&name) {
                    format!(
                        "Cannot modify readonly property {}::${}",
                        object_display_class_name, name
                    )
                } else {
                    format!(
                        "Cannot create dynamic property {}::${}",
                        object_display_class_name, name
                    )
                };
                let err = make_error_value("Error", &message);
                drop(php_obj);
                match throw_in_frame(eg, frame, err)? {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
        // Readonly property check
        let mut consume_clone_reinitialization = false;
        let readonly_display_class = property_defining_class.as_deref().map_or(
            object_display_class_name.as_ref(),
            |class| {
                class
                    .strip_prefix("class@anonymous#")
                    .map_or(class, |_| "class@anonymous")
            },
        );
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
                            readonly_display_class, name
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err)? {
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
                            readonly_display_class, name
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err)? {
                            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                        }
                    }
                } else {
                    // PHP 8.4+ readonly writes are protected(set): first
                    // initialization is available to the receiver's class
                    // family, including a parent constructor on a child object.
                    let publicly_initializable = eg
                        .find_property_set_visibility(&php_obj.class_name, &name)
                        .is_some_and(|(visibility, _)| visibility == Visibility::Public)
                        && eg.property_has_asymmetric_set_visibility(
                            &php_obj.class_name,
                            &name,
                        );
                    if !receiver_in_scope && !publicly_initializable {
                        let err = make_error_value("Error", &format!(
                            "Cannot modify protected(set) readonly property {}::${} from {}",
                            readonly_display_class, name,
                            caller_class.as_deref().map_or("global scope".to_string(), |c| format!("scope {}", c))
                        ));
                        drop(php_obj);
                        match throw_in_frame(eg, frame, err)? {
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
        let set_hook_declaring_class = definition
            .filter(|definition| definition.has_set_hook)
            .map(|definition| definition.declaring_class.clone());
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
            && !property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_HOOK_SET)
        {
            let hook_name = format!("${name}::set");
            let hook_value = call_guarded_property_hook_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_HOOK_SET,
                set_hook_declaring_class
                    .as_deref()
                    .expect("set hook must retain its declaring class"),
                &hook_name,
                std::slice::from_ref(&assigned),
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
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
            )?);
        }
        if !prop_exists && internal_class_forbids_dynamic_properties(&object_class_name) {
            return Ok(object_property_throw(
                eg,
                frame,
                "Error",
                format!("Cannot create dynamic property {object_display_class_name}::${name}"),
            )?);
        }
        // A setter may execute arbitrary user code. Reacquire the stable
        // class-table definition after that call; inline caches must never
        // retain pointers to task-local snapshots.
        let definition = declared_slot
            .and_then(|slot| eg.instance_property_definition(object_class_id, slot));
        if let Some(definition_ref) = definition {
            if opline._pad & ASSIGN_OBJ_MODIFY != 0
                && assigned.value_type() == ValueType::Array
                && let Some((stored_type, constraints)) = obj
                    .as_object()
                    .and_then(|object| {
                        object.get_property(&key).map(|stored| {
                            (
                                stored.dereferenced().value_type(),
                                stored.reference_property_constraints(),
                            )
                        })
                    })
                && matches!(stored_type, ValueType::Null | ValueType::Undef)
            {
                let message = reference_array_auto_init_error(&constraints, eg).or_else(|| {
                    property_array_auto_init_error(
                        definition_ref,
                        eg,
                        &object_class_name,
                    )
                });
                if let Some(message) = message {
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "TypeError",
                        message,
                    )?);
                }
            }
            if !definition_ref.has_get_hook
                && !definition_ref.has_set_hook
                && let Some(overflow) =
                    PropertyIncDecOverflow::from_assignment_flags(opline._pad)
            {
                let message = obj.as_object().and_then(|object| {
                    object.get_property(&key).and_then(|stored| {
                        property_incdec_overflow_message(
                            stored,
                            definition_ref,
                            eg,
                            &object_class_name,
                            overflow,
                        )
                    })
                });
                if let Some(message) = message {
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "TypeError",
                        message,
                    )?);
                }
            }
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
                )?);
            }
            if definition_ref.is_typed() && definition_ref.generic_declaration.is_none() {
                let definition = (*definition_ref).clone();
                let diagnostic_source = assigned.dereferenced().clone();
                let prepared = prepare_property_assignment_with_stringable(
                    assigned,
                    &definition,
                    eg,
                    op_array.strict_types,
                    &object_class_name,
                    obj as *const Value,
                )?;
                if let Some(result) = take_magic_exception(eg, frame)? {
                    return Ok(result);
                }
                let (prepared, diagnostic) = match prepared {
                    Ok(prepared) => prepared,
                    Err(message) => {
                        return Ok(object_property_throw(
                            eg,
                            frame,
                            "TypeError",
                            message,
                        )?);
                    }
                };
                if let Some(diagnostic) = diagnostic {
                    report_scalar_coercion_diagnostic(
                        eg,
                        frame,
                        op_array,
                        opline,
                        &diagnostic_source,
                        diagnostic,
                    )?;
                    if let Some(result) = take_magic_exception(eg, frame)? {
                        return Ok(result);
                    }
                }
                assigned = prepared;
            }
        }
        assigned = match prepare_reference_assignment_scalar(
            assigned,
            &property_constraints,
            eg,
            op_array.strict_types,
        ) {
            Ok(value) => value,
            Err(message) => {
                return Ok(object_property_throw(eg, frame, "TypeError", message)?);
            }
        };

        // Cache: if public, not enum, not readonly, key == name → mark for write fast path.
        if prop_is_public && prop_is_writable && key == name && object_class_id != 0 {
            // SAFETY: `opline` belongs to `op_array.instructions`, and the
            // instruction-indexed cache slot remains live for this op array.
            let ic_mut = unsafe {
                let ip = (opline as *const Instruction)
                    .offset_from(op_array.instructions.as_ptr()) as usize;
                &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache)
            };
            if let Some(slot) = declared_slot {
                if let Some(definition) = eg.instance_property_definition(object_class_id, slot)
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
            let assignment_result = (opline._pad & ASSIGN_PROP_RESULT_VALUE != 0)
                .then(|| assigned.clone());
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
                {
                    let property = if force_dynamic {
                        php_obj.get_dynamic_property_mut(&key)
                    } else {
                        php_obj.get_property_mut(&key)
                    }
                        .expect("existing property must remain addressable during assignment");
                    assignment_slot_set(property, assigned);
                }
            }
            if let Some(assignment_result) = assignment_result.as_ref() {
                publish_property_assignment_result(frame, opline, assignment_result);
            }
            run_prepared_value_destructor(eg, destructor)?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
        } else {
            // Property not found — try __set magic method
            let guarded = property_guard_active(eg, magic_receiver, &name, PROPERTY_GUARD_SET);
            if name.starts_with('\0') && guarded {
                let instruction_index = (opline as *const Instruction as usize
                    - op_array.instructions.as_ptr() as usize)
                    / std::mem::size_of::<Instruction>();
                return Ok(object_property_throw_at(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    "Error",
                    "Cannot access property starting with \"\\0\"".into(),
                )?);
            }
            let magic = call_guarded_property_magic_method(
                eg,
                magic_receiver,
                &name,
                PROPERTY_GUARD_SET,
                "__set",
                &[Value::string(name.clone()), assigned.clone()],
            )?;
            if let Some(result) = take_magic_exception(eg, frame)? {
                return Ok(result);
            }
            if guarded || magic.is_none() {
                if name.starts_with('\0') {
                    let instruction_index = (opline as *const Instruction as usize
                        - op_array.instructions.as_ptr() as usize)
                        / std::mem::size_of::<Instruction>();
                    return Ok(object_property_throw_at(
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                        "Error",
                        "Cannot access property starting with \"\\0\"".into(),
                    )?);
                }
                if readonly_class && !magic_get_handles_indirect_writeback {
                    return Ok(object_property_throw(
                        eg,
                        frame,
                        "Error",
                        format!(
                            "Cannot create dynamic property {object_display_class_name}::${name}"
                        ),
                    )?);
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
                    if let Some(result) = take_magic_exception(eg, frame)? {
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
                property_write_receiver_type(obj)
            ),
        )?);
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
            return Ok(match throw_in_frame(eg, frame, error)? {
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
        let (func_ptr, has_generic_contract, magic_method, trait_scope_class_id) = if !ic.func.is_null()
            && ic.class_id == obj_class_id
            && obj_class_id != 0
        {
            drop(obj); // release borrow — class_name not needed on cache hit
            (
                ic.func,
                cfg!(any(feature = "php-generics-erased", feature = "php-generics-reified"))
                    && ic.method_has_generic_contract(),
                None,
                ic.method_trait_scope_class_id(),
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
            let method_info = eg.find_method_info(&dispatch_class, method);
            let inaccessible = method_info.as_ref().is_some_and(
                |(visibility, _, defining_class)| {
                    *visibility != Visibility::Public
                        && !eg.check_instance_method_visibility(
                            caller_class.as_deref(),
                            &target_class_name,
                            method,
                            defining_class,
                            *visibility,
                        )
                },
            );
            let direct = eg.find_function(&full_name);
            let magic = (direct.is_none() || inaccessible)
                .then(|| resolve_magic_call_method(eg, &target_class_name, "__call", false));
            let (resolved, magic_method) = match magic {
                Some(MagicCallMethod::Concrete(magic)) => {
                    (magic, Some(Value::string(method)))
                }
                _ if inaccessible => {
                    let (visibility, _, defining_class) = method_info
                        .as_ref()
                        .expect("inaccessible method retains declaration metadata");
                    let visibility = match visibility {
                        Visibility::Protected => "protected",
                        Visibility::Private => "private",
                        Visibility::Public => unreachable!(),
                    };
                    let scope = caller_class.as_deref().map_or_else(
                        || "global scope".to_string(),
                        |scope| format!("scope {scope}"),
                    );
                    let error = make_error_value(
                        "Error",
                        &format!(
                            "Call to {visibility} method {defining_class}::{method}() from {scope}"
                        ),
                    );
                    attach_throwable_origin(&error, eg, frame, op_array, ip);
                    return Ok(match throw_in_frame(eg, frame, error)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                }
                _ => {
                    let Some(direct) = direct else {
                        let error = make_error_value(
                            "Error",
                            &format!(
                                "Call to undefined method {dispatch_class}::{method}()"
                            ),
                        );
                        return Ok(match throw_in_frame(eg, frame, error)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => {
                                ColdResult::Unhandled(thrown)
                            }
                        });
                    };
                    (direct, None)
                }
            };
            let resolved_has_generic_contract = cfg!(any(
                feature = "php-generics-erased",
                feature = "php-generics-reified"
            ))
                && magic_method.is_none()
                && eg
                    .generic_metadata
                    .has_instance_method_contract(&target_class_name, method);
            let linked_generic_long_contract = cfg!(any(
                feature = "php-generics-erased",
                feature = "php-generics-reified"
            ))
                && magic_method.is_none()
                && eg
                    .generic_metadata
                    .linked_instance_method_contract_admits_exact_long(
                        &target_class_name,
                        method,
                        opline.extended_value,
                    );
            // SAFETY: method resolution returns a request-owned function
            // descriptor that remains live for the duration of execution.
            let common = unsafe { &*resolved };
            let trait_scope_class_id = if common.plan.needs_trait_class_scope() {
                trait_class_scope_for_dispatch(eg, resolved, &dispatch_class)
            } else {
                0
            };

            // Cache the resolution (don't cache if class_id is 0 = unknown)
            if obj_class_id != 0 && magic_method.is_none() {
                let ic_mut = unsafe { &mut *(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache) };
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
                if trait_scope_class_id != 0 {
                    ic_mut.set_method_trait_scope_class_id(trait_scope_class_id);
                }
            }
            (
                resolved,
                resolved_has_generic_contract,
                magic_method,
                trait_scope_class_id,
            )
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
            && !common.plan.needs_trait_class_scope()
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
            if magic_method.is_some() {
                (*call).set_magic_call(true);
            }
            if common.plan.borrow_this() {
                frame_set_borrowed_this(call, obj_val as *const Value);
            } else {
                frame_set_this(call, obj_val.clone());
            }
        }
        initialize_trait_class_scope(eg, call, func_ptr, trait_scope_class_id);
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
        match throw_in_frame(eg, frame, err)? {
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
enum MagicCallMethod {
    Concrete(*const FunctionCommon),
    Abstract,
    Missing,
}

#[cold]
fn find_abstract_method_declaration(
    eg: &ExecutorGlobals,
    class: &str,
    method: &str,
) -> Option<(String, String)> {
    let mut current = eg.find_public_class(class).or_else(|| eg.find_class(class));
    while let Some(definition) = current {
        if let Some((name, _, _, _, _)) = definition.methods.iter().find(|(name, _, _, _, _)| {
            name.eq_ignore_ascii_case(method)
                && (definition.is_interface || definition.method_is_abstract(name))
        }) {
            return Some((definition.name.clone(), name.clone()));
        }
        for trait_name in &definition.uses {
            let Some(trait_definition) = eg.find_class(trait_name) else {
                continue;
            };
            if let Some((name, _, _, _, _)) = trait_definition
                .methods
                .iter()
                .find(|(name, _, _, _, _)| {
                    name.eq_ignore_ascii_case(method)
                        && trait_definition.method_is_abstract(name)
                })
            {
                return Some((definition.name.clone(), name.clone()));
            }
        }
        current = definition
            .parent
            .as_deref()
            .and_then(|parent| eg.find_class(parent));
    }
    None
}

#[cold]
fn resolve_magic_call_method(
    eg: &ExecutorGlobals,
    class: &str,
    magic: &str,
    require_static: bool,
) -> MagicCallMethod {
    // Concrete magic methods declared directly on the target class are the
    // overwhelmingly common trampoline case. Classify that one declaration
    // before walking the hierarchy for an abstract shadow; repeated magic
    // calls intentionally remain uncached, so this must stay one bounded
    // metadata probe rather than two full hierarchy traversals.
    if let Some(definition) = eg.find_public_class(class).or_else(|| eg.find_class(class))
        && let Some((name, visibility, is_static, _, _)) = definition
            .methods
            .iter()
            .find(|(name, _, _, _, _)| name.eq_ignore_ascii_case(magic))
    {
        if definition.is_interface || definition.method_is_abstract(name) {
            return MagicCallMethod::Abstract;
        }
        if *visibility == Visibility::Public
            && *is_static == require_static
            && let Some(function) = eg.find_function(&format!("{}::{magic}", definition.name))
        {
            return MagicCallMethod::Concrete(function);
        }
        return MagicCallMethod::Missing;
    }
    if find_abstract_method_declaration(eg, class, magic).is_some() {
        return MagicCallMethod::Abstract;
    }
    let lookup_class = eg
        .find_public_class(class)
        .or_else(|| eg.find_class(class))
        .map_or(class, |definition| definition.name.as_str());
    if let Some((visibility, is_static, defining)) = eg.find_method_info(lookup_class, magic)
        && visibility == Visibility::Public
        && is_static == require_static
        && let Some(function) = eg.find_function(&format!("{defining}::{magic}"))
    {
        return MagicCallMethod::Concrete(function);
    }
    MagicCallMethod::Missing
}

#[inline(never)]
fn class_callback_requires_instance(
    eg: &ExecutorGlobals,
    class: &str,
    method: &str,
    caller_class: Option<&str>,
) -> bool {
    if let Some((visibility, is_static, defining)) = eg.find_method_info(class, method) {
        return !is_static
            && (visibility == Visibility::Public
                || eg.check_visibility(caller_class, &defining, visibility));
    }
    if eg.find_method_info(class, "__callStatic").is_some() {
        return false;
    }
    eg.find_method_info(class, "__call")
        .is_some_and(|(_, is_static, _)| !is_static)
}

/// Locate the DoFcall paired with a call initializer while stepping over
/// complete nested calls used to evaluate arguments. Static-call init opcodes
/// deliberately carry no duplicate source entry, so their cold errors use the
/// paired call's line and trace origin.
#[cold]
fn call_site_instruction_index(
    op_array: &crate::compiler::OpArray,
    initializer_index: usize,
) -> usize {
    let mut nested_calls = 0usize;
    for (index, instruction) in op_array
        .instructions
        .iter()
        .enumerate()
        .skip(initializer_index.saturating_add(1))
    {
        match instruction.opcode {
            OpCode::InitFcall
            | OpCode::InitUserCall
            | OpCode::InitMethodCall
            | OpCode::InitStaticCall
            | OpCode::InitLateStaticCall
            | OpCode::InitDynamicCall
            | OpCode::InitDynamicStaticCall
            | OpCode::NewObj => nested_calls += 1,
            OpCode::DoFcall if nested_calls == 0 => return index,
            OpCode::DoFcall => nested_calls -= 1,
            _ => {}
        }
    }
    initializer_index
}

// InitStaticCall uses the two low bits of a cached FunctionCommon pointer to
// retain the resolved method's staticness and the PHP 8.5 direct-trait
// deprecation without growing the per-instruction cache. Ordinary static
// method pointers keep both bits clear and retain their one-load hot path.
const STATIC_CALL_NON_STATIC: usize = 1;
const STATIC_CALL_DIRECT_TRAIT: usize = 2;
const STATIC_CALL_TAG_MASK: usize = STATIC_CALL_NON_STATIC | STATIC_CALL_DIRECT_TRAIT;
const _: () = assert!(std::mem::align_of::<FunctionCommon>() >= 4);

fn throw_non_static_callback_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    class: &str,
    method: &str,
) -> Result<ColdResult<'a>, VmError> {
    let error = make_error_value(
        "Error",
        &format!("Non-static method {class}::{method}() cannot be called statically"),
    );
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    Ok(match throw_in_frame(eg, frame, error)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(error) => ColdResult::Unhandled(error),
    })
}

#[cold]
#[inline(never)]
fn throw_located_call_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    message: &str,
) -> Result<ColdResult<'a>, VmError> {
    let error = make_error_value("Error", message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    Ok(match throw_in_frame(eg, frame, error)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(error) => ColdResult::Unhandled(error),
    })
}

enum StaticCallTargetResolution<'a> {
    Resolved(*const FunctionCommon, bool, Option<Value>),
    Flow(ColdResult<'a>),
}

#[cold]
#[inline(never)]
fn resolve_static_call_target<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    class: &str,
    method: &str,
    dynamic_scope: bool,
) -> Result<StaticCallTargetResolution<'a>, VmError> {
    let method_info = eg.find_method_info(class, method);
    let caller_class = if dynamic_scope {
        resolve_static_call_class(eg, frame, "self", true)
    } else {
        get_caller_class(frame, eg)
    };
    let inaccessible = method_info.as_ref().is_some_and(
        |(visibility, _, defining_class)| {
            *visibility != Visibility::Public
                && !eg.check_visibility(caller_class.as_deref(), defining_class, *visibility)
        },
    );
    let direct = eg.find_function(&format!("{class}::{method}"));
    if let Some((defining_class, declared_method)) =
        find_abstract_method_declaration(eg, class, method)
    {
        return Ok(StaticCallTargetResolution::Flow(
            throw_located_call_error(
                eg,
                frame,
                op_array,
                instruction_index,
                &format!("Cannot call abstract method {defining_class}::{declared_method}()"),
            )?,
        ));
    }

    let (resolved, magic_method, instance_magic) = if direct.is_some() && !inaccessible {
        (direct.expect("checked direct static method"), None, false)
    } else {
        // A scoped static spelling retains a compatible live `$this` and
        // therefore prefers __call over __callStatic. Global static syntax has
        // no receiver and continues directly to the static trampoline.
        let live_receiver = get_caller_class(frame, eg).and_then(|_| {
            closure_bound_this(frame, op_array, false).filter(|receiver| {
                receiver
                    .as_object()
                    .is_some_and(|object| eg.class_is_a(&object.class_name, class))
            })
        });
        match live_receiver
            .as_ref()
            .map(|_| resolve_magic_call_method(eg, class, "__call", false))
        {
            Some(MagicCallMethod::Concrete(magic)) => {
                (magic, Some(Value::string(method)), true)
            }
            Some(MagicCallMethod::Abstract) => {
                let diagnostic_class = eg
                    .find_public_class(class)
                    .map_or(class, |definition| definition.name.as_str());
                return Ok(StaticCallTargetResolution::Flow(
                    throw_located_call_error(
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                        &format!("Cannot call abstract method {diagnostic_class}::{method}()"),
                    )?,
                ));
            }
            Some(MagicCallMethod::Missing) | None => {
                if direct.is_none() && method.eq_ignore_ascii_case("__construct") {
                    return Ok(StaticCallTargetResolution::Flow(
                        throw_located_call_error(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            "Cannot call constructor",
                        )?,
                    ));
                }
                match resolve_magic_call_method(eg, class, "__callStatic", true) {
                    MagicCallMethod::Concrete(magic) => {
                        (magic, Some(Value::string(method)), false)
                    }
                    MagicCallMethod::Abstract => {
                        let diagnostic_class = eg
                            .find_public_class(class)
                            .map_or(class, |definition| definition.name.as_str());
                        return Ok(StaticCallTargetResolution::Flow(
                            throw_located_call_error(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &format!(
                                    "Cannot call abstract method {diagnostic_class}::{method}()"
                                ),
                            )?,
                        ));
                    }
                    MagicCallMethod::Missing if inaccessible => {
                        let (visibility, _, defining_class) = method_info
                            .as_ref()
                            .expect("inaccessible method retains declaration metadata");
                        let visibility = match visibility {
                            Visibility::Protected => "protected",
                            Visibility::Private => "private",
                            Visibility::Public => unreachable!(),
                        };
                        let scope = caller_class.as_deref().map_or_else(
                            || "global scope".to_string(),
                            |scope| format!("scope {scope}"),
                        );
                        return Ok(StaticCallTargetResolution::Flow(
                            throw_located_call_error(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &format!(
                                    "Call to {visibility} method {defining_class}::{method}() from {scope}"
                                ),
                            )?,
                        ));
                    }
                    MagicCallMethod::Missing => {
                        let diagnostic_class = eg
                            .find_public_class(class)
                            .map_or(class, |definition| definition.name.as_str());
                        return Ok(StaticCallTargetResolution::Flow(
                            throw_located_call_error(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &format!(
                                    "Call to undefined method {diagnostic_class}::{method}()"
                                ),
                            )?,
                        ));
                    }
                }
            }
        }
    };
    let method_is_non_static = instance_magic
        || (magic_method.is_none()
            && method_info
                .as_ref()
                .is_some_and(|(_, is_static, _)| !is_static));
    Ok(StaticCallTargetResolution::Resolved(
        resolved,
        method_is_non_static,
        magic_method,
    ))
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
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }
    let class = resolved_class.unwrap_or_else(|| raw_class.clone());
    let num_args = opline.extended_value;
    let cached = op_array.cache[ip].func;
    if !cached.is_null() && cached as usize & STATIC_CALL_DIRECT_TRAIT != 0 {
        let class_id = eg.class_id_of(&class);
        if let Some(result) = report_direct_static_trait_member_access(
            eg, frame, op_array, opline, class_id, &method, true,
        )? {
            return Ok(result);
        }
    }
    let (func_ptr, method_is_non_static, magic_method, trait_scope_class_id) = if !cached.is_null() {
        let tagged = cached as usize;
        if tagged & STATIC_CALL_TAG_MASK == 0 {
            // Keep the overwhelmingly common static-method cache hit as the
            // original pointer without an unconditional mask operation.
            (cached, false, None, op_array.cache[ip].class_id)
        } else {
            (
                (tagged & !STATIC_CALL_TAG_MASK) as *const FunctionCommon,
                tagged & STATIC_CALL_NON_STATIC != 0,
                None,
                op_array.cache[ip].class_id,
            )
        }
    } else {
        let relative_scope = raw_class.eq_ignore_ascii_case("self")
            || raw_class.eq_ignore_ascii_case("parent")
            || raw_class.eq_ignore_ascii_case("static");
        let class_is_available = if relative_scope {
            eg.find_class(&class).is_some()
        } else {
            eg.find_public_class(&class).is_some()
        };
        if !class_is_available {
            let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &class)?;
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            if !loaded {
                let error = make_error_value("Error", &format!("Class \"{class}\" not found"));
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }

        let direct_trait_call = eg
            .find_class(&class)
            .is_some_and(|definition| definition.is_trait);
        if direct_trait_call {
            let class_id = eg.class_id_of(&class);
            if let Some(result) = report_direct_static_trait_member_access(
                eg, frame, op_array, opline, class_id, &method, true,
            )? {
                return Ok(result);
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
                return Ok(match throw_in_frame(eg, frame, error)? {
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
                return Ok(match throw_in_frame(eg, frame, error)? {
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
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
        }

        let call_site_ip = call_site_instruction_index(op_array, ip);
        let (resolved, method_is_non_static, magic_method) =
            match resolve_static_call_target(
                eg,
                frame,
                op_array,
                call_site_ip,
                &class,
                &method,
                dynamic_scope,
            )? {
                StaticCallTargetResolution::Resolved(
                    resolved,
                    method_is_non_static,
                    magic_method,
                ) => (resolved, method_is_non_static, magic_method),
                StaticCallTargetResolution::Flow(result) => return Ok(result),
            };
        // SAFETY: `find_function` returns a request-owned immutable function
        // descriptor that remains live throughout execution.
        let trait_scope_class_id = if unsafe { &*resolved }
            .plan
            .needs_trait_class_scope()
        {
            trait_class_scope_for_dispatch(eg, resolved, &class)
        } else {
            0
        };

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
                cache.func = ((resolved as usize)
                    | usize::from(method_is_non_static) * STATIC_CALL_NON_STATIC
                    | usize::from(direct_trait_call) * STATIC_CALL_DIRECT_TRAIT)
                    as *const FunctionCommon;
                cache.class_id = trait_scope_class_id;
            }
        }
        (
            resolved,
            method_is_non_static,
            magic_method,
            trait_scope_class_id,
        )
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
            let call_site_ip = call_site_instruction_index(op_array, ip);
            return Ok(throw_non_static_callback_error(
                eg,
                frame,
                op_array,
                call_site_ip,
                &class,
                &method,
            )?);
        }
    }
    if magic_method.is_none()
        && !common.plan.needs_trait_class_scope()
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
        if magic_method.is_some() {
            (*call).set_magic_call(true);
        }
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
    initialize_trait_class_scope(eg, call, func_ptr, trait_scope_class_id);
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
    // SAFETY: dispatch supplies a live frame and the compiler-validated method
    // operand for this InitLateStaticCall instruction.
    let method_name =
        unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    if eg
        .class_by_id(class_id)
        .is_some_and(|definition| definition.is_trait)
    {
        let method = method_name.as_str().unwrap_or("");
        if let Some(result) = report_direct_static_trait_member_access(
            eg, frame, op_array, opline, class_id, method, true,
        )? {
            return Ok(result);
        }
    }
    let cache = &op_array.cache[ip];
    let (func_ptr, trait_scope_class_id, magic_method) = if cache.class_id == class_id
        && !cache.func.is_null()
    {
        (cache.func, cache.method_trait_scope_class_id(), None)
    } else {
        let call_site_ip = call_site_instruction_index(op_array, ip);
        let Some(class_definition) = eg.class_by_id(class_id) else {
            let error = make_error_value(
                "Error",
                "Cannot access \"static\" when no class scope is active",
            );
            attach_throwable_origin(&error, eg, frame, op_array, call_site_ip);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        };
        let class = class_definition.name.clone();
        let method = method_name.as_str().unwrap_or("");
        let method_info = eg.find_method_info(&class, method);
        let caller_class = if opline._pad & CALL_FLAG_DYNAMIC_STATIC_SCOPE != 0 {
            resolve_static_call_class(eg, frame, "self", true)
        } else {
            get_caller_class(frame, eg)
        };
        let inaccessible = method_info.as_ref().is_some_and(
            |(visibility, _, defining_class)| {
                *visibility != Visibility::Public
                    && !eg.check_visibility(
                        caller_class.as_deref(),
                        defining_class,
                        *visibility,
                    )
            },
        );
        let full_name = format!("{}::{}", class, method);
        let direct = eg.find_function(&full_name);
        if let Some((defining_class, declared_method)) =
            find_abstract_method_declaration(eg, &class, method)
        {
            let error = make_error_value(
                "Error",
                &format!(
                    "Cannot call abstract method {defining_class}::{declared_method}()"
                ),
            );
            attach_throwable_origin(&error, eg, frame, op_array, call_site_ip);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }

        let (resolved, magic_method) = if direct.is_some() && !inaccessible {
            (direct.expect("checked direct late-static method"), None)
        } else {
            let live_receiver = get_caller_class(frame, eg).and_then(|_| {
                closure_bound_this(frame, op_array, false).filter(|receiver| {
                    receiver
                        .as_object()
                        .is_some_and(|object| eg.class_is_a(&object.class_name, &class))
                })
            });
            match live_receiver
                .as_ref()
                .map(|_| resolve_magic_call_method(eg, &class, "__call", false))
            {
                Some(MagicCallMethod::Concrete(magic)) => {
                    (magic, Some(Value::string(method)))
                }
                Some(MagicCallMethod::Abstract) => {
                    let error = make_error_value(
                        "Error",
                        &format!("Cannot call abstract method {class}::{method}()"),
                    );
                    attach_throwable_origin(&error, eg, frame, op_array, call_site_ip);
                    return Ok(match throw_in_frame(eg, frame, error)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                    });
                }
                Some(MagicCallMethod::Missing) | None => {
                    if direct.is_none() && method.eq_ignore_ascii_case("__construct") {
                        let error = make_error_value("Error", "Cannot call constructor");
                        attach_throwable_origin(&error, eg, frame, op_array, call_site_ip);
                        return Ok(match throw_in_frame(eg, frame, error)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                        });
                    }
                    match resolve_magic_call_method(eg, &class, "__callStatic", true) {
                        MagicCallMethod::Concrete(magic) => {
                            (magic, Some(Value::string(method)))
                        }
                        MagicCallMethod::Abstract => {
                            let error = make_error_value(
                                "Error",
                                &format!("Cannot call abstract method {class}::{method}()"),
                            );
                            attach_throwable_origin(
                                &error,
                                eg,
                                frame,
                                op_array,
                                call_site_ip,
                            );
                            return Ok(match throw_in_frame(eg, frame, error)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    ColdResult::Unhandled(thrown)
                                }
                            });
                        }
                        MagicCallMethod::Missing if inaccessible => {
                            let (visibility, _, defining_class) = method_info
                                .as_ref()
                                .expect("inaccessible method retains declaration metadata");
                            let visibility = match visibility {
                                Visibility::Protected => "protected",
                                Visibility::Private => "private",
                                Visibility::Public => unreachable!(),
                            };
                            let scope = caller_class.as_deref().map_or_else(
                                || "global scope".to_string(),
                                |scope| format!("scope {scope}"),
                            );
                            let error = make_error_value(
                                "Error",
                                &format!(
                                    "Call to {visibility} method {defining_class}::{method}() from {scope}"
                                ),
                            );
                            attach_throwable_origin(
                                &error,
                                eg,
                                frame,
                                op_array,
                                call_site_ip,
                            );
                            return Ok(match throw_in_frame(eg, frame, error)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    ColdResult::Unhandled(thrown)
                                }
                            });
                        }
                        MagicCallMethod::Missing => {
                            let error = make_error_value(
                                "Error",
                                &format!("Call to undefined method {class}::{method}()"),
                            );
                            attach_throwable_origin(
                                &error,
                                eg,
                                frame,
                                op_array,
                                call_site_ip,
                            );
                            return Ok(match throw_in_frame(eg, frame, error)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    ColdResult::Unhandled(thrown)
                                }
                            });
                        }
                    }
                }
            }
        };
        // SAFETY: `find_function` returns a request-owned immutable function
        // descriptor that remains live throughout execution.
        let trait_scope_class_id = if unsafe { &*resolved }
            .plan
            .needs_trait_class_scope()
        {
            trait_class_scope_for_dispatch(eg, resolved, &class)
        } else {
            0
        };

        if magic_method.is_none() {
            unsafe {
                let cache = &mut *(op_array.cache.as_ptr().add(ip)
                    as *mut crate::vm::instruction::InlineCache);
                cache.class_id = class_id;
                cache.func = resolved;
                if trait_scope_class_id != 0 {
                    cache.set_method_trait_scope_class_id(trait_scope_class_id);
                }
            }
        }
        (resolved, trait_scope_class_id, magic_method)
    };

    let num_args = opline.extended_value;
    let common = unsafe { &*func_ptr };
    if magic_method.is_none()
        && !common.plan.needs_trait_class_scope()
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

    let pending_call = unsafe { (*frame).call };
    let target_is_instance = !common.plan.is_static_method();
    let live_receiver = target_is_instance
        .then(|| closure_bound_this(frame, op_array, false))
        .flatten()
        .filter(|receiver| {
            receiver.as_object().is_some_and(|object| {
                eg.class_by_id(class_id)
                    .is_some_and(|class| eg.class_is_a(&object.class_name, &class.name))
            })
        });
    if target_is_instance && live_receiver.is_none() {
        let call_site_ip = call_site_instruction_index(op_array, ip);
        let class = eg
            .class_by_id(class_id)
            .map_or_else(|| "static".to_string(), |definition| definition.name.clone());
        return Ok(throw_non_static_callback_error(
            eg,
            frame,
            op_array,
            call_site_ip,
            &class,
            method_name.as_str().unwrap_or(""),
        )?);
    }
    let storage_slots = if magic_method.is_some() {
        (num_args + 1).max(3)
    } else {
        num_args + 1
    };
    let call = eg
        .vm_stack
        .push_call_frame(func_ptr, storage_slots, num_args, frame, pending_call);
    unsafe {
        (*frame).call = call;
        if magic_method.is_some() {
            (*call).set_magic_call(true);
        }
        if let Some(receiver) = live_receiver {
            if common.plan.borrow_this() {
                frame_set_borrowed_this(call, (*frame).cv(0) as *const Value);
            } else {
                frame_set_this(call, receiver);
            }
        } else {
            // Late-static method calls use the same hidden class-method slot
            // as ordinary static calls. A genuine static target has no
            // receiver to publish there, so initialize it before SendVal
            // fills CV 1..N. Wide-frame cleanup scans every CV.
            frame_slot_init(call, (*call).cv_mut(0) as *mut Value, Value::undef());
        }
    }
    if class_id != 0 {
        publish_late_static_call_class_id(eg, call, class_id);
    }
    initialize_trait_class_scope(eg, call, func_ptr, trait_scope_class_id);
    if let Some(method) = magic_method {
        push_pending_magic_call(eg, call as usize, method);
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
    let ordinary = resolve_user_call_at_opline(eg, frame, op_array, opline);
    let checked = if ordinary.is_some() {
        ordinary
    } else {
        resolve_user_call_at_opline_checked(eg, frame, op_array, opline, ordinary)?
    };
    let resolved = match checked {
        Some(resolved) => resolved,
        None => {
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                });
            }
            let callback_raw = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            let callback = if callback_raw.is_reference() {
                unsafe { &*callback_raw.as_ref_ptr() }
            } else {
                callback_raw
            };
            let function = if opline._pad == 1 {
                "call_user_func_array"
            } else {
                "call_user_func"
            };
            let reason = crate::stdlib::ordinary_callback_invalid_reason(callback, eg);
            let message = format!(
                "{function}(): Argument #1 ($callback) must be a valid callback, {reason}"
            );
            let error = make_error_value("TypeError", &message);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    };

    if let Some(name) = crate::stdlib::scope_introspection_callback_name(&resolved) {
        let error = make_error_value("Error", &format!("Cannot call {name}() dynamically"));
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    }

    init_resolved_user_call(eg, frame, opline.extended_value, resolved);
    Ok(ColdResult::Done)
}

#[cold]
fn legacy_callback_deprecation_type_error(
    eg: &ExecutorGlobals,
    function: &str,
    previous: Value,
) -> Value {
    let error = make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #1 ($callback) must be a valid callback, (null)"
        ),
    );
    if let Some(mut object) = error.as_object_mut() {
        let key = eg
            .find_property_visibility("TypeError", "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring)| crate::runtime::mangle_private_prop(&declaring, "previous"),
            );
        object.set_property(&key, previous);
    }
    error
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
    let callback = callback_raw.dereferenced();
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache_slot = unsafe {
        op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache
    };
    let caller_class = get_caller_class(frame, eg);
    let ordinary = crate::stdlib::resolve_callback_with_cache(
        callback,
        eg,
        caller_class.as_deref(),
        Some(cache_slot),
    );
    if ordinary
        .as_ref()
        .is_some_and(|resolved| !resolved.is_magic_call)
    {
        return ordinary;
    }
    resolve_user_call_magic_fallback(
        eg,
        frame,
        op_array,
        callback,
        caller_class.as_deref(),
        ordinary,
    )
}

#[cold]
#[inline(never)]
fn resolve_user_call_magic_fallback(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    callback: &Value,
    caller_class: Option<&str>,
    ordinary: Option<crate::stdlib::ResolvedCallback>,
) -> Option<crate::stdlib::ResolvedCallback> {
    let receiver = closure_bound_this(frame, op_array, false);
    crate::stdlib::resolve_live_scoped_instance_callback(
        callback,
        eg,
        caller_class,
        receiver.as_ref(),
    )
    .or(ordinary)
}

#[cold]
#[inline(never)]
fn resolve_user_call_at_opline_checked(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    mut ordinary: Option<crate::stdlib::ResolvedCallback>,
) -> Result<Option<crate::stdlib::ResolvedCallback>, VmError> {
    let callback_raw = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    let callback = if callback_raw.is_reference() {
        unsafe { &*callback_raw.as_ref_ptr() }
    } else {
        callback_raw
    };
    if ordinary.is_none() && crate::stdlib::ensure_callback_class_loaded(callback, eg)? {
        ordinary = resolve_user_call_at_opline(eg, frame, op_array, opline);
    }
    if ordinary.is_some() || eg.exception.is_some() {
        return Ok(ordinary);
    }
    if !crate::stdlib::callback_uses_legacy_scope(callback) {
        return Ok(ordinary);
    }
    let lexical_class = get_caller_class(frame, eg);
    let receiver = closure_bound_this(frame, op_array, false);
    let called_class = receiver
        .as_ref()
        .and_then(Value::as_object)
        .map(|object| object.class_name.to_string())
        .or_else(|| {
            eg.class_by_id(late_static_call_class_id(eg, frame))
                .map(|class| class.name.clone())
        });
    match crate::stdlib::resolve_legacy_callback(
        callback,
        eg,
        lexical_class.as_deref(),
        called_class.as_deref(),
        receiver.as_ref(),
    ) {
        crate::stdlib::LegacyCallbackResolution::NotLegacy => Ok(ordinary),
        crate::stdlib::LegacyCallbackResolution::Legacy {
            resolved,
            deprecation,
        } => {
            let invalid_reason = resolved.is_none().then(|| {
                crate::stdlib::legacy_callback_invalid_reason(
                    callback,
                    eg,
                    lexical_class.as_deref(),
                    called_class.as_deref(),
                    receiver.as_ref(),
                )
            });
            if let Some(deprecation) = deprecation {
                report_php_deprecation(eg, frame, op_array, opline, &deprecation)?;
            }
            if let Some(previous) = eg.exception.take() {
                let function = if opline._pad == 1 {
                    "call_user_func_array"
                } else {
                    "call_user_func"
                };
                eg.exception = Some(legacy_callback_deprecation_type_error(
                    eg, function, previous,
                ));
                Ok(None)
            } else if let Some(reason) = invalid_reason {
                let function = if opline._pad == 1 {
                    "call_user_func_array"
                } else {
                    "call_user_func"
                };
                eg.exception = Some(make_error_value(
                    "TypeError",
                    &format!(
                        "{function}(): Argument #1 ($callback) must be a valid callback, {reason}"
                    ),
                ));
                Ok(None)
            } else {
                Ok(resolved)
            }
        }
    }
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
    let trait_scope_class_id = if resolved.common().plan.needs_trait_class_scope() {
        resolved
            .bound_this
            .as_ref()
            .and_then(Value::as_object)
            .map(|object| object.class_name.to_string())
            .or_else(|| {
                resolved
                    .prepend_args
                    .first()
                    .and_then(Value::as_object)
                    .map(|object| object.class_name.to_string())
            })
            .or_else(|| {
                eg.class_by_id(resolved.called_scope_class_id)
                    .map(|class| class.name.clone())
            })
            .as_deref()
            .map_or(0, |class| {
                trait_class_scope_for_dispatch(eg, resolved.func_ptr, class)
            })
    } else {
        0
    };
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
        if magic_method.is_some() {
            (*call).set_magic_call(true);
        }
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
    initialize_trait_class_scope(eg, call, resolved.func_ptr, trait_scope_class_id);
}

#[cold]
#[inline(never)]
fn unresolved_array_callable_message(
    eg: &ExecutorGlobals,
    callable_array: &PhpArray,
    closure_receiver: bool,
    class_name: Option<&str>,
    static_semantics: bool,
) -> String {
    let members = callback_array_members(callable_array);
    if closure_receiver {
        let method = members
            .map(|(_, method)| method)
            .and_then(Value::as_str)
            .unwrap_or("");
        return format!("Call to undefined method Closure::{method}()");
    }
    let Some(method) = members
        .map(|(_, method)| method)
        .and_then(Value::as_str)
    else {
        return "Array is not callable".to_string();
    };
    let class = class_name.map(str::to_string).or_else(|| {
        members.map(|(receiver, _)| receiver).and_then(|receiver| {
            receiver
                .as_object()
                .map(|object| object.class_name.to_string())
                .or_else(|| receiver.as_str().map(str::to_string))
        })
    });
    let Some(class) = class else {
        return "Array is not callable".to_string();
    };
    if let Some((defining, declared_method)) =
        find_abstract_method_declaration(eg, &class, method)
    {
        return format!("Cannot call abstract method {defining}::{declared_method}()");
    }
    if matches!(
        resolve_magic_call_method(
            eg,
            &class,
            if static_semantics {
                "__callStatic"
            } else {
                "__call"
            },
            static_semantics,
        ),
        MagicCallMethod::Abstract
    ) {
        let diagnostic_class = eg
            .find_public_class(&class)
            .map_or(class.as_str(), |definition| definition.name.as_str());
        return format!("Cannot call abstract method {diagnostic_class}::{method}()");
    }
    format!("Call to undefined method {class}::{method}()")
}

#[inline]
fn callback_array_members(callable: &PhpArray) -> Option<(&Value, &Value)> {
    if let Some(values) = callable.packed_values() {
        return values.first().zip(values.get(1));
    }
    Some((callable.get_int(0)?, callable.get_int(1)?))
}

#[cold]
#[inline(never)]
fn resolve_nonpacked_array_callback(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    owner: &Value,
    method: &Value,
) -> Option<crate::stdlib::ResolvedCallback> {
    // Callback semantics are keyed by integer indices 0 and 1, not by
    // insertion order. Normalize the uncommon hash-backed representation so
    // the shared resolver can keep its packed fast path.
    let mut callback = PhpArray::with_packed_capacity(2);
    callback.push(owner.clone());
    callback.push(method.clone());
    let callback = Value::array(callback);
    let caller_class = get_caller_class(frame, eg);
    let ordinary = crate::stdlib::resolve_callback_with_cache(
        &callback,
        eg,
        caller_class.as_deref(),
        None,
    );
    if ordinary
        .as_ref()
        .is_some_and(|resolved| !resolved.is_magic_call)
    {
        ordinary
    } else {
        resolve_user_call_magic_fallback(
            eg,
            frame,
            op_array,
            &callback,
            caller_class.as_deref(),
            ordinary,
        )
    }
}

#[inline(never)]
fn op_init_dynamic_static_member_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let (callable, instruction_index) = dynamic_call_operand(frame, op_array, opline);
    let Some(callable_array) = callable.as_array() else {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            "Array is not callable",
        )?);
    };
    if callable_array.len() != 2 {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            "Array callback must have exactly two elements",
        )?);
    }
    let Some((owner, method_value)) = callback_array_members(callable_array) else {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            "Array callback has to contain indices 0 and 1",
        )?);
    };
    let Some(method) = method_value.as_str() else {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            "Method name must be a string",
        )?);
    };
    let class_name = owner
        .as_str()
        .map(str::to_string)
        .or_else(|| owner.as_object().map(|object| object.class_name.to_string()));
    let Some(class_name) = class_name else {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            "Class name must be a valid object or a string",
        )?);
    };
    if matches!(
        class_name.to_ascii_lowercase().as_str(),
        "self" | "parent" | "static"
    ) && get_caller_class(frame, eg).is_none()
    {
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            &format!(
                "Cannot access \"{}\" when no class scope is active",
                class_name.to_ascii_lowercase()
            ),
        )?);
    }
    if eg.find_class(&class_name).is_none() {
        let loaded = crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        if !loaded {
            return Ok(throw_located_call_error(
                eg,
                frame,
                op_array,
                instruction_index,
                &format!("Class \"{class_name}\" not found"),
            )?);
        }
    }
    if class_callback_requires_instance(
        eg,
        &class_name,
        method,
        get_caller_class(frame, eg).as_deref(),
    ) {
        return Ok(throw_non_static_callback_error(
            eg,
            frame,
            op_array,
            instruction_index,
            &class_name,
            method,
        )?);
    }

    let transformed = owner.as_object().map(|_| {
        let mut callback = PhpArray::with_packed_capacity(2);
        callback.push(Value::string(&class_name));
        callback.push(Value::string(method));
        Value::array(callback)
    });
    let resolved = if let Some(callback) = transformed.as_ref() {
        let caller_class = get_caller_class(frame, eg);
        let ordinary = crate::stdlib::resolve_callback_with_cache(
            callback,
            eg,
            caller_class.as_deref(),
            None,
        );
        if ordinary
            .as_ref()
            .is_some_and(|resolved| !resolved.is_magic_call)
        {
            ordinary
        } else {
            let receiver = closure_bound_this(frame, op_array, false);
            crate::stdlib::resolve_live_scoped_instance_callback(
                callback,
                eg,
                caller_class.as_deref(),
                receiver.as_ref(),
            )
            .or(ordinary)
        }
    } else {
        resolve_user_call_at_opline(eg, frame, op_array, opline)
    };
    let Some(resolved) = resolved else {
        let message = unresolved_array_callable_message(
            eg,
            &callable_array,
            false,
            Some(&class_name),
            true,
        );
        return Ok(throw_located_call_error(
            eg,
            frame,
            op_array,
            instruction_index,
            &message,
        )?);
    };
    init_resolved_user_call_mode(eg, frame, opline.extended_value, resolved, true);
    Ok(ColdResult::Done)
}

#[inline(always)]
fn dynamic_call_operand<'a>(
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> (&'a Value, usize) {
    // SAFETY: dispatch supplies the live frame and an instruction from this
    // op array; the compiler validated the operand kind and slot index.
    unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
        )
    }
}

#[inline(always)]
fn init_closure_dynamic_call(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    explicit_args: u32,
    closure: &PhpClosure,
) {
    let func_ptr = closure.func;
    let mut resolved = crate::stdlib::ResolvedCallback {
        func_ptr,
        prepend_args: vec![],
        use_vars: closure.clone_captures(),
        called_scope_class_id: closure.called_scope_class_id,
        bound_this: closure.bound_this.clone(),
        closure_static_vars: closure.static_vars.clone(),
        is_magic_call: crate::stdlib::closure_is_magic_call(closure, eg),
    };
    let is_method = resolved.is_method();
    if is_method {
        resolved.prepend_args = vec![resolved.bound_this.clone().unwrap_or_else(Value::null)];
    }
    init_resolved_user_call_mode(eg, frame, explicit_args, resolved, is_method);
    // SAFETY: the live frame owns the call initialized immediately above.
    let call = unsafe { (*frame).call };
    initialize_trait_class_scope(eg, call, func_ptr, closure.trait_scope_class_id);
}

#[inline(never)]
fn op_init_dynamic_call<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let (callable, instruction_index) = dynamic_call_operand(frame, op_array, opline);
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
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let Some((callback_owner, callback_method)) = callback_array_members(callable_array) else {
            let error = make_error_value(
                "Error",
                "Array callback has to contain indices 0 and 1",
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        };
        let closure_receiver = callback_owner.value_type() == ValueType::Closure;
        if closure_receiver
            && callback_method
                .as_str()
                .is_some_and(|method| method.eq_ignore_ascii_case("__invoke"))
        {
            let closure = callback_owner
                .as_closure()
                .expect("Closure-tagged array receiver must retain its payload");
            init_closure_dynamic_call(eg, frame, opline.extended_value, closure);
            return Ok(ColdResult::Done);
        }
        if !closure_receiver
            && callback_owner.as_str().is_none()
            && callback_owner.as_object().is_none()
        {
            let error = make_error_value(
                "Error",
                "Class name must be a valid object or a string",
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        if callback_method.as_str().is_none() {
            let error = make_error_value("Error", "Method name must be a string");
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let class_name = callback_method
            .as_str()
            .and_then(|_| callback_owner.as_str())
            .map(str::to_string);
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
            return Ok(match throw_in_frame(eg, frame, error)? {
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
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
            if !loaded {
                let error = make_error_value("Error", &format!("Class \"{class_name}\" not found"));
                attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        }
        if let Some(class_name) = class_name.as_deref()
            && let Some(method) = callback_method.as_str()
            && class_callback_requires_instance(
                eg,
                class_name,
                method,
                get_caller_class(frame, eg).as_deref(),
            )
        {
            return Ok(throw_non_static_callback_error(
                eg,
                frame,
                op_array,
                instruction_index,
                class_name,
                method,
            )?);
        }
        let resolved = if callable_array.is_packed() {
            resolve_user_call_at_opline(eg, frame, op_array, opline)
        } else {
            resolve_nonpacked_array_callback(
                eg,
                frame,
                op_array,
                callback_owner,
                callback_method,
            )
        };
        let Some(resolved) = resolved else {
            let message = unresolved_array_callable_message(
                eg,
                &callable_array,
                closure_receiver,
                class_name.as_deref(),
                callback_owner.as_str().is_some(),
            );
            let error = make_error_value("Error", &message);
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
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
        // Dynamic sends start at CV 0. A first-class method closure retains
        // the hidden receiver slot, so the shared initializer defers it until
        // DoFcall shifts the explicit argument prefix.
        init_closure_dynamic_call(eg, frame, opline.extended_value, closure);
        return Ok(ColdResult::Done);
    } else if let Some(func_name) = callable.as_str() {
        // Simple string function call: $func = "my_func"; $func()
        if let Some((class_name, method)) = func_name.rsplit_once("::") {
            let class_name = class_name.trim_start_matches('\\');
            if class_callback_requires_instance(
                eg,
                class_name,
                method,
                get_caller_class(frame, eg).as_deref(),
            ) {
                return Ok(throw_non_static_callback_error(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    class_name,
                    method,
                )?);
            }
        }
        if let Some(normalized) = scope_introspection_function_name(func_name) {
            let error = make_error_value(
                "Error",
                &format!("Cannot call {normalized}() dynamically"),
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        let lookup_name = crate::stdlib::dynamic_function_lookup_name(func_name);
        let Some(func_ptr) = eg.find_function(lookup_name) else {
            let diagnostic_name = if opline.op1_type == OpType::Const {
                lookup_name
            } else {
                func_name
            };
            let error = make_error_value(
                "Error",
                &format!("Call to undefined function {diagnostic_name}()"),
            );
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error)? {
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
                return Ok(match throw_in_frame(eg, frame, error)? {
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
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    }
    Ok(ColdResult::Done)
}
