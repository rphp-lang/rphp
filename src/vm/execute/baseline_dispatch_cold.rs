// Included in the execute module so cold opcode helpers keep private access to
// the canonical frame machinery without adding abstractions to hot dispatch.

/// Publish globals dirtied by a recursively invoked callback back into the
/// suspended caller's tracked CVs. Ordinary VM calls do this while unwinding;
/// callbacks entered directly from an opcode or stdlib handler return across
/// an execution boundary and need the same synchronization explicitly.
pub(crate) unsafe fn sync_dirty_globals_to_frame(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) {
    if frame.is_null() || eg.dirty_globals.is_empty() {
        return;
    }
    let op_array = unsafe { (*frame).op_array() };
    let vars = if !op_array.main_scope_vars.is_empty() {
        &op_array.main_scope_vars
    } else {
        &op_array.global_vars
    };
    for (cv, name) in vars {
        if eg.dirty_globals.contains(name)
            && let Some(value) = eg.globals.get(name).cloned()
        {
            let destination = unsafe { (*frame).get_op_mut(*cv, OpType::Cv) };
            unsafe { frame_slot_set(frame, destination, value) };
        }
    }
    if !vars.is_empty() {
        eg.dirty_globals.clear();
    }
}

/// Emit the PHP 8.2 undefined-local diagnostic for one already-snapshotted
/// read. The caller owns control-flow handling when a user handler throws.
fn report_undefined_variable_read(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    name_literal: u16,
    suppressed: bool,
) -> Result<(), VmError> {
    let name = op_array.literals()[name_literal as usize]
        .as_str()
        .unwrap_or("");
    let instruction_index = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let line = op_array.source_line(instruction_index).unwrap_or(0);
    let file = if op_array.source_file.is_empty() {
        op_array.name.as_str()
    } else {
        op_array.source_file.as_str()
    };
    let message = format!("Undefined variable ${name}");
    if suppressed {
        eg.begin_error_suppression(frame as usize);
    }
    let handled = crate::stdlib::dispatch_php_error(eg, frame, 2, &message, file, line);
    // Decide whether the built-in diagnostic is visible while the suppression
    // scope is still active. Restoring the caller's mask first would make an
    // ordinary `@$missing` warn merely because the outer mask contains
    // E_WARNING. A handler may explicitly re-enable E_WARNING inside `@`, in
    // which case PHP does expose the declined built-in diagnostic.
    let report_builtin = eg.error_reporting & 2 != 0;
    if suppressed {
        eg.end_error_suppression(frame as usize);
    }
    let handled = handled?;
    if !handled && report_builtin {
        eg.write_output(format!("\nWarning: {message} in {file} on line {line}\n").as_bytes());
    }
    Ok(())
}

/// Snapshot a runtime-resolved send operand. A by-reference caller bypasses
/// this helper; every by-value path consumes null before invoking the handler,
/// so a re-entrant assignment cannot change the current argument value.
fn snapshot_runtime_send_rvalue(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<Value, VmError> {
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
    if !source.is_undef() {
        return Ok(source.clone());
    }

    let snapshot = Value::null();
    if opline._pad & crate::vm::instruction::SEND_FLAG_FETCH_CV_R != 0 {
        report_undefined_variable_read(
            eg,
            frame,
            op_array,
            opline,
            opline.result,
            opline._pad & crate::vm::instruction::SEND_FLAG_ERROR_SUPPRESS != 0,
        )?;
    }
    Ok(snapshot)
}

#[inline(never)]
fn op_check_generic_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    op_check_generic_args_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_check_late_static_generic_args(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    op_check_generic_args_impl::<true>(eg, frame, op_array, opline)
}

#[inline(always)]
fn op_check_generic_args_impl<const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    // SAFETY: dispatch supplies an instruction from this op-array and a live
    // frame whose compiler-emitted owner operand remains valid for this check.
    // The cache has exactly one stable entry per instruction.
    let (cache, raw_owner) = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        (
            &mut *(op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache),
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
        )
    };

    let kind = crate::generics::GenericDeclarationKind::from_tag(opline._pad)
        .ok_or_else(|| VmError::Fatal("Invalid generic declaration kind".into()))?;
    let owner_value = if raw_owner.is_reference() {
        // SAFETY: a Reference-tagged owner points at a live VM slot for the
        // duration of this non-reentrant opcode.
        unsafe { &*raw_owner.as_ref_ptr() }
    } else {
        raw_owner
    };

    if let Some(declaration) = cache.generic_declaration() {
        let cache_hit = if LATE_STATIC {
            let class_id = late_static_call_class_id(eg, frame);
            class_id != 0 && cache.class_id == class_id
        } else if kind == crate::generics::GenericDeclarationKind::Method
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
            {
                if cache.generic_signature_uses_class_scope() {
                    let class_id = generic_scope_class_id(eg, frame, kind, owner_value);
                    eg.push_reified_binding_scope_with_class(
                        frame as usize,
                        binding,
                        class_id,
                    );
                } else {
                    eg.push_reified_binding_scope(frame as usize, binding);
                }
            }
            #[cfg(not(feature = "php-generics-reified"))]
            let _ = binding;
            return Ok(());
        }
    }

    resolve_generic_args_cache_miss::<LATE_STATIC>(
        eg,
        frame,
        op_array,
        opline,
        cache,
        kind,
        owner_value,
    )
}

