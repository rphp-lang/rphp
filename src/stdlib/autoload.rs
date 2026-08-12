//! Request-local SPL autoload registry and symbol-existence probes.
//!
//! Callback resolution is performed once at registration. Missing-symbol
//! probes stay allocation-free when no autoloader has ever been registered;
//! active stacks take a snapshot so callbacks may register or unregister
//! loaders while the VM is re-entered.

use crate::runtime::{AutoloadEntry, AutoloadState, ExecutorGlobals};
use crate::value::{PhpArray, Value, ValueType, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

use super::{ResolvedCallback, call_resolved_with_values, resolve_callback_at_callsite};

#[derive(Clone, Copy)]
enum SymbolKind {
    Any,
    Class,
    Interface,
    Trait,
    Enum,
}

#[inline]
fn normalized_symbol_name(name: &str) -> &str {
    name.strip_prefix('\\').unwrap_or(name)
}

#[inline]
fn symbol_exists(eg: &ExecutorGlobals, name: &str, kind: SymbolKind) -> bool {
    let name = normalized_symbol_name(name);
    let definition = eg
        .class_table
        .get(name)
        .map(|definition| definition.as_ref())
        .or_else(|| {
            eg.class_table
                .iter()
                .find(|(registered, _)| registered.eq_ignore_ascii_case(name))
                .map(|(_, definition)| definition.as_ref())
        });
    definition.is_some_and(|definition| match kind {
        SymbolKind::Any => true,
        SymbolKind::Class => !definition.is_interface && !definition.is_trait,
        SymbolKind::Interface => definition.is_interface,
        SymbolKind::Trait => definition.is_trait,
        SymbolKind::Enum => definition.is_enum,
    })
}

pub(super) fn ensure_symbol_loaded(eg: &mut ExecutorGlobals, name: &str) -> Result<bool, VmError> {
    exists_with_autoload(eg, name, SymbolKind::Any, true)
}

fn callback_equal(left: &Value, right: &Value) -> bool {
    if left.value_type() != right.value_type() {
        return false;
    }

    match left.value_type() {
        ValueType::String => left
            .as_str()
            .zip(right.as_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right)),
        ValueType::Array => {
            let Some(left) = left.as_array() else {
                return false;
            };
            let Some(right) = right.as_array() else {
                return false;
            };
            if left.len() != right.len() {
                return false;
            }
            left.iter().zip(right.iter()).all(
                |((left_key, left_value), (right_key, right_value))| {
                    left_key == right_key && callback_equal(left_value, right_value)
                },
            )
        }
        ValueType::Object => left.object_identity() == right.object_identity(),
        ValueType::Closure => {
            let Some(left) = left.as_closure() else {
                return false;
            };
            let Some(right) = right.as_closure() else {
                return false;
            };
            left.same_identity(right)
        }
        ValueType::Null | ValueType::Undef | ValueType::True | ValueType::False => true,
        ValueType::Long => left.as_long() == right.as_long(),
        ValueType::Double => left.as_double() == right.as_double(),
        ValueType::Resource => left.as_resource_id() == right.as_resource_id(),
        ValueType::Reference => false,
    }
}

fn invalid_callback(function: &str, callback: &Value, eg: &mut ExecutorGlobals) {
    let description = callback.echo_to_string();
    eg.exception = Some(make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #1 ($callback) must be a valid callback, function \"{description}\" not found or not callable"
        ),
    ));
}

fn resolved_entry(callback: Value, resolved: ResolvedCallback) -> AutoloadEntry {
    AutoloadEntry {
        callback,
        func_ptr: resolved.func_ptr,
        prepend_args: resolved.prepend_args,
        use_vars: resolved.use_vars,
    }
}

fn invoke_entry(
    eg: &mut ExecutorGlobals,
    entry: &AutoloadEntry,
    class_name: &Value,
) -> Result<(), VmError> {
    let resolved = ResolvedCallback {
        func_ptr: entry.func_ptr,
        prepend_args: entry.prepend_args.clone(),
        use_vars: entry.use_vars.clone(),
    };
    let _ = call_resolved_with_values(eg, &resolved, std::slice::from_ref(class_name))?;
    Ok(())
}

