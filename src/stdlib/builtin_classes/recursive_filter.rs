//! Native callback/recursive policies over the existing FilterIterator loop.
//! Callback values are ordinary traced edges, never synthetic PHP objects.
use super::*;
use crate::value::NativeFilterCallback;
use std::rc::Rc;

#[cold]
fn error(eg: &mut ExecutorGlobals, kind: &str, message: &str) {
    eg.exception = Some(make_error_value(kind, message));
}

#[cold]
fn invocation_snapshot(callable: &Value) -> Option<Value> {
    let array = callable.as_array()?;
    if !array
        .values()
        .any(|v| v.is_reference() || v.is_owned_reference())
    {
        return None;
    }
    let mut snapshot = array.clone();
    for index in 0..array.len() {
        let value = array.get_value_at(index).unwrap();
        if value.is_reference() || value.is_owned_reference() {
            snapshot.set_value_at(index, value.dereferenced().clone());
        }
    }
    Some(Value::array(snapshot))
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn construct(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    owner: &'static str,
    callback: bool,
    recursive: bool,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    let inner = owned_argument(ed, 1);
    if !regex_iterator::validate_inner(
        &receiver,
        &inner,
        eg,
        owner,
        if recursive {
            "RecursiveIterator"
        } else {
            "Iterator"
        },
    ) {
        return Ok(());
    }
    let configuration = if callback {
        let callable = owned_argument(ed, 2).dereferenced().clone();
        // Freeze nested reference members before a legacy diagnostic can run
        // PHP. The same frozen callable is passed to child constructors.
        let callable = invocation_snapshot(&callable).unwrap_or(callable);
        let target = &callable;
        let lexical_class = if target.as_array().is_some()
            || target.as_str().is_some_and(|name| name.contains("::"))
        {
            crate::vm::execute::lexical_class_name_for_internal_call(eg, ed)
        } else {
            None
        };
        let legacy = callback_uses_legacy_scope(target);
        let resolved = resolve_callback_at_callsite_checked(target, eg, ed)?;
        let Some(resolved) = resolved else {
            if eg.exception.is_none() {
                let reason = ordinary_callback_invalid_reason(target, eg);
                error(
                    eg,
                    "TypeError",
                    &format!(
                        "{owner}::__construct(): Argument #2 ($callback) must be a valid callback, {reason}"
                    ),
                );
            }
            return Ok(());
        };
        let called_class = legacy
            .then(|| {
                eg.class_by_id(resolved.called_scope_class_id)
                    .map(|c| c.name.clone())
            })
            .flatten();
        let legacy_receiver = legacy
            .then(|| {
                resolved
                    .prepend_args
                    .first()
                    .filter(|v| v.as_object().is_some())
                    .cloned()
            })
            .flatten();
        Some(Box::new(NativeFilterCallback {
            callable,
            lexical_class,
            called_class,
            legacy_receiver,
            legacy,
        }))
    } else {
        None
    };
    regex_iterator::install(&receiver, inner, None, eg);
    receiver
        .as_object_mut()
        .unwrap()
        .native_iterator_delegate_mut()
        .unwrap()
        .callback = configuration;
    ret!(rv, Value::null());
}

macro_rules! constructor {
    ($handler:ident, $owner:literal, $callback:expr, $recursive:expr) => {
        #[cold]
        fn $handler(
            ed: *mut ExecuteData,
            rv: *mut Value,
            eg: &mut ExecutorGlobals,
        ) -> Result<(), VmError> {
            construct(ed, rv, eg, $owner, $callback, $recursive)
        }
    };
}
constructor!(construct_callback, "CallbackFilterIterator", true, false);
constructor!(construct_recursive, "RecursiveFilterIterator", false, true);
constructor!(
    construct_recursive_callback,
    "RecursiveCallbackFilterIterator",
    true,
    true
);
constructor!(construct_parent, "ParentIterator", false, true);

/// Resolution here cannot autoload or invoke PHP. The constructor already
/// validated the retained snapshot and captured its lexical/legacy scope.
#[cold]
fn resolved_callback(
    state: &NativeFilterCallback,
    eg: &ExecutorGlobals,
) -> Option<ResolvedCallback> {
    let callable = &state.callable;
    if state.legacy {
        match resolve_legacy_callback(
            callable,
            eg,
            state.lexical_class.as_deref(),
            state.called_class.as_deref(),
            state.legacy_receiver.as_ref(),
        ) {
            LegacyCallbackResolution::Legacy { resolved, .. } => resolved,
            LegacyCallbackResolution::NotLegacy => None,
        }
    } else {
        resolve_callback_with_cache(callable, eg, state.lexical_class.as_deref(), None)
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn prepare_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    arguments: &mut [Value; 3],
    retirement_exception: &mut Option<Value>,
) -> Result<bool, VmError> {
    let (common, user) = resolved.metadata();
    let Some(user) = user else { return Ok(true) };
    // A trampoline receives its own method-name/argument-array envelope.
    if resolved.is_magic_call || common.sig.param_type_hints.is_empty() {
        return Ok(true);
    }
    let scope = eg.declaring_class_of(resolved.func_ptr).map(str::to_owned);
    for index in 0..arguments.len() {
        let parameter = if common.sig.is_variadic {
            index.min(common.sig.public_arity() as usize)
        } else {
            index
        };
        let Some(hint) = common.sig.param_type_hints.get(parameter) else {
            continue;
        };
        if matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed) {
            continue;
        }
        let source = arguments[index].dereferenced().clone();
        match prepare_call_argument(&source, hint, eg, false, scope.as_deref())? {
            CallArgumentPreparation::Exact => {}
            CallArgumentPreparation::Coerced(prepared, diagnostic) => {
                if let Some(diagnostic) = diagnostic {
                    crate::vm::execute::report_scalar_coercion_diagnostic_at(
                        eg,
                        ed,
                        &source,
                        diagnostic,
                        &user.source_file,
                        user.declaration_line().unwrap_or(0),
                    )?;
                }
                if eg.exception.is_some() {
                    return Ok(false);
                }
                if common.sig.is_param_by_ref(parameter as u32) && arguments[index].is_reference() {
                    arguments[index].assign_dereferenced(prepared);
                } else {
                    let previous = std::mem::replace(&mut arguments[index], prepared);
                    iterator_delegate::discard(previous, eg)?;
                }
                // __toString() may remove the original cached/inner value.
                // This snapshot can then own its final PHP reference.
                iterator_delegate::discard(source, eg)?;
                // PHP finishes the callback after a converted argument's
                // destructor throws, then restores that first exception.
                if let Some(exception) = eg.exception.take() {
                    if retirement_exception.is_none() {
                        *retirement_exception = Some(exception);
                    }
                }
            }
            CallArgumentPreparation::Invalid => {
                if eg.exception.is_some() {
                    return Ok(false);
                }
                let parameter_name = common
                    .sig
                    .param_names
                    .get(parameter)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                let error = make_error_value(
                    "TypeError",
                    &format!(
                        "{}(): Argument #{} (${parameter_name}) must be of type {}, {} given",
                        displayed_function_name(eg, resolved.func_ptr),
                        index + 1,
                        hint.diagnostic_display_name(),
                        source.diagnostic_type_name(),
                    ),
                );
                // Materialize the rejected callee through the existing cold
                // entry so declaration origin, trace and argument retirement
                // follow ordinary detached-call rules without running its body.
                crate::vm::execute::call_function_owned_iter_with_context_and_named_from(
                    eg,
                    ed,
                    resolved.func_ptr,
                    resolved.prepend_args.len() + arguments.len() + resolved.use_vars.len(),
                    resolved
                        .prepend_args
                        .iter()
                        .cloned()
                        .chain(arguments.iter().map(Value::clone_for_php_storage))
                        .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
                    resolved.called_scope_class_id,
                    resolved.closure_scope_class_id,
                    resolved.bound_this.clone(),
                    resolved.use_vars.len(),
                    resolved.closure_static_vars.clone(),
                    Vec::new(),
                    false,
                    ("Unknown".into(), 0),
                    Some(&error),
                    false,
                )?;
                eg.exception = Some(error);
                return Ok(false);
            }
        }
    }
    Ok(eg.exception.is_none())
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn accept(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        return Ok(());
    }
    let request = {
        let object = receiver.as_object().unwrap();
        let state = object.native_iterator_delegate().unwrap();
        if state.current.is_undef() {
            ret!(rv, Value::bool(false));
        }
        state.callback.as_ref().and_then(|callback| {
            resolved_callback(callback, eg).map(|resolved| {
                let display = callback_has_hard_reference_parameters(&resolved)
                    .then(|| callable_display_name(&callback.callable, eg));
                (
                    resolved,
                    [
                        state.current.clone_for_php_storage(),
                        state.key.clone_for_php_storage(),
                        state.inner.clone(),
                    ],
                    display,
                )
            })
        })
    };
    let Some((resolved, mut arguments, display)) = request else {
        error(
            eg,
            "Error",
            "The object is in an invalid state as the parent constructor was not called",
        );
        return Ok(());
    };
    let result = invoke(ed, eg, &resolved, &mut arguments, display.as_deref());
    for argument in arguments {
        discard_after_call(argument, eg)?;
    }
    ret!(rv, result?);
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn invoke(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    arguments: &mut [Value; 3],
    display: Option<&str>,
) -> Result<Value, VmError> {
    if let Some(display) = display {
        let mut values = PhpArray::new();
        for value in arguments.iter() {
            values.push(value.clone_for_php_storage());
        }
        if !report_callback_reference_warnings(eg, ed, resolved, &values, false, display)? {
            return Ok(Value::null());
        }
    }
    let mut retirement_exception = None;
    let prepared = prepare_arguments(ed, eg, resolved, arguments, &mut retirement_exception);
    let result = match prepared {
        Err(error) => Err(error),
        Ok(false) => Ok(Value::null()),
        Ok(true) if display.is_some() => {
            // Retain explicit aliases beneath the live native callback frame.
            // Ordinary callbacks keep the existing scalar-proven entry.
            call_array_walk_resolved_owned_iter(
                ed,
                eg,
                resolved,
                resolved.prepend_args.len() + arguments.len() + resolved.use_vars.len(),
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(arguments.iter().map(Value::clone_for_php_storage))
                    .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
            )
        }
        Ok(true) => call_resolved_with_values_from_internal(ed, eg, resolved, arguments, true),
    };
    if let Some(exception) = retirement_exception {
        eg.exception = Some(exception);
    }
    result
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn has_children(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        return Ok(());
    }
    let inner = iterator_delegate::inner(&receiver);
    let result = call_object_protocol_method(eg, &inner, "RecursiveIterator", "hasChildren", &[])?
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result);
}

/// Retiring a native call's last argument can invoke a destructor while the
/// callee already has a pending exception. Keep that exception, or chain
/// it behind a replacement thrown by the destructor, like ordinary unwinding.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn discard_after_call(value: Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let pending = eg.exception.take();
    let result = iterator_delegate::discard(value, eg);
    if let Some(previous) = pending {
        if let Some(replacement) = eg.exception.as_ref() {
            crate::vm::execute::append_replaced_exception(replacement, &previous, eg);
        } else {
            eg.exception = Some(previous);
        }
    }
    result
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn get_children(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = owned_argument(ed, 0);
    if !iterator_delegate::initialized(&receiver, eg) {
        return Ok(());
    }
    let (inner, callback, class_id) = {
        let object = receiver.as_object().unwrap();
        let state = object.native_iterator_delegate().unwrap();
        (
            state.iterator.clone(),
            state.callback.as_ref().map(|c| c.callable.clone()),
            object.class_id,
        )
    };
    let inner_child =
        call_object_protocol_method(eg, &inner, "RecursiveIterator", "getChildren", &[])?
            .unwrap_or_else(Value::null);
    if eg.exception.is_some() {
        return Ok(());
    }
    let class = eg.class_by_id(class_id).expect("native filter class");
    let constructor = eg
        .find_function(&format!("{}::__construct", class.name))
        .or_else(|| {
            find_method_in_class_hierarchy(eg, &class.name, "__construct").map(|(_, _, f, _)| f)
        });
    let child = Value::object(PhpObject::with_layout_from_defaults(
        class_id,
        Rc::clone(&class.property_layout),
        &class.property_defaults,
    ));
    if let Some(func_ptr) = constructor {
        // Native child factories obey the same failed-construction lifecycle
        // as source-level new, including a constructor that publishes $this.
        child.suppress_unconstructed_object_destructor();
        let resolved = ResolvedCallback {
            func_ptr,
            prepend_args: vec![child.clone()],
            use_vars: vec![],
            called_scope_class_id: class_id,
            closure_scope_class_id: None,
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        };
        let mut arguments = vec![inner_child];
        arguments.extend(callback);
        let result = call_resolved_with_values_from_internal(ed, eg, &resolved, &arguments, true);
        if result.is_ok() && eg.exception.is_none() {
            child.enable_constructed_object_destructor();
        }
        // A subclass need not retain its inner-child argument. The factory's
        // snapshot can own the final handle after the callee frame retires.
        for argument in arguments {
            discard_after_call(argument, eg)?;
        }
        discard_after_call(result?, eg)?;
    }
    if eg.exception.is_some() {
        discard_after_call(child, eg)?;
        return Ok(());
    }
    ret!(rv, child);
}

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Bool, Callable, ClassName, Nullable};
    let mut functions = Vec::with_capacity(10);
    for (owner, parent, recursive, abstract_class, count) in [
        ("CallbackFilterIterator", "FilterIterator", false, false, 2),
        ("RecursiveFilterIterator", "FilterIterator", true, true, 3),
        (
            "RecursiveCallbackFilterIterator",
            "CallbackFilterIterator",
            true,
            false,
            3,
        ),
        ("ParentIterator", "RecursiveFilterIterator", false, false, 2),
    ] {
        let mut class = empty_internal_type(
            owner,
            if recursive {
                vec!["RecursiveIterator".into()]
            } else {
                vec![]
            },
            false,
            false,
        );
        class.parent = Some(parent.into());
        class.is_abstract = abstract_class;
        if abstract_class {
            class.abstract_methods.push("accept".into());
        }
        eg.register_class_with_complete_native_parent(class)
            .unwrap();
        eg.reserve_internal_method_contracts(owner, count);
        macro_rules! method {
            ($name:expr, $handler:expr, $names:expr, $hints:expr, $result:expr) => {{
                let names: &[&str] = $names;
                recursive_iterator::register_method(
                    eg,
                    &mut functions,
                    owner,
                    $name,
                    $handler,
                    names,
                    $hints,
                    &[None, None][..names.len()],
                    $result,
                );
            }};
        }
        let (handler, callback) = match owner {
            "CallbackFilterIterator" => (
                construct_callback as crate::vm::function::InternalFunctionHandler,
                true,
            ),
            "RecursiveFilterIterator" => (
                construct_recursive as crate::vm::function::InternalFunctionHandler,
                false,
            ),
            "RecursiveCallbackFilterIterator" => (
                construct_recursive_callback as crate::vm::function::InternalFunctionHandler,
                true,
            ),
            _ => (
                construct_parent as crate::vm::function::InternalFunctionHandler,
                false,
            ),
        };
        let inner_type = if owner == "CallbackFilterIterator" {
            "Iterator"
        } else {
            "RecursiveIterator"
        };
        if callback {
            method!(
                "__construct",
                handler,
                &["iterator", "callback"],
                vec![ClassName(inner_type.into()), Callable],
                ParamTypeHint::None
            );
        } else {
            method!(
                "__construct",
                handler,
                &["iterator"],
                vec![ClassName(inner_type.into())],
                ParamTypeHint::None
            );
        }
        if owner == "CallbackFilterIterator" {
            method!("accept", accept, &[], vec![], Bool);
        }
        if owner == "ParentIterator" {
            method!("accept", has_children, &[], vec![], Bool);
        }
        if recursive {
            method!("hasChildren", has_children, &[], vec![], Bool);
            method!(
                "getChildren",
                get_children,
                &[],
                vec![],
                if abstract_class {
                    Nullable(Box::new(ClassName(owner.into())))
                } else {
                    ClassName(owner.into())
                }
            );
        }
    }
    functions
}