#[cold]
#[inline(never)]
fn resolve_generic_args_cache_miss<const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    cache: &mut crate::vm::instruction::InlineCache,
    mut kind: crate::generics::GenericDeclarationKind,
    owner_value: &Value,
) -> Result<(), VmError> {
    let mut cacheable = opline.op1_type == OpType::Const;
    let mut receiver_class_id = 0;
    let mut callable = std::ptr::null();

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let mut static_receiver_scope = None;
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
        let target_class = object.class_name.to_string();
        let caller_class = get_caller_class(frame, eg);
        let dispatch_class = if let Some(ref caller) = caller_class {
            if let Some((Visibility::Private, defining)) =
                eg.find_method_visibility(caller, method)
            {
                if defining.eq_ignore_ascii_case(caller)
                    && eg.class_is_a(&target_class, caller)
                {
                    caller.clone()
                } else {
                    target_class
                }
            } else {
                target_class
            }
        } else {
            target_class
        };
        format!("{}::{}", dispatch_class, method)
    } else if let Some(name) = owner_value.as_str() {
        if kind == crate::generics::GenericDeclarationKind::Method {
            if name
                .rsplit_once("::")
                .is_some_and(|(class, _)| {
                    class.eq_ignore_ascii_case("self")
                        || class.eq_ignore_ascii_case("parent")
                        || class.eq_ignore_ascii_case("static")
                })
            {
                // Late-static and shared-trait bytecode can resolve to
                // multiple declarations. Its dedicated opcode validates the
                // cached declaration against the recovered called class;
                // legacy/unmarked pseudo owners remain safely uncached.
                cacheable = LATE_STATIC;
            }
            let resolved = resolve_static_method_owner(eg, frame, name)
                .unwrap_or_else(|| name.to_string());
            receiver_class_id = resolved
                .rsplit_once("::")
                .map_or(0, |(class, _)| eg.class_id_of(class));
            #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
            {
                static_receiver_scope = resolved
                    .rsplit_once("::")
                    .map(|(class, _)| class.to_string());
            }
            resolved
        } else {
            name.to_string()
        }
    } else if let Some(closure) = owner_value.as_closure() {
        let Some(user) = closure.user_function() else {
            return Err(VmError::Fatal(
                "Generic arguments are not supported for this callable".into(),
            ));
        };
        kind = crate::generics::GenericDeclarationKind::Closure;
        cacheable = true;
        callable = closure.func;
        user.op_array.name.clone()
    } else {
        return Err(VmError::Fatal(
            "Generic arguments require a named function, method, class, or closure".into(),
        ));
    };

    // Metadata belongs to the concrete body declaration, not to aliases
    // installed for inheritance or trait composition. Resolve it by the same
    // function pointer the subsequent method call will execute. This also
    // prevents a non-generic override from falling through to generic parent
    // metadata merely because the names match.
    if kind == crate::generics::GenericDeclarationKind::Method {
        let method = owner
            .rsplit_once("::")
            .map(|(_, method)| method.to_string());
        if let Some(method) = method {
            if let Some(function) = eg.find_function(&owner) {
                if let Some(definition_owner) =
                    eg.method_definition_owner(function, &method)
                {
                    owner = format!("{}::{}", definition_owner, method);
                }
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

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let receiver_scope = if kind == crate::generics::GenericDeclarationKind::Method
        && owner_value.value_type() == ValueType::Object
    {
        Some(unsafe { owner_value.object_class_name_unchecked() })
    } else if kind == crate::generics::GenericDeclarationKind::Method {
        static_receiver_scope.as_deref()
    } else {
        None
    };
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let declaration_scope = eg.generic_declaration_scope(&owner, receiver_scope);
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    let binding = eg
        .generic_metadata
        .resolve_binding(kind, &owner, opline.extended_value, |actual, bound| {
            eg.class_is_a_in_generic_scopes(
                actual,
                bound,
                declaration_scope,
                receiver_scope,
            )
        })
        .map_err(VmError::Fatal)?;
    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    let binding = eg
        .generic_metadata
        .resolve_binding(kind, &owner, opline.extended_value, |actual, bound| {
            eg.class_is_a(actual, bound)
        })
        .map_err(VmError::Fatal)?;

    let uses_class_scope = eg
        .generic_metadata
        .declaration(binding)
        .is_some_and(|declaration| declaration.signature_uses_class_pseudo);

    if cacheable {
        cache.set_generic_declaration(
            binding.declaration,
            receiver_class_id,
            callable,
            uses_class_scope,
        );
    }

    #[cfg(feature = "php-generics-reified")]
    {
        if uses_class_scope {
            let class_id = generic_scope_class_id(eg, frame, kind, owner_value);
            eg.push_reified_binding_scope_with_class(frame as usize, binding, class_id);
        } else {
            eg.push_reified_binding_scope(frame as usize, binding);
        }
    }

    #[cfg(not(feature = "php-generics-reified"))]
    let _ = binding;

    Ok(())
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_scope_class_id(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    kind: crate::generics::GenericDeclarationKind,
    owner: &Value,
) -> u32 {
    if kind != crate::generics::GenericDeclarationKind::Method {
        return 0;
    }
    if owner.value_type() == ValueType::Object {
        return unsafe { owner.object_class_id_unchecked() };
    }
    let class = owner
        .as_str()
        .map(|name| name.split_once("::").map_or(name, |(class, _)| class));
    class.map_or(0, |class| {
        if class.eq_ignore_ascii_case("self")
            || class.eq_ignore_ascii_case("parent")
            || class.eq_ignore_ascii_case("static")
        {
            resolve_static_call_class(eg, frame, class, true)
                .map_or(0, |class| eg.class_id_of(&class))
        } else {
            eg.class_id_of(class)
        }
    })
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_call_class_is_a(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    scope_owner: usize,
    actual: &str,
    expected: &str,
    declared_scope: &str,
) -> bool {
    let common = unsafe { &*(*call).func };
    let receiver_scope = if common.sig.this_offset == 1 {
        let receiver = unsafe { &*(*call).cv(0) };
        if receiver.value_type() == ValueType::Object {
            Some(unsafe { receiver.object_class_name_unchecked() })
        } else {
            None
        }
    } else {
        None
    };
    let called_scope = receiver_scope.or_else(|| {
        eg.class_by_id(eg.reified_binding_scope_class_id(scope_owner))
            .map(|class| class.name.as_str())
    });
    let scope = eg.generic_declaration_scope(declared_scope, called_scope);
    eg.class_is_a_in_generic_scopes(actual, expected, scope, called_scope)
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn generic_call_reified_arguments_match(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    scope_owner: usize,
    value: &Value,
    expected: &str,
    arguments: &[crate::generics::GenericType],
    declaration: &crate::generics::GenericDeclaration,
    site: &crate::generics::GenericUseSite,
    declared_scope: &str,
) -> bool {
    let common = unsafe { &*(*call).func };
    let receiver_scope = if common.sig.this_offset == 1 {
        let receiver = unsafe { &*(*call).cv(0) };
        (receiver.value_type() == ValueType::Object)
            .then(|| unsafe { receiver.object_class_name_unchecked() })
    } else {
        None
    };
    let called_scope = receiver_scope.or_else(|| {
        eg.class_by_id(eg.reified_binding_scope_class_id(scope_owner))
            .map(|class| class.name.as_str())
    });
    let scope = eg.generic_declaration_scope(declared_scope, called_scope);
    eg.reified_object_arguments_match_binding(
        value,
        expected,
        arguments,
        declaration,
        site,
        scope,
        called_scope,
    )
}

#[cfg(feature = "php-generics-reified")]
#[inline]
fn value_matches_reified_default(
    eg: &ExecutorGlobals,
    scope_owner: usize,
    value: &Value,
    expected: &crate::generics::GenericType,
    binding: crate::generics::ReifiedBinding,
    declared_scope: &str,
) -> bool {
    let class_id = eg.reified_binding_scope_class_id(scope_owner);
    let receiver_scope = eg.class_by_id(class_id).map(|class| class.name.as_str());
    let scope = eg.generic_declaration_scope(declared_scope, receiver_scope);
    eg.generic_metadata.value_matches_binding_reified(
        value,
        expected,
        binding,
        |actual, bound| {
            eg.class_is_a_in_generic_scopes(actual, bound, scope, receiver_scope)
        },
        |value, name, arguments, declaration, site| {
            eg.reified_object_arguments_match_binding(
                value,
                name,
                arguments,
                declaration,
                site,
                scope,
                receiver_scope,
            )
        },
    )
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
        let declared_scope = eg
            .generic_metadata
            .symbol(declaration.owner)
            .unwrap_or("?");
        let fixed = declaration
            .value_parameters
            .len()
            .saturating_sub(usize::from(common.sig.is_variadic));
        for index in 0..fixed {
            let expected = &declaration.value_parameters[index];
            let Some(expected) = expected else {
                continue;
            };
            if index >= unsafe { (*call).num_args as usize } {
                continue;
            }
            let slot = common.sig.param_cv_index(index as u32);
            if slot >= unsafe { (*call).num_cvs } {
                break;
            }
            let value = unsafe { (*call).cv(slot) };
            if value.is_undef() {
                continue;
            }
            if !eg.generic_metadata.value_matches_binding_reified(
                value,
                expected,
                binding,
                |actual, bound| {
                    generic_call_class_is_a(
                        eg,
                        call,
                        frame as usize,
                        actual,
                        bound,
                        declared_scope,
                    )
                },
                |value, name, arguments, declaration, site| {
                    generic_call_reified_arguments_match(
                        eg,
                        call,
                        frame as usize,
                        value,
                        name,
                        arguments,
                        declaration,
                        site,
                        declared_scope,
                    )
                },
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
        if common.sig.is_variadic
            && let Some(expected) = declaration
                .value_parameters
                .last()
                .and_then(Option::as_ref)
        {
            let public_max = common.sig.public_arity();
            let extra = unsafe { (*call).num_args }.saturating_sub(public_max);
            for index in 0..extra {
                let value = unsafe { &*(*call).cv(common.sig.variadic_cv_index + index) };
                if !eg.generic_metadata.value_matches_binding_reified(
                    value,
                    expected,
                    binding,
                    |actual, bound| {
                        generic_call_class_is_a(
                            eg,
                            call,
                            frame as usize,
                            actual,
                            bound,
                            declared_scope,
                        )
                    },
                    |value, name, arguments, declaration, site| {
                        generic_call_reified_arguments_match(
                            eg,
                            call,
                            frame as usize,
                            value,
                            name,
                            arguments,
                            declaration,
                            site,
                            declared_scope,
                        )
                    },
                ) {
                    let owner = eg
                        .generic_metadata
                        .symbol(declaration.owner)
                        .unwrap_or("?");
                    return Err(VmError::Fatal(format!(
                        "Variadic argument #{} passed to {} does not match its reified generic type",
                        fixed + index as usize + 1,
                        owner
                    )));
                }
            }
            if let Some(named) = eg.pending_named_variadic.get(&(call as usize)) {
                for (name, value) in named {
                    if !eg.generic_metadata.value_matches_binding_reified(
                        value,
                        expected,
                        binding,
                        |actual, bound| {
                            generic_call_class_is_a(
                                eg,
                                call,
                                frame as usize,
                                actual,
                                bound,
                                declared_scope,
                            )
                        },
                        |value, name, arguments, declaration, site| {
                            generic_call_reified_arguments_match(
                                eg,
                                call,
                                frame as usize,
                                value,
                                name,
                                arguments,
                                declaration,
                                site,
                                declared_scope,
                            )
                        },
                    ) {
                        let owner = eg
                            .generic_metadata
                            .symbol(declaration.owner)
                            .unwrap_or("?");
                        return Err(VmError::Fatal(format!(
                            "Named variadic argument ${} passed to {} does not match its reified generic type",
                            name, owner
                        )));
                    }
                }
            }
        }
        eg.activate_reified_binding_scope(frame as usize, call as usize);
        Ok(())
    }
}

#[cold]
#[inline(never)]
fn op_check_generic_default(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
) -> Result<(), VmError> {
    #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
    {
        let _ = (eg, frame, opline);
        return Err(VmError::Fatal(
            "Generic default check emitted without generic runtime support".into(),
        ));
    }

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    {
        let parameter_index = opline.extended_value as usize;
        let value = unsafe { &*(*frame).cv(opline.op1 as u32) };

        #[cfg(feature = "php-generics-reified")]
        if let Some((scope_owner, binding)) =
            eg.active_reified_binding_scope(frame as usize)
        {
            let declaration = eg
                .generic_metadata
                .declaration(binding)
                .ok_or_else(|| VmError::Fatal("Invalid reified generic declaration".into()))?;
            if let Some(expected) = declaration
                .value_parameters
                .get(parameter_index)
                .and_then(Option::as_ref)
            {
                let declared_scope = eg
                    .generic_metadata
                    .symbol(declaration.owner)
                    .unwrap_or("?");
                if !value_matches_reified_default(
                    eg,
                    scope_owner,
                    value,
                    expected,
                    binding,
                    declared_scope,
                ) {
                    return Err(VmError::Fatal(format!(
                        "Default value for argument #{} of {} does not match its reified generic type",
                        parameter_index + 1,
                        declared_scope
                    )));
                }
            }
        }

        if let Some(contract) = eg.active_generic_member_call(frame as usize)
            && let Some(expected) = contract
                .value_parameters
                .get(parameter_index)
                .and_then(Option::as_ref)
            && !eg.value_matches_generic_method_contract(value, expected, contract)
        {
            return Err(VmError::Fatal(format!(
                "Default value for argument #{} of {}::{}() does not match its {}",
                parameter_index + 1,
                contract.owner,
                contract.method,
                generic_method_contract_kind(contract.runtime_mode)
            )));
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
        let declared_scope = eg
            .generic_metadata
            .symbol(declaration.owner)
            .unwrap_or("?");
        if let Some(expected) = declaration.return_type.as_ref() {
            let value = unsafe {
                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
            };
            let matches_return = matches!(expected, crate::generics::GenericType::Void)
                || eg.generic_metadata.value_matches_binding_reified(
                    value,
                    expected,
                    binding,
                    |actual, bound| {
                        let class_id = eg.reified_binding_scope_class_id(frame as usize);
                        let receiver_scope = eg
                            .class_by_id(class_id)
                            .map(|class| class.name.as_str());
                        let scope =
                            eg.generic_declaration_scope(declared_scope, receiver_scope);
                        eg.class_is_a_in_generic_scopes(actual, bound, scope, receiver_scope)
                    },
                    |value, name, arguments, declaration, site| {
                        let class_id = eg.reified_binding_scope_class_id(frame as usize);
                        let receiver_scope = eg
                            .class_by_id(class_id)
                            .map(|class| class.name.as_str());
                        let scope =
                            eg.generic_declaration_scope(declared_scope, receiver_scope);
                        eg.reified_object_arguments_match_binding(
                            value,
                            name,
                            arguments,
                            declaration,
                            site,
                            scope,
                            receiver_scope,
                        )
                    },
                );
            if !matches_return {
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
        eg.finish_reified_binding_scope(frame as usize);
        Ok(())
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(never)]
fn validate_generic_member_arguments(
    eg: &ExecutorGlobals,
    call: *mut ExecuteData,
    contract: &crate::generics::GenericMethodContract,
) -> Result<(), VmError> {
    let contract_kind = generic_method_contract_kind(contract.runtime_mode);
    let common = unsafe { &*(*call).func };
    let fixed = contract
        .value_parameters
        .len()
        .saturating_sub(usize::from(contract.is_variadic));
    for index in 0..fixed {
        let Some(expected) = contract
            .value_parameters
            .get(index)
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let value = unsafe { &*(*call).cv(common.sig.param_cv_index(index as u32)) };
        if value.is_undef() {
            continue;
        }
        if !eg.value_matches_generic_method_contract(value, expected, contract) {
            return Err(VmError::Fatal(format!(
                "Argument #{} passed to {}::{}() does not match its {}",
                index + 1,
                contract.owner,
                contract.method,
                contract_kind
            )));
        }
    }

    if contract.is_variadic {
        let expected = contract
            .value_parameters
            .last()
            .and_then(Option::as_ref);
        if let Some(expected) = expected {
            let public_max = common.sig.public_arity();
            let extra = unsafe { (*call).num_args }.saturating_sub(public_max);
            for index in 0..extra {
                let value = unsafe { &*(*call).cv(common.sig.variadic_cv_index + index) };
                if !eg.value_matches_generic_method_contract(value, expected, contract) {
                    return Err(VmError::Fatal(format!(
                        "Variadic argument #{} passed to {}::{}() does not match its {}",
                        fixed + index as usize + 1,
                        contract.owner,
                        contract.method,
                        contract_kind
                    )));
                }
            }
            if let Some(named) = eg.pending_named_variadic.get(&(call as usize)) {
                for (name, value) in named {
                    if !eg.value_matches_generic_method_contract(value, expected, contract) {
                        return Err(VmError::Fatal(format!(
                            "Named variadic argument ${} passed to {}::{}() does not match its {}",
                            name, contract.owner, contract.method, contract_kind
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(never)]
fn validate_generic_member_return(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    contract: &crate::generics::GenericMethodContract,
) -> Result<(), VmError> {
    let Some(expected) = contract.return_type.as_ref() else {
        return Ok(());
    };
    if matches!(expected, crate::generics::GenericType::Void) {
        return if opline.extended_value == 0 {
            Ok(())
        } else {
            Err(VmError::Fatal(format!(
                "Return value of {}::{}() does not match its {}",
                contract.owner,
                contract.method,
                generic_method_contract_kind(contract.runtime_mode)
            )))
        };
    }
    let implicit_null;
    let value = if opline.op1_type == OpType::Unused {
        implicit_null = Value::null();
        &implicit_null
    } else {
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }
    };
    if eg.value_matches_generic_method_contract(value, expected, contract) {
        return Ok(());
    }
    Err(VmError::Fatal(format!(
        "Return value of {}::{}() does not match its {}",
        contract.owner,
        contract.method,
        generic_method_contract_kind(contract.runtime_mode)
    )))
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[inline(always)]
fn generic_method_contract_kind(mode: crate::generics::GenericRuntimeMode) -> &'static str {
    match mode {
        crate::generics::GenericRuntimeMode::BoundErased => "linked generic class type",
        crate::generics::GenericRuntimeMode::Reified => "reified class type",
    }
}

#[inline(never)]
fn op_call_user_func_array<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and compiler-emitted operands.
    // Reference targets remain live through this non-reentrant call setup.
    let (callback, args) = unsafe {
        let callback = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let callback = if callback.is_reference() {
            &*callback.as_ref_ptr()
        } else {
            callback
        };
        let args = &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array);
        let args = if args.is_reference() {
            &*args.as_ref_ptr()
        } else {
            args
        };
        (callback, args)
    };
    let callback = if opline.extended_value != 0
        && callback
            .as_str()
            .is_some_and(|name| eg.find_function(name).is_none())
    {
        op_array
            .literals
            .get(opline.extended_value as usize)
            .unwrap_or(callback)
    } else {
        callback
    };

    // SAFETY: `opline` belongs to this op-array, whose cache has one stable
    // entry per instruction.
    let cache_slot = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache
    };
    let caller_class = get_caller_class(frame, eg);
    let result = if opline._pad & CALL_USER_FUNC_ARRAY_SOURCE_UNPACK != 0 {
        let source_file = if op_array.source_file.is_empty() {
            op_array.name.as_str()
        } else {
            op_array.source_file.as_str()
        };
        crate::stdlib::invoke_source_unpacked_call(
            callback,
            args,
            eg,
            caller_class.as_deref(),
            Some(cache_slot),
            source_file,
            op_array.strict_types,
        )?
    } else {
        crate::stdlib::invoke_call_user_func_array(
            callback,
            args,
            eg,
            caller_class.as_deref(),
            Some(cache_slot),
        )?
    };

    if let Some(exc) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exc) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    if opline.result_type != OpType::Unused {
        // SAFETY: the compiler emitted a writable result for this call and the
        // frame remains live until the opcode returns. CallUserFuncArray
        // results are fresh TMP slots, whose reused stack bytes are not
        // necessarily an initialized Value; use the frame-aware first-write
        // path instead of dropping stale bytes through slot_set().
        unsafe {
            let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            frame_tmp_set(frame, result_ptr, result);
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_fetch_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_static_prop_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_static_prop_impl::<true>(eg, frame, op_array, opline)
}

/// Static storage is commonly scalar. Copy that 16-byte representation
/// directly and reserve refcount/reference cloning for the uncommon heap path.
#[inline(always)]
fn clone_static_property_value(value: &Value) -> Value {
    if value.needs_cleanup() || value.is_reference() {
        clone_heap_static_property_value(value)
    } else {
        let mut cloned = std::mem::MaybeUninit::<Value>::uninit();
        unsafe {
            Value::raw_copy(value as *const Value, cloned.as_mut_ptr());
            cloned.assume_init()
        }
    }
}

/// Keep reference counting and reference-wrapper cloning out of scalar static
/// property dispatch. Inlining `Value::clone` here noticeably grows both
/// monomorphized static-property write handlers and perturbs their hot layout.
#[inline(never)]
fn clone_heap_static_property_value(value: &Value) -> Value {
    value.clone()
}

#[inline]
fn static_property_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    class: &str,
    message: String,
) -> ColdResult<'a> {
    let error = make_error_value(class, &message);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }
}

#[inline(always)]
fn op_fetch_static_prop_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies an instruction from this op-array and a live
    // frame with compiler-emitted operand/result slots. The cache has one
    // stable entry per instruction and execution owns its mutable access.
    let (class_name_val, prop_name_val, result_ptr, cache) = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
            &mut *(op_array.cache.as_ptr().add(ip)
                as *mut crate::vm::instruction::InlineCache),
        )
    };

    let raw_class = class_name_val.as_str().unwrap_or("");
    let prop = prop_name_val.as_str().unwrap_or("");
    let class_id = static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class);

    if class_id != 0 && cache.class_id == class_id && cache.property_flags() == 1 {
        // SAFETY: the class/cache guards prove the storage slot is valid; the
        // compiler-emitted result pointer was validated above.
        unsafe {
            let value = clone_static_property_value(
                eg.static_property_value_unchecked(cache.property_slot()),
            );
            frame_tmp_set(frame, result_ptr, value);
        }
        return Ok(ColdResult::Done);
    }

    resolve_static_property_read_cache_miss(
        eg,
        frame,
        result_ptr,
        cache,
        class_id,
        raw_class,
        prop,
        opline._pad & FETCH_OBJ_SILENT != 0,
    )
}

#[inline(always)]
fn static_property_class_id<const LATE_STATIC: bool>(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
    cache: &crate::vm::instruction::InlineCache,
    raw_class: &str,
) -> u32 {
    if LATE_STATIC {
        if opline._pad & LATE_STATIC_PROP_EMBEDDED_SCOPE != 0 {
            unsafe { ((*frame).heap_bitmap >> 32) as u32 }
        } else if raw_class.eq_ignore_ascii_case("parent") {
            eg.class_by_id(late_static_call_class_id(eg, frame))
                .and_then(|class| class.parent.as_deref())
                .map_or(0, |parent| eg.class_id_of(parent))
        } else if raw_class.eq_ignore_ascii_case("static")
            || raw_class.eq_ignore_ascii_case("self")
        {
            late_static_call_class_id(eg, frame)
        } else {
            eg.class_id_of(raw_class)
        }
    } else if cache.class_id != 0 && cache.property_flags() != 0 {
        cache.class_id
    } else {
        eg.class_id_of(raw_class)
    }
}

#[inline(never)]
fn op_fetch_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<true>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_dynamic_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_fetch_late_dynamic_class_const<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_fetch_class_const_impl::<true>(eg, frame, op_array, opline)
}

#[inline(always)]
fn op_fetch_class_const_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: dispatch supplies a live frame and an instruction from this
    // op-array; its operand kinds and writable result slot were emitted by the
    // compiler and remain valid until this opcode completes.
    let (raw_class_value, raw_constant_value, result_ptr) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    // SAFETY: a Reference-tagged operand was created from a live VM slot; the
    // class/name borrows do not outlive this opcode or mutate that slot.
    let (class_value, constant_value) = unsafe {
        (
            if raw_class_value.is_reference() {
                &*raw_class_value.as_ref_ptr()
            } else {
                raw_class_value
            },
            if raw_constant_value.is_reference() {
                &*raw_constant_value.as_ref_ptr()
            } else {
                raw_constant_value
            },
        )
    };
    let set_result = |value| {
        // SAFETY: `result_ptr` is the initialized writable result slot proven
        // above, and every call transfers exactly one owned Value into it.
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, value) };
    };
    let dynamic_owner = opline._pad & CLASS_CONST_DYNAMIC_OWNER != 0;
    let dynamic_name = opline._pad & CLASS_CONST_DYNAMIC_NAME != 0;
    let compile_time_name = opline._pad & CLASS_CONST_COMPILE_TIME_NAME != 0;
    let raw_class = class_value.as_str().unwrap_or("");
    let constant = constant_value.as_str();

    if dynamic_owner && class_value.as_object().is_none() && class_value.as_str().is_none() {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            "Class name must be a valid object or a string".to_string(),
        ));
    }
    if dynamic_owner
        && !dynamic_name
        && class_value.as_str().is_some()
        && constant.is_some_and(|name| name.eq_ignore_ascii_case("class"))
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            "Cannot use \"::class\" on string".to_string(),
        ));
    }
    if dynamic_owner
        && !dynamic_name
        && constant.is_some_and(|name| name.eq_ignore_ascii_case("class"))
        && let Some(object) = class_value.as_object()
    {
        let class_name = object.class_name.clone();
        drop(object);
        set_result(Value::string(class_name.to_string()));
        return Ok(ColdResult::Done);
    }
    let scoped_owner = raw_class.eq_ignore_ascii_case("self")
        || raw_class.eq_ignore_ascii_case("static")
        || raw_class.eq_ignore_ascii_case("parent");
    if class_value.as_object().is_none()
        && !raw_class.is_empty()
        && !scoped_owner
        && eg.find_class(raw_class).is_none()
    {
        let _ = crate::stdlib::autoload::ensure_symbol_loaded(eg, raw_class)?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
    }
    // SAFETY: `opline` belongs to this op-array, and cache has one stable entry
    // per instruction. Execution is single-threaded, so this opcode owns the
    // mutable cache access for the duration of the lookup.
    let cache = unsafe {
        let ip = (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        &mut *(op_array.cache.as_ptr().add(ip)
            as *mut crate::vm::instruction::InlineCache)
    };
    let class_id = if dynamic_owner && !LATE_STATIC {
        class_value
            .as_object()
            .map(|object| {
                if object.class_id == 0 {
                    eg.class_id_of(&object.class_name)
                } else {
                    object.class_id
                }
            })
            .unwrap_or_else(|| eg.class_id_of(raw_class))
    } else {
        static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class)
    };

    let Some(class) = eg.class_by_id(class_id) else {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!("Class \"{}\" not found", raw_class),
        ));
    };
    let Some(constant) = constant else {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            format!(
                "Cannot use value of type {} as class constant name",
                constant_value.type_name()
            ),
        ));
    };
    if constant.eq_ignore_ascii_case("class") && (!dynamic_name || !compile_time_name) {
        set_result(Value::string(class.name.clone()));
        return Ok(ColdResult::Done);
    }

    if class_id != 0 && cache.class_id == class_id && cache.property_flags() == 1 {
        let class = eg
            .class_by_id(class_id)
            .expect("cached class constant owner must stay registered");
        let definition = class
            .constants
            .get(cache.property_slot())
            .expect("cached class constant index must stay valid");
        if !dynamic_name || definition.name == constant {
            set_result(definition.value.clone());
            return Ok(ColdResult::Done);
        }
    }
    if !dynamic_name && class_id != 0 && cache.class_id == class_id && cache.property_flags() == 2 {
        let stored = eg
            .static_property_value(cache.property_slot())
            .expect("cached enum-case storage slot must stay valid");
        let value = clone_static_property_value(stored);
        set_result(value);
        return Ok(ColdResult::Done);
    }

    if class.is_trait {
        let message = format!("Cannot access trait constant {}::{} directly", class.name, constant);
        return Ok(static_property_throw(eg, frame, "Error", message));
    }
    let display_class = class.name.clone();
    let Some((constant_index, definition)) = eg.find_class_constant(class_id, constant) else {
        // Enum cases occupy immutable static storage but share PHP's
        // `Enum::Case` syntax with class constants. Preserve their existing
        // representation while keeping a distinct cache tag.
        if class.is_enum
            && let Some(case_index) = class
                .static_properties
                .iter()
                .position(|case| case.name == constant)
            && let Some(storage_slot) = eg.static_property_storage_slot(class_id, case_index)
        {
            let stored = eg
                .static_property_value(storage_slot)
                .expect("resolved enum-case storage slot must stay valid");
            let value = clone_static_property_value(stored);
            cache.set_property(class_id, storage_slot, 2);
            set_result(value);
            return Ok(ColdResult::Done);
        }
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!("Undefined constant {}::{}", display_class, constant),
        ));
    };
    let caller = get_caller_class(frame, eg);
    if !eg.check_visibility(
        caller.as_deref(),
        &definition.declaring_class,
        definition.visibility,
    ) {
        let visibility = match definition.visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Cannot access {} constant {}::{}",
                visibility, display_class, constant
            ),
        ));
    }
    if let Some(message) = &definition.evaluation_error {
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            message.clone(),
        ));
    }
    let value = definition.value.clone();
    cache.set_property(class_id, constant_index, 1);
    set_result(value);
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_assign_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_assign_static_prop_impl::<false>(eg, frame, op_array, opline)
}

