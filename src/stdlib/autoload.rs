//! Request-local SPL autoload registry and symbol-existence probes.
//!
//! Callback resolution is performed once at registration. Missing-symbol
//! probes stay allocation-free when no autoloader has ever been registered;
//! active stacks take a snapshot so callbacks may register or unregister
//! loaders while the VM is re-entered.

use crate::runtime::{AutoloadEntry, AutoloadState, ExecutorGlobals};
use crate::value::{PhpArray, Value, ValueType, make_error_value};
use crate::vm::execute::{IncludeFileOutcome, VmError, execute_included_file};
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

const DEFAULT_AUTOLOAD_EXTENSIONS: &str = ".inc,.php";

#[inline]
fn normalized_symbol_name(name: &str) -> &str {
    name.strip_prefix('\\').unwrap_or(name)
}

#[inline]
fn symbol_exists(eg: &ExecutorGlobals, name: &str, kind: SymbolKind) -> bool {
    let name = normalized_symbol_name(name);
    let definition = eg.find_class(name);
    definition.is_some_and(|definition| match kind {
        SymbolKind::Any => true,
        SymbolKind::Class => !definition.is_interface && !definition.is_trait,
        SymbolKind::Interface => definition.is_interface,
        SymbolKind::Trait => definition.is_trait,
        SymbolKind::Enum => definition.is_enum,
    })
}

pub(crate) fn ensure_symbol_loaded(eg: &mut ExecutorGlobals, name: &str) -> Result<bool, VmError> {
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

fn invalid_callback(function: &str, callback: &Value, nullable: bool, eg: &mut ExecutorGlobals) {
    let description = callback.echo_to_string();
    let nullable = if nullable { " or null" } else { "" };
    eg.exception = Some(make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #1 ($callback) must be a valid callback{nullable}, function \"{description}\" not found or not callable"
        ),
    ));
}