fn exists_with_autoload(
    eg: &mut ExecutorGlobals,
    name: &str,
    kind: SymbolKind,
    autoload: bool,
) -> Result<bool, VmError> {
    if symbol_exists(eg, name, kind) {
        return Ok(true);
    }
    if !autoload
        || eg
            .autoload
            .as_ref()
            .is_none_or(|state| state.entries.is_empty())
    {
        return Ok(false);
    }

    let normalized = normalized_symbol_name(name);
    let guard_key = normalized.to_ascii_lowercase();
    let already_active = eg
        .autoload
        .as_ref()
        .is_some_and(|state| state.active_classes.contains(&guard_key));
    if already_active {
        return Ok(false);
    }

    let entries = eg
        .autoload
        .as_ref()
        .map(|state| state.entries.clone())
        .unwrap_or_default();
    eg.autoload
        .as_mut()
        .expect("autoload state disappeared before invocation")
        .active_classes
        .insert(guard_key.clone());

    let class_name = Value::string(normalized);
    let mut invocation_result = Ok(());
    for entry in entries.iter() {
        invocation_result = invoke_entry(eg, entry, &class_name);
        if invocation_result.is_err()
            || eg.exception.is_some()
            || symbol_exists(eg, normalized, kind)
        {
            break;
        }
    }

    if let Some(state) = eg.autoload.as_mut() {
        state.active_classes.remove(&guard_key);
    }
    invocation_result?;
    Ok(eg.exception.is_none() && symbol_exists(eg, normalized, kind))
}

fn symbol_exists_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    kind: SymbolKind,
) -> Result<(), VmError> {
    let name = arg!(ed, 0).echo_to_string();
    let autoload = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    let exists = exists_with_autoload(eg, &name, kind, autoload)?;
    if eg.exception.is_none() {
        ret!(rv, Value::bool(exists));
    }
    Ok(())
}

pub(crate) fn fn_class_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    symbol_exists_handler(ed, rv, eg, SymbolKind::Class)
}

pub(crate) fn fn_interface_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    symbol_exists_handler(ed, rv, eg, SymbolKind::Interface)
}

pub(crate) fn fn_trait_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    symbol_exists_handler(ed, rv, eg, SymbolKind::Trait)
}

pub(crate) fn fn_enum_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    symbol_exists_handler(ed, rv, eg, SymbolKind::Enum)
}

pub(crate) fn fn_spl_autoload_register(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0).clone();
    let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) else {
        invalid_callback("spl_autoload_register", &callback, eg);
        return Ok(());
    };
    let prepend = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    let state = eg
        .autoload
        .get_or_insert_with(|| Box::new(AutoloadState::default()));

    if state
        .entries
        .iter()
        .any(|entry| callback_equal(&entry.callback, &callback))
    {
        ret!(rv, Value::bool(true));
    }

    let entry = resolved_entry(callback, resolved);
    let mut entries = state.entries.to_vec();
    if prepend {
        entries.insert(0, entry);
    } else {
        entries.push(entry);
    }
    state.entries = entries.into();
    ret!(rv, Value::bool(true));
}

pub(crate) fn fn_spl_autoload_unregister(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0).clone();
    if resolve_callback_at_callsite(&callback, eg, ed).is_none() {
        invalid_callback("spl_autoload_unregister", &callback, eg);
        return Ok(());
    }

    let removed = eg.autoload.as_mut().is_some_and(|state| {
        let mut entries = state.entries.to_vec();
        let old_len = entries.len();
        entries.retain(|entry| !callback_equal(&entry.callback, &callback));
        if entries.len() == old_len {
            return false;
        }
        state.entries = entries.into();
        true
    });
    ret!(rv, Value::bool(removed));
}

pub(crate) fn fn_spl_autoload_functions(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut callbacks =
        PhpArray::with_packed_capacity(eg.autoload.as_ref().map_or(0, |state| state.entries.len()));
    if let Some(state) = eg.autoload.as_ref() {
        for entry in state.entries.iter() {
            callbacks.push(entry.callback.clone());
        }
    }
    ret!(rv, Value::array(callbacks));
}