#[inline(never)]
fn op_assign_late_static_prop<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    op_assign_static_prop_impl::<true>(eg, frame, op_array, opline)
}

#[inline(always)]
fn op_assign_static_prop_impl<'a, const LATE_STATIC: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // Compact late-static frames already carry the called class ID. Check the
    // monomorphic untyped cache before decoding the two constant string
    // operands; a cache miss still takes the canonical resolver below.
    if LATE_STATIC && opline._pad & LATE_STATIC_PROP_EMBEDDED_SCOPE != 0 {
        let ip = unsafe {
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
        };
        let cache = unsafe {
            &*(op_array.cache.as_ptr().add(ip) as *const crate::vm::instruction::InlineCache)
        };
        let class_id = unsafe { ((*frame).heap_bitmap >> 32) as u32 };
        let flags = cache.property_flags();
        let exact_int = flags == 1
            && cache.typed_static_property_tag()
                == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT;
        if class_id != 0 && cache.class_id == class_id && (flags == 3 || exact_int) {
            let source = unsafe {
                &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
            };
            let source = if source.is_reference() {
                unsafe { &*source.as_ref_ptr() }
            } else {
                source
            };
            let value = clone_static_property_value(source);
            if flags == 3 || value.value_type() == ValueType::Long {
                unsafe { eg.set_static_property_value_unchecked(cache.property_slot(), value) };
                return Ok(ColdResult::Done);
            }
        }
    }

    let class_name =
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let property_name =
        unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
    };
    let source = if source.is_reference() {
        unsafe { &*source.as_ref_ptr() }
    } else {
        source
    };
    let mut value = clone_static_property_value(source);
    let raw_class = class_name.as_str().unwrap_or("");
    let property = property_name.as_str().unwrap_or("");
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = unsafe {
        &mut *(op_array.cache.as_ptr().add(ip)
            as *mut crate::vm::instruction::InlineCache)
    };
    let class_id = static_property_class_id::<LATE_STATIC>(eg, frame, opline, cache, raw_class);
    if class_id != 0 && cache.class_id == class_id {
        if cache.property_flags() == 3 {
            unsafe { eg.set_static_property_value_unchecked(cache.property_slot(), value) };
            return Ok(ColdResult::Done);
        }
        if cache.property_flags() == 1 {
            let tag = cache.typed_static_property_tag();
            let value_type = value.value_type();
            if tag == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT
                && value_type == ValueType::Long
            {
                unsafe {
                    eg.set_static_property_value_unchecked(cache.property_slot(), value)
                };
                return Ok(ColdResult::Done);
            }
            #[cfg(feature = "php-generics-reified")]
            let reified_contract = if tag
                == crate::vm::instruction::InlineCache::TYPED_PROPERTY_REIFIED
            {
                cache.reified_static_property_contract()
            } else {
                std::ptr::null()
            };
            #[cfg(feature = "php-generics-reified")]
            if !reified_contract.is_null()
                && unsafe {
                    eg.static_generic_property_contract_remembers(reified_contract, &value)
                }
            {
                unsafe {
                    eg.set_static_property_value_unchecked(cache.property_slot(), value)
                };
                return Ok(ColdResult::Done);
            }
            if tag == crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                && value_type == ValueType::Long
            {
                value = Value::double(value.as_long().unwrap() as f64);
            }
            let fast_match = match tag {
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                    if matches!(value_type, ValueType::Double | ValueType::Long) => true,
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_STRING
                    if value_type == ValueType::String => true,
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_BOOL
                    if matches!(value_type, ValueType::True | ValueType::False) =>
                {
                    true
                }
                crate::vm::instruction::InlineCache::TYPED_PROPERTY_ARRAY
                    if value_type == ValueType::Array => true,
                _ => false,
            };
            if fast_match {
                unsafe {
                    eg.set_static_property_value_unchecked(cache.property_slot(), value)
                };
                return Ok(ColdResult::Done);
            }
            return validate_cached_typed_static_property(
                eg,
                frame,
                op_array,
                cache,
                class_id,
                raw_class,
                value,
            );
        }
    }

    assign_static_property_cache_miss(
        eg,
        frame,
        op_array,
        cache,
        class_id,
        raw_class,
        property,
        value,
    )
}