#[cold]
fn resolve_autoload_candidate(
    eg: &ExecutorGlobals,
    execute_data: *mut ExecuteData,
    filename: &str,
) -> Option<String> {
    #[cfg(feature = "include-path")]
    if let Some(path) = crate::stdlib::include_path::resolve_existing(eg, filename) {
        return Some(path);
    }
    #[cfg(not(feature = "include-path"))]
    if std::path::Path::new(filename).exists() {
        return Some(filename.to_string());
    }

    if let Some(directory) = eg
        .autoload
        .as_ref()
        .and_then(|state| state.base_directory.as_deref())
    {
        let candidate = std::path::Path::new(directory).join(filename);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    callsite_source_directory(execute_data).and_then(|directory| {
        let candidate = std::path::Path::new(&directory).join(filename);
        candidate
            .exists()
            .then(|| candidate.to_string_lossy().into_owned())
    })
}

fn callsite_source_directory(execute_data: *mut ExecuteData) -> Option<String> {
    // SAFETY: the active internal-function frame and every predecessor remain
    // linked and alive for the duration of this synchronous callback.
    unsafe {
        let mut caller = (*execute_data).prev_execute_data;
        while !caller.is_null() {
            let function = &*(*caller).func;
            if function.fn_type == crate::vm::function::FunctionType::User {
                let source = (*caller).op_array().name.as_str();
                return std::path::Path::new(source)
                    .parent()
                    .map(|path| path.to_string_lossy().into_owned());
            }
            caller = (*caller).prev_execute_data;
        }
    }
    None
}

pub(crate) fn fn_spl_autoload(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name = arg!(ed, 0).echo_to_string();
    let explicit_extensions = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .map(Value::echo_to_string);
    let configured_extensions = eg
        .autoload
        .as_ref()
        .and_then(|state| state.extensions.clone());
    let extensions = explicit_extensions
        .as_deref()
        .or(configured_extensions.as_deref())
        .unwrap_or(DEFAULT_AUTOLOAD_EXTENSIONS);
    let lower_name = normalized_symbol_name(&class_name)
        .to_ascii_lowercase()
        .replace('\\', std::path::MAIN_SEPARATOR_STR);

    let mut remaining = extensions;
    while !remaining.is_empty() && eg.exception.is_none() {
        let (extension, next) = remaining
            .split_once(',')
            .map_or((remaining, None), |(extension, rest)| {
                (extension, Some(rest))
            });
        let filename = format!("{lower_name}{extension}");
        if let Some(path) = resolve_autoload_candidate(eg, ed, &filename) {
            match execute_included_file(eg, &path, true, None)? {
                IncludeFileOutcome::Executed(_) | IncludeFileOutcome::AlreadyIncluded
                    if symbol_exists(eg, &class_name, SymbolKind::Any) =>
                {
                    break;
                }
                IncludeFileOutcome::Executed(_)
                | IncludeFileOutcome::AlreadyIncluded
                | IncludeFileOutcome::Missing(_)
                | IncludeFileOutcome::Thrown(_) => {}
            }
        }
        let Some(next) = next else {
            break;
        };
        remaining = next;
    }

    if eg.exception.is_none() {
        ret!(rv, Value::null());
    }
    Ok(())
}

pub(crate) fn fn_spl_autoload_extensions(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(value) = arg_opt!(ed, 0)
        && value.value_type() != ValueType::Null
    {
        eg.autoload
            .get_or_insert_with(|| Box::new(AutoloadState::default()))
            .extensions = Some(value.echo_to_string().into());
    }
    let extensions = eg
        .autoload
        .as_ref()
        .and_then(|state| state.extensions.as_deref())
        .unwrap_or(DEFAULT_AUTOLOAD_EXTENSIONS);
    ret!(rv, Value::string(extensions));
}

fn resolved_entry(callback: Value, resolved: ResolvedCallback) -> AutoloadEntry {
    AutoloadEntry {
        callback,
        func_ptr: resolved.func_ptr,
        prepend_args: resolved.prepend_args,
        use_vars: resolved.use_vars,
        called_scope_class_id: resolved.called_scope_class_id,
        bound_this: resolved.bound_this,
        is_magic_call: resolved.is_magic_call,
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
        called_scope_class_id: entry.called_scope_class_id,
        bound_this: entry.bound_this.clone(),
        is_magic_call: entry.is_magic_call,
    };
    let _ = call_resolved_with_values(eg, &resolved, std::slice::from_ref(class_name))?;
    Ok(())
}

fn invoke_autoload_stack(
    eg: &mut ExecutorGlobals,
    name: &str,
    stop_kind: SymbolKind,
) -> Result<(), VmError> {
    if eg
        .autoload
        .as_ref()
        .is_none_or(|state| state.entries.is_empty())
    {
        return Ok(());
    }

    let normalized = normalized_symbol_name(name);
    let guard_key = normalized.to_ascii_lowercase();
    let already_active = eg
        .autoload
        .as_ref()
        .is_some_and(|state| state.active_classes.contains(&guard_key));
    if already_active {
        return Ok(());
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
            || symbol_exists(eg, normalized, stop_kind)
        {
            break;
        }
    }

    if let Some(state) = eg.autoload.as_mut() {
        state.active_classes.remove(&guard_key);
    }
    invocation_result
}

fn exists_with_autoload(
    eg: &mut ExecutorGlobals,
    name: &str,
    kind: SymbolKind,
    autoload: bool,
) -> Result<bool, VmError> {
    // A symbol of another class-like kind still owns this name. PHP returns
    // false for e.g. class_exists(LoadedInterface::class) without invoking
    // autoload again and redeclaring the already loaded interface.
    if symbol_exists(eg, name, SymbolKind::Any) {
        return Ok(symbol_exists(eg, name, kind));
    }
    if !autoload
        || eg
            .autoload
            .as_ref()
            .is_none_or(|state| state.entries.is_empty())
    {
        return Ok(false);
    }

    invoke_autoload_stack(eg, name, kind)?;
    Ok(eg.exception.is_none() && symbol_exists(eg, name, kind))
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

pub(crate) fn fn_class_alias(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let original = arg!(ed, 0).echo_to_string();
    let alias = arg!(ed, 1).echo_to_string();
    let autoload = arg_opt!(ed, 2).is_none_or(Value::is_truthy);

    if !symbol_exists(eg, &original, SymbolKind::Any) {
        if !autoload || !exists_with_autoload(eg, &original, SymbolKind::Any, true)? {
            if eg.exception.is_some() {
                return Ok(());
            }
            eg.write_output(
                format!("Warning: class_alias(): Class \"{original}\" not found\n").as_bytes(),
            );
            ret!(rv, Value::bool(false));
        }
    }

    match eg.register_class_alias(&original, &alias) {
        Ok(()) => ret!(rv, Value::bool(true)),
        Err(message) => {
            eg.write_output(format!("Warning: class_alias(): {message}\n").as_bytes());
            ret!(rv, Value::bool(false));
        }
    }
}

pub(crate) fn fn_spl_autoload_register(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = match arg_opt!(ed, 0) {
        None => Value::string("spl_autoload"),
        Some(value) if value.value_type() == ValueType::Null => Value::string("spl_autoload"),
        Some(value) => value.clone(),
    };
    if arg_opt!(ed, 1).is_some_and(|value| !value.is_truthy()) {
        eg.write_output(
            b"Notice: spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw\n",
        );
    }
    let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) else {
        invalid_callback("spl_autoload_register", &callback, true, eg);
        return Ok(());
    };
    let prepend = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    let state = eg
        .autoload
        .get_or_insert_with(|| Box::new(AutoloadState::default()));
    if state.base_directory.is_none() && callback.as_str() == Some("spl_autoload") {
        state.base_directory = callsite_source_directory(ed).map(Into::into);
    }

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

pub(crate) fn fn_spl_autoload_call(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name = arg!(ed, 0).echo_to_string();
    invoke_autoload_stack(eg, &class_name, SymbolKind::Any)?;
    if eg.exception.is_none() {
        ret!(rv, Value::null());
    }
    Ok(())
}

pub(crate) fn fn_spl_autoload_unregister(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0).clone();
    if callback
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("spl_autoload_call"))
    {
        eg.write_output(
            b"Deprecated: spl_autoload_unregister(): Using spl_autoload_call() as a callback for spl_autoload_unregister() is deprecated, to remove all registered autoloaders, call spl_autoload_unregister() for all values returned from spl_autoload_functions()\n",
        );
        let removed = eg.autoload.as_mut().is_some_and(|state| {
            if state.entries.is_empty() {
                return false;
            }
            state.entries = Default::default();
            true
        });
        ret!(rv, Value::bool(removed));
    }
    if resolve_callback_at_callsite(&callback, eg, ed).is_none() {
        invalid_callback("spl_autoload_unregister", &callback, false, eg);
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