#[inline(never)]
fn validate_cached_typed_static_property<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    mut value: Value,
) -> Result<ColdResult<'a>, VmError> {
    #[cfg(feature = "php-generics-reified")]
    let reified_contract = if cache.typed_static_property_tag()
        == crate::vm::instruction::InlineCache::TYPED_PROPERTY_REIFIED
    {
        cache.reified_static_property_contract()
    } else {
        std::ptr::null()
    };
    #[cfg(feature = "php-generics-reified")]
    let definition = if reified_contract.is_null() {
        cache
            .typed_static_property_definition()
            .expect("typed static cache must retain its definition")
    } else {
        // SAFETY: tag 6 was published from an executor-owned boxed contract
        // and remains stable until executor teardown.
        unsafe { &*eg.static_generic_property_contract_definition(reified_contract) }
    };
    #[cfg(not(feature = "php-generics-reified"))]
    let definition = cache
        .typed_static_property_definition()
        .expect("typed static cache must retain its definition");
    let called_class = eg
        .class_by_id(class_id)
        .map_or(raw_class, |class| class.name.as_str());
    value = match prepare_property_assignment(
        value,
        definition,
        eg,
        op_array.strict_types,
        called_class,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            ));
        }
    };
    #[cfg(feature = "php-generics-reified")]
    if definition.requires_reified_check
        && let Err(message) = eg.check_reified_static_property_value(
            called_class,
            &definition.name,
            &value,
        )
    {
        return Ok(static_property_throw(
            eg,
            frame,
            "TypeError",
            message,
        ));
    }
    #[cfg(feature = "php-generics-reified")]
    if !reified_contract.is_null() {
        unsafe { eg.remember_static_generic_property_contract(reified_contract, &value) };
    }
    unsafe { eg.set_static_property_value_unchecked(cache.property_slot(), value) };
    Ok(ColdResult::Done)
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn assign_static_property_cache_miss<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    property: &str,
    mut value: Value,
) -> Result<ColdResult<'a>, VmError> {
    let resolved = resolve_static_property(eg, frame, class_id, raw_class, property, true)?;
    let definition = unsafe { &*resolved.definition };
    if definition.is_typed() {
        let called_class = eg
            .class_by_id(class_id)
            .map_or(raw_class, |class| class.name.as_str());
        value = match prepare_property_assignment(
            value,
            definition,
            eg,
            op_array.strict_types,
            called_class,
        ) {
            Ok(value) => value,
            Err(message) => {
                return Ok(static_property_throw(
                    eg,
                    frame,
                    "TypeError",
                    message,
                ));
            }
        };
        #[cfg(feature = "php-generics-reified")]
        if definition.requires_reified_check
            && let Err(message) = eg.check_reified_static_property_value(
                called_class,
                &definition.name,
                &value,
            )
        {
            return Ok(static_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            ));
        }
    }
    #[cfg(feature = "php-generics-reified")]
    let reified_contract = if definition.requires_reified_check {
        eg.cache_static_generic_property_contract(resolved.definition, &value)
    } else {
        std::ptr::null()
    };
    if !eg.set_static_property_value(resolved.storage_slot, value) {
        return Err(VmError::Fatal("Invalid static property storage slot".into()));
    }
    #[cfg(feature = "php-generics-reified")]
    if !reified_contract.is_null() {
        cache.set_reified_static_property(reified_contract, class_id, resolved.storage_slot);
    } else if definition.is_typed() {
        cache.set_typed_static_property(definition, class_id, resolved.storage_slot);
    } else {
        cache.set_property(class_id, resolved.storage_slot, 3);
    }
    #[cfg(not(feature = "php-generics-reified"))]
    if definition.is_typed() {
        cache.set_typed_static_property(definition, class_id, resolved.storage_slot);
    } else {
        cache.set_property(class_id, resolved.storage_slot, 3);
    }
    Ok(ColdResult::Done)
}

#[cold]
#[inline(never)]
fn resolve_static_property_read_cache_miss<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    result_ptr: *mut Value,
    cache: &mut crate::vm::instruction::InlineCache,
    class_id: u32,
    raw_class: &str,
    property: &str,
    silent: bool,
) -> Result<ColdResult<'a>, VmError> {
    let resolved =
        resolve_static_property(eg, frame, class_id, raw_class, property, false)?;
    let stored = eg
        .static_property_value(resolved.storage_slot)
        .ok_or_else(|| VmError::Fatal("Invalid static property storage slot".into()))?;
    if stored.is_undef() {
        if silent {
            // SAFETY: result_ptr is the compiler-owned output slot for this
            // live frame and has been prepared for one result write.
            unsafe { frame_tmp_set(frame, result_ptr, Value::null()) };
            return Ok(ColdResult::Done);
        }
        let definition = unsafe { &*resolved.definition };
        return Ok(static_property_throw(
            eg,
            frame,
            "Error",
            format!(
                "Typed static property {}::${} must not be accessed before initialization",
                definition.declaring_class, definition.name
            ),
        ));
    }
    let value = clone_static_property_value(stored);
    cache.set_property(class_id, resolved.storage_slot, 1);
    unsafe { frame_tmp_set(frame, result_ptr, value) };
    Ok(ColdResult::Done)
}

struct ResolvedStaticProperty {
    storage_slot: usize,
    definition: *const crate::compiler::compile::PropertyDefinition,
}

#[cold]
#[inline(never)]
fn resolve_static_property(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    class_id: u32,
    raw_class: &str,
    property: &str,
    for_write: bool,
) -> Result<ResolvedStaticProperty, VmError> {
    let class = eg.class_by_id(class_id).ok_or_else(|| {
        VmError::Fatal(format!("Class \"{}\" not found", raw_class))
    })?;
    let Some((property_index, definition)) = class
        .static_properties
        .iter()
        .enumerate()
        .find(|(_, definition)| definition.name == property)
    else {
        return Err(VmError::Fatal(format!(
            "Access to undeclared static property {}::${}",
            class.name, property
        )));
    };
    if for_write && class.is_enum {
        return Err(VmError::Fatal(format!(
            "Cannot modify readonly property {}::${}",
            class.name, property
        )));
    }
    let caller = get_caller_class(frame, eg);
    if !eg.check_visibility(
        caller.as_deref(),
        &definition.declaring_class,
        definition.visibility,
    ) {
        let visibility = match definition.visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return Err(VmError::Fatal(format!(
            "Cannot access {} property {}::${}",
            visibility, definition.declaring_class, property
        )));
    }
    let storage_slot = eg
        .static_property_storage_slot(class_id, property_index)
        .ok_or_else(|| VmError::Fatal("Invalid static property storage mapping".into()))?;
    Ok(ResolvedStaticProperty {
        storage_slot,
        definition,
    })
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
    let raw_target = class_name.as_str().unwrap_or("");
    let dynamic_target = (opline._pad & INSTANCEOF_DYNAMIC_STATIC_SCOPE != 0)
        .then(|| resolve_static_call_class(eg, frame, raw_target, true))
        .flatten();
    let target = dynamic_target.as_deref().unwrap_or(raw_target);
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    let is_instance = if obj_val.value_type() == ValueType::Closure {
        eg.class_is_a("Closure", target)
    } else {
        obj_val
            .as_object()
            .is_some_and(|object| eg.class_is_a(&object.class_name, target))
    };
    unsafe { frame_result_set(frame, result_ptr, opline.result_type, Value::bool(is_instance)) };
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
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, value) };
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
    // SAFETY: the compiler validated the global-name operand for this live
    // frame and its op array before dispatch reached this opcode.
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let name = name_val.as_str().unwrap_or("").to_string();
    let cv_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, OpType::Cv) };
    // At top level `global $name` binds the symbol table to the CV that is
    // already the same global-scope variable. Do not replace an initialized
    // value with null merely because the detached globals snapshot has not
    // been populated yet. A function-local CV, by contrast, must discard its
    // prior local value and acquire the global binding.
    if !op_array.main_scope_vars.is_empty() && !unsafe { (*cv_ptr).is_undef() } {
        return;
    }
    let value = eg.globals.get(&name).cloned().unwrap_or_else(Value::null);
    unsafe { slot_set(cv_ptr, value) };
}

#[inline(never)]
fn set_global_snapshot_entry(snapshot: &mut PhpArray, name: &str, value: Value) {
    if let Some(key) = canonical_decimal_array_key(name) {
        snapshot.set_int(key, value);
    } else {
        snapshot.set_str(name, value);
    }
}

#[inline(never)]
fn op_global_dimension(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let scope_vars = if !op_array.main_scope_vars.is_empty() {
        &op_array.main_scope_vars
    } else {
        &op_array.global_vars
    };

    // SAFETY: each dedicated global opcode is emitted only with validated
    // operands belonging to this live frame/op-array pair. CV indices in
    // scope metadata come from the same compiler allocation, and every slot
    // replacement goes through the frame bitmap helpers before old owners
    // are dropped.
    unsafe {
        if opline.opcode == OpCode::FetchGlobals {
            let mut snapshot = PhpArray::with_hash_capacity(eg.globals.len() + scope_vars.len());

            for (name, value) in &eg.globals {
                if name != "GLOBALS" && value.value_type() != ValueType::Undef {
                    set_global_snapshot_entry(&mut snapshot, name, value.clone());
                }
            }
            for (cv, name) in scope_vars {
                if name == "GLOBALS" {
                    continue;
                }
                let value = (&*(*frame).get_op_ptr(*cv, OpType::Cv, op_array)).clone();
                if value.value_type() == ValueType::Undef {
                    let key = canonical_decimal_array_key(name)
                        .map(ArrayKey::Int)
                        .unwrap_or_else(|| ArrayKey::String(name.clone()));
                    snapshot.remove(&key);
                } else {
                    set_global_snapshot_entry(&mut snapshot, name, value);
                }
            }

            let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            write_fetch_dim_result(frame, result, Value::array(snapshot));
            return Ok(());
        }

        let key = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let name = value_to_global_name(key)?;
        match opline.opcode {
            OpCode::FetchGlobal => {
                let local = scope_vars
                    .iter()
                    .find(|(_, variable)| variable == &name)
                    .map(|(cv, _)| {
                        (&*(*frame).get_op_ptr(*cv, OpType::Cv, op_array)).clone()
                    });
                let value = local
                    .or_else(|| eg.globals.get(&name).cloned())
                    .unwrap_or_else(Value::undef);
                let value = if opline._pad & FETCH_DIM_ISSET != 0 {
                    Value::bool(!matches!(
                        value.value_type(),
                        ValueType::Null | ValueType::Undef
                    ))
                } else {
                    value
                };
                let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
                write_fetch_dim_result(frame, result, value);
            }
            OpCode::AssignGlobal => {
                let value =
                    (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone();
                globals_assign(&mut eg.globals, &name, value.clone());
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    let is_reference = (*frame).cv(*cv).is_reference();
                    let destination = (*frame).get_op_mut(*cv, OpType::Cv);
                    if is_reference {
                        slot_set(destination, value);
                    } else {
                        frame_slot_set(frame, destination, value);
                    }
                }
            }
            OpCode::UnsetGlobal => {
                globals_set(&mut eg.globals, &name, Value::undef());
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    frame_slot_set(frame, (*frame).cv_mut(*cv), Value::undef());
                }
            }
            OpCode::BindGlobalRef => {
                let current_cv = scope_vars
                    .iter()
                    .find(|(_, variable)| variable == &name)
                    .map(|(cv, _)| *cv);
                let binding = if let Some(cv) = current_cv {
                    let slot = (*frame).cv_mut(cv);
                    if slot.is_owned_reference() {
                        slot.clone_owned_reference_alias()
                    } else {
                        let owned = Value::owned_reference(reference_initial_value(slot.clone()));
                        let alias = owned.clone_owned_reference_alias();
                        frame_slot_set(frame, slot, owned);
                        alias
                    }
                } else if let Some(value) = eg.globals.get(&name) {
                    if value.is_owned_reference() {
                        value.clone_owned_reference_alias()
                    } else {
                        Value::owned_reference(reference_initial_value(value.clone()))
                    }
                } else {
                    Value::owned_reference(Value::null())
                };
                globals_set(
                    &mut eg.globals,
                    &name,
                    binding.clone_owned_reference_alias(),
                );
                frame_slot_set(
                    frame,
                    (*frame).cv_mut(opline.result as u32),
                    binding.clone_owned_reference_alias(),
                );
            }
            OpCode::AssignGlobalRef => {
                let source = (*frame).cv_mut(opline.op2 as u32);
                let binding = if source.is_owned_reference() {
                    source.clone_owned_reference_alias()
                } else {
                    let owned = Value::owned_reference(reference_initial_value(source.clone()));
                    let alias = owned.clone_owned_reference_alias();
                    frame_slot_set(frame, source, owned);
                    alias
                };
                globals_set(
                    &mut eg.globals,
                    &name,
                    binding.clone_owned_reference_alias(),
                );
                eg.dirty_globals.insert(name.clone());
                if let Some((cv, _)) = scope_vars.iter().find(|(_, variable)| variable == &name) {
                    frame_slot_set(
                        frame,
                        (*frame).cv_mut(*cv),
                        binding.clone_owned_reference_alias(),
                    );
                }
            }
            _ => unreachable!("op_global_dimension called for a non-global opcode"),
        }
    }
    Ok(())
}

#[inline(never)]
fn op_bind_static(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    // SAFETY: the compiler validated the static-name operand for this live
    // frame and its op array before dispatch reached this opcode.
    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    let var_name = name_val.as_str().unwrap_or("").to_string();
    let func_name = op_array.literals[opline.extended_value as usize]
        .as_str()
        .unwrap_or("")
        .to_string();
    // PHP lets a later declaration of the same static replace its initializer
    // during the frame that first creates the request-owned cell. Recursive
    // and subsequent calls bind the existing cell without reinitializing it.
    // SAFETY: `BindStatic` carries validated CV and default-value operands for
    // this live frame and op array. The raw CV is intentionally not resolved
    // through `get_op_mut`: rebinding replaces its reference wrapper. When the
    // frame-local marker is present, it proves that wrapper owns a live,
    // distinct Rc-backed target whose initialized Value `slot_set` replaces.
    let cv_ptr = unsafe {
        let cv_ptr = (*frame).cv_mut(opline.op1 as u32) as *mut Value;
        if (*cv_ptr).is_local_static_initializer() {
            let initial = if opline.result_type != OpType::Unused {
                (&*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)).clone()
            } else {
                Value::null()
            };
            slot_set((*cv_ptr).as_ref_ptr(), initial);
            return;
        }
        cv_ptr
    };

    let statics = eg.static_vars.entry(func_name).or_default();
    let (mut binding, created) = if let Some(binding) = statics.get(&var_name) {
        if binding.is_owned_reference() {
            (binding.clone_owned_reference_alias(), false)
        } else {
            let binding = Value::owned_reference(binding.clone());
            statics.insert(var_name.clone(), binding.clone_owned_reference_alias());
            (binding, false)
        }
    } else {
        let initial = if opline.result_type != OpType::Unused {
            // SAFETY: the default-value operand was validated with the op array;
            // the frame and literal storage outlive this dispatch step.
            unsafe {
                (&*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)).clone()
            }
        } else {
            Value::null()
        };
        let binding = Value::owned_reference(initial);
        statics.insert(var_name, binding.clone_owned_reference_alias());
        (binding, true)
    };
    if created {
        binding.mark_local_static_initializer();
    }
    // SAFETY: `cv_ptr` is the initialized live CV slot resolved above, and
    // `slot_set` replaces its value without retaining the raw pointer.
    unsafe { slot_set(cv_ptr, binding) };
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
    let common = unsafe { &*func_ptr };
    let called_scope_class_id = if common.plan.needs_late_static_scope() {
        late_static_call_class_id(eg, frame)
    } else if opline.op2_type == OpType::Const {
        op_array.literals[opline.op2 as usize]
            .as_str()
            .map_or(0, |class| eg.class_id_of(class))
    } else {
        get_caller_class(frame, eg)
            .as_deref()
            .map_or(0, |class| eg.class_id_of(class))
    };
    let is_static = (opline._pad & crate::vm::instruction::CLOSURE_FLAG_STATIC) != 0;
    let bound_this = closure_bound_this(frame, op_array, is_static);
    let closure = PhpClosure {
        func: func_ptr,
        called_scope_class_id,
        is_static,
        bound_this,
        captures: Vec::with_capacity(opline.extended_value as usize),
        has_heap_captures: false,
    };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    unsafe { frame_tmp_set(frame, result_ptr, Value::closure(closure)) };
}

#[inline(never)]
fn op_create_first_class_callable<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: opline operands and result identify compiler-allocated slots in
    // this live frame; the read is cloned before callback resolution mutates VM state.
    let callable = unsafe {
        (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)).clone()
    };
    let caller_class = get_caller_class(frame, eg);
    let resolved = crate::stdlib::resolve_callback_with_cache(
        &callable,
        eg,
        caller_class.as_deref(),
        None,
    )
    .or_else(|| {
        if opline.extended_value == 0 {
            return None;
        }
        let fallback = &op_array.literals[opline.extended_value as usize];
        crate::stdlib::resolve_callback_with_cache(
            fallback,
            eg,
            caller_class.as_deref(),
            None,
        )
    });
    let Some(resolved) = resolved else {
        let error = make_error_value("TypeError", "Failed to create closure from callable");
        return Ok(match throw_in_frame(eg, frame, error) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    };

    let bound_this = resolved.bound_this.or_else(|| {
        resolved
            .prepend_args
            .first()
            .filter(|value| value.value_type() == ValueType::Object)
            .cloned()
    });
    let has_heap_captures = resolved.use_vars.iter().any(Value::needs_cleanup);
    let closure = PhpClosure {
        func: resolved.func_ptr,
        called_scope_class_id: resolved.called_scope_class_id,
        is_static: bound_this.is_none(),
        bound_this,
        captures: resolved.use_vars,
        has_heap_captures,
    };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    // SAFETY: result_ptr is the prepared compiler-owned result slot and is
    // initialized exactly once with the newly owned closure.
    unsafe { frame_tmp_set(frame, result_ptr, Value::closure(closure)) };
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_closure_use_var(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) {
    let cloned_value = if opline._pad & crate::vm::instruction::CLOSURE_USE_REFERENCE != 0 {
        // Closure use variables are compiler-guaranteed CVs. Promote an
        // ordinary local to a request-owned cell so both the active frame and
        // every closure copy can retain it after this frame returns.
        let source = unsafe { (*frame).cv_mut(opline.op2 as u32) as *mut Value };
        unsafe {
            if (*source).is_owned_reference() {
                (*source).clone_owned_reference_alias()
            } else if (*source).is_reference() {
                Value::reference((*source).as_ref_ptr())
            } else {
                let current = reference_initial_value(std::mem::replace(
                    &mut *source,
                    Value::undef(),
                ));
                let binding = Value::owned_reference(current);
                frame_slot_set(frame, source, binding.clone_owned_reference_alias());
                binding
            }
        }
    } else {
        let value = unsafe {
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
        };
        value.clone()
    };
    let closure_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
    // SAFETY: ClosureUseVar targets the live CreateClosure result TMP, which
    // remains initialized and exclusively owned during this bytecode sequence.
    unsafe { &mut *closure_ptr }.push_closure_capture(cloned_value);
}
