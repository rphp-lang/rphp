/// Standard library — built-in PHP functions.
/// Each function follows the InternalFunctionHandler signature.
///
/// Design: helper macros eliminate per-function boilerplate:
///   - `arg!(ed, N)` → &Value at CV(N)
///   - `arg_mut!(ed, N)` → *mut Value at CV(N)
///   - `arg_str!(ed, N)` → Cow<str> (zero-copy when String, coerce otherwise)
///   - `arg_long!(ed, N)` → i64 (type-juggling)
///   - `arg_float!(ed, N)` → f64 (type-juggling)
///   - `arg_opt!(ed, N)` → Option<&Value> (None if Undef — for optional params)
///   - `ret!(rv, expr)` → writes to return_value with null check
use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};

use serde::Serialize as _;

use crate::compiler::compile::{ClassConstantDefinition, PropertyDefinition};
use crate::compiler::{
    make_direct_internal_function, make_internal_function, make_internal_function_ref,
    make_internal_function_variadic, make_internal_function_variadic_prefer_ref,
    make_internal_function_variadic_raw, make_internal_function_variadic_ref,
    make_internal_function_variadic_ref_raw_all, make_internal_method,
    make_internal_method_variadic,
};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{
    ArrayKey, ClosureStaticVars, PhpArray, PhpClosure, PhpObject, Value, ValueType,
};
use crate::vm::execute::{
    ArrayKeyError, CallArgumentPreparation, ExplicitNumericCastTarget,
    ScalarLongReferenceMutationCallback, ScalarLongSortOrder, VmError, arithmetic_operator_operand,
    call_function, call_function_iter, call_function_iter_with_context, call_function_owned_iter,
    call_function_owned_iter_readback_arg0_with_context, call_function_owned_iter_with_context,
    call_function_owned_iter_with_context_and_named, call_object_property_get_hook,
    call_object_property_magic_get, call_object_property_magic_isset, check_type_hint,
    displayed_function_name, explicit_float_conversion, explicit_long_conversion,
    explicit_numeric_cast_warning, php_numeric_string_to_float, prepare_call_argument,
    prepare_scalar_long_callback, prepare_scalar_long_reference_mutation_callback,
    try_execute_scalar_long_callback, value_to_array_key, values_equal_checked_with_precision,
    values_identical_checked,
};
use crate::vm::frame::ExecuteData;
use crate::vm::function::InternalFunction;
use crate::vm::function::{Function, FunctionCommon, FunctionType, ParamTypeHint, UserFunction};
use crate::vm::instruction::InlineCache;
use crate::vm::opcode::OpCode;

#[cfg(feature = "include-path")]
pub(crate) mod include_path;
mod json_decode;
mod legacy_encoding;
mod meta_tags;
mod pack;
mod parse_ini;
mod random;
pub(crate) mod reflection;
mod regex_callback;
mod registry;
mod serialization;
mod tokenizer;

pub use registry::register_stdlib;

const BUILTIN_EXCEPTION_SUBCLASSES: &[(&str, &str)] = &[
    ("ClosedGeneratorException", "Exception"),
    ("JsonException", "Exception"),
    ("LogicException", "Exception"),
    ("BadFunctionCallException", "LogicException"),
    ("BadMethodCallException", "BadFunctionCallException"),
    ("DomainException", "LogicException"),
    ("InvalidArgumentException", "LogicException"),
    ("LengthException", "LogicException"),
    ("OutOfRangeException", "LogicException"),
    ("RuntimeException", "Exception"),
    ("OutOfBoundsException", "RuntimeException"),
    ("OverflowException", "RuntimeException"),
    ("RangeException", "RuntimeException"),
    ("UnderflowException", "RuntimeException"),
    ("UnexpectedValueException", "RuntimeException"),
];

const BUILTIN_ERROR_SUBCLASSES: &[(&str, &str)] = &[
    ("ArithmeticError", "Error"),
    ("DivisionByZeroError", "ArithmeticError"),
    ("AssertionError", "Error"),
];

// ============================================================================
// Helper macros — zero-cost abstractions for stdlib handlers
// ============================================================================

/// Read CV(n) as &Value — follows references transparently
#[allow(unused_unsafe)]
macro_rules! arg {
    ($ed:expr, $n:expr) => {{
        unsafe {
            let v = (*$ed).cv($n);
            if v.is_reference() {
                &*v.as_ref_ptr()
            } else {
                v
            }
        }
    }};
}

/// Read CV(n) as *mut Value — follows references (returns pointer to original)
#[allow(unused_unsafe)]
macro_rules! arg_mut {
    ($ed:expr, $n:expr) => {{
        unsafe {
            let ptr = (*$ed).cv_mut($n) as *mut Value;
            if (*ptr).is_reference() {
                (*ptr).as_ref_ptr()
            } else {
                ptr
            }
        }
    }};
    ($ed:expr, $n:expr, $value:expr) => {{
        unsafe {
            let ptr = (*$ed).cv_mut($n) as *mut Value;
            let destination = if (*ptr).is_reference() {
                (*ptr).as_ref_ptr()
            } else {
                ptr
            };
            *destination = $value;
        }
    }};
}

/// Read CV(n) as Cow<str> — zero-copy for String values, coerced otherwise
macro_rules! arg_str {
    ($ed:expr, $n:expr) => {{
        let v = arg!($ed, $n);
        match v.as_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(v.echo_to_string()),
        }
    }};
}

/// Read CV(n) as i64 via PHP type juggling
macro_rules! arg_long {
    ($ed:expr, $n:expr) => {
        arg!($ed, $n).to_long_val()
    };
}

/// Read CV(n) as f64 via PHP type juggling
macro_rules! arg_float {
    ($ed:expr, $n:expr) => {
        arg!($ed, $n).to_float_val()
    };
}

/// Read CV(n) as Option<&Value> — None when Undef (optional param not passed)
macro_rules! arg_opt {
    ($ed:expr, $n:expr) => {{
        let v = arg!($ed, $n);
        if v.value_type() == ValueType::Undef {
            None
        } else {
            Some(v)
        }
    }};
}

/// Write value to return_value pointer (with null guard) and return Ok(()).
/// SAFETY: rv must be a valid pointer or null.
macro_rules! ret {
    ($rv:expr, $val:expr) => {{
        // The return expression may itself perform observable work (for
        // example array_pop() mutates its by-reference argument).  An unused
        // call result suppresses only the result write, never evaluation of
        // the internal function's return expression.
        let value = $val;
        if !$rv.is_null() {
            unsafe { $rv.write(value) };
        }
        return Ok(());
    }};
}

mod array_assoc_sets;
mod array_traversal;
mod builtin_classes;
mod directory;
mod fiber;
mod filesystem;
#[cfg(feature = "formatted-io")]
mod formatted_io;
mod hebrew;
mod html_entities;
mod process;
mod recursive_arrays;
mod source_filters;
mod strings;
mod weak;

use filesystem::{bytes_to_php_string, php_string_to_bytes};

pub use builtin_classes::register_builtin_classes;

pub(super) fn owned_argument(ed: *mut ExecuteData, index: u32) -> Value {
    // SAFETY: internal handlers receive a live ExecuteData frame and their
    // registered arity guarantees this CV index. Reference payloads remain
    // live for the request; cloning detaches the returned owned Value.
    unsafe {
        let value = (*ed).cv(index);
        if value.is_reference() {
            (&*value.as_ref_ptr()).clone()
        } else {
            value.clone()
        }
    }
}

pub(super) fn write_return_value(rv: *mut Value, value: Value) {
    if rv.is_null() {
        return;
    }
    // SAFETY: the VM supplies either null for a discarded result or one live,
    // uninitialized return slot owned by the current internal call.
    unsafe { rv.write(value) };
}

/// Render the ordinary PHP callable error tail shared by fixed and variadic
/// internal functions. Legacy relative-scope callbacks take their separate
/// checked-resolution path before this helper is reached.
pub(crate) fn ordinary_callback_invalid_reason(callback: &Value, eg: &ExecutorGlobals) -> String {
    let callback = callback.dereferenced();
    if let Some(name) = callback.as_str() {
        let Some((class, method)) = name.rsplit_once("::") else {
            return format!("function \"{name}\" not found or invalid function name");
        };
        return callback_class_method_invalid_reason(
            eg,
            class.trim_start_matches('\\'),
            method,
            false,
        );
    }

    let Some(array) = callback.as_array() else {
        return "no array or string given".to_string();
    };
    if array.len() != 2 {
        return "array callback must have exactly two members".to_string();
    }
    let Some(first) = array.get_value_at(0).map(Value::dereferenced) else {
        return "first array member is not a valid class name or object".to_string();
    };
    if first.as_str().is_none()
        && first.as_object().is_none()
        && first.value_type() != ValueType::Closure
    {
        return "first array member is not a valid class name or object".to_string();
    }
    let Some(method) = array
        .get_value_at(1)
        .map(Value::dereferenced)
        .and_then(Value::as_str)
    else {
        return "second array member is not a valid method".to_string();
    };
    if let Some(class) = first.as_str() {
        return callback_class_method_invalid_reason(
            eg,
            class.trim_start_matches('\\'),
            method,
            false,
        );
    }
    if let Some(object) = first.as_object() {
        return callback_class_method_invalid_reason(eg, &object.class_name, method, true);
    }
    if first.value_type() == ValueType::Closure {
        return callback_class_method_invalid_reason(eg, "Closure", method, true);
    }
    "first array member is not a valid class name or object".to_string()
}

pub(crate) fn ensure_callback_class_loaded(
    callback: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<bool, VmError> {
    let callback = callback.dereferenced();
    let class = if let Some(name) = callback.as_str() {
        name.rsplit_once("::").map(|(class, _)| class)
    } else {
        callback
            .as_array()
            .filter(|array| array.len() == 2)
            .and_then(|array| array.get_value_at(0))
            .map(Value::dereferenced)
            .and_then(Value::as_str)
    };
    let Some(class) = class.map(|class| class.trim_start_matches('\\')) else {
        return Ok(false);
    };
    if class.is_empty()
        || matches!(
            class.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
        || find_class_case_insensitive(eg, class).is_some()
    {
        return Ok(false);
    }
    autoload::ensure_symbol_loaded(eg, class)
}

fn callback_class_method_invalid_reason(
    eg: &ExecutorGlobals,
    class: &str,
    method: &str,
    object_form: bool,
) -> String {
    let Some(class_definition) = find_class_case_insensitive(eg, class) else {
        return format!("class \"{class}\" not found");
    };
    let canonical = class_definition.name.clone();
    let Some((visibility, is_static, _, declaring)) =
        find_method_in_class_hierarchy(eg, &canonical, method)
    else {
        return format!("class {canonical} does not have a method \"{method}\"");
    };
    if visibility != Visibility::Public {
        let visibility = match visibility {
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Public => unreachable!(),
        };
        return format!("cannot access {visibility} method {canonical}::{method}()");
    }
    if !object_form && !is_static {
        return format!("non-static method {declaring}::{method}() cannot be called statically");
    }
    "no array or string given".to_string()
}

pub(crate) mod autoload;

/// Read a borrowed argument for the frame-free internal ABI, following PHP
/// references with the same semantics as `arg!` on an ExecuteData frame.
#[inline(always)]
fn direct_arg(args: &[Value], index: usize) -> &Value {
    args[index].dereferenced()
}

#[inline(always)]
fn direct_arg_str(args: &[Value], index: usize) -> Cow<'_, str> {
    let value = direct_arg(args, index);
    match value.as_str() {
        Some(string) => Cow::Borrowed(string),
        None => Cow::Owned(value.echo_to_string()),
    }
}

#[inline(always)]
fn json_decode_values(input: &Value, associative: Option<&Value>) -> Value {
    let input = if input.is_reference() {
        unsafe { &*input.as_ref_ptr() }
    } else {
        input
    };
    let associative = associative.map(|value| {
        if value.is_reference() {
            unsafe { &*value.as_ref_ptr() }
        } else {
            value
        }
    });
    let json = match input.as_str() {
        Some(json) => Cow::Borrowed(json),
        None => Cow::Owned(input.echo_to_string()),
    };
    json_decode_string(&json, associative.is_some_and(Value::is_truthy))
}

#[inline(always)]
fn direct_json_decode(args: &[Value]) -> Result<Value, VmError> {
    Ok(json_decode_values(&args[0], args.get(1)))
}

/// Attempt the exact-string unary `chunk_split` fast path. Callers must resume
/// through the canonical internal frame when this guard returns `None`.
#[inline(always)]
pub(crate) fn try_direct_chunk_split1(argument: &Value) -> Option<Value> {
    strings::direct_chunk_split_default_string(argument)
}

/// Dispatch a compiler-identified pure builtin without resolving a runtime
/// FunctionCommon or crossing the generic internal-function ABI.
#[inline(always)]
pub(crate) fn invoke_direct_internal1(
    kind: crate::builtin_metadata::DirectInternalKind,
    argument: &Value,
    precision: i32,
) -> Result<Value, VmError> {
    use crate::builtin_metadata::DirectInternalKind;

    let args = std::slice::from_ref(argument);
    match kind {
        DirectInternalKind::Strlen => direct_strlen(args, precision),
        DirectInternalKind::Strtolower => direct_strtolower(args),
        DirectInternalKind::Strtoupper => direct_strtoupper(args),
        DirectInternalKind::Ord => direct_ord(args),
        DirectInternalKind::Abs => direct_abs(args),
        DirectInternalKind::Floor => direct_floor(args),
        DirectInternalKind::Sqrt => direct_sqrt(args),
        DirectInternalKind::Sin => direct_sin(args),
        DirectInternalKind::Tan => direct_tan(args),
        DirectInternalKind::Asin => direct_asin(args),
        DirectInternalKind::Acos => direct_acos(args),
        DirectInternalKind::Atan => direct_atan(args),
        DirectInternalKind::Exp => direct_exp(args),
        DirectInternalKind::ChunkSplit => Err(VmError::Fatal(
            "chunk_split direct call requires canonical fallback".into(),
        )),
        DirectInternalKind::Intdiv
        | DirectInternalKind::JsonDecode
        | DirectInternalKind::Min2
        | DirectInternalKind::Max2 => Err(VmError::Fatal(
            "Invalid unary invocation of binary direct builtin".into(),
        )),
    }
}

/// Dispatch a compiler-identified pure binary builtin without a call frame.
#[inline(always)]
pub(crate) fn invoke_direct_internal2(
    kind: crate::builtin_metadata::DirectInternalKind,
    first: &Value,
    second: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    use crate::builtin_metadata::DirectInternalKind;

    match kind {
        DirectInternalKind::Intdiv => direct_intdiv_values(first, second),
        DirectInternalKind::JsonDecode => Ok(json_decode_values(first, Some(second))),
        DirectInternalKind::Min2 => direct_extrema2::<false>(first, second, eg),
        DirectInternalKind::Max2 => direct_extrema2::<true>(first, second, eg),
        _ => Err(VmError::Fatal(
            "Invalid binary direct internal handler ID".into(),
        )),
    }
}

fn fn_assert(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !eg.assertion_state.active {
        ret!(rv, Value::bool(true));
    }
    if arg!(ed, 0).is_truthy() {
        ret!(rv, Value::bool(true));
    }

    let description = arg_opt!(ed, 1).map(Value::dereferenced);
    if let Some(object) = description.and_then(Value::as_object) {
        if eg.class_is_a(object.class_name.as_ref(), "Throwable") {
            eg.exception = description.cloned();
            return Ok(());
        }
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "assert(): Argument #2 ($description) must be of type Throwable|string|null, {} given",
                object.class_name
            ),
        ));
        return Ok(());
    }
    if let Some(description) = description.filter(|value| value.value_type() == ValueType::Array) {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "assert(): Argument #2 ($description) must be of type Throwable|string|null, {} given",
                description.type_name()
            ),
        ));
        return Ok(());
    }
    let message = description
        .filter(|value| value.value_type() != ValueType::Null)
        .map(Value::echo_to_string)
        .unwrap_or_default();

    let (file, line) = internal_call_source(ed);
    let callback = eg.assertion_state.callback.clone();
    let mut invalid_callback = None;
    if let Some(callback) = callback {
        if let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) {
            let mut arguments = vec![
                Value::string(file.clone()),
                Value::long(line as i64),
                Value::null(),
            ];
            if !message.is_empty() {
                arguments.push(Value::string(message.clone()));
            }
            let _ = call_resolved_with_values(eg, &resolved, &arguments)?;
        } else {
            let display = callback.echo_to_string();
            invalid_callback = Some(format!(
                "Invalid callback {display}, function \"{display}\" not found or invalid function name"
            ));
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                invalid_callback.as_deref().unwrap_or_default(),
            ));
        }
    }

    if eg.assertion_state.exception {
        eg.exception = Some(crate::value::make_error_value("AssertionError", &message));
    } else if eg.assertion_state.warning {
        let description = if message.is_empty() {
            "Assertion"
        } else {
            &message
        };
        eg.write_output(
            format!("\nWarning: assert(): {description} failed in {file} on line {line}\n")
                .as_bytes(),
        );
    }

    if eg.assertion_state.bail {
        if let Some(exception) = eg.exception.take() {
            let rendered = if let Some(invalid) = invalid_callback {
                format!(
                    "Uncaught Error: {invalid} in {file}:{line}\nStack trace:\n#0 {file}({line}): assert(false, '{}')\n#1 {{main}}\n  thrown in {file} on line {line}",
                    message.replace('\\', "\\\\").replace('\'', "\\'")
                )
            } else {
                crate::vm::execute::format_uncaught_throwable(eg, &exception)
            };
            eg.write_output(format!("\nWarning: {rendered}\n").as_bytes());
        }
        return Err(VmError::Exit(255));
    }

    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(false));
}

fn assertion_option_bool(value: &Value) -> bool {
    match value
        .as_str()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "" | "0" | "off" | "no" | "false" | "none") => {
            false
        }
        Some(_) => true,
        None => value.is_truthy(),
    }
}

fn fn_assert_options(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let what = arg_long!(ed, 0);
    let value = arg_opt!(ed, 1).cloned();
    match what {
        1 => {
            let previous = eg.assertion_state.active;
            if let Some(value) = value.as_ref() {
                eg.assertion_state.active = assertion_option_bool(value);
            }
            ret!(rv, Value::long(previous as i64));
        }
        2 => {
            let previous = eg
                .assertion_state
                .callback
                .clone()
                .unwrap_or_else(Value::null);
            if let Some(value) = value {
                eg.assertion_state.callback =
                    (value.value_type() != ValueType::Null).then_some(value);
            }
            ret!(rv, previous);
        }
        3 => {
            let previous = eg.assertion_state.bail;
            if let Some(value) = value.as_ref() {
                eg.assertion_state.bail = assertion_option_bool(value);
            }
            ret!(rv, Value::long(previous as i64));
        }
        4 => {
            let previous = eg.assertion_state.warning;
            if let Some(value) = value.as_ref() {
                eg.assertion_state.warning = assertion_option_bool(value);
            }
            ret!(rv, Value::long(previous as i64));
        }
        5 => {
            let previous = eg.assertion_state.exception;
            if let Some(value) = value.as_ref() {
                eg.assertion_state.exception = assertion_option_bool(value);
            }
            ret!(rv, Value::long(previous as i64));
        }
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "assert_options(): Argument #1 ($option) must be an ASSERT_* constant",
            ));
            Ok(())
        }
    }
}

// ============================================================================
// Array functions
// ============================================================================

fn recursive_array_count(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    source: Value,
) -> Result<Option<i64>, VmError> {
    struct CountFrame {
        _owner: Value,
        identity: usize,
        values: Vec<Value>,
        next: usize,
    }

    fn frame(value: Value) -> CountFrame {
        let value = value.dereferenced().clone();
        let identity = value
            .array_identity()
            .expect("recursive count frame must retain an array");
        let values = value.as_array().unwrap().values().cloned().collect();
        CountFrame {
            _owner: value,
            identity,
            values,
            next: 0,
        }
    }

    let mut frames = vec![frame(source)];
    let mut active = std::collections::HashSet::new();
    active.insert(frames[0].identity);
    let mut count = frames[0].values.len() as i64;

    while let Some(current) = frames.last_mut() {
        if current.next == current.values.len() {
            active.remove(&current.identity);
            frames.pop();
            continue;
        }
        let child = current.values[current.next].clone();
        current.next += 1;
        let child = child.dereferenced();
        let Some(array) = child.as_array() else {
            continue;
        };
        let identity = child.array_identity().unwrap();
        if active.contains(&identity) {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("{function}(): Recursion detected"),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            continue;
        }
        count = count.saturating_add(array.len() as i64);
        active.insert(identity);
        frames.push(frame(child.clone()));
    }
    Ok(Some(count))
}

fn fn_count_named(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    let mode = if arg_opt!(ed, 1).is_some() {
        let Some(mode) = typed_internal_int_argument(ed, eg, function, 1, "mode")? else {
            return Ok(());
        };
        mode
    } else {
        0
    };
    if !matches!(mode, 0 | 1) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "{function}(): Argument #2 ($mode) must be either COUNT_NORMAL or COUNT_RECURSIVE"
            ),
        ));
        return Ok(());
    }

    let value = arg!(ed, 0);
    if let Some(array) = value.as_array() {
        if mode == 0 {
            ret!(rv, Value::long(array.len() as i64));
        }
        let Some(count) = recursive_array_count(ed, eg, function, owned_argument(ed, 0))? else {
            return Ok(());
        };
        ret!(rv, Value::long(count));
    }

    if let Some(object) = value.as_object() {
        let class_name = object.class_name.to_string();
        drop(object);
        if eg.class_is_a(&class_name, "Countable") {
            let receiver = value.clone();
            if let Some(result) = call_object_public_method(eg, &receiver, "count", &[])? {
                if eg.exception.is_none() {
                    ret!(rv, Value::long(result.to_long_val()));
                }
            }
            if eg.exception.is_some() {
                return Ok(());
            }
        }
    }

    typed_internal_argument_error(eg, function, value, 1, "value", "Countable|array");
    Ok(())
}

fn fn_count(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_count_named(ed, rv, eg, "count")
}

fn fn_sizeof(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_count_named(ed, rv, eg, "sizeof")
}

fn fn_array_push(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let values = arg!(ed, 1)
        .as_array()
        .expect("variadic array_push values must be packed into an array");
    array_push_values(
        ed,
        rv,
        eg,
        values.values().map(|value| value.dereferenced().clone()),
    )
}

fn fn_array_push_raw_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    let value = (supplied_num_args > 1).then(|| owned_argument(ed, 1));
    array_push_values(ed, rv, eg, value)
}

fn array_push_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    values: impl IntoIterator<Item = Value>,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let arr = unsafe { &mut *ptr };
    let Some(array) = arr.as_array_mut() else {
        typed_internal_argument_error(eg, "array_push", arr.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    for value in values {
        if !array.try_push(value) {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "Cannot add element to the array as the next element is already occupied",
            ));
            return Ok(());
        }
    }
    ret!(rv, Value::long(array.len() as i64));
}

fn fn_array_pop(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    let Some(array) = arr.as_array_mut() else {
        typed_internal_argument_error(eg, "array_pop", arr.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let value = array
        .pop()
        .map(|value| value.dereferenced().clone())
        .unwrap_or_else(Value::null);
    array.cursor_rewind();
    write_array_mutator_return(ed, rv, eg, value)
}

fn fn_array_shift(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    let Some(array) = arr.as_array_mut() else {
        typed_internal_argument_error(eg, "array_shift", arr.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let had_value = !array.is_empty();
    let value = array
        .shift()
        .map(|value| value.dereferenced().clone())
        .unwrap_or_else(Value::null);
    if had_value {
        crate::vm::execute::adjust_live_foreach_reference_positions_for_splice(ed, 0, 0, 1, 0);
    }
    array.cursor_rewind();
    write_array_mutator_return(ed, rv, eg, value)
}

fn fn_array_unshift(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let values = arg!(ed, 1)
        .as_array()
        .expect("variadic array_unshift values must be packed into an array");
    array_unshift_values(
        ed,
        rv,
        eg,
        values.values().map(|value| value.dereferenced().clone()),
        values.len(),
    )
}

fn fn_array_unshift_raw_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    let value = (supplied_num_args > 1).then(|| owned_argument(ed, 1));
    let inserted = usize::from(value.is_some());
    array_unshift_values(ed, rv, eg, value, inserted)
}

fn array_unshift_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    values: impl IntoIterator<Item = Value>,
    inserted: usize,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let arr = unsafe { &mut *ptr };
    let Some(array) = arr.as_array() else {
        typed_internal_argument_error(eg, "array_unshift", arr.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let Some(total) = array.len().checked_add(inserted) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "The total number of elements must be lower than 1073741824",
        ));
        return Ok(());
    };
    if total >= 1 << 30 {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "The total number of elements must be lower than 1073741824",
        ));
        return Ok(());
    }

    let mut result = if array.has_string_keys() {
        PhpArray::with_deferred_hash_capacity(total)
    } else {
        PhpArray::with_packed_capacity(total)
    };
    for value in values {
        result.push(value);
    }
    for (key, value) in array.iter() {
        let value = array_projection_value(value);
        match key {
            ArrayKey::Int(_) => result.push(value),
            ArrayKey::String(key) => result.set_str(&key, value),
        }
    }
    *arr = Value::array(result);
    if inserted != 0 {
        crate::vm::execute::adjust_live_foreach_reference_positions_for_splice(
            ed, 0, 0, 0, inserted,
        );
    }
    ret!(rv, Value::long(total as i64));
}

#[inline(always)]
fn write_array_mutator_return(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    value: Value,
) -> Result<(), VmError> {
    if !rv.is_null() {
        write_return_value(rv, value);
        return Ok(());
    }
    // SAFETY: the internal activation and its synchronous caller remain live
    // for the complete handler invocation.
    let caller = unsafe { (*ed).prev_execute_data };
    crate::vm::execute::run_value_destructors(eg, &[value], caller)
}

fn fn_array_key_exists_named(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    let source = arg!(ed, 1);
    let Some(array) = source.as_array() else {
        typed_internal_argument_error(eg, function, source, 2, "array", "array");
        return Ok(());
    };
    let key = arg!(ed, 0);
    if let Some(key) = key.as_long() {
        ret!(rv, Value::bool(array.get_int(key).is_some()));
    }
    if let Some(key) = key.as_str() {
        if array.has_utf8_text_keys() && arg!(ed, 0).is_binary_string() {
            let key = array.normalize_utf8_text_key(ArrayKey::String(key.to_string()), arg!(ed, 0));
            let exists = match key {
                ArrayKey::Int(key) => array.get_int(key).is_some(),
                ArrayKey::String(key) => array.get_str(&key).is_some(),
            };
            ret!(rv, Value::bool(exists));
        }
        let exists = crate::value::canonical_decimal_array_key(key).map_or_else(
            || array.get_str(key).is_some(),
            |key| array.get_int(key).is_some(),
        );
        ret!(rv, Value::bool(exists));
    }

    // A diagnostic handler may mutate the caller's array or key. Retain both
    // by-value call snapshots before entering the slow conversion path.
    let source = owned_argument(ed, 1);
    let array = source.dereferenced().as_array().unwrap();

    // Array keys are normalized through the same scalar conversion used by
    // ordinary dimensions. The null diagnostic is specific to this API, while
    // float/resource diagnostics and illegal-key errors retain the shared PHP
    // array-offset contract.
    let key_source = owned_argument(ed, 0);
    let key_value = key_source.dereferenced();
    let key = match value_to_array_key(key_value) {
        Ok(key) => key,
        Err(ArrayKeyError::DeprecatedNull) => {
            report_internal_deprecation(
                eg,
                ed,
                "Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ArrayKey::String(String::new())
        }
        Err(ArrayKeyError::DeprecatedFloat(integer)) => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "Implicit conversion from float {} to int loses precision",
                    key_value.echo_to_string_with_precision(-1)
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::NonRepresentableFloat {
            integer,
            also_deprecated,
        }) => {
            let rendered = key_value.echo_to_string_with_precision(-1);
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("The float {rendered} is not representable as an int, cast occurred"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if also_deprecated {
                report_internal_deprecation(
                    eg,
                    ed,
                    &format!("Implicit conversion from float {rendered} to int loses precision"),
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::Resource(resource)) => {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("Resource ID#{resource} used as offset, casting to integer ({resource})"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ArrayKey::Int(resource)
        }
        Err(ArrayKeyError::Illegal) => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "Cannot access offset of type {} on array",
                    key_value.diagnostic_type_name()
                ),
            ));
            return Ok(());
        }
    };
    let key = array.normalize_utf8_text_key(key, key_value);
    let exists = match key {
        ArrayKey::Int(key) => array.get_int(key).is_some(),
        ArrayKey::String(key) => array.get_str(&key).is_some(),
    };
    ret!(rv, Value::bool(exists));
}

fn fn_array_key_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_array_key_exists_named(ed, rv, eg, "array_key_exists")
}

fn fn_key_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_array_key_exists_named(ed, rv, eg, "key_exists")
}

fn array_change_key_case_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<i64>, VmError> {
    let argument = owned_argument(ed, 1);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::Long => argument.as_long(),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                "array_change_key_case(): Passing null to parameter #2 ($case) of type int is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(0)
        }
        ValueType::True | ValueType::False if !strict => Some(i64::from(argument.is_truthy())),
        ValueType::Double if !strict => {
            let number = argument.as_double().unwrap_or(f64::NAN);
            let upper_exclusive = -(i64::MIN as f64);
            if !number.is_finite() || number < i64::MIN as f64 || number >= upper_exclusive {
                None
            } else {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            argument.echo_to_string_with_precision(-1)
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                Some(integer)
            }
        }
        ValueType::String if !strict => {
            let source = argument.as_str().unwrap_or("");
            let Some(number) = php_numeric_string_to_float(source) else {
                typed_internal_argument_error(
                    eg,
                    "array_change_key_case",
                    argument,
                    2,
                    "case",
                    "int",
                );
                return Ok(None);
            };
            let upper_exclusive = -(i64::MIN as f64);
            if !number.is_finite() || number < i64::MIN as f64 || number >= upper_exclusive {
                None
            } else {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float-string \"{source}\" to int loses precision"
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                Some(integer)
            }
        }
        _ => None,
    };
    if converted.is_none() && eg.exception.is_none() {
        typed_internal_argument_error(eg, "array_change_key_case", argument, 2, "case", "int");
    }
    Ok(converted)
}

fn fn_array_change_key_case(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_change_key_case",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let case = if arg_opt!(ed, 1).is_some() {
        let Some(case) = array_change_key_case_argument(ed, eg)? else {
            return Ok(());
        };
        case
    } else {
        0
    };

    let mut result = PhpArray::new();
    for (key, value) in array.iter() {
        match key {
            ArrayKey::Int(index) => result.set_int(index, array_projection_value(value)),
            ArrayKey::String(mut name) => {
                if case != 0 {
                    name.make_ascii_uppercase();
                } else {
                    name.make_ascii_lowercase();
                }
                result.set_str(&name, array_projection_value(value));
            }
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_in_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let haystack = arg!(ed, 1);
    if haystack.as_array().is_none() {
        typed_internal_argument_error(eg, "in_array", haystack, 2, "haystack", "array");
        return Ok(());
    }
    let strict = if arg_opt!(ed, 2).is_some() {
        let Some(strict) = typed_internal_bool_argument(ed, eg, "in_array", 2, "strict")? else {
            return Ok(());
        };
        strict
    } else {
        false
    };

    // Reacquire the call-frame values after optional bool conversion, whose
    // null deprecation may dispatch user code.
    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1).as_array().unwrap();
    let numeric_string_needle = array_lookup_numeric_string_needle(needle, strict);
    for value in haystack.values() {
        let matches = match array_lookup_values_match(
            needle,
            value,
            strict,
            numeric_string_needle,
            eg.precision,
        ) {
            Ok(matches) => matches,
            Err(()) => {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        };
        if matches {
            ret!(rv, Value::bool(true));
        }
    }
    ret!(rv, Value::bool(false));
}

fn fn_array_reverse(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_reverse",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let preserve_keys = if arg_opt!(ed, 1).is_some() {
        let Some(preserve_keys) =
            typed_internal_bool_argument(ed, eg, "array_reverse", 1, "preserve_keys")?
        else {
            return Ok(());
        };
        preserve_keys
    } else {
        false
    };
    let key_policy = if preserve_keys {
        ArrayProjectionKeys::PreserveAll
    } else {
        ArrayProjectionKeys::PreserveStrings
    };

    if !preserve_keys && let Some(values) = array.packed_values() {
        let mut result = PhpArray::with_packed_capacity(values.len());
        append_projected_values(&mut result, values.iter().rev());
        ret!(rv, Value::array(result));
    }

    let mut result = if preserve_keys || array.has_string_keys() {
        PhpArray::with_deferred_hash_capacity(array.len())
    } else {
        PhpArray::with_packed_capacity(array.len())
    };
    let entries = array.iter().collect::<Vec<_>>();
    for (key, value) in entries.into_iter().rev() {
        array_projection_insert(&mut result, key, value, key_policy);
    }
    ret!(rv, Value::array(result));
}

fn fn_iterator_to_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0).dereferenced();
    let preserve_keys = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    let entries = if let Some(array) = source.as_array() {
        array
            .iter()
            .map(|(key, value)| (key, value.dereferenced().clone()))
            .collect()
    } else if let Some(entries) = crate::vm::execute::collect_traversable_entries(eg, source)? {
        entries
    } else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, {} given",
                source.type_name()
            ),
        ));
        ret!(rv, Value::null());
    };

    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    let mut result = PhpArray::new();
    for (key, value) in entries {
        if preserve_keys {
            match key {
                ArrayKey::Int(key) => result.set_int(key, value),
                ArrayKey::String(key) => result.set_str(&key, value),
            }
        } else {
            result.push(value);
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_is_list(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let is_list = arg!(ed, 0).as_array().is_some_and(|array| array.is_list());
    ret!(rv, Value::bool(is_list));
}

fn fn_array_merge(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_ARRAY_MERGE_INPUT_ELEMENTS: usize = 1 << 30;

    let Some(arrays) = arg!(ed, 0).as_array() else {
        ret!(rv, Value::array(PhpArray::new()));
    };
    for (index, value) in arrays.values().enumerate() {
        if value.as_array().is_some() {
            continue;
        }
        let actual = match value.dereferenced().value_type() {
            ValueType::True => "true".to_string(),
            ValueType::False => "false".to_string(),
            _ => value.dereferenced().diagnostic_type_name().into_owned(),
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "array_merge(): Argument #{} must be of type array, {actual} given",
                index + 1
            ),
        ));
        return Ok(());
    }
    let mut total = 0usize;
    let mut has_string_keys = false;
    for value in arrays.values() {
        let array = value
            .as_array()
            .expect("array_merge arguments were validated before sizing");
        total = total.saturating_add(array.len());
        if total >= MAX_ARRAY_MERGE_INPUT_ELEMENTS {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "The total number of elements must be lower than 1073741824",
            ));
            return Ok(());
        }
        has_string_keys |= array.has_string_keys();
    }

    let mut merged = if has_string_keys {
        PhpArray::with_deferred_hash_capacity(total)
    } else {
        PhpArray::with_packed_capacity(total)
    };
    for value in arrays.values() {
        let array = value
            .as_array()
            .expect("array_merge arguments were validated before allocation");
        for (key, val) in array.iter() {
            match &key {
                ArrayKey::Int(_) => merged.push(array_projection_value(val)),
                ArrayKey::String(k) => merged.set_str(k, array_projection_value(val)),
            }
        }
    }
    ret!(rv, Value::array(merged));
}

fn fn_array_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(first) = arg!(ed, 0).as_array() else {
        ret!(rv, Value::null());
    };
    let mut result = first.clone();
    let replacements = arg!(ed, 1);
    if let Some(replacements) = replacements.as_array() {
        for replacement in replacements.values() {
            let Some(replacement) = replacement.as_array() else {
                ret!(rv, Value::null());
            };
            for (key, value) in replacement.iter() {
                match key {
                    ArrayKey::Int(key) => result.set_int(key, value.clone()),
                    ArrayKey::String(key) => result.set_str(&key, value.clone()),
                }
            }
        }
    } else if replacements.value_type() != ValueType::Undef {
        ret!(rv, Value::null());
    }
    ret!(rv, Value::array(result));
}

#[inline(never)]
fn collect_array_keys(array: &PhpArray) -> PhpArray {
    let mut result = PhpArray::new();
    for (key, _) in array.iter() {
        match key {
            ArrayKey::Int(key) => result.push(Value::long(key)),
            ArrayKey::String(key) => result.push(Value::string(key)),
        }
    }
    result
}

fn fn_array_keys(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0);
    if source.as_array().is_none() {
        typed_internal_argument_error(eg, "array_keys", source, 1, "array", "array");
        return Ok(());
    }
    let has_filter = arg_opt!(ed, 1).is_some();
    if !has_filter && arg_opt!(ed, 2).is_some() {
        eg.exception = Some(crate::value::make_error_value(
            "ArgumentCountError",
            "array_keys(): Argument #2 ($filter_value) must be passed explicitly, because the default value is not known",
        ));
        return Ok(());
    }
    let strict = if has_filter && arg_opt!(ed, 2).is_some() {
        let Some(strict) = typed_internal_bool_argument(ed, eg, "array_keys", 2, "strict")? else {
            return Ok(());
        };
        strict
    } else {
        false
    };

    // Reacquire frame arguments after optional bool conversion, which may
    // dispatch a deprecation handler for a supplied null.
    let array = arg!(ed, 0).as_array().unwrap();
    if !has_filter {
        ret!(rv, Value::array(collect_array_keys(array)));
    }
    let filter = arg!(ed, 1);
    let mut result = PhpArray::with_packed_capacity(array.len());
    for (key, value) in array.iter() {
        let matches = match array_values_match(value, filter, strict, eg.precision) {
            Ok(matches) => matches,
            Err(()) => {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        };
        if matches {
            result.push(array_key_into_value(key));
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0);
    let Some(array) = source.as_array() else {
        typed_internal_argument_error(eg, "array_values", source, 1, "array", "array");
        return Ok(());
    };
    ret!(rv, Value::array(array.project_values()));
}

fn fn_array_slice(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_slice",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let Some(offset) = typed_internal_int_argument(ed, eg, "array_slice", 1, "offset")? else {
        return Ok(());
    };
    let length = if let Some(argument) = arg_opt!(ed, 2) {
        if argument.dereferenced().value_type() == ValueType::Null {
            None
        } else {
            let Some(length) =
                typed_internal_int_argument_expected(ed, eg, "array_slice", 2, "length", "?int")?
            else {
                return Ok(());
            };
            Some(length)
        }
    } else {
        None
    };
    let preserve_keys = if arg_opt!(ed, 3).is_some() {
        let Some(preserve_keys) =
            typed_internal_bool_argument(ed, eg, "array_slice", 3, "preserve_keys")?
        else {
            return Ok(());
        };
        preserve_keys
    } else {
        false
    };

    let array_length = i64::try_from(array.len()).unwrap_or(i64::MAX);
    let start = if offset < 0 {
        array_length.saturating_add(offset).max(0)
    } else {
        offset.min(array_length)
    };
    let end = match length {
        None => array_length,
        Some(length) if length < 0 => array_length.saturating_add(length).max(start),
        Some(length) => start.saturating_add(length).min(array_length),
    };
    let key_policy = if preserve_keys {
        ArrayProjectionKeys::PreserveAll
    } else {
        ArrayProjectionKeys::PreserveStrings
    };
    let start = start as usize;
    let result_length = end.saturating_sub(start as i64) as usize;
    if !preserve_keys && let Some(values) = array.packed_values() {
        let mut result = PhpArray::with_packed_capacity(result_length);
        append_projected_values(&mut result, values[start..start + result_length].iter());
        ret!(rv, Value::array(result));
    }

    let mut result = if preserve_keys || array.has_string_keys() {
        PhpArray::with_deferred_hash_capacity(result_length)
    } else {
        PhpArray::with_packed_capacity(result_length)
    };
    for (key, value) in array.iter().skip(start).take(result_length) {
        array_projection_insert(&mut result, key, value, key_policy);
    }
    ret!(rv, Value::array(result));
}

fn fn_array_unique(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_unique",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let flags = if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, "array_unique", 1, "flags")? else {
            return Ok(());
        };
        flags
    } else {
        SORT_STRING
    };
    if flags == SORT_STRING
        && arg_opt!(ed, 1).is_none()
        && let Some(values) = array.packed_values()
        && values
            .iter()
            .all(|value| value.value_type() == ValueType::String)
    {
        let mut result = PhpArray::new();
        let mut linear_seen = Vec::with_capacity(values.len().min(64));
        let mut hashed_seen = None::<std::collections::HashSet<&str>>;
        for (index, value) in values.iter().enumerate() {
            let rendered = value.as_str().unwrap_or("");
            let unseen = if let Some(seen) = hashed_seen.as_mut() {
                seen.insert(rendered)
            } else if linear_seen.contains(&rendered) {
                false
            } else if linear_seen.len() < 64 {
                linear_seen.push(rendered);
                true
            } else {
                let mut seen = std::collections::HashSet::with_capacity(values.len());
                seen.extend(linear_seen.drain(..));
                let unseen = seen.insert(rendered);
                hashed_seen = Some(seen);
                unseen
            };
            if unseen {
                result.set_int(index as i64, value.clone());
            }
        }
        ret!(rv, Value::array(result));
    }
    if matches!(flags & !SORT_FLAG_CASE, SORT_STRING | SORT_LOCALE_STRING) {
        let mut result = PhpArray::new();
        let mut linear_seen = Vec::with_capacity(array.len().min(64));
        let mut hashed_seen = None::<std::collections::HashSet<String>>;
        for (key, value) in array.iter() {
            let Some(mut rendered) = internal_value_to_string(ed, eg, value)? else {
                return Ok(());
            };
            if eg.exception.is_some() {
                return Ok(());
            }
            if flags & SORT_FLAG_CASE != 0 {
                rendered.make_ascii_lowercase();
            }
            let unseen = if let Some(seen) = hashed_seen.as_mut() {
                seen.insert(rendered)
            } else if linear_seen.contains(&rendered) {
                false
            } else if linear_seen.len() < 64 {
                linear_seen.push(rendered);
                true
            } else {
                let mut seen = std::collections::HashSet::with_capacity(array.len());
                seen.extend(linear_seen.drain(..));
                let unseen = seen.insert(rendered);
                hashed_seen = Some(seen);
                unseen
            };
            if unseen {
                array_projection_insert(&mut result, key, value, ArrayProjectionKeys::PreserveAll);
            }
        }
        ret!(rv, Value::array(result));
    }

    let entries = array
        .iter()
        .map(|(key, value)| (key, array_sort_snapshot_value(value)))
        .collect::<Vec<_>>();
    let mut result = PhpArray::with_deferred_hash_capacity(entries.len());
    let mut accepted = Vec::with_capacity(entries.len());
    for (key, value) in &entries {
        let mut duplicate = false;
        for previous in &accepted {
            if sort_value_order_runtime(ed, eg, previous, value, flags)?.is_eq() {
                duplicate = true;
                break;
            }
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        if eg.exception.is_some() {
            return Ok(());
        }
        if !duplicate {
            accepted.push(value.clone());
            array_projection_insert(
                &mut result,
                key.clone(),
                value,
                ArrayProjectionKeys::PreserveAll,
            );
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_flip(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(eg, "array_flip", source.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let mut result = PhpArray::with_packed_capacity(array.len());
    for (key, value) in array.iter() {
        let flipped_value = array_key_into_value(key);
        let value = value.dereferenced();
        match value.value_type() {
            ValueType::Long => result.set_int(value.as_long().unwrap(), flipped_value),
            ValueType::String => {
                if let Some(key) = value
                    .as_str()
                    .and_then(crate::value::canonical_decimal_array_key)
                {
                    result.set_int(key, flipped_value);
                } else {
                    result.set_str_value(value, flipped_value);
                }
            }
            _ => {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "array_flip(): Can only flip string and integer values, entry skipped",
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_combine(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let keys_argument = owned_argument(ed, 0);
    let Some(keys) = keys_argument.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_combine",
            keys_argument.dereferenced(),
            1,
            "keys",
            "array",
        );
        return Ok(());
    };
    let values_argument = owned_argument(ed, 1);
    let Some(values) = values_argument.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_combine",
            values_argument.dereferenced(),
            2,
            "values",
            "array",
        );
        return Ok(());
    };
    if keys.len() != values.len() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements",
        ));
        return Ok(());
    }

    // owned_argument() gives both inputs their PHP call-boundary COW snapshot.
    // Clone only the current pair before a key conversion that can reenter
    // user code instead of materializing two additional full vectors.
    // Optimistically retain packed storage when the first converted key is 0;
    // set() transitions safely if a later key proves the result associative.
    let starts_packed = keys.values().next().is_none_or(|key| {
        let key = key.dereferenced();
        key.as_long().or_else(|| {
            key.as_str()
                .and_then(crate::value::canonical_decimal_array_key)
        }) == Some(0)
    });
    let mut result = if starts_packed {
        PhpArray::with_packed_capacity(keys.len())
    } else {
        PhpArray::with_deferred_hash_capacity(keys.len())
    };
    for (key, value) in keys.values().zip(values.values()) {
        let scalar_key = {
            let key = key.dereferenced();
            key.as_long().map(ArrayKey::Int).or_else(|| {
                key.as_str().map(|key| {
                    crate::value::canonical_decimal_array_key(key)
                        .map_or_else(|| ArrayKey::String(key.to_string()), ArrayKey::Int)
                })
            })
        };
        if let Some(key) = scalar_key {
            result.set(key, array_projection_value(value));
            continue;
        }

        let key = array_projection_value(key);
        let value = array_projection_value(value);
        let Some(key) = array_constructor_key(ed, eg, &key)? else {
            return Ok(());
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        result.set(key, value);
    }
    ret!(rv, Value::array(result));
}

#[derive(Clone, Copy)]
enum ArrayAggregateNumber {
    Long(i64),
    Double(f64),
}

impl ArrayAggregateNumber {
    #[inline(always)]
    fn apply_long<const PRODUCT: bool>(&mut self, right: i64) {
        *self = match *self {
            Self::Long(left) => {
                let integer = if PRODUCT {
                    left.checked_mul(right)
                } else {
                    left.checked_add(right)
                };
                integer.map_or_else(
                    || {
                        Self::Double(if PRODUCT {
                            left as f64 * right as f64
                        } else {
                            left as f64 + right as f64
                        })
                    },
                    Self::Long,
                )
            }
            Self::Double(left) => Self::Double(if PRODUCT {
                left * right as f64
            } else {
                left + right as f64
            }),
        };
    }

    #[inline(always)]
    fn apply_double<const PRODUCT: bool>(&mut self, right: f64) {
        let left = match *self {
            Self::Long(value) => value as f64,
            Self::Double(value) => value,
        };
        *self = Self::Double(if PRODUCT { left * right } else { left + right });
    }

    #[inline(always)]
    fn apply_value<const PRODUCT: bool>(&mut self, operand: Value) {
        if let Some(operand) = operand.as_long() {
            self.apply_long::<PRODUCT>(operand);
        } else {
            self.apply_double::<PRODUCT>(operand.as_double().unwrap());
        }
    }

    #[inline(always)]
    fn into_value(self) -> Value {
        match self {
            Self::Long(value) => Value::long(value),
            Self::Double(value) => Value::double(value),
        }
    }
}

#[inline(never)]
fn aggregate_add_as_double(left: i64, right: f64) -> f64 {
    left as f64 + right
}

#[inline(never)]
fn aggregate_multiply_as_double(left: i64, right: f64) -> f64 {
    left as f64 * right
}

#[inline]
fn fast_packed_nonnegative_long_sum(values: &[Value]) -> Option<Value> {
    if values.is_empty() {
        return Some(Value::long(0));
    }
    let per_value_limit = i64::MAX / i64::try_from(values.len()).ok()?;
    let bit_width = 63 - per_value_limit.leading_zeros();
    let bitmask_limit = (1_u64 << bit_width) - 1;
    let mut sum = 0_i64;
    let mut chunks = values.chunks_exact(4);
    for chunk in &mut chunks {
        if chunk[0].value_type() != ValueType::Long
            || chunk[1].value_type() != ValueType::Long
            || chunk[2].value_type() != ValueType::Long
            || chunk[3].value_type() != ValueType::Long
        {
            return None;
        }
        let first = chunk[0].as_long().unwrap();
        let second = chunk[1].as_long().unwrap();
        let third = chunk[2].as_long().unwrap();
        let fourth = chunk[3].as_long().unwrap();
        if (first as u64 | second as u64 | third as u64 | fourth as u64) > bitmask_limit {
            return None;
        }
        // The per-value bound proves that the full nonnegative reduction fits
        // in a PHP integer, so no intermediate addition can overflow.
        sum = sum
            .wrapping_add(first)
            .wrapping_add(second)
            .wrapping_add(third)
            .wrapping_add(fourth);
    }
    for value in chunks.remainder() {
        if value.value_type() != ValueType::Long {
            return None;
        }
        let value = value.as_long().unwrap();
        if value as u64 > bitmask_limit {
            return None;
        }
        sum = sum.wrapping_add(value);
    }
    Some(Value::long(sum))
}

#[inline]
fn fast_packed_unit_long_product(values: &[Value]) -> Option<Value> {
    let mut product = 1_i64;
    for value in values {
        if value.value_type() != ValueType::Long {
            return None;
        }
        match value.as_long().unwrap() {
            1 => {}
            0 => product = 0,
            -1 => product = -product,
            _ => return None,
        }
    }
    Some(Value::long(product))
}

#[inline]
fn fast_packed_double_sum(values: &[Value]) -> Option<Value> {
    if values.is_empty() {
        return None;
    }
    let mut sum = 0.0_f64;
    for value in values {
        if value.value_type() != ValueType::Double {
            return None;
        }
        sum += value.as_double().unwrap();
    }
    Some(Value::double(sum))
}

#[inline]
fn fast_packed_double_product(values: &[Value]) -> Option<Value> {
    if values.is_empty() {
        return None;
    }
    let mut product = 1.0_f64;
    for value in values {
        if value.value_type() != ValueType::Double {
            return None;
        }
        product *= value.as_double().unwrap();
    }
    Some(Value::double(product))
}

#[inline]
fn fast_array_sum(array: &PhpArray) -> Option<Value> {
    if let Some(values) = array.packed_values() {
        if let Some(result) = fast_packed_nonnegative_long_sum(values) {
            return Some(result);
        }
        if let Some(result) = fast_packed_double_sum(values) {
            return Some(result);
        }
    }

    macro_rules! sum_values {
        ($values:expr) => {{
            let mut long = 0_i64;
            let mut double = 0.0_f64;
            let mut is_double = false;
            'values: for source in $values {
                let mut value = source;
                let operand = loop {
                    break match value.value_type() {
                        ValueType::Long => value.as_long().unwrap(),
                        ValueType::True => 1,
                        ValueType::False | ValueType::Null | ValueType::Undef => 0,
                        ValueType::Double => {
                            let right = value.as_double().unwrap();
                            if !is_double {
                                double = aggregate_add_as_double(long, right);
                                is_double = true;
                            } else {
                                double += right;
                            }
                            continue 'values;
                        }
                        ValueType::String => {
                            let operand = arithmetic_operator_operand(value).ok()?;
                            if operand.leading_numeric {
                                return None;
                            }
                            if let Some(right) = operand.value.as_long() {
                                right
                            } else {
                                let right = operand.value.as_double().unwrap();
                                if !is_double {
                                    double = aggregate_add_as_double(long, right);
                                    is_double = true;
                                } else {
                                    double += right;
                                }
                                continue 'values;
                            }
                        }
                        ValueType::Reference => {
                            value = value.dereferenced();
                            continue;
                        }
                        _ => return None,
                    };
                };
                if is_double {
                    double += operand as f64;
                } else if operand != 0 {
                    match long.checked_add(operand) {
                        Some(value) => long = value,
                        None => {
                            double = aggregate_add_as_double(long, operand as f64);
                            is_double = true;
                        }
                    }
                }
            }
            Some(if is_double {
                Value::double(double)
            } else {
                Value::long(long)
            })
        }};
    }

    if let Some(values) = array.packed_values() {
        sum_values!(values.iter())
    } else {
        sum_values!(array.values())
    }
}

#[inline]
fn fast_array_product(array: &PhpArray) -> Option<Value> {
    if let Some(values) = array.packed_values() {
        if let Some(result) = fast_packed_unit_long_product(values) {
            return Some(result);
        }
        if let Some(result) = fast_packed_double_product(values) {
            return Some(result);
        }
    }

    macro_rules! product_values {
        ($values:expr) => {{
            let mut long = 1_i64;
            let mut double = 1.0_f64;
            let mut is_double = false;
            'values: for source in $values {
                let mut value = source;
                let operand = loop {
                    break match value.value_type() {
                        ValueType::Long => value.as_long().unwrap(),
                        ValueType::True => 1,
                        ValueType::False | ValueType::Null | ValueType::Undef => 0,
                        ValueType::Double => {
                            let right = value.as_double().unwrap();
                            if !is_double {
                                double = aggregate_multiply_as_double(long, right);
                                is_double = true;
                            } else {
                                double *= right;
                            }
                            continue 'values;
                        }
                        ValueType::String => {
                            let operand = arithmetic_operator_operand(value).ok()?;
                            if operand.leading_numeric {
                                return None;
                            }
                            if let Some(right) = operand.value.as_long() {
                                right
                            } else {
                                let right = operand.value.as_double().unwrap();
                                if !is_double {
                                    double = aggregate_multiply_as_double(long, right);
                                    is_double = true;
                                } else {
                                    double *= right;
                                }
                                continue 'values;
                            }
                        }
                        ValueType::Reference => {
                            value = value.dereferenced();
                            continue;
                        }
                        _ => return None,
                    };
                };
                if is_double {
                    double *= operand as f64;
                } else if operand != 1 {
                    match long.checked_mul(operand) {
                        Some(value) => long = value,
                        None => {
                            double = aggregate_multiply_as_double(long, operand as f64);
                            is_double = true;
                        }
                    }
                }
            }
            Some(if is_double {
                Value::double(double)
            } else {
                Value::long(long)
            })
        }};
    }

    if let Some(values) = array.packed_values() {
        product_values!(values.iter())
    } else {
        product_values!(array.values())
    }
}

fn fn_array_aggregate_slow<const PRODUCT: bool>(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    source: Value,
) -> Result<(), VmError> {
    let function = if PRODUCT {
        "array_product"
    } else {
        "array_sum"
    };
    let noun = if PRODUCT {
        "Multiplication"
    } else {
        "Addition"
    };
    let array = source.dereferenced().as_array().unwrap();
    let mut result = if PRODUCT {
        ArrayAggregateNumber::Long(1)
    } else {
        ArrayAggregateNumber::Long(0)
    };

    macro_rules! aggregate_values {
        ($values:expr) => {
            for value in $values {
                let value = value.dereferenced();
                match value.value_type() {
                    ValueType::Long => {
                        result.apply_long::<PRODUCT>(value.as_long().unwrap());
                        continue;
                    }
                    ValueType::Double => {
                        result.apply_double::<PRODUCT>(value.as_double().unwrap());
                        continue;
                    }
                    ValueType::True => {
                        result.apply_long::<PRODUCT>(1);
                        continue;
                    }
                    ValueType::False | ValueType::Null | ValueType::Undef => {
                        result.apply_long::<PRODUCT>(0);
                        continue;
                    }
                    _ => {}
                }
                let operand = match arithmetic_operator_operand(value) {
                    Ok(operand) => {
                        if operand.leading_numeric {
                            report_internal_diagnostic(
                                eg,
                                ed,
                                2,
                                "Warning",
                                "A non-numeric value encountered",
                            )?;
                            if eg.exception.is_some() {
                                return Ok(());
                            }
                        }
                        Some(operand.value)
                    }
                    Err(()) => {
                        report_internal_diagnostic(
                            eg,
                            ed,
                            2,
                            "Warning",
                            &format!(
                                "{function}(): {noun} is not supported on type {}",
                                value.diagnostic_type_name()
                            ),
                        )?;
                        if eg.exception.is_some() {
                            return Ok(());
                        }
                        match value.value_type() {
                            ValueType::Resource => {
                                Some(Value::long(value.as_resource_id().unwrap()))
                            }
                            // A non-numeric string participates as zero after
                            // its function-specific warning. Containers and
                            // objects are skipped, which matters for product.
                            ValueType::String => Some(Value::long(0)),
                            _ => None,
                        }
                    }
                };
                if let Some(operand) = operand {
                    result.apply_value::<PRODUCT>(operand);
                }
            }
        };
    }
    if let Some(values) = array.packed_values() {
        aggregate_values!(values.iter());
    } else {
        aggregate_values!(array.values());
    }
    ret!(rv, result.into_value());
}

fn fn_array_sum(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0);
    let Some(array) = source.as_array() else {
        typed_internal_argument_error(eg, "array_sum", source, 1, "array", "array");
        return Ok(());
    };
    if let Some(result) = fast_array_sum(array) {
        ret!(rv, result);
    }
    fn_array_aggregate_slow::<false>(ed, rv, eg, owned_argument(ed, 0))
}

fn fn_array_product(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0);
    let Some(array) = source.as_array() else {
        typed_internal_argument_error(eg, "array_product", source, 1, "array", "array");
        return Ok(());
    };
    if let Some(result) = fast_array_product(array) {
        ret!(rv, result);
    }
    fn_array_aggregate_slow::<true>(ed, rv, eg, owned_argument(ed, 0))
}

fn fn_array_count_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_count_values",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let mut result = PhpArray::with_packed_capacity(array.len());
    for value in array.values() {
        let value = value.dereferenced();
        match value.value_type() {
            ValueType::Long => {
                let key = value.as_long().unwrap();
                if let Some(count) = result.get_int_mut(key) {
                    *count = Value::long(count.as_long().unwrap().saturating_add(1));
                } else {
                    result.set_int(key, Value::long(1));
                }
            }
            ValueType::String => {
                let key = value.as_str().unwrap();
                if let Some(key) = crate::value::canonical_decimal_array_key(key) {
                    if let Some(count) = result.get_int_mut(key) {
                        *count = Value::long(count.as_long().unwrap().saturating_add(1));
                    } else {
                        result.set_int(key, Value::long(1));
                    }
                } else if let Some(count) = result.get_str_mut(key) {
                    *count = Value::long(count.as_long().unwrap().saturating_add(1));
                } else {
                    result.set_str_value(value, Value::long(1));
                }
            }
            _ => {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "array_count_values(): Can only count string and integer values, entry skipped",
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
        }
    }
    ret!(rv, Value::array(result));
}

fn array_constructor_key(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<ArrayKey>, VmError> {
    let value = value.dereferenced();
    if let Some(key) = value.as_long() {
        return Ok(Some(ArrayKey::Int(key)));
    }
    if value.as_double().is_some_and(f64::is_nan) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "unexpected NAN value was coerced to string",
        )?;
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    let Some(key) = internal_value_to_string(ed, eg, value)? else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(Some(
        crate::value::canonical_decimal_array_key(&key)
            .map_or_else(|| ArrayKey::String(key), ArrayKey::Int),
    ))
}

fn fn_array_fill_keys(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let keys_argument = arg!(ed, 0);
    let Some(keys) = keys_argument
        .as_array()
        .map(|keys| keys.values().cloned().collect::<Vec<_>>())
    else {
        let actual = match keys_argument.value_type() {
            ValueType::True => "true".to_string(),
            ValueType::False => "false".to_string(),
            _ => keys_argument.diagnostic_type_name().into_owned(),
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "array_fill_keys(): Argument #1 ($keys) must be of type array, {actual} given"
            ),
        ));
        return Ok(());
    };
    let value = arg!(ed, 1).clone();
    let mut result = PhpArray::new();
    for source_key in &keys {
        let Some(key) = array_constructor_key(ed, eg, source_key)? else {
            return Ok(());
        };
        result.set(key, value.clone());
    }
    ret!(rv, Value::array(result));
}

fn fn_array_fill(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_ARRAY_FILL_COUNT: i64 = i32::MAX as i64;
    const MAX_MATERIALIZED_ARRAY_ELEMENTS: usize = 1 << 30;

    let Some(start) = typed_internal_int_argument(ed, eg, "array_fill", 0, "start_index")? else {
        return Ok(());
    };
    let Some(count) = typed_internal_int_argument(ed, eg, "array_fill", 1, "count")? else {
        return Ok(());
    };
    if count < 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_fill(): Argument #2 ($count) must be greater than or equal to 0",
        ));
        return Ok(());
    }
    if count > MAX_ARRAY_FILL_COUNT {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_fill(): Argument #2 ($count) is too large",
        ));
        return Ok(());
    }
    let count = count as usize;
    if count == 0 {
        ret!(rv, Value::array(PhpArray::new()));
    }
    if start.checked_add((count - 1) as i64).is_none() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Cannot add element to the array as the next element is already occupied",
        ));
        return Ok(());
    }
    if count >= MAX_MATERIALIZED_ARRAY_ELEMENTS {
        let (file, line) = internal_call_source(ed);
        let slot = std::mem::size_of::<Value>();
        return Err(VmError::Fatal(format!(
            "Possible integer overflow in memory allocation ({count} * {slot} + {slot}) in {file} on line {line}"
        )));
    }

    let value = owned_argument(ed, 2);
    let mut result = if start == 0 {
        PhpArray::with_packed_capacity(count)
    } else {
        PhpArray::with_deferred_hash_capacity(count)
    };
    if start == 0 {
        for _ in 0..count {
            result.push(value.clone());
        }
    } else {
        for index in 0..count {
            result.set_int(start + index as i64, value.clone());
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_pad(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const MAX_ARRAY_PAD_LENGTH: u64 = 1 << 30;

    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(eg, "array_pad", source.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let Some(length) = typed_internal_int_argument(ed, eg, "array_pad", 1, "length")? else {
        return Ok(());
    };
    let value = owned_argument(ed, 2);
    let target_length = length.unsigned_abs();
    if target_length > MAX_ARRAY_PAD_LENGTH {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_pad(): Argument #2 ($length) must not exceed the maximum allowed array size",
        ));
        return Ok(());
    }
    let target_length = target_length as usize;
    if target_length <= array.len() {
        ret!(rv, source.dereferenced().clone());
    }

    let pad_count = target_length - array.len();
    if let Some(values) = array.packed_values() {
        let mut result = PhpArray::with_packed_capacity(target_length);
        if length < 0 {
            for _ in 0..pad_count {
                result.push(value.clone());
            }
        }
        append_projected_values(&mut result, values.iter());
        if length >= 0 {
            for _ in 0..pad_count {
                result.push(value.clone());
            }
        }
        ret!(rv, Value::array(result));
    }

    let mut result = if array.has_string_keys() {
        PhpArray::with_deferred_hash_capacity(target_length)
    } else {
        PhpArray::with_packed_capacity(target_length)
    };
    if length < 0 {
        for _ in 0..pad_count {
            result.push(value.clone());
        }
    }
    for (key, value) in array.iter() {
        array_projection_insert(
            &mut result,
            key,
            value,
            ArrayProjectionKeys::PreserveStrings,
        );
    }
    if length >= 0 {
        for _ in 0..pad_count {
            result.push(value.clone());
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_array_chunk(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_chunk",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let Some(length) = typed_internal_int_argument(ed, eg, "array_chunk", 1, "length")? else {
        return Ok(());
    };
    if length < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_chunk(): Argument #2 ($length) must be greater than 0",
        ));
        return Ok(());
    }
    let preserve_keys = if arg_opt!(ed, 2).is_some() {
        let Some(preserve_keys) =
            typed_internal_bool_argument(ed, eg, "array_chunk", 2, "preserve_keys")?
        else {
            return Ok(());
        };
        preserve_keys
    } else {
        false
    };
    let key_policy = if preserve_keys {
        ArrayProjectionKeys::PreserveAll
    } else {
        ArrayProjectionKeys::ReindexAll
    };
    let length = length as usize;
    let result_length = array.len().div_ceil(length);
    let chunk_capacity = length.min(array.len());
    if !preserve_keys && let Some(values) = array.packed_values() {
        let mut result = PhpArray::with_packed_capacity(result_length);
        for values in values.chunks(length) {
            let mut chunk = PhpArray::with_packed_capacity(values.len());
            append_projected_values(&mut chunk, values.iter());
            result.push(Value::array(chunk));
        }
        ret!(rv, Value::array(result));
    }

    let mut result = PhpArray::with_packed_capacity(result_length);
    let mut chunk = if preserve_keys {
        PhpArray::with_deferred_hash_capacity(chunk_capacity)
    } else {
        PhpArray::with_packed_capacity(chunk_capacity)
    };
    let mut chunk_length = 0usize;
    for (key, value) in array.iter() {
        array_projection_insert(&mut chunk, key, value, key_policy);
        chunk_length += 1;
        if chunk_length == length {
            result.push(Value::array(chunk));
            chunk = if preserve_keys {
                PhpArray::with_deferred_hash_capacity(chunk_capacity)
            } else {
                PhpArray::with_packed_capacity(chunk_capacity)
            };
            chunk_length = 0;
        }
    }
    if chunk_length != 0 {
        result.push(Value::array(chunk));
    }
    ret!(rv, Value::array(result));
}

#[derive(Clone)]
enum ArrayColumnSelector {
    WholeRow,
    Key(ArrayKey),
}

fn typed_array_column_selector(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
) -> Result<Option<ArrayColumnSelector>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let selector = match argument.value_type() {
        ValueType::Null => Some(ArrayColumnSelector::WholeRow),
        ValueType::Long => Some(ArrayColumnSelector::Key(ArrayKey::Int(
            argument.as_long().unwrap(),
        ))),
        ValueType::String => {
            let key = argument.as_str().unwrap_or("");
            Some(ArrayColumnSelector::Key(
                crate::value::canonical_decimal_array_key(key)
                    .map_or_else(|| ArrayKey::String(key.to_string()), ArrayKey::Int),
            ))
        }
        ValueType::True | ValueType::False if !strict => Some(ArrayColumnSelector::Key(
            ArrayKey::Int(i64::from(argument.is_truthy())),
        )),
        ValueType::Double if !strict => {
            let number = argument.as_double().unwrap_or(f64::NAN);
            let upper_exclusive = -(i64::MIN as f64);
            if !number.is_finite() || number < i64::MIN as f64 || number >= upper_exclusive {
                None
            } else {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            argument.echo_to_string_with_precision(-1)
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                Some(ArrayColumnSelector::Key(ArrayKey::Int(integer)))
            }
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            rendered.and_then(|rendered| {
                let rendered = rendered.dereferenced();
                rendered.as_str().map(|key| {
                    ArrayColumnSelector::Key(
                        crate::value::canonical_decimal_array_key(key)
                            .map_or_else(|| ArrayKey::String(key.to_string()), ArrayKey::Int),
                    )
                })
            })
        }
        _ => None,
    };
    if selector.is_none() && eg.exception.is_none() {
        typed_internal_argument_error(
            eg,
            "array_column",
            argument,
            index as usize + 1,
            parameter,
            "string|int|null",
        );
    }
    Ok(selector)
}

#[inline]
fn array_column_array_value(array: &PhpArray, key: &ArrayKey) -> Option<Value> {
    let value = match key {
        ArrayKey::Int(key) => array.get_int(*key),
        ArrayKey::String(key) => array.get_str(key),
    }?;
    Some(value.dereferenced().clone())
}

fn array_column_object_value(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    object_value: &Value,
    key: &ArrayKey,
) -> Result<Option<Value>, VmError> {
    let name = match key {
        ArrayKey::Int(key) => key.to_string(),
        ArrayKey::String(key) => key.clone(),
    };
    let object = object_value
        .as_object()
        .expect("array_column object projection requires an object");
    let class_name = object.class_name.to_string();
    let class_id = object.class_id;

    let visibility = eg.find_property_visibility(&class_name, &name);
    if visibility
        .as_ref()
        .is_some_and(|(visibility, _)| *visibility == Visibility::Public)
    {
        if let Some(slot) = object.property_slot(&name) {
            let definition = eg.instance_property_definition(class_id, slot);
            if definition.is_some_and(|definition| definition.has_get_hook) {
                drop(object);
                return call_object_property_get_hook(eg, object_value, &name)
                    .map(|value| value.map(|value| value.dereferenced().clone()));
            }
            if let Some(value) = object.get_property_slot(slot)
                && !value.is_undef()
            {
                return Ok(Some(value.dereferenced().clone()));
            }
        }
    } else if visibility.is_none()
        && let Some((value, _)) = object.get_dynamic_property_with_position(&name)
        && !value.is_undef()
    {
        return Ok(Some(value.dereferenced().clone()));
    }
    drop(object);

    let Some(isset) = call_object_property_magic_isset(eg, object_value, &name)? else {
        return Ok(None);
    };
    if eg.exception.is_some() || !isset.is_truthy() {
        return Ok(None);
    }
    if let Some(value) = call_object_property_magic_get(eg, object_value, &name)? {
        return Ok(Some(value.dereferenced().clone()));
    }
    if eg.exception.is_some() {
        return Ok(None);
    }
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("Undefined property: {class_name}::${name}"),
    )?;
    Ok(eg.exception.is_none().then(Value::null))
}

fn array_column_row_value(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    row: &Value,
    selector: &ArrayColumnSelector,
) -> Result<Option<Value>, VmError> {
    if matches!(selector, ArrayColumnSelector::WholeRow) {
        return Ok(Some(array_projection_value(row)));
    }
    let ArrayColumnSelector::Key(key) = selector else {
        unreachable!();
    };
    let row = row.dereferenced();
    if let Some(array) = row.as_array() {
        return Ok(array_column_array_value(array, key));
    }
    if row.as_object().is_none() {
        return Ok(None);
    }
    let initialized = if eg.is_uninitialized_lazy_object(row) {
        Some(reflection::initialize_lazy_object(eg, row)?)
    } else {
        eg.lazy_proxy_instance(row)
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    array_column_object_value(ed, eg, initialized.as_ref().unwrap_or(row), key)
}

fn array_column_result_key(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<ArrayKey>, VmError> {
    let value = value.dereferenced();
    let key = match value_to_array_key(value) {
        Ok(key) => key,
        Err(ArrayKeyError::DeprecatedNull) => {
            report_internal_deprecation(
                eg,
                ed,
                "Using null as an array offset is deprecated, use an empty string instead",
            )?;
            ArrayKey::String(String::new())
        }
        Err(ArrayKeyError::DeprecatedFloat(integer)) => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "Implicit conversion from float {} to int loses precision",
                    value.echo_to_string_with_precision(-1)
                ),
            )?;
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::NonRepresentableFloat {
            integer,
            also_deprecated,
        }) => {
            let rendered = value.echo_to_string_with_precision(-1);
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("The float {rendered} is not representable as an int, cast occurred"),
            )?;
            if eg.exception.is_none() && also_deprecated {
                report_internal_deprecation(
                    eg,
                    ed,
                    &format!("Implicit conversion from float {rendered} to int loses precision"),
                )?;
            }
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::Resource(resource)) => {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("Resource ID#{resource} used as offset, casting to integer ({resource})"),
            )?;
            ArrayKey::Int(resource)
        }
        Err(ArrayKeyError::Illegal) => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "Cannot access offset of type {} on array",
                    value.diagnostic_type_name()
                ),
            ));
            return Ok(None);
        }
    };
    Ok(eg.exception.is_none().then_some(key))
}

fn fn_array_column(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_column",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let Some(column) = typed_array_column_selector(ed, eg, 1, "column_key")? else {
        return Ok(());
    };
    let index = if arg_opt!(ed, 2).is_some() {
        let Some(index) = typed_array_column_selector(ed, eg, 2, "index_key")? else {
            return Ok(());
        };
        (!matches!(index, ArrayColumnSelector::WholeRow)).then_some(index)
    } else {
        None
    };

    let mut result = PhpArray::with_packed_capacity(array.len());
    if index.is_none() {
        match &column {
            ArrayColumnSelector::WholeRow => {
                for row in array.values() {
                    result.push(array_projection_value(row));
                }
                ret!(rv, Value::array(result));
            }
            ArrayColumnSelector::Key(key) => {
                for row in array.values() {
                    let row = row.dereferenced();
                    if let Some(inner) = row.as_array() {
                        if let Some(value) = array_column_array_value(inner, key) {
                            result.push(value);
                        }
                        continue;
                    }
                    if row.as_object().is_none() {
                        continue;
                    }
                    let Some(value) = array_column_row_value(ed, eg, row, &column)? else {
                        if eg.exception.is_some() {
                            return Ok(());
                        }
                        continue;
                    };
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                    result.push(value);
                }
                ret!(rv, Value::array(result));
            }
        }
    }

    for row in array.values() {
        let Some(value) = array_column_row_value(ed, eg, row, &column)? else {
            if eg.exception.is_some() {
                return Ok(());
            }
            continue;
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        let key = if let Some(index) = index.as_ref() {
            array_column_row_value(ed, eg, row, index)?
        } else {
            None
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        if let Some(key) = key {
            let Some(key) = array_column_result_key(ed, eg, &key)? else {
                return Ok(());
            };
            result.set(key, value);
        } else {
            result.push(value);
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_sort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries = a
            .values()
            .map(array_sort_snapshot_value)
            .collect::<Vec<_>>();
        if !sort_direct_long_entries(&mut entries, flags, false, |value| value)
            && !sort_direct_total_scalar_entries(
                &mut entries,
                flags,
                false,
                eg.precision,
                |value| value,
            )
        {
            stable_sort_checked(&mut entries, |left, right| {
                sort_value_order_runtime(ed, eg, left, right, flags)
            })?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        let mut new = PhpArray::new();
        for value in entries {
            new.push(array_projection_value(&value));
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_rsort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries = a
            .values()
            .map(array_sort_snapshot_value)
            .collect::<Vec<_>>();
        if !sort_direct_long_entries(&mut entries, flags, true, |value| value)
            && !sort_direct_total_scalar_entries(&mut entries, flags, true, eg.precision, |value| {
                value
            })
        {
            stable_sort_checked(&mut entries, |left, right| {
                sort_value_order_runtime(ed, eg, left, right, flags)
                    .map(std::cmp::Ordering::reverse)
            })?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        let mut new = PhpArray::new();
        for value in entries {
            new.push(array_projection_value(&value));
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

const SORT_REGULAR: i64 = 0;
const SORT_NUMERIC: i64 = 1;
const SORT_STRING: i64 = 2;
const SORT_DESC: i64 = 3;
const SORT_ASC: i64 = 4;
const SORT_LOCALE_STRING: i64 = 5;

struct MultisortColumn {
    entries: Vec<(ArrayKey, Value)>,
    direction: i64,
    direction_set: bool,
    flags: i64,
    flags_set: bool,
    destination: *mut Value,
}

#[derive(Clone, Copy)]
enum MultisortFlagKind {
    Direction,
    Comparison,
}

#[derive(Clone, Copy)]
enum MultisortArgumentViolation {
    NotArrayOrFlag,
    DuplicateFlag,
    InvalidFlag,
}

fn multisort_flag_kind(flag: i64) -> Option<MultisortFlagKind> {
    if matches!(flag, SORT_ASC | SORT_DESC) {
        Some(MultisortFlagKind::Direction)
    } else if matches!(
        flag & !SORT_FLAG_CASE,
        SORT_REGULAR | SORT_NUMERIC | SORT_STRING | SORT_LOCALE_STRING | SORT_NATURAL
    ) {
        Some(MultisortFlagKind::Comparison)
    } else {
        None
    }
}

fn multisort_argument_error(
    eg: &mut ExecutorGlobals,
    position: usize,
    violation: MultisortArgumentViolation,
) {
    let parameter = if position == 1 { " ($array)" } else { "" };
    let (class, requirement) = match violation {
        MultisortArgumentViolation::NotArrayOrFlag => {
            ("TypeError", "must be an array or a sort flag")
        }
        MultisortArgumentViolation::DuplicateFlag => (
            "TypeError",
            "must be an array or a sort flag that has not already been specified",
        ),
        MultisortArgumentViolation::InvalidFlag => ("ValueError", "must be a valid sort flag"),
    };
    eg.exception = Some(crate::value::make_error_value(
        class,
        &format!("array_multisort(): Argument #{position}{parameter} {requirement}"),
    ));
}

fn multisort_array_value(value: &Value) -> Option<Vec<(ArrayKey, Value)>> {
    value.as_array().map(|array| {
        array
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect()
    })
}

fn multisort_rebuild(entries: &[(ArrayKey, Value)], order: &[usize]) -> Value {
    let mut result = PhpArray::new();
    for &index in order {
        let (key, value) = &entries[index];
        match key {
            ArrayKey::Int(_) => {
                result.push(array_projection_value(value));
            }
            ArrayKey::String(key) => {
                result.set_str(key, array_projection_value(value));
            }
        }
    }
    Value::array(result)
}

/// array_multisort(array &$array, mixed &...$rest): bool
///
/// Arrays form lexicographic sort columns. Direction and comparison flags
/// following an array apply to that column; all columns are permuted together.
fn fn_array_multisort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // SAFETY: the registered signature requires CV 0 and the internal handler
    // is called only while its ExecuteData frame and argument slots are live.
    let first_raw = unsafe { (*ed).cv_mut(0) as *mut Value };
    // SAFETY: `first_raw` is the initialized CV established above; reference
    // arguments keep their target alive for the complete synchronous call.
    let first_value = unsafe { (&*first_raw).dereferenced() };
    if first_value.is_undef() {
        eg.exception = Some(crate::value::make_error_value(
            "ArgumentCountError",
            "array_multisort() expects at least 1 argument, 0 given",
        ));
        return Ok(());
    }
    let Some(first_entries) = multisort_array_value(first_value) else {
        let violation = match first_value.as_long() {
            Some(flag) if multisort_flag_kind(flag).is_some() => {
                MultisortArgumentViolation::DuplicateFlag
            }
            Some(_) => MultisortArgumentViolation::InvalidFlag,
            None => MultisortArgumentViolation::NotArrayOrFlag,
        };
        multisort_argument_error(eg, 1, violation);
        return Ok(());
    };
    // SAFETY: the raw CV is live, and an explicit reference owns or borrows a
    // target whose lifetime covers this handler invocation.
    let first_destination = unsafe {
        if (*first_raw).is_reference() {
            (*first_raw).as_ref_ptr()
        } else {
            first_raw
        }
    };
    let mut columns = vec![MultisortColumn {
        entries: first_entries,
        direction: SORT_ASC,
        direction_set: false,
        flags: SORT_REGULAR,
        flags_set: false,
        destination: first_destination,
    }];
    let expected_len = columns[0].entries.len();

    if let Some(rest) = arg!(ed, 1).as_array() {
        for (offset, argument) in rest.values().enumerate() {
            let value = argument.dereferenced();
            if let Some(entries) = multisort_array_value(value) {
                if entries.len() != expected_len {
                    eg.exception = Some(crate::value::make_error_value(
                        "ValueError",
                        "Array sizes are inconsistent",
                    ));
                    return Ok(());
                }
                // SAFETY: each reference stored in the live variadic bucket
                // retains its target through the synchronous handler call.
                let destination = if argument.is_reference() {
                    unsafe { argument.as_ref_ptr() }
                } else {
                    std::ptr::null_mut()
                };
                columns.push(MultisortColumn {
                    entries,
                    direction: SORT_ASC,
                    direction_set: false,
                    flags: SORT_REGULAR,
                    flags_set: false,
                    destination,
                });
                continue;
            }

            let position = offset + 2;
            let Some(flag) = value.as_long() else {
                multisort_argument_error(eg, position, MultisortArgumentViolation::NotArrayOrFlag);
                return Ok(());
            };
            let column = columns.last_mut().expect("the first sort column exists");
            match multisort_flag_kind(flag) {
                Some(MultisortFlagKind::Direction) => {
                    if column.direction_set {
                        multisort_argument_error(
                            eg,
                            position,
                            MultisortArgumentViolation::DuplicateFlag,
                        );
                        return Ok(());
                    }
                    column.direction = flag;
                    column.direction_set = true;
                }
                Some(MultisortFlagKind::Comparison) => {
                    if column.flags_set {
                        multisort_argument_error(
                            eg,
                            position,
                            MultisortArgumentViolation::DuplicateFlag,
                        );
                        return Ok(());
                    }
                    column.flags = flag;
                    column.flags_set = true;
                }
                None => {
                    multisort_argument_error(eg, position, MultisortArgumentViolation::InvalidFlag);
                    return Ok(());
                }
            }
        }
    }

    let mut order: Vec<usize> = (0..expected_len).collect();
    if columns.iter().all(|column| {
        sort_domain_has_total_order(&column.entries, column.flags, |(_, value)| value)
    }) {
        order.sort_by(|left, right| {
            for column in &columns {
                let ordering = sort_value_order(
                    &column.entries[*left].1,
                    &column.entries[*right].1,
                    column.flags,
                    eg.precision,
                )
                .unwrap_or(std::cmp::Ordering::Equal);
                let ordering = if column.direction == SORT_DESC {
                    ordering.reverse()
                } else {
                    ordering
                };
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            std::cmp::Ordering::Equal
        });
    } else {
        stable_sort_checked(&mut order, |left, right| {
            for column in &columns {
                let ordering = sort_value_order_runtime(
                    ed,
                    eg,
                    &column.entries[*left].1,
                    &column.entries[*right].1,
                    column.flags,
                )?;
                let ordering = if column.direction == SORT_DESC {
                    ordering.reverse()
                } else {
                    ordering
                };
                if ordering != std::cmp::Ordering::Equal {
                    return Ok(ordering);
                }
            }
            Ok(std::cmp::Ordering::Equal)
        })?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    for column in columns {
        let sorted = multisort_rebuild(&column.entries, &order);
        if !column.destination.is_null() {
            // SAFETY: destinations are either the live fixed CV or targets of
            // reference handles retained by the live variadic argument array.
            unsafe { *column.destination = sorted };
        }
    }
    ret!(rv, Value::bool(true));
}

fn fn_array_search(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let haystack = arg!(ed, 1);
    if haystack.as_array().is_none() {
        typed_internal_argument_error(eg, "array_search", haystack, 2, "haystack", "array");
        return Ok(());
    }
    let strict = if arg_opt!(ed, 2).is_some() {
        let Some(strict) = typed_internal_bool_argument(ed, eg, "array_search", 2, "strict")?
        else {
            return Ok(());
        };
        strict
    } else {
        false
    };

    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1).as_array().unwrap();
    let numeric_string_needle = array_lookup_numeric_string_needle(needle, strict);
    for (key, value) in haystack.iter() {
        let matches = match array_lookup_values_match(
            needle,
            value,
            strict,
            numeric_string_needle,
            eg.precision,
        ) {
            Ok(matches) => matches,
            Err(()) => {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        };
        if matches {
            ret!(rv, array_key_into_value(key));
        }
    }
    ret!(rv, Value::bool(false));
}

#[derive(Clone, Copy)]
enum RangeNumber {
    Int(i64),
    Float(f64),
}

struct RangeStringEndpoint {
    bytes: Vec<u8>,
    numeric: Option<RangeNumber>,
}

enum RangeEndpoint {
    Number(RangeNumber),
    String(RangeStringEndpoint),
}

#[derive(Clone, Copy)]
enum RangeStep {
    Int(i64),
    Float(f64),
}

impl RangeStep {
    fn number(self) -> f64 {
        match self {
            Self::Int(number) => number as f64,
            Self::Float(number) => number,
        }
    }

    fn is_integral(self) -> bool {
        match self {
            Self::Int(_) => true,
            Self::Float(number) => number.fract() == 0.0,
        }
    }

    fn integer_magnitude(self) -> Option<u128> {
        match self {
            Self::Int(number) => Some(u128::from(number.unsigned_abs())),
            Self::Float(number)
                if number.is_finite()
                    && number.fract() == 0.0
                    && number.abs() <= u64::MAX as f64 =>
            {
                Some(number.abs() as u128)
            }
            Self::Float(_) => None,
        }
    }
}

fn range_numeric_string(value: &str) -> Option<RangeNumber> {
    let value = value.trim_matches(|character: char| character.is_ascii_whitespace());
    if let Ok(integer) = value.parse::<i64>() {
        return Some(RangeNumber::Int(integer));
    }
    php_numeric_string_to_float(value).map(RangeNumber::Float)
}

fn range_endpoint_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
) -> Result<Option<RangeEndpoint>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let endpoint = match argument.value_type() {
        ValueType::Long => RangeEndpoint::Number(RangeNumber::Int(
            argument.as_long().expect("long argument has long payload"),
        )),
        ValueType::Double => RangeEndpoint::Number(RangeNumber::Float(
            argument
                .as_double()
                .expect("double argument has double payload"),
        )),
        ValueType::String => {
            let source = argument.as_str().unwrap_or("");
            RangeEndpoint::String(RangeStringEndpoint {
                bytes: php_string_to_bytes(source),
                numeric: range_numeric_string(source),
            })
        }
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "range(): Passing null to parameter #{} (${parameter}) of type string|int|float is deprecated",
                    index + 1
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            RangeEndpoint::Number(RangeNumber::Int(0))
        }
        ValueType::True | ValueType::False if !strict => {
            RangeEndpoint::Number(RangeNumber::Int(i64::from(argument.is_truthy())))
        }
        _ => {
            typed_internal_argument_error(
                eg,
                "range",
                argument,
                index as usize + 1,
                parameter,
                "string|int|float",
            );
            return Ok(None);
        }
    };
    Ok(Some(endpoint))
}

fn range_step_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<RangeStep>, VmError> {
    let Some(_) = arg_opt!(ed, 2) else {
        return Ok(Some(RangeStep::Int(1)));
    };
    let argument = owned_argument(ed, 2);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let step = match argument.value_type() {
        ValueType::Long => {
            RangeStep::Int(argument.as_long().expect("long argument has long payload"))
        }
        ValueType::Double => RangeStep::Float(
            argument
                .as_double()
                .expect("double argument has double payload"),
        ),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                "range(): Passing null to parameter #3 ($step) of type int|float is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            RangeStep::Int(0)
        }
        ValueType::True | ValueType::False if !strict => {
            RangeStep::Int(i64::from(argument.is_truthy()))
        }
        ValueType::String if !strict => match range_numeric_string(argument.as_str().unwrap_or(""))
        {
            Some(RangeNumber::Int(number)) => RangeStep::Int(number),
            Some(RangeNumber::Float(number)) => RangeStep::Float(number),
            None => {
                typed_internal_argument_error(eg, "range", argument, 3, "step", "int|float");
                return Ok(None);
            }
        },
        _ => {
            typed_internal_argument_error(eg, "range", argument, 3, "step", "int|float");
            return Ok(None);
        }
    };
    Ok(Some(step))
}

fn range_non_finite_name(number: f64) -> &'static str {
    if number.is_nan() {
        "NAN"
    } else if number.is_sign_negative() {
        "-INF"
    } else {
        "INF"
    }
}

fn range_endpoint_finite_error(
    eg: &mut ExecutorGlobals,
    endpoint: &RangeEndpoint,
    position: usize,
    parameter: &str,
) -> bool {
    let number = match endpoint {
        RangeEndpoint::Number(RangeNumber::Float(number))
        | RangeEndpoint::String(RangeStringEndpoint {
            numeric: Some(RangeNumber::Float(number)),
            ..
        }) => Some(*number),
        _ => None,
    };
    let Some(number) = number.filter(|number| !number.is_finite()) else {
        return false;
    };
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        &format!(
            "range(): Argument #{position} (${parameter}) must be a finite number, {} provided",
            range_non_finite_name(number)
        ),
    ));
    true
}

fn range_value_error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(crate::value::make_error_value("ValueError", message));
}

fn range_character_candidate(start: &RangeEndpoint, end: &RangeEndpoint) -> bool {
    let (RangeEndpoint::String(start), RangeEndpoint::String(end)) = (start, end) else {
        return false;
    };
    if start.bytes.is_empty() || end.bytes.is_empty() {
        return false;
    }
    (start.bytes.len() == 1 && end.bytes.len() == 1)
        || (start.numeric.is_none() && end.numeric.is_none())
}

fn range_warn_empty_strings(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    endpoints: [&RangeEndpoint; 2],
) -> Result<bool, VmError> {
    for (index, endpoint) in endpoints.into_iter().enumerate() {
        let RangeEndpoint::String(endpoint) = endpoint else {
            continue;
        };
        if !endpoint.bytes.is_empty() {
            continue;
        }
        let parameter = if index == 0 { "start" } else { "end" };
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "range(): Argument #{} (${parameter}) must not be empty, casted to 0",
                index + 1
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn range_warn_character_widths(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    endpoints: [&RangeEndpoint; 2],
) -> Result<bool, VmError> {
    for (index, endpoint) in endpoints.into_iter().enumerate() {
        let RangeEndpoint::String(endpoint) = endpoint else {
            continue;
        };
        if endpoint.bytes.len() <= 1 {
            continue;
        }
        let parameter = if index == 0 { "start" } else { "end" };
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "range(): Argument #{} (${parameter}) must be a single byte, subsequent bytes are ignored",
                index + 1
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn range_character_values(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    start: &RangeEndpoint,
    end: &RangeEndpoint,
    step: RangeStep,
) -> Result<Option<PhpArray>, VmError> {
    if !range_warn_character_widths(ed, eg, [start, end])? {
        return Ok(None);
    }
    let (RangeEndpoint::String(start), RangeEndpoint::String(end)) = (start, end) else {
        unreachable!("character ranges require two strings")
    };
    let start = u16::from(start.bytes[0]);
    let end = u16::from(end.bytes[0]);
    let signed_step = step.number();
    if start < end && signed_step.is_sign_negative() {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be greater than 0 for increasing ranges",
        );
        return Ok(None);
    }
    if start == end {
        let mut result = PhpArray::with_packed_capacity(1);
        result.push(Value::string(bytes_to_php_string(&[start as u8])));
        return Ok(Some(result));
    }
    let distance = start.abs_diff(end);
    let step = step
        .integer_magnitude()
        .expect("character range admitted only an integral finite step");
    if step > u128::from(distance) {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)",
        );
        return Ok(None);
    }
    let count = usize::from(distance) / step as usize + 1;
    let increasing = start < end;
    let mut result = PhpArray::with_packed_capacity(count);
    for index in 0..count {
        let delta = index * step as usize;
        let byte = if increasing {
            start as usize + delta
        } else {
            start as usize - delta
        } as u8;
        result.push(Value::string(bytes_to_php_string(&[byte])));
    }
    Ok(Some(result))
}

fn range_warn_mixed_character(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    character_position: usize,
) -> Result<bool, VmError> {
    let (other_position, other_parameter, character_parameter) = if character_position == 1 {
        (2, "end", "start")
    } else {
        (1, "start", "end")
    };
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "range(): Argument #{other_position} (${other_parameter}) must be a single byte string if argument #{character_position} (${character_parameter}) is a single byte string, argument #{character_position} (${character_parameter}) converted to 0"
        ),
    )?;
    Ok(eg.exception.is_none())
}

fn range_numeric_endpoint(endpoint: &RangeEndpoint) -> Option<RangeNumber> {
    match endpoint {
        RangeEndpoint::Number(number) => Some(*number),
        RangeEndpoint::String(endpoint) => endpoint.numeric,
    }
}

fn range_float_text(number: f64) -> String {
    let mut rendered = Value::double(number).echo_to_string_with_precision(-1);
    if !rendered.contains(['.', 'E']) {
        rendered.push_str(".0");
    }
    rendered
}

fn range_too_large_integer(
    eg: &mut ExecutorGlobals,
    start: i64,
    end: i64,
    step: u128,
    increments: u128,
) {
    const MAX_RANGE_SIZE: u128 = 1 << 30;
    let excess = increments.saturating_sub(MAX_RANGE_SIZE - 1);
    range_value_error(
        eg,
        &format!(
            "The supplied range exceeds the maximum array size by {excess} elements: start={start}, end={end}, step={step}. Calculated size: {increments}. Maximum size: {MAX_RANGE_SIZE}."
        ),
    );
}

fn range_too_large_float(
    eg: &mut ExecutorGlobals,
    start: f64,
    end: f64,
    step: f64,
    increments: f64,
) {
    const MAX_RANGE_SIZE: f64 = (1_u64 << 30) as f64;
    let excess = increments - (MAX_RANGE_SIZE - 1.0);
    range_value_error(
        eg,
        &format!(
            "The supplied range exceeds the maximum array size by {} elements: start={}, end={}, step={}. Max size: {}",
            range_float_text(excess),
            range_float_text(start),
            range_float_text(end),
            range_float_text(step),
            MAX_RANGE_SIZE as u64,
        ),
    );
}

fn range_materialization_fatal(ed: *mut ExecuteData, count: usize) -> VmError {
    let (file, line) = internal_call_source(ed);
    let bytes = count.saturating_mul(std::mem::size_of::<Value>());
    VmError::Fatal(format!(
        "Allowed memory size exhausted while constructing an array of {count} elements (tried to allocate {bytes} bytes) in {file} on line {line}"
    ))
}

fn range_integer_values(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    start: i64,
    end: i64,
    step: RangeStep,
) -> Result<Option<PhpArray>, VmError> {
    const MAX_RANGE_SIZE: u128 = 1 << 30;
    if start < end && step.number().is_sign_negative() {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be greater than 0 for increasing ranges",
        );
        return Ok(None);
    }
    if start == end {
        let mut result = PhpArray::with_packed_capacity(1);
        result.push(Value::long(start));
        return Ok(Some(result));
    }
    let distance = u128::from(start.abs_diff(end));
    let Some(step_magnitude) = step.integer_magnitude() else {
        return Ok(None);
    };
    if step_magnitude > distance {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)",
        );
        return Ok(None);
    }
    let increments = distance / step_magnitude;
    if increments >= MAX_RANGE_SIZE {
        range_too_large_integer(eg, start, end, step_magnitude, increments);
        return Ok(None);
    }
    let count = increments as usize + 1;
    if count >= MAX_RANGE_SIZE as usize {
        return Err(range_materialization_fatal(ed, count));
    }
    let increasing = start < end;
    let mut result = PhpArray::with_packed_capacity(count);
    for index in 0..count {
        let delta = (step_magnitude * index as u128) as i128;
        let number = if increasing {
            i128::from(start) + delta
        } else {
            i128::from(start) - delta
        };
        result.push(Value::long(number as i64));
    }
    Ok(Some(result))
}

fn range_float_values(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    start: f64,
    end: f64,
    step: RangeStep,
) -> Result<Option<PhpArray>, VmError> {
    const MAX_RANGE_SIZE: f64 = (1_u64 << 30) as f64;
    let signed_step = step.number();
    if start < end && signed_step.is_sign_negative() {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be greater than 0 for increasing ranges",
        );
        return Ok(None);
    }
    if start == end {
        let mut result = PhpArray::with_packed_capacity(1);
        result.push(Value::double(start));
        return Ok(Some(result));
    }
    let distance = (end - start).abs();
    let step_magnitude = signed_step.abs();
    if step_magnitude > distance {
        range_value_error(
            eg,
            "range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)",
        );
        return Ok(None);
    }
    let quotient = distance / step_magnitude;
    let nearest = quotient.round();
    let tolerance = f64::EPSILON * quotient.abs().max(1.0) * 8.0;
    let increments = if (quotient - nearest).abs() <= tolerance {
        nearest
    } else {
        quotient.floor()
    };
    if increments >= MAX_RANGE_SIZE {
        range_too_large_float(eg, start, end, step_magnitude, increments);
        return Ok(None);
    }
    let count = increments as usize + 1;
    if count >= MAX_RANGE_SIZE as usize {
        return Err(range_materialization_fatal(ed, count));
    }
    let increasing = start < end;
    let mut result = PhpArray::with_packed_capacity(count);
    for index in 0..count {
        let delta = step_magnitude * index as f64;
        let number = if increasing {
            start + delta
        } else {
            start - delta
        };
        result.push(Value::double(number));
    }
    Ok(Some(result))
}

fn fn_range(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(start) = range_endpoint_argument(ed, eg, 0, "start")? else {
        return Ok(());
    };
    if range_endpoint_finite_error(eg, &start, 1, "start") {
        return Ok(());
    }
    let Some(end) = range_endpoint_argument(ed, eg, 1, "end")? else {
        return Ok(());
    };
    if range_endpoint_finite_error(eg, &end, 2, "end") {
        return Ok(());
    }
    let Some(step) = range_step_argument(ed, eg)? else {
        return Ok(());
    };
    let step_number = step.number();
    if !step_number.is_finite() {
        range_value_error(
            eg,
            &format!(
                "range(): Argument #3 ($step) must be a finite number, {} provided",
                range_non_finite_name(step_number)
            ),
        );
        return Ok(());
    }
    if step_number == 0.0 {
        range_value_error(eg, "range(): Argument #3 ($step) cannot be 0");
        return Ok(());
    }
    if !range_warn_empty_strings(ed, eg, [&start, &end])? {
        return Ok(());
    }

    let character_candidate = range_character_candidate(&start, &end);
    if character_candidate && step.is_integral() {
        let Some(result) = range_character_values(ed, eg, &start, &end, step)? else {
            return Ok(());
        };
        ret!(rv, Value::array(result));
    }

    let mut start_number = range_numeric_endpoint(&start);
    let mut end_number = range_numeric_endpoint(&end);
    if character_candidate && start_number.is_none() && end_number.is_none() {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        start_number = Some(RangeNumber::Float(0.0));
        end_number = Some(RangeNumber::Float(0.0));
    } else {
        if start_number.is_none() {
            if matches!(&start, RangeEndpoint::String(value) if value.bytes.len() == 1)
                && !range_warn_mixed_character(ed, eg, 1)?
            {
                return Ok(());
            }
            start_number = Some(RangeNumber::Int(0));
        }
        if end_number.is_none() {
            if matches!(&end, RangeEndpoint::String(value) if value.bytes.len() == 1)
                && !range_warn_mixed_character(ed, eg, 2)?
            {
                return Ok(());
            }
            end_number = Some(RangeNumber::Int(0));
        }
    }

    let start_number = start_number.expect("range start was normalized");
    let end_number = end_number.expect("range end was normalized");
    if let (RangeNumber::Int(start), RangeNumber::Int(end)) = (start_number, end_number)
        && step.integer_magnitude().is_some()
    {
        let Some(result) = range_integer_values(ed, eg, start, end, step)? else {
            return Ok(());
        };
        ret!(rv, Value::array(result));
    }

    let start = match start_number {
        RangeNumber::Int(number) => number as f64,
        RangeNumber::Float(number) => number,
    };
    let end = match end_number {
        RangeNumber::Int(number) => number as f64,
        RangeNumber::Float(number) => number,
    };
    let Some(result) = range_float_values(ed, eg, start, end, step)? else {
        return Ok(());
    };
    ret!(rv, Value::array(result));
}

fn fn_array_splice(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let target = unsafe { &mut *ptr };
    if target.as_array().is_none() {
        typed_internal_argument_error(
            eg,
            "array_splice",
            target.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    }
    let Some(offset) = typed_internal_int_argument(ed, eg, "array_splice", 1, "offset")? else {
        return Ok(());
    };
    let length = if arg_opt!(ed, 2)
        .is_none_or(|value| value.dereferenced().value_type() == ValueType::Null)
    {
        None
    } else {
        let Some(length) =
            typed_internal_int_argument_expected(ed, eg, "array_splice", 2, "length", "?int")?
        else {
            return Ok(());
        };
        Some(length)
    };

    let replacement = arg_opt!(ed, 3).map(|_| owned_argument(ed, 3));
    let replacement = match replacement.as_ref().map(Value::dereferenced) {
        None => Vec::new(),
        Some(value) if value.value_type() == ValueType::Null => Vec::new(),
        Some(value) if value.as_array().is_some() => value
            .as_array()
            .expect("checked array replacement")
            .values()
            .map(array_projection_value)
            .collect::<Vec<_>>(),
        Some(value) if value.value_type() == ValueType::Object => {
            let projected = crate::vm::execute::cast_object_to_array(value, eg);
            if eg.exception.is_some() {
                return Ok(());
            }
            projected
                .as_array()
                .expect("object replacement projection must be an array")
                .values()
                .map(array_projection_value)
                .collect::<Vec<_>>()
        }
        Some(value) => vec![value.clone()],
    };

    let target = unsafe { &mut *ptr };
    let source = target
        .as_array()
        .expect("array_splice target was validated before conversion");
    let entries = source
        .iter()
        .map(|(key, value)| (key, array_projection_value(value)))
        .collect::<Vec<_>>();
    let source_len = entries.len();
    let start = if offset < 0 {
        source_len.saturating_sub(
            usize::try_from(offset.unsigned_abs())
                .unwrap_or(usize::MAX)
                .min(source_len),
        )
    } else {
        usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(source_len)
    };
    let end = match length {
        None => source_len,
        Some(length) if length >= 0 => start.saturating_add(
            usize::try_from(length)
                .unwrap_or(usize::MAX)
                .min(source_len - start),
        ),
        Some(length) => source_len
            .saturating_sub(
                usize::try_from(length.unsigned_abs())
                    .unwrap_or(usize::MAX)
                    .min(source_len),
            )
            .max(start),
    };
    let removed_len = end - start;
    let Some(result_len) = source_len
        .checked_sub(removed_len)
        .and_then(|length| length.checked_add(replacement.len()))
    else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "The total number of elements must be lower than 1073741824",
        ));
        return Ok(());
    };
    if result_len >= 1 << 30 {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "The total number of elements must be lower than 1073741824",
        ));
        return Ok(());
    }

    let result_has_strings = entries[..start]
        .iter()
        .chain(&entries[end..])
        .any(|(key, _)| matches!(key, ArrayKey::String(_)));
    let removed_has_strings = entries[start..end]
        .iter()
        .any(|(key, _)| matches!(key, ArrayKey::String(_)));
    let mut result = if result_has_strings {
        PhpArray::with_deferred_hash_capacity(result_len)
    } else {
        PhpArray::with_packed_capacity(result_len)
    };
    let mut removed = if removed_has_strings {
        PhpArray::with_deferred_hash_capacity(removed_len)
    } else {
        PhpArray::with_packed_capacity(removed_len)
    };
    let replacement_len = replacement.len();
    let mut replacement = Some(replacement);
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index == start {
            for replacement_value in replacement.take().unwrap_or_default() {
                result.push(replacement_value);
            }
        }
        let destination = if index >= start && index < end {
            &mut removed
        } else {
            &mut result
        };
        match key {
            ArrayKey::Int(_) => destination.push(value),
            ArrayKey::String(key) => destination.set_str(&key, value),
        }
    }
    if start == source_len {
        for replacement_value in replacement.take().unwrap_or_default() {
            result.push(replacement_value);
        }
    }

    *target = Value::array(result);
    crate::vm::execute::adjust_live_foreach_reference_positions_for_splice(
        ed,
        0,
        start,
        removed_len,
        replacement_len,
    );
    let removed = Value::array(removed);
    if !rv.is_null() {
        write_return_value(rv, removed);
        return Ok(());
    }

    // Keep a COW identity snapshot while discarded removed values run their
    // destructors. Any reentrant write to the by-reference input detaches or
    // replaces the target and is therefore detected without a hot-path array
    // generation counter.
    let mutation_snapshot = target.clone();
    let expected_identity = mutation_snapshot.array_identity();
    // SAFETY: the internal activation and synchronous caller stay live while
    // discarded return-value destructors execute.
    let caller = unsafe { (*ed).prev_execute_data };
    crate::vm::execute::run_value_destructors(eg, &[removed], caller)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if target.array_identity() != expected_identity {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Array was modified during array_splice operation",
        ));
    }
    Ok(())
}

fn fn_array_rand(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(eg, "array_rand", source.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let num = if arg_opt!(ed, 1).is_some() {
        let Some(num) = typed_internal_int_argument(ed, eg, "array_rand", 1, "num")? else {
            return Ok(());
        };
        num
    } else {
        1
    };
    if array.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_rand(): Argument #1 ($array) must not be empty",
        ));
        return Ok(());
    }
    if num < 1
        || usize::try_from(num)
            .ok()
            .is_none_or(|num| num > array.len())
    {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)",
        ));
        return Ok(());
    }

    let num = num as usize;
    if num == 1 {
        let (_, key) = array
            .get_at(shuffle_index(eg, array.len()))
            .expect("non-empty array random position must exist");
        ret!(rv, array_key_into_value(key));
    }

    // Sequential sampling selects every k-subset uniformly without a second
    // key buffer or a post-selection sort. Selected keys therefore retain the
    // insertion order required by PHP.
    let mut result = PhpArray::with_packed_capacity(num);
    let mut needed = num;
    let mut remaining = array.len();
    for (key, _) in array.iter() {
        if needed == remaining || shuffle_index(eg, remaining) < needed {
            result.push(array_key_into_value(key));
            needed -= 1;
        }
        remaining -= 1;
        if needed == 0 {
            break;
        }
    }
    ret!(rv, Value::array(result));
}

fn initial_shuffle_random_state() -> u64 {
    let mut bytes = [0u8; 8];
    let system_seed = std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .ok()
        .map(|()| u64::from_ne_bytes(bytes));
    let fallback = || {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        nanos ^ (u64::from(std::process::id()) << 32)
    };
    let seed = system_seed.unwrap_or_else(fallback);
    if seed == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        seed
    }
}

#[inline]
fn next_shuffle_random(eg: &mut ExecutorGlobals) -> u64 {
    let state = &mut eg
        .string_utility_state
        .get_or_insert_with(|| Box::new(crate::runtime::StringUtilityState::default()))
        .shuffle_random;
    if *state == 0 {
        *state = initial_shuffle_random_state();
    }
    // xorshift64* retains compact request-local state and a 2^64-1 period.
    // Range reduction below rejects the modulo-biased numerical prefix.
    let mut value = *state;
    value ^= value >> 12;
    value ^= value << 25;
    value ^= value >> 27;
    *state = value;
    value.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

#[inline]
fn shuffle_index(eg: &mut ExecutorGlobals, upper_exclusive: usize) -> usize {
    debug_assert!(upper_exclusive > 0);
    let upper = upper_exclusive as u64;
    let rejection_threshold = upper.wrapping_neg() % upper;
    loop {
        let sample = next_shuffle_random(eg);
        if sample >= rejection_threshold {
            return (sample % upper) as usize;
        }
    }
}

fn shuffle_slice<T>(eg: &mut ExecutorGlobals, values: &mut [T]) {
    for index in (1..values.len()).rev() {
        let replacement = shuffle_index(eg, index + 1);
        values.swap(index, replacement);
    }
}

fn fn_shuffle(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        shuffle_slice(eg, &mut entries);
        let mut new = PhpArray::new();
        for v in entries {
            new.push(v);
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        typed_internal_argument_error(eg, "shuffle", arr, 1, "array", "array");
        Ok(())
    }
}

/// array_map($callback, $array, ...$arrays) — map aligned array rows.
fn fn_array_map(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let resolved = if callback.value_type() == ValueType::Null {
        None
    } else {
        match resolve_callback_at_callsite_checked(callback, eg, ed)? {
            Some(resolved) => Some(resolved),
            None => {
                if eg.exception.is_some() {
                    return Ok(());
                }
                let reason = ordinary_callback_invalid_reason(callback, eg);
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "array_map(): Argument #1 ($callback) must be a valid callback or null, {reason}"
                    ),
                ));
                return Ok(());
            }
        }
    };
    let first = arg!(ed, 1);
    let Some(first_array) = first.as_array() else {
        typed_internal_argument_error(eg, "array_map", first, 2, "array", "array");
        return Ok(());
    };
    let mut arrays = vec![first_array];
    if let Some(extra) = arg_opt!(ed, 2).and_then(Value::as_array) {
        for (index, value) in extra.values().enumerate() {
            let Some(array) = value.dereferenced().as_array() else {
                typed_internal_argument_error(
                    eg,
                    "array_map",
                    value.dereferenced(),
                    index + 3,
                    "",
                    "array",
                );
                return Ok(());
            };
            arrays.push(array);
        }
    }
    let length = arrays.iter().map(|array| array.len()).max().unwrap_or(0);
    if resolved.is_none() && arrays.len() == 1 {
        ret!(rv, first.clone());
    }
    let mut result = if arrays.len() == 1 && first_array.is_packed() {
        PhpArray::with_packed_capacity(length)
    } else if arrays.len() == 1 {
        PhpArray::with_deferred_hash_capacity(length)
    } else {
        PhpArray::with_packed_capacity(length)
    };
    if let Some(resolved) = resolved.as_ref() {
        let publish_live_trace_caller = resolved.requires_live_internal_trace_caller();
        let mapped = with_internal_trace_origin(ed, eg, |eg| {
            if arrays.len() == 1 {
                for (key, value) in first_array.iter() {
                    let argument = value.dereferenced().clone();
                    let mapped = call_resolved_with_values_from_internal(
                        ed,
                        eg,
                        resolved,
                        std::slice::from_ref(&argument),
                        publish_live_trace_caller,
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                    result.set(key, mapped);
                }
                return Ok(Some(result));
            }

            for position in 0..length {
                let row = arrays
                    .iter()
                    .map(|array| {
                        array
                            .get_value_at(position)
                            .map(|value| value.dereferenced().clone())
                            .unwrap_or_else(Value::null)
                    })
                    .collect::<Vec<_>>();
                let mapped = call_resolved_with_values_from_internal(
                    ed,
                    eg,
                    resolved,
                    &row,
                    publish_live_trace_caller,
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
                result.push(mapped);
            }
            Ok(Some(result))
        })?;
        if let Some(mapped) = mapped {
            ret!(rv, Value::array(mapped));
        }
        return Ok(());
    }

    for position in 0..length {
        let mut tuple = PhpArray::with_packed_capacity(arrays.len());
        for array in &arrays {
            let value = array
                .get_value_at(position)
                .map(array_projection_value)
                .unwrap_or_else(Value::null);
            tuple.push(value);
        }
        result.push(Value::array(tuple));
    }
    ret!(rv, Value::array(result));
}

/// array_filter($array, $callback = null, $mode = 0): array
fn fn_array_filter(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(
            eg,
            "array_filter",
            source.dereferenced(),
            1,
            "array",
            "array",
        );
        return Ok(());
    };
    let callback = arg_opt!(ed, 1)
        .filter(|callback| callback.dereferenced().value_type() != ValueType::Null)
        .cloned();
    let resolved = if let Some(callback) = callback.as_ref() {
        match resolve_callback_at_callsite_checked(callback, eg, ed)? {
            Some(resolved) => Some(resolved),
            None => {
                if eg.exception.is_none() {
                    let reason = ordinary_callback_invalid_reason(callback, eg);
                    eg.exception = Some(crate::value::make_error_value(
                        "TypeError",
                        &format!(
                            "array_filter(): Argument #2 ($callback) must be a valid callback or null, {reason}"
                        ),
                    ));
                }
                return Ok(());
            }
        }
    } else {
        None
    };
    let mode = if arg_opt!(ed, 2).is_some() {
        let Some(mode) = typed_internal_int_argument(ed, eg, "array_filter", 2, "mode")? else {
            return Ok(());
        };
        mode
    } else {
        0
    };

    let mut result = PhpArray::new();
    if let Some(resolved) = resolved.as_ref() {
        match mode {
            1 => {
                for (key, value) in array.iter() {
                    let arguments = [
                        value.dereferenced().clone(),
                        array_key_into_value(key.clone()),
                    ];
                    let accepted = call_resolved_with_values(eg, resolved, &arguments)?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                    if accepted.is_truthy() {
                        result.set(key, array_projection_value(value));
                    }
                }
            }
            2 => {
                for (key, value) in array.iter() {
                    let argument = array_key_into_value(key.clone());
                    let accepted =
                        call_resolved_with_values(eg, resolved, std::slice::from_ref(&argument))?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                    if accepted.is_truthy() {
                        result.set(key, array_projection_value(value));
                    }
                }
            }
            _ => {
                for (key, value) in array.iter() {
                    let accepted = call_resolved_with_values(
                        eg,
                        resolved,
                        std::slice::from_ref(value.dereferenced()),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                    if accepted.is_truthy() {
                        result.set(key, array_projection_value(value));
                    }
                }
            }
        }
    } else {
        // PHP ignores the mode for a null callback, but still validates its
        // declared int boundary above before applying ordinary truthiness.
        for (key, value) in array.iter() {
            if value.dereferenced().is_truthy() {
                result.set(key, array_projection_value(value));
            }
        }
    }
    ret!(rv, Value::array(result));
}

struct CompactArrayFrame {
    _owner: Value,
    identity: usize,
    values: Vec<Value>,
    next: usize,
}

fn compact_array_frame(value: &Value) -> CompactArrayFrame {
    let owner = value.dereferenced().clone();
    let identity = owner
        .array_identity()
        .expect("compact array frame requires an array");
    let values = owner
        .as_array()
        .expect("compact array frame owner remains an array")
        .values()
        .cloned()
        .collect();
    CompactArrayFrame {
        _owner: owner,
        identity,
        values,
        next: 0,
    }
}

fn compact_invalid_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    position: usize,
    value: &Value,
) -> Result<(), VmError> {
    let actual = match value.dereferenced().value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => value.dereferenced().diagnostic_type_name().into_owned(),
    };
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "compact(): Argument #{position} must be string or array of strings, {actual} given"
        ),
    )?;
    Ok(())
}

fn compact_scope_name(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    result: &mut PhpArray,
    name: &str,
) -> Result<(), VmError> {
    let value = crate::vm::execute::caller_scope_variable(eg, ed, name).or_else(|| {
        (name == "this")
            .then(|| crate::vm::execute::receiver_for_internal_call(ed))
            .flatten()
    });
    if let Some(value) = value {
        result.set_str(name, value.dereferenced().clone());
    } else if name != "this" {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("compact(): Undefined variable ${name}"),
        )?;
    }
    Ok(())
}

fn compact_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    result: &mut PhpArray,
    argument: &Value,
    position: usize,
) -> Result<(), VmError> {
    let argument = argument.dereferenced();
    if let Some(name) = argument.as_str() {
        return compact_scope_name(ed, eg, result, name);
    }
    if argument.as_array().is_none() {
        return compact_invalid_argument(ed, eg, position, argument);
    }

    let mut frames = vec![compact_array_frame(argument)];
    let mut active = std::collections::HashSet::new();
    active.insert(frames[0].identity);
    while let Some(frame) = frames.last_mut() {
        if frame.next == frame.values.len() {
            active.remove(&frame.identity);
            frames.pop();
            continue;
        }
        let value = frame.values[frame.next].clone();
        frame.next += 1;
        let value = value.dereferenced();
        if let Some(name) = value.as_str() {
            compact_scope_name(ed, eg, result, name)?;
        } else if value.as_array().is_some() {
            let child = compact_array_frame(value);
            if !active.insert(child.identity) {
                eg.exception = Some(crate::value::make_error_value(
                    "Error",
                    "Recursion detected",
                ));
                return Ok(());
            }
            frames.push(child);
        } else {
            compact_invalid_argument(ed, eg, position, value)?;
        }
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    Ok(())
}

/// compact($var_name, ...$var_names): array
fn fn_compact(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = owned_argument(ed, 0);
    let extra = owned_argument(ed, 1);
    let extra = extra
        .as_array()
        .expect("compact variadic arguments are packed into an array");
    if extra.has_string_keys() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "compact() does not accept unknown named parameters",
        ));
        return Ok(());
    }

    let mut result = PhpArray::new();
    compact_argument(ed, eg, &mut result, &first, 1)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    for (index, argument) in extra.values().enumerate() {
        compact_argument(ed, eg, &mut result, argument, index + 2)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::array(result));
}

// ============================================================================
// String functions
// ============================================================================

#[inline(always)]
pub(crate) fn direct_strlen_len(argument: &Value, precision: i32) -> i64 {
    let argument = if argument.is_reference() {
        unsafe { &*argument.as_ref_ptr() }
    } else {
        argument
    };
    match argument.php_string_len() {
        Some(length) => length as i64,
        None => argument.echo_to_string_with_precision(precision).len() as i64,
    }
}

#[inline(always)]
fn direct_strlen(args: &[Value], precision: i32) -> Result<Value, VmError> {
    Ok(Value::long(direct_strlen_len(&args[0], precision)))
}

fn fn_strlen(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if reject_strict_internal_string(eg, ed, arg!(ed, 0), "strlen", "string") {
        return Ok(());
    }
    if arg!(ed, 0).dereferenced().value_type() == ValueType::Null {
        report_internal_deprecation(
            eg,
            ed,
            "strlen(): Passing null to parameter #1 ($string) of type string is deprecated",
        )?;
    }
    let result = direct_strlen(std::slice::from_ref(arg!(ed, 0)), eg.precision)?;
    ret!(rv, result);
}

#[inline(always)]
fn compare_php_strings(left: &[u8], right: &[u8], length: usize, fold_ascii_case: bool) -> i64 {
    let compared_length = left.len().min(right.len()).min(length);
    for index in 0..compared_length {
        let mut left_byte = left[index];
        let mut right_byte = right[index];
        if fold_ascii_case {
            left_byte = left_byte.to_ascii_lowercase();
            right_byte = right_byte.to_ascii_lowercase();
        }
        if left_byte != right_byte {
            return i64::from(left_byte) - i64::from(right_byte);
        }
    }

    if compared_length == length {
        return 0;
    }
    match left.len().cmp(&right.len()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod scalar_string_boundary_tests {
    use super::{compare_php_strings, direct_strlen_len};
    use crate::value::Value;

    #[test]
    fn byte_comparison_matches_unsigned_php_difference_for_every_pair() {
        for left in 0_u8..=u8::MAX {
            for right in 0_u8..=u8::MAX {
                assert_eq!(
                    compare_php_strings(&[left], &[right], usize::MAX, false),
                    i64::from(left) - i64::from(right)
                );
                assert_eq!(
                    compare_php_strings(&[left], &[right], usize::MAX, true),
                    i64::from(left.to_ascii_lowercase()) - i64::from(right.to_ascii_lowercase())
                );
            }
        }
        assert_eq!(compare_php_strings(b"a", b"z", 0, false), 0);
        assert_eq!(compare_php_strings(b"a", b"aa", usize::MAX, false), -1);
        assert_eq!(compare_php_strings(b"aa", b"a", usize::MAX, false), 1);
    }

    #[test]
    fn direct_strlen_uses_request_precision_and_php_byte_length() {
        assert_eq!(direct_strlen_len(&Value::double(1.23456789012345), 12), 13);
        assert_eq!(direct_strlen_len(&Value::double(1.23456789012345), 3), 4);
        assert_eq!(
            direct_strlen_len(&Value::binary_string(&[0, 128, 255]), 14),
            3
        );
        assert_eq!(direct_strlen_len(&Value::string("Ž"), 14), 2);
    }
}

#[inline(always)]
fn compare_php_string_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function_name: &str,
    bounded: bool,
    fold_ascii_case: bool,
) -> Result<Option<i64>, VmError> {
    {
        let left = arg!(ed, 0).dereferenced();
        let right = arg!(ed, 1).dereferenced();
        if left.value_type() == ValueType::String && right.value_type() == ValueType::String {
            let left = left.php_string_bytes().unwrap_or_default();
            let right = right.php_string_bytes().unwrap_or_default();
            let Some(length) = compare_php_string_length(ed, eg, function_name, bounded)? else {
                return Ok(None);
            };
            return Ok(Some(compare_php_strings(
                &left,
                &right,
                length,
                fold_ascii_case,
            )));
        }
    }

    let Some(left) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        function_name,
        0,
        "string1",
        "string",
    )?
    else {
        return Ok(None);
    };
    let Some(right) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        function_name,
        1,
        "string2",
        "string",
    )?
    else {
        return Ok(None);
    };
    let left = left.php_string_bytes().unwrap_or_default();
    let right = right.php_string_bytes().unwrap_or_default();
    let Some(length) = compare_php_string_length(ed, eg, function_name, bounded)? else {
        return Ok(None);
    };
    Ok(Some(compare_php_strings(
        &left,
        &right,
        length,
        fold_ascii_case,
    )))
}

#[inline(always)]
fn compare_php_string_length(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function_name: &str,
    bounded: bool,
) -> Result<Option<usize>, VmError> {
    if !bounded {
        return Ok(Some(usize::MAX));
    }
    let Some(length) = typed_internal_int_argument(ed, eg, function_name, 2, "length")? else {
        return Ok(None);
    };
    if length < 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!("{function_name}(): Argument #3 ($length) must be greater than or equal to 0"),
        ));
        return Ok(None);
    }
    Ok(Some(usize::try_from(length).unwrap_or(usize::MAX)))
}

fn fn_strcmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(result) = compare_php_string_arguments(ed, eg, "strcmp", false, false)? else {
        return Ok(());
    };
    ret!(rv, Value::long(result));
}

fn fn_strcasecmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(result) = compare_php_string_arguments(ed, eg, "strcasecmp", false, true)? else {
        return Ok(());
    };
    ret!(rv, Value::long(result));
}

fn compare_php_strings_with_length(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function_name: &str,
    fold_ascii_case: bool,
) -> Result<(), VmError> {
    let Some(result) = compare_php_string_arguments(ed, eg, function_name, true, fold_ascii_case)?
    else {
        return Ok(());
    };
    ret!(rv, Value::long(result));
}

fn fn_strncmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    compare_php_strings_with_length(ed, rv, eg, "strncmp", false)
}

fn fn_strncasecmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    compare_php_strings_with_length(ed, rv, eg, "strncasecmp", true)
}

fn md5_digest(input: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const CONSTANTS: [u32; 64] = [
        0xd76a_a478,
        0xe8c7_b756,
        0x2420_70db,
        0xc1bd_ceee,
        0xf57c_0faf,
        0x4787_c62a,
        0xa830_4613,
        0xfd46_9501,
        0x6980_98d8,
        0x8b44_f7af,
        0xffff_5bb1,
        0x895c_d7be,
        0x6b90_1122,
        0xfd98_7193,
        0xa679_438e,
        0x49b4_0821,
        0xf61e_2562,
        0xc040_b340,
        0x265e_5a51,
        0xe9b6_c7aa,
        0xd62f_105d,
        0x0244_1453,
        0xd8a1_e681,
        0xe7d3_fbc8,
        0x21e1_cde6,
        0xc337_07d6,
        0xf4d5_0d87,
        0x455a_14ed,
        0xa9e3_e905,
        0xfcef_a3f8,
        0x676f_02d9,
        0x8d2a_4c8a,
        0xfffa_3942,
        0x8771_f681,
        0x6d9d_6122,
        0xfde5_380c,
        0xa4be_ea44,
        0x4bde_cfa9,
        0xf6bb_4b60,
        0xbebf_bc70,
        0x289b_7ec6,
        0xeaa1_27fa,
        0xd4ef_3085,
        0x0488_1d05,
        0xd9d4_d039,
        0xe6db_99e5,
        0x1fa2_7cf8,
        0xc4ac_5665,
        0xf429_2244,
        0x432a_ff97,
        0xab94_23a7,
        0xfc93_a039,
        0x655b_59c3,
        0x8f0c_cc92,
        0xffef_f47d,
        0x8584_5dd1,
        0x6fa8_7e4f,
        0xfe2c_e6e0,
        0xa301_4314,
        0x4e08_11a1,
        0xf753_7e82,
        0xbd3a_f235,
        0x2ad7_d2bb,
        0xeb86_d391,
    ];

    let bit_length = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(input.len().saturating_add(72));
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_le_bytes());

    let mut state = [0x6745_2301_u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 16];
        for (word, bytes) in words.iter_mut().zip(chunk.chunks_exact(4)) {
            *word = u32::from_le_bytes(bytes.try_into().unwrap());
        }

        let [mut a, mut b, mut c, mut d] = state;
        for round in 0..64 {
            let (function, word) = match round {
                0..=15 => ((b & c) | (!b & d), round),
                16..=31 => ((d & b) | (!d & c), (5 * round + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * round + 5) % 16),
                _ => (c ^ (b | !d), (7 * round) % 16),
            };
            let previous_d = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(function)
                    .wrapping_add(CONSTANTS[round])
                    .wrapping_add(words[word])
                    .rotate_left(SHIFTS[round]),
            );
            a = previous_d;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    let mut output = [0_u8; 16];
    for (bytes, word) in output.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    output
}

fn format_hex_digest(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(output, "{byte:02x}").unwrap();
    }
    output
}

/// PHP's `crc32()` uses the reflected IEEE CRC-32 recurrence and exposes the
/// resulting unsigned 32-bit word as a positive integer on AMD64.
fn php_crc32_ieee(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let polynomial = if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
            crc = (crc >> 1) ^ polynomial;
        }
    }
    !crc
}

fn sha1_digest(input: &[u8]) -> [u8; 20] {
    let bit_length = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(input.len().saturating_add(72));
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut state = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (word, bytes) in words.iter_mut().take(16).zip(chunk.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().unwrap());
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (round, word) in words.into_iter().enumerate() {
            let (function, constant) = match round {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }

    let mut output = [0_u8; 20];
    for (bytes, word) in output.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    output
}

#[cfg(test)]
mod checksum_digest_tests {
    use super::{format_hex_digest, php_crc32_ieee, sha1_digest};

    #[test]
    fn crc32_matches_php_for_empty_binary_and_block_edge_inputs() {
        assert_eq!(php_crc32_ieee(b""), 0);
        assert_eq!(php_crc32_ieee(b"checksum lane"), 1_074_860_217);
        assert_eq!(php_crc32_ieee(b"a\0\xffz"), 2_167_024_170);
        assert_eq!(php_crc32_ieee(&vec![b'q'; 65]), 425_929_956);
    }

    #[test]
    fn sha1_matches_php_for_binary_and_multi_block_inputs() {
        assert_eq!(
            format_hex_digest(&sha1_digest(b"checksum lane")),
            "184b813dadd5b407c44692826da5be14cefde813"
        );
        assert_eq!(
            format_hex_digest(&sha1_digest(b"a\0\xffz")),
            "6b419a441881c5640e2654f6f0e553c37da893e0"
        );
        assert_eq!(
            format_hex_digest(&sha1_digest(&vec![b'q'; 65])),
            "b0931a65ae5cf3e027199de5f7c56eb0f073c552"
        );
    }
}

#[cfg(test)]
mod md5_tests {
    use super::{format_hex_digest, md5_digest};

    #[test]
    fn matches_rfc_1321_vectors() {
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(format_hex_digest(&md5_digest(input.as_bytes())), expected);
        }
    }

    #[test]
    fn handles_embedded_nul_and_multi_block_input() {
        assert_eq!(
            format_hex_digest(&md5_digest(b"a\0b")),
            "70350f6027bce3713f6b76473084309b"
        );
        assert_eq!(
            format_hex_digest(&md5_digest(&vec![b'a'; 1_000_000])),
            "7707d6ae4e027c70eea2a935c2296f21"
        );
    }
}

fn fn_md5(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_argument(ed, eg, "md5", 0, "string")? else {
        return Ok(());
    };
    let binary = if arg_opt!(ed, 1).is_some() {
        let Some(binary) = typed_internal_bool_argument(ed, eg, "md5", 1, "binary")? else {
            return Ok(());
        };
        binary
    } else {
        false
    };
    let digest = md5_digest(&php_string_to_bytes(&input));
    if binary {
        ret!(rv, Value::string(bytes_to_php_string(&digest)));
    }
    ret!(rv, Value::string(format_hex_digest(&digest)));
}

fn fn_crc32(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "crc32", 0, "string", "string")?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    ret!(rv, Value::long(i64::from(php_crc32_ieee(&bytes))));
}

fn fn_sha1(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "sha1", 0, "string", "string")?
    else {
        return Ok(());
    };
    let binary = if arg_opt!(ed, 1).is_some() {
        let Some(binary) = typed_internal_bool_argument(ed, eg, "sha1", 1, "binary")? else {
            return Ok(());
        };
        binary
    } else {
        false
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    let digest = sha1_digest(&bytes);
    if binary {
        ret!(rv, php_byte_result(digest.to_vec(), true));
    }
    ret!(rv, Value::string(format_hex_digest(&digest)));
}

fn digest_file_error_reason(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        std::io::ErrorKind::IsADirectory => "Is a directory".to_string(),
        _ if error.raw_os_error() == Some(36) => "File name too long".to_string(),
        _ => error
            .to_string()
            .split_once(" (os error ")
            .map_or_else(|| error.to_string(), |(message, _)| message.to_string()),
    }
}

fn file_digest_builtin<const N: usize>(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    digest: fn(&[u8]) -> [u8; N],
) -> Result<(), VmError> {
    let Some(filename) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "filename", "string")?
    else {
        return Ok(());
    };
    let binary = if arg_opt!(ed, 1).is_some() {
        let Some(binary) = typed_internal_bool_argument(ed, eg, function, 1, "binary")? else {
            return Ok(());
        };
        binary
    } else {
        false
    };
    let filename_bytes = filename.php_string_bytes().unwrap_or_default();
    if filename_bytes.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "Path must not be empty",
        ));
        return Ok(());
    }
    if filename_bytes.contains(&b'\0') {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!("{function}(): Argument #1 ($filename) must not contain any null bytes"),
        ));
        return Ok(());
    }
    let filename = filename.as_str().unwrap_or_default();
    let bytes = if filename == "php://memory" || filename == "php://temp" {
        Vec::new()
    } else {
        let path = filename.strip_prefix("file://").unwrap_or(filename);
        match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    &format!(
                        "{function}({filename}): Failed to open stream: {}",
                        digest_file_error_reason(&error)
                    ),
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
                ret!(rv, Value::bool(false));
            }
        }
    };
    let digest = digest(&bytes);
    if binary {
        ret!(rv, php_byte_result(digest.to_vec(), true));
    }
    ret!(rv, Value::string(format_hex_digest(&digest)));
}

fn fn_md5_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_digest_builtin(ed, rv, eg, "md5_file", md5_digest)
}

fn fn_sha1_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    file_digest_builtin(ed, rv, eg, "sha1_file", sha1_digest)
}

fn fn_hash(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let algorithm = arg_str!(ed, 0);
    let data = arg_str!(ed, 1);
    let binary = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    if algorithm.eq_ignore_ascii_case("md5") {
        let digest = md5_digest(&php_string_to_bytes(&data));
        if binary {
            ret!(rv, Value::string(bytes_to_php_string(&digest)));
        }
        ret!(rv, Value::string(format_hex_digest(&digest)));
    }
    if algorithm.eq_ignore_ascii_case("xxh128") {
        let digest = xxhash_rust::xxh3::xxh3_128(data.as_bytes());
        if binary {
            ret!(
                rv,
                Value::string(bytes_to_php_string(&digest.to_be_bytes()))
            );
        }
        ret!(rv, Value::string(format!("{digest:032x}")));
    }
    if algorithm.eq_ignore_ascii_case("crc32") {
        let digest = php_crc32(data.as_bytes()).to_le_bytes();
        if binary {
            ret!(rv, Value::string(bytes_to_php_string(&digest)));
        }
        let mut output = String::with_capacity(8);
        for byte in digest {
            write!(output, "{byte:02x}").unwrap();
        }
        ret!(rv, Value::string(output));
    }
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        "hash(): Argument #1 ($algo) must be a valid hashing algorithm",
    ));
    ret!(rv, Value::null());
}

fn hash_context_buffer(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if !object.class_name.eq_ignore_ascii_case("HashContext") {
        return None;
    }
    object
        .get_property("__rphp_hash_buffer")?
        .as_str()
        .map(str::to_owned)
}

fn fn_hash_init(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let algorithm = arg_str!(ed, 0);
    if !algorithm.eq_ignore_ascii_case("xxh128") {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "hash_init(): Argument #1 ($algo) must be a valid hashing algorithm",
        ));
        return Ok(());
    }
    let mut properties = std::collections::HashMap::new();
    properties.insert("__rphp_hash_algorithm".to_string(), Value::string("xxh128"));
    properties.insert("__rphp_hash_buffer".to_string(), Value::string(""));
    ret!(
        rv,
        Value::object(crate::value::PhpObject::dynamic(
            "HashContext".to_string(),
            0,
            properties,
        ))
    );
}

fn fn_hash_update(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let data = arg_str!(ed, 1).into_owned();
    let context = arg!(ed, 0);
    let Some(mut object) = context.as_object_mut() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "hash_update(): Argument #1 ($context) must be a valid, non-finalized HashContext",
        ));
        return Ok(());
    };
    let Some(buffer) = object
        .get_property("__rphp_hash_buffer")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "hash_update(): Argument #1 ($context) must be a valid, non-finalized HashContext",
        ));
        return Ok(());
    };
    object.set_property("__rphp_hash_buffer", Value::string(buffer + &data));
    ret!(rv, Value::bool(true));
}

fn fn_hash_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let context = arg!(ed, 0);
    let Some(buffer) = hash_context_buffer(context) else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "hash_final(): Argument #1 ($context) must be a valid, non-finalized HashContext",
        ));
        return Ok(());
    };
    if let Some(mut object) = context.as_object_mut() {
        object.unset_property("__rphp_hash_buffer");
    }
    let digest = xxhash_rust::xxh3::xxh3_128(buffer.as_bytes());
    if arg_opt!(ed, 1).is_some_and(Value::is_truthy) {
        ret!(
            rv,
            Value::string(bytes_to_php_string(&digest.to_be_bytes()))
        );
    }
    ret!(rv, Value::string(format!("{digest:032x}")));
}

/// PHP's `hash('crc32', ...)` uses the non-reflected CRC-32/BZIP2 recurrence
/// and renders the resulting word in little-endian byte order. This is
/// intentionally distinct from PHP's `crc32b`/`crc32()` algorithm.
fn php_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    !crc
}

#[inline]
fn substr_bounds(byte_len: usize, offset: i64, length: Option<i64>) -> (usize, usize) {
    let start = if offset < 0 {
        usize::try_from(offset.unsigned_abs())
            .ok()
            .and_then(|distance| byte_len.checked_sub(distance))
            .unwrap_or(0)
    } else {
        usize::try_from(offset)
            .ok()
            .unwrap_or(usize::MAX)
            .min(byte_len)
    };
    let end = match length {
        None => byte_len,
        Some(length) if length >= 0 => usize::try_from(length)
            .ok()
            .and_then(|length| start.checked_add(length))
            .unwrap_or(usize::MAX)
            .min(byte_len),
        Some(length) => usize::try_from(length.unsigned_abs())
            .ok()
            .and_then(|distance| byte_len.checked_sub(distance))
            .unwrap_or(0)
            .max(start),
    };
    (start, end)
}

fn fn_substr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0);
    let exact_offset = arg!(ed, 1);
    let exact_length = match arg_opt!(ed, 2) {
        None => Some(None),
        Some(value) if value.value_type() == ValueType::Null => Some(None),
        Some(value) if value.value_type() == ValueType::Long => value.as_long().map(Some),
        Some(_) => None,
    };
    if exact_string.value_type() == ValueType::String
        && exact_offset.value_type() == ValueType::Long
        && let Some(offset) = exact_offset.as_long()
        && let Some(length) = exact_length
    {
        let binary = exact_string.is_binary_string();
        let bytes = exact_string.php_string_bytes().unwrap_or_default();
        let (start, end) = substr_bounds(bytes.len(), offset, length);
        ret!(rv, php_byte_result(bytes[start..end].to_vec(), binary));
    }

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "substr", 0, "string", "string")?
    else {
        return Ok(());
    };
    let Some(offset) = typed_internal_int_argument(ed, eg, "substr", 1, "offset")? else {
        return Ok(());
    };
    let length = match arg_opt!(ed, 2) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(length) =
                typed_internal_int_argument_expected(ed, eg, "substr", 2, "length", "?int")?
            else {
                return Ok(());
            };
            Some(length)
        }
    };

    let binary = string.is_binary_string();
    let bytes = string.php_string_bytes().unwrap_or_default();
    let (start, end) = substr_bounds(bytes.len(), offset, length);
    ret!(rv, php_byte_result(bytes[start..end].to_vec(), binary));
}

fn natural_compare(left: &[u8], right: &[u8], case_insensitive: bool) -> std::cmp::Ordering {
    fn compare_integer(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        let left = left
            .iter()
            .position(|byte| *byte != b'0')
            .map_or(&left[left.len()..], |start| &left[start..]);
        let right = right
            .iter()
            .position(|byte| *byte != b'0')
            .map_or(&right[right.len()..], |start| &right[start..]);
        left.len().cmp(&right.len()).then_with(|| left.cmp(right))
    }

    let mut left_index = 0;
    let mut right_index = 0;
    while left.get(left_index) == Some(&b'0')
        && left.get(left_index + 1).is_some_and(u8::is_ascii_digit)
    {
        left_index += 1;
    }
    while right.get(right_index) == Some(&b'0')
        && right.get(right_index + 1).is_some_and(u8::is_ascii_digit)
    {
        right_index += 1;
    }
    while left_index < left.len() || right_index < right.len() {
        match (left.get(left_index), right.get(right_index)) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(_), Some(_)) => {}
        }
        while left.get(left_index).is_some_and(u8::is_ascii_whitespace) {
            left_index += 1;
        }
        while right.get(right_index).is_some_and(u8::is_ascii_whitespace) {
            right_index += 1;
        }

        let Some(&left_byte) = left.get(left_index) else {
            return if right_index == right.len() {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Less
            };
        };
        let Some(&right_byte) = right.get(right_index) else {
            return std::cmp::Ordering::Greater;
        };

        if left_byte.is_ascii_digit() && right_byte.is_ascii_digit() {
            let left_end = left[left_index..]
                .iter()
                .position(|byte| !byte.is_ascii_digit())
                .map_or(left.len(), |offset| left_index + offset);
            let right_end = right[right_index..]
                .iter()
                .position(|byte| !byte.is_ascii_digit())
                .map_or(right.len(), |offset| right_index + offset);
            let left_digits = &left[left_index..left_end];
            let right_digits = &right[right_index..right_end];
            let ordering = if left_byte == b'0' || right_byte == b'0' {
                left_digits.cmp(right_digits)
            } else {
                compare_integer(left_digits, right_digits)
            };
            if ordering != std::cmp::Ordering::Equal {
                return ordering;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let left_byte = if case_insensitive {
            left_byte.to_ascii_lowercase()
        } else {
            left_byte
        };
        let right_byte = if case_insensitive {
            right_byte.to_ascii_lowercase()
        } else {
            right_byte
        };
        let ordering = left_byte.cmp(&right_byte);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
        left_index += 1;
        right_index += 1;
    }
    std::cmp::Ordering::Equal
}

fn fn_strnatcmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ordering = if let (Some(left), Some(right)) = (arg!(ed, 0).as_str(), arg!(ed, 1).as_str()) {
        natural_compare(left.as_bytes(), right.as_bytes(), false)
    } else {
        let Some(left) = typed_internal_string_argument(ed, eg, "strnatcmp", 0, "string1")? else {
            return Ok(());
        };
        let Some(right) = typed_internal_string_argument(ed, eg, "strnatcmp", 1, "string2")? else {
            return Ok(());
        };
        natural_compare(left.as_bytes(), right.as_bytes(), false)
    };
    let result = match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    ret!(rv, Value::long(result));
}

fn fn_strnatcasecmp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ordering = if let (Some(left), Some(right)) = (arg!(ed, 0).as_str(), arg!(ed, 1).as_str()) {
        natural_compare(left.as_bytes(), right.as_bytes(), true)
    } else {
        let Some(left) = typed_internal_string_argument(ed, eg, "strnatcasecmp", 0, "string1")?
        else {
            return Ok(());
        };
        let Some(right) = typed_internal_string_argument(ed, eg, "strnatcasecmp", 1, "string2")?
        else {
            return Ok(());
        };
        natural_compare(left.as_bytes(), right.as_bytes(), true)
    };
    let result = match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    ret!(rv, Value::long(result));
}

fn fn_substr_compare(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(haystack) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "substr_compare",
        0,
        "haystack",
        "string",
    )?
    else {
        return Ok(());
    };
    let Some(needle) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "substr_compare",
        1,
        "needle",
        "string",
    )?
    else {
        return Ok(());
    };
    let Some(offset) = typed_internal_int_argument(ed, eg, "substr_compare", 2, "offset")? else {
        return Ok(());
    };
    let length = match arg_opt!(ed, 3) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(length) = typed_internal_int_argument_expected(
                ed,
                eg,
                "substr_compare",
                3,
                "length",
                "?int",
            )?
            else {
                return Ok(());
            };
            Some(length)
        }
    };
    let case_insensitive = if arg_opt!(ed, 4).is_some() {
        let Some(case_insensitive) =
            typed_internal_bool_argument(ed, eg, "substr_compare", 4, "case_insensitive")?
        else {
            return Ok(());
        };
        case_insensitive
    } else {
        false
    };
    if length.is_some_and(|length| length < 0) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_compare(): Argument #4 ($length) must be greater than or equal to 0",
        ));
        return Ok(());
    }
    if length == Some(0) {
        ret!(rv, Value::long(0));
    }

    let haystack_bytes = haystack.php_string_bytes().unwrap_or_default();
    let needle_bytes = needle.php_string_bytes().unwrap_or_default();
    let start = if offset < 0 {
        Some(
            haystack_bytes
                .len()
                .saturating_sub(usize::try_from(offset.unsigned_abs()).unwrap_or(usize::MAX)),
        )
    } else {
        usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= haystack_bytes.len())
    };
    let Some(start) = start else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
        return Ok(());
    };

    let available = haystack_bytes.len() - start;
    let compared_length = length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(available)
        .min(available);
    let left = &haystack_bytes[start..start + compared_length];
    let right = &needle_bytes[..length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(needle_bytes.len())
        .min(needle_bytes.len())];
    let ordering = if case_insensitive {
        left.iter()
            .copied()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(right.iter().copied().map(|byte| byte.to_ascii_lowercase()))
    } else {
        left.cmp(right)
    };
    let result = match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    ret!(rv, Value::long(result));
}

fn fn_strpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let haystack = arg!(ed, 0);
    let needle = arg!(ed, 1);
    let exact_offset = match arg_opt!(ed, 2) {
        None => Some(0),
        Some(offset) if offset.value_type() == ValueType::Long => offset.as_long(),
        Some(_) => None,
    };
    if haystack.value_type() == ValueType::String
        && needle.value_type() == ValueType::String
        && !haystack.is_binary_string()
        && !needle.is_binary_string()
        && let Some(offset) = exact_offset
    {
        let haystack = haystack.as_str().unwrap_or_default();
        let needle = needle.as_str().unwrap_or_default();
        let boundary = if offset < 0 {
            usize::try_from(offset.unsigned_abs())
                .ok()
                .and_then(|distance| haystack.len().checked_sub(distance))
        } else {
            usize::try_from(offset)
                .ok()
                .filter(|offset| *offset <= haystack.len())
        };
        let Some(boundary) = boundary else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
            ));
            return Ok(());
        };
        if haystack.is_char_boundary(boundary) {
            let position = haystack[boundary..]
                .find(needle)
                .map(|position| boundary + position);
            ret!(
                rv,
                position.map_or_else(
                    || Value::bool(false),
                    |position| Value::long(position as i64),
                )
            );
        }
    }
    string_position_builtin(ed, rv, eg, "strpos", StringSearchDirection::First, false)
}

fn fn_strstr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_before_needle = match arg_opt!(ed, 2) {
        None => Some(false),
        Some(value) if matches!(value.value_type(), ValueType::True | ValueType::False) => {
            Some(value.is_truthy())
        }
        Some(_) => None,
    };
    let exact_haystack = arg!(ed, 0);
    let exact_needle = arg!(ed, 1);
    if exact_haystack.value_type() == ValueType::String
        && exact_needle.value_type() == ValueType::String
        && let Some(before_needle) = exact_before_needle
    {
        let binary = exact_haystack.is_binary_string();
        let haystack = exact_haystack.php_string_bytes().unwrap_or_default();
        let needle = exact_needle.php_string_bytes().unwrap_or_default();
        let Some(position) = string_search_position(&haystack, &needle, false, false) else {
            ret!(rv, Value::bool(false));
        };
        let bytes = if before_needle {
            &haystack[..position]
        } else {
            &haystack[position..]
        };
        ret!(rv, php_byte_result(bytes.to_vec(), binary));
    }

    let Some(haystack) =
        typed_internal_string_value_argument_expected(ed, eg, "strstr", 0, "haystack", "string")?
    else {
        return Ok(());
    };
    let Some(needle) =
        typed_internal_string_value_argument_expected(ed, eg, "strstr", 1, "needle", "string")?
    else {
        return Ok(());
    };
    let before_needle = if arg_opt!(ed, 2).is_some() {
        let Some(before_needle) =
            typed_internal_bool_argument(ed, eg, "strstr", 2, "before_needle")?
        else {
            return Ok(());
        };
        before_needle
    } else {
        false
    };
    let binary = haystack.is_binary_string();
    let haystack = haystack.php_string_bytes().unwrap_or_default();
    let needle = needle.php_string_bytes().unwrap_or_default();
    let position = string_search_position(&haystack, &needle, false, false);
    let Some(position) = position else {
        ret!(rv, Value::bool(false));
    };
    let bytes = if before_needle {
        &haystack[..position]
    } else {
        &haystack[position..]
    };
    ret!(rv, php_byte_result(bytes.to_vec(), binary));
}

fn typed_internal_argument_error(
    eg: &mut ExecutorGlobals,
    function: &str,
    argument: &Value,
    position: usize,
    parameter: &str,
    expected: &str,
) {
    let actual = match argument.value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => argument.diagnostic_type_name().into_owned(),
    };
    let parameter = if parameter.is_empty() {
        String::new()
    } else {
        format!(" (${parameter})")
    };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{position}{parameter} must be of type {expected}, {actual} given"
        ),
    ));
}

fn typed_internal_string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<String>, VmError> {
    typed_internal_string_argument_expected(ed, eg, function, index, parameter, "string")
}

fn typed_internal_string_argument_expected(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
) -> Result<Option<String>, VmError> {
    Ok(
        typed_internal_string_value_argument_expected(
            ed, eg, function, index, parameter, expected,
        )?
        .map(|value| value.as_str().unwrap_or("").to_string()),
    )
}

fn typed_internal_string_value_argument_expected(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
) -> Result<Option<Value>, VmError> {
    typed_internal_string_value_argument_with_null_expected(
        ed, eg, function, index, parameter, expected, expected,
    )
}

fn typed_internal_string_value_argument_with_null_expected(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
    null_expected: &str,
) -> Result<Option<Value>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::String => Some(argument.clone()),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #{} (${parameter}) of type {null_expected} is deprecated",
                    index + 1
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(Value::string(String::new()))
        }
        ValueType::False if !strict => Some(Value::string(String::new())),
        ValueType::True if !strict => Some(Value::string("1")),
        ValueType::Long | ValueType::Double if !strict => {
            if argument.as_double().is_some_and(f64::is_nan) {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to string",
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            Some(Value::string(
                argument.echo_to_string_with_precision(eg.precision),
            ))
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(rendered) = rendered else {
                typed_internal_argument_error(
                    eg,
                    function,
                    argument,
                    index as usize + 1,
                    parameter,
                    expected,
                );
                return Ok(None);
            };
            let rendered = rendered.dereferenced();
            match rendered.value_type() {
                ValueType::String => Some(rendered.clone()),
                ValueType::Long | ValueType::Double | ValueType::True | ValueType::False => {
                    if rendered.as_double().is_some_and(f64::is_nan) {
                        report_internal_diagnostic(
                            eg,
                            ed,
                            2,
                            "Warning",
                            "unexpected NAN value was coerced to string",
                        )?;
                        if eg.exception.is_some() {
                            return Ok(None);
                        }
                    }
                    Some(Value::string(
                        rendered.echo_to_string_with_precision(eg.precision),
                    ))
                }
                _ => {
                    let class_name = argument.diagnostic_type_name();
                    let actual = rendered.diagnostic_type_name();
                    eg.exception = Some(crate::value::make_error_value(
                        "TypeError",
                        &format!(
                            "{class_name}::__toString(): Return value must be of type string, {actual} returned"
                        ),
                    ));
                    return Ok(None);
                }
            }
        }
        _ => {
            typed_internal_argument_error(
                eg,
                function,
                argument,
                index as usize + 1,
                parameter,
                expected,
            );
            None
        }
    };
    Ok(converted)
}

fn typed_internal_bool_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<bool>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::True => Some(true),
        ValueType::False => Some(false),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #{} (${parameter}) of type bool is deprecated",
                    index + 1
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(false)
        }
        ValueType::Long | ValueType::Double | ValueType::String if !strict => {
            if argument.as_double().is_some_and(f64::is_nan) {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to bool",
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            Some(argument.is_truthy())
        }
        _ => {
            typed_internal_argument_error(
                eg,
                function,
                argument,
                index as usize + 1,
                parameter,
                "bool",
            );
            None
        }
    };
    Ok(converted)
}

fn ascii_case_insensitive_position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|candidate| {
        candidate
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

#[derive(Clone, Copy)]
enum StringSearchDirection {
    First,
    Last,
}

#[inline]
fn string_search_candidate_matches(candidate: &[u8], needle: &[u8], ascii_fold: bool) -> bool {
    if ascii_fold {
        candidate
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    } else {
        candidate == needle
    }
}

#[inline]
fn string_search_position(
    haystack: &[u8],
    needle: &[u8],
    ascii_fold: bool,
    reverse: bool,
) -> Option<usize> {
    if !ascii_fold {
        return if reverse {
            memchr::memmem::rfind(haystack, needle)
        } else {
            memchr::memmem::find(haystack, needle)
        };
    }

    let maximum_start = haystack.len().checked_sub(needle.len())?;
    let first = needle[0];
    let folded = first.to_ascii_lowercase();
    let other = first.to_ascii_uppercase();
    let find_first = |bytes: &[u8]| {
        if folded == other {
            memchr::memchr(first, bytes)
        } else {
            memchr::memchr2(folded, other, bytes)
        }
    };
    let find_last = |bytes: &[u8]| {
        if folded == other {
            memchr::memrchr(first, bytes)
        } else {
            memchr::memrchr2(folded, other, bytes)
        }
    };

    if !reverse {
        let mut start = 0;
        while start <= maximum_start {
            let position = start + find_first(&haystack[start..=maximum_start])?;
            if string_search_candidate_matches(
                &haystack[position..position + needle.len()],
                needle,
                true,
            ) {
                return Some(position);
            }
            start = position + 1;
        }
        return None;
    }

    let mut end = maximum_start + 1;
    while end > 0 {
        let position = find_last(&haystack[..end])?;
        if string_search_candidate_matches(
            &haystack[position..position + needle.len()],
            needle,
            true,
        ) {
            return Some(position);
        }
        end = position;
    }
    None
}

fn string_position_at_offset(
    haystack: &[u8],
    needle: &[u8],
    offset: i64,
    direction: StringSearchDirection,
    ascii_fold: bool,
) -> Option<Option<usize>> {
    let boundary = if offset < 0 {
        let distance = usize::try_from(offset.unsigned_abs()).ok()?;
        haystack.len().checked_sub(distance)?
    } else {
        usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= haystack.len())?
    };

    if needle.is_empty() {
        return Some(Some(match direction {
            StringSearchDirection::First => boundary,
            StringSearchDirection::Last if offset < 0 => boundary,
            StringSearchDirection::Last => haystack.len(),
        }));
    }

    let position = match direction {
        StringSearchDirection::First => {
            string_search_position(&haystack[boundary..], needle, ascii_fold, false)
                .map(|position| boundary + position)
        }
        StringSearchDirection::Last if offset < 0 => {
            let end = boundary.saturating_add(needle.len()).min(haystack.len());
            string_search_position(&haystack[..end], needle, ascii_fold, true)
                .filter(|position| *position <= boundary)
        }
        StringSearchDirection::Last => {
            string_search_position(&haystack[boundary..], needle, ascii_fold, true)
                .map(|position| boundary + position)
        }
    };
    Some(position)
}

fn string_position_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    direction: StringSearchDirection,
    ascii_fold: bool,
) -> Result<(), VmError> {
    let exact_offset = match arg_opt!(ed, 2) {
        None => Some(0),
        Some(offset) if offset.value_type() == ValueType::Long => offset.as_long(),
        Some(_) => None,
    };
    let exact_haystack = arg!(ed, 0);
    let exact_needle = arg!(ed, 1);
    if exact_haystack.value_type() == ValueType::String
        && exact_needle.value_type() == ValueType::String
        && let Some(offset) = exact_offset
    {
        if !ascii_fold
            && matches!(direction, StringSearchDirection::First)
            && !exact_haystack.is_binary_string()
            && !exact_needle.is_binary_string()
        {
            let haystack = exact_haystack.as_str().unwrap_or_default();
            let needle = exact_needle.as_str().unwrap_or_default();
            let boundary = if offset < 0 {
                usize::try_from(offset.unsigned_abs())
                    .ok()
                    .and_then(|distance| haystack.len().checked_sub(distance))
            } else {
                usize::try_from(offset)
                    .ok()
                    .filter(|offset| *offset <= haystack.len())
            };
            let Some(boundary) = boundary else {
                eg.exception = Some(crate::value::make_error_value(
                    "ValueError",
                    &format!(
                        "{function}(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
                    ),
                ));
                return Ok(());
            };
            if haystack.is_char_boundary(boundary) {
                let position = haystack[boundary..]
                    .find(needle)
                    .map(|position| boundary + position);
                ret!(
                    rv,
                    position.map_or_else(
                        || Value::bool(false),
                        |position| Value::long(position as i64),
                    )
                );
            }
        }

        let haystack = exact_haystack.php_string_bytes().unwrap_or_default();
        let needle = exact_needle.php_string_bytes().unwrap_or_default();
        let Some(position) = string_position_at_offset(
            haystack.as_ref(),
            needle.as_ref(),
            offset,
            direction,
            ascii_fold,
        ) else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                &format!(
                    "{function}(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
                ),
            ));
            return Ok(());
        };
        ret!(
            rv,
            position.map_or_else(
                || Value::bool(false),
                |position| Value::long(position as i64),
            )
        );
    }

    let Some(haystack) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "haystack", "string")?
    else {
        return Ok(());
    };
    let Some(needle) =
        typed_internal_string_value_argument_expected(ed, eg, function, 1, "needle", "string")?
    else {
        return Ok(());
    };
    let offset = if arg_opt!(ed, 2).is_some() {
        let Some(offset) = typed_internal_int_argument(ed, eg, function, 2, "offset")? else {
            return Ok(());
        };
        offset
    } else {
        0
    };
    if !ascii_fold && offset == 0 && !haystack.is_binary_string() && !needle.is_binary_string() {
        let haystack = haystack.as_str().unwrap_or("");
        let needle = needle.as_str().unwrap_or("");
        let position = match direction {
            StringSearchDirection::First => haystack.find(needle),
            StringSearchDirection::Last => haystack.rfind(needle),
        };
        ret!(
            rv,
            position.map_or_else(
                || Value::bool(false),
                |position| Value::long(position as i64),
            )
        );
    }
    let haystack = haystack.php_string_bytes().unwrap_or_default();
    let needle = needle.php_string_bytes().unwrap_or_default();
    let Some(position) = string_position_at_offset(
        haystack.as_ref(),
        needle.as_ref(),
        offset,
        direction,
        ascii_fold,
    ) else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "{function}(): Argument #3 ($offset) must be contained in argument #1 ($haystack)"
            ),
        ));
        return Ok(());
    };
    ret!(
        rv,
        position.map_or_else(
            || Value::bool(false),
            |position| Value::long(position as i64)
        )
    );
}

fn fn_stristr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(haystack) = typed_internal_string_argument(ed, eg, "stristr", 0, "haystack")? else {
        return Ok(());
    };
    let Some(needle) = typed_internal_string_argument(ed, eg, "stristr", 1, "needle")? else {
        return Ok(());
    };
    let before_needle = if arg_opt!(ed, 2).is_some() {
        let Some(before_needle) =
            typed_internal_bool_argument(ed, eg, "stristr", 2, "before_needle")?
        else {
            return Ok(());
        };
        before_needle
    } else {
        false
    };

    let haystack = php_string_to_bytes(&haystack);
    let needle = php_string_to_bytes(&needle);
    let Some(position) = ascii_case_insensitive_position(&haystack, &needle) else {
        ret!(rv, Value::bool(false));
    };
    let bytes = if before_needle {
        &haystack[..position]
    } else {
        &haystack[position..]
    };
    ret!(rv, Value::string(bytes_to_php_string(bytes)));
}

fn fn_strrpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_position_builtin(ed, rv, eg, "strrpos", StringSearchDirection::Last, false)
}

fn fn_strrchr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_before_needle = match arg_opt!(ed, 2) {
        None => Some(false),
        Some(value) if matches!(value.value_type(), ValueType::True | ValueType::False) => {
            Some(value.is_truthy())
        }
        Some(_) => None,
    };
    let exact_haystack = arg!(ed, 0);
    let exact_needle = arg!(ed, 1);
    if exact_haystack.value_type() == ValueType::String
        && exact_needle.value_type() == ValueType::String
        && let Some(before_needle) = exact_before_needle
    {
        let binary = exact_haystack.is_binary_string();
        let haystack = exact_haystack.php_string_bytes().unwrap_or_default();
        let needle = exact_needle
            .php_string_bytes()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(0);
        let value = haystack
            .iter()
            .rposition(|byte| *byte == needle)
            .map_or_else(
                || Value::bool(false),
                |position| {
                    let bytes = if before_needle {
                        &haystack[..position]
                    } else {
                        &haystack[position..]
                    };
                    php_byte_result(bytes.to_vec(), binary)
                },
            );
        ret!(rv, value);
    }

    let Some(haystack) =
        typed_internal_string_value_argument_expected(ed, eg, "strrchr", 0, "haystack", "string")?
    else {
        return Ok(());
    };
    let Some(needle) =
        typed_internal_string_value_argument_expected(ed, eg, "strrchr", 1, "needle", "string")?
    else {
        return Ok(());
    };
    let before_needle = if arg_opt!(ed, 2).is_some() {
        let Some(before_needle) =
            typed_internal_bool_argument(ed, eg, "strrchr", 2, "before_needle")?
        else {
            return Ok(());
        };
        before_needle
    } else {
        false
    };
    let binary = haystack.is_binary_string();
    let haystack = haystack.php_string_bytes().unwrap_or_default();
    let needle = needle
        .php_string_bytes()
        .and_then(|bytes| bytes.first().copied())
        .unwrap_or(0);
    let value = haystack
        .iter()
        .rposition(|byte| *byte == needle)
        .map_or_else(
            || Value::bool(false),
            |position| {
                let bytes = if before_needle {
                    &haystack[..position]
                } else {
                    &haystack[position..]
                };
                php_byte_result(bytes.to_vec(), binary)
            },
        );
    ret!(rv, value);
}

fn string_span_bounds(byte_len: usize, raw_offset: i64, raw_length: Option<i64>) -> (usize, usize) {
    let len = byte_len as i64;
    let start = if raw_offset < 0 {
        (len + raw_offset).max(0)
    } else {
        raw_offset.min(len)
    };
    let end = match raw_length {
        Some(length) => {
            if length < 0 {
                (len + length).max(start)
            } else {
                (start + length).min(len)
            }
        }
        None => len,
    };
    (start as usize, end as usize)
}

fn string_span_bytes(
    string: &[u8],
    characters: &[u8],
    start: usize,
    end: usize,
    accept_matches: bool,
) -> i64 {
    let mut accepted = [false; 256];
    for byte in characters {
        accepted[*byte as usize] = true;
    }
    string[start..end]
        .iter()
        .take_while(|byte| accepted[**byte as usize] == accept_matches)
        .count() as i64
}

fn string_span_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    accept_matches: bool,
) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0);
    let exact_characters = arg!(ed, 1);
    if exact_string.value_type() == ValueType::String
        && exact_characters.value_type() == ValueType::String
        && arg_opt!(ed, 2).is_none()
    {
        let string = exact_string.php_string_bytes().unwrap_or_default();
        let characters = exact_characters.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            Value::long(string_span_bytes(
                &string,
                &characters,
                0,
                string.len(),
                accept_matches,
            ))
        );
    }

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "string", "string")?
    else {
        return Ok(());
    };
    let Some(characters) =
        typed_internal_string_value_argument_expected(ed, eg, function, 1, "characters", "string")?
    else {
        return Ok(());
    };
    let offset = if arg_opt!(ed, 2).is_some() {
        let Some(offset) = typed_internal_int_argument(ed, eg, function, 2, "offset")? else {
            return Ok(());
        };
        offset
    } else {
        0
    };
    let length = match arg_opt!(ed, 3) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(length) =
                typed_internal_int_argument_expected(ed, eg, function, 3, "length", "?int")?
            else {
                return Ok(());
            };
            Some(length)
        }
    };
    let string = string.php_string_bytes().unwrap_or_default();
    let characters = characters.php_string_bytes().unwrap_or_default();
    let (start, end) = string_span_bounds(string.len(), offset, length);
    ret!(
        rv,
        Value::long(string_span_bytes(
            &string,
            &characters,
            start,
            end,
            accept_matches,
        ))
    );
}

fn fn_strspn(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_span_builtin(ed, rv, eg, "strspn", true)
}

fn fn_strcspn(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_span_builtin(ed, rv, eg, "strcspn", false)
}

#[inline]
fn strpbrk_result(string: &Value, characters: &Value) -> Result<Value, ()> {
    let string_bytes = string.php_string_bytes().unwrap_or_default();
    let character_bytes = characters.php_string_bytes().unwrap_or_default();
    if character_bytes.is_empty() {
        return Err(());
    }
    let mut accepted = [false; 256];
    for byte in character_bytes.iter().copied() {
        accepted[byte as usize] = true;
    }
    Ok(
        match string_bytes
            .iter()
            .position(|byte| accepted[*byte as usize])
        {
            Some(position) => php_byte_result(string_bytes[position..].to_vec(), false),
            None => Value::bool(false),
        },
    )
}

fn fn_strpbrk(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0);
    let exact_characters = arg!(ed, 1);
    if exact_string.value_type() == ValueType::String
        && exact_characters.value_type() == ValueType::String
    {
        match strpbrk_result(exact_string, exact_characters) {
            Ok(result) => ret!(rv, result),
            Err(()) => {
                eg.exception = Some(crate::value::make_error_value(
                    "ValueError",
                    "strpbrk(): Argument #2 ($characters) must be a non-empty string",
                ));
                return Ok(());
            }
        }
    }
    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "strpbrk", 0, "string", "string")?
    else {
        return Ok(());
    };
    let Some(characters) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "strpbrk",
        1,
        "characters",
        "string",
    )?
    else {
        return Ok(());
    };
    match strpbrk_result(&string, &characters) {
        Ok(result) => ret!(rv, result),
        Err(()) => {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "strpbrk(): Argument #2 ($characters) must be a non-empty string",
            ));
            Ok(())
        }
    }
}

fn fn_strtr(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let exact_subject = arg!(ed, 0);
    let exact_from = arg!(ed, 1);
    let exact_to = arg_opt!(ed, 2);
    if let Some(exact_to) = exact_to
        && exact_subject.value_type() == ValueType::String
        && exact_from.value_type() == ValueType::String
        && exact_to.value_type() == ValueType::String
        && !exact_subject.is_binary_string()
        && !exact_from.is_binary_string()
        && !exact_to.is_binary_string()
        && exact_subject.as_str().is_some_and(str::is_ascii)
        && exact_from.as_str().is_some_and(str::is_ascii)
        && exact_to.as_str().is_some_and(str::is_ascii)
    {
        let result = strtr_character_bytes(
            exact_subject.as_str().unwrap_or_default().as_bytes(),
            exact_from.as_str().unwrap_or_default().as_bytes(),
            exact_to.as_str().unwrap_or_default().as_bytes(),
        );
        ret!(
            rv,
            Value::string(String::from_utf8(result).expect("ASCII strtr result is valid UTF-8"))
        );
    }
    if exact_to.is_none()
        && exact_subject.value_type() == ValueType::String
        && !exact_subject.is_binary_string()
        && exact_subject.as_str().is_some_and(str::is_ascii)
        && let Some(pairs) = exact_from.as_array()
        && let Some(result) = strtr_ascii_pairs(exact_subject.as_str().unwrap_or_default(), pairs)
    {
        ret!(rv, Value::string(result));
    }

    let Some(subject) =
        typed_internal_string_value_argument_expected(ed, eg, "strtr", 0, "string", "string")?
    else {
        return Ok(());
    };

    if exact_to.is_some() {
        let Some(from) = typed_internal_string_value_argument_with_null_expected(
            ed,
            eg,
            "strtr",
            1,
            "from",
            "string",
            "array|string",
        )?
        else {
            return Ok(());
        };
        let Some(to) = typed_internal_string_value_argument_with_null_expected(
            ed, eg, "strtr", 2, "to", "string", "?string",
        )?
        else {
            return Ok(());
        };
        let source = subject.php_string_bytes().unwrap_or_default();
        let from_bytes = from.php_string_bytes().unwrap_or_default();
        let to_bytes = to.php_string_bytes().unwrap_or_default();
        let result = strtr_character_bytes(&source, &from_bytes, &to_bytes);
        let binary = subject.is_binary_string()
            || (to.is_binary_string()
                && source.iter().any(|byte| {
                    from_bytes
                        .iter()
                        .take(to_bytes.len())
                        .any(|from| from == byte)
                }));
        ret!(rv, php_byte_result(result, binary));
    }

    let from_or_pairs = owned_argument(ed, 1);
    let from_or_pairs = from_or_pairs.dereferenced();
    let Some(pairs) = from_or_pairs.as_array() else {
        typed_internal_argument_error(eg, "strtr", from_or_pairs, 2, "from", "array");
        return Ok(());
    };

    let input = subject.php_string_bytes().unwrap_or_default();
    if input.is_empty() || pairs.is_empty() {
        ret!(rv, subject);
    }

    let external_byte_keys = pairs.has_external_byte_keys();
    let mut replacements = Vec::with_capacity(pairs.len());
    for (key, value) in pairs.iter() {
        let search = match key {
            ArrayKey::Int(value) => value.to_string().into_bytes(),
            ArrayKey::String(value) if external_byte_keys => php_string_to_bytes(&value),
            ArrayKey::String(value) => value.into_bytes(),
        };
        if search.is_empty() {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "strtr(): Ignoring replacement of empty string",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            continue;
        }
        replacements.push(StrtrReplacement {
            search,
            source: value.clone(),
            converted: None,
        });
    }
    replacements.sort_by(|left, right| right.search.len().cmp(&left.search.len()));

    let mut translated = Vec::new();
    if translated.try_reserve_exact(input.len()).is_err() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "strtr(): Failed to allocate result string",
        ));
        return Ok(());
    }
    let mut position = 0usize;
    let mut used_binary_replacement = false;
    while position < input.len() {
        let matched = replacements
            .iter()
            .position(|replacement| input[position..].starts_with(&replacement.search));
        let Some(index) = matched else {
            translated.push(input[position]);
            position += 1;
            continue;
        };
        if replacements[index].converted.is_none() {
            let Some(converted) = replacement_item_text(ed, eg, &replacements[index].source)?
            else {
                return Ok(());
            };
            replacements[index].converted = Some(converted);
        }
        let replacement = replacements[index]
            .converted
            .as_ref()
            .expect("matched strtr replacement was converted");
        let Some(result_length) = translated.len().checked_add(replacement.bytes.len()) else {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "strtr(): Failed to allocate result string",
            ));
            return Ok(());
        };
        if result_length > translated.capacity()
            && translated
                .try_reserve(result_length - translated.len())
                .is_err()
        {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "strtr(): Failed to allocate result string",
            ));
            return Ok(());
        }
        translated.extend_from_slice(&replacement.bytes);
        used_binary_replacement |= replacement.binary && !replacement.bytes.is_empty();
        position += replacements[index].search.len();
    }
    ret!(
        rv,
        php_byte_result(
            translated,
            subject.is_binary_string() || used_binary_replacement
        )
    );
}

struct StrtrReplacement {
    search: Vec<u8>,
    source: Value,
    converted: Option<ReplacementText>,
}

fn strtr_character_bytes(subject: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut translations = std::array::from_fn::<_, 256, _>(|byte| byte as u8);
    for (&from, &to) in from.iter().zip(to) {
        translations[from as usize] = to;
    }
    subject
        .iter()
        .map(|byte| translations[*byte as usize])
        .collect()
}

fn strtr_ascii_pairs(subject: &str, pairs: &PhpArray) -> Option<String> {
    if subject.is_empty() || pairs.is_empty() {
        return Some(subject.to_string());
    }
    if pairs.has_external_byte_keys() {
        return None;
    }

    let mut replacements = Vec::with_capacity(pairs.len());
    for (key, replacement) in pairs.iter() {
        let search = match key {
            ArrayKey::Int(value) => value.to_string().into_bytes(),
            ArrayKey::String(value) if value.is_ascii() && !value.is_empty() => value.into_bytes(),
            ArrayKey::String(_) => return None,
        };
        let replacement = replacement.dereferenced();
        if replacement.value_type() != ValueType::String
            || replacement.is_binary_string()
            || !replacement.as_str().is_some_and(str::is_ascii)
        {
            return None;
        }
        replacements.push((search, replacement.as_str().unwrap_or_default().as_bytes()));
    }
    replacements.sort_by(|(left, _), (right, _)| right.len().cmp(&left.len()));

    let input = subject.as_bytes();
    let mut translated = Vec::with_capacity(input.len());
    let mut position = 0usize;
    while position < input.len() {
        if let Some((search, replacement)) = replacements
            .iter()
            .find(|(search, _)| input[position..].starts_with(search))
        {
            translated.extend_from_slice(replacement);
            position += search.len();
        } else {
            translated.push(input[position]);
            position += 1;
        }
    }
    Some(String::from_utf8(translated).expect("ASCII strtr map result is valid UTF-8"))
}

fn php_byte_result(bytes: Vec<u8>, binary: bool) -> Value {
    if binary || !bytes.is_ascii() {
        Value::binary_string(&bytes)
    } else {
        Value::string(String::from_utf8(bytes).expect("ASCII PHP byte result is valid UTF-8"))
    }
}

fn fn_str_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    string_replace_builtin(ed, rv, eg, "str_replace", false)
}

#[derive(Clone)]
struct ReplacementText {
    bytes: Vec<u8>,
    binary: bool,
}

impl ReplacementText {
    fn from_string_value(value: &Value) -> Self {
        Self {
            bytes: value.php_string_bytes().unwrap_or_default().into_owned(),
            binary: value.is_binary_string(),
        }
    }

    fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            binary: false,
        }
    }
}

fn replacement_item_text(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<ReplacementText>, VmError> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::String {
        return Ok(Some(ReplacementText::from_string_value(value)));
    }
    if value.value_type() == ValueType::Array {
        report_internal_diagnostic(eg, ed, 2, "Warning", "Array to string conversion")?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        return Ok(Some(ReplacementText {
            bytes: b"Array".to_vec(),
            binary: false,
        }));
    }
    if value.value_type() == ValueType::Closure {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Object of class Closure could not be converted to string",
        ));
        return Ok(None);
    }
    if value.value_type() != ValueType::Object {
        return Ok(Some(ReplacementText {
            bytes: value
                .echo_to_string_with_precision(eg.precision)
                .into_bytes(),
            binary: false,
        }));
    }

    let class_name = value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "object".to_string());
    let rendered = crate::vm::execute::call_object_string_conversion(eg, value)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(rendered) = rendered else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Object of class {class_name} could not be converted to string"),
        ));
        return Ok(None);
    };
    let rendered = rendered.dereferenced();
    if rendered.value_type() != ValueType::String {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{class_name}::__toString(): Return value must be of type string"),
        ));
        return Ok(None);
    }
    Ok(Some(ReplacementText::from_string_value(rendered)))
}

fn typed_internal_array_or_string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Value>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    if matches!(argument.value_type(), ValueType::Array | ValueType::String) {
        return Ok(Some(argument.clone()));
    }
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #{} (${parameter}) of type array|string is deprecated",
                    index + 1
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(Value::string(String::new()))
        }
        ValueType::False if !strict => Some(Value::string(String::new())),
        ValueType::True if !strict => Some(Value::string("1")),
        ValueType::Long | ValueType::Double if !strict => {
            if argument.as_double().is_some_and(f64::is_nan) {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to string",
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            Some(Value::string(
                argument.echo_to_string_with_precision(eg.precision),
            ))
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(rendered) = rendered else {
                typed_internal_argument_error(
                    eg,
                    function,
                    argument,
                    index as usize + 1,
                    parameter,
                    "array|string",
                );
                return Ok(None);
            };
            let rendered = rendered.dereferenced();
            if rendered.value_type() != ValueType::String {
                let class_name = argument.diagnostic_type_name();
                let actual = rendered.diagnostic_type_name();
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "{class_name}::__toString(): Return value must be of type string, {actual} returned"
                    ),
                ));
                return Ok(None);
            }
            Some(rendered.clone())
        }
        _ => {
            typed_internal_argument_error(
                eg,
                function,
                argument,
                index as usize + 1,
                parameter,
                "array|string",
            );
            None
        }
    };
    Ok(converted)
}

fn replace_php_bytes_once(
    source: &[u8],
    source_binary: bool,
    search: &ReplacementText,
    replacement: &ReplacementText,
    case_insensitive: bool,
    count: &mut usize,
) -> (Vec<u8>, bool) {
    if search.bytes.is_empty() || search.bytes.len() > source.len() {
        return (source.to_vec(), source_binary);
    }

    let matches_at = |candidate: &[u8]| {
        if case_insensitive {
            candidate.eq_ignore_ascii_case(&search.bytes)
        } else {
            candidate == search.bytes
        }
    };
    let mut result = Vec::with_capacity(source.len());
    let mut position = 0;
    let mut used_binary_replacement = false;
    while position + search.bytes.len() <= source.len() {
        let candidate = &source[position..position + search.bytes.len()];
        if matches_at(candidate) {
            result.extend_from_slice(&replacement.bytes);
            *count = count.saturating_add(1);
            used_binary_replacement |= replacement.binary;
            position += search.bytes.len();
        } else {
            result.push(source[position]);
            position += 1;
        }
    }
    result.extend_from_slice(&source[position..]);
    (
        result,
        source_binary || (used_binary_replacement && !replacement.bytes.is_empty()),
    )
}

fn replace_php_bytes(
    subject: ReplacementText,
    replacements: &[(ReplacementText, ReplacementText)],
    case_insensitive: bool,
    count: &mut usize,
) -> Value {
    let mut bytes = subject.bytes;
    let mut binary = subject.binary;
    for (search, replacement) in replacements {
        (bytes, binary) =
            replace_php_bytes_once(&bytes, binary, search, replacement, case_insensitive, count);
    }
    if binary || !bytes.is_ascii() {
        Value::binary_string(&bytes)
    } else {
        Value::string(String::from_utf8(bytes).expect("ASCII replacement result is valid UTF-8"))
    }
}

fn replace_ascii_case_insensitive(
    subject: &str,
    search: &str,
    replacement: &str,
    count: &mut usize,
) -> String {
    if search.is_empty() {
        return subject.to_string();
    }
    let source = subject.as_bytes();
    let needle = search.as_bytes();
    let first = needle[0];
    let mut result = String::with_capacity(subject.len());
    let mut copied = 0;
    let mut scan = 0;
    while scan + needle.len() <= source.len() {
        if source[scan].eq_ignore_ascii_case(&first)
            && source[scan..scan + needle.len()].eq_ignore_ascii_case(needle)
        {
            result.push_str(&subject[copied..scan]);
            result.push_str(replacement);
            *count = count.saturating_add(1);
            copied = scan + needle.len();
            scan = copied;
        } else {
            scan += 1;
        }
    }
    result.push_str(&subject[copied..]);
    result
}

fn replace_text_case_sensitive(
    subject: &str,
    search: &str,
    replacement: &str,
    count: &mut usize,
) -> String {
    if search.is_empty() {
        return subject.to_string();
    }
    let mut result = String::with_capacity(subject.len());
    let mut copied = 0;
    for (matched, _) in subject.match_indices(search) {
        result.push_str(&subject[copied..matched]);
        result.push_str(replacement);
        *count = count.saturating_add(1);
        copied = matched + search.len();
    }
    result.push_str(&subject[copied..]);
    result
}

fn string_replace_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    case_insensitive: bool,
) -> Result<(), VmError> {
    let Some(search) = typed_internal_array_or_string_argument(ed, eg, function, 0, "search")?
    else {
        return Ok(());
    };
    let Some(replace) = typed_internal_array_or_string_argument(ed, eg, function, 1, "replace")?
    else {
        return Ok(());
    };
    let Some(subject) = typed_internal_array_or_string_argument(ed, eg, function, 2, "subject")?
    else {
        return Ok(());
    };

    if search.value_type() == ValueType::String && replace.value_type() == ValueType::Array {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string"
            ),
        ));
        return Ok(());
    }

    if search.value_type() == ValueType::String
        && replace.value_type() == ValueType::String
        && subject.value_type() == ValueType::String
        && !search.is_binary_string()
        && !replace.is_binary_string()
        && !subject.is_binary_string()
        && search.as_str().is_some_and(str::is_ascii)
        && replace.as_str().is_some_and(str::is_ascii)
        && subject.as_str().is_some_and(str::is_ascii)
    {
        let mut count = 0;
        let subject = subject.as_str().unwrap_or_default();
        let search = search.as_str().unwrap_or_default();
        let replace = replace.as_str().unwrap_or_default();
        let result = if case_insensitive {
            replace_ascii_case_insensitive(subject, search, replace, &mut count)
        } else {
            replace_text_case_sensitive(subject, search, replace, &mut count)
        };
        arg_mut!(ed, 3, Value::long(count as i64));
        ret!(rv, Value::string(result));
    }

    let mut replacements = Vec::new();
    if let Some(searches) = search.as_array() {
        let replacement_values = replace
            .as_array()
            .map(|array| array.values().collect::<Vec<_>>());
        let scalar_replacement = if replace.value_type() == ValueType::String {
            Some(ReplacementText::from_string_value(&replace))
        } else {
            None
        };
        replacements.reserve(searches.len());
        for (index, search) in searches.values().enumerate() {
            let Some(search) = replacement_item_text(ed, eg, search)? else {
                return Ok(());
            };
            let replacement = if let Some(values) = replacement_values.as_ref() {
                match values.get(index) {
                    Some(value) => {
                        let Some(value) = replacement_item_text(ed, eg, value)? else {
                            return Ok(());
                        };
                        value
                    }
                    None => ReplacementText::empty(),
                }
            } else {
                scalar_replacement
                    .as_ref()
                    .map(|value| ReplacementText {
                        bytes: value.bytes.clone(),
                        binary: value.binary,
                    })
                    .unwrap_or_else(ReplacementText::empty)
            };
            replacements.push((search, replacement));
        }
    } else {
        replacements.push((
            ReplacementText::from_string_value(&search),
            ReplacementText::from_string_value(&replace),
        ));
    }

    let mut count = 0;
    let result = if let Some(subjects) = subject.as_array() {
        let mut result = PhpArray::new();
        for (key, subject) in subjects.iter() {
            let Some(subject) = replacement_item_text(ed, eg, subject)? else {
                return Ok(());
            };
            result.set(
                key,
                replace_php_bytes(subject, &replacements, case_insensitive, &mut count),
            );
        }
        Value::array(result)
    } else {
        replace_php_bytes(
            ReplacementText::from_string_value(&subject),
            &replacements,
            case_insensitive,
            &mut count,
        )
    };

    // Writing the omitted optional frame slot is unobservable. When &$count
    // was supplied, arg_mut! follows its reference, including Reference(Undef).
    arg_mut!(ed, 3, Value::long(count as i64));
    ret!(rv, result);
}

enum SubstrIntegerArgument {
    Scalar(Option<i64>),
    Array(Value),
}

fn typed_internal_array_or_int_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    index: u32,
    parameter: &str,
    nullable: bool,
) -> Result<Option<SubstrIntegerArgument>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    if argument.value_type() == ValueType::Array {
        return Ok(Some(SubstrIntegerArgument::Array(argument.clone())));
    }
    if nullable && argument.value_type() == ValueType::Null {
        return Ok(Some(SubstrIntegerArgument::Scalar(None)));
    }

    let expected = if nullable {
        "array|int|null"
    } else {
        "array|int"
    };
    if argument.value_type() == ValueType::Null && !internal_call_is_strict(ed) {
        report_internal_deprecation(
            eg,
            ed,
            &format!(
                "substr_replace(): Passing null to parameter #{} (${parameter}) of type {expected} is deprecated",
                index + 1
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        return Ok(Some(SubstrIntegerArgument::Scalar(Some(0))));
    }

    let Some(integer) =
        typed_internal_int_argument_expected(ed, eg, "substr_replace", index, parameter, expected)?
    else {
        return Ok(None);
    };
    Ok(Some(SubstrIntegerArgument::Scalar(Some(integer))))
}

fn substr_control_item_to_long(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<i64>, VmError> {
    let value = value.dereferenced();
    if matches!(value.value_type(), ValueType::Object | ValueType::Closure) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "Object of class {} could not be converted to int",
                value.diagnostic_type_name()
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(crate::vm::execute::explicit_long_conversion(value)))
}

fn substr_replace_bounds(source_length: usize, offset: i64, length: Option<i64>) -> (usize, usize) {
    let start = if offset < 0 {
        source_length.saturating_sub(usize::try_from(offset.unsigned_abs()).unwrap_or(usize::MAX))
    } else {
        usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(source_length)
    };
    let end = match length {
        None => source_length,
        Some(length) if length < 0 => source_length
            .saturating_sub(usize::try_from(length.unsigned_abs()).unwrap_or(usize::MAX))
            .max(start),
        Some(length) => start
            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
            .min(source_length),
    };
    (start, end)
}

fn replace_php_substring(
    eg: &mut ExecutorGlobals,
    source: ReplacementText,
    replacement: &ReplacementText,
    offset: i64,
    length: Option<i64>,
) -> Option<Value> {
    let (start, end) = substr_replace_bounds(source.bytes.len(), offset, length);
    let result_length = start
        .checked_add(replacement.bytes.len())
        .and_then(|length| length.checked_add(source.bytes.len() - end));
    let Some(result_length) = result_length else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "substr_replace(): Failed to allocate result string",
        ));
        return None;
    };
    let mut result = Vec::new();
    if result.try_reserve_exact(result_length).is_err() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "substr_replace(): Failed to allocate result string",
        ));
        return None;
    }
    result.extend_from_slice(&source.bytes[..start]);
    result.extend_from_slice(&replacement.bytes);
    result.extend_from_slice(&source.bytes[end..]);
    let binary = source.binary || (replacement.binary && !replacement.bytes.is_empty());
    Some(if binary || !result.is_ascii() {
        Value::binary_string(&result)
    } else {
        Value::string(String::from_utf8(result).expect("ASCII substring result is valid UTF-8"))
    })
}

fn replace_text_substring(
    eg: &mut ExecutorGlobals,
    source: &str,
    replacement: &str,
    start: usize,
    end: usize,
) -> Option<Value> {
    let result_length = start
        .checked_add(replacement.len())
        .and_then(|length| length.checked_add(source.len() - end));
    let Some(result_length) = result_length else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "substr_replace(): Failed to allocate result string",
        ));
        return None;
    };
    let mut result = String::with_capacity(result_length);
    result.push_str(&source[..start]);
    result.push_str(replacement);
    result.push_str(&source[end..]);
    Some(Value::string(result))
}

pub(super) fn substr_replace_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_subject = arg!(ed, 0).dereferenced();
    let exact_replacement = arg!(ed, 1).dereferenced();
    let exact_offset = arg!(ed, 2).dereferenced();
    let exact_length = match arg_opt!(ed, 3) {
        None => Some(None),
        Some(length) => match length.dereferenced().value_type() {
            ValueType::Null => Some(None),
            ValueType::Long => Some(length.dereferenced().as_long()),
            _ => None,
        },
    };
    if exact_subject.value_type() == ValueType::String
        && exact_replacement.value_type() == ValueType::String
        && exact_offset.value_type() == ValueType::Long
        && let Some(length) = exact_length
        && !exact_subject.is_binary_string()
        && !exact_replacement.is_binary_string()
        && exact_subject.as_str().is_some_and(str::is_ascii)
        && exact_replacement.as_str().is_some_and(str::is_ascii)
    {
        let source = exact_subject.as_str().unwrap_or_default();
        let replacement = exact_replacement.as_str().unwrap_or_default();
        let (start, end) = substr_replace_bounds(
            source.len(),
            exact_offset.as_long().unwrap_or_default(),
            length,
        );
        if source.is_char_boundary(start) && source.is_char_boundary(end) {
            let Some(result) = replace_text_substring(eg, source, replacement, start, end) else {
                return Ok(());
            };
            ret!(rv, result);
        }
    }

    let Some(subject) =
        typed_internal_array_or_string_argument(ed, eg, "substr_replace", 0, "string")?
    else {
        return Ok(());
    };
    let Some(replacement) =
        typed_internal_array_or_string_argument(ed, eg, "substr_replace", 1, "replace")?
    else {
        return Ok(());
    };
    let Some(offset) = typed_internal_array_or_int_argument(ed, eg, 2, "offset", false)? else {
        return Ok(());
    };
    let length = if arg_opt!(ed, 3).is_some() {
        let Some(length) = typed_internal_array_or_int_argument(ed, eg, 3, "length", true)? else {
            return Ok(());
        };
        length
    } else {
        SubstrIntegerArgument::Scalar(None)
    };

    if subject.value_type() == ValueType::String {
        if matches!(offset, SubstrIntegerArgument::Array(_)) {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "substr_replace(): Argument #3 ($offset) cannot be an array when working on a single string",
            ));
            return Ok(());
        }
        if matches!(length, SubstrIntegerArgument::Array(_)) {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "substr_replace(): Argument #4 ($length) cannot be an array when working on a single string",
            ));
            return Ok(());
        }
        let replacement = if let Some(replacements) = replacement.as_array() {
            match replacements.values().next() {
                Some(value) => {
                    let Some(value) = replacement_item_text(ed, eg, value)? else {
                        return Ok(());
                    };
                    value
                }
                None => ReplacementText::empty(),
            }
        } else {
            ReplacementText::from_string_value(&replacement)
        };
        let SubstrIntegerArgument::Scalar(Some(offset)) = offset else {
            unreachable!("the required offset has a scalar integer")
        };
        let SubstrIntegerArgument::Scalar(length) = length else {
            unreachable!("the scalar-subject length was rejected above")
        };
        let Some(result) = replace_php_substring(
            eg,
            ReplacementText::from_string_value(&subject),
            &replacement,
            offset,
            length,
        ) else {
            return Ok(());
        };
        ret!(rv, result);
    }

    let replacement_values = replacement
        .as_array()
        .map(|array| array.values().cloned().collect::<Vec<_>>());
    let scalar_replacement = if replacement.value_type() == ValueType::String {
        Some(ReplacementText::from_string_value(&replacement))
    } else {
        None
    };
    let offset_values = match &offset {
        SubstrIntegerArgument::Array(value) => Some(
            value
                .as_array()
                .expect("array offset argument")
                .values()
                .cloned()
                .collect::<Vec<_>>(),
        ),
        SubstrIntegerArgument::Scalar(_) => None,
    };
    let scalar_offset = match offset {
        SubstrIntegerArgument::Scalar(Some(offset)) => Some(offset),
        SubstrIntegerArgument::Array(_) => None,
        SubstrIntegerArgument::Scalar(None) => unreachable!("offset is not nullable"),
    };
    let length_values = match &length {
        SubstrIntegerArgument::Array(value) => Some(
            value
                .as_array()
                .expect("array length argument")
                .values()
                .cloned()
                .collect::<Vec<_>>(),
        ),
        SubstrIntegerArgument::Scalar(_) => None,
    };
    let scalar_length = match length {
        SubstrIntegerArgument::Scalar(length) => length,
        SubstrIntegerArgument::Array(_) => None,
    };
    let subjects = subject
        .as_array()
        .expect("typed subject is either a string or an array")
        .iter()
        .map(|(key, value)| (key, value.clone()))
        .collect::<Vec<_>>();
    let mut result = PhpArray::new();
    for (index, (key, subject)) in subjects.into_iter().enumerate() {
        let Some(subject) = replacement_item_text(ed, eg, &subject)? else {
            return Ok(());
        };
        let replacement = if let Some(values) = replacement_values.as_ref() {
            match values.get(index) {
                Some(value) => {
                    let Some(value) = replacement_item_text(ed, eg, value)? else {
                        return Ok(());
                    };
                    value
                }
                None => ReplacementText::empty(),
            }
        } else {
            scalar_replacement
                .as_ref()
                .cloned()
                .unwrap_or_else(ReplacementText::empty)
        };
        let offset = if let Some(values) = offset_values.as_ref() {
            match values.get(index) {
                Some(value) => {
                    let Some(value) = substr_control_item_to_long(ed, eg, value)? else {
                        return Ok(());
                    };
                    value
                }
                None => 0,
            }
        } else {
            scalar_offset.expect("scalar offset exists when no offset array exists")
        };
        let length = if let Some(values) = length_values.as_ref() {
            match values.get(index) {
                Some(value) => {
                    let Some(value) = substr_control_item_to_long(ed, eg, value)? else {
                        return Ok(());
                    };
                    Some(value)
                }
                None => None,
            }
        } else {
            scalar_length
        };
        let Some(value) = replace_php_substring(eg, subject, &replacement, offset, length) else {
            return Ok(());
        };
        result.set(key, value);
    }
    ret!(rv, Value::array(result));
}

#[inline(always)]
fn direct_strtolower(args: &[Value]) -> Result<Value, VmError> {
    let s = direct_arg_str(args, 0);
    // PHP strtolower is ASCII-only — use make_ascii_lowercase for performance
    let mut bytes = s.as_bytes().to_vec();
    bytes.make_ascii_lowercase();
    Ok(Value::string(unsafe { String::from_utf8_unchecked(bytes) }))
}

fn fn_strtolower(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = direct_strtolower(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

#[inline(always)]
fn direct_strtoupper(args: &[Value]) -> Result<Value, VmError> {
    let s = direct_arg_str(args, 0);
    let mut bytes = s.as_bytes().to_vec();
    bytes.make_ascii_uppercase();
    Ok(Value::string(unsafe { String::from_utf8_unchecked(bytes) }))
}

fn fn_strtoupper(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = direct_strtoupper(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

fn alphanumeric_ascii_string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<Value>, VmError> {
    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "string", "string")?
    else {
        return Ok(None);
    };
    let bytes = string.php_string_bytes().unwrap_or_default();
    let message = if bytes.is_empty() {
        Some(format!(
            "{function}(): Argument #1 ($string) must not be empty"
        ))
    } else if bytes.iter().any(|byte| !byte.is_ascii_alphanumeric()) {
        Some(format!(
            "{function}(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters"
        ))
    } else {
        None
    };
    if let Some(message) = message {
        eg.exception = Some(crate::value::make_error_value("ValueError", &message));
        return Ok(None);
    }
    Ok(Some(string))
}

#[inline]
fn string_result_preserving_bytes(source: &Value, result: String) -> Value {
    if source.is_binary_string() {
        Value::binary_string_from_storage(result)
    } else {
        Value::string(result)
    }
}

fn fn_str_increment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = alphanumeric_ascii_string_argument(ed, eg, "str_increment")? else {
        return Ok(());
    };
    let result =
        crate::vm::execute::increment_php_alphanumeric_string(string.as_str().unwrap_or_default());
    ret!(rv, string_result_preserving_bytes(&string, result));
}

fn decrement_php_alphanumeric_string(value: &str) -> Option<String> {
    let mut bytes = value.as_bytes().to_vec();
    if bytes.first() == Some(&b'0') {
        return None;
    }

    let mut borrow = true;
    for byte in bytes.iter_mut().rev() {
        if !borrow {
            break;
        }
        match *byte {
            b'1'..=b'9' | b'B'..=b'Z' | b'b'..=b'z' => {
                *byte -= 1;
                borrow = false;
            }
            b'0' => *byte = b'9',
            b'A' => *byte = b'Z',
            b'a' => *byte = b'z',
            _ => unreachable!("caller validates alphanumeric ASCII"),
        }
    }

    if borrow {
        if bytes.len() == 1 {
            return None;
        }
        bytes.remove(0);
    } else if bytes.len() > 1 && bytes[0] == b'0' {
        bytes.remove(0);
    }
    Some(String::from_utf8(bytes).expect("ASCII decrement preserves UTF-8"))
}

fn fn_str_decrement(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = alphanumeric_ascii_string_argument(ed, eg, "str_decrement")? else {
        return Ok(());
    };
    let source = string.as_str().unwrap_or_default();
    let Some(result) = decrement_php_alphanumeric_string(source) else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "str_decrement(): Argument #1 ($string) \"{source}\" is out of decrement range"
            ),
        ));
        return Ok(());
    };
    ret!(rv, string_result_preserving_bytes(&string, result));
}

const DEFAULT_TRIM_MASK: [bool; 256] = {
    let mut mask = [false; 256];
    mask[0] = true;
    mask[b'\t' as usize] = true;
    mask[b'\n' as usize] = true;
    mask[11] = true;
    mask[b'\r' as usize] = true;
    mask[b' ' as usize] = true;
    mask
};

/// Parse the warning-free charlist grammar used by ordinary trim calls. Any
/// ambiguous or invalid dot run falls back to the diagnostic parser below.
#[inline(always)]
fn valid_php_charlist_mask(characters: &[u8]) -> Option<[bool; 256]> {
    let mut mask = [false; 256];
    let mut index = 0;
    while index < characters.len() {
        if index + 3 < characters.len()
            && characters[index + 1] == b'.'
            && characters[index + 2] == b'.'
        {
            let start = characters[index];
            let end = characters[index + 3];
            if start > end {
                return None;
            }
            for byte in start..=end {
                mask[byte as usize] = true;
            }
            index += 4;
        } else {
            if characters[index] == b'.' && characters.get(index + 1) == Some(&b'.') {
                return None;
            }
            mask[characters[index] as usize] = true;
            index += 1;
        }
    }
    Some(mask)
}

fn php_charlist_mask(characters: &[u8]) -> ([bool; 256], Vec<&'static str>) {
    let mut mask = [false; 256];
    let mut warnings = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if index + 3 < characters.len()
            && characters[index + 1] == b'.'
            && characters[index + 2] == b'.'
        {
            if characters[index] <= characters[index + 3] {
                for byte in characters[index]..=characters[index + 3] {
                    mask[byte as usize] = true;
                }
                index += 4;
                continue;
            }

            // A dot can itself begin the next range. PHP shifts to that
            // interpretation when another byte follows it (for example,
            // `a...z` is the literal `a` followed by the range `...z`).
            if characters[index + 3] == b'.' && index + 4 < characters.len() {
                mask[characters[index] as usize] = true;
                index += 1;
                continue;
            }

            warnings.push("Invalid '..'-range, '..'-range needs to be incrementing");
            mask[characters[index] as usize] = true;
            index += 2;
            continue;
        }

        if characters[index] == b'.' && characters.get(index + 1) == Some(&b'.') {
            if index == 0 {
                warnings.push("Invalid '..'-range, no character to the left of '..'");
            } else if index + 2 == characters.len() {
                warnings.push("Invalid '..'-range, no character to the right of '..'");
            } else if characters[index - 1] > characters[index + 2] {
                warnings.push("Invalid '..'-range, '..'-range needs to be incrementing");
            } else {
                warnings.push("Invalid '..'-range");
            }
        }
        mask[characters[index] as usize] = true;
        index += 1;
    }
    (mask, warnings)
}

fn addcslashes_bytes(string: &[u8], mask: &[bool; 256]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(string.len());
    for &byte in string {
        if !mask[byte as usize] {
            escaped.push(byte);
            continue;
        }
        escaped.push(b'\\');
        match byte {
            7 => escaped.push(b'a'),
            8 => escaped.push(b'b'),
            b'\t' => escaped.push(b't'),
            b'\n' => escaped.push(b'n'),
            11 => escaped.push(b'v'),
            12 => escaped.push(b'f'),
            b'\r' => escaped.push(b'r'),
            0..=31 | 127..=255 => {
                escaped.push(b'0' + ((byte >> 6) & 7));
                escaped.push(b'0' + ((byte >> 3) & 7));
                escaped.push(b'0' + (byte & 7));
            }
            _ => escaped.push(byte),
        }
    }
    escaped
}

fn fn_addcslashes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "addcslashes",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let Some(characters) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "addcslashes",
        1,
        "characters",
        "string",
    )?
    else {
        return Ok(());
    };
    let string = string.php_string_bytes().unwrap_or_default();
    let characters = characters.php_string_bytes().unwrap_or_default();
    let (mask, warnings) = php_charlist_mask(&characters);
    for warning in warnings {
        report_internal_diagnostic(eg, ed, 2, "Warning", &format!("addcslashes(): {warning}"))?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::binary_string(&addcslashes_bytes(&string, &mask)));
}

fn addslashes_bytes(string: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(string.len());
    for &byte in string {
        match byte {
            0 => escaped.extend_from_slice(b"\\0"),
            b'\'' | b'"' | b'\\' => {
                escaped.push(b'\\');
                escaped.push(byte);
            }
            _ => escaped.push(byte),
        }
    }
    escaped
}

fn stripslashes_bytes(string: &[u8]) -> Vec<u8> {
    let mut stripped = Vec::with_capacity(string.len());
    let mut position = 0;
    while position < string.len() {
        if string[position] != b'\\' {
            stripped.push(string[position]);
            position += 1;
            continue;
        }
        let Some(&escaped) = string.get(position + 1) else {
            break;
        };
        stripped.push(if escaped == b'0' { 0 } else { escaped });
        position += 2;
    }
    stripped
}

fn stripcslashes_bytes(string: &[u8]) -> Vec<u8> {
    let mut stripped = Vec::with_capacity(string.len());
    let mut position = 0;
    while position < string.len() {
        let byte = string[position];
        if byte != b'\\' {
            stripped.push(byte);
            position += 1;
            continue;
        }
        let Some(&escaped) = string.get(position + 1) else {
            stripped.push(b'\\');
            break;
        };
        position += 2;
        match escaped {
            b'a' => stripped.push(7),
            b'b' => stripped.push(8),
            b't' => stripped.push(b'\t'),
            b'n' => stripped.push(b'\n'),
            b'v' => stripped.push(11),
            b'f' => stripped.push(12),
            b'r' => stripped.push(b'\r'),
            b'0'..=b'7' => {
                let mut value = escaped - b'0';
                let mut digits = 1;
                while digits < 3 {
                    let Some(&digit @ b'0'..=b'7') = string.get(position) else {
                        break;
                    };
                    value = value.wrapping_mul(8).wrapping_add(digit - b'0');
                    position += 1;
                    digits += 1;
                }
                stripped.push(value);
            }
            b'x' | b'X' => {
                let mut value = 0_u8;
                let mut digits = 0;
                while digits < 2 {
                    let Some(&digit) = string.get(position) else {
                        break;
                    };
                    let nibble = match digit {
                        b'0'..=b'9' => digit - b'0',
                        b'a'..=b'f' => digit - b'a' + 10,
                        b'A'..=b'F' => digit - b'A' + 10,
                        _ => break,
                    };
                    value = (value << 4) | nibble;
                    position += 1;
                    digits += 1;
                }
                if digits == 0 {
                    stripped.push(escaped);
                } else {
                    stripped.push(value);
                }
            }
            _ => stripped.push(escaped),
        }
    }
    stripped
}

fn unary_slash_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    transform: fn(&[u8]) -> Vec<u8>,
) -> Result<(), VmError> {
    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "string", "string")?
    else {
        return Ok(());
    };
    let string = string.php_string_bytes().unwrap_or_default();
    ret!(rv, Value::binary_string(&transform(&string)));
}

fn fn_addslashes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    unary_slash_string(ed, rv, eg, "addslashes", addslashes_bytes)
}

fn fn_stripslashes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    unary_slash_string(ed, rv, eg, "stripslashes", stripslashes_bytes)
}

fn fn_stripcslashes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    unary_slash_string(ed, rv, eg, "stripcslashes", stripcslashes_bytes)
}

#[inline(always)]
fn trim_php_byte_bounds(
    bytes: &[u8],
    mask: &[bool; 256],
    trim_left: bool,
    trim_right: bool,
) -> (usize, usize) {
    let mut start = 0;
    let mut end = bytes.len();
    if trim_left {
        while start < end && mask[bytes[start] as usize] {
            start += 1;
        }
    }
    if trim_right {
        while end > start && mask[bytes[end - 1] as usize] {
            end -= 1;
        }
    }
    (start, end)
}

#[inline(always)]
fn trimmed_php_string_value(
    source: &Value,
    bytes: &[u8],
    mask: &[bool; 256],
    trim_left: bool,
    trim_right: bool,
) -> Value {
    let (start, end) = trim_php_byte_bounds(bytes, mask, trim_left, trim_right);
    if start == 0 && end == bytes.len() {
        return source.clone();
    }
    let trimmed = &bytes[start..end];
    if !source.is_binary_string()
        && let Some(text) = source.as_str().and_then(|storage| storage.get(start..end))
    {
        Value::string(text.to_owned())
    } else {
        Value::binary_string(trimmed)
    }
}

fn fn_trim_direction(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    trim_left: bool,
    trim_right: bool,
) -> Result<(), VmError> {
    // Ordinary string calls are overwhelmingly dominant. They need neither
    // scalar coercion nor a defensive Value clone, and valid charlists cannot
    // re-enter through an error handler. Keep that path allocation-free until
    // the returned substring itself must be materialized.
    let source_argument = arg!(ed, 0);
    if source_argument.value_type() == ValueType::String {
        if let Some(characters) = arg_opt!(ed, 1) {
            if characters.value_type() == ValueType::String {
                let characters = characters.php_string_bytes().unwrap_or_default();
                if let Some(mask) = valid_php_charlist_mask(&characters) {
                    let bytes = source_argument.php_string_bytes().unwrap_or_default();
                    ret!(
                        rv,
                        trimmed_php_string_value(
                            source_argument,
                            &bytes,
                            &mask,
                            trim_left,
                            trim_right,
                        )
                    );
                }
            }
        } else {
            let bytes = source_argument.php_string_bytes().unwrap_or_default();
            ret!(
                rv,
                trimmed_php_string_value(
                    source_argument,
                    &bytes,
                    &DEFAULT_TRIM_MASK,
                    trim_left,
                    trim_right,
                )
            );
        }
    }

    let function = if function == "rtrim" {
        crate::vm::execute::invoked_internal_alias_name(eg, ed).unwrap_or(function)
    } else {
        function
    };

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "string", "string")?
    else {
        return Ok(());
    };
    let explicit_mask;
    let mask = if arg_opt!(ed, 1).is_some() {
        let Some(characters) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            function,
            1,
            "characters",
            "string",
        )?
        else {
            return Ok(());
        };
        let characters = characters.php_string_bytes().unwrap_or_default();
        let (mask, warnings) = php_charlist_mask(&characters);
        for warning in warnings {
            report_internal_diagnostic(eg, ed, 2, "Warning", &format!("{function}(): {warning}"))?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        explicit_mask = mask;
        &explicit_mask
    } else {
        &DEFAULT_TRIM_MASK
    };
    let bytes = string.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        trimmed_php_string_value(&string, &bytes, mask, trim_left, trim_right)
    );
}

fn fn_trim(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_trim_direction(ed, rv, eg, "trim", true, true)
}

fn fn_rtrim(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_trim_direction(ed, rv, eg, "rtrim", false, true)
}

fn fn_ltrim(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_trim_direction(ed, rv, eg, "ltrim", true, false)
}

fn fn_explode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_separator = arg!(ed, 0);
    let exact_string = arg!(ed, 1);
    let exact_limit = match arg_opt!(ed, 2) {
        None => Some(i64::MAX),
        Some(limit) if limit.value_type() == ValueType::Long => limit.as_long(),
        Some(_) => None,
    };
    if exact_separator.value_type() == ValueType::String
        && exact_string.value_type() == ValueType::String
        && let Some(limit) = exact_limit
    {
        let separator = exact_separator.php_string_bytes().unwrap_or_default();
        let string = exact_string.php_string_bytes().unwrap_or_default();
        if separator.is_empty() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "explode(): Argument #1 ($separator) must not be empty",
            ));
            return Ok(());
        }
        let result = explode_php_bytes(&separator, &string, limit, exact_string.is_binary_string());
        ret!(rv, Value::array(result));
    }

    let Some(separator) =
        typed_internal_string_value_argument_expected(ed, eg, "explode", 0, "separator", "string")?
    else {
        return Ok(());
    };
    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "explode", 1, "string", "string")?
    else {
        return Ok(());
    };
    let limit = if arg_opt!(ed, 2).is_some() {
        let Some(limit) = typed_internal_int_argument(ed, eg, "explode", 2, "limit")? else {
            return Ok(());
        };
        limit
    } else {
        i64::MAX
    };
    let separator_bytes = separator.php_string_bytes().unwrap_or_default();
    if separator_bytes.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "explode(): Argument #1 ($separator) must not be empty",
        ));
        return Ok(());
    }
    let string_bytes = string.php_string_bytes().unwrap_or_default();
    let result = explode_php_bytes(
        &separator_bytes,
        &string_bytes,
        limit,
        string.is_binary_string(),
    );
    ret!(rv, Value::array(result));
}

fn explode_php_bytes(separator: &[u8], string: &[u8], limit: i64, binary: bool) -> PhpArray {
    let mut arr = PhpArray::new();
    if limit >= 0 {
        let maximum_parts = usize::try_from(limit.max(1)).unwrap_or(usize::MAX);
        let mut start = 0usize;
        for position in memchr::memmem::find_iter(string, separator) {
            if arr.len().saturating_add(1) >= maximum_parts {
                break;
            }
            arr.push(php_byte_result(string[start..position].to_vec(), binary));
            start = position + separator.len();
        }
        arr.push(php_byte_result(string[start..].to_vec(), binary));
        return arr;
    }

    let positions = memchr::memmem::find_iter(string, separator).collect::<Vec<_>>();
    let retained = positions
        .len()
        .saturating_add(1)
        .saturating_sub(usize::try_from(limit.unsigned_abs()).unwrap_or(usize::MAX));
    if retained == 0 {
        return arr;
    }
    let mut start = 0usize;
    for index in 0..retained {
        let end = if index + 1 == retained {
            positions.get(index).copied().unwrap_or(string.len())
        } else {
            positions[index]
        };
        arr.push(php_byte_result(string[start..end].to_vec(), binary));
        if index < positions.len() {
            start = positions[index] + separator.len();
        }
    }
    arr
}

fn fn_ucwords(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let string = arg_str!(ed, 0);
    let separators = arg_opt!(ed, 1)
        .and_then(Value::as_str)
        .unwrap_or(" \t\r\n\u{000b}\u{000c}");
    let mut result = String::with_capacity(string.len());
    let mut start = true;
    for character in string.chars() {
        if start {
            result.extend(character.to_uppercase());
        } else {
            result.push(character);
        }
        start = separators.contains(character);
    }
    ret!(rv, Value::string(result));
}

fn fn_implode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    implode_or_join(ed, rv, eg, "implode")
}

fn fn_join(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    implode_or_join(ed, rv, eg, "join")
}

#[inline]
fn implode_or_join(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    let first = arg!(ed, 0).dereferenced();
    let second = arg!(ed, 1).dereferenced();
    if first.value_type() == ValueType::String
        && let Some(pieces) = second.as_array()
    {
        if !first.is_binary_string()
            && let Some(result) = implode_text_fast(first.as_str().unwrap_or_default(), pieces, eg)
        {
            ret!(rv, Value::string(result));
        }
        return implode_array(ed, rv, eg, first, pieces);
    }

    if second.value_type() == ValueType::Undef {
        if let Some(pieces) = first.as_array() {
            if let Some(result) = implode_text_fast("", pieces, eg) {
                ret!(rv, Value::string(result));
            }
            return implode_array(ed, rv, eg, &Value::string(String::new()), pieces);
        }
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given"
            ),
        ));
        return Ok(());
    }

    let glue = if first.value_type() == ValueType::Null && !internal_call_is_strict(ed) {
        report_internal_deprecation(
            eg,
            ed,
            &format!(
                "{function}(): Passing null to parameter #1 ($separator) of type array|string is deprecated"
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        Value::string(String::new())
    } else {
        let expected = if matches!(
            first.value_type(),
            ValueType::Object | ValueType::Closure | ValueType::Resource
        ) {
            "array|string"
        } else {
            "string"
        };
        let Some(glue) = typed_internal_string_value_argument_expected(
            ed,
            eg,
            function,
            0,
            "separator",
            expected,
        )?
        else {
            return Ok(());
        };
        glue
    };
    let Some(array) = second.as_array() else {
        if second.value_type() == ValueType::Null {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{function}(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given"
                ),
            ));
        } else {
            typed_internal_argument_error(eg, function, second, 2, "array", "?array");
        }
        return Ok(());
    };
    implode_array(ed, rv, eg, &glue, array)
}

#[inline]
fn implode_text_fast(glue: &str, pieces: &PhpArray, eg: &ExecutorGlobals) -> Option<String> {
    let glue_bytes = glue.len().saturating_mul(pieces.len().saturating_sub(1));
    let value_bytes = pieces.values().map(Value::echo_len_hint).sum::<usize>();
    let mut result = String::with_capacity(glue_bytes.saturating_add(value_bytes));
    for (index, value) in pieces.values().enumerate() {
        let value = value.dereferenced();
        if index > 0 {
            result.push_str(glue);
        }
        match value.value_type() {
            ValueType::String if !value.is_binary_string() => {
                result.push_str(value.as_str().unwrap_or_default());
            }
            ValueType::Array | ValueType::Object | ValueType::Closure | ValueType::String => {
                return None;
            }
            _ => value.append_echo_to_with_precision(&mut result, eg.precision),
        }
    }
    Some(result)
}

#[inline]
fn implode_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    glue: &Value,
    pieces: &PhpArray,
) -> Result<(), VmError> {
    let glue_bytes = glue.php_string_bytes().unwrap_or_default();
    let glue_size = glue_bytes
        .len()
        .saturating_mul(pieces.len().saturating_sub(1));
    let value_size = pieces
        .values()
        .map(|value| value.dereferenced().echo_len_hint())
        .sum::<usize>();
    let mut result = Vec::with_capacity(glue_size.saturating_add(value_size));
    let mut binary = glue.is_binary_string();
    for (index, value) in pieces.values().enumerate() {
        if index > 0 {
            result.extend_from_slice(&glue_bytes);
        }
        let Some(text) = replacement_item_text(ed, eg, value)? else {
            return Ok(());
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        binary |= text.binary;
        result.extend_from_slice(&text.bytes);
    }
    ret!(rv, php_byte_result(result, binary));
}

fn fn_str_repeat(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source_value = arg!(ed, 0);
    let binary = source_value.is_binary_string();
    let s = arg_str!(ed, 0);
    let source_bytes = if binary {
        source_value.php_string_bytes().unwrap_or_default()
    } else {
        Cow::Borrowed(s.as_bytes())
    };
    let times = arg_long!(ed, 1);
    if times < 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "str_repeat(): Argument #2 ($times) must be greater than or equal to 0",
        ));
        ret!(rv, Value::null());
    }
    let times = times as usize;
    let allocation_failure = |bytes: usize| {
        let (file, line) = internal_call_source(ed);
        VmError::Fatal(format!(
            "Allowed memory size of 134217728 bytes exhausted (tried to allocate {bytes} bytes) in {file} on line {line}"
        ))
    };
    let total_bytes = source_bytes
        .len()
        .checked_mul(times)
        .ok_or_else(|| allocation_failure(usize::MAX))?;
    let mut repeated = Vec::new();
    repeated
        .try_reserve_exact(total_bytes)
        .map_err(|_| allocation_failure(total_bytes))?;
    if total_bytes != 0 {
        repeated.extend_from_slice(&source_bytes);
        while repeated.len() < total_bytes {
            let remaining = total_bytes - repeated.len();
            let copy_len = repeated.len().min(remaining);
            repeated.extend_from_within(..copy_len);
        }
    }
    if binary {
        ret!(rv, php_byte_result(repeated, true));
    }
    // SAFETY: this branch is reachable only when `binary` is false, making
    // `source_bytes` exactly `s.as_bytes()`. The buffer consists exclusively
    // of complete copies of UTF-8 `s`; both its current and remaining lengths
    // are multiples of `s.len()`, so the final doubling cannot split a code
    // point.
    ret!(
        rv,
        Value::string(unsafe { String::from_utf8_unchecked(repeated) })
    );
}

fn fn_substr_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_haystack = arg!(ed, 0);
    let exact_needle = arg!(ed, 1);
    if exact_haystack.value_type() == ValueType::String
        && exact_needle.value_type() == ValueType::String
        && arg_opt!(ed, 2).is_none()
    {
        let haystack = exact_haystack.php_string_bytes().unwrap_or_default();
        let needle = exact_needle.php_string_bytes().unwrap_or_default();
        if needle.is_empty() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "substr_count(): Argument #2 ($needle) must not be empty",
            ));
            return Ok(());
        }
        let count = memchr::memmem::find_iter(&haystack, &needle).count() as i64;
        ret!(rv, Value::long(count));
    }

    let Some(haystack) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "substr_count",
        0,
        "haystack",
        "string",
    )?
    else {
        return Ok(());
    };
    let Some(needle) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "substr_count",
        1,
        "needle",
        "string",
    )?
    else {
        return Ok(());
    };
    let offset = if arg_opt!(ed, 2).is_some() {
        let Some(offset) = typed_internal_int_argument(ed, eg, "substr_count", 2, "offset")? else {
            return Ok(());
        };
        offset
    } else {
        0
    };
    let length = match arg_opt!(ed, 3) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(length) =
                typed_internal_int_argument_expected(ed, eg, "substr_count", 3, "length", "?int")?
            else {
                return Ok(());
            };
            Some(length)
        }
    };

    let haystack = haystack.php_string_bytes().unwrap_or_default();
    let needle = needle.php_string_bytes().unwrap_or_default();
    if needle.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_count(): Argument #2 ($needle) must not be empty",
        ));
        return Ok(());
    }
    let start = if offset < 0 {
        usize::try_from(offset.unsigned_abs())
            .ok()
            .and_then(|distance| haystack.len().checked_sub(distance))
    } else {
        usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= haystack.len())
    };
    let Some(start) = start else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_count(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
        return Ok(());
    };
    let end = match length {
        None => Some(haystack.len()),
        Some(length) if length >= 0 => usize::try_from(length)
            .ok()
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= haystack.len()),
        Some(length) => usize::try_from(length.unsigned_abs())
            .ok()
            .and_then(|distance| haystack.len().checked_sub(distance))
            .filter(|end| *end >= start),
    };
    let Some(end) = end else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_count(): Argument #4 ($length) must be contained in argument #1 ($haystack)",
        ));
        return Ok(());
    };
    let count = memchr::memmem::find_iter(&haystack[start..end], &needle).count() as i64;
    ret!(rv, Value::long(count));
}

fn fn_str_contains(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.contains(n.as_ref())));
}

fn fn_str_starts_with(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.starts_with(n.as_ref())));
}

fn fn_str_ends_with(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.ends_with(n.as_ref())));
}

const STR_PAD_LEFT: i64 = 0;
const STR_PAD_RIGHT: i64 = 1;
const STR_PAD_BOTH: i64 = 2;

fn fn_str_pad(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = typed_internal_string_argument(ed, eg, "str_pad", 0, "string")? else {
        return Ok(());
    };
    let Some(length) = typed_internal_int_argument(ed, eg, "str_pad", 1, "length")? else {
        return Ok(());
    };
    let pad = if arg_opt!(ed, 2).is_some() {
        let Some(pad) = typed_internal_string_argument(ed, eg, "str_pad", 2, "pad_string")? else {
            return Ok(());
        };
        pad
    } else {
        " ".to_string()
    };
    let pad_type = if arg_opt!(ed, 3).is_some() {
        let Some(pad_type) = typed_internal_int_argument(ed, eg, "str_pad", 3, "pad_type")? else {
            return Ok(());
        };
        pad_type
    } else {
        STR_PAD_RIGHT
    };

    let input_bytes = php_string_to_bytes(&input);
    if length <= input_bytes.len() as i64 {
        ret!(rv, Value::string(input));
    }
    let pad_bytes = php_string_to_bytes(&pad);
    if pad_bytes.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "str_pad(): Argument #3 ($pad_string) must not be empty",
        ));
        return Ok(());
    }
    if !matches!(pad_type, STR_PAD_LEFT | STR_PAD_RIGHT | STR_PAD_BOTH) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH",
        ));
        return Ok(());
    }

    let target_length = usize::try_from(length).unwrap_or(usize::MAX);
    let padding_length = target_length - input_bytes.len();
    let left_length = match pad_type {
        STR_PAD_LEFT => padding_length,
        STR_PAD_BOTH => padding_length / 2,
        _ => 0,
    };
    let right_length = padding_length - left_length;
    let mut output = Vec::new();
    if output.try_reserve_exact(target_length).is_err() {
        return Err(VmError::Fatal(
            "str_pad(): requested string length is too large".to_string(),
        ));
    }
    append_repeated_padding(&mut output, &pad_bytes, left_length);
    output.extend_from_slice(&input_bytes);
    append_repeated_padding(&mut output, &pad_bytes, right_length);
    ret!(rv, Value::string(bytes_to_php_string(&output)));
}

fn append_repeated_padding(output: &mut Vec<u8>, padding: &[u8], length: usize) {
    if length == 0 {
        return;
    }
    let start = output.len();
    let initial = length.min(padding.len());
    output.extend_from_slice(&padding[..initial]);
    let mut produced = initial;
    while produced < length {
        let copied = produced.min(length - produced);
        output.extend_from_within(start..start + copied);
        produced += copied;
    }
}

fn fn_str_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "str_split", 0, "string", "string")?
    else {
        return Ok(());
    };
    let length = match arg_opt!(ed, 1) {
        Some(_) => {
            let Some(length) = typed_internal_int_argument(ed, eg, "str_split", 1, "length")?
            else {
                return Ok(());
            };
            length
        }
        None => 1,
    };
    if length < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "str_split(): Argument #2 ($length) must be greater than 0",
        ));
        return Ok(());
    }

    let chunk = usize::try_from(length).unwrap_or(usize::MAX);
    let bytes = string.php_string_bytes().unwrap_or_default();
    let mut arr = PhpArray::with_packed_capacity(bytes.len().div_ceil(chunk));
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + chunk).min(bytes.len());
        let part = &bytes[i..end];
        let value = if string.is_binary_string() || !part.is_ascii() {
            Value::binary_string(part)
        } else {
            Value::string(String::from_utf8(part.to_vec()).expect("ASCII chunk is valid UTF-8"))
        };
        arr.push(value);
        i = end;
    }
    ret!(rv, Value::array(arr));
}

fn fn_ucfirst(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    if s.is_empty() {
        ret!(rv, Value::string(""));
    } else {
        // PHP ucfirst is ASCII-only
        let mut bytes = s.as_bytes().to_vec();
        bytes[0] = bytes[0].to_ascii_uppercase();
        ret!(
            rv,
            Value::string(unsafe { String::from_utf8_unchecked(bytes) })
        );
    }
}

fn fn_lcfirst(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    if s.is_empty() {
        ret!(rv, Value::string(""));
    } else {
        let mut bytes = s.as_bytes().to_vec();
        bytes[0] = bytes[0].to_ascii_lowercase();
        ret!(
            rv,
            Value::string(unsafe { String::from_utf8_unchecked(bytes) })
        );
    }
}

fn fn_levenshtein(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = arg_str!(ed, 0);
    let second = arg_str!(ed, 1);
    let insertion = arg_opt!(ed, 2).map_or(1, Value::to_long_val).max(0);
    let replacement = arg_opt!(ed, 3).map_or(1, Value::to_long_val).max(0);
    let deletion = arg_opt!(ed, 4).map_or(1, Value::to_long_val).max(0);
    let first = first.as_bytes();
    let second = second.as_bytes();

    let mut previous = Vec::with_capacity(second.len() + 1);
    previous.push(0i64);
    for index in 1..=second.len() {
        previous.push((index as i64).saturating_mul(insertion));
    }
    let mut current = vec![0i64; second.len() + 1];
    for (first_index, first_byte) in first.iter().enumerate() {
        current[0] = ((first_index + 1) as i64).saturating_mul(deletion);
        for (second_index, second_byte) in second.iter().enumerate() {
            let inserted = current[second_index].saturating_add(insertion);
            let deleted = previous[second_index + 1].saturating_add(deletion);
            let replaced = previous[second_index].saturating_add(if first_byte == second_byte {
                0
            } else {
                replacement
            });
            current[second_index + 1] = inserted.min(deleted).min(replaced);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    ret!(rv, Value::long(previous[second.len()]));
}

fn similar_text_score(first: &[u8], second: &[u8]) -> usize {
    let mut score = 0usize;
    let mut pending = vec![(0usize, first.len(), 0usize, second.len())];
    while let Some((first_start, first_end, second_start, second_end)) = pending.pop() {
        let mut longest = 0usize;
        let mut longest_first = first_start;
        let mut longest_second = second_start;
        for first_position in first_start..first_end {
            for second_position in second_start..second_end {
                let mut length = 0usize;
                while first_position + length < first_end
                    && second_position + length < second_end
                    && first[first_position + length] == second[second_position + length]
                {
                    length += 1;
                }
                if length > longest {
                    longest = length;
                    longest_first = first_position;
                    longest_second = second_position;
                }
            }
        }
        if longest == 0 {
            continue;
        }

        score += longest;
        if longest_first > first_start && longest_second > second_start {
            pending.push((first_start, longest_first, second_start, longest_second));
        }
        let first_after = longest_first + longest;
        let second_after = longest_second + longest;
        if first_after < first_end && second_after < second_end {
            pending.push((first_after, first_end, second_after, second_end));
        }
    }
    score
}

fn fn_similar_text(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(first) = typed_internal_string_argument(ed, eg, "similar_text", 0, "string1")? else {
        return Ok(());
    };
    let Some(second) = typed_internal_string_argument(ed, eg, "similar_text", 1, "string2")? else {
        return Ok(());
    };
    let first = php_string_to_bytes(&first);
    let second = php_string_to_bytes(&second);
    let score = similar_text_score(&first, &second);

    if arg_opt!(ed, 2).is_some() {
        let total_length = first.len().saturating_add(second.len());
        let percent = if total_length == 0 {
            0.0
        } else {
            score as f64 * 200.0 / total_length as f64
        };
        arg_mut!(ed, 2, Value::double(percent));
    }
    ret!(rv, Value::long(score as i64));
}

enum StrtokStep {
    Token(Vec<u8>),
    Exhausted,
    Invalidated,
}

fn strtok_step(state: &mut crate::runtime::StrtokState, delimiters: &[u8]) -> StrtokStep {
    if state.position >= state.input.len() {
        return StrtokStep::Exhausted;
    }

    let mut delimiter = [false; 256];
    for byte in delimiters {
        delimiter[*byte as usize] = true;
    }
    while state.position < state.input.len() && delimiter[state.input[state.position] as usize] {
        state.position += 1;
    }
    if state.position == state.input.len() {
        return StrtokStep::Invalidated;
    }

    let start = state.position;
    while state.position < state.input.len() && !delimiter[state.input[state.position] as usize] {
        state.position += 1;
    }
    let token = state.input[start..state.position].to_vec();
    if state.position < state.input.len() {
        state.position += 1;
    }
    StrtokStep::Token(token)
}

fn fn_strtok(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(first) = typed_internal_string_argument(ed, eg, "strtok", 0, "string")? else {
        return Ok(());
    };
    let second = arg_opt!(ed, 1);
    let starts_tokenization = second.is_some_and(|value| value.value_type() != ValueType::Null);
    let delimiters = if starts_tokenization {
        let Some(token) =
            typed_internal_string_argument_expected(ed, eg, "strtok", 1, "token", "?string")?
        else {
            return Ok(());
        };
        eg.string_utility_state
            .get_or_insert_with(|| Box::new(crate::runtime::StringUtilityState::default()))
            .strtok = Some(crate::runtime::StrtokState {
            input: php_string_to_bytes(&first),
            position: 0,
        });
        php_string_to_bytes(&token)
    } else {
        php_string_to_bytes(&first)
    };

    let step = eg
        .string_utility_state
        .as_mut()
        .and_then(|state| state.strtok.as_mut())
        .map(|state| strtok_step(state, &delimiters));
    let Some(step) = step else {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "strtok(): Both arguments must be provided when starting tokenization",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    };

    match step {
        StrtokStep::Token(token) => ret!(rv, Value::string(bytes_to_php_string(&token))),
        StrtokStep::Exhausted => ret!(rv, Value::bool(false)),
        StrtokStep::Invalidated => {
            if let Some(state) = eg.string_utility_state.as_mut() {
                state.strtok = None;
            }
            ret!(rv, Value::bool(false));
        }
    }
}

fn fn_str_shuffle(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = typed_internal_string_argument(ed, eg, "str_shuffle", 0, "string")? else {
        return Ok(());
    };
    let mut bytes = php_string_to_bytes(&string);
    shuffle_slice(eg, &mut bytes);
    ret!(rv, Value::string(bytes_to_php_string(&bytes)));
}

#[cfg(test)]
mod similar_text_tests {
    use super::similar_text_score;

    #[test]
    fn matches_public_examples_and_first_match_tie_breaking() {
        assert_eq!(similar_text_score(b"abcdefgh", b"efg"), 3);
        assert_eq!(similar_text_score(b"abcdefgh", b"mno"), 0);
        assert_eq!(similar_text_score(b"abcdefghcc", b"c"), 1);
        assert_eq!(similar_text_score(b"abcdefghabcdef", b"zzzzabcdefggg"), 7);
        assert_eq!(similar_text_score(b"bafoobar", b"barfoo"), 5);
        assert_eq!(similar_text_score(b"barfoo", b"bafoobar"), 3);
    }

    #[test]
    fn treats_inputs_as_bytes_and_handles_empty_regions() {
        assert_eq!(similar_text_score(b"a\0bc", b"\0b"), 2);
        assert_eq!(similar_text_score(&[0xc4], &[0xe4]), 0);
        assert_eq!(similar_text_score(b"", b""), 0);
        assert_eq!(similar_text_score(b"same", b"same"), 4);
    }
}

#[cfg(test)]
mod strtok_shuffle_tests {
    use std::collections::HashSet;

    use super::{StrtokStep, shuffle_slice, strtok_step};
    use crate::runtime::{ExecutorGlobals, StringUtilityState, StrtokState};

    fn token(step: StrtokStep) -> Option<Vec<u8>> {
        match step {
            StrtokStep::Token(value) => Some(value),
            StrtokStep::Exhausted | StrtokStep::Invalidated => None,
        }
    }

    #[test]
    fn tokenizer_uses_current_byte_delimiters_and_preserves_end_state() {
        let mut state = StrtokState {
            input: b",a,,b;c d".to_vec(),
            position: 0,
        };
        assert_eq!(token(strtok_step(&mut state, b",")), Some(b"a".to_vec()));
        assert_eq!(
            token(strtok_step(&mut state, b",")),
            Some(b"b;c d".to_vec())
        );
        assert!(matches!(
            strtok_step(&mut state, b","),
            StrtokStep::Exhausted
        ));

        let mut state = StrtokState {
            input: b"a,b;c d".to_vec(),
            position: 0,
        };
        assert_eq!(token(strtok_step(&mut state, b",")), Some(b"a".to_vec()));
        assert_eq!(token(strtok_step(&mut state, b";")), Some(b"b".to_vec()));
        assert_eq!(token(strtok_step(&mut state, b" ")), Some(b"c".to_vec()));
        assert_eq!(token(strtok_step(&mut state, b",")), Some(b"d".to_vec()));
    }

    #[test]
    fn tokenizer_distinguishes_exhausted_and_delimiter_only_suffixes() {
        let mut empty = StrtokState {
            input: Vec::new(),
            position: 0,
        };
        assert!(matches!(
            strtok_step(&mut empty, b","),
            StrtokStep::Exhausted
        ));

        let mut delimiters = StrtokState {
            input: b",,,".to_vec(),
            position: 0,
        };
        assert!(matches!(
            strtok_step(&mut delimiters, b","),
            StrtokStep::Invalidated
        ));

        let mut binary = StrtokState {
            input: vec![0x80, 0, 0x81, b',', b't', b'a', b'i', b'l'],
            position: 0,
        };
        assert_eq!(token(strtok_step(&mut binary, b"\0,")), Some(vec![0x80]));
        assert_eq!(token(strtok_step(&mut binary, b"\0,")), Some(vec![0x81]));
        assert_eq!(
            token(strtok_step(&mut binary, b"\0,")),
            Some(b"tail".to_vec())
        );
    }

    #[test]
    fn request_local_shuffle_reaches_every_four_byte_permutation() {
        let mut eg = ExecutorGlobals::new();
        eg.string_utility_state = Some(Box::new(StringUtilityState {
            strtok: None,
            shuffle_random: 1,
        }));
        let mut permutations = HashSet::new();
        for _ in 0..1_000 {
            let mut value = *b"abcd";
            shuffle_slice(&mut eg, &mut value);
            permutations.insert(value);
        }
        assert_eq!(permutations.len(), 24);
    }
}

fn count_chars_value(input: &[u8], mode: i64, binary: bool) -> Value {
    let counts = crate::string_byte_utilities::count_chars(input);
    if mode <= 2 {
        let mut result = PhpArray::new();
        for (byte, count) in counts.into_iter().enumerate() {
            if mode == 0 || (mode == 1 && count != 0) || (mode == 2 && count == 0) {
                result.set_int(byte as i64, Value::long(count as i64));
            }
        }
        return Value::array(result);
    }

    let bytes = counts
        .into_iter()
        .enumerate()
        .filter_map(|(byte, count)| {
            ((mode == 3 && count != 0) || (mode == 4 && count == 0)).then_some(byte as u8)
        })
        .collect();
    php_byte_result(bytes, binary)
}

fn fn_count_chars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_input = arg!(ed, 0);
    let exact_mode = arg_opt!(ed, 1);
    if exact_input.value_type() == ValueType::String
        && exact_mode.is_none_or(|mode| mode.value_type() == ValueType::Long)
    {
        let mode = exact_mode.map_or(0, |mode| mode.as_long().unwrap_or_default());
        if !(0..=4).contains(&mode) {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)",
            ));
            return Ok(());
        }
        let binary = exact_input.is_binary_string();
        let input = exact_input.php_string_bytes().unwrap_or_default();
        ret!(rv, count_chars_value(&input, mode, binary));
    }

    let Some(input) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "count_chars",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let mode = if arg_opt!(ed, 1).is_some() {
        let Some(mode) = typed_internal_int_argument(ed, eg, "count_chars", 1, "mode")? else {
            return Ok(());
        };
        mode
    } else {
        0
    };
    if !(0..=4).contains(&mode) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "count_chars(): Argument #2 ($mode) must be between 0 and 4 (inclusive)",
        ));
        return Ok(());
    }
    let binary = input.is_binary_string();
    let input = input.php_string_bytes().unwrap_or_default();
    ret!(rv, count_chars_value(&input, mode, binary));
}

fn fn_metaphone(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_input = arg!(ed, 0);
    let exact_limit = arg_opt!(ed, 1);
    if exact_input.value_type() == ValueType::String
        && exact_limit.is_none_or(|limit| limit.value_type() == ValueType::Long)
    {
        let limit = exact_limit.map_or(0, |limit| limit.as_long().unwrap_or_default());
        if limit < 0 {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0",
            ));
            return Ok(());
        }
        let input = exact_input.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            php_byte_result(
                crate::string_byte_utilities::metaphone(&input, limit as usize),
                false
            )
        );
    }

    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "metaphone", 0, "string", "string")?
    else {
        return Ok(());
    };
    let limit = if arg_opt!(ed, 1).is_some() {
        let Some(limit) = typed_internal_int_argument(ed, eg, "metaphone", 1, "max_phonemes")?
        else {
            return Ok(());
        };
        limit
    } else {
        0
    };
    if limit < 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0",
        ));
        return Ok(());
    }
    let input = input.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(
            crate::string_byte_utilities::metaphone(&input, limit as usize),
            false
        )
    );
}

fn fn_quotemeta(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg!(ed, 0);
    if input.value_type() == ValueType::String {
        let binary = input.is_binary_string();
        let input = input.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            php_byte_result(crate::string_byte_utilities::quotemeta(&input), binary)
        );
    }
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "quotemeta", 0, "string", "string")?
    else {
        return Ok(());
    };
    let binary = input.is_binary_string();
    let input = input.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(crate::string_byte_utilities::quotemeta(&input), binary)
    );
}

fn fn_soundex(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg!(ed, 0);
    if input.value_type() == ValueType::String {
        let input = input.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            Value::string(bytes_to_php_string(&crate::string_byte_utilities::soundex(
                &input
            )))
        );
    }
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "soundex", 0, "string", "string")?
    else {
        return Ok(());
    };
    let input = input.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        Value::string(bytes_to_php_string(&crate::string_byte_utilities::soundex(
            &input
        )))
    );
}

fn fn_str_rot13(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg!(ed, 0);
    if input.value_type() == ValueType::String {
        let binary = input.is_binary_string();
        let input = input.php_string_bytes().unwrap_or_default();
        ret!(
            rv,
            php_byte_result(crate::string_byte_utilities::str_rot13(&input), binary)
        );
    }
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "str_rot13", 0, "string", "string")?
    else {
        return Ok(());
    };
    let binary = input.is_binary_string();
    let input = input.php_string_bytes().unwrap_or_default();
    ret!(
        rv,
        php_byte_result(crate::string_byte_utilities::str_rot13(&input), binary)
    );
}

fn legacy_utf8_transform(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    transform: fn(&[u8]) -> Vec<u8>,
) -> Result<(), VmError> {
    let input = arg!(ed, 0);
    if input.value_type() == ValueType::String {
        let bytes = input.php_string_bytes().unwrap_or_default();
        if bytes.is_ascii() {
            ret!(rv, input.clone());
        }
        ret!(rv, php_byte_result(transform(&bytes), false));
    }
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, function, 0, "string", "string")?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    if bytes.is_ascii() {
        ret!(rv, input);
    }
    ret!(rv, php_byte_result(transform(&bytes), false));
}

fn fn_utf8_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    legacy_utf8_transform(
        ed,
        rv,
        eg,
        "utf8_encode",
        crate::string_byte_utilities::utf8_encode_latin1,
    )
}

fn fn_utf8_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    legacy_utf8_transform(
        ed,
        rv,
        eg,
        "utf8_decode",
        crate::string_byte_utilities::utf8_decode_latin1,
    )
}

fn fn_str_word_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(string) = typed_internal_string_value_argument_expected(
        ed,
        eg,
        "str_word_count",
        0,
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    let format = if arg_opt!(ed, 1).is_some() {
        let Some(format) = typed_internal_int_argument(ed, eg, "str_word_count", 1, "format")?
        else {
            return Ok(());
        };
        format
    } else {
        0
    };
    if !(0..=2).contains(&format) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "str_word_count(): Argument #2 ($format) must be a valid format value",
        ));
        return Ok(());
    }

    let characters = match arg_opt!(ed, 2) {
        None => None,
        Some(value) if value.dereferenced().value_type() == ValueType::Null => None,
        Some(_) => {
            let Some(characters) = typed_internal_string_value_argument_expected(
                ed,
                eg,
                "str_word_count",
                2,
                "characters",
                "?string",
            )?
            else {
                return Ok(());
            };
            Some(characters)
        }
    };
    let mut additional = [false; 256];
    if let Some(characters) = characters {
        let characters = characters.php_string_bytes().unwrap_or_default();
        let (mask, warnings) = php_charlist_mask(&characters);
        for warning in warnings {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("str_word_count(): {warning}"),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        additional = mask;
    }

    let binary = string.is_binary_string();
    let bytes = string.php_string_bytes().unwrap_or_default();
    if format == 0 {
        let count = crate::string_byte_utilities::str_word_count(&bytes, &additional);
        ret!(rv, Value::long(count as i64));
    }
    let ranges = crate::string_byte_utilities::str_word_ranges(&bytes, &additional);
    let mut result = PhpArray::new();
    for (start, end) in ranges {
        let word = php_byte_result(bytes[start..end].to_vec(), binary);
        if format == 1 {
            result.push(word);
        } else {
            result.set_int(start as i64, word);
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_wordwrap(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0);
    let exact_width = match arg_opt!(ed, 1) {
        None => Some(75),
        Some(width) if width.value_type() == ValueType::Long => width.as_long(),
        Some(_) => None,
    };
    let exact_line_break = arg_opt!(ed, 2);
    let exact_cut = match arg_opt!(ed, 3) {
        None => Some(false),
        Some(value) if value.value_type() == ValueType::True => Some(true),
        Some(value) if value.value_type() == ValueType::False => Some(false),
        Some(_) => None,
    };
    if exact_string.value_type() == ValueType::String
        && exact_line_break.is_none_or(|line_break| line_break.value_type() == ValueType::String)
        && let Some(width) = exact_width
        && let Some(cut) = exact_cut
    {
        let (break_bytes, break_binary) = match exact_line_break {
            Some(line_break) => (
                line_break.php_string_bytes().unwrap_or_default(),
                line_break.is_binary_string(),
            ),
            None => (std::borrow::Cow::Borrowed(&b"\n"[..]), false),
        };
        if break_bytes.is_empty() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "wordwrap(): Argument #3 ($break) must not be empty",
            ));
            return Ok(());
        }
        if width == 0 && cut {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0",
            ));
            return Ok(());
        }
        let input = exact_string.php_string_bytes().unwrap_or_default();
        let (result, inserted_break) =
            crate::string_byte_utilities::wordwrap(&input, width, &break_bytes, cut);
        if !inserted_break {
            ret!(rv, exact_string.clone());
        }
        ret!(
            rv,
            php_byte_result(
                result,
                exact_string.is_binary_string() || (inserted_break && break_binary),
            )
        );
    }

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "wordwrap", 0, "string", "string")?
    else {
        return Ok(());
    };
    let width = if arg_opt!(ed, 1).is_some() {
        let Some(width) = typed_internal_int_argument(ed, eg, "wordwrap", 1, "width")? else {
            return Ok(());
        };
        width
    } else {
        75
    };
    let line_break = if arg_opt!(ed, 2).is_some() {
        let Some(line_break) = typed_internal_string_value_argument_expected(
            ed, eg, "wordwrap", 2, "break", "string",
        )?
        else {
            return Ok(());
        };
        line_break
    } else {
        Value::string("\n")
    };
    let cut = if arg_opt!(ed, 3).is_some() {
        let Some(cut) = typed_internal_bool_argument(ed, eg, "wordwrap", 3, "cut_long_words")?
        else {
            return Ok(());
        };
        cut
    } else {
        false
    };
    let break_bytes = line_break.php_string_bytes().unwrap_or_default();
    if break_bytes.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "wordwrap(): Argument #3 ($break) must not be empty",
        ));
        return Ok(());
    }
    if width == 0 && cut {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0",
        ));
        return Ok(());
    }

    let input = string.php_string_bytes().unwrap_or_default();
    let (result, inserted_break) =
        crate::string_byte_utilities::wordwrap(&input, width, &break_bytes, cut);
    if !inserted_break {
        ret!(rv, string);
    }
    ret!(
        rv,
        php_byte_result(
            result,
            string.is_binary_string() || (inserted_break && line_break.is_binary_string()),
        )
    );
}

fn fn_nl2br(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let exact_string = arg!(ed, 0).dereferenced();
    let exact_xhtml = match arg_opt!(ed, 1) {
        None => Some(true),
        Some(value) => match value.dereferenced().value_type() {
            ValueType::True => Some(true),
            ValueType::False => Some(false),
            _ => None,
        },
    };
    if exact_string.value_type() == ValueType::String
        && !exact_string.is_binary_string()
        && exact_string.as_str().is_some_and(str::is_ascii)
        && let Some(use_xhtml) = exact_xhtml
    {
        let source = exact_string.as_str().unwrap_or_default();
        let bytes = source.as_bytes();
        let Some(first_newline) = memchr::memchr2(b'\r', b'\n', bytes) else {
            ret!(rv, Value::string(source.to_string()));
        };
        let line_break = if use_xhtml { "<br />" } else { "<br>" };
        let estimated_growth = line_break
            .len()
            .checked_mul(source.len().min(8))
            .and_then(|growth| source.len().checked_add(growth));
        let Some(estimated_capacity) = estimated_growth else {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "nl2br(): Failed to allocate result string",
            ));
            return Ok(());
        };
        let mut result = String::with_capacity(estimated_capacity);
        let mut copied = 0usize;
        let mut newline = first_newline;
        loop {
            let newline_length = php_newline_length(bytes, newline);
            result.push_str(&source[copied..newline]);
            result.push_str(line_break);
            result.push_str(&source[newline..newline + newline_length]);
            copied = newline + newline_length;
            let Some(relative) = memchr::memchr2(b'\r', b'\n', &bytes[copied..]) else {
                break;
            };
            newline = copied + relative;
        }
        result.push_str(&source[copied..]);
        ret!(rv, Value::string(result));
    }

    let Some(string) =
        typed_internal_string_value_argument_expected(ed, eg, "nl2br", 0, "string", "string")?
    else {
        return Ok(());
    };
    let use_xhtml = if arg_opt!(ed, 1).is_some() {
        let Some(use_xhtml) = typed_internal_bool_argument(ed, eg, "nl2br", 1, "use_xhtml")? else {
            return Ok(());
        };
        use_xhtml
    } else {
        true
    };

    let source = string.php_string_bytes().unwrap_or_default();
    let line_break: &[u8] = if use_xhtml { b"<br />" } else { b"<br>" };
    let mut newline_count = 0usize;
    let mut position = 0usize;
    while let Some(relative) = memchr::memchr2(b'\r', b'\n', &source[position..]) {
        let newline = position + relative;
        newline_count = newline_count.saturating_add(1);
        position = newline + php_newline_length(&source, newline);
    }

    let result_length = line_break
        .len()
        .checked_mul(newline_count)
        .and_then(|growth| source.len().checked_add(growth));
    let Some(result_length) = result_length else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "nl2br(): Failed to allocate result string",
        ));
        return Ok(());
    };
    let mut result = Vec::new();
    if result.try_reserve_exact(result_length).is_err() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "nl2br(): Failed to allocate result string",
        ));
        return Ok(());
    }
    position = 0;
    let mut copied = 0usize;
    while let Some(relative) = memchr::memchr2(b'\r', b'\n', &source[position..]) {
        let newline = position + relative;
        let newline_length = php_newline_length(&source, newline);
        result.extend_from_slice(&source[copied..newline]);
        result.extend_from_slice(line_break);
        result.extend_from_slice(&source[newline..newline + newline_length]);
        copied = newline + newline_length;
        position = copied;
    }
    result.extend_from_slice(&source[copied..]);

    let value = if string.is_binary_string() || !result.is_ascii() {
        Value::binary_string(&result)
    } else {
        Value::string(String::from_utf8(result).expect("ASCII nl2br result is valid UTF-8"))
    };
    ret!(rv, value);
}

#[inline]
fn php_newline_length(source: &[u8], position: usize) -> usize {
    usize::from(
        source
            .get(position + 1)
            .is_some_and(|next| *next != source[position] && matches!(*next, b'\r' | b'\n')),
    ) + 1
}

fn fn_strrev(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    // The current Value string backend is UTF-8-backed, so retain valid
    // internal strings by reversing represented characters. Exact reversal
    // of arbitrary PHP binary strings requires the planned byte-string value
    // representation and remains outside this checkpoint.
    let reversed: String = s.chars().rev().collect();
    ret!(rv, Value::string(reversed));
}

fn fn_number_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let num = arg_float!(ed, 0);
    let decimals = match arg_opt!(ed, 1) {
        Some(v) => v.to_long_val().max(0) as usize,
        None => 0,
    };
    let dec_point = arg_opt!(ed, 2).and_then(Value::as_str).unwrap_or(".");
    let thousands_sep = arg_opt!(ed, 3).and_then(Value::as_str).unwrap_or(",");

    // Format and group in one owned buffer. Inserting separators from right
    // to left keeps all yet-to-be-used byte positions stable and avoids the
    // quadratic front insertion plus intermediate Strings of the old path.
    let mut result = String::with_capacity(decimals.saturating_add(32));
    let _ = write!(&mut result, "{:.prec$}", num, prec = decimals);
    let decimal_position = result.find('.');
    let integer_end = decimal_position.unwrap_or(result.len());
    let digits_start = usize::from(result.as_bytes().first() == Some(&b'-'));
    let digit_count = integer_end.saturating_sub(digits_start);
    let separator_count = digit_count.saturating_sub(1) / 3;
    let decimal_growth = decimal_position
        .map(|_| dec_point.len().saturating_sub(1))
        .unwrap_or(0);
    result.reserve(
        separator_count
            .saturating_mul(thousands_sep.len())
            .saturating_add(decimal_growth),
    );

    if let Some(position) = decimal_position {
        result.replace_range(position..position + 1, dec_point);
    }
    let mut separator_position = integer_end;
    while separator_position > digits_start + 3 {
        separator_position -= 3;
        result.insert_str(separator_position, thousands_sep);
    }

    ret!(rv, Value::string(result));
}

#[inline(always)]
fn direct_ord(args: &[Value]) -> Result<Value, VmError> {
    let byte = if let Some(bytes) = args.first().and_then(Value::php_string_bytes) {
        bytes.first().copied().unwrap_or(0)
    } else {
        direct_arg_str(args, 0)
            .as_bytes()
            .first()
            .copied()
            .unwrap_or(0)
    };
    Ok(Value::long(i64::from(byte)))
}

#[inline(always)]
pub(crate) fn try_direct_ord_string(argument: &Value) -> Option<(u8, usize)> {
    let bytes = argument.php_string_bytes()?;
    Some((bytes.first().copied().unwrap_or(0), bytes.len()))
}

fn fn_ord(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(character) =
        typed_internal_string_value_argument_expected(ed, eg, "ord", 0, "character", "string")?
    else {
        return Ok(());
    };
    let bytes = character.php_string_bytes().unwrap_or_default();
    if bytes.is_empty() {
        report_internal_deprecation(eg, ed, "ord(): Providing an empty string is deprecated")?;
    } else if bytes.len() != 1 {
        report_internal_deprecation(
            eg,
            ed,
            "ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead",
        )?;
    }
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(
        rv,
        Value::long(i64::from(bytes.first().copied().unwrap_or(0)))
    );
}

fn fn_chr(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(codepoint) = typed_internal_int_argument(ed, eg, "chr", 0, "codepoint")? else {
        return Ok(());
    };
    if !(0..=255).contains(&codepoint) {
        report_internal_deprecation(
            eg,
            ed,
            "chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::binary_string(&[(codepoint & 0xff) as u8]));
}

fn fn_bin2hex(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let input = arg_str!(ed, 0);
    let bytes = php_string_to_bytes(&input);
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    ret!(rv, Value::string(output));
}

fn fn_random_bytes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let length = arg_long!(ed, 0);
    if length <= 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "random_bytes(): Argument #1 ($length) must be greater than 0",
        ));
        return Ok(());
    }
    let length = usize::try_from(length).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(length).is_err() {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "random_bytes(): Unable to allocate the requested buffer",
        ));
        return Ok(());
    }
    bytes.resize(length, 0);
    let read_result =
        std::fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut bytes));
    if read_result.is_err() {
        eg.exception = Some(crate::value::make_error_value(
            "RuntimeException",
            "random_bytes(): Unable to read from the system random source",
        ));
        return Ok(());
    }
    ret!(rv, Value::string(bytes_to_php_string(&bytes)));
}

fn fn_hex2bin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "hex2bin", 0, "string", "string")?
    else {
        return Ok(());
    };
    let bytes = input.php_string_bytes().unwrap_or_default();
    let invalid_message = if bytes.len() % 2 != 0 {
        Some("hex2bin(): Hexadecimal input string must have an even length")
    } else if bytes.iter().any(|byte| !byte.is_ascii_hexdigit()) {
        Some("hex2bin(): Input string must be hexadecimal string")
    } else {
        None
    };
    if let Some(message) = invalid_message {
        report_internal_diagnostic(eg, ed, 2, "Warning", message)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(false));
    }

    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = decode_hex_nibble(pair[0]);
        let low = decode_hex_nibble(pair[1]);
        output.push((high << 4) | low);
    }
    ret!(rv, php_byte_result(output, false));
}

fn emit_pack_warnings(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    warnings: Vec<String>,
) -> Result<bool, VmError> {
    for warning in warnings {
        report_internal_diagnostic(eg, ed, 2, "Warning", &warning)?;
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn fn_pack(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let format = arg_str!(ed, 0);
    let values = arg!(ed, 1).as_array();
    let outcome = pack::pack_values(&format, values.as_deref());
    if !emit_pack_warnings(ed, eg, outcome.warnings)? {
        return Ok(());
    }
    match outcome.value {
        Ok(bytes) => ret!(rv, Value::binary_string(&bytes)),
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("ValueError", &message));
            Ok(())
        }
    }
}

fn fn_unpack(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if internal_call_is_strict(ed) {
        for (index, parameter, expected, matches) in [
            (
                0,
                "format",
                "string",
                arg!(ed, 0).dereferenced().value_type() == ValueType::String,
            ),
            (
                1,
                "string",
                "string",
                arg!(ed, 1).dereferenced().value_type() == ValueType::String,
            ),
            (
                2,
                "offset",
                "int",
                arg_opt!(ed, 2)
                    .is_none_or(|value| value.dereferenced().value_type() == ValueType::Long),
            ),
        ] {
            if !matches {
                let value = arg!(ed, index).dereferenced();
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "unpack(): Argument #{} (${parameter}) must be of type {expected}, {} given",
                        index + 1,
                        value.type_name()
                    ),
                ));
                return Ok(());
            }
        }
    }
    let format = arg_str!(ed, 0);
    let data = arg_str!(ed, 1);
    let bytes = php_string_to_bytes(&data);
    let offset = arg_opt!(ed, 2).map_or(0, explicit_long_conversion);
    if offset < 0 || usize::try_from(offset).map_or(true, |offset| offset > bytes.len()) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "unpack(): Argument #3 ($offset) must be contained in argument #2 ($data)",
        ));
        return Ok(());
    }
    let outcome = pack::unpack_values(&format, &bytes, offset as usize);
    if !emit_pack_warnings(ed, eg, outcome.warnings)? {
        return Ok(());
    }
    match outcome.value {
        Ok(Some(array)) => ret!(rv, Value::array(array)),
        Ok(None) => ret!(rv, Value::bool(false)),
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("ValueError", &message));
            Ok(())
        }
    }
}

#[inline]
fn decode_hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("hex input was validated before decoding"),
    }
}

#[derive(Clone, Copy)]
enum SprintfCall {
    Variadic,
    Array,
}

#[derive(Default)]
struct SprintfFlags {
    left: bool,
    plus: bool,
    zero: bool,
    pad: Option<u8>,
}

enum SprintfOutput {
    Text(String),
    Bytes(Vec<u8>),
}

impl SprintfOutput {
    fn len(&self) -> usize {
        match self {
            Self::Text(output) => output.len(),
            Self::Bytes(output) => output.len(),
        }
    }

    fn write_to(&self, eg: &ExecutorGlobals) {
        match self {
            Self::Text(output) => eg.write_output(output.as_bytes()),
            Self::Bytes(output) => eg.write_output(output),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Text(output) => Value::string(output),
            Self::Bytes(output) => Value::binary_string(&output),
        }
    }
}

fn sprintf_type_error(
    eg: &mut ExecutorGlobals,
    function: &str,
    index: usize,
    name: &str,
    expected: &str,
    value: &Value,
) {
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{} (${name}) must be of type {expected}, {} given",
            index + 1,
            match value.dereferenced().value_type() {
                ValueType::True => "true".into(),
                ValueType::False => "false".into(),
                _ => value.dereferenced().diagnostic_type_name(),
            }
        ),
    ));
}

fn sprintf_format_argument<'a>(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<Cow<'a, [u8]>>, VmError> {
    let value = arg!(ed, 0).dereferenced();
    if value.value_type() == ValueType::String {
        return Ok(value.php_string_bytes());
    }
    if internal_call_is_strict(ed)
        || matches!(
            value.value_type(),
            ValueType::Array | ValueType::Closure | ValueType::Resource
        )
    {
        sprintf_type_error(eg, function, 0, "format", "string", value);
        return Ok(None);
    }
    let value = value.clone();
    let rendered = internal_value_to_string(ed, eg, &value)?;
    if rendered.is_none() {
        pin_sprintf_conversion_error_to_caller(ed, eg);
    }
    Ok(rendered.map(|rendered| Cow::Owned(rendered.into_bytes())))
}

fn fn_sprintf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(format) = sprintf_format_argument(ed, eg, "sprintf")? else {
        return Ok(());
    };
    let args = arg!(ed, 1).as_array();
    let Some(result) = format_sprintf_values(
        ed,
        eg,
        &format,
        args.as_deref(),
        SprintfCall::Variadic,
        "sprintf",
    )?
    else {
        return Ok(());
    };
    ret!(rv, result.into_value());
}

fn sprintf_array_argument<'a>(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<&'a PhpArray> {
    let value = arg!(ed, 1);
    let Some(values) = value.as_array() else {
        sprintf_type_error(eg, function, 1, "values", "array", value);
        return None;
    };
    Some(values)
}

fn fn_vsprintf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(format) = sprintf_format_argument(ed, eg, "vsprintf")? else {
        return Ok(());
    };
    let Some(args) = sprintf_array_argument(ed, eg, "vsprintf") else {
        return Ok(());
    };
    let Some(result) =
        format_sprintf_values(ed, eg, &format, Some(args), SprintfCall::Array, "vsprintf")?
    else {
        return Ok(());
    };
    ret!(rv, result.into_value());
}

fn fn_printf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(format) = sprintf_format_argument(ed, eg, "printf")? else {
        return Ok(());
    };
    let args = arg!(ed, 1).as_array();
    let Some(result) = format_sprintf_values(
        ed,
        eg,
        &format,
        args.as_deref(),
        SprintfCall::Variadic,
        "printf",
    )?
    else {
        return Ok(());
    };
    let length = result.len() as i64;
    result.write_to(eg);
    ret!(rv, Value::long(length));
}

fn fn_vprintf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(format) = sprintf_format_argument(ed, eg, "vprintf")? else {
        return Ok(());
    };
    let Some(args) = sprintf_array_argument(ed, eg, "vprintf") else {
        return Ok(());
    };
    let Some(result) =
        format_sprintf_values(ed, eg, &format, Some(args), SprintfCall::Array, "vprintf")?
    else {
        return Ok(());
    };
    let length = result.len() as i64;
    result.write_to(eg);
    ret!(rv, Value::long(length));
}

#[inline]
fn parse_sprintf_decimal(bytes: &[u8], index: &mut usize) -> Option<usize> {
    let start = *index;
    let mut value = 0usize;
    while let Some(digit @ b'0'..=b'9') = bytes.get(*index).copied() {
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'));
        *index += 1;
    }
    (*index > start).then_some(value)
}

const SPRINTF_POSITION_ERROR: &str =
    "Argument number specifier must be greater than zero and less than 2147483647";

#[inline]
fn normalize_sprintf_position(number: Option<usize>) -> Result<usize, ()> {
    match number {
        Some(number) if number > 0 && number < i32::MAX as usize => Ok(number - 1),
        _ => Err(()),
    }
}

fn sprintf_value_error(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        message.as_ref(),
    ));
}

fn pin_sprintf_conversion_error_to_caller(ed: *mut ExecuteData, eg: &mut ExecutorGlobals) {
    let Some(exception) = eg.exception.as_ref() else {
        return;
    };
    let missing_origin = exception.as_object().is_some_and(|object| {
        object
            .get_property("file")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    });
    if !missing_origin {
        return;
    }
    let (file, line) = internal_call_source(ed);
    if file.is_empty() || line == 0 {
        return;
    }
    if let Some(mut object) = exception.as_object_mut() {
        object.set_property("file", Value::string(file));
        object.set_property("line", Value::long(line as i64));
    }
}

fn take_sprintf_argument(
    args: Option<&PhpArray>,
    next: &mut usize,
    position: Option<usize>,
    call: SprintfCall,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let index = position.unwrap_or_else(|| {
        let index = *next;
        *next += 1;
        index
    });
    if let Some(value) = args.and_then(|args| args.get_value_at(index)) {
        return Some(value.dereferenced().clone());
    }
    let count = args.map_or(0, PhpArray::len);
    match call {
        SprintfCall::Variadic => {
            eg.exception = Some(crate::value::make_error_value(
                "ArgumentCountError",
                &format!("{} arguments are required, {} given", index + 2, count + 1),
            ));
        }
        SprintfCall::Array => sprintf_value_error(
            eg,
            format!(
                "The arguments array must contain {} items, {} given",
                index + 1,
                count
            ),
        ),
    }
    None
}

fn parse_sprintf_position(
    bytes: &[u8],
    index: &mut usize,
    eg: &mut ExecutorGlobals,
) -> Option<Result<Option<usize>, ()>> {
    let start = *index;
    let number = parse_sprintf_decimal(bytes, index);
    if bytes.get(*index) != Some(&b'$') {
        *index = start;
        return Some(Ok(None));
    }
    *index += 1;
    match normalize_sprintf_position(number) {
        Ok(position) => Some(Ok(Some(position))),
        Err(()) => {
            sprintf_value_error(eg, SPRINTF_POSITION_ERROR);
            Some(Err(()))
        }
    }
}

fn sprintf_numeric_long(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<i64>, VmError> {
    let number = explicit_long_conversion(value);
    if let Some(message) = explicit_numeric_cast_warning(value, ExplicitNumericCastTarget::Int) {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(number))
}

fn sprintf_numeric_float(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<f64>, VmError> {
    let number = explicit_float_conversion(value);
    if let Some(message) = explicit_numeric_cast_warning(value, ExplicitNumericCastTarget::Float) {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(number))
}

fn add_sprintf_sign(mut rendered: String, flags: &SprintfFlags, nonnegative: bool) -> String {
    if flags.plus && nonnegative {
        rendered.insert(0, '+');
    }
    rendered
}

fn normalize_sprintf_exponent(mut rendered: String, upper: bool) -> String {
    if upper {
        rendered = rendered.replace('e', "E");
    }
    let marker = if upper { 'E' } else { 'e' };
    if let Some(position) = rendered.find(marker) {
        let sign = position + 1;
        if !matches!(rendered.as_bytes().get(sign), Some(b'+' | b'-')) {
            rendered.insert(sign, '+');
        }
        let digits = sign + usize::from(matches!(rendered.as_bytes().get(sign), Some(b'+' | b'-')));
        while rendered.len() > digits + 1 && rendered.as_bytes().get(digits) == Some(&b'0') {
            rendered.remove(digits);
        }
    }
    rendered
}

fn trim_sprintf_fraction(mut rendered: String) -> String {
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

fn render_sprintf_general(number: f64, precision: i64, upper: bool) -> String {
    if !number.is_finite() {
        return if number.is_nan() {
            "NaN".to_string()
        } else if number.is_sign_negative() {
            "-INF".to_string()
        } else {
            "INF".to_string()
        };
    }
    if number == 0.0 {
        return if number.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    let exponent = number.abs().log10().floor() as i32;
    if precision < 0 {
        if exponent < -4 || exponent >= 17 {
            return normalize_sprintf_exponent(format!("{number:e}"), upper);
        }
        return number.to_string();
    }
    let significant = usize::try_from(precision.max(1)).unwrap_or(usize::MAX);
    if exponent < -4 || exponent >= significant as i32 {
        let decimals = significant.saturating_sub(1);
        let mut rendered = format!("{number:.decimals$e}");
        if decimals == 0
            && let Some(position) = rendered.find('e')
        {
            rendered.insert_str(position, ".0");
        } else if let Some(position) = rendered.find('e') {
            let exponent_part = rendered.split_off(position);
            rendered = trim_sprintf_fraction(rendered);
            rendered.push_str(&exponent_part);
        }
        return normalize_sprintf_exponent(rendered, upper);
    }
    let decimals = (significant as i32 - exponent - 1).max(0) as usize;
    trim_sprintf_fraction(format!("{number:.decimals$}"))
}

fn truncate_sprintf_string(rendered: &mut String, precision: usize) {
    if rendered.len() <= precision {
        return;
    }
    let mut end = precision;
    while end != 0 && !rendered.is_char_boundary(end) {
        end -= 1;
    }
    rendered.truncate(end);
}

fn render_sprintf_value(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
    specifier: u8,
    precision: Option<i64>,
    flags: &SprintfFlags,
) -> Result<Option<Vec<u8>>, VmError> {
    let rendered = match specifier {
        b's' => {
            if let Some(bytes) = value.php_string_bytes() {
                let mut rendered = bytes.into_owned();
                if let Some(precision) = precision {
                    rendered.truncate(rendered.len().min(precision.max(0) as usize));
                }
                return Ok(Some(rendered));
            }
            let Some(mut rendered) = internal_value_to_string(ed, eg, value)? else {
                pin_sprintf_conversion_error_to_caller(ed, eg);
                return Ok(None);
            };
            if let Some(precision) = precision {
                truncate_sprintf_string(&mut rendered, precision.max(0) as usize);
            }
            rendered.into_bytes()
        }
        b'c' => {
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            vec![(number & 0xff) as u8]
        }
        b'd' => {
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            add_sprintf_sign(number.to_string(), flags, number >= 0).into_bytes()
        }
        b'u' => {
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            (number as u64).to_string().into_bytes()
        }
        b'b' => {
            if precision.is_some() {
                return Ok(Some(Vec::new()));
            }
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            format!("{number:b}").into_bytes()
        }
        b'o' => {
            if precision.is_some() {
                return Ok(Some(Vec::new()));
            }
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            format!("{number:o}").into_bytes()
        }
        b'x' => {
            if precision.is_some() {
                return Ok(Some(Vec::new()));
            }
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            format!("{number:x}").into_bytes()
        }
        b'X' => {
            if precision.is_some() {
                return Ok(Some(Vec::new()));
            }
            let Some(number) = sprintf_numeric_long(ed, eg, value)? else {
                return Ok(None);
            };
            format!("{number:X}").into_bytes()
        }
        b'f' | b'F' => {
            let Some(number) = sprintf_numeric_float(ed, eg, value)? else {
                return Ok(None);
            };
            let precision = usize::try_from(precision.unwrap_or(6).max(0)).unwrap_or(usize::MAX);
            let rendered = if number.is_nan() {
                "NaN".to_string()
            } else if number == f64::INFINITY {
                "INF".to_string()
            } else if number == f64::NEG_INFINITY {
                "-INF".to_string()
            } else {
                format!("{number:.precision$}")
            };
            add_sprintf_sign(rendered, flags, number >= 0.0).into_bytes()
        }
        b'e' | b'E' => {
            let Some(number) = sprintf_numeric_float(ed, eg, value)? else {
                return Ok(None);
            };
            let precision = usize::try_from(precision.unwrap_or(6).max(0)).unwrap_or(usize::MAX);
            let rendered = if number.is_nan() {
                "NaN".to_string()
            } else if number == f64::INFINITY {
                "INF".to_string()
            } else if number == f64::NEG_INFINITY {
                "-INF".to_string()
            } else {
                normalize_sprintf_exponent(format!("{number:.precision$e}"), specifier == b'E')
            };
            add_sprintf_sign(rendered, flags, number >= 0.0).into_bytes()
        }
        b'g' | b'G' | b'h' | b'H' => {
            let Some(number) = sprintf_numeric_float(ed, eg, value)? else {
                return Ok(None);
            };
            add_sprintf_sign(
                render_sprintf_general(
                    number,
                    precision.unwrap_or(6),
                    matches!(specifier, b'G' | b'H'),
                ),
                flags,
                number >= 0.0,
            )
            .into_bytes()
        }
        b'%' => vec![b'%'],
        _ => unreachable!("validated sprintf specifier"),
    };
    Ok(Some(rendered))
}

fn apply_sprintf_width(
    mut rendered: Vec<u8>,
    width: usize,
    flags: &SprintfFlags,
    specifier: u8,
) -> Vec<u8> {
    if specifier == b'c' {
        return rendered;
    }
    let padding = width.saturating_sub(rendered.len());
    if padding == 0 {
        return rendered;
    }
    let float = matches!(
        specifier,
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H'
    );
    let string = matches!(specifier, b's' | b'%');
    let pad = flags
        .pad
        .unwrap_or(if flags.zero && (!flags.left || float || string) {
            b'0'
        } else {
            b' '
        });
    if flags.left {
        rendered.extend(std::iter::repeat_n(pad, padding));
        return rendered;
    }
    if pad == b'0' && !string && matches!(rendered.first(), Some(b'+' | b'-')) {
        let sign = rendered.remove(0);
        let mut padded = Vec::with_capacity(width);
        padded.push(sign);
        padded.extend(std::iter::repeat_n(pad, padding));
        padded.extend_from_slice(&rendered);
        return padded;
    }
    let mut padded = Vec::with_capacity(width);
    padded.extend(std::iter::repeat_n(pad, padding));
    padded.extend_from_slice(&rendered);
    padded
}

fn parse_sprintf_star_argument(
    bytes: &[u8],
    index: &mut usize,
    args: Option<&PhpArray>,
    next: &mut usize,
    call: SprintfCall,
    label: &str,
    eg: &mut ExecutorGlobals,
) -> Option<i64> {
    *index += 1;
    let position = match parse_sprintf_position(bytes, index, eg)? {
        Ok(position) => position,
        Err(()) => return None,
    };
    let value = take_sprintf_argument(args, next, position, call, eg)?;
    if value.value_type() != ValueType::Long {
        sprintf_value_error(eg, format!("{label} must be an integer"));
        return None;
    }
    value.as_long()
}

fn count_sprintf_arguments(format: &[u8]) -> Result<usize, ()> {
    fn record(position: Option<usize>, next: &mut usize, required: &mut usize) {
        let index = position.unwrap_or_else(|| {
            let index = *next;
            *next = next.saturating_add(1);
            index
        });
        *required = (*required).max(index.saturating_add(1));
    }

    fn position(format: &[u8], index: &mut usize) -> Result<Option<usize>, ()> {
        let start = *index;
        let number = parse_sprintf_decimal(format, index);
        if format.get(*index) != Some(&b'$') {
            *index = start;
            return Ok(None);
        }
        *index += 1;
        normalize_sprintf_position(number).map(Some)
    }

    let mut index = 0usize;
    let mut next = 0usize;
    let mut required = 0usize;
    while index < format.len() {
        if format[index] != b'%' {
            index += 1;
            continue;
        }
        index += 1;
        if format.get(index) == Some(&b'%') {
            index += 1;
            continue;
        }
        let value_position = position(format, &mut index)?;
        loop {
            match format.get(index).copied() {
                Some(b'-' | b'+' | b' ' | b'0') => index += 1,
                Some(b'\'') => index = (index + 2).min(format.len()),
                _ => break,
            }
        }
        if format.get(index) == Some(&b'*') {
            index += 1;
            let width_position = position(format, &mut index)?;
            record(width_position, &mut next, &mut required);
        } else {
            let _ = parse_sprintf_decimal(format, &mut index);
        }
        if format.get(index) == Some(&b'.') {
            index += 1;
            if format.get(index) == Some(&b'*') {
                index += 1;
                let precision_position = position(format, &mut index)?;
                record(precision_position, &mut next, &mut required);
            } else {
                let _ = parse_sprintf_decimal(format, &mut index);
            }
        }
        if format.get(index) == Some(&b'l') {
            index += 1;
        }
        index = (index + 1).min(format.len());
        record(value_position, &mut next, &mut required);
    }
    Ok(required)
}

#[cfg(test)]
mod sprintf_position_tests {
    use super::count_sprintf_arguments;

    #[test]
    fn positional_count_accepts_the_documented_amd64_range() {
        assert_eq!(count_sprintf_arguments(b"%1$s"), Ok(1));
        assert_eq!(count_sprintf_arguments(b"%2147483646$s"), Ok(2_147_483_646));

        for position in 1..=128 {
            let format = format!("%{position}$s");
            assert_eq!(count_sprintf_arguments(format.as_bytes()), Ok(position));
        }
    }

    #[test]
    fn positional_count_rejects_missing_zero_limit_and_decimal_overflow() {
        for format in [
            "%$s",
            "%0$s",
            "%2147483647$s",
            "%2147483648$s",
            "%999999999999999999999999999999999999$s",
            "%3$s %2147483648$s",
            "%*2147483647$s",
            "%.*999999999999999999999999999999$s",
        ] {
            assert_eq!(
                count_sprintf_arguments(format.as_bytes()),
                Err(()),
                "{format}"
            );
        }
    }
}

fn check_sprintf_argument_count(
    format: &[u8],
    args: Option<&PhpArray>,
    call: SprintfCall,
    eg: &mut ExecutorGlobals,
) -> bool {
    let required = match count_sprintf_arguments(format) {
        Ok(required) => required,
        Err(()) => {
            sprintf_value_error(eg, SPRINTF_POSITION_ERROR);
            return false;
        }
    };
    let count = args.map_or(0, PhpArray::len);
    if count >= required {
        return true;
    }
    match call {
        SprintfCall::Variadic => {
            eg.exception = Some(crate::value::make_error_value(
                "ArgumentCountError",
                &format!(
                    "{} arguments are required, {} given",
                    required + 1,
                    count + 1
                ),
            ));
        }
        SprintfCall::Array => sprintf_value_error(
            eg,
            format!("The arguments array must contain {required} items, {count} given"),
        ),
    }
    false
}

#[inline]
fn try_format_sprintf_simple(
    format: &[u8],
    args: Option<&PhpArray>,
    call: SprintfCall,
    precision: i32,
    eg: &mut ExecutorGlobals,
) -> Option<String> {
    let format_text = str::from_utf8(format).ok()?;
    let count = args.map_or(0, PhpArray::len);
    let mut output = String::with_capacity(format.len().saturating_add(count * 8));
    let mut index = 0usize;
    let mut literal = 0usize;
    let mut required = 0usize;
    let mut missing = false;
    while index < format.len() {
        if format[index] != b'%' {
            index += 1;
            continue;
        }
        output.push_str(&format_text[literal..index]);
        let specifier = *format.get(index + 1)?;
        if specifier == b'%' {
            output.push('%');
            index += 2;
            literal = index;
            continue;
        }
        if !matches!(
            specifier,
            b's' | b'd' | b'u' | b'f' | b'F' | b'x' | b'X' | b'o' | b'b'
        ) {
            return None;
        }
        let argument_index = required;
        required += 1;
        let Some(value) = args
            .and_then(|args| args.get_value_at(argument_index))
            .map(Value::dereferenced)
        else {
            missing = true;
            index += 2;
            literal = index;
            continue;
        };
        let value_type = value.value_type();
        if matches!(
            value_type,
            ValueType::Array | ValueType::Object | ValueType::Closure
        ) || value.is_binary_string()
        {
            return None;
        }
        if specifier != b's'
            && value_type == ValueType::Double
            && explicit_numeric_cast_warning(
                value,
                if matches!(specifier, b'f' | b'F') {
                    ExplicitNumericCastTarget::Float
                } else {
                    ExplicitNumericCastTarget::Int
                },
            )
            .is_some()
        {
            return None;
        }
        if matches!(specifier, b'f' | b'F')
            && value.as_double().is_some_and(|number| !number.is_finite())
        {
            return None;
        }
        match specifier {
            b's' => {
                if let Some(text) = value.as_str() {
                    output.push_str(text);
                } else {
                    value.append_echo_to_with_precision(&mut output, precision);
                }
            }
            b'd' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{number}");
            }
            b'u' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{}", number as u64);
            }
            b'f' | b'F' => {
                let number = value
                    .as_double()
                    .or_else(|| value.as_long().map(|number| number as f64))
                    .unwrap_or_else(|| explicit_float_conversion(value));
                let _ = write!(output, "{number:.6}");
            }
            b'x' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{number:x}");
            }
            b'X' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{number:X}");
            }
            b'o' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{number:o}");
            }
            b'b' => {
                let number = value
                    .as_long()
                    .unwrap_or_else(|| explicit_long_conversion(value));
                let _ = write!(output, "{number:b}");
            }
            _ => unreachable!("simple sprintf specifier was prevalidated"),
        }
        index += 2;
        literal = index;
    }
    if missing || count < required {
        let _ = check_sprintf_argument_count(format, args, call, eg);
        return None;
    }
    output.push_str(&format_text[literal..]);
    Some(output)
}

fn format_sprintf_values(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    format: &[u8],
    args: Option<&PhpArray>,
    call: SprintfCall,
    function: &str,
) -> Result<Option<SprintfOutput>, VmError> {
    let bytes = format;
    if let Some(output) = try_format_sprintf_simple(format, args, call, eg.precision, eg) {
        return Ok(Some(SprintfOutput::Text(output)));
    }
    if eg.exception.is_some() {
        return Ok(None);
    }
    if !check_sprintf_argument_count(format, args, call, eg) {
        return Ok(None);
    }
    let count = args.map_or(0, PhpArray::len);
    let mut output = Vec::with_capacity(format.len().saturating_add(count * 8));
    let mut index = 0usize;
    let mut literal = 0usize;
    let mut next = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        output.extend_from_slice(&format[literal..index]);
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            output.push(b'%');
            index += 1;
            literal = index;
            continue;
        }
        if index >= bytes.len() {
            sprintf_value_error(eg, "Missing format specifier at end of string");
            return Ok(None);
        }

        let position = match parse_sprintf_position(bytes, &mut index, eg).unwrap() {
            Ok(position) => position,
            Err(()) => return Ok(None),
        };
        let mut flags = SprintfFlags::default();
        loop {
            match bytes.get(index).copied() {
                Some(b'-') => flags.left = true,
                Some(b'+') => flags.plus = true,
                Some(b' ') => {}
                Some(b'0') => flags.zero = true,
                Some(b'\'') => {
                    index += 1;
                    let Some(pad) = bytes.get(index).copied() else {
                        sprintf_value_error(eg, "Missing padding character");
                        return Ok(None);
                    };
                    flags.pad = Some(pad);
                }
                _ => break,
            }
            index += 1;
        }

        let width = if bytes.get(index) == Some(&b'*') {
            let Some(width) =
                parse_sprintf_star_argument(bytes, &mut index, args, &mut next, call, "Width", eg)
            else {
                return Ok(None);
            };
            if !(0..i32::MAX as i64).contains(&width) {
                sprintf_value_error(eg, "Width must be between 0 and 2147483647");
                return Ok(None);
            }
            width as usize
        } else {
            let width = parse_sprintf_decimal(bytes, &mut index).unwrap_or(0);
            if width >= i32::MAX as usize {
                sprintf_value_error(eg, "Width must be between 0 and 2147483647");
                return Ok(None);
            }
            width
        };

        let mut precision = if bytes.get(index) == Some(&b'.') {
            index += 1;
            let precision = if bytes.get(index) == Some(&b'*') {
                let Some(precision) = parse_sprintf_star_argument(
                    bytes,
                    &mut index,
                    args,
                    &mut next,
                    call,
                    "Precision",
                    eg,
                ) else {
                    return Ok(None);
                };
                precision
            } else {
                let precision = parse_sprintf_decimal(bytes, &mut index).unwrap_or(0);
                if precision >= i32::MAX as usize {
                    sprintf_value_error(eg, "Precision must be between 0 and 2147483647");
                    return Ok(None);
                }
                precision as i64
            };
            Some(precision)
        } else {
            None
        };

        if bytes.get(index) == Some(&b'l') {
            index += 1;
        }
        let Some(specifier) = bytes.get(index).copied() else {
            sprintf_value_error(eg, "Missing format specifier at end of string");
            return Ok(None);
        };
        index += 1;
        if !matches!(
            specifier,
            b'b' | b'c'
                | b'd'
                | b'e'
                | b'E'
                | b'f'
                | b'F'
                | b'g'
                | b'G'
                | b'h'
                | b'H'
                | b'o'
                | b's'
                | b'u'
                | b'x'
                | b'X'
                | b'%'
        ) {
            sprintf_value_error(
                eg,
                format!("Unknown format specifier \"{}\"", specifier as char),
            );
            return Ok(None);
        }
        if precision.is_some_and(|precision| precision < -1) {
            sprintf_value_error(eg, "Precision must be between -1 and 2147483647");
            return Ok(None);
        }
        if precision.is_some_and(|precision| precision < 0)
            && !matches!(specifier, b'g' | b'G' | b'h' | b'H')
        {
            sprintf_value_error(
                eg,
                format!(
                    "Precision {} is only supported for %g, %G, %h and %H",
                    precision.unwrap()
                ),
            );
            return Ok(None);
        }
        if precision.is_some_and(|precision| precision > 53)
            && matches!(
                specifier,
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H'
            )
        {
            report_internal_diagnostic(
                eg,
                ed,
                8,
                "Notice",
                &format!(
                    "{function}(): Requested precision of {} digits was truncated to PHP maximum of 53 digits",
                    precision.unwrap()
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            precision = Some(53);
        }
        let Some(value) = take_sprintf_argument(args, &mut next, position, call, eg) else {
            return Ok(None);
        };
        let Some(rendered) = render_sprintf_value(ed, eg, &value, specifier, precision, &flags)?
        else {
            return Ok(None);
        };
        output.extend_from_slice(&apply_sprintf_width(rendered, width, &flags, specifier));
        literal = index;
    }
    output.extend_from_slice(&format[literal..]);
    Ok(Some(SprintfOutput::Bytes(output)))
}

// ============================================================================
// Type functions
// ============================================================================

fn fn_intval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let argument = arg!(ed, 0);
    let converted = explicit_long_conversion(argument);
    if let Some(message) = explicit_numeric_cast_warning(argument, ExplicitNumericCastTarget::Int) {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::long(converted));
}

fn fn_strval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    if value.as_double().is_some_and(f64::is_nan) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "unexpected NAN value was coerced to string",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    let Some(rendered) = internal_value_to_string(ed, eg, value)? else {
        return Ok(());
    };
    ret!(rv, Value::string(rendered));
}

fn fn_floatval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let argument = arg!(ed, 0);
    let converted = explicit_float_conversion(argument);
    if let Some(message) = explicit_numeric_cast_warning(argument, ExplicitNumericCastTarget::Float)
    {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::double(converted));
}

fn fn_boolval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let argument = arg!(ed, 0);
    let converted = argument.is_truthy();
    if argument.as_double().is_some_and(f64::is_nan) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "unexpected NAN value was coerced to bool",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::bool(converted));
}

fn fn_settype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let requested_type = arg_str!(ed, 1).to_ascii_lowercase();
    let target_type = match requested_type.as_str() {
        "int" | "integer" => "int",
        "float" | "double" => "float",
        "string" => "string",
        "bool" | "boolean" => "bool",
        "array" => "array",
        "object" => "object",
        "null" => "null",
        "resource" => {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "Cannot convert to resource type",
            ));
            return Ok(());
        }
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "settype(): Argument #2 ($type) must be a valid type",
            ));
            return Ok(());
        }
    };

    // A diagnostic handler may write to or unset the by-reference argument.
    // Scalar conversions use the value observed at call entry; container
    // conversions intentionally inspect the live reference after the handler.
    let clone_argument = || {
        // SAFETY: `arg_mut!` returns the live, initialized argument slot for
        // this internal call. Cloning does not retain a borrow across a PHP
        // diagnostic handler, which may re-enter and mutate that same slot.
        unsafe { (&*ptr).clone() }
    };
    let original = clone_argument();
    let original_is_nan = original.as_double().is_some_and(f64::is_nan);
    let numeric_warning = match target_type {
        "int" => explicit_numeric_cast_warning(&original, ExplicitNumericCastTarget::Int),
        "float" => explicit_numeric_cast_warning(&original, ExplicitNumericCastTarget::Float),
        _ => None,
    };
    let nan_warning = (original_is_nan && !matches!(target_type, "int" | "float"))
        .then(|| format!("unexpected NAN value was coerced to {target_type}"));
    if let Some(message) = numeric_warning.or(nan_warning) {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
    }

    let live = matches!(target_type, "array" | "object").then(clone_argument);
    let new_val = match target_type {
        "int" => Value::long(explicit_long_conversion(&original)),
        "float" => Value::double(explicit_float_conversion(&original)),
        "string" => {
            let Some(rendered) = internal_value_to_string(ed, eg, &original)? else {
                return Ok(());
            };
            Value::string(rendered)
        }
        "bool" => Value::bool(original.is_truthy()),
        "array" if original_is_nan => settype_nan_array_value(live.as_ref().unwrap(), &original),
        "array" => settype_array_value(live.as_ref().unwrap(), eg),
        "object" if original_is_nan => settype_nan_object_value(live.as_ref().unwrap(), &original),
        "object" => settype_object_value(live.as_ref().unwrap()),
        "null" => Value::null(),
        _ => unreachable!(),
    };
    unsafe {
        std::ptr::drop_in_place(ptr);
        ptr.write(new_val);
    }
    ret!(rv, Value::bool(true));
}

fn settype_array_value(value: &Value, eg: &ExecutorGlobals) -> Value {
    match value.value_type() {
        ValueType::Array => value.clone(),
        ValueType::Object => crate::vm::execute::cast_object_to_array(value, eg),
        ValueType::Closure | ValueType::Null | ValueType::Undef => Value::array(PhpArray::new()),
        _ => {
            let mut result = PhpArray::new();
            result.push(value.clone());
            Value::array(result)
        }
    }
}

fn settype_nan_array_value<'a>(live: &'a Value, original: &'a Value) -> Value {
    let value = if live.value_type() == ValueType::Undef {
        original
    } else {
        live
    };
    let mut result = PhpArray::new();
    result.push(value.clone());
    Value::array(result)
}

fn settype_object_value(value: &Value) -> Value {
    match value.value_type() {
        ValueType::Object | ValueType::Closure => value.clone(),
        ValueType::Array => {
            let array = value.as_array().unwrap();
            let mut object = PhpObject::std_class(std::collections::HashMap::new());
            for (key, property) in array.iter() {
                let key = match key {
                    ArrayKey::Int(key) => key.to_string(),
                    ArrayKey::String(key) => key,
                };
                object.set_property(&key, property.clone());
            }
            Value::object(object)
        }
        ValueType::Null | ValueType::Undef => {
            Value::object(PhpObject::std_class(std::collections::HashMap::new()))
        }
        _ => {
            let mut object = PhpObject::std_class(std::collections::HashMap::new());
            object.set_property("scalar", value.clone());
            Value::object(object)
        }
    }
}

fn settype_nan_object_value<'a>(live: &'a Value, original: &'a Value) -> Value {
    let value = if live.value_type() == ValueType::Undef {
        original
    } else {
        live
    };
    let mut object = PhpObject::std_class(std::collections::HashMap::new());
    object.set_property("scalar", value.clone());
    Value::object(object)
}

fn internal_value_to_string(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<String>, VmError> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::Array {
        report_internal_diagnostic(eg, ed, 2, "Warning", "Array to string conversion")?;
        return Ok(Some("Array".to_string()));
    }
    if value.value_type() == ValueType::Closure {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Object of class Closure could not be converted to string",
        ));
        return Ok(None);
    }
    if value.value_type() != ValueType::Object {
        return Ok(Some(value.echo_to_string_with_precision(eg.precision)));
    }

    let class_name = value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "object".to_string());
    let rendered = crate::vm::execute::call_object_string_conversion(eg, value)?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(rendered) = rendered else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Object of class {class_name} could not be converted to string"),
        ));
        return Ok(None);
    };
    let Some(rendered) = rendered.as_str() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{class_name}::__toString(): Return value must be of type string"),
        ));
        return Ok(None);
    };
    Ok(Some(rendered.to_string()))
}

fn fn_is_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(arg!(ed, 0).value_type() == ValueType::Array)
    );
}

fn fn_is_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(arg!(ed, 0).value_type() == ValueType::String)
    );
}

fn fn_is_int(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Long));
}

fn fn_is_float(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(arg!(ed, 0).value_type() == ValueType::Double)
    );
}

fn fn_is_null(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(matches!(
            arg!(ed, 0).value_type(),
            ValueType::Null | ValueType::Undef
        ))
    );
}

fn fn_is_bool(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let t = arg!(ed, 0).value_type();
    ret!(
        rv,
        Value::bool(t == ValueType::True || t == ValueType::False)
    );
}

fn fn_is_numeric(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let result = match v.value_type() {
        ValueType::Long | ValueType::Double => true,
        ValueType::String => {
            let s = v.as_str().unwrap().trim();
            s.parse::<i64>().is_ok() || s.parse::<f64>().is_ok()
        }
        _ => false,
    };
    ret!(rv, Value::bool(result));
}

fn fn_is_object(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(matches!(
            arg!(ed, 0).value_type(),
            ValueType::Object | ValueType::Closure
        ))
    );
}

fn fn_is_iterable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let iterable = value.value_type() == ValueType::Array
        || value
            .as_object()
            .is_some_and(|object| eg.class_is_a(object.class_name.as_ref(), "Traversable"));
    ret!(rv, Value::bool(iterable));
}

fn fn_gettype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let name = match value.value_type() {
        ValueType::Null => "NULL",
        ValueType::True | ValueType::False => "boolean",
        ValueType::Long => "integer",
        ValueType::Double => "double",
        ValueType::String => "string",
        ValueType::Array => "array",
        ValueType::Object | ValueType::Closure => "object",
        ValueType::Resource => {
            if resource::is_open_for_request(eg, value.as_resource_id().unwrap()) {
                "resource"
            } else {
                "resource (closed)"
            }
        }
        _ => "unknown type",
    };
    ret!(rv, Value::string(name));
}

fn fn_get_debug_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let name = match value.value_type() {
        ValueType::Undef | ValueType::Null => "null".to_string(),
        ValueType::True | ValueType::False => "bool".to_string(),
        ValueType::Long => "int".to_string(),
        ValueType::Double => "float".to_string(),
        ValueType::String => "string".to_string(),
        ValueType::Array => "array".to_string(),
        ValueType::Object => value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string()),
        ValueType::Closure => "Closure".to_string(),
        ValueType::Resource => {
            if resource::is_open_for_request(eg, value.as_resource_id().unwrap()) {
                "resource (stream)".to_string()
            } else {
                "resource (closed)".to_string()
            }
        }
        ValueType::Reference => value.dereferenced().type_name().to_string(),
    };
    ret!(rv, Value::string(name));
}

// ============================================================================
// Reflection / class introspection
// ============================================================================

fn fn_get_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if v.value_type() == ValueType::Undef {
        let caller_class = get_calling_scope_class(ed, eg).map(str::to_owned);
        if let Some(cls) = caller_class {
            report_internal_deprecation(
                eg,
                ed,
                "Calling get_class() without arguments is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ret!(rv, Value::string(cls));
        }
        // Outside class scope: PHP throws Error
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "get_class() without arguments must be called from within a class",
        ));
        return Ok(());
    }
    if v.value_type() == ValueType::Closure {
        ret!(rv, Value::string("Closure"));
    }
    if let Some(obj) = v.as_object() {
        let class_name = if obj.class_name.starts_with("class@anonymous#") {
            eg.class_by_id(obj.class_id)
                .and_then(|class| class.anonymous_public_name())
                .unwrap_or_else(|| obj.class_name.to_string())
        } else {
            obj.class_name.to_string()
        };
        ret!(rv, Value::string(class_name));
    }
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "get_class(): Argument #1 ($object) must be of type object, {} given",
            v.dereferenced().type_name()
        ),
    ));
    ret!(rv, Value::null());
}

fn fn_get_called_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(class_name) = crate::vm::execute::called_class_name_for_internal_call(eg, ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "get_called_class() must be called from within a class",
        ));
        return Ok(());
    };
    ret!(rv, Value::string(class_name));
}

fn method_visible_from_scope(
    eg: &ExecutorGlobals,
    visibility: Visibility,
    declaring_class: &str,
    caller_class: Option<&str>,
) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Private => {
            caller_class.is_some_and(|caller| caller.eq_ignore_ascii_case(declaring_class))
        }
        Visibility::Protected => caller_class.is_some_and(|caller| {
            caller.eq_ignore_ascii_case(declaring_class)
                || eg.class_is_a(caller, declaring_class)
                || eg.class_is_a(declaring_class, caller)
        }),
    }
}

fn collect_composed_trait_methods(
    eg: &ExecutorGlobals,
    trait_name: &str,
    adaptation_owner: &crate::compiler::compile::ClassDef,
    consuming_class: &str,
    caller_class: Option<&str>,
    seen: &mut std::collections::HashSet<String>,
    methods: &mut Vec<String>,
) {
    let Some(trait_def) = find_class_case_insensitive(eg, trait_name) else {
        return;
    };
    for (name, visibility, _, _, _) in &trait_def.methods {
        if name.starts_with('$') && (name.ends_with("::get") || name.ends_with("::set")) {
            continue;
        }
        for adaptation in adaptation_owner.trait_aliases.iter().filter(|adaptation| {
            adaptation.alias.is_some()
                && adaptation.method.eq_ignore_ascii_case(name)
                && adaptation
                    .trait_name
                    .as_deref()
                    .is_none_or(|source| source.eq_ignore_ascii_case(trait_name))
        }) {
            let alias = adaptation.alias.as_deref().unwrap_or(name);
            let alias_visibility = adaptation.visibility.unwrap_or(*visibility);
            if seen.insert(alias.to_ascii_lowercase())
                && method_visible_from_scope(eg, alias_visibility, consuming_class, caller_class)
            {
                methods.push(alias.to_string());
            }
        }
        let effective_visibility = adaptation_owner
            .trait_aliases
            .iter()
            .find(|adaptation| {
                adaptation.alias.is_none() && adaptation.method.eq_ignore_ascii_case(name)
            })
            .and_then(|adaptation| adaptation.visibility)
            .unwrap_or(*visibility);
        if seen.insert(name.to_ascii_lowercase())
            && method_visible_from_scope(eg, effective_visibility, consuming_class, caller_class)
        {
            methods.push(name.clone());
        }
    }
    for nested_trait in &trait_def.uses {
        collect_composed_trait_methods(
            eg,
            nested_trait,
            trait_def,
            consuming_class,
            caller_class,
            seen,
            methods,
        );
    }
}

fn collect_visible_class_methods(
    eg: &ExecutorGlobals,
    class_name: &str,
    caller_class: Option<&str>,
    seen: &mut std::collections::HashSet<String>,
    methods: &mut Vec<String>,
) {
    let Some(class) = find_class_case_insensitive(eg, class_name) else {
        return;
    };
    for (name, visibility, _, _, _) in &class.methods {
        if name.starts_with('$') && (name.ends_with("::get") || name.ends_with("::set")) {
            continue;
        }
        if seen.insert(name.to_ascii_lowercase())
            && method_visible_from_scope(eg, *visibility, &class.name, caller_class)
        {
            methods.push(name.clone());
        }
    }
    if let Some(parent) = &class.parent {
        collect_visible_class_methods(eg, parent, caller_class, seen, methods);
    }
    for trait_name in &class.uses {
        collect_composed_trait_methods(
            eg,
            trait_name,
            class,
            &class.name,
            caller_class,
            seen,
            methods,
        );
    }
    for interface in &class.implements {
        collect_visible_class_methods(eg, interface, caller_class, seen, methods);
    }
}

fn fn_get_class_methods(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = arg!(ed, 0);
    let class_name = if let Some(object) = target.as_object() {
        object.class_name.to_string()
    } else if target.value_type() == ValueType::Closure {
        "Closure".to_string()
    } else if let Some(class_name) = target.as_str() {
        if !autoload::ensure_symbol_loaded(eg, class_name)? {
            if eg.exception.is_none() {
                invalid_class_methods_argument(eg, target);
            }
            return Ok(());
        }
        class_name
            .strip_prefix('\\')
            .unwrap_or(class_name)
            .to_string()
    } else {
        invalid_class_methods_argument(eg, target);
        return Ok(());
    };

    let caller_class = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed);
    let mut names = Vec::new();
    collect_visible_class_methods(
        eg,
        &class_name,
        caller_class.as_deref(),
        &mut std::collections::HashSet::new(),
        &mut names,
    );
    let mut result = PhpArray::with_packed_capacity(names.len());
    for name in names {
        result.push(Value::string(name));
    }
    ret!(rv, Value::array(result));
}

fn fn_get_class_vars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = arg!(ed, 0);
    let Some(class_name) = target.as_str() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "get_class_vars(): Argument #1 ($class) must be a valid class name, {} given",
                target.type_name()
            ),
        ));
        return Ok(());
    };
    if !autoload::ensure_symbol_loaded(eg, class_name)? {
        if eg.exception.is_none() {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "get_class_vars(): Argument #1 ($class) must be a valid class name, {} given",
                    class_name
                ),
            ));
        }
        return Ok(());
    }

    let caller_class = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed);
    let Some(class) = find_class_case_insensitive(eg, class_name) else {
        return Ok(());
    };
    let mut result =
        PhpArray::with_hash_capacity(class.properties.len() + class.static_properties.len());
    for property in class.properties.iter().chain(&class.static_properties) {
        if method_visible_from_scope(
            eg,
            property.visibility,
            &property.declaring_class,
            caller_class.as_deref(),
        ) {
            result.set_str(
                &property.name,
                property.default.clone().unwrap_or_else(Value::null),
            );
        }
    }
    ret!(rv, Value::array(result));
}

fn clone_object_var(value: &Value) -> Value {
    if value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else {
        value.clone()
    }
}

fn set_object_var(result: &mut PhpArray, name: &str, value: Value) {
    if let Some(key) = crate::value::canonical_decimal_array_key(name) {
        result.set_int(key, value);
    } else {
        result.set_str(name, value);
    }
}

fn fn_get_object_vars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target_owner = arg!(ed, 0).clone();
    let target = if eg.is_uninitialized_lazy_object(&target_owner) {
        reflection::initialize_lazy_object(eg, &target_owner)?
    } else {
        eg.lazy_proxy_instance(&target_owner)
            .unwrap_or(target_owner)
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let target = &target;
    if target.value_type() == ValueType::Closure {
        ret!(rv, Value::array(PhpArray::new()));
    }
    let Some(object) = target.as_object() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "get_object_vars(): Argument #1 ($object) must be of type object, {} given",
                target.type_name()
            ),
        ));
        return Ok(());
    };

    let caller_class = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed);
    let dynamic_len = object
        .dynamic_properties
        .as_ref()
        .map_or(0, |properties| properties.len());
    let mut result = PhpArray::with_hash_capacity(object.property_values.len() + dynamic_len);
    let mut declared_names = std::collections::HashSet::new();
    for slot in eg.visible_instance_property_slots(object.class_id, caller_class.as_deref()) {
        let value = &object.property_values[slot];
        if value.is_undef() {
            continue;
        }
        let Some(definition) = eg.instance_property_definition(object.class_id, slot) else {
            continue;
        };
        declared_names.insert(definition.name.clone());
        set_object_var(&mut result, &definition.name, clone_object_var(value));
    }
    object.for_each_dynamic_property(|name, value| {
        if !value.is_undef() && !declared_names.contains(name) {
            set_object_var(&mut result, name, clone_object_var(value));
        }
    });
    ret!(rv, Value::array(result));
}

fn fn_get_mangled_object_vars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target_owner = arg!(ed, 0).clone();
    let target = eg
        .lazy_proxy_instance(&target_owner)
        .unwrap_or(target_owner);
    if target.value_type() == ValueType::Closure {
        ret!(rv, Value::array(PhpArray::new()));
    }
    if target.value_type() != ValueType::Object {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "get_mangled_object_vars(): Argument #1 ($object) must be of type object, {} given",
                target.type_name()
            ),
        ));
        return Ok(());
    }
    ret!(rv, crate::vm::execute::cast_object_to_array(&target, eg));
}

fn invalid_class_methods_argument(eg: &mut ExecutorGlobals, value: &Value) {
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
            value.type_name()
        ),
    ));
}

fn invalid_parent_class_argument(eg: &mut ExecutorGlobals, value: &Value) {
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, {} given",
            value.type_name()
        ),
    ));
}

fn fn_get_parent_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name = if let Some(target) = arg_opt!(ed, 0) {
        if let Some(object) = target.as_object() {
            object.class_name.to_string()
        } else if target.value_type() == ValueType::Closure {
            ret!(rv, Value::bool(false));
        } else if let Some(class_name) = target.as_str() {
            if !autoload::ensure_symbol_loaded(eg, class_name)? {
                if eg.exception.is_none() {
                    invalid_parent_class_argument(eg, target);
                }
                return Ok(());
            }
            class_name
                .strip_prefix('\\')
                .unwrap_or(class_name)
                .to_string()
        } else {
            invalid_parent_class_argument(eg, target);
            return Ok(());
        }
    } else {
        report_internal_deprecation(
            eg,
            ed,
            "Calling get_parent_class() without arguments is deprecated",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let Some(class_name) = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed)
        else {
            ret!(rv, Value::bool(false));
        };
        class_name
    };

    let parent = find_class_case_insensitive(eg, &class_name)
        .and_then(|class| class.parent.as_deref())
        .map(str::to_owned);
    match parent {
        Some(parent) => ret!(rv, Value::string(parent)),
        None => ret!(rv, Value::bool(false)),
    }
}

fn declared_names_value(names: Vec<String>) -> Value {
    let mut result = PhpArray::with_packed_capacity(names.len());
    for name in names {
        result.push(Value::string(name));
    }
    Value::array(result)
}

fn fn_get_declared_classes(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, declared_names_value(eg.declared_class_names()));
}

fn builtin_constants_value(eg: &ExecutorGlobals) -> PhpArray {
    let mut constants = PhpArray::new();
    for name in crate::BUILTIN_CONSTANT_NAMES {
        if let Some(value) = crate::builtin_constant(name) {
            constants.set_str(name, value);
        }
    }
    for name in ["STDIN", "STDOUT", "STDERR"] {
        if let Some(value) = eg.constant_table.borrow().get(name).cloned() {
            constants.set_str(name, value);
        }
    }
    constants
}

fn user_constants_value(eg: &ExecutorGlobals) -> PhpArray {
    let mut result = PhpArray::new();
    for (name, value) in eg.defined_dynamic_constants() {
        if name.starts_with('\0') || matches!(name.as_str(), "STDIN" | "STDOUT" | "STDERR") {
            continue;
        }
        result.set_str(&name, value);
    }
    result
}

/// get_defined_constants($categorize = false): array
fn fn_get_defined_constants(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let categorize = if arg_opt!(ed, 0).is_some() {
        let Some(categorize) =
            typed_internal_bool_argument(ed, eg, "get_defined_constants", 0, "categorize")?
        else {
            return Ok(());
        };
        categorize
    } else {
        false
    };
    let builtins = builtin_constants_value(eg);
    let user = user_constants_value(eg);
    if categorize {
        let mut result = PhpArray::new();
        result.set_str("Core", Value::array(builtins));
        if !user.is_empty() {
            result.set_str("user", Value::array(user));
        }
        ret!(rv, Value::array(result));
    }
    let mut result = builtins;
    for (key, value) in user.iter() {
        if let ArrayKey::String(name) = key {
            result.set_str(&name, value.clone());
        }
    }
    ret!(rv, Value::array(result));
}

fn get_defined_functions_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<bool, VmError> {
    let Some(argument) = arg_opt!(ed, 0).map(|argument| argument.dereferenced().clone()) else {
        return Ok(true);
    };
    let strict = internal_call_is_strict(ed);
    let valid = match argument.value_type() {
        ValueType::True | ValueType::False => true,
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                "get_defined_functions(): Passing null to parameter #1 ($exclude_disabled) of type bool is deprecated",
            )?;
            eg.exception.is_none()
        }
        ValueType::Long | ValueType::String if !strict => true,
        ValueType::Double if !strict => {
            if argument.as_double().is_some_and(f64::is_nan) {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to bool",
                )?;
            }
            eg.exception.is_none()
        }
        _ => false,
    };
    if !valid {
        if eg.exception.is_none() {
            let actual = match argument.value_type() {
                ValueType::True => "true".to_string(),
                ValueType::False => "false".to_string(),
                _ => argument.diagnostic_type_name().into_owned(),
            };
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "get_defined_functions(): Argument #1 ($exclude_disabled) must be of type bool, {actual} given"
                ),
            ));
        }
        return Ok(false);
    }

    report_internal_deprecation(
        eg,
        ed,
        "get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0",
    )?;
    Ok(eg.exception.is_none())
}

fn fn_get_defined_functions(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !get_defined_functions_argument(ed, eg)? {
        return Ok(());
    }

    let mut internal = Vec::new();
    let mut user = Vec::new();
    for (name, &function) in &eg.function_table {
        if name.contains("::") || name.starts_with("__closure_") {
            continue;
        }
        // SAFETY: every function-table entry is registered from a live
        // FunctionCommon owner and remains valid for the complete request.
        let function_type = unsafe { Function::from_common_ptr(function) }.fn_type();
        match function_type {
            FunctionType::Internal => internal.push(name.clone()),
            FunctionType::User => user.push(name.clone()),
            FunctionType::Undef => {}
        }
    }
    internal.extend(
        crate::builtin_metadata::INTERNAL_FUNCTION_ALIASES
            .iter()
            .filter(|alias| eg.function_table.contains_key(alias.target))
            .map(|alias| alias.alias.to_string()),
    );
    // The function table is hash-backed. PHP does not specify list ordering,
    // so make the exposed RPHP inventory stable across repeated requests.
    internal.sort_unstable();
    user.sort_unstable();

    let mut result = PhpArray::new();
    result.set_str("internal", declared_names_value(internal));
    result.set_str("user", declared_names_value(user));
    ret!(rv, Value::array(result));
}

fn fn_get_included_files(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, declared_names_value(eg.included_file_names().to_vec()));
}

fn fn_get_declared_interfaces(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, declared_names_value(eg.declared_interface_names()));
}

fn fn_get_declared_traits(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, declared_names_value(eg.declared_trait_names()));
}

fn fn_method_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = arg!(ed, 0);
    let method_name = arg_str!(ed, 1);

    // Resolve the class name: from object or string
    let (class_name, needs_autoload): (String, bool) = if first.value_type() == ValueType::Closure {
        ("Closure".to_string(), false)
    } else if let Some(obj) = first.as_object() {
        (obj.class_name.to_string(), false)
    } else if let Some(s) = first.as_str() {
        (s.to_string(), true)
    } else {
        ret!(rv, Value::bool(false));
    };

    if needs_autoload && !autoload::ensure_symbol_loaded(eg, &class_name)? {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }

    // method_exists() includes abstract and non-public declarations; callback
    // resolution deliberately uses the stricter callable-only helper below.
    let found = method_declared_in_class_hierarchy(eg, &class_name, &method_name)
        || (class_name.eq_ignore_ascii_case("Closure")
            && eg
                .function_table
                .contains_key(&format!("closure::{}", method_name.to_ascii_lowercase())));
    ret!(rv, Value::bool(found));
}

fn property_declared_in_class(eg: &ExecutorGlobals, class_name: &str, property_name: &str) -> bool {
    find_class_case_insensitive(eg, class_name).is_some_and(|class| {
        class
            .properties
            .iter()
            .chain(&class.static_properties)
            .any(|property| property.name == property_name)
    })
}

fn fn_property_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = arg!(ed, 0);
    let property_name = arg_str!(ed, 1);

    if let Some(object) = target.as_object() {
        let found = property_declared_in_class(eg, &object.class_name, &property_name)
            || object.contains_property(&property_name);
        ret!(rv, Value::bool(found));
    }
    if target.value_type() == ValueType::Closure {
        ret!(rv, Value::bool(false));
    }
    let Some(class_name) = target.as_str() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "property_exists(): Argument #1 ($object_or_class) must be of type object|string, {} given",
                target.type_name()
            ),
        ));
        return Ok(());
    };
    if !autoload::ensure_symbol_loaded(eg, class_name)? {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }
    ret!(
        rv,
        Value::bool(property_declared_in_class(eg, class_name, &property_name))
    );
}

fn class_relation_operands(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    default_allow_string: bool,
) -> Result<Option<(String, String)>, VmError> {
    let first = arg!(ed, 0);
    let target = arg_str!(ed, 1).into_owned();
    if let Some(object) = first.as_object() {
        return Ok(Some((object.class_name.to_string(), target)));
    }
    if first.value_type() == ValueType::Closure {
        return Ok(Some(("Closure".to_string(), target)));
    }
    let allow_string = arg_opt!(ed, 2)
        .map(Value::is_truthy)
        .unwrap_or(default_allow_string);
    let Some(class_name) = first.as_str().filter(|_| allow_string) else {
        return Ok(None);
    };
    let class_name = class_name.to_string();
    if !autoload::ensure_symbol_loaded(eg, &class_name)? {
        return Ok(None);
    }
    Ok(Some((class_name, target)))
}

fn fn_is_a(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some((class_name, target)) = class_relation_operands(ed, eg, false)? else {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    };
    ret!(rv, Value::bool(eg.class_is_a(&class_name, &target)));
}

fn fn_is_subclass_of(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((class_name, target)) = class_relation_operands(ed, eg, true)? else {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    };
    let same_identity = eg
        .find_class(&class_name)
        .zip(eg.find_class(&target))
        .is_some_and(|(class, target)| std::ptr::eq(class, target));
    let is_subclass = !same_identity && eg.class_is_a(&class_name, &target);
    ret!(rv, Value::bool(is_subclass));
}

fn fn_class_implements(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let class_name = if value.value_type() == ValueType::Closure {
        "Closure".to_string()
    } else if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_public_class(&class_name).is_none()
        && (!autoload_enabled || !autoload::ensure_symbol_loaded(eg, &class_name)?)
    {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }

    let mut project_stringable = false;
    let mut result = PhpArray::new();
    let mut classes = vec![class_name];
    let mut interfaces = Vec::new();
    while let Some(class_name) = classes.pop() {
        if let Some(class) = eg.find_class(&class_name) {
            project_stringable |= !class.is_trait && eg.class_contributes_stringable(class);
            interfaces.extend(class.implements.iter().cloned());
            if let Some(parent) = &class.parent {
                classes.push(parent.clone());
            }
        }
    }
    while let Some(interface_name) = interfaces.pop() {
        let reported_name = eg
            .find_class(&interface_name)
            .filter(|interface| interface.name.eq_ignore_ascii_case("Stringable"))
            .map_or(interface_name.as_str(), |_| "Stringable");
        if result.get_str(reported_name).is_some() {
            continue;
        }
        result.set_str(reported_name, Value::string(reported_name));
        if let Some(interface) = eg.find_class(&interface_name) {
            project_stringable |= eg.class_contributes_stringable(interface);
            interfaces.extend(interface.implements.iter().cloned());
        }
    }
    if project_stringable && result.get_str("Stringable").is_none() {
        result.set_str("Stringable", Value::string("Stringable"));
    }
    ret!(rv, Value::array(result));
}

fn fn_class_parents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let class_name = if value.value_type() == ValueType::Closure {
        "Closure".to_string()
    } else if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_public_class(&class_name).is_none()
        && (!autoload_enabled || !autoload::ensure_symbol_loaded(eg, &class_name)?)
    {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }

    let mut result = PhpArray::new();
    let mut current = eg
        .find_class(&class_name)
        .and_then(|class| class.parent.clone());
    while let Some(parent_name) = current {
        let canonical = eg
            .find_class(&parent_name)
            .map_or(parent_name, |class| class.name.clone());
        result.set_str(&canonical, Value::string(canonical.clone()));
        current = eg
            .find_class(&canonical)
            .and_then(|class| class.parent.clone());
    }
    ret!(rv, Value::array(result));
}

fn fn_class_uses(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let class_name = if value.value_type() == ValueType::Closure {
        "Closure".to_string()
    } else if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_public_class(&class_name).is_none()
        && (!autoload_enabled || !autoload::ensure_symbol_loaded(eg, &class_name)?)
    {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }

    let mut result = PhpArray::new();
    if let Some(class) = eg.find_class(&class_name) {
        for trait_name in &class.uses {
            let canonical = eg.find_class(trait_name).map_or_else(
                || trait_name.clone(),
                |trait_class| trait_class.name.clone(),
            );
            result.set_str(&canonical, Value::string(canonical.clone()));
        }
    }
    ret!(rv, Value::array(result));
}

// ============================================================================
// Math functions
// ============================================================================

#[inline(always)]
fn direct_abs(args: &[Value]) -> Result<Value, VmError> {
    let value = direct_arg(args, 0);
    Ok(match value.value_type() {
        ValueType::Long => Value::long(value.as_long().unwrap().abs()),
        ValueType::Double => Value::double(value.as_double().unwrap().abs()),
        _ => Value::long(0),
    })
}

fn fn_abs(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_abs(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

#[inline]
fn extrema_comparison(candidate: &Value, current: &Value, precision: i32) -> Result<i32, ()> {
    let candidate = candidate.dereferenced();
    let current = current.dereferenced();
    match (candidate.value_type(), current.value_type()) {
        (ValueType::Long, ValueType::Long) => Ok(
            match candidate
                .as_long()
                .unwrap()
                .cmp(&current.as_long().unwrap())
            {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        ),
        (ValueType::Double, ValueType::Double) => Ok(candidate
            .as_double()
            .unwrap()
            .partial_cmp(&current.as_double().unwrap())
            .map_or(
                crate::vm::execute::PHP_COMPARISON_UNORDERED,
                |ordering| match ordering {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                },
            )),
        _ => {
            crate::vm::execute::values_compare_checked_with_precision(candidate, current, precision)
        }
    }
}

#[inline(always)]
fn direct_extrema2<const MAXIMUM: bool>(
    first: &Value,
    second: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let comparison = match extrema_comparison(first, second, eg.precision) {
        Ok(comparison) => comparison,
        Err(()) => {
            report_recursive_sort_comparison(eg);
            return Ok(Value::undef());
        }
    };
    let use_first = if MAXIMUM {
        matches!(comparison, 0 | 1)
    } else {
        comparison == -1
    };
    Ok(if use_first {
        first.dereferenced().clone()
    } else {
        second.dereferenced().clone()
    })
}

fn fn_extrema<const MAXIMUM: bool>(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let function = if MAXIMUM { "max" } else { "min" };
    let extras = arg!(ed, 1)
        .as_array()
        .expect("variadic extrema arguments must be packed into an array");

    let mut best = if extras.is_empty() {
        let source = arg!(ed, 0);
        let Some(array) = source.as_array() else {
            typed_internal_argument_error(eg, function, source, 1, "value", "array");
            return Ok(());
        };
        let Some(first) = array.values().next() else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                &format!("{function}(): Argument #1 ($value) must contain at least one element"),
            ));
            return Ok(());
        };
        first.dereferenced().clone()
    } else {
        owned_argument(ed, 0)
    };

    let mut consider = |candidate: &Value| -> Result<(), ()> {
        let comparison = extrema_comparison(candidate, &best, eg.precision)?;
        let replace = if MAXIMUM {
            comparison == 1
        } else {
            comparison == -1
        };
        if replace {
            best = candidate.dereferenced().clone();
        }
        Ok(())
    };

    if extras.is_empty() {
        let array = arg!(ed, 0).as_array().unwrap();
        for candidate in array.values().skip(1) {
            if consider(candidate).is_err() {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        }
    } else {
        for candidate in extras.values() {
            if consider(candidate).is_err() {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        }
    }
    ret!(rv, best);
}

fn fn_extrema_raw_variadic<const MAXIMUM: bool>(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    let function = if MAXIMUM { "max" } else { "min" };
    if supplied_num_args == 1 {
        let source = arg!(ed, 0);
        let Some(array) = source.as_array() else {
            typed_internal_argument_error(eg, function, source, 1, "value", "array");
            return Ok(());
        };
        let mut values = array.values();
        let Some(first) = values.next() else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                &format!("{function}(): Argument #1 ($value) must contain at least one element"),
            ));
            return Ok(());
        };
        let mut best = first.dereferenced().clone();
        for candidate in values {
            let comparison = match extrema_comparison(candidate, &best, eg.precision) {
                Ok(comparison) => comparison,
                Err(()) => {
                    report_recursive_sort_comparison(eg);
                    return Ok(());
                }
            };
            if (MAXIMUM && comparison == 1) || (!MAXIMUM && comparison == -1) {
                best = candidate.dereferenced().clone();
            }
        }
        ret!(rv, best);
    }

    let mut best = owned_argument(ed, 0);
    for index in 1..supplied_num_args {
        let candidate = arg!(ed, index);
        let comparison = match extrema_comparison(candidate, &best, eg.precision) {
            Ok(comparison) => comparison,
            Err(()) => {
                report_recursive_sort_comparison(eg);
                return Ok(());
            }
        };
        if (MAXIMUM && comparison == 1) || (!MAXIMUM && comparison == -1) {
            best = candidate.dereferenced().clone();
        }
    }
    ret!(rv, best);
}

fn fn_max(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_extrema::<true>(ed, rv, eg)
}

fn fn_min(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    fn_extrema::<false>(ed, rv, eg)
}

fn fn_max_raw_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    fn_extrema_raw_variadic::<true>(ed, rv, eg, supplied_num_args)
}

fn fn_min_raw_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    fn_extrema_raw_variadic::<false>(ed, rv, eg, supplied_num_args)
}

#[inline(always)]
fn direct_floor(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().floor()))
}

fn fn_floor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = direct_floor(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

fn fn_ceil(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).ceil()));
}

fn fn_round(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let d = arg_float!(ed, 0);
    let precision = match arg_opt!(ed, 1) {
        Some(v) => v.to_long_val(),
        None => 0,
    };
    if precision == 0 {
        ret!(rv, Value::double(d.round()));
    } else {
        let factor = 10f64.powi(precision as i32);
        ret!(rv, Value::double((d * factor).round() / factor));
    }
}

fn fn_pow(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let base = arg!(ed, 0);
    let exp = arg!(ed, 1);
    if let (Some(b), Some(e)) = (base.as_long(), exp.as_long()) {
        if e >= 0 {
            ret!(rv, Value::long(b.wrapping_pow(e as u32)));
        }
    }
    ret!(
        rv,
        Value::double(base.to_float_val().powf(exp.to_float_val()))
    );
}

#[inline(always)]
fn direct_sqrt(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().sqrt()))
}

fn fn_sqrt(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_sqrt(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

#[inline(always)]
fn direct_intdiv_values(first: &Value, second: &Value) -> Result<Value, VmError> {
    let a = if first.is_reference() {
        unsafe { &*first.as_ref_ptr() }
    } else {
        first
    }
    .to_long_val();
    let b = if second.is_reference() {
        unsafe { &*second.as_ref_ptr() }
    } else {
        second
    }
    .to_long_val();
    if b == 0 {
        Ok(Value::bool(false)) // PHP throws DivisionByZeroError
    } else {
        Ok(Value::long(a / b))
    }
}

#[inline(always)]
fn direct_intdiv(args: &[Value]) -> Result<Value, VmError> {
    direct_intdiv_values(direct_arg(args, 0), direct_arg(args, 1))
}

fn fn_intdiv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = direct_intdiv_values(arg!(ed, 0), arg!(ed, 1))?;
    ret!(rv, result);
}

fn fn_fmod(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg_float!(ed, 0);
    let b = arg_float!(ed, 1);
    ret!(rv, Value::double(a % b));
}

fn fn_fdiv(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let numerator = arg_float!(ed, 0);
    let denominator = arg_float!(ed, 1);
    ret!(rv, Value::double(numerator / denominator));
}

fn fn_log(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).ln()));
}

fn fn_log10(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).log10()));
}

fn fn_log2(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).log2()));
}

fn fn_pi(_ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(std::f64::consts::PI));
}

/// Convert the shared `float $num` contract used by PHP's floating-point
/// classifiers. Integers always widen exactly as call-boundary float hints do;
/// the other scalar conversions are available only to weak callers.
fn floating_classification_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<Option<f64>, VmError> {
    let argument = arg!(ed, 0).dereferenced();
    let strict = internal_call_is_strict(ed);
    let number = match argument.value_type() {
        ValueType::Double => argument.as_double(),
        ValueType::Long => argument.as_long().map(|number| number as f64),
        ValueType::String if !strict => argument.as_str().and_then(php_numeric_string_to_float),
        ValueType::True | ValueType::False if !strict => Some(f64::from(argument.is_truthy())),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #1 ($num) of type float is deprecated"
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(0.0)
        }
        _ => None,
    };
    if number.is_none() {
        let actual = match argument.value_type() {
            ValueType::True => Cow::Borrowed("true"),
            ValueType::False => Cow::Borrowed("false"),
            _ => argument.diagnostic_type_name(),
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): Argument #1 ($num) must be of type float, {} given",
                actual
            ),
        ));
    }
    Ok(number)
}

fn fn_is_nan(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = floating_classification_argument(ed, eg, "is_nan")? else {
        return Ok(());
    };
    ret!(rv, Value::bool(number.is_nan()));
}

fn fn_is_finite(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = floating_classification_argument(ed, eg, "is_finite")? else {
        return Ok(());
    };
    ret!(rv, Value::bool(number.is_finite()));
}

fn fn_is_infinite(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = floating_classification_argument(ed, eg, "is_infinite")? else {
        return Ok(());
    };
    ret!(rv, Value::bool(number.is_infinite()));
}

fn fn_rand(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // Simple pseudo-random
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let (lo, hi) = match arg_opt!(ed, 0) {
        Some(v) => (
            v.to_long_val(),
            match arg_opt!(ed, 1) {
                Some(v2) => v2.to_long_val(),
                None => i32::MAX as i64,
            },
        ),
        None => (0, i32::MAX as i64),
    };
    let range = (hi - lo + 1).max(1);
    let val = lo + (seed as i64 % range);
    ret!(rv, Value::long(val));
}

fn fn_random_int(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let min = arg_long!(ed, 0);
    let max = arg_long!(ed, 1);
    if min > max {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "random_int(): Argument #1 ($min) must be less than or equal to argument #2 ($max)",
        ));
        return Ok(());
    }

    let range = (max as i128 - min as i128 + 1) as u128;
    let sample_space = 1u128 << 64;
    let accepted = sample_space - sample_space % range;
    let mut source = match std::fs::File::open("/dev/urandom") {
        Ok(source) => source,
        Err(_) => {
            eg.exception = Some(crate::value::make_error_value(
                "RuntimeException",
                "random_int(): Unable to read from the system random source",
            ));
            return Ok(());
        }
    };
    loop {
        let mut bytes = [0u8; 8];
        if source.read_exact(&mut bytes).is_err() {
            eg.exception = Some(crate::value::make_error_value(
                "RuntimeException",
                "random_int(): Unable to read from the system random source",
            ));
            return Ok(());
        }
        let sample = u64::from_ne_bytes(bytes) as u128;
        if sample < accepted {
            let value = min as i128 + (sample % range) as i128;
            ret!(rv, Value::long(value as i64));
        }
    }
}

// ============================================================================
// Output functions
// ============================================================================

fn initialize_lazy_output_value(eg: &mut ExecutorGlobals, value: Value) -> Result<Value, VmError> {
    if eg.lazy_object_state(&value).is_some() {
        reflection::resolve_lazy_object_chain(eg, &value)
    } else {
        Ok(value)
    }
}

fn fn_var_dump(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first_value = arg!(ed, 0).clone();
    let remaining = arg!(ed, 1)
        .as_array()
        .map(|arguments| arguments.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let first = var_dump_output_value(&first_value, eg, ed)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if first_value.is_binary_string() {
        let bytes = first_value.php_string_bytes().unwrap_or_default();
        eg.write_output(format!("string({}) \"", bytes.len()).as_bytes());
        eg.write_output(&bytes);
        eg.write_output(b"\"\n");
    } else if first_value
        .as_array()
        .is_some_and(PhpArray::has_external_byte_keys)
    {
        eg.write_output(&php_string_to_bytes(&first));
    } else {
        eg.write_output(first.as_bytes());
    }
    for value in remaining {
        let output = var_dump_output_value(&value, eg, ed)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        if value.is_binary_string() {
            let bytes = value.php_string_bytes().unwrap_or_default();
            eg.write_output(format!("string({}) \"", bytes.len()).as_bytes());
            eg.write_output(&bytes);
            eg.write_output(b"\"\n");
        } else if value
            .as_array()
            .is_some_and(PhpArray::has_external_byte_keys)
        {
            eg.write_output(&php_string_to_bytes(&output));
        } else {
            eg.write_output(output.as_bytes());
        }
    }
    Ok(())
}

fn debug_zval_dump_value(
    value: &Value,
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
) -> Result<String, VmError> {
    dump_output_value(value, eg, ed, DumpContext::debug(ed))
}

fn fn_debug_zval_dump(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = debug_zval_dump_value(arg!(ed, 0), eg, ed)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    eg.write_output(first.as_bytes());
    if let Some(remaining) = arg!(ed, 1).as_array() {
        for value in remaining.values() {
            let output = debug_zval_dump_value(value, eg, ed)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            eg.write_output(output.as_bytes());
        }
    }
    Ok(())
}

fn fn_print_r(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let output = print_r_value(v, 0, eg);
    if arg_opt!(ed, 1).is_some_and(Value::is_truthy) {
        ret!(rv, php_byte_result(output, false));
    }
    eg.write_output(&output);
    ret!(rv, Value::bool(true));
}

fn fn_var_export(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = initialize_lazy_output_value(eg, arg!(ed, 0).clone())?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let return_str = match arg_opt!(ed, 1) {
        Some(v) => v.is_truthy(),
        None => false,
    };
    let mut state = VarExportState::default();
    let output = var_export_value(&v, eg, &mut state);
    for _ in 0..state.recursive_values {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "var_export does not handle circular references",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    if return_str {
        ret!(rv, Value::string(output));
    } else {
        eg.write_output(output.as_bytes());
        ret!(rv, Value::null());
    }
}

fn spl_object_handle_argument(
    function: &str,
    value: &Value,
    eg: &mut ExecutorGlobals,
) -> Option<u32> {
    let value = value.dereferenced();
    let Some(handle) = value.object_handle() else {
        let actual = match value.value_type() {
            ValueType::True => "true".to_string(),
            ValueType::False => "false".to_string(),
            _ => value.diagnostic_type_name().into_owned(),
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function}(): Argument #1 ($object) must be of type object, {actual} given"),
        ));
        return None;
    };
    Some(handle)
}

fn fn_spl_object_id(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(handle) = spl_object_handle_argument("spl_object_id", arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, Value::long(i64::from(handle)));
}

fn fn_spl_object_hash(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(handle) = spl_object_handle_argument("spl_object_hash", arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, Value::string(format!("{handle:016x}0000000000000000")));
}

// ============================================================================
// Constant functions
// ============================================================================

fn fn_define(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if reject_strict_internal_string(eg, ed, arg!(ed, 0), "define", "constant_name") {
        return Ok(());
    }
    let name_value = arg!(ed, 0).dereferenced();
    if matches!(
        name_value.value_type(),
        ValueType::Array | ValueType::Object | ValueType::Closure | ValueType::Resource
    ) {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "define(): Argument #1 ($constant_name) must be of type string, {} given",
                name_value.type_name()
            ),
        ));
        ret!(rv, Value::null());
    }
    if name_value.value_type() == ValueType::Null {
        report_internal_deprecation(
            eg,
            ed,
            "define(): Passing null to parameter #1 ($constant_name) of type string is deprecated",
        )?;
    }
    let name = arg_str!(ed, 0);
    let val = arg!(ed, 1).clone();
    if name == "__COMPILER_HALT_OFFSET__" || eg.find_constant(&name).is_some() {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &crate::runtime::constant_redefinition_message(&name),
        )?;
        ret!(rv, Value::bool(false));
    }
    let result = eg.define_constant(&name, val);
    ret!(rv, Value::bool(result.is_ok()));
}

fn fn_defined(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg!(ed, 0).dereferenced().value_type() == ValueType::Null {
        report_internal_deprecation(
            eg,
            ed,
            "defined(): Passing null to parameter #1 ($constant_name) of type string is deprecated",
        )?;
    }
    let name = arg_str!(ed, 0);
    if let Some((class_name, constant_name)) = name.split_once("::") {
        let exists = eg.find_class(class_name).is_some_and(|class| {
            !class.is_trait
                && (class
                    .constants
                    .iter()
                    .any(|definition| definition.name == constant_name)
                    || (class.is_enum
                        && class
                            .static_properties
                            .iter()
                            .any(|case| case.name == constant_name)))
        });
        ret!(rv, Value::bool(exists));
    }
    ret!(rv, Value::bool(eg.find_constant(&name).is_some()));
}

fn fn_constant(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    if name == "__COMPILER_HALT_OFFSET__" {
        let (file, _) = internal_call_source(ed);
        if let Some(offset) = eg.compiler_halt_offset(&file) {
            ret!(rv, Value::long(offset));
        }
    }
    if let Some(value) = eg.find_constant(&name) {
        let (file, line) = internal_call_source(ed);
        let use_site = reflection::DeprecatedUseSite {
            frame: ed,
            file,
            line,
        };
        reflection::report_deprecated_global_constant_use(&name, &use_site, eg)?;
        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
        ret!(rv, value);
    }
    if let Some((class_name, constant_name)) = name.split_once("::") {
        let resolved = eg.find_class(class_name).map(|class| {
            let trait_constant = class.is_trait
                && class
                    .constants
                    .iter()
                    .any(|definition| definition.name == constant_name);
            let definition = (!class.is_trait)
                .then(|| {
                    class
                        .constants
                        .iter()
                        .find(|definition| definition.name == constant_name)
                        .cloned()
                })
                .flatten();
            let case = (definition.is_none() && class.is_enum)
                .then(|| {
                    class
                        .static_properties
                        .iter()
                        .enumerate()
                        .find(|(_, case)| case.name == constant_name)
                        .map(|(index, case)| (index, case.clone()))
                })
                .flatten();
            (
                class.name.clone(),
                class.class_id,
                trait_constant,
                definition,
                case,
            )
        });
        if let Some((display_class, class_id, trait_constant, definition, case)) = resolved {
            if trait_constant {
                eg.exception = Some(crate::value::make_error_value(
                    "Error",
                    &format!(
                        "Cannot access trait constant {display_class}::{constant_name} directly"
                    ),
                ));
                ret!(rv, Value::null());
            }
            let (file, line) = internal_call_source(ed);
            let use_site = reflection::DeprecatedUseSite {
                frame: ed,
                file,
                line,
            };
            if let Some(definition) = definition {
                reflection::report_deprecated_class_constant_use(
                    &display_class,
                    &definition,
                    &use_site,
                    eg,
                )?;
                if eg.exception.is_some() {
                    ret!(rv, Value::null());
                }
                let value = if definition.value_is_deferred {
                    let Some(value) =
                        reflection::evaluate_deferred_class_constant_value(&definition, eg)?
                    else {
                        if let Some(exception) = eg.exception.as_ref() {
                            crate::vm::execute::attach_internal_constant_expression_trace(
                                exception, ed, eg,
                            );
                        }
                        ret!(rv, Value::null());
                    };
                    value
                } else {
                    definition.value
                };
                ret!(rv, value);
            }
            if let Some((case_index, case)) = case {
                reflection::report_deprecated_enum_case_use(&display_class, &case, &use_site, eg)?;
                if eg.exception.is_some() {
                    ret!(rv, Value::null());
                }
                if let Some(storage_slot) = eg.static_property_storage_slot(class_id, case_index)
                    && let Some(value) = eg.static_property_value(storage_slot).cloned()
                {
                    ret!(rv, value);
                }
            }
        }
    }
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        &format!("Undefined constant \"{name}\""),
    ));
    ret!(rv, Value::null());
}

// ============================================================================
// JSON functions
// ============================================================================

const JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG: i64 = 512;
const JSON_PRESERVE_ZERO_FRACTION_FLAG: i64 = 1024;
const JSON_THROW_ON_ERROR_FLAG: i64 = 4_194_304;

const JSON_ERROR_NONE: i64 = 0;
const JSON_ERROR_RECURSION: i64 = 6;
const JSON_ERROR_INF_OR_NAN: i64 = 7;
const JSON_ERROR_NON_BACKED_ENUM: i64 = 11;

fn json_error_message(code: i64) -> &'static str {
    match code {
        0 => "No error",
        1 => "Maximum stack depth exceeded",
        2 => "State mismatch (invalid or malformed JSON)",
        3 => "Control character error, possibly incorrectly encoded",
        4 => "Syntax error",
        5 => "Malformed UTF-8 characters, possibly incorrectly encoded",
        6 => "Recursion detected",
        7 => "Inf and NaN cannot be JSON encoded",
        8 => "Type is not supported",
        9 => "The decoded property name is invalid",
        10 => "Single unpaired UTF-16 surrogate in unicode escape",
        11 => "Non-backed enums have no default serialization",
        _ => "Unknown error",
    }
}

fn make_json_exception(code: i64) -> Value {
    let exception = crate::value::make_error_value("JsonException", json_error_message(code));
    if let Some(mut object) = exception.as_object_mut() {
        object.set_property("code", Value::long(code));
    }
    exception
}

fn fn_json_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let value = initialize_lazy_output_value(eg, arg!(ed, 0).clone())?;
    if eg.exception.is_some() {
        if flags & JSON_THROW_ON_ERROR_FLAG == 0 || flags & JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG != 0 {
            eg.set_json_last_error(JSON_ERROR_NONE);
        }
        ret!(rv, Value::bool(false));
    }
    let encoded = json_encode_value(&value, flags, eg)?;
    if eg.exception.is_some() {
        if flags & JSON_THROW_ON_ERROR_FLAG == 0 || flags & JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG != 0 {
            eg.set_json_last_error(encoded.error_code);
        }
        ret!(rv, Value::bool(false));
    }
    if encoded.error_code != JSON_ERROR_NONE {
        if flags & JSON_THROW_ON_ERROR_FLAG != 0 && flags & JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG == 0 {
            eg.exception = Some(make_json_exception(encoded.error_code));
            ret!(rv, Value::bool(false));
        }
        eg.set_json_last_error(encoded.error_code);
        if flags & JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG == 0 {
            ret!(rv, Value::bool(false));
        }
    } else if flags & JSON_THROW_ON_ERROR_FLAG == 0
        || flags & JSON_PARTIAL_OUTPUT_ON_ERROR_FLAG != 0
    {
        eg.set_json_last_error(JSON_ERROR_NONE);
    }
    ret!(rv, Value::string(encoded.output));
}

fn fn_json_last_error(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(eg.json_last_error()));
}

fn fn_json_last_error_msg(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(json_error_message(eg.json_last_error())));
}

fn fn_json_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let assoc = match arg_opt!(ed, 1) {
        Some(v) => v.is_truthy(),
        None => false,
    };
    ret!(rv, json_decode_string(&s, assoc));
}

// ============================================================================
// Misc functions
// ============================================================================

fn fn_isset_func(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    ret!(
        rv,
        Value::bool(v.value_type() != ValueType::Null && v.value_type() != ValueType::Undef)
    );
}

fn fn_empty_func(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(!arg!(ed, 0).is_truthy()));
}

fn fn_unset_func(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    unsafe {
        std::ptr::drop_in_place(ptr);
        ptr.write(Value::null());
    }
    ret!(rv, Value::null());
}

fn invalid_handler_callback_detail(callback: &Value, eg: &ExecutorGlobals) -> String {
    if let Some(name) = callback.as_str() {
        if let Some((class, _)) = name.rsplit_once("::")
            && find_class_case_insensitive(eg, class.trim_start_matches('\\')).is_none()
        {
            return format!("class \"{}\" not found", class.trim_start_matches('\\'));
        }
        return format!("function \"{name}\" not found or invalid function name");
    }
    if let Some(array) = callback.as_array() {
        if let Some(class) = array.get_value_at(0).and_then(Value::as_str)
            && find_class_case_insensitive(eg, class.trim_start_matches('\\')).is_none()
        {
            return format!("class \"{}\" not found", class.trim_start_matches('\\'));
        }
        return "first array member is not a valid class name or object".to_string();
    }
    format!("{} given", callback.diagnostic_type_name())
}

fn validated_handler_callback(
    function: &str,
    callback: Option<&Value>,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Option<Option<Value>> {
    let callback = callback.map(Value::dereferenced);
    let Some(callback) = callback.filter(|value| value.value_type() != ValueType::Null) else {
        return Some(None);
    };
    if resolve_callback_at_callsite(callback, eg, ed).is_none() {
        let detail = invalid_handler_callback_detail(callback, eg);
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): Argument #1 ($callback) must be a valid callback or null, {detail}"
            ),
        ));
        return None;
    }
    Some(Some(callback.clone()))
}

fn fn_set_error_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(handler) = validated_handler_callback("set_error_handler", arg_opt!(ed, 0), ed, eg)
    else {
        return Ok(());
    };
    let levels = if arg_opt!(ed, 1).is_some() {
        let Some(levels) =
            typed_internal_int_argument(ed, eg, "set_error_handler", 1, "error_levels")?
        else {
            return Ok(());
        };
        levels
    } else {
        crate::PHP_E_ALL
    };
    let previous = eg.error_handler.clone().unwrap_or_else(Value::null);
    eg.error_handler_stack
        .push((eg.error_handler.take(), eg.error_handler_levels));
    eg.error_handler = handler;
    eg.error_handler_levels = levels;
    ret!(rv, previous);
}

fn fn_restore_error_handler(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let restored = eg.error_handler_stack.pop();
    if let Some((handler, levels)) = restored {
        eg.error_handler = handler;
        eg.error_handler_levels = levels;
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

fn fn_get_error_handler(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, eg.error_handler.clone().unwrap_or_else(Value::null));
}

fn fn_error_get_last(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(error) = eg.last_error.as_ref() else {
        ret!(rv, Value::null());
    };
    let mut result = PhpArray::with_hash_capacity(4);
    result.set_str("type", Value::long(error.level));
    result.set_str("message", Value::string(error.message.clone()));
    result.set_str("file", Value::string(error.file.clone()));
    result.set_str("line", Value::long(error.line as i64));
    ret!(rv, Value::array(result));
}

fn fn_error_clear_last(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.last_error = None;
    ret!(rv, Value::null());
}

const E_USER_ERROR: i64 = 256;
const E_USER_WARNING: i64 = 512;
const E_USER_NOTICE: i64 = 1024;
const E_USER_DEPRECATED: i64 = 16_384;

/// Route a recoverable PHP diagnostic through the request-local user handler.
/// Only a strict `false` result declines the diagnostic; null and every other
/// return value count as handled, matching PHP 8.2. A handler is never entered
/// recursively, while its own diagnostics remain eligible for the standard
/// reporting path.
pub(crate) fn dispatch_php_error(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    level: i64,
    message: &str,
    file: &str,
    line: usize,
) -> Result<bool, VmError> {
    if eg.handling_error || level & eg.error_handler_levels == 0 {
        return Ok(false);
    }
    let Some(callback) = eg.error_handler.clone() else {
        return Ok(false);
    };
    let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) else {
        return Ok(false);
    };

    eg.handling_error = true;
    let result = call_resolved_with_values_from(
        eg,
        &resolved,
        &[
            Value::long(level),
            Value::string(message.to_string()),
            Value::string(file.to_string()),
            Value::long(line as i64),
        ],
        ed,
        file,
        line,
        false,
    );
    eg.handling_error = false;
    // SAFETY: `ed` is the suspended active call frame supplied to this
    // synchronous internal handler and remains live across the callback.
    let frame = unsafe { &mut *ed };
    crate::vm::execute::sync_dirty_globals_to_frame(eg, frame);
    let result = result?;
    Ok(eg.exception.is_some() || result.value_type() != ValueType::False)
}

/// Enter the request's uncaught-exception callback through PHP's synthetic
/// internal call boundary. The active handler is removed before invocation so
/// `get_exception_handler()` returns null inside it and a replacement
/// exception cannot recursively re-enter the same callback.
pub(crate) fn dispatch_uncaught_exception_handler(
    eg: &mut ExecutorGlobals,
    caller: *mut ExecuteData,
    exception: &Value,
) -> Result<bool, VmError> {
    let Some(callback) = eg.exception_handler.take() else {
        return Ok(false);
    };
    let Some(resolved) = resolve_callback_with_cache(&callback, eg, None, None) else {
        return Ok(false);
    };
    call_resolved_with_values_from(
        eg,
        &resolved,
        std::slice::from_ref(exception),
        caller,
        "Unknown",
        0,
        false,
    )?;
    Ok(eg.exception.is_none())
}

fn internal_call_source(ed: *mut ExecuteData) -> (String, usize) {
    // SAFETY: an internal handler executes synchronously beneath its live
    // caller. The caller opline has advanced one instruction past DoFcall.
    unsafe {
        let caller = (*ed).prev_execute_data;
        if caller.is_null() || (*caller).func.is_null() {
            return (String::new(), 0);
        }
        let function = Function::from_common_ptr((*caller).func);
        if function.fn_type() != FunctionType::User {
            return (String::new(), 0);
        }
        let op_array = &function.as_user().op_array;
        let file = if op_array.source_file.is_empty() {
            op_array.name.clone()
        } else {
            op_array.source_file.to_string()
        };
        let next = (*caller).opline.offset_from(op_array.instructions.as_ptr());
        let line = usize::try_from(next)
            .ok()
            .and_then(|next| {
                op_array
                    .source_lines
                    .iter()
                    .rev()
                    .find(|(index, _)| *index <= next as u32)
                    .map(|(_, line)| *line as usize)
            })
            .unwrap_or(0);
        (file, line)
    }
}

#[inline]
fn with_internal_trace_origin<T>(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    action: impl FnOnce(&mut ExecutorGlobals) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let publish = eg.detached_trace_origin(ed as usize).is_none();
    if publish {
        let (file, line) = internal_call_source(ed);
        if !file.is_empty() && line != 0 {
            eg.publish_detached_trace_origin(ed as usize, file, line);
        }
    }
    let result = action(eg);
    if publish {
        eg.discard_detached_trace_origin(ed as usize);
    }
    result
}

fn internal_call_is_strict(ed: *mut ExecuteData) -> bool {
    // SAFETY: an internal handler executes synchronously beneath its live
    // caller, whose function header remains valid for the duration of the call.
    unsafe {
        if (*ed).is_detached_strict_call() {
            return true;
        }
        let caller = (*ed).prev_execute_data;
        if caller.is_null() || (*caller).func.is_null() {
            return false;
        }
        let function = Function::from_common_ptr((*caller).func);
        function.fn_type() == FunctionType::User && function.as_user().op_array.strict_types
    }
}

fn reject_strict_internal_string(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    argument: &Value,
    function: &str,
    parameter: &str,
) -> bool {
    let argument = argument.dereferenced();
    if !internal_call_is_strict(ed) || argument.value_type() == ValueType::String {
        return false;
    }
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #1 (${parameter}) must be of type string, {} given",
            argument.type_name()
        ),
    ));
    true
}

fn report_internal_deprecation(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    message: &str,
) -> Result<(), VmError> {
    report_internal_diagnostic(eg, ed, 8192, "Deprecated", message).map(|_| ())
}

fn report_internal_diagnostic(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    level: i64,
    label: &str,
    message: &str,
) -> Result<bool, VmError> {
    let (file, line) = internal_call_source(ed);
    let handled = dispatch_php_error(eg, ed, level, message, &file, line)?;
    if !handled {
        eg.record_last_error(level, message, &file, line);
    }
    if !handled && eg.error_reporting & level != 0 {
        eg.write_output(format!("\n{label}: {message} in {file} on line {line}\n").as_bytes());
    }
    Ok(handled)
}

/// Raise one of PHP's user-generated diagnostics. Eligible handlers receive
/// the physical callsite; unhandled recoverable levels use the same metadata
/// for PHP's standard diagnostic output.
fn fn_trigger_error(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let message = arg_str!(ed, 0).into_owned();
    let level = arg_opt!(ed, 1).map_or(E_USER_NOTICE, Value::to_long_val);
    if !matches!(
        level,
        E_USER_ERROR | E_USER_WARNING | E_USER_NOTICE | E_USER_DEPRECATED
    ) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "trigger_error(): Argument #2 ($error_level) must be one of E_USER_ERROR, E_USER_WARNING, E_USER_NOTICE, or E_USER_DEPRECATED",
        ));
        return Ok(());
    }

    if level == E_USER_ERROR {
        report_internal_deprecation(
            eg,
            ed,
            "Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let (file, line) = internal_call_source(ed);
        if dispatch_php_error(eg, ed, level, &message, &file, line)? {
            ret!(rv, Value::bool(true));
        }
        eg.record_last_error(level, &message, &file, line);
        return Err(VmError::Fatal(format!(
            "{message} in {file} on line {line}"
        )));
    }
    let label = match level {
        E_USER_WARNING => "Warning",
        E_USER_NOTICE => "Notice",
        E_USER_DEPRECATED => "Deprecated",
        _ => unreachable!(),
    };
    report_internal_diagnostic(eg, ed, level, label, &message)?;
    ret!(rv, Value::bool(true));
}

fn fn_set_exception_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(handler) =
        validated_handler_callback("set_exception_handler", arg_opt!(ed, 0), ed, eg)
    else {
        return Ok(());
    };
    let previous = eg.exception_handler.clone().unwrap_or_else(Value::null);
    eg.exception_handler_stack.push(eg.exception_handler.take());
    eg.exception_handler = handler;
    ret!(rv, previous);
}

fn fn_restore_exception_handler(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let restored = eg.exception_handler_stack.pop();
    if let Some(handler) = restored {
        eg.exception_handler = handler;
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

fn fn_get_exception_handler(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, eg.exception_handler.clone().unwrap_or_else(Value::null));
}

fn fn_register_shutdown_function(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let Some(resolved) = resolve_callback_at_callsite_checked(callback, eg, ed)? else {
        if eg.exception.is_some() {
            return Ok(());
        }
        let detail = if callback.as_str().is_some() || callback.as_array().is_some() {
            invalid_handler_callback_detail(callback, eg)
        } else {
            "no array or string given".to_string()
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "register_shutdown_function(): Argument #1 ($callback) must be a valid callback, {detail}"
            ),
        ));
        return Ok(());
    };
    let arguments = arg_opt!(ed, 1)
        .and_then(Value::as_array)
        .map(|arguments| arguments.values().cloned().collect())
        .unwrap_or_default();
    eg.shutdown_functions
        .get_or_insert_with(|| Box::new(std::collections::VecDeque::new()))
        .push_back(ShutdownFunction {
            callback: resolved,
            arguments,
        });
    ret!(rv, Value::null());
}

/// Execute the request-local FIFO while allowing a callback to append another
/// shutdown callback. An escaping VM error or Throwable stops the remaining
/// callbacks, as it terminates PHP's request-shutdown function phase.
pub fn run_shutdown_functions(
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
) -> Result<(), VmError> {
    let mut release_roots = Vec::new();
    let drain_pending_roots = |eg: &mut ExecutorGlobals, release_roots: &mut Vec<Value>| {
        if let Some(mut pending) = eg.shutdown_functions.take() {
            while let Some(callback) = pending.pop_front() {
                release_roots.extend(callback.into_release_roots());
            }
        }
    };
    loop {
        let next = eg
            .shutdown_functions
            .as_deref_mut()
            .and_then(std::collections::VecDeque::pop_front);
        let Some(next) = next else {
            eg.shutdown_functions = None;
            crate::vm::execute::run_value_destructors(eg, &release_roots, logical_caller)?;
            return Ok(());
        };
        let result = call_resolved_with_values_from(
            eg,
            &next.callback,
            &next.arguments,
            logical_caller,
            "Unknown",
            0,
            true,
        );
        release_roots.extend(next.into_release_roots());
        if let Err(error) = result {
            drain_pending_roots(eg, &mut release_roots);
            crate::vm::execute::run_value_destructors(eg, &release_roots, logical_caller)?;
            return Err(error);
        }
        if let Some(exception) = eg.exception.take() {
            match dispatch_uncaught_exception_handler(eg, logical_caller, &exception) {
                Ok(true) => continue,
                Ok(false) => {
                    if eg.exception.is_none() {
                        eg.exception = Some(exception);
                    }
                }
                Err(error) => {
                    drain_pending_roots(eg, &mut release_roots);
                    crate::vm::execute::run_value_destructors(eg, &release_roots, logical_caller)?;
                    return Err(error);
                }
            }
            let exception = eg
                .exception
                .take()
                .expect("unhandled shutdown exception must remain pending");
            drain_pending_roots(eg, &mut release_roots);
            crate::vm::execute::run_value_destructors(eg, &release_roots, logical_caller)?;
            return Err(VmError::Fatal(
                crate::vm::execute::format_uncaught_throwable(eg, &exception),
            ));
        }
    }
}

fn fn_error_reporting(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = eg.error_reporting;
    let argument = arg_opt!(ed, 0).map(Value::dereferenced);
    if argument.is_some_and(|value| value.value_type() != ValueType::Null) {
        let Some(level) = typed_internal_int_argument_expected(
            ed,
            eg,
            "error_reporting",
            0,
            "error_level",
            "?int",
        )?
        else {
            return Ok(());
        };
        eg.set_error_reporting(level);
    }
    ret!(rv, Value::long(previous));
}

/// Write the default error-log destination to the process diagnostic stream.
/// File/mail destinations remain outside the current compatibility envelope.
fn fn_error_log(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let message = arg_str!(ed, 0);
    let message_type = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    if message_type != 0 {
        ret!(rv, Value::bool(false));
    }
    eprintln!("{message}");
    ret!(rv, Value::bool(true));
}

fn fn_flush(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.flush_output();
    ret!(rv, Value::null());
}

const OUTPUT_HANDLER_START: i64 = 1;
const OUTPUT_HANDLER_CLEAN: i64 = 2;
const OUTPUT_HANDLER_FLUSH: i64 = 4;
const OUTPUT_HANDLER_FINAL: i64 = 8;
const OUTPUT_HANDLER_CLEANABLE: i64 = 16;
const OUTPUT_HANDLER_FLUSHABLE: i64 = 32;
const OUTPUT_HANDLER_REMOVABLE: i64 = 64;
const OUTPUT_HANDLER_DEFAULT_FLAGS: i64 =
    OUTPUT_HANDLER_CLEANABLE | OUTPUT_HANDLER_FLUSHABLE | OUTPUT_HANDLER_REMOVABLE;

fn fn_ob_start(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let handler = arg_opt!(ed, 0).cloned().and_then(|callback| {
        (!matches!(callback.value_type(), ValueType::Null | ValueType::False)).then_some(callback)
    });
    if handler
        .as_ref()
        .is_some_and(|callback| resolve_callback_at_callsite(callback, eg, ed).is_none())
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "ob_start(): Argument #1 ($callback) must be a valid callback",
        ));
        return Ok(());
    }
    // Keep accepting PHP's chunk-size argument. Automatic chunk flushing is
    // deliberately deferred; explicit operations and request teardown are exact.
    let _chunk_size = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let flags = arg_opt!(ed, 2).map_or(OUTPUT_HANDLER_DEFAULT_FLAGS, Value::to_long_val);
    eg.push_output_buffer(handler, flags);
    ret!(rv, Value::bool(true));
}

fn fn_ob_get_level(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(eg.output_buffer_level() as i64));
}

fn fn_ob_get_contents(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(contents) = eg.output_buffer_contents() else {
        ret!(rv, Value::bool(false));
    };
    ret!(rv, php_byte_result(contents, false));
}

fn fn_ob_get_length(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(contents) = eg.output_buffer_contents() else {
        ret!(rv, Value::bool(false));
    };
    ret!(rv, Value::long(contents.len() as i64));
}

fn transform_output_buffer(
    eg: &mut ExecutorGlobals,
    buffer: &mut crate::runtime::OutputBuffer,
    operation: i64,
    caller: Option<*mut ExecuteData>,
) -> Result<Vec<u8>, VmError> {
    let raw = std::mem::take(&mut buffer.data);
    let mode = operation
        | if buffer.started {
            0
        } else {
            OUTPUT_HANDLER_START
        };
    buffer.started = true;
    let Some(callback) = buffer.handler.as_ref() else {
        return Ok(raw);
    };
    let resolved = caller
        .and_then(|ed| resolve_callback_at_callsite(callback, eg, ed))
        .or_else(|| resolve_callback_with_cache(callback, eg, None, None));
    let Some(resolved) = resolved else {
        return Ok(raw);
    };
    let arguments = [Value::string(bytes_to_php_string(&raw)), Value::long(mode)];
    let previous_handler_depth = eg.enter_output_handler();
    let transformed = call_resolved_with_values(eg, &resolved, &arguments);
    eg.leave_output_handler(previous_handler_depth);
    let transformed = transformed?;
    if transformed.value_type() == ValueType::False {
        Ok(raw)
    } else {
        Ok(transformed.echo_to_string().into_bytes())
    }
}

fn clean_output_buffer(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    remove: bool,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(mut buffer) = eg.pop_output_buffer() else {
        return Ok(None);
    };
    let required = OUTPUT_HANDLER_CLEANABLE | if remove { OUTPUT_HANDLER_REMOVABLE } else { 0 };
    if buffer.flags & required != required {
        eg.restore_output_buffer(buffer);
        return Ok(None);
    }
    let raw = buffer.data.clone();
    let operation = OUTPUT_HANDLER_CLEAN | if remove { OUTPUT_HANDLER_FINAL } else { 0 };
    let result = transform_output_buffer(eg, &mut buffer, operation, Some(ed));
    if !remove {
        eg.restore_output_buffer(buffer);
    }
    result.map(|_| Some(raw))
}

fn flush_output_buffer(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    remove: bool,
) -> Result<Option<Vec<u8>>, VmError> {
    let Some(mut buffer) = eg.pop_output_buffer() else {
        return Ok(None);
    };
    let required = OUTPUT_HANDLER_FLUSHABLE | if remove { OUTPUT_HANDLER_REMOVABLE } else { 0 };
    if buffer.flags & required != required {
        eg.restore_output_buffer(buffer);
        return Ok(None);
    }
    let raw = buffer.data.clone();
    let operation = if remove {
        OUTPUT_HANDLER_FINAL
    } else {
        OUTPUT_HANDLER_FLUSH
    };
    let transformed = transform_output_buffer(eg, &mut buffer, operation, Some(ed));
    match transformed {
        Ok(output) => {
            eg.write_output(&output);
            if !remove {
                eg.restore_output_buffer(buffer);
            }
            Ok(Some(raw))
        }
        Err(error) => {
            if !remove {
                eg.restore_output_buffer(buffer);
            }
            Err(error)
        }
    }
}

fn fn_ob_get_clean(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(raw) = eg.output_buffer_contents() else {
        ret!(rv, Value::bool(false));
    };
    let _ = clean_output_buffer(eg, ed, true)?;
    ret!(rv, php_byte_result(raw, false));
}

fn fn_ob_get_flush(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(raw) = eg.output_buffer_contents() else {
        ret!(rv, Value::bool(false));
    };
    let _ = flush_output_buffer(eg, ed, true)?;
    ret!(rv, php_byte_result(raw, false));
}

fn fn_ob_clean(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(clean_output_buffer(eg, ed, false)?.is_some())
    );
}

fn fn_ob_flush(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(flush_output_buffer(eg, ed, false)?.is_some())
    );
}

fn fn_ob_end_clean(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(clean_output_buffer(eg, ed, true)?.is_some())
    );
}

fn fn_ob_end_flush(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(flush_output_buffer(eg, ed, true)?.is_some())
    );
}

pub(crate) fn flush_all_output_buffers(eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    while let Some(mut buffer) = eg.pop_output_buffer() {
        // PHP keeps the output subsystem locked until the final handler and
        // the handler value's captured roots have both been released. A
        // destructor reached from those captures must therefore observe the
        // display-handler re-entry guard.
        let previous_handler_depth = eg.enter_output_handler();
        let transformed = transform_output_buffer(eg, &mut buffer, OUTPUT_HANDLER_FINAL, None);
        let handler = buffer.handler.take();
        let destructor_result = handler.as_ref().map_or(Ok(()), |handler| {
            crate::vm::execute::run_value_destructors(
                eg,
                std::slice::from_ref(handler),
                eg.current_execute_data.get(),
            )
        });
        drop(handler);
        drop(buffer);
        eg.leave_output_handler(previous_handler_depth);
        let output = match transformed {
            Ok(output) => output,
            Err(error) => {
                eg.flush_output();
                return Err(error);
            }
        };
        eg.write_output(&output);
        if let Err(error) = destructor_result {
            eg.flush_output();
            return Err(error);
        }
    }
    Ok(())
}

fn fn_gc_mem_caches(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // RPHP does not retain Zend MM free-list caches between these calls.
    // PHP reports the number of bytes released, hence zero is exact here.
    ret!(rv, Value::long(0));
}

fn caller_argument(ed: *mut ExecuteData, index: u32, eg: &ExecutorGlobals) -> Option<Value> {
    // SAFETY: internal handlers receive a live frame whose predecessor remains
    // live for the handler call. The argument count bounds every CV selected
    // below, including the raw pre-pack variadic tail.
    unsafe {
        let caller = (*ed).prev_execute_data;
        if caller.is_null() || index >= (*caller).num_args {
            return None;
        }
        let function = &*(*caller).func;
        if index >= function.sig.public_arity()
            && let Some(arguments) = eg.function_arguments.get(&(caller as usize))
        {
            return arguments.get(index as usize).cloned();
        }
        let value = if function.sig.is_variadic && index >= function.sig.public_arity() {
            let offset = index - function.sig.public_arity();
            return (*caller)
                .cv(function.sig.variadic_cv_index)
                .as_array()
                .and_then(|arguments| arguments.get_value_at(offset as usize))
                .map(live_argument_value);
        } else {
            (*caller).cv(function.sig.param_cv_index(index))
        };
        Some(live_argument_value(value))
    }
}

fn live_argument_value(value: &Value) -> Value {
    let value = value.dereferenced();
    if value.is_undef() {
        Value::null()
    } else {
        value.clone()
    }
}

fn caller_function_frame(ed: *mut ExecuteData) -> Option<*mut ExecuteData> {
    // SAFETY: `ed` is the live internal-function frame for this handler; its
    // predecessor, when non-null, remains allocated until the handler returns.
    unsafe {
        let caller = (*ed).prev_execute_data;
        if caller.is_null() || (*caller).op_array().is_main_script() {
            None
        } else {
            Some(caller)
        }
    }
}

fn caller_num_args(ed: *mut ExecuteData) -> Option<u32> {
    caller_function_frame(ed).map(|caller| unsafe { (*caller).num_args })
}

fn fn_func_num_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(count) = caller_num_args(ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "func_num_args() must be called from a function context",
        ));
        return Ok(());
    };
    ret!(rv, Value::long(i64::from(count)));
}

fn fn_func_get_arg(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(count) = caller_num_args(ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "func_get_arg() cannot be called from the global scope",
        ));
        return Ok(());
    };
    let index = arg!(ed, 0).to_long_val();
    if index < 0 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "func_get_arg(): Argument #1 ($position) must be greater than or equal to 0",
        ));
        return Ok(());
    }
    let index = index as u32;
    if index >= count {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "func_get_arg(): Argument #1 ($position) must be less than the number of the arguments passed to the currently executed function",
        ));
        return Ok(());
    }
    ret!(
        rv,
        caller_argument(ed, index, eg).unwrap_or_else(|| Value::bool(false))
    );
}

fn fn_func_get_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(count) = caller_num_args(ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "func_get_args() cannot be called from the global scope",
        ));
        return Ok(());
    };
    let mut arguments = PhpArray::with_packed_capacity(count as usize);
    for index in 0..count {
        arguments.push(caller_argument(ed, index, eg).unwrap_or_else(Value::null));
    }
    ret!(rv, Value::array(arguments));
}

const EXTR_SKIP: i64 = 1;
const EXTR_PREFIX_SAME: i64 = 2;
const EXTR_PREFIX_ALL: i64 = 3;
const EXTR_PREFIX_INVALID: i64 = 4;
const EXTR_PREFIX_IF_EXISTS: i64 = 5;
const EXTR_IF_EXISTS: i64 = 6;
const EXTR_REFS: i64 = 256;

fn fn_extract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = if let Some(flags) = arg_opt!(ed, 1).and_then(Value::as_long) {
        flags
    } else if arg_opt!(ed, 1).is_some() {
        let Some(flags) = typed_internal_int_argument(ed, eg, "extract", 1, "flags")? else {
            return Ok(());
        };
        flags
    } else {
        0
    };
    let mode = flags & 0xff;
    if !matches!(mode, 0..=6) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "extract(): Argument #2 ($flags) must be a valid extract type",
        ));
        return Ok(());
    }
    let prefix_supplied = arg_opt!(ed, 2).is_some();
    if !prefix_supplied
        && matches!(
            mode,
            EXTR_PREFIX_SAME | EXTR_PREFIX_ALL | EXTR_PREFIX_INVALID | EXTR_PREFIX_IF_EXISTS
        )
    {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "extract(): Argument #3 ($prefix) is required when using this extract type",
        ));
        return Ok(());
    }
    let prefix = if prefix_supplied {
        let Some(prefix) = typed_internal_string_argument(ed, eg, "extract", 2, "prefix")? else {
            return Ok(());
        };
        prefix
    } else {
        String::new()
    };
    if !prefix.is_empty() && !valid_variable_name(&prefix) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "extract(): Argument #3 ($prefix) must be a valid identifier",
        ));
        return Ok(());
    }
    let may_write_indirect_target = matches!(
        mode,
        EXTR_PREFIX_SAME | EXTR_PREFIX_ALL | EXTR_PREFIX_INVALID | EXTR_PREFIX_IF_EXISTS
    );

    let array_pointer = arg_mut!(ed, 0);
    // SAFETY: CV(0) and the pointer returned by `arg_mut!` are the live
    // argument slot and its synchronously borrowed PHP value, respectively.
    let (array, source_has_external_alias) = unsafe {
        let source_has_external_alias =
            may_write_indirect_target && extract_source_has_external_alias((*ed).cv(0));
        (
            (&mut *array_pointer).as_array_mut(),
            source_has_external_alias,
        )
    };
    let Some(array) = array else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "extract(): Argument #1 ($array) must be of type array",
        ));
        return Ok(());
    };
    if array.is_empty() {
        ret!(rv, Value::long(0));
    }
    let references = flags & EXTR_REFS != 0;
    let inspect_indirect_targets = source_has_external_alias
        || (may_write_indirect_target && crate::vm::execute::caller_scope_is_global(eg, ed));
    let Some((candidates, target_requires_snapshot)) =
        extract_candidates(ed, eg, array, &prefix, mode, inspect_indirect_targets)
    else {
        return Ok(());
    };
    // Prefix modes can write a sibling caller CV whose value owns or aliases
    // the source array. Materialize before the first scope write even when the
    // current target is scalar: the write itself may retire another candidate
    // through caller-slot aliasing.
    let requires_snapshot = target_requires_snapshot || may_write_indirect_target;
    let extracted = if references {
        extract_reference_candidates(ed, eg, array, candidates, requires_snapshot)
    } else {
        extract_value_candidates(ed, eg, array, candidates, requires_snapshot)
    };
    let Some(extracted) = extracted else {
        return Ok(());
    };
    ret!(rv, Value::long(extracted));
}

#[cold]
#[inline(never)]
fn extract_source_has_external_alias(argument: &Value) -> bool {
    argument.owned_reference_handle_count() > 2
}

#[inline(never)]
fn extract_value_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array: &PhpArray,
    candidates: Vec<(ArrayKey, String)>,
    requires_snapshot: bool,
) -> Option<i64> {
    if requires_snapshot {
        let mut materialized = Vec::with_capacity(candidates.len());
        for (key, name) in candidates {
            let value = match key {
                ArrayKey::Int(key) => array.get_int(key),
                ArrayKey::String(key) => array.get_str(&key),
            };
            let Some(value) = value else {
                debug_assert!(false, "extract candidate disappeared before scope writes");
                continue;
            };
            materialized.push((name, value.dereferenced().clone()));
        }
        return assign_extract_candidates(ed, eg, materialized, false);
    }

    assign_extract_value_candidates(ed, eg, array, candidates)
}

#[inline(never)]
fn extract_reference_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array: &mut PhpArray,
    candidates: Vec<(ArrayKey, String)>,
    requires_snapshot: bool,
) -> Option<i64> {
    if !requires_snapshot {
        return assign_extract_reference_candidates(ed, eg, array, candidates);
    }

    // A write may replace the caller variable which owns `array` (for example,
    // extracting the key "source" from `$source`). Materialize every result
    // before the first scope write so later candidates never borrow through a
    // value that the extraction itself has already destroyed. EXTR_REFS still
    // promotes the source entries in place and retains aliases to those cells.
    let mut materialized = Vec::with_capacity(candidates.len());
    for (key, name) in candidates {
        let value = match key {
            ArrayKey::Int(key) => array.get_int_mut(key),
            ArrayKey::String(key) => array.get_str_mut(&key),
        };
        let Some(value) = value else {
            debug_assert!(false, "extract candidate disappeared before scope writes");
            continue;
        };
        let extracted_value = if value.is_owned_reference() {
            value.clone_owned_reference_alias()
        } else {
            let owned = Value::owned_reference(value.dereferenced().clone());
            let alias = owned.clone_owned_reference_alias();
            *value = owned;
            alias
        };
        materialized.push((name, extracted_value));
    }
    assign_extract_candidates(ed, eg, materialized, true)
}

#[inline(never)]
fn assign_extract_value_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array: &PhpArray,
    candidates: Vec<(ArrayKey, String)>,
) -> Option<i64> {
    let mut extracted = 0;
    for (key, name) in candidates {
        let value = match key {
            ArrayKey::Int(key) => array.get_int(key),
            ArrayKey::String(key) => array.get_str(&key),
        };
        let Some(value) = value else {
            debug_assert!(false, "extract candidate disappeared before scope writes");
            continue;
        };
        if !assign_extract_candidate(ed, eg, &name, value.dereferenced().clone(), false)? {
            continue;
        }
        extracted += 1;
    }
    Some(extracted)
}

#[inline(never)]
fn assign_extract_reference_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array: &mut PhpArray,
    candidates: Vec<(ArrayKey, String)>,
) -> Option<i64> {
    let mut extracted = 0;
    for (key, name) in candidates {
        let value = match key {
            ArrayKey::Int(key) => array.get_int_mut(key),
            ArrayKey::String(key) => array.get_str_mut(&key),
        };
        let Some(value) = value else {
            debug_assert!(false, "extract candidate disappeared before scope writes");
            continue;
        };
        let extracted_value = if value.is_owned_reference() {
            value.clone_owned_reference_alias()
        } else {
            let owned = Value::owned_reference(value.dereferenced().clone());
            let alias = owned.clone_owned_reference_alias();
            *value = owned;
            alias
        };
        if !assign_extract_candidate(ed, eg, &name, extracted_value, true)? {
            continue;
        }
        extracted += 1;
    }
    Some(extracted)
}

#[inline(never)]
fn assign_extract_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    candidates: impl IntoIterator<Item = (String, Value)>,
    references: bool,
) -> Option<i64> {
    let mut extracted = 0;
    for (name, extracted_value) in candidates {
        if !assign_extract_candidate(ed, eg, &name, extracted_value, references)? {
            continue;
        }
        extracted += 1;
    }
    Some(extracted)
}

#[inline]
fn assign_extract_candidate(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    name: &str,
    value: Value,
    references: bool,
) -> Option<bool> {
    match crate::vm::execute::set_caller_scope_variable(eg, ed, name, value, references, false) {
        Ok(written) => Some(written),
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("TypeError", &message));
            None
        }
    }
}

#[inline(never)]
fn extract_candidates(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array: &PhpArray,
    prefix: &str,
    mode: i64,
    inspect_indirect_targets: bool,
) -> Option<(Vec<(ArrayKey, String)>, bool)> {
    let mut candidates = Vec::with_capacity(array.len());
    let mut requires_snapshot = false;
    for (key, _) in array.iter() {
        let raw_name = extract_key_name(&key);
        let candidate =
            extract_candidate_name(ed, eg, raw_name, prefix, mode, inspect_indirect_targets)?;
        let Some((name, target_requires_snapshot)) = candidate else {
            continue;
        };
        requires_snapshot |= target_requires_snapshot;
        candidates.push((key, name));
    }
    Some((candidates, requires_snapshot))
}

fn extract_key_name(key: &ArrayKey) -> String {
    match key {
        ArrayKey::Int(key) => key.to_string(),
        ArrayKey::String(key) => key.clone(),
    }
}

fn extract_candidate_name(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    raw_name: String,
    prefix: &str,
    mode: i64,
    inspect_indirect_targets: bool,
) -> Option<Option<(String, bool)>> {
    let valid = valid_variable_name(&raw_name);
    if mode == 0 {
        if raw_name == "this" {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "Cannot re-assign $this",
            ));
            return None;
        }
        if !valid {
            return Some(None);
        }
        let target = crate::vm::execute::caller_scope_variable(eg, ed, &raw_name);
        let target_requires_snapshot = extract_target_requires_snapshot(target.as_ref());
        return Some(Some((raw_name, target_requires_snapshot)));
    }
    if raw_name == "this" {
        return match mode {
            EXTR_SKIP | EXTR_PREFIX_IF_EXISTS | EXTR_IF_EXISTS => Some(None),
            EXTR_PREFIX_SAME | EXTR_PREFIX_ALL | EXTR_PREFIX_INVALID => {
                let name = format!("{prefix}_{raw_name}");
                let requires_snapshot = if inspect_indirect_targets {
                    let target = crate::vm::execute::caller_scope_variable(eg, ed, &name);
                    extract_target_requires_snapshot(target.as_ref())
                } else {
                    false
                };
                Some(Some((name, requires_snapshot)))
            }
            0 => unreachable!("EXTR_OVERWRITE returned through the fast path"),
            _ => unreachable!("extract mode was validated by the caller"),
        };
    }
    let inspected_raw_name = matches!(
        mode,
        EXTR_SKIP | EXTR_PREFIX_SAME | EXTR_PREFIX_IF_EXISTS | EXTR_IF_EXISTS
    );
    let existing = (valid && inspected_raw_name)
        .then(|| crate::vm::execute::caller_scope_variable(eg, ed, &raw_name))
        .flatten();
    let exists = existing.is_some();
    let name = match mode {
        EXTR_SKIP if exists => return Some(None),
        EXTR_PREFIX_SAME if exists => format!("{prefix}_{raw_name}"),
        EXTR_PREFIX_ALL if raw_name.is_empty() => return Some(None),
        EXTR_PREFIX_ALL => format!("{prefix}_{raw_name}"),
        EXTR_PREFIX_INVALID if !valid => format!("{prefix}_{raw_name}"),
        EXTR_PREFIX_IF_EXISTS if exists => format!("{prefix}_{raw_name}"),
        EXTR_PREFIX_IF_EXISTS | EXTR_IF_EXISTS if !exists => return Some(None),
        _ if !valid => return Some(None),
        _ => {
            let target = if inspected_raw_name {
                existing
            } else {
                crate::vm::execute::caller_scope_variable(eg, ed, &raw_name)
            };
            let target_requires_snapshot = extract_target_requires_snapshot(target.as_ref());
            return Some(Some((raw_name, target_requires_snapshot)));
        }
    };
    if !valid_variable_name(&name) {
        return Some(None);
    }
    let target_requires_snapshot = if inspect_indirect_targets {
        let target = crate::vm::execute::caller_scope_variable(eg, ed, &name);
        extract_target_requires_snapshot(target.as_ref())
    } else {
        false
    };
    Some(Some((name, target_requires_snapshot)))
}

#[inline]
fn extract_target_requires_snapshot(target: Option<&Value>) -> bool {
    let Some(target) = target else { return false };
    matches!(
        target.dereferenced().value_type(),
        ValueType::Array | ValueType::Object | ValueType::Closure
    )
}

fn valid_variable_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic() || !first.is_ascii())
        && characters.all(|character| {
            character == '_' || character.is_alphanumeric() || !character.is_ascii()
        })
}

fn fn_get_defined_vars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::array(crate::vm::execute::caller_scope_variables(eg, ed))
    );
}

fn trace_parameter_is_sensitive(
    user: Option<&UserFunction>,
    common: &FunctionCommon,
    index: u32,
) -> bool {
    let Some(user) = user else { return false };
    let parameter = if index < common.sig.public_arity() {
        Some(index as usize)
    } else if common.sig.is_variadic {
        Some(common.sig.public_arity() as usize)
    } else {
        None
    };
    parameter.is_some_and(|parameter| {
        user.parameter_attributes
            .get(parameter)
            .is_some_and(|attributes| {
                attributes
                    .iter()
                    .any(|attribute| attribute.name.eq_ignore_ascii_case("SensitiveParameter"))
            })
    })
}

fn redact_trace_argument(
    user: Option<&UserFunction>,
    common: &FunctionCommon,
    index: u32,
    argument: Value,
    eg: &ExecutorGlobals,
) -> Value {
    if trace_parameter_is_sensitive(user, common, index) {
        builtin_classes::sensitive_parameter_value(eg, argument)
    } else {
        argument
    }
}

/// Collect the live PHP call chain behind the internal debug_backtrace frame.
///
/// # Safety (debug and creation modes)
///
/// The frame must be either the active internal-function frame supplied to its
/// handler or the live Throwable creation frame selected by
/// `include_creation_frame`. Every predecessor and function header must remain
/// live for this synchronous walk, as guaranteed by the VM call-frame stack.
pub(crate) unsafe fn collect_debug_backtrace(
    ed: *mut ExecuteData,
    options: i64,
    limit: usize,
    eg: &ExecutorGlobals,
    include_creation_frame: bool,
) -> PhpArray {
    let include_object = options & 1 != 0;
    let include_arguments = options & 2 == 0;
    let mut trace = PhpArray::new();
    // The built-in call frame itself is not part of PHP's reported trace;
    // frame zero is the user/internal caller that invoked debug_backtrace().
    let mut frame = if include_creation_frame {
        ed
    } else {
        eg.trace_caller(ed as usize, (*ed).prev_execute_data)
    };
    // Eval uses a synchronous scope bridge and writes variables back only when
    // its detached activation returns. Retain active eval frames while walking
    // to their logical caller so its declared trace arguments observe changes
    // made before that writeback boundary.
    let mut eval_scope_frames = Vec::new();
    while !frame.is_null() && (limit == 0 || trace.len() < limit) {
        // The top-level script is represented by an executable frame in RPHP,
        // but PHP traces stop at the last function/method called from it.
        let caller = eg.trace_caller(frame as usize, (*frame).prev_execute_data);
        if caller.is_null() {
            break;
        }
        let function = Function::from_common_ptr((*frame).func);
        if function.fn_type() == FunctionType::Undef {
            break;
        }
        let synthetic_frame = eg.detached_trace_function(frame as usize);
        if synthetic_frame == Some("eval") {
            eval_scope_frames.push(frame);
        }
        let name = synthetic_frame.map_or_else(
            || crate::vm::execute::displayed_frame_function_name(eg, frame),
            str::to_string,
        );
        if name.is_empty() {
            break;
        }
        let common = &*(*frame).func;
        let user = (function.fn_type() == FunctionType::User).then(|| function.as_user());
        let mut entry = PhpArray::new();
        if let Some((file, line)) = eg.detached_trace_origin(frame as usize) {
            // PHP's engine-dispatched callbacks use Unknown:0 for diagnostics
            // but appear as `[internal function]` in Throwable traces.
            if file != "Unknown" || line != 0 {
                entry.set_str("file", Value::string(file.to_string()));
                entry.set_str("line", Value::long(line as i64));
            }
        } else if !caller.is_null() && !(*caller).func.is_null() {
            let caller_function = Function::from_common_ptr((*caller).func);
            if caller_function.fn_type() == FunctionType::User {
                let caller_op_array = &caller_function.as_user().op_array;
                if !caller_op_array.source_file.is_empty()
                    && !caller_op_array.instructions.is_empty()
                {
                    let next = (*caller)
                        .opline
                        .offset_from(caller_op_array.instructions.as_ptr());
                    if let Ok(next) = usize::try_from(next)
                        && let Some(call_index) =
                            if eg.detached_trace_caller_is_current_site(frame as usize) {
                                Some(next)
                            } else {
                                next.checked_sub(1)
                            }
                        && let Some(line) = caller_op_array.source_line(call_index)
                    {
                        entry.set_str(
                            "file",
                            Value::shared_string(caller_op_array.source_file.clone()),
                        );
                        entry.set_str("line", Value::long(line as i64));
                    }
                }
            }
        }
        if (name == "{closure}" || name.starts_with("{closure:"))
            && let Some(class) = eg.declaring_class_of((*frame).func)
        {
            let receiver = user.and_then(|function| {
                function
                    .op_array
                    .all_cvs
                    .iter()
                    .find(|(_, candidate)| candidate == "this")
                    .map(|(this_cv, _)| (*frame).cv(*this_cv).dereferenced())
                    .filter(|value| value.as_object().is_some())
            });
            entry.set_str("function", Value::string(name));
            entry.set_str("class", Value::string(class.to_string()));
            if include_object && let Some(receiver) = receiver {
                entry.set_str("object", receiver.clone());
            }
            entry.set_str(
                "type",
                Value::string(if receiver.is_some() { "->" } else { "::" }),
            );
        } else if (name == "{closure}" || name.starts_with("{closure:"))
            && function.fn_type() == FunctionType::User
            && let Some((this_cv, _)) = function
                .as_user()
                .op_array
                .all_cvs
                .iter()
                .find(|(_, candidate)| candidate == "this")
            && (*frame).cv(*this_cv).dereferenced().as_object().is_some()
        {
            entry.set_str("function", Value::string(name));
            entry.set_str("class", Value::string("Closure"));
            if include_object {
                entry.set_str("object", (*frame).cv(*this_cv).dereferenced().clone());
            }
            entry.set_str("type", Value::string("->"));
        } else if let Some((class, hook)) = name.split_once("::$") {
            entry.set_str("function", Value::string(format!("${hook}")));
            entry.set_str("class", Value::string(class.to_string()));
            if include_object {
                let object = (*frame).cv(0).dereferenced();
                if object.as_object().is_some() {
                    entry.set_str("object", object.clone());
                }
            }
            entry.set_str("type", Value::string("->"));
        } else if let Some((class, method)) = name.rsplit_once("::") {
            entry.set_str("function", Value::string(method.to_string()));
            entry.set_str("class", Value::string(class.to_string()));
            let is_instance = !eg.internal_method_is_static((*frame).func)
                && !eg
                    .find_method_info(class, method)
                    .is_some_and(|(_, is_static, _)| is_static);
            if include_object && is_instance {
                let object = (*frame).cv(0).dereferenced();
                if object.as_object().is_some() {
                    entry.set_str("object", object.clone());
                }
            }
            entry.set_str("type", Value::string(if is_instance { "->" } else { "::" }));
        } else {
            entry.set_str("function", Value::string(name));
        }
        if include_arguments && synthetic_frame.is_none() {
            let count = (*frame).num_args;
            let mut arguments = PhpArray::with_packed_capacity(count as usize);
            for index in 0..count {
                let scoped_argument = common.sig.param_names.get(index as usize).and_then(|name| {
                    eval_scope_frames.iter().find_map(|scope_frame| {
                        let scope_function = Function::from_common_ptr((**scope_frame).func);
                        (scope_function.fn_type() == FunctionType::User)
                            .then(|| scope_function.as_user())
                            .and_then(|scope_user| {
                                scope_user
                                    .op_array
                                    .all_cvs
                                    .iter()
                                    .find(|(_, candidate)| candidate == name)
                                    .map(|(cv, _)| live_argument_value((**scope_frame).cv(*cv)))
                            })
                    })
                });
                let argument = if scoped_argument.is_some() {
                    scoped_argument
                } else if index as usize >= common.sig.param_names.len()
                    && let Some(saved) = eg.function_arguments.get(&(frame as usize))
                {
                    saved.get(index as usize).cloned()
                } else if common.sig.is_variadic && index >= common.sig.public_arity() {
                    let offset = index - common.sig.public_arity();
                    (*frame)
                        .cv(common.sig.variadic_cv_index)
                        .as_array()
                        .and_then(|values| values.get_value_at(offset as usize))
                        .map(live_argument_value)
                } else {
                    Some(live_argument_value(
                        (*frame).cv(common.sig.param_cv_index(index)),
                    ))
                };
                if let Some(argument) = argument {
                    arguments.push(redact_trace_argument(user, common, index, argument, eg));
                }
            }
            if common.sig.is_variadic
                && let Some(values) = (*frame).cv(common.sig.variadic_cv_index).as_array()
            {
                for (key, value) in values.iter() {
                    if let ArrayKey::String(name) = key {
                        arguments.set_str(
                            &name,
                            redact_trace_argument(
                                user,
                                common,
                                common.sig.public_arity(),
                                value.dereferenced().clone(),
                                eg,
                            ),
                        );
                    }
                }
            }
            entry.set_str("args", Value::array(arguments));
        }
        trace.push(Value::array(entry));
        if synthetic_frame.is_none() {
            eval_scope_frames.clear();
        }
        frame = caller;
    }
    trace
}

fn fn_debug_backtrace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let options = arg_opt!(ed, 0).map_or(1, Value::to_long_val);
    let limit = arg_opt!(ed, 1)
        .map(Value::to_long_val)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    // SAFETY: internal handlers receive their currently active call frame and
    // the VM keeps its entire synchronous predecessor chain alive.
    // SAFETY: the internal debug_backtrace activation and its synchronous
    // predecessor chain stay live until this handler returns.
    let trace = unsafe { collect_debug_backtrace(ed, options, limit, eg, false) };
    ret!(rv, Value::array(trace));
}

fn fn_debug_print_backtrace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let options = arg_opt!(ed, 0).map_or(0, Value::to_long_val);
    let limit = arg_opt!(ed, 1)
        .map(Value::to_long_val)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    // SAFETY: the internal activation and its synchronous predecessor chain
    // remain live until this handler returns.
    let trace = unsafe { collect_debug_backtrace(ed, options, limit, eg, false) };
    let output = crate::vm::trace::format_debug_print_backtrace(
        &trace,
        exception_string_param_max_len(eg),
        eg,
    );
    eg.write_output(output.as_bytes());
    ret!(rv, Value::null());
}

// ============================================================================
// Helpers
// ============================================================================

#[inline]
fn cmp_val(cmp: i32) -> std::cmp::Ordering {
    if cmp < 0 {
        std::cmp::Ordering::Less
    } else if cmp > 0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

fn natural_string_cmp(left: &str, right: &str, case_insensitive: bool) -> std::cmp::Ordering {
    natural_compare(left.as_bytes(), right.as_bytes(), case_insensitive)
}

fn natural_value_order(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
    case_insensitive: bool,
) -> Result<Option<std::cmp::Ordering>, VmError> {
    let Some(left) = internal_value_to_string(ed, eg, left)? else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(right) = internal_value_to_string(ed, eg, right)? else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(Some(natural_string_cmp(&left, &right, case_insensitive)))
}

fn array_sort_snapshot_value(value: &Value) -> Value {
    if value.is_owned_reference() {
        let mut alias = value.clone_owned_reference_alias();
        alias.mark_internal_reference_alias();
        alias
    } else {
        value.clone()
    }
}

#[inline(always)]
fn array_projection_value(value: &Value) -> Value {
    if value.is_owned_reference() && value.owned_reference_is_aliased() {
        value.clone_owned_reference_alias()
    } else {
        value.clone()
    }
}

#[derive(Clone, Copy)]
enum ArrayProjectionKeys {
    PreserveAll,
    PreserveStrings,
    ReindexAll,
}

fn array_projection_insert(
    result: &mut PhpArray,
    key: ArrayKey,
    value: &Value,
    key_policy: ArrayProjectionKeys,
) {
    let value = array_projection_value(value);
    match (key_policy, key) {
        (ArrayProjectionKeys::PreserveAll, key)
        | (ArrayProjectionKeys::PreserveStrings, key @ ArrayKey::String(_)) => {
            result.set(key, value);
        }
        (ArrayProjectionKeys::PreserveStrings, ArrayKey::Int(_))
        | (ArrayProjectionKeys::ReindexAll, _) => {
            result.push(value);
        }
    }
}

fn append_projected_values<'a>(
    result: &mut PhpArray,
    values: impl Iterator<Item = &'a Value> + Clone,
) {
    let preserve_references = values
        .clone()
        .any(|value| value.is_owned_reference() && value.owned_reference_is_aliased());
    if preserve_references {
        for value in values {
            result.push(array_projection_value(value));
        }
    } else {
        for value in values {
            result.push(value.clone());
        }
    }
}

fn sort_natural_entries(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    entries: &mut Vec<(ArrayKey, Value)>,
    case_insensitive: bool,
) -> Result<bool, VmError> {
    let length = entries.len();
    if length < 2 {
        return Ok(true);
    }

    let mut order = (0..length).collect::<Vec<_>>();
    let mut merged = order.clone();
    let mut width = 1usize;
    while width < length {
        let mut start = 0usize;
        while start < length {
            let middle = (start + width).min(length);
            let end = (middle + width).min(length);
            let (mut left, mut right, mut output) = (start, middle, start);
            while left < middle && right < end {
                let Some(ordering) = natural_value_order(
                    ed,
                    eg,
                    &entries[order[left]].1,
                    &entries[order[right]].1,
                    case_insensitive,
                )?
                else {
                    return Ok(false);
                };
                if ordering == std::cmp::Ordering::Greater {
                    merged[output] = order[right];
                    right += 1;
                } else {
                    merged[output] = order[left];
                    left += 1;
                }
                output += 1;
            }
            while left < middle {
                merged[output] = order[left];
                left += 1;
                output += 1;
            }
            while right < end {
                merged[output] = order[right];
                right += 1;
                output += 1;
            }
            start = end;
        }
        std::mem::swap(&mut order, &mut merged);
        width = width.saturating_mul(2);
    }

    let original = std::mem::take(entries);
    let mut slots = original.into_iter().map(Some).collect::<Vec<_>>();
    entries.reserve(length);
    for index in order {
        entries.push(
            slots[index]
                .take()
                .expect("natural-sort permutation consumes each entry once"),
        );
    }
    Ok(true)
}

fn fn_natural_key_preserving_sort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    case_insensitive: bool,
) -> Result<(), VmError> {
    let source = owned_argument(ed, 0);
    let Some(array) = source.dereferenced().as_array() else {
        typed_internal_argument_error(eg, function, source.dereferenced(), 1, "array", "array");
        return Ok(());
    };
    let mut entries = array
        .iter()
        .map(|(key, value)| (key, array_sort_snapshot_value(value)))
        .collect::<Vec<_>>();
    if !sort_natural_entries(ed, eg, &mut entries, case_insensitive)? {
        return Ok(());
    }

    let mut result = PhpArray::new();
    for (key, value) in entries {
        result.set(key, array_projection_value(&value));
    }
    arg_mut!(ed, 0, Value::array(result));
    ret!(rv, Value::bool(true));
}

fn fn_natsort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_natural_key_preserving_sort(ed, rv, eg, "natsort", false)
}

fn fn_natcasesort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_natural_key_preserving_sort(ed, rv, eg, "natcasesort", true)
}

fn ascii_case_insensitive_order(left: &str, right: &str) -> std::cmp::Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn sort_value_order(
    left: &Value,
    right: &Value,
    flags: i64,
    precision: i32,
) -> Result<std::cmp::Ordering, ()> {
    Ok(match flags & !SORT_FLAG_CASE {
        SORT_NUMERIC => explicit_float_conversion(left)
            .partial_cmp(&explicit_float_conversion(right))
            .unwrap_or(std::cmp::Ordering::Equal),
        SORT_STRING | SORT_LOCALE_STRING => {
            let left = left.echo_to_string_with_precision(precision);
            let right = right.echo_to_string_with_precision(precision);
            if flags & SORT_FLAG_CASE != 0 {
                ascii_case_insensitive_order(&left, &right)
            } else {
                left.cmp(&right)
            }
        }
        SORT_NATURAL => natural_string_cmp(
            &left.echo_to_string_with_precision(precision),
            &right.echo_to_string_with_precision(precision),
            flags & SORT_FLAG_CASE != 0,
        ),
        _ => cmp_val(crate::vm::execute::values_compare_checked_with_precision(
            left, right, precision,
        )?),
    })
}

fn sort_value_order_runtime(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
    flags: i64,
) -> Result<std::cmp::Ordering, VmError> {
    if eg.exception.is_some() {
        return Ok(std::cmp::Ordering::Equal);
    }

    let ordering = match flags & !SORT_FLAG_CASE {
        SORT_NUMERIC => {
            for value in [left, right] {
                if let Some(message) =
                    explicit_numeric_cast_warning(value, ExplicitNumericCastTarget::Float)
                {
                    report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
                    if eg.exception.is_some() {
                        return Ok(std::cmp::Ordering::Equal);
                    }
                }
            }
            explicit_float_conversion(left)
                .partial_cmp(&explicit_float_conversion(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        SORT_STRING | SORT_LOCALE_STRING => {
            let Some(left) = internal_value_to_string(ed, eg, left)? else {
                return Ok(std::cmp::Ordering::Equal);
            };
            if eg.exception.is_some() {
                return Ok(std::cmp::Ordering::Equal);
            }
            let Some(right) = internal_value_to_string(ed, eg, right)? else {
                return Ok(std::cmp::Ordering::Equal);
            };
            if eg.exception.is_some() {
                return Ok(std::cmp::Ordering::Equal);
            }
            if flags & SORT_FLAG_CASE != 0 {
                ascii_case_insensitive_order(&left, &right)
            } else {
                left.cmp(&right)
            }
        }
        SORT_NATURAL => {
            let Some(left) = internal_value_to_string(ed, eg, left)? else {
                return Ok(std::cmp::Ordering::Equal);
            };
            if eg.exception.is_some() {
                return Ok(std::cmp::Ordering::Equal);
            }
            let Some(right) = internal_value_to_string(ed, eg, right)? else {
                return Ok(std::cmp::Ordering::Equal);
            };
            if eg.exception.is_some() {
                return Ok(std::cmp::Ordering::Equal);
            }
            natural_string_cmp(&left, &right, flags & SORT_FLAG_CASE != 0)
        }
        _ => sort_regular_value_order_runtime(eg, left, right)?,
    };
    Ok(ordering)
}

fn sort_comparison_object_string_conversion(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    let class_name = object.diagnostic_type_name();
    let rendered = crate::vm::execute::call_object_string_conversion(eg, object)?;
    if eg.exception.is_none()
        && let Some(rendered) = rendered.as_ref()
        && rendered.as_str().is_none()
    {
        let outcome = if rendered.value_type() == ValueType::Null {
            "none returned".to_string()
        } else {
            format!("{} returned", rendered.diagnostic_type_name())
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{class_name}::__toString(): Return value must be of type string, {outcome}"),
        ));
    }
    Ok(rendered)
}

fn sort_regular_value_order_runtime(
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
) -> Result<std::cmp::Ordering, VmError> {
    let left_value = left.dereferenced();
    let right_value = right.dereferenced();
    let prepared = match (left_value.value_type(), right_value.value_type()) {
        (ValueType::Null | ValueType::Undef, ValueType::String) => {
            return Ok("".cmp(right_value.as_str().unwrap()));
        }
        (ValueType::String, ValueType::Null | ValueType::Undef) => {
            return Ok(left_value.as_str().unwrap().cmp(""));
        }
        (ValueType::Object, ValueType::String) => {
            match sort_comparison_object_string_conversion(eg, left_value)? {
                Some(rendered) => Some((rendered, right_value.clone())),
                None => return Ok(std::cmp::Ordering::Greater),
            }
        }
        (ValueType::String, ValueType::Object) => {
            match sort_comparison_object_string_conversion(eg, right_value)? {
                Some(rendered) => Some((left_value.clone(), rendered)),
                None => return Ok(std::cmp::Ordering::Less),
            }
        }
        _ => None,
    };
    if eg.exception.is_some() {
        return Ok(std::cmp::Ordering::Equal);
    }
    let (left, right) = prepared
        .as_ref()
        .map_or((left_value, right_value), |(left, right)| (left, right));
    match sort_value_order(left, right, SORT_REGULAR, eg.precision) {
        Ok(ordering) => Ok(ordering),
        Err(()) => {
            report_recursive_sort_comparison(eg);
            Ok(std::cmp::Ordering::Equal)
        }
    }
}

fn sort_key_string(key: &ArrayKey) -> Cow<'_, str> {
    match key {
        ArrayKey::Int(value) => Cow::Owned(value.to_string()),
        ArrayKey::String(value) => Cow::Borrowed(value),
    }
}

fn sort_key_order(
    left: &ArrayKey,
    right: &ArrayKey,
    flags: i64,
    precision: i32,
) -> Result<std::cmp::Ordering, ()> {
    match flags & !SORT_FLAG_CASE {
        SORT_NUMERIC => Ok(match (left, right) {
            (ArrayKey::Int(left), ArrayKey::Int(right)) => left.cmp(right),
            _ => {
                let left = php_numeric_string_to_float(&sort_key_string(left)).unwrap_or(0.0);
                let right = php_numeric_string_to_float(&sort_key_string(right)).unwrap_or(0.0);
                left.partial_cmp(&right)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        }),
        SORT_STRING | SORT_LOCALE_STRING => {
            let left = sort_key_string(left);
            let right = sort_key_string(right);
            Ok(if flags & SORT_FLAG_CASE != 0 {
                ascii_case_insensitive_order(&left, &right)
            } else {
                left.cmp(&right)
            })
        }
        SORT_NATURAL => {
            let left = sort_key_string(left);
            let right = sort_key_string(right);
            Ok(natural_string_cmp(
                &left,
                &right,
                flags & SORT_FLAG_CASE != 0,
            ))
        }
        _ => {
            let left = match left {
                ArrayKey::Int(value) => Value::long(*value),
                ArrayKey::String(value) => Value::string(value.clone()),
            };
            let right = match right {
                ArrayKey::Int(value) => Value::long(*value),
                ArrayKey::String(value) => Value::string(value.clone()),
            };
            sort_value_order(&left, &right, SORT_REGULAR, precision)
        }
    }
}

fn sort_direct_long_entries<T>(
    entries: &mut [T],
    flags: i64,
    reverse: bool,
    value: impl for<'a> Fn(&'a T) -> &'a Value,
) -> bool {
    let mode = flags & !SORT_FLAG_CASE;
    if !matches!(mode, SORT_REGULAR | SORT_NUMERIC)
        || !entries
            .iter()
            .all(|entry| value(entry).value_type() == ValueType::Long)
    {
        return false;
    }
    entries.sort_by(|left, right| {
        let left = value(left).as_long().unwrap();
        let right = value(right).as_long().unwrap();
        let ordering = if mode == SORT_NUMERIC {
            (left as f64)
                .partial_cmp(&(right as f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            left.cmp(&right)
        };
        if reverse {
            ordering.reverse()
        } else {
            ordering
        }
    });
    true
}

/// Comparison-pure scalar domains have no observable comparator schedule.
/// Keep them on the host's stable sort while routing heterogeneous, warning,
/// hook and non-transitive domains through the observed scheduler below.
fn sort_direct_total_scalar_entries<T>(
    entries: &mut [T],
    flags: i64,
    reverse: bool,
    precision: i32,
    value: impl for<'a> Fn(&'a T) -> &'a Value,
) -> bool {
    if !sort_domain_has_total_order(entries, flags, &value) {
        return false;
    }

    entries.sort_by(|left, right| {
        let ordering = sort_value_order(value(left), value(right), flags, precision)
            .unwrap_or(std::cmp::Ordering::Equal);
        if reverse {
            ordering.reverse()
        } else {
            ordering
        }
    });
    true
}

fn sort_domain_has_total_order<T>(
    entries: &[T],
    flags: i64,
    value: impl for<'a> Fn(&'a T) -> &'a Value,
) -> bool {
    let mode = flags & !SORT_FLAG_CASE;
    let all_strings = entries
        .iter()
        .all(|entry| value(entry).dereferenced().value_type() == ValueType::String);
    let all_regular_numeric = entries.iter().all(|entry| {
        let value = value(entry).dereferenced();
        match value.value_type() {
            ValueType::Long => true,
            ValueType::Double => !value.as_double().unwrap().is_nan(),
            ValueType::String => php_numeric_string_to_float(value.as_str().unwrap()).is_some(),
            _ => false,
        }
    });
    let all_regular_non_numeric_strings = entries.iter().all(|entry| {
        let value = value(entry).dereferenced();
        value.value_type() == ValueType::String
            && php_numeric_string_to_float(value.as_str().unwrap()).is_none()
    });
    let all_numeric_casts_are_total = entries.iter().all(|entry| {
        let value = value(entry).dereferenced();
        match value.value_type() {
            ValueType::Null
            | ValueType::Undef
            | ValueType::False
            | ValueType::True
            | ValueType::Long
            | ValueType::String
            | ValueType::Resource => true,
            ValueType::Double => !value.as_double().unwrap().is_nan(),
            _ => false,
        }
    });
    match mode {
        SORT_REGULAR => all_regular_numeric || all_regular_non_numeric_strings,
        SORT_NUMERIC => all_numeric_casts_are_total,
        SORT_STRING | SORT_LOCALE_STRING | SORT_NATURAL => all_strings,
        _ => false,
    }
}

/// Match PHP 8.5's observable two-to-five-element user-callback schedule.
/// Return `false` without another comparison when the callback asks the caller
/// to preserve an already-published exception.
fn stable_sort_small_optional_checked<T, E>(
    entries: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<Option<std::cmp::Ordering>, E>,
) -> Result<bool, E> {
    macro_rules! compare_or_abort {
        ($left:expr, $right:expr) => {
            match compare($left, $right)? {
                Some(ordering) => ordering,
                None => return Ok(false),
            }
        };
    }

    if entries.len() < 2 {
        return Ok(true);
    }
    let first = compare_or_abort!(&entries[0], &entries[1]);
    if first == std::cmp::Ordering::Greater {
        entries.swap(0, 1);
    }
    if entries.len() == 2 {
        return Ok(true);
    }

    if first == std::cmp::Ordering::Greater {
        if compare_or_abort!(&entries[2], &entries[0]) == std::cmp::Ordering::Less {
            entries.swap(1, 2);
            entries.swap(0, 1);
        } else if compare_or_abort!(&entries[1], &entries[2]) == std::cmp::Ordering::Greater {
            entries.swap(1, 2);
        }
    } else if compare_or_abort!(&entries[1], &entries[2]) == std::cmp::Ordering::Greater {
        entries.swap(1, 2);
        if compare_or_abort!(&entries[0], &entries[1]) == std::cmp::Ordering::Greater {
            entries.swap(0, 1);
        }
    }

    for index in 3..entries.len() {
        let mut current = index;
        while current > 0 {
            if compare_or_abort!(&entries[current - 1], &entries[current])
                != std::cmp::Ordering::Greater
            {
                break;
            }
            entries.swap(current - 1, current);
            current -= 1;
        }
    }
    Ok(true)
}

fn stable_sort_checked<T, E>(
    entries: &mut Vec<T>,
    mut compare: impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<(), E> {
    let length = entries.len();
    if length < 2 {
        return Ok(());
    }

    // Original positions provide PHP's stable fallback when the public
    // comparator reports equality. Sorting indices also keeps every cloned
    // entry owned by this vector if an observable comparison fails.
    let mut order = (0..length).collect::<Vec<_>>();
    php_observed_sort_schedule(entries, &mut order, &mut compare)?;

    let original = std::mem::take(entries);
    let mut slots = original.into_iter().map(Some).collect::<Vec<_>>();
    entries.reserve(length);
    for index in order {
        entries.push(
            slots[index]
                .take()
                .expect("stable-sort permutation consumes each entry once"),
        );
    }
    Ok(())
}

#[inline]
fn stable_observed_compare<T, E>(
    entries: &[T],
    order: &[usize],
    left: usize,
    right: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<std::cmp::Ordering, E> {
    let left_index = order[left];
    let right_index = order[right];
    let ordering = compare(&entries[left_index], &entries[right_index])?;
    Ok(if ordering == std::cmp::Ordering::Equal {
        left_index.cmp(&right_index)
    } else {
        ordering
    })
}

/// Run the small-input comparison transcript inferred from PHP 8.5 oracle
/// output. The three-item branch is intentionally directional: reversing its
/// second comparison changes warning order for heterogeneous values.
fn observed_small_sort_schedule<T, E>(
    entries: &[T],
    order: &mut [usize],
    positions: &[usize],
    compare: &mut impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<(), E> {
    match positions {
        [] | [_] => Ok(()),
        [left, right] => {
            if stable_observed_compare(entries, order, *left, *right, compare)?.is_gt() {
                order.swap(*left, *right);
            }
            Ok(())
        }
        [first, second, third] => {
            if !stable_observed_compare(entries, order, *first, *second, compare)?.is_gt() {
                if !stable_observed_compare(entries, order, *second, *third, compare)?.is_gt() {
                    return Ok(());
                }
                order.swap(*second, *third);
                if stable_observed_compare(entries, order, *first, *second, compare)?.is_gt() {
                    order.swap(*first, *second);
                }
                return Ok(());
            }
            if !stable_observed_compare(entries, order, *third, *second, compare)?.is_gt() {
                order.swap(*first, *third);
                return Ok(());
            }
            order.swap(*first, *second);
            if stable_observed_compare(entries, order, *second, *third, compare)?.is_gt() {
                order.swap(*second, *third);
            }
            Ok(())
        }
        [..] if positions.len() <= 5 => {
            observed_small_sort_schedule(
                entries,
                order,
                &positions[..positions.len() - 1],
                compare,
            )?;
            let mut cursor = positions.len() - 1;
            while cursor != 0
                && stable_observed_compare(
                    entries,
                    order,
                    positions[cursor - 1],
                    positions[cursor],
                    compare,
                )?
                .is_gt()
            {
                order.swap(positions[cursor - 1], positions[cursor]);
                cursor -= 1;
            }
            Ok(())
        }
        _ => unreachable!("small observed schedule accepts at most five positions"),
    }
}

fn place_observed_entry(order: &mut [usize], destination: usize, source: usize) {
    order[destination..=source].rotate_right(1);
}

fn observed_insertion_schedule<T, E>(
    entries: &[T],
    order: &mut [usize],
    start: usize,
    length: usize,
    compare: &mut impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<(), E> {
    match length {
        0 | 1 => return Ok(()),
        2..=5 => {
            let positions = (start..start + length).collect::<Vec<_>>();
            return observed_small_sort_schedule(entries, order, &positions, compare);
        }
        _ => {}
    }

    let end = start + length;
    let sentry = start + 6;
    for source in start + 1..sentry {
        let mut destination = source - 1;
        if !stable_observed_compare(entries, order, destination, source, compare)?.is_gt() {
            continue;
        }
        while destination != start {
            destination -= 1;
            if !stable_observed_compare(entries, order, destination, source, compare)?.is_gt() {
                destination += 1;
                break;
            }
        }
        place_observed_entry(order, destination, source);
    }

    for source in sentry..end {
        let mut destination = source - 1;
        if !stable_observed_compare(entries, order, destination, source, compare)?.is_gt() {
            continue;
        }
        loop {
            destination -= 2;
            if !stable_observed_compare(entries, order, destination, source, compare)?.is_gt() {
                destination += 1;
                if !stable_observed_compare(entries, order, destination, source, compare)?.is_gt() {
                    destination += 1;
                }
                break;
            }
            if destination == start {
                break;
            }
            if destination == start + 1 {
                destination -= 1;
                if stable_observed_compare(entries, order, source, destination, compare)?.is_gt() {
                    destination += 1;
                }
                break;
            }
        }
        place_observed_entry(order, destination, source);
    }
    Ok(())
}

/// Repository-owned scheduler whose PHP 8.5 contract is frozen by black-box
/// comparison and warning transcripts. Original tests cover the 5/6, 16/17
/// and 1023/1024 boundaries.
fn php_observed_sort_schedule<T, E>(
    entries: &[T],
    order: &mut [usize],
    compare: &mut impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<(), E> {
    let mut pending = vec![(0usize, order.len())];
    while let Some((mut start, mut length)) = pending.pop() {
        while length > 16 {
            let end = start + length;
            let offset = length >> 1;
            if length >= 1024 {
                let delta = offset >> 1;
                let sample = [
                    start,
                    start + delta,
                    start + offset,
                    start + offset + delta,
                    end - 1,
                ];
                observed_small_sort_schedule(entries, order, &sample, compare)?;
            } else {
                let sample = [start, start + offset, end - 1];
                observed_small_sort_schedule(entries, order, &sample, compare)?;
            }

            let pivot = start + 1;
            order.swap(pivot, start + offset);
            let mut low = pivot + 1;
            let mut high = end - 1;
            loop {
                while stable_observed_compare(entries, order, pivot, low, compare)?.is_gt() {
                    low += 1;
                    if low == high {
                        break;
                    }
                }
                if low == high {
                    break;
                }
                high -= 1;
                if high == low {
                    break;
                }
                while stable_observed_compare(entries, order, high, pivot, compare)?.is_gt() {
                    high -= 1;
                    if high == low {
                        break;
                    }
                }
                if high == low {
                    break;
                }
                order.swap(low, high);
                low += 1;
                if low == high {
                    break;
                }
            }
            order.swap(pivot, low - 1);

            let left = (start, low - start - 1);
            let right = (low, end - low);
            let (next, later) = if left.1 < right.1 {
                (left, right)
            } else {
                (right, left)
            };
            if later.1 != 0 {
                pending.push(later);
            }
            (start, length) = next;
        }
        observed_insertion_schedule(entries, order, start, length, compare)?;
    }
    Ok(())
}

fn report_recursive_sort_comparison(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Nesting level too deep - recursive dependency?",
    ));
}

#[inline]
fn array_values_match(
    left: &Value,
    right: &Value,
    strict: bool,
    precision: i32,
) -> Result<bool, ()> {
    if strict {
        values_identical_checked(left, right)
    } else {
        values_equal_checked_with_precision(left, right, precision)
    }
}

#[inline]
fn array_lookup_numeric_string_needle(needle: &Value, strict: bool) -> Option<f64> {
    if strict || needle.value_type() != ValueType::String {
        return None;
    }
    let integer = needle.as_str().unwrap().parse::<i64>().ok()?;
    // Integers outside the exact f64 range must retain the full integer-aware
    // PHP numeric-string comparison; otherwise adjacent values may collapse.
    (integer.unsigned_abs() <= (1_u64 << 53)).then_some(integer as f64)
}

#[inline]
fn array_lookup_values_match(
    needle: &Value,
    value: &Value,
    strict: bool,
    numeric_string_needle: Option<f64>,
    precision: i32,
) -> Result<bool, ()> {
    if let Some(needle_number) = numeric_string_needle {
        let value = value.dereferenced();
        return match value.value_type() {
            ValueType::Long | ValueType::Double | ValueType::Resource => Ok(value
                .to_double()
                .is_some_and(|number| number == needle_number)),
            ValueType::String => Ok(php_numeric_string_to_float(value.as_str().unwrap())
                .map_or_else(
                    || needle.as_str() == value.as_str(),
                    |number| number == needle_number,
                )),
            _ => array_values_match(needle, value, strict, precision),
        };
    }
    array_values_match(needle, value, strict, precision)
}

#[derive(Clone, Copy)]
struct DumpContext {
    debug_zval: bool,
    execute_data: *mut ExecuteData,
    immutable_array_member: bool,
    refcount_bias: usize,
}

impl DumpContext {
    const PLAIN: Self = Self {
        debug_zval: false,
        execute_data: std::ptr::null_mut(),
        immutable_array_member: false,
        refcount_bias: 0,
    };

    #[inline]
    fn debug(execute_data: *mut ExecuteData) -> Self {
        Self {
            debug_zval: true,
            execute_data,
            immutable_array_member: false,
            refcount_bias: 0,
        }
    }

    #[inline]
    fn child(self) -> Self {
        Self {
            immutable_array_member: false,
            refcount_bias: 0,
            ..self
        }
    }

    #[inline]
    fn array_member(self, immutable: bool) -> Self {
        Self {
            immutable_array_member: immutable,
            refcount_bias: 0,
            ..self
        }
    }

    #[inline]
    fn lazy_proxy_instance(self) -> Self {
        Self {
            immutable_array_member: false,
            // Zend's initialized proxy retains its real instance through an
            // engine-visible owner that is not represented by an ordinary
            // PHP property or frame slot in RPHP.
            refcount_bias: 1,
            ..self
        }
    }

    #[inline]
    fn ownership(self, value: &Value, eg: &ExecutorGlobals) -> PhpVisibleOwnership {
        php_visible_ownership(value, eg, self.execute_data)
    }

    #[inline]
    fn refcount(self, value: &Value, eg: &ExecutorGlobals) -> usize {
        self.ownership(value, eg).count.max(1) + self.refcount_bias
    }

    #[inline]
    fn refcount_with_literal_source(self, value: &Value, eg: &ExecutorGlobals) -> usize {
        let ownership = self.ownership(value, eg);
        ownership.count.max(1)
            + self.refcount_bias
            + usize::from(!ownership.target_in_immutable_array)
    }
}

#[derive(Clone, Copy, Default)]
struct PhpVisibleOwnership {
    count: usize,
    target_in_immutable_array: bool,
}

#[derive(Default)]
struct PhpVisibleOwnerCounter {
    target: Option<(u8, usize)>,
    count: usize,
    target_in_immutable_array: bool,
    visited_arrays: std::collections::HashSet<usize>,
    visited_objects: std::collections::HashSet<usize>,
    visited_references: std::collections::HashSet<usize>,
}

impl PhpVisibleOwnerCounter {
    fn new(target: &Value) -> Self {
        Self {
            target: dump_owner_identity(target.dereferenced()),
            ..Self::default()
        }
    }

    fn visit(&mut self, value: &Value) {
        self.visit_from(value, false);
    }

    fn visit_from(&mut self, value: &Value, immutable_array_member: bool) {
        if value.is_reference() {
            let identity = value.reference_identity();
            if identity.is_some_and(|identity| !self.visited_references.insert(identity)) {
                return;
            }
            self.visit_from(value.dereferenced(), immutable_array_member);
            return;
        }

        let identity = dump_owner_identity(value);
        if identity.is_some() && identity == self.target {
            self.count += 1;
            self.target_in_immutable_array |= immutable_array_member;
        }

        match value.value_type() {
            ValueType::Array => {
                let Some(identity) = value.array_identity() else {
                    return;
                };
                if !self.visited_arrays.insert(identity) {
                    return;
                }
                if let Some(array) = value.as_array() {
                    let immutable = value.is_immutable_array_literal();
                    for child in array.values() {
                        self.visit_from(child, immutable);
                    }
                }
            }
            ValueType::Object => {
                let Some(identity) = value.object_identity() else {
                    return;
                };
                if !self.visited_objects.insert(identity) {
                    return;
                }
                if let Some(object) = value.as_object() {
                    object.for_each_property(|_, child| self.visit(child));
                }
            }
            _ => {}
        }
    }
}

fn dump_owner_identity(value: &Value) -> Option<(u8, usize)> {
    match value.value_type() {
        ValueType::String => value
            .string_rc_ptr()
            .map(|pointer| (ValueType::String as u8, pointer as usize)),
        ValueType::Array => value
            .array_identity()
            .map(|identity| (ValueType::Array as u8, identity)),
        ValueType::Object => value
            .object_identity()
            .map(|identity| (ValueType::Object as u8, identity)),
        ValueType::Closure => value.as_closure().map(|closure| {
            (
                ValueType::Closure as u8,
                closure as *const PhpClosure as usize,
            )
        }),
        _ => None,
    }
}

fn php_visible_ownership(
    target: &Value,
    eg: &ExecutorGlobals,
    mut execute_data: *mut ExecuteData,
) -> PhpVisibleOwnership {
    let mut counter = PhpVisibleOwnerCounter::new(target);
    let mut visited_frames = std::collections::HashSet::new();
    let mut saw_main_frame = false;
    while !execute_data.is_null() && visited_frames.insert(execute_data as usize) {
        // SAFETY: debug_zval_dump receives the active call frame, and every
        // predecessor remains live until this synchronous internal call exits.
        unsafe {
            let common = &*(*execute_data).func;
            if common.fn_type == FunctionType::User {
                let function = &*((*execute_data).func as *const UserFunction);
                saw_main_frame |= function.op_array.is_main_script();
                for (slot, name) in &function.op_array.all_cvs {
                    let aliases_global = function
                        .op_array
                        .global_vars
                        .iter()
                        .any(|(global_slot, _)| global_slot == slot);
                    if !name.starts_with('\0') && !aliases_global {
                        counter.visit((*execute_data).cv(*slot));
                    }
                }
            } else {
                // Variadic normalization may retain raw extra-argument slots
                // after building the canonical variadic array. Only formal
                // parameter CVs are PHP-visible storage at handler entry.
                for slot in 0..common.sig.parameter_cv_count() {
                    counter.visit((*execute_data).cv(slot));
                }
            }
            execute_data = (*execute_data).prev_execute_data;
        }
    }

    if !saw_main_frame {
        for value in eg.globals.values() {
            counter.visit(value);
        }
    }

    for value in eg.static_property_values() {
        counter.visit(value);
    }
    for values in eg.static_vars.values() {
        for value in values.values() {
            counter.visit(value);
        }
    }
    for values in eg.dynamic_variables.values() {
        for value in values.values() {
            counter.visit(value);
        }
    }
    for value in eg.constant_table.borrow().values() {
        counter.visit(value);
    }
    PhpVisibleOwnership {
        count: counter.count,
        target_in_immutable_array: counter.target_in_immutable_array,
    }
}

fn dump_object_header(
    context: DumpContext,
    value: &Value,
    prefix: &str,
    kind: &str,
    class_name: &str,
    handle: u32,
    property_count: usize,
    eg: &ExecutorGlobals,
) -> String {
    if context.debug_zval {
        format!(
            "{prefix}{kind}object({class_name})#{handle} ({property_count}) refcount({}){{\n",
            context.refcount(value, eg)
        )
    } else {
        format!("{prefix}{kind}object({class_name})#{handle} ({property_count}) {{\n")
    }
}

fn var_dump_output_value(
    value: &Value,
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
) -> Result<String, VmError> {
    dump_output_value(value, eg, ed, DumpContext::PLAIN)
}

fn dump_output_value(
    value: &Value,
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    context: DumpContext,
) -> Result<String, VmError> {
    if value.value_type() != ValueType::Object {
        return Ok(dump_value(value, 0, eg, context));
    }

    // Retain the receiver across the synchronous user call. __debugInfo() may
    // rebind the variable that supplied var_dump() or initialize a lazy proxy.
    let receiver = value.clone();
    let Some(debug_info) = crate::vm::execute::call_object_debug_info(eg, &receiver)? else {
        return Ok(dump_value(&receiver, 0, eg, context));
    };
    if eg.exception.is_some() {
        return Ok(String::new());
    }
    let debug_info = debug_info.dereferenced();
    let empty_projection;
    let debug_info = if debug_info.value_type() == ValueType::Null {
        let class_name = receiver
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string());
        report_internal_deprecation(
            eg,
            ed,
            &format!(
                "Returning null from {class_name}::__debugInfo() is deprecated, return an empty array instead"
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(String::new());
        }
        // The legacy null form projects an empty object rather than falling
        // back to the receiver's ordinary properties.
        empty_projection = Value::array(PhpArray::new());
        &empty_projection
    } else if debug_info.value_type() == ValueType::Array {
        debug_info
    } else {
        // Invalid __debugInfo() returns are an engine fatal, not a catchable
        // TypeError. This boundary intentionally escapes the internal call.
        let (file, line) = internal_call_source(ed);
        return Err(VmError::Fatal(format!(
            "__debuginfo() must return an array in {file} on line {line}"
        )));
    };
    Ok(var_dump_debug_info_object(
        &receiver, debug_info, 0, eg, context,
    ))
}

fn var_dump_debug_info_object(
    object_value: &Value,
    debug_info: &Value,
    indent: usize,
    eg: &ExecutorGlobals,
    context: DumpContext,
) -> String {
    let prefix = "  ".repeat(indent);
    let object = object_value
        .as_object()
        .expect("debug projection receiver must remain an object");
    let display_class = object
        .class_name
        .strip_prefix("class@anonymous#")
        .map_or(object.class_name.as_ref(), |_| "class@anonymous");
    let lazy_prefix = eg
        .lazy_object_state(object_value)
        .filter(|state| !state.initializing)
        .map_or("", |state| match state.strategy {
            crate::runtime::LazyObjectStrategy::Ghost => "lazy ghost ",
            crate::runtime::LazyObjectStrategy::Proxy => "lazy proxy ",
        });
    let properties = debug_info
        .as_array()
        .expect("validated debug projection must remain an array");
    let mut output = dump_object_header(
        context,
        object_value,
        &prefix,
        lazy_prefix,
        display_class,
        object_value
            .object_handle()
            .expect("live debug projection receiver must retain its handle"),
        properties.len(),
        eg,
    );
    drop(object);

    let mut visited_arrays = std::collections::HashSet::new();
    let mut visited_objects = std::collections::HashSet::new();
    if let Some(identity) = object_value.object_identity() {
        visited_objects.insert(identity);
    }
    for (key, value) in properties.iter() {
        let key = match key {
            ArrayKey::Int(key) => format!("[{key}]"),
            ArrayKey::String(key) => var_dump_debug_info_key(&key),
        };
        output.push_str(&format!("{}  {}=>\n", prefix, key));
        output.push_str(&var_dump_value_inner(
            value,
            indent + 1,
            eg,
            true,
            context.child(),
            &mut visited_arrays,
            &mut visited_objects,
        ));
    }
    output.push_str(&format!("{}}}\n", prefix));
    output
}

fn var_dump_debug_info_key(key: &str) -> String {
    if let Some(property) = key.strip_prefix("\0*\0") {
        return format!("[\"{property}\":protected]");
    }
    if let Some(private) = key.strip_prefix('\0')
        && let Some((class, property)) = private.split_once('\0')
    {
        return format!("[\"{property}\":\"{class}\":private]");
    }
    format!("[\"{key}\"]")
}

fn dump_value(val: &Value, indent: usize, eg: &ExecutorGlobals, context: DumpContext) -> String {
    var_dump_value_inner(
        val,
        indent,
        eg,
        false,
        context,
        &mut std::collections::HashSet::new(),
        &mut std::collections::HashSet::new(),
    )
}

fn var_dump_value_inner(
    val: &Value,
    indent: usize,
    eg: &ExecutorGlobals,
    show_reference: bool,
    context: DumpContext,
    visited_arrays: &mut std::collections::HashSet<usize>,
    visited_objects: &mut std::collections::HashSet<usize>,
) -> String {
    if val.is_reference() {
        let mut output = var_dump_value_inner(
            val.dereferenced(),
            indent,
            eg,
            false,
            context,
            visited_arrays,
            visited_objects,
        );
        let recursive = output[indent * 2..].starts_with("*RECURSION*");
        if show_reference && val.owned_reference_is_aliased() && !recursive {
            output.insert(indent * 2, '&');
        }
        return output;
    }
    let prefix = "  ".repeat(indent);
    match val.value_type() {
        ValueType::Null => format!("{}NULL\n", prefix),
        ValueType::True => format!("{}bool(true)\n", prefix),
        ValueType::False => format!("{}bool(false)\n", prefix),
        ValueType::Long => format!("{}int({})\n", prefix, val.as_long().unwrap()),
        ValueType::Double => {
            let number = val.as_double().unwrap();
            let display = if number.is_nan() {
                "NAN".to_string()
            } else if number == f64::INFINITY {
                "INF".to_string()
            } else if number == f64::NEG_INFINITY {
                "-INF".to_string()
            } else {
                val.echo_to_string_with_precision(eg.serialize_precision)
            };
            format!("{prefix}float({display})\n")
        }
        ValueType::String => {
            let s = val.as_str().unwrap();
            if !context.debug_zval {
                return format!("{}string({}) \"{}\"\n", prefix, s.len(), s);
            }
            let annotation = if val.is_interned_string() {
                if context.immutable_array_member && s.len() > 1 {
                    format!("refcount({})", context.refcount(val, eg))
                } else {
                    "interned".to_string()
                }
            } else if val.has_string_literal_source_owner() {
                format!(
                    "refcount({})",
                    context.refcount_with_literal_source(val, eg)
                )
            } else {
                format!("refcount({})", context.refcount(val, eg))
            };
            format!("{}string({}) \"{}\" {}\n", prefix, s.len(), s, annotation)
        }
        ValueType::Array => {
            let identity = val
                .array_identity()
                .expect("array tag must expose array identity");
            if !visited_arrays.insert(identity) {
                return format!("{}*RECURSION*\n", prefix);
            }
            let arr = val.as_array().unwrap();
            let mut out = if context.debug_zval {
                if arr.is_pristine_empty() {
                    format!("{}array(0) interned {{\n", prefix)
                } else {
                    let packed = if arr.is_packed() { " packed" } else { "" };
                    format!(
                        "{}array({}){} refcount({}){{\n",
                        prefix,
                        arr.len(),
                        packed,
                        if val.has_array_literal_source_owner() {
                            context.refcount_with_literal_source(val, eg)
                        } else {
                            context.refcount(val, eg)
                        }
                    )
                }
            } else {
                format!("{}array({}) {{\n", prefix, arr.len())
            };
            let member_context = context.array_member(val.is_immutable_array_literal());
            for (key, v) in arr.iter() {
                let key_str = match &key {
                    ArrayKey::Int(k) => format!("[{}]", k),
                    ArrayKey::String(k) => format!("[\"{}\"]", k),
                };
                out.push_str(&format!("{}  {}=>\n", prefix, key_str));
                out.push_str(&var_dump_value_inner(
                    v,
                    indent + 1,
                    eg,
                    true,
                    member_context,
                    visited_arrays,
                    visited_objects,
                ));
            }
            out.push_str(&format!("{}}}\n", prefix));
            visited_arrays.remove(&identity);
            out
        }
        ValueType::Object => {
            let identity = val
                .object_identity()
                .expect("object tag must expose object identity");
            if !visited_objects.insert(identity) {
                return format!("{}*RECURSION*\n", prefix);
            }
            let object = val.as_object().unwrap();
            // PHP exposes an object as ordinary storage while its lazy
            // initializer is running. This also keeps var_dump() inside a
            // proxy factory from presenting the not-yet-produced instance as
            // an initialized lazy proxy.
            let lazy_state = eg
                .lazy_object_state(val)
                .filter(|state| !state.initializing);
            let initialized_proxy = lazy_state.and_then(|state| state.proxy_instance.clone());
            let output = if object.class_name.as_ref() == "SensitiveParameterValue" {
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    "",
                    "SensitiveParameterValue",
                    val.object_handle()
                        .expect("live sensitive value must retain its object handle"),
                    0,
                    eg,
                );
                out.push_str(&format!("{}}}\n", prefix));
                out
            } else if let Some(instance) = initialized_proxy {
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    "lazy proxy ",
                    &object.class_name,
                    val.object_handle()
                        .expect("live lazy proxy must retain its request-local handle"),
                    1,
                    eg,
                );
                out.push_str(&format!("{}  [\"instance\"]=>\n", prefix));
                drop(object);
                out.push_str(&var_dump_value_inner(
                    &instance,
                    indent + 1,
                    eg,
                    true,
                    context.lazy_proxy_instance(),
                    visited_arrays,
                    visited_objects,
                ));
                out.push_str(&format!("{}}}\n", prefix));
                out
            } else if let Some(generator) = object.generator.as_ref() {
                let generator = generator.borrow();
                // SAFETY: every live Generator is created from a retained user
                // function allocation, and its pointer remains stable for the request.
                let function = unsafe { generator.user_function() };
                let internal_name = function.op_array.name.as_str();
                let function_name = if internal_name.starts_with("__closure_")
                    || internal_name
                        .rsplit_once("::")
                        .map_or(internal_name, |(_, method)| method)
                        .starts_with("__closure_")
                {
                    internal_name
                        .split_once('@')
                        .map(|(_, public_name)| public_name)
                        .unwrap_or("{closure}")
                } else {
                    internal_name
                };
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    "",
                    "Generator",
                    val.object_handle()
                        .expect("live generator must retain its request-local handle"),
                    1,
                    eg,
                );
                out.push_str(&format!("{}  [\"function\"]=>\n", prefix));
                out.push_str(&var_dump_value_inner(
                    &Value::string(function_name),
                    indent + 1,
                    eg,
                    false,
                    context.child(),
                    visited_arrays,
                    visited_objects,
                ));
                out.push_str(&format!("{}}}\n", prefix));
                out
            } else if object.class_name.as_ref() == "WeakReference" {
                drop(object);
                let target = eg.weak_reference_target(val).unwrap_or_else(Value::null);
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    "",
                    "WeakReference",
                    val.object_handle()
                        .expect("live WeakReference must retain its object handle"),
                    1,
                    eg,
                );
                out.push_str(&format!("{}  [\"object\"]=>\n", prefix));
                out.push_str(&var_dump_value_inner(
                    &target,
                    indent + 1,
                    eg,
                    true,
                    context.child(),
                    visited_arrays,
                    visited_objects,
                ));
                out.push_str(&format!("{}}}\n", prefix));
                out
            } else if object.class_name.as_ref() == "WeakMap" {
                drop(object);
                let entries = eg.weak_map_entries(val);
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    "",
                    "WeakMap",
                    val.object_handle()
                        .expect("live WeakMap must retain its object handle"),
                    entries.len(),
                    eg,
                );
                for (index, (key, value)) in entries.iter().enumerate() {
                    out.push_str(&format!(
                        "{}  [{}]=>\n{}  array(2) {{\n{}    [\"key\"]=>\n",
                        prefix, index, prefix, prefix,
                    ));
                    out.push_str(&var_dump_value_inner(
                        key,
                        indent + 2,
                        eg,
                        true,
                        context.child(),
                        visited_arrays,
                        visited_objects,
                    ));
                    out.push_str(&format!("{}    [\"value\"]=>\n", prefix));
                    out.push_str(&var_dump_value_inner(
                        value,
                        indent + 2,
                        eg,
                        true,
                        context.child(),
                        visited_arrays,
                        visited_objects,
                    ));
                    out.push_str(&format!("{}  }}\n", prefix));
                }
                out.push_str(&format!("{}}}\n", prefix));
                out
            } else if eg
                .class_table
                .get(object.class_name.as_ref())
                .is_some_and(|class| class.is_enum)
                && !context.debug_zval
            {
                let case = object
                    .get_property("name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!("{}enum({}::{})\n", prefix, object.class_name, case)
            } else {
                let class = eg.class_by_id(object.class_id);
                let mut property_count = if let Some(class) = class {
                    class
                        .properties
                        .iter()
                        .enumerate()
                        .filter(|(slot, _)| {
                            !class.properties[*slot].is_virtual_hook_property()
                                && object
                                    .get_property_slot(*slot)
                                    .is_some_and(|value| value.value_type() != ValueType::Undef)
                        })
                        .count()
                } else {
                    0
                };
                object.for_each_dynamic_property(|_, value| {
                    property_count += usize::from(value.value_type() != ValueType::Undef);
                });
                let display_class = object
                    .class_name
                    .strip_prefix("class@anonymous#")
                    .map_or(object.class_name.as_ref(), |_| "class@anonymous");
                let lazy_prefix = lazy_state.map_or("", |state| match state.strategy {
                    crate::runtime::LazyObjectStrategy::Ghost => "lazy ghost ",
                    crate::runtime::LazyObjectStrategy::Proxy => "lazy proxy ",
                });
                let mut out = dump_object_header(
                    context,
                    val,
                    &prefix,
                    lazy_prefix,
                    display_class,
                    val.object_handle()
                        .expect("live object must retain its request-local handle"),
                    property_count,
                    eg,
                );
                if let Some(class) = class {
                    for slot in var_dump_property_slots(eg, object.class_id) {
                        let definition = &class.properties[slot];
                        let Some(value) = object.get_property_slot(slot) else {
                            continue;
                        };
                        if definition.is_virtual_hook_property()
                            && value.value_type() != ValueType::Undef
                        {
                            continue;
                        }
                        if value.value_type() == ValueType::Undef {
                            if !matches!(definition.type_hint, ParamTypeHint::None) {
                                let key = var_dump_property_key(definition);
                                out.push_str(&format!("{}  {}=>\n", prefix, key));
                                out.push_str(&format!(
                                    "{}  uninitialized({})\n",
                                    prefix,
                                    definition.type_hint.display_name()
                                ));
                            }
                            continue;
                        }
                        let key = var_dump_property_key(definition);
                        out.push_str(&format!("{}  {}=>\n", prefix, key));
                        out.push_str(&var_dump_value_inner(
                            value,
                            indent + 1,
                            eg,
                            true,
                            context.child(),
                            visited_arrays,
                            visited_objects,
                        ));
                    }
                }
                object.for_each_dynamic_property(|name, value| {
                    if value.value_type() == ValueType::Undef {
                        return;
                    }
                    out.push_str(&format!("{}  [\"{}\"]=>\n", prefix, name));
                    out.push_str(&var_dump_value_inner(
                        value,
                        indent + 1,
                        eg,
                        true,
                        context.child(),
                        visited_arrays,
                        visited_objects,
                    ));
                });
                out.push_str(&format!("{}}}\n", prefix));
                out
            };
            visited_objects.remove(&identity);
            output
        }
        ValueType::Closure => {
            let identity = val
                .as_closure()
                .map(|closure| closure as *const PhpClosure as usize)
                .expect("closure tag must expose closure identity");
            if !visited_objects.insert(identity) {
                return format!("{}*RECURSION*\n", prefix);
            }
            let closure = val.as_closure().unwrap();
            let common = closure
                .common()
                .expect("live Closure must retain a registered function");
            let user_function = closure.user_function();
            let anonymous_metadata = user_function.and_then(|function| {
                let public_name =
                    function
                        .op_array
                        .name
                        .split_once('@')
                        .and_then(|(internal, public)| {
                            internal.starts_with("__closure_").then_some(public)
                        })?;
                let declaration_line = function
                    .op_array
                    .source_lines
                    .last()
                    .filter(|(opline, _)| *opline == u32::MAX)
                    .map_or(0, |(_, line)| i64::from(*line));
                Some((
                    public_name.to_string(),
                    function.op_array.source_file.as_ref().clone(),
                    declaration_line,
                ))
            });
            let function_name = user_function
                .map(|function| function.op_array.name.as_str())
                .filter(|name| {
                    !name.starts_with("__closure_")
                        && !name
                            .rsplit_once("::")
                            .map_or(*name, |(_, method)| method)
                            .starts_with("__closure_")
                })
                .map(str::to_owned)
                .or_else(|| {
                    (common.fn_type == FunctionType::Internal).then(|| {
                        eg.function_table
                            .iter()
                            .find_map(|(name, pointer)| {
                                std::ptr::eq(*pointer, closure.func).then_some(name.clone())
                            })
                            .unwrap_or_else(|| "internal function".to_string())
                    })
                });

            let mut static_values = PhpArray::new();
            if let Some(function) = user_function {
                let capture_start = common.sig.parameter_cv_count();
                for (index, capture) in closure.captures.iter().enumerate() {
                    if capture.value_type() == ValueType::Undef {
                        continue;
                    }
                    let cv = capture_start + index as u32;
                    let name = function
                        .op_array
                        .all_cvs
                        .iter()
                        .find_map(|(candidate, name)| (*candidate == cv).then_some(name.as_str()))
                        .unwrap_or("unknown");
                    static_values.set_str(name, capture.clone_closure_capture());
                }
                let closure_statics = closure.static_vars.as_ref().map(|storage| storage.borrow());
                let runtime_statics = eg.static_vars.get(&function.op_array.name);
                for (_, name, default) in &function.op_array.static_vars {
                    let value = if let Some(values) = closure_statics.as_ref() {
                        values.get(name).cloned()
                    } else {
                        runtime_statics.and_then(|values| values.get(name)).cloned()
                    }
                    .or_else(|| default.clone())
                    .unwrap_or_else(Value::null);
                    static_values.set_str(name, value);
                }
            }

            let mut parameters = PhpArray::new();
            for (index, name) in common.sig.param_names.iter().enumerate() {
                let state = if index < common.sig.required_num_args as usize {
                    "<required>"
                } else {
                    "<optional>"
                };
                parameters.set_str(&format!("${name}"), Value::string(state));
            }
            let property_count = anonymous_metadata.as_ref().map_or(0, |_| 3)
                + usize::from(function_name.is_some())
                + usize::from(!static_values.is_empty())
                + usize::from(closure.bound_this.is_some())
                + usize::from(!parameters.is_empty());
            let mut out = dump_object_header(
                context,
                val,
                &prefix,
                "",
                "Closure",
                closure.object_handle,
                property_count,
                eg,
            );
            let mut append_property = |name: &str, value: &Value| {
                out.push_str(&format!("{}  [\"{}\"]=>\n", prefix, name));
                out.push_str(&var_dump_value_inner(
                    value,
                    indent + 1,
                    eg,
                    true,
                    context.child(),
                    visited_arrays,
                    visited_objects,
                ));
            };
            if let Some((name, file, line)) = anonymous_metadata {
                append_property("name", &Value::string(name));
                append_property("file", &Value::string(file));
                append_property("line", &Value::long(line));
            } else if let Some(function_name) = function_name {
                append_property("function", &Value::string(function_name));
            }
            if !static_values.is_empty() {
                append_property("static", &Value::array(static_values));
            }
            if let Some(bound_this) = closure.bound_this.as_ref() {
                append_property("this", bound_this);
            }
            if !parameters.is_empty() {
                append_property("parameter", &Value::array(parameters));
            }
            out.push_str(&format!("{}}}\n", prefix));
            visited_objects.remove(&identity);
            out
        }
        ValueType::Resource => {
            let id = val.as_resource_id().unwrap();
            format!(
                "{}resource({}) of type ({})\n",
                prefix,
                id,
                resource::type_for_request(eg, id)
            )
        }
        _ => format!("{}unknown\n", prefix),
    }
}

fn var_dump_property_key(definition: &PropertyDefinition) -> String {
    let declaring_class = definition
        .declaring_class
        .strip_prefix("class@anonymous#")
        .map_or(definition.declaring_class.as_str(), |_| "class@anonymous");
    match definition.visibility {
        Visibility::Public => format!("[\"{}\"]", definition.name),
        Visibility::Protected => format!("[\"{}\":protected]", definition.name),
        Visibility::Private => format!("[\"{}\":\"{}\":private]", definition.name, declaring_class),
    }
}

fn var_dump_property_slots(eg: &ExecutorGlobals, class_id: u32) -> Vec<usize> {
    eg.instance_property_slots_in_iteration_order(class_id)
}

fn print_r_value(val: &Value, indent: usize, eg: &ExecutorGlobals) -> Vec<u8> {
    let mut visited_arrays = std::collections::HashSet::new();
    let mut visited_objects = std::collections::HashSet::new();
    print_r_value_inner(val, indent, eg, &mut visited_arrays, &mut visited_objects)
}

fn print_r_value_inner(
    val: &Value,
    indent: usize,
    eg: &ExecutorGlobals,
    visited_arrays: &mut std::collections::HashSet<usize>,
    visited_objects: &mut std::collections::HashSet<usize>,
) -> Vec<u8> {
    let val = val.dereferenced();
    match val.value_type() {
        ValueType::Null => Vec::new(),
        ValueType::True => b"1".to_vec(),
        ValueType::False => Vec::new(),
        ValueType::Long => val.as_long().unwrap().to_string().into_bytes(),
        ValueType::Double => val.echo_to_string_with_precision(eg.precision).into_bytes(),
        ValueType::String => val.php_string_bytes().unwrap_or_default().into_owned(),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let identity = val
                .array_identity()
                .expect("live print_r array must retain an identity");
            if !visited_arrays.insert(identity) {
                return b"Array\n *RECURSION*".to_vec();
            }
            // print_r() indents a nested array's body relative to both the
            // containing key and its `=>` value column.
            let prefix = "    ".repeat(indent * 2);
            let inner = "    ".repeat(indent * 2 + 1);
            let mut out = b"Array\n".to_vec();
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(b"(\n");
            for (key, v) in arr.iter() {
                let key_bytes = match &key {
                    ArrayKey::Int(k) => k.to_string().into_bytes(),
                    ArrayKey::String(k) if arr.has_external_byte_keys() => php_string_to_bytes(k),
                    ArrayKey::String(k) => k.as_bytes().to_vec(),
                };
                out.extend_from_slice(inner.as_bytes());
                out.push(b'[');
                out.extend_from_slice(&key_bytes);
                out.extend_from_slice(b"] => ");
                out.extend_from_slice(&print_r_value_inner(
                    v,
                    indent + 1,
                    eg,
                    visited_arrays,
                    visited_objects,
                ));
                out.push(b'\n');
            }
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(b")\n");
            visited_arrays.remove(&identity);
            out
        }
        ValueType::Object => {
            let Some(object) = val.as_object() else {
                return Vec::new();
            };
            if object.class_name.as_ref() == "SensitiveParameterValue" {
                return b"SensitiveParameterValue Object\n(\n)\n".to_vec();
            }
            let Some(class) = eg.class_by_id(object.class_id) else {
                return Vec::new();
            };
            if class.is_enum {
                let Some(name) = object.get_property("name").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let prefix = "    ".repeat(indent * 2);
                let inner = "    ".repeat(indent * 2 + 1);
                let value = object.get_property("value");
                let backing = value.map_or("", |value| match value.value_type() {
                    ValueType::Long => ":int",
                    ValueType::String => ":string",
                    _ => "",
                });
                let mut out = Vec::new();
                out.extend_from_slice(object.class_name.as_bytes());
                out.extend_from_slice(b" Enum");
                out.extend_from_slice(backing.as_bytes());
                out.push(b'\n');
                out.extend_from_slice(prefix.as_bytes());
                out.extend_from_slice(b"(\n");
                out.extend_from_slice(inner.as_bytes());
                out.extend_from_slice(b"[name] => ");
                out.extend_from_slice(name.as_bytes());
                out.push(b'\n');
                if let Some(value) = value {
                    out.extend_from_slice(inner.as_bytes());
                    out.extend_from_slice(b"[value] => ");
                    if let Some(bytes) = value.php_string_bytes() {
                        out.extend_from_slice(&bytes);
                    } else {
                        out.extend_from_slice(
                            value.echo_to_string_with_precision(eg.precision).as_bytes(),
                        );
                    }
                    out.push(b'\n');
                }
                out.extend_from_slice(prefix.as_bytes());
                out.extend_from_slice(b")\n");
                return out;
            }

            let display_class = object
                .class_name
                .strip_prefix("class@anonymous#")
                .map_or(object.class_name.as_ref(), |_| "class@anonymous");
            let identity = val
                .object_identity()
                .expect("live print_r object must retain an identity");
            if !visited_objects.insert(identity) {
                let mut out = display_class.as_bytes().to_vec();
                out.extend_from_slice(b" Object\n *RECURSION*");
                return out;
            }

            let prefix = "    ".repeat(indent * 2);
            let inner = "    ".repeat(indent * 2 + 1);
            let mut out = display_class.as_bytes().to_vec();
            out.extend_from_slice(b" Object\n");
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(b"(\n");
            for slot in var_dump_property_slots(eg, object.class_id) {
                let definition = &class.properties[slot];
                if definition.is_virtual_hook_property() {
                    continue;
                }
                let Some(value) = object.get_property_slot(slot) else {
                    continue;
                };
                if value.value_type() == ValueType::Undef {
                    continue;
                }
                out.extend_from_slice(inner.as_bytes());
                out.extend_from_slice(print_r_property_key(definition).as_bytes());
                out.extend_from_slice(b" => ");
                out.extend_from_slice(&print_r_value_inner(
                    value,
                    indent + 1,
                    eg,
                    visited_arrays,
                    visited_objects,
                ));
                out.push(b'\n');
            }
            object.for_each_dynamic_property(|name, value| {
                if value.value_type() == ValueType::Undef {
                    return;
                }
                out.extend_from_slice(inner.as_bytes());
                out.push(b'[');
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(b"] => ");
                out.extend_from_slice(&print_r_value_inner(
                    value,
                    indent + 1,
                    eg,
                    visited_arrays,
                    visited_objects,
                ));
                out.push(b'\n');
            });
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(b")\n");
            visited_objects.remove(&identity);
            out
        }
        ValueType::Resource => val.echo_to_string().into_bytes(),
        _ => Vec::new(),
    }
}

fn print_r_property_key(definition: &PropertyDefinition) -> String {
    let declaring_class = definition
        .declaring_class
        .strip_prefix("class@anonymous#")
        .map_or(definition.declaring_class.as_str(), |_| "class@anonymous");
    match definition.visibility {
        Visibility::Public => format!("[{}]", definition.name),
        Visibility::Protected => format!("[{}:protected]", definition.name),
        Visibility::Private => format!("[{}:{}:private]", definition.name, declaring_class),
    }
}

fn enum_case_export(val: &Value, eg: &ExecutorGlobals) -> Option<String> {
    let object = val.as_object()?;
    eg.find_class(&object.class_name)?.is_enum.then_some(())?;
    let case = object.get_property("name")?.as_str()?;
    Some(format!(
        "\\{}::{}",
        object.class_name.trim_start_matches('\\'),
        case
    ))
}

#[inline]
fn var_export_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('\'');
    for character in value.chars() {
        match character {
            '\0' => output.push_str("' . \"\\0\" . '"),
            '\\' | '\'' => {
                output.push('\\');
                output.push(character);
            }
            _ => output.push(character),
        }
    }
    output.push('\'');
    output
}

fn push_var_export_indent(output: &mut String, level: usize) {
    for _ in 0..level {
        output.push_str("  ");
    }
}

fn var_export_nested_value(value: &Value) -> bool {
    matches!(
        value.dereferenced().value_type(),
        ValueType::Array | ValueType::Object
    )
}

#[derive(Default)]
struct VarExportState {
    arrays: Vec<usize>,
    objects: Vec<usize>,
    recursive_values: usize,
}

fn push_var_export_entry(
    output: &mut String,
    key: &str,
    value: &Value,
    eg: &ExecutorGlobals,
    level: usize,
    key_padding: usize,
    state: &mut VarExportState,
) {
    let exported = var_export_value_at(value, eg, level + 1, state);
    push_var_export_indent(output, level);
    for _ in 0..key_padding {
        output.push(' ');
    }
    output.push_str(key);
    if var_export_nested_value(value) && exported != "NULL" {
        output.push_str(" => \n");
        push_var_export_indent(output, level + 1);
        output.push_str(&exported);
    } else {
        output.push_str(" => ");
        output.push_str(&exported);
    }
    output.push_str(",\n");
}

fn var_export_object_property_name(key: &str) -> &str {
    key.rsplit_once('\0').map_or(key, |(_, name)| name)
}

fn var_export_value_at(
    val: &Value,
    eg: &ExecutorGlobals,
    level: usize,
    state: &mut VarExportState,
) -> String {
    let val = val.dereferenced();
    match val.value_type() {
        ValueType::Null => "NULL".to_string(),
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        ValueType::Long => val.as_long().unwrap().to_string(),
        ValueType::Double => crate::value::php_var_export_float_to_string(
            val.as_double().unwrap(),
            eg.serialize_precision,
        ),
        ValueType::String => var_export_string(val.as_str().unwrap()),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let can_recurse = arr.values().any(var_export_nested_value);
            let identity = if can_recurse {
                let identity = val
                    .array_identity()
                    .expect("array export requires a live array identity");
                if state.arrays.contains(&identity) {
                    state.recursive_values += 1;
                    return "NULL".to_string();
                }
                state.arrays.push(identity);
                Some(identity)
            } else {
                None
            };
            let mut out = "array (\n".to_string();
            for (key, v) in arr.iter() {
                let key_str = match &key {
                    ArrayKey::Int(k) => k.to_string(),
                    ArrayKey::String(k) => var_export_string(k),
                };
                push_var_export_entry(&mut out, &key_str, v, eg, level, 2, state);
            }
            push_var_export_indent(&mut out, level);
            out.push(')');
            if let Some(identity) = identity {
                let completed = state.arrays.pop();
                debug_assert_eq!(completed, Some(identity));
            }
            out
        }
        ValueType::Object => {
            if let Some(case) = enum_case_export(val, eg) {
                return case;
            }
            let identity = val
                .object_identity()
                .expect("object export requires a live object identity");
            if state.objects.contains(&identity) {
                state.recursive_values += 1;
                return "NULL".to_string();
            }
            state.objects.push(identity);
            let object = val
                .as_object()
                .expect("object export requires a live object value");
            let class_name = object.class_name.to_string();
            drop(object);
            let properties = crate::vm::execute::cast_object_to_array(val, eg);
            let properties = properties
                .as_array()
                .expect("object-to-array projection must return an array");
            let std_class = class_name.eq_ignore_ascii_case("stdClass");
            let mut out = if std_class {
                "(object) array(\n".to_string()
            } else {
                format!(
                    "\\{}::__set_state(array(\n",
                    class_name.trim_start_matches('\\')
                )
            };
            for (key, value) in properties.iter() {
                let key = match key {
                    ArrayKey::Int(key) => key.to_string(),
                    ArrayKey::String(key) => {
                        var_export_string(var_export_object_property_name(&key))
                    }
                };
                push_var_export_entry(&mut out, &key, value, eg, level, 3, state);
            }
            push_var_export_indent(&mut out, level);
            out.push(')');
            if !std_class {
                out.push(')');
            }
            let completed = state.objects.pop();
            debug_assert_eq!(completed, Some(identity));
            out
        }
        _ => "NULL".to_string(),
    }
}

fn var_export_value(val: &Value, eg: &ExecutorGlobals, state: &mut VarExportState) -> String {
    var_export_value_at(val, eg, 0, state)
}

/// JSON value that retains PHP array insertion order for object-shaped arrays.
/// `serde_json::Value::Object` uses its map representation's key order, which
/// is not a PHP array's observable iteration order without an extra dependency.
enum PhpJsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<PhpJsonValue>),
    Object(Vec<(String, PhpJsonValue)>),
}

impl serde::Serialize for PhpJsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(values) => values.serialize(serializer),
            Self::Object(entries) => {
                use serde::ser::SerializeMap as _;

                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

#[derive(Default)]
struct PhpJsonEncodeState {
    error_code: i64,
}

struct PhpJsonContainer<'a> {
    key: usize,
    parent: Option<&'a PhpJsonContainer<'a>>,
}

impl PhpJsonEncodeState {
    fn record_error(&mut self, code: i64) {
        self.error_code = code;
    }
}

fn json_container_is_recursive(mut container: Option<&PhpJsonContainer<'_>>, key: usize) -> bool {
    while let Some(active) = container {
        if active.key == key {
            return true;
        }
        container = active.parent;
    }
    false
}

fn project_ordinary_json_object(
    val: &Value,
    class_id: u32,
    eg: &mut ExecutorGlobals,
    compact_formatter_compatible: &mut bool,
    state: &mut PhpJsonEncodeState,
    parent: Option<&PhpJsonContainer<'_>>,
) -> Result<PhpJsonValue, VmError> {
    let identity = val
        .object_identity()
        .expect("object JSON projection lost its identity");
    debug_assert_eq!(identity & 1, 0);
    let key = identity | 1;
    if json_container_is_recursive(parent, key) {
        state.record_error(JSON_ERROR_RECURSION);
        return Ok(PhpJsonValue::Null);
    }
    let container = PhpJsonContainer { key, parent };

    let slots = eg.visible_instance_property_slots(class_id, None);
    let mut properties = Vec::with_capacity(slots.len());
    let mut declared_names = std::collections::HashSet::new();
    for slot in slots {
        let definition = eg
            .instance_property_definition(class_id, slot)
            .expect("visible JSON property slot must retain its definition")
            .clone();
        declared_names.insert(definition.name.clone());
        let property = if definition.has_get_hook {
            crate::vm::execute::call_object_property_get_hook(eg, val, &definition.name)?
                .map(|value| value.dereferenced().clone())
        } else {
            val.as_object().and_then(|object| {
                object
                    .get_property_slot(slot)
                    .filter(|property| !property.is_undef())
                    .cloned()
            })
        };
        if eg.exception.is_some() {
            return Ok(PhpJsonValue::Null);
        }
        if let Some(property) = property {
            properties.push((definition.name, property));
        }
    }
    if let Some(object) = val.as_object() {
        object.for_each_dynamic_property(|name, property| {
            if !property.is_undef() && !declared_names.contains(name) {
                properties.push((name.to_string(), property.clone()));
            }
        });
    }
    let mut entries = Vec::with_capacity(properties.len());
    for (key, value) in properties {
        entries.push((
            key,
            value_to_json(
                &value,
                eg,
                compact_formatter_compatible,
                state,
                Some(&container),
            )?,
        ));
    }
    // ReflectionProperty's two engine-declared public slots have a canonical
    // PHP order (`name`, then `class`). Other objects retain the established
    // deterministic projection until general property insertion order is a
    // separately admitted runtime contract.
    if val
        .as_object()
        .is_none_or(|object| object.class_name.as_ref() != "ReflectionProperty")
    {
        entries.sort_by(|left, right| left.0.cmp(&right.0));
    }
    Ok(PhpJsonValue::Object(entries))
}

/// `JsonSerializable::jsonSerialize()` returning the receiver bypasses the
/// callback and exposes the enum's engine-provided public properties.
fn project_ordinary_json_enum(
    val: &Value,
    eg: &mut ExecutorGlobals,
    compact_formatter_compatible: &mut bool,
    state: &mut PhpJsonEncodeState,
    parent: Option<&PhpJsonContainer<'_>>,
) -> Result<PhpJsonValue, VmError> {
    let identity = val
        .object_identity()
        .expect("enum JSON projection lost its identity");
    debug_assert_eq!(identity & 1, 0);
    let key = identity | 1;
    if json_container_is_recursive(parent, key) {
        state.record_error(JSON_ERROR_RECURSION);
        return Ok(PhpJsonValue::Null);
    }
    let container = PhpJsonContainer { key, parent };
    let Some(object) = val.as_object() else {
        return Ok(PhpJsonValue::Null);
    };
    let mut properties = Vec::with_capacity(2);
    for name in ["name", "value"] {
        if let Some(value) = object.get_property(name).cloned() {
            properties.push((name.to_string(), value));
        }
    }
    drop(object);
    let mut entries = Vec::with_capacity(properties.len());
    for (key, value) in properties {
        entries.push((
            key,
            value_to_json(
                &value,
                eg,
                compact_formatter_compatible,
                state,
                Some(&container),
            )?,
        ));
    }
    Ok(PhpJsonValue::Object(entries))
}

/// Convert a PHP value to the order-preserving JSON projection used above.
fn value_to_json(
    val: &Value,
    eg: &mut ExecutorGlobals,
    compact_formatter_compatible: &mut bool,
    state: &mut PhpJsonEncodeState,
    parent: Option<&PhpJsonContainer<'_>>,
) -> Result<PhpJsonValue, VmError> {
    let referenced_container = val.is_reference();
    let val = val.dereferenced();
    Ok(match val.value_type() {
        ValueType::Null | ValueType::Undef => PhpJsonValue::Null,
        ValueType::True => PhpJsonValue::Bool(true),
        ValueType::False => PhpJsonValue::Bool(false),
        ValueType::Long => PhpJsonValue::Number(serde_json::Number::from(val.as_long().unwrap())),
        ValueType::Double => {
            let d = val.as_double().unwrap();
            if d.is_finite() {
                if *compact_formatter_compatible {
                    let magnitude = d.abs();
                    *compact_formatter_compatible =
                        d.fract() != 0.0 && (1e-4..1e17).contains(&magnitude);
                }
                serde_json::Number::from_f64(d)
                    .map(PhpJsonValue::Number)
                    .unwrap_or(PhpJsonValue::Null)
            } else {
                state.record_error(JSON_ERROR_INF_OR_NAN);
                PhpJsonValue::Number(serde_json::Number::from(0))
            }
        }
        ValueType::String => PhpJsonValue::String(val.as_str().unwrap().to_string()),
        ValueType::Array => {
            // A JsonSerializable callback reached through a referenced array
            // may mutate or unset that array. Retain its current COW storage
            // for the complete traversal so the encoder observes its input
            // snapshot and never keeps dangling element borrows.
            let retained = referenced_container.then(|| val.clone());
            let val = retained.as_ref().unwrap_or(val);
            let identity = val
                .array_identity()
                .expect("array JSON projection lost its identity");
            debug_assert_eq!(identity & 1, 0);
            if json_container_is_recursive(parent, identity) {
                state.record_error(JSON_ERROR_RECURSION);
                return Ok(PhpJsonValue::Null);
            }
            let container = PhpJsonContainer {
                key: identity,
                parent,
            };
            let arr = val.as_array().unwrap();
            let is_list = arr
                .iter()
                .enumerate()
                .all(|(i, (k, _))| matches!(k, ArrayKey::Int(n) if n == i as i64));
            if is_list {
                let mut values = Vec::with_capacity(arr.len());
                for value in arr.values() {
                    values.push(value_to_json(
                        value,
                        eg,
                        compact_formatter_compatible,
                        state,
                        Some(&container),
                    )?);
                }
                PhpJsonValue::Array(values)
            } else {
                let mut entries = Vec::with_capacity(arr.len());
                for (key, value) in arr.iter() {
                    let key = match key {
                        ArrayKey::Int(number) => number.to_string(),
                        ArrayKey::String(string) => string,
                    };
                    entries.push((
                        key,
                        value_to_json(
                            value,
                            eg,
                            compact_formatter_compatible,
                            state,
                            Some(&container),
                        )?,
                    ));
                }
                PhpJsonValue::Object(entries)
            }
        }
        ValueType::Object => {
            let projection_owner = if eg.lazy_object_state(val).is_some() {
                Some(reflection::resolve_lazy_object_chain(eg, val)?)
            } else {
                None
            };
            if eg.exception.is_some() {
                return Ok(PhpJsonValue::Null);
            }
            let val = projection_owner.as_ref().unwrap_or(val);
            let Some((class_id, identity)) = val.as_object().map(|object| {
                (
                    object.class_id,
                    val.object_identity()
                        .expect("object JSON projection lost its identity"),
                )
            }) else {
                return Ok(PhpJsonValue::Null);
            };

            let (may_implement_protocol, is_enum, is_backed_enum) = eg
                .class_by_id(class_id)
                .map_or((false, false, false), |class| {
                    (
                        class.parent.is_some() || !class.implements.is_empty(),
                        class.is_enum,
                        class
                            .implements
                            .iter()
                            .any(|name| name.eq_ignore_ascii_case("BackedEnum")),
                    )
                });
            let is_json_serializable = if may_implement_protocol {
                let class_name = val
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_default();
                eg.class_is_a(&class_name, "JsonSerializable")
            } else {
                false
            };
            if is_json_serializable {
                if !eg.enter_json_serializable_object(identity) {
                    state.record_error(JSON_ERROR_RECURSION);
                    return Ok(PhpJsonValue::Null);
                }
                let result = (|| {
                    let Some(serialized) =
                        call_object_public_method(eg, val, "jsonSerialize", &[])?
                    else {
                        return project_ordinary_json_object(
                            val,
                            class_id,
                            eg,
                            compact_formatter_compatible,
                            state,
                            parent,
                        );
                    };
                    if eg.exception.is_some() {
                        return Ok(PhpJsonValue::Null);
                    }
                    if serialized.dereferenced().object_identity() == Some(identity) {
                        if is_enum {
                            project_ordinary_json_enum(
                                val,
                                eg,
                                compact_formatter_compatible,
                                state,
                                parent,
                            )
                        } else {
                            project_ordinary_json_object(
                                val,
                                class_id,
                                eg,
                                compact_formatter_compatible,
                                state,
                                parent,
                            )
                        }
                    } else {
                        value_to_json(&serialized, eg, compact_formatter_compatible, state, parent)
                    }
                })();
                eg.leave_json_serializable_object(identity);
                return result;
            }

            if is_enum {
                if !is_backed_enum {
                    state.record_error(JSON_ERROR_NON_BACKED_ENUM);
                    return Ok(PhpJsonValue::Number(serde_json::Number::from(0)));
                }
                let backing_value = val
                    .as_object()
                    .and_then(|object| object.get_property("value").cloned())
                    .unwrap_or_else(Value::null);
                return value_to_json(
                    &backing_value,
                    eg,
                    compact_formatter_compatible,
                    state,
                    parent,
                );
            }

            project_ordinary_json_object(
                val,
                class_id,
                eg,
                compact_formatter_compatible,
                state,
                parent,
            )?
        }
        _ => PhpJsonValue::Null,
    })
}

struct PhpJsonFormatter {
    serialize_precision: i32,
    preserve_zero_fraction: bool,
}

impl serde_json::ser::Formatter for PhpJsonFormatter {
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if self.serialize_precision == -1 {
            let magnitude = value.abs();
            let php_uses_fixed = magnitude == 0.0 || (1e-4..1e17).contains(&magnitude);
            if php_uses_fixed {
                let mut formatter = serde_json::ser::CompactFormatter;
                if self.preserve_zero_fraction || value.fract() != 0.0 {
                    return serde_json::ser::Formatter::write_f64(&mut formatter, writer, value);
                }
                if value == 0.0 {
                    return writer.write_all(if value.is_sign_negative() {
                        b"-0"
                    } else {
                        b"0"
                    });
                }
                return serde_json::ser::Formatter::write_i64(&mut formatter, writer, value as i64);
            }
        }

        let mut output =
            crate::value::php_serialized_float_to_string(value, self.serialize_precision);
        if self.preserve_zero_fraction && !output.bytes().any(|byte| matches!(byte, b'.' | b'E')) {
            output.push_str(".0");
        }
        output.make_ascii_lowercase();
        writer.write_all(output.as_bytes())
    }
}

struct PhpJsonEncodeResult {
    output: String,
    error_code: i64,
}

fn json_encode_value(
    val: &Value,
    flags: i64,
    eg: &mut ExecutorGlobals,
) -> Result<PhpJsonEncodeResult, VmError> {
    let preserve_zero_fraction = flags & JSON_PRESERVE_ZERO_FRACTION_FLAG != 0;
    let mut compact_formatter_compatible = eg.serialize_precision == -1 && !preserve_zero_fraction;
    let mut state = PhpJsonEncodeState::default();
    let value = value_to_json(val, eg, &mut compact_formatter_compatible, &mut state, None)?;
    let output = if compact_formatter_compatible {
        serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
    } else {
        let mut output = Vec::new();
        let result = {
            let formatter = PhpJsonFormatter {
                serialize_precision: eg.serialize_precision,
                preserve_zero_fraction,
            };
            let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
            value.serialize(&mut serializer)
        };
        if result.is_err() {
            "null".to_string()
        } else {
            String::from_utf8(output).unwrap_or_else(|_| "null".to_string())
        }
    };
    Ok(PhpJsonEncodeResult {
        output,
        error_code: state.error_code,
    })
}

pub(crate) fn json_decode_string(s: &str, assoc: bool) -> Value {
    json_decode::decode_php_value(s, assoc).unwrap_or_else(|_| Value::null())
}

// ============================================================================
// Generator methods
// ============================================================================

/// Helper: extract GeneratorRef from $this (CV 0)
fn get_generator_ref(ed: *mut ExecuteData) -> Option<crate::vm::generator::GeneratorRef> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        if obj.class_name.as_ref() == "Generator" {
            if let Some(rc) = this_val.as_object_rc() {
                let borrowed = rc.borrow();
                return borrowed.generator.clone();
            }
        }
    }
    None
}

/// Ensure generator is started (first next/send/rewind triggers initial execution)
fn ensure_generator_started(
    ed: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;
    let state = gen_ref.borrow().state;
    if state == GeneratorState::Created {
        resume_generator_method(ed, eg, gen_ref, Value::null())?;
    }
    Ok(())
}

/// Generator methods execute as internal calls. Preserve an escaped PHP
/// exception in the standard executor sidecar so `execute_full_call` can
/// inject it into the user caller after the handler returns.
fn resume_generator_method(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<(), VmError> {
    let saved_execute_data = eg.current_execute_data.replace(ed);
    let outcome = crate::vm::execute::resume_generator(eg, gen_ref, send_value);
    eg.current_execute_data.set(saved_execute_data);
    match outcome? {
        crate::vm::execute::GeneratorResumeOutcome::Advanced => Ok(()),
        crate::vm::execute::GeneratorResumeOutcome::Threw(exception) => {
            eg.exception = Some(exception);
            Ok(())
        }
    }
}

#[cold]
#[inline(never)]
fn reject_running_generator(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Cannot resume an already running generator",
    ));
}

fn fn_generator_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(ed, &gen_ref, eg)?;
        synchronize_aborted_generator_delegate(ed, &gen_ref, eg)?;
        let visible = visible_generator_delegate(&gen_ref);
        let val = visible.borrow().value.clone();
        ret!(rv, val);
    }
    ret!(rv, Value::null());
}

fn fn_generator_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(ed, &gen_ref, eg)?;
        synchronize_aborted_generator_delegate(ed, &gen_ref, eg)?;
        if has_completed_generator_delegate(&gen_ref) {
            ret!(rv, Value::null());
        }
        let visible = visible_generator_delegate(&gen_ref);
        let gen_data = visible.borrow();
        let val = if gen_data.state == crate::vm::generator::GeneratorState::Completed {
            Value::null()
        } else {
            gen_data.key.clone()
        };
        ret!(rv, val);
    }
    ret!(rv, Value::null());
}

fn fn_generator_next(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(ed, &gen_ref, eg)?;
        // Advance past current yield
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Running {
            reject_running_generator(eg);
        } else if state == crate::vm::generator::GeneratorState::Suspended {
            if gen_ref.borrow().rewindable {
                gen_ref.borrow_mut().rewindable = false;
            }
            resume_generator_method(ed, eg, &gen_ref, Value::null())?;
        }
    }
    ret!(rv, Value::null());
}

fn fn_generator_valid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(ed, &gen_ref, eg)?;
        synchronize_aborted_generator_delegate(ed, &gen_ref, eg)?;
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        ret!(rv, Value::bool(is_valid));
    }
    ret!(rv, Value::bool(false));
}

fn synchronize_aborted_generator_delegate(
    ed: *mut ExecuteData,
    gen_ref: &crate::vm::generator::GeneratorRef,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let mut current = gen_ref.clone();
    loop {
        let delegate = {
            let generator = current.borrow();
            match generator.delegate.as_ref() {
                Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate.clone()),
                Some(YieldFromDelegate::Array(_, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            }
        };
        let Some(delegate) = delegate else {
            return Ok(());
        };
        let delegate_state = delegate.borrow().state;
        if delegate_state == GeneratorState::Completed {
            if !delegate.borrow().has_returned {
                resume_generator_method(ed, eg, gen_ref, Value::null())?;
            }
            return Ok(());
        }
        current = delegate;
    }
}

fn visible_generator_delegate(
    gen_ref: &crate::vm::generator::GeneratorRef,
) -> crate::vm::generator::GeneratorRef {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let mut current = gen_ref.clone();
    loop {
        let delegate = {
            let generator = current.borrow();
            match generator.delegate.as_ref() {
                Some(YieldFromDelegate::Generator(delegate, _))
                    if delegate.borrow().state != GeneratorState::Completed =>
                {
                    Some(delegate.clone())
                }
                Some(YieldFromDelegate::Generator(_, _))
                | Some(YieldFromDelegate::Array(_, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            }
        };
        let Some(delegate) = delegate else {
            return current;
        };
        current = delegate;
    }
}

fn has_completed_generator_delegate(gen_ref: &crate::vm::generator::GeneratorRef) -> bool {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let mut current = gen_ref.clone();
    loop {
        let delegate = {
            let generator = current.borrow();
            match generator.delegate.as_ref() {
                Some(YieldFromDelegate::Generator(delegate, _)) => Some(delegate.clone()),
                Some(YieldFromDelegate::Array(_, _))
                | Some(YieldFromDelegate::Iterator(_))
                | None => None,
            }
        };
        let Some(delegate) = delegate else {
            return false;
        };
        if delegate.borrow().state == GeneratorState::Completed {
            return true;
        }
        current = delegate;
    }
}

fn fn_generator_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Created {
            ensure_generator_started(ed, &gen_ref, eg)?;
        } else if !gen_ref.borrow().rewindable {
            eg.exception = Some(crate::value::make_error_value(
                "Exception",
                "Cannot rewind a generator that was already run",
            ));
        }
    }
    ret!(rv, Value::null());
}

fn fn_generator_send(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        let send_val = arg!(ed, 1).clone();

        // PHP semantics: ensure_initialized first, then inject send value.
        // If Created: start generator (runs to first yield), THEN resume with send value.
        // If Suspended: resume with send value directly.
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Created {
            // Start generator — runs to first yield, sets up send_target
            resume_generator_method(ed, eg, &gen_ref, Value::null())?;
            // Now resume with the actual send value (if still suspended)
            let state2 = gen_ref.borrow().state;
            if state2 == crate::vm::generator::GeneratorState::Suspended {
                gen_ref.borrow_mut().rewindable = false;
                resume_generator_method(ed, eg, &gen_ref, send_val)?;
            }
        } else if state == crate::vm::generator::GeneratorState::Running {
            reject_running_generator(eg);
        } else if state == crate::vm::generator::GeneratorState::Suspended {
            if gen_ref.borrow().rewindable {
                gen_ref.borrow_mut().rewindable = false;
            }
            resume_generator_method(ed, eg, &gen_ref, send_val)?;
        }

        // Return current yielded value
        let val = gen_ref.borrow().value.clone();
        ret!(rv, val);
    }
    ret!(rv, Value::null());
}

fn fn_generator_get_return(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(ed, &gen_ref, eg)?;
        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
        let gen_data = gen_ref.borrow();
        if gen_data.state != crate::vm::generator::GeneratorState::Completed
            || !gen_data.has_returned
        {
            drop(gen_data);
            eg.exception = Some(crate::value::make_error_value(
                "Exception",
                "Cannot get return value of a generator that hasn't returned",
            ));
            ret!(rv, Value::null());
        }
        ret!(rv, gen_data.return_value.clone());
    }
    ret!(rv, Value::null());
}

fn fn_generator_throw(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exception = arg!(ed, 1).dereferenced().clone();
    let is_throwable = exception
        .as_object()
        .is_some_and(|object| eg.class_is_a(object.class_name.as_ref(), "Throwable"));
    if !is_throwable {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "Generator::throw(): Argument #1 ($exception) must be of type Throwable, {} given",
                exception.diagnostic_type_name()
            ),
        ));
        ret!(rv, Value::null());
    }

    if let Some(gen_ref) = get_generator_ref(ed) {
        if gen_ref.borrow().state == crate::vm::generator::GeneratorState::Created {
            resume_generator_method(ed, eg, &gen_ref, Value::null())?;
            if eg.exception.is_some() {
                ret!(rv, Value::null());
            }
        }

        let state = gen_ref.borrow().state;
        match state {
            crate::vm::generator::GeneratorState::Suspended => {
                if gen_ref.borrow().rewindable {
                    gen_ref.borrow_mut().rewindable = false;
                }
                let saved_execute_data = eg.current_execute_data.replace(ed);
                let outcome = crate::vm::execute::throw_into_generator(eg, &gen_ref, exception);
                eg.current_execute_data.set(saved_execute_data);
                match outcome? {
                    crate::vm::execute::GeneratorResumeOutcome::Advanced => {}
                    crate::vm::execute::GeneratorResumeOutcome::Threw(exception) => {
                        eg.exception = Some(exception);
                    }
                }
            }
            crate::vm::generator::GeneratorState::Completed => {
                eg.exception = Some(exception);
            }
            crate::vm::generator::GeneratorState::Running => {
                reject_running_generator(eg);
            }
            crate::vm::generator::GeneratorState::Created => unreachable!(),
        }

        if eg.exception.is_some() {
            ret!(rv, Value::null());
        }
        ret!(rv, gen_ref.borrow().value.clone());
    }
    ret!(rv, Value::null());
}

// ============================================================================
// Regex (PCRE) functions
// ============================================================================

/// preg_match($pattern, $subject, &$matches = null, $flags = 0, $offset = 0): int|false
fn fn_preg_match(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);
    let flags = arg_opt!(ed, 3).map_or(0, Value::to_long_val);
    let offset_capture = flags & 256 != 0;
    let raw_offset = arg_opt!(ed, 4).map_or(0, Value::to_long_val);
    let subject_len = subject.len() as i64;
    let offset = if raw_offset < 0 {
        (subject_len + raw_offset).max(0)
    } else {
        raw_offset.min(subject_len)
    } as usize;
    let searched_subject = &subject[offset..];

    let has_matches = {
        let raw = unsafe { (*ed).cv(2) };
        !raw.is_undef()
    };

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_e) => {
            // PHP emits a warning and returns false for invalid patterns
            ret!(rv, Value::bool(false));
        }
    };

    if !has_matches {
        ret!(rv, Value::long(re.is_match(searched_subject) as i64));
    }

    match re.captures(searched_subject) {
        Some(caps) => {
            if has_matches {
                let matches_ptr = arg_mut!(ed, 2);
                let mut arr = PhpArray::new();
                for i in 0..caps.len() {
                    let value = match caps.get(i) {
                        Some(m) if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(m.as_str(searched_subject)));
                            pair.push(Value::long((offset + m.start) as i64));
                            Value::array(pair)
                        }
                        Some(m) => Value::string(m.as_str(searched_subject)),
                        None if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(""));
                            pair.push(Value::long(-1));
                            Value::array(pair)
                        }
                        None => Value::string(""),
                    };
                    arr.push(value);
                }
                // Add named capture groups as string-keyed entries
                for (name, &idx) in caps.named_groups() {
                    if let Some(m) = caps.get(idx) {
                        let value = if offset_capture {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(m.as_str(searched_subject)));
                            pair.push(Value::long((offset + m.start) as i64));
                            Value::array(pair)
                        } else {
                            Value::string(m.as_str(searched_subject))
                        };
                        arr.set_str(name, value);
                    }
                }
                if let Some(mark) = caps.mark() {
                    arr.set_str("MARK", Value::string(mark));
                }
                unsafe {
                    std::ptr::drop_in_place(matches_ptr);
                    matches_ptr.write(Value::array(arr));
                }
            }
            ret!(rv, Value::long(1));
        }
        None => {
            if has_matches {
                let matches_ptr = arg_mut!(ed, 2);
                unsafe {
                    std::ptr::drop_in_place(matches_ptr);
                    matches_ptr.write(Value::array(PhpArray::new()));
                }
            }
            ret!(rv, Value::long(0));
        }
    }
}

fn preg_replace_strings(
    eg: &mut ExecutorGlobals,
    patterns: &[String],
    replacements: &[String],
    replacement_is_array: bool,
    subject: &str,
    limit: usize,
) -> Option<(String, usize)> {
    let mut result = subject.to_string();
    let mut count = 0;
    for (index, pattern) in patterns.iter().enumerate() {
        let replacement = if replacement_is_array {
            replacements.get(index).map_or("", String::as_str)
        } else {
            replacements.first().map_or("", String::as_str)
        };
        let regex = eg.regex_cache.get_or_compile(pattern).ok()?;
        let (replaced, replacements) = regex.replace_limit(&result, replacement, limit);
        result = replaced;
        count += replacements;
    }
    Some((result, count))
}

fn preg_replace_argument_strings(value: &Value) -> (Vec<String>, bool) {
    if let Some(values) = value.as_array() {
        (
            values
                .iter()
                .map(|(_, value)| value.dereferenced().echo_to_string())
                .collect(),
            true,
        )
    } else {
        (vec![value.dereferenced().echo_to_string()], false)
    }
}

/// preg_replace($pattern, $replacement, $subject) -> string|array|null
fn fn_preg_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let limit = arg_opt!(ed, 3).map_or(-1, Value::to_long_val);
    let limit = if limit < 0 {
        usize::MAX
    } else {
        usize::try_from(limit).unwrap_or(usize::MAX)
    };
    let has_count = !arg!(ed, 4).is_undef();

    // Preserve the allocation profile of the overwhelmingly common scalar
    // form. Array-capable normalization below necessarily owns each element,
    // but scalar strings can continue to borrow directly from their Values.
    if limit == usize::MAX
        && !has_count
        && arg!(ed, 0).as_array().is_none()
        && arg!(ed, 1).as_array().is_none()
        && arg!(ed, 2).as_array().is_none()
    {
        let pattern = arg_str!(ed, 0);
        let replacement = arg_str!(ed, 1);
        let subject = arg_str!(ed, 2);
        let regex = match eg.regex_cache.get_or_compile(&pattern) {
            Ok(regex) => regex,
            Err(_) => ret!(rv, Value::null()),
        };
        let result = regex.replace_all(&subject, &replacement);
        ret!(rv, Value::string(result));
    }

    let (patterns, _) = preg_replace_argument_strings(arg!(ed, 0));
    let (replacements, replacement_is_array) = preg_replace_argument_strings(arg!(ed, 1));
    let mut total_count = 0;

    if let Some(subjects) = arg!(ed, 2).as_array() {
        let subjects: Vec<_> = subjects
            .iter()
            .map(|(key, value)| (key, value.dereferenced().echo_to_string()))
            .collect();
        let mut result = PhpArray::new();
        for (key, subject) in subjects {
            let Some((replaced, count)) = preg_replace_strings(
                eg,
                &patterns,
                &replacements,
                replacement_is_array,
                &subject,
                limit,
            ) else {
                ret!(rv, Value::null());
            };
            total_count += count;
            match key {
                ArrayKey::Int(key) => result.set_int(key, Value::string(replaced)),
                ArrayKey::String(key) => result.set_str(&key, Value::string(replaced)),
            }
        }
        if has_count {
            arg_mut!(ed, 4, Value::long(total_count as i64));
        }
        ret!(rv, Value::array(result));
    }

    let subject = arg!(ed, 2).dereferenced().echo_to_string();
    let Some((result, count)) = preg_replace_strings(
        eg,
        &patterns,
        &replacements,
        replacement_is_array,
        &subject,
        limit,
    ) else {
        ret!(rv, Value::null());
    };
    if has_count {
        arg_mut!(ed, 4, Value::long(count as i64));
    }
    ret!(rv, Value::string(result));
}

// ============================================================================
// Callable functions
// ============================================================================

/// Find a class without allocating a normalized name. Runtime-originated names
/// (for example an object's class name) hit the exact lookup; unusual casing
/// falls back to a case-insensitive scan for PHP compatibility.
#[inline]
fn find_class_case_insensitive<'a>(
    eg: &'a ExecutorGlobals,
    class_name: &str,
) -> Option<&'a crate::compiler::compile::ClassDef> {
    eg.find_class(class_name)
}

/// PHP's method_exists() is a declaration probe, not a callability check: it
/// sees abstract, private and protected methods across the full hierarchy.
fn method_declared_in_class_hierarchy(
    eg: &ExecutorGlobals,
    class_name: &str,
    method_name: &str,
) -> bool {
    let mut current = find_class_case_insensitive(eg, class_name);
    while let Some(class) = current {
        if class
            .methods
            .iter()
            .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case(method_name))
        {
            return true;
        }
        if class.uses.iter().any(|trait_name| {
            find_class_case_insensitive(eg, trait_name).is_some_and(|trait_def| {
                trait_def
                    .methods
                    .iter()
                    .any(|(name, _, _, _, _)| name.eq_ignore_ascii_case(method_name))
            })
        }) {
            return true;
        }
        current = class
            .parent
            .as_deref()
            .and_then(|parent| find_class_case_insensitive(eg, parent));
    }
    false
}

/// Search for a method in a class hierarchy and return its direct function
/// pointer. This avoids rebuilding `class::method` strings and looking the
/// method up a second time in the global function table.
fn find_method_in_class_hierarchy<'a>(
    eg: &'a ExecutorGlobals,
    class_name: &str,
    method_name: &str,
) -> Option<(Visibility, bool, *const FunctionCommon, &'a str)> {
    let mut current = find_class_case_insensitive(eg, class_name);
    while let Some(class) = current {
        if let Some((_, visibility, is_static, _, function)) =
            class.methods.iter().find(|(name, _, _, _, _)| {
                name.eq_ignore_ascii_case(method_name) && !class.method_is_abstract(name)
            })
        {
            return Some((
                *visibility,
                *is_static,
                &function.common as *const FunctionCommon,
                class.name.as_str(),
            ));
        }

        // Trait methods keep the using class as their visibility scope, which
        // matches the previous resolver and PHP's composed-method semantics.
        for trait_name in &class.uses {
            let Some(trait_def) = find_class_case_insensitive(eg, trait_name) else {
                continue;
            };
            if let Some((_, visibility, is_static, _, function)) =
                trait_def.methods.iter().find(|(name, _, _, _, _)| {
                    name.eq_ignore_ascii_case(method_name) && !trait_def.method_is_abstract(name)
                })
            {
                return Some((
                    *visibility,
                    *is_static,
                    &function.common as *const FunctionCommon,
                    class.name.as_str(),
                ));
            }
        }

        // Internal methods live only in the function table. Check them after
        // ClassDef metadata so user methods retain their explicit static flag.
        // Inherited aliases also live in the child namespace, but their user
        // ABI does not encode `static` in `this_offset`; defer those aliases
        // to the declaring parent's metadata instead of misclassifying them.
        if let Some(function) = eg.find_function(&format!("{}::{method_name}", class.name))
            && eg
                .declaring_class_of(function)
                .is_some_and(|declaring| declaring.eq_ignore_ascii_case(&class.name))
        {
            // SAFETY: request function-table entries retain immutable
            // FunctionCommon metadata for the complete callback lookup.
            return Some((
                Visibility::Public,
                eg.internal_method_is_static(function)
                    || unsafe { (*function).sig.this_offset == 0 },
                function,
                class.name.as_str(),
            ));
        }

        current = class
            .parent
            .as_deref()
            .and_then(|parent| find_class_case_insensitive(eg, parent));
    }
    None
}

/// Invoke a public instance method selected by a VM protocol operation such as
/// ArrayAccess. Internal methods live in the function table while user methods
/// carry their direct pointer in ClassDef, so the protocol boundary resolves
/// both without manufacturing a PHP callback array.
pub(crate) fn call_object_protocol_method(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    interface: &str,
    method: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let public_method = if method.eq_ignore_ascii_case("offsetSetAppend") {
        "offsetSet"
    } else {
        method
    };
    let Some(object) = receiver.as_object() else {
        return Ok(None);
    };
    let class_name = object.class_name.to_string();
    drop(object);
    if !eg.class_is_a(&class_name, interface) {
        return Ok(None);
    }
    if class_name == "WeakMap" && interface == "ArrayAccess" {
        return weak::call_map_protocol(eg, receiver, method, args).map(Some);
    }
    call_object_public_method(eg, receiver, public_method, args)
}

/// Resolve an ordinary public instance method without manufacturing a PHP
/// callback value. Cold protocol paths can retain this descriptor when they
/// need to inspect hook availability before invoking it.
pub(crate) fn resolve_object_public_method(
    eg: &ExecutorGlobals,
    receiver: &Value,
    method: &str,
) -> Option<ResolvedCallback> {
    let object = receiver.as_object()?;
    let class_name = object.class_name.to_string();
    let class_id = object.class_id;
    drop(object);

    let internal_name = format!("{class_name}::{method}");
    let func_ptr = if let Some(function) = eg.find_function(&internal_name) {
        function
    } else {
        let (visibility, is_static, function, _) =
            find_method_in_class_hierarchy(eg, &class_name, method)?;
        if visibility != Visibility::Public || is_static {
            return None;
        }
        function
    };
    Some(ResolvedCallback {
        func_ptr,
        prepend_args: vec![receiver.clone()],
        use_vars: vec![],
        called_scope_class_id: class_id,
        bound_this: None,
        closure_static_vars: None,
        is_magic_call: false,
    })
}

/// Invoke an ordinary public instance method without constructing a callback
/// descriptor. Serialization hooks and VM protocols share this cold path.
pub(crate) fn call_object_public_method(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    method: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(resolved) = resolve_object_public_method(eg, receiver, method) else {
        return Ok(None);
    };
    call_resolved_with_values(eg, &resolved, args).map(Some)
}

/// Result of resolving a callback: func pointer + args to prepend (e.g. $this, use_vars).
pub(crate) struct ResolvedCallback {
    pub(crate) func_ptr: *const FunctionCommon,
    /// Args to prepend before user-supplied args.
    /// For plain functions: empty.
    /// For method calls: [$this].
    /// For closures: use_vars (appended after user args, not prepended).
    pub(crate) prepend_args: Vec<Value>,
    /// Captured use_vars for closures (appended after all params).
    pub(crate) use_vars: Vec<Value>,
    /// Lexical visibility/late-static scope carried by a bound closure.
    pub(crate) called_scope_class_id: u32,
    /// Object bound as `$this`; it is frame metadata, not a public argument.
    pub(crate) bound_this: Option<Value>,
    /// Per-object function-static cells when this descriptor came from an
    /// anonymous Closure. Named functions and methods leave this empty.
    pub(crate) closure_static_vars: Option<ClosureStaticVars>,
    /// Invocation must pack the requested method name and public arguments for
    /// a resolved `__call` or `__callStatic` trampoline.
    pub(crate) is_magic_call: bool,
}

pub(crate) struct ShutdownFunction {
    callback: ResolvedCallback,
    arguments: Vec<Value>,
}

impl ShutdownFunction {
    fn into_release_roots(self) -> Vec<Value> {
        let ResolvedCallback {
            prepend_args,
            use_vars,
            bound_this,
            ..
        } = self.callback;
        let mut roots = prepend_args;
        roots.extend(use_vars);
        roots.extend(bound_this);
        roots.extend(self.arguments);
        roots
    }
}

impl Clone for ResolvedCallback {
    fn clone(&self) -> Self {
        Self {
            func_ptr: self.func_ptr,
            prepend_args: self.prepend_args.clone(),
            use_vars: self
                .use_vars
                .iter()
                .map(Value::clone_closure_capture)
                .collect(),
            called_scope_class_id: self.called_scope_class_id,
            bound_this: self.bound_this.clone(),
            closure_static_vars: self.closure_static_vars.clone(),
            is_magic_call: self.is_magic_call,
        }
    }
}

impl ResolvedCallback {
    #[inline]
    fn metadata(&self) -> (&FunctionCommon, Option<&crate::compiler::OpArray>) {
        // SAFETY: callback resolution only publishes pointers owned by the
        // request's immutable function table, which outlives the descriptor;
        // the common header tag is checked before borrowing a UserFunction.
        unsafe {
            let common = &*self.func_ptr;
            let user = (common.fn_type == FunctionType::User)
                .then(|| &(*(self.func_ptr as *const crate::vm::function::UserFunction)).op_array);
            (common, user)
        }
    }

    #[inline(always)]
    fn plain_function(func_ptr: *const FunctionCommon) -> Self {
        Self {
            func_ptr,
            prepend_args: vec![],
            use_vars: vec![],
            called_scope_class_id: 0,
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        }
    }

    #[inline]
    pub(crate) fn has_context(&self) -> bool {
        self.called_scope_class_id != 0 || self.bound_this.is_some()
    }

    /// Recover the immutable common header retained by the request for this
    /// descriptor's lifetime.
    #[inline]
    pub(crate) fn common(&self) -> &FunctionCommon {
        self.metadata().0
    }

    #[inline]
    fn requires_live_internal_trace_caller(&self) -> bool {
        self.metadata().1.is_none_or(|op_array| {
            op_array.instructions.iter().any(|instruction| {
                matches!(
                    instruction.opcode,
                    OpCode::Echo
                        | OpCode::InitFcall
                        | OpCode::DoFcall
                        | OpCode::CallUserFuncArray
                        | OpCode::InitUserCall
                        | OpCode::InitMethodCall
                        | OpCode::InitStaticCall
                        | OpCode::InitDynamicCall
                        | OpCode::InitLateStaticCall
                        | OpCode::Throw
                        | OpCode::NewObj
                        | OpCode::FetchObjR
                        | OpCode::AssignObjProp
                        | OpCode::AssignObjDim
                        | OpCode::IssetObj
                        | OpCode::UnsetObj
                        | OpCode::BindObjPropRef
                        | OpCode::FetchDimR
                        | OpCode::AssignDim
                        | OpCode::UnsetDim
                        | OpCode::BindArrayDimRef
                        | OpCode::ForeachInit
                        | OpCode::ForeachNext
                        | OpCode::ForeachNextRef
                        | OpCode::ForeachNextPlain
                        | OpCode::Cast
                        | OpCode::Instanceof
                        | OpCode::CloneObj
                        | OpCode::Include
                        | OpCode::Eval
                        | OpCode::AssertCheck
                        | OpCode::Yield
                        | OpCode::YieldFrom
                )
            })
        })
    }

    #[inline]
    fn signature(&self) -> &crate::vm::function::SignatureInfo {
        &self.common().sig
    }

    pub(crate) fn supports_suspended_root(&self) -> bool {
        // SAFETY: callback pointers are request-owned immutable descriptors;
        // the discriminant is checked before reading the UserFunction tail.
        unsafe {
            (*self.func_ptr).fn_type == FunctionType::User
                && !(*(self.func_ptr as *const crate::vm::function::UserFunction))
                    .op_array
                    .is_generator
                && !self.is_magic_call
        }
    }

    #[inline]
    pub(crate) fn is_method(&self) -> bool {
        self.signature().this_offset == 1
    }
}

pub(crate) fn resolved_callback_into_closure(
    resolved: ResolvedCallback,
    eg: &ExecutorGlobals,
) -> Value {
    let trait_scope_class_id = if resolved.common().plan.needs_trait_class_scope() {
        let dispatch_class = resolved
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
            });
        eg.declaring_class_of(resolved.func_ptr)
            .filter(|declared| {
                eg.find_class(declared)
                    .is_some_and(|definition| definition.is_trait)
            })
            .and_then(|declared| eg.trait_composition_scope(dispatch_class.as_deref()?, declared))
            .map_or(0, |scope| eg.class_id_of(scope))
    } else {
        0
    };
    let is_method = resolved.is_method();
    let bound_this = resolved.bound_this.or_else(|| {
        resolved
            .prepend_args
            .first()
            .filter(|value| value.value_type() == ValueType::Object)
            .cloned()
    });
    let is_static =
        bound_this.is_none() && !is_method && eg.declaring_class_of(resolved.func_ptr).is_some();
    let has_heap_captures = resolved.use_vars.iter().any(Value::needs_cleanup);
    let static_vars = resolved.closure_static_vars;
    Value::closure(PhpClosure {
        object_handle: 0,
        func: resolved.func_ptr,
        called_scope_class_id: resolved.called_scope_class_id,
        trait_scope_class_id,
        is_static,
        bound_this,
        captures: resolved.use_vars,
        static_vars,
        has_heap_captures,
        scope_is_dummy: false,
    })
}

#[inline]
pub(crate) fn scope_introspection_callback_name(
    resolved: &ResolvedCallback,
) -> Option<&'static str> {
    // SAFETY: resolved callback pointers come from the request-owned immutable
    // function table and remain live for the descriptor's lifetime.
    let function = unsafe { Function::from_common_ptr(resolved.func_ptr) };
    let handler = function.dispatch(|_| None, |internal| Some(internal.handler))?;
    let name = if std::ptr::fn_addr_eq(
        handler,
        fn_extract as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("extract")
    } else if std::ptr::fn_addr_eq(
        handler,
        fn_compact as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("compact")
    } else if std::ptr::fn_addr_eq(
        handler,
        fn_get_defined_vars as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("get_defined_vars")
    } else if std::ptr::fn_addr_eq(
        handler,
        fn_func_get_args as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("func_get_args")
    } else if std::ptr::fn_addr_eq(
        handler,
        fn_func_get_arg as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("func_get_arg")
    } else if std::ptr::fn_addr_eq(
        handler,
        fn_func_num_args as crate::vm::function::InternalFunctionHandler,
    ) {
        Some("func_num_args")
    } else {
        None
    };
    name
}

#[inline]
fn reject_scope_introspection_callback(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
) -> bool {
    if let Some(error) = scope_introspection_callback_error(resolved) {
        eg.exception = Some(error);
        true
    } else {
        false
    }
}

#[inline]
fn scope_introspection_callback_error(resolved: &ResolvedCallback) -> Option<Value> {
    scope_introspection_callback_name(resolved).map(|name| {
        crate::value::make_error_value("Error", &format!("Cannot call {name}() dynamically"))
    })
}

/// Get the calling scope's class name from an ExecuteData frame.
/// Walks prev_execute_data to find the caller's declaring class.
fn get_calling_scope_class<'a>(
    ed: *mut crate::vm::frame::ExecuteData,
    eg: &'a ExecutorGlobals,
) -> Option<&'a str> {
    if ed.is_null() {
        return None;
    }
    // SAFETY: an internal handler receives its live frame and executes
    // synchronously beneath a caller frame that remains allocated until the
    // handler returns. Function metadata is request-owned immutable storage.
    unsafe {
        let caller = (*ed).prev_execute_data;
        if caller.is_null() || (*caller).func.is_null() {
            return None;
        }
        eg.declaring_class_of((*caller).func)
    }
}

fn resolve_magic_callback(
    eg: &ExecutorGlobals,
    class_name: &str,
    requested_method: &str,
    magic_method: &str,
    receiver: Option<&Value>,
) -> Option<ResolvedCallback> {
    let (visibility, is_static, func_ptr, _) =
        find_method_in_class_hierarchy(eg, class_name, magic_method)?;
    if visibility != Visibility::Public || (receiver.is_none() && !is_static) {
        return None;
    }
    Some(ResolvedCallback {
        func_ptr,
        prepend_args: vec![receiver.cloned().unwrap_or_else(Value::null)],
        use_vars: vec![Value::string(requested_method)],
        called_scope_class_id: eg.find_class(class_name)?.class_id,
        bound_this: None,
        closure_static_vars: None,
        is_magic_call: true,
    })
}

/// Legacy PHP callback spellings that carry a class-scope keyword are still
/// accepted in PHP 8.5, but every attempt to consume one is deprecated. Keep
/// recognition and semantic resolution separate from diagnostic delivery so
/// compiler-lowered calls and internal callback consumers can report at their
/// own source boundary without duplicating method lookup.
pub(crate) enum LegacyCallbackResolution {
    NotLegacy,
    Legacy {
        resolved: Option<ResolvedCallback>,
        deprecation: Option<String>,
    },
}

#[inline]
fn legacy_relative_owner(
    eg: &ExecutorGlobals,
    relative: &str,
    lexical_class: Option<&str>,
    called_class: Option<&str>,
) -> Option<String> {
    if relative.eq_ignore_ascii_case("self") {
        lexical_class.map(str::to_owned)
    } else if relative.eq_ignore_ascii_case("parent") {
        lexical_class
            .and_then(|class| eg.find_class(class))
            .and_then(|class| class.parent.clone())
    } else if relative.eq_ignore_ascii_case("static") {
        called_class.map(str::to_owned)
    } else {
        None
    }
}

#[inline]
fn legacy_called_scope_id(eg: &ExecutorGlobals, owner: &str, called_class: Option<&str>) -> u32 {
    called_class
        .filter(|called| eg.class_is_a(called, owner))
        .map_or_else(|| eg.class_id_of(owner), |called| eg.class_id_of(called))
}

fn resolve_legacy_scoped_method(
    eg: &ExecutorGlobals,
    owner: &str,
    method: &str,
    lexical_class: Option<&str>,
    called_class: Option<&str>,
    receiver: Option<&Value>,
    object_form: bool,
) -> Option<ResolvedCallback> {
    let Some((visibility, is_static, func_ptr, declaring)) =
        find_method_in_class_hierarchy(eg, owner, method)
    else {
        let compatible_receiver = receiver.filter(|receiver| {
            receiver
                .as_object()
                .is_some_and(|object| eg.class_is_a(&object.class_name, owner))
        });
        let mut resolved = compatible_receiver
            .and_then(|receiver| {
                resolve_magic_callback(eg, owner, method, "__call", Some(receiver))
            })
            .or_else(|| {
                (!object_form)
                    .then(|| resolve_magic_callback(eg, owner, method, "__callStatic", None))
                    .flatten()
            })?;
        if let Some(object) = receiver.and_then(Value::as_object) {
            resolved.called_scope_class_id = object.class_id;
        } else {
            resolved.called_scope_class_id = legacy_called_scope_id(eg, owner, called_class);
        }
        return Some(resolved);
    };
    if !eg.check_visibility(lexical_class, declaring, visibility) {
        return None;
    }
    if is_static {
        return Some(ResolvedCallback {
            func_ptr,
            prepend_args: vec![Value::null()],
            use_vars: vec![],
            called_scope_class_id: legacy_called_scope_id(eg, owner, called_class),
            bound_this: None,
            closure_static_vars: None,
            is_magic_call: false,
        });
    }

    let receiver = receiver?.clone();
    let object = receiver.as_object()?;
    if !eg.class_is_a(&object.class_name, owner) {
        return None;
    }
    let called_scope_class_id = object.class_id;
    drop(object);
    Some(ResolvedCallback {
        func_ptr,
        prepend_args: vec![receiver.clone()],
        use_vars: vec![],
        called_scope_class_id,
        bound_this: Some(receiver),
        closure_static_vars: None,
        is_magic_call: false,
    })
}

/// Resolve the PHP 8.5 legacy `self`/`parent`/`static` callback forms.
///
/// `lexical_class` is the declaring scope of the consuming call site, while
/// `called_class` is its forwarding late-static scope. `receiver` is the live
/// `$this`, if any. A matched-but-invalid form remains distinguishable from an
/// ordinary callback because PHP reports its deprecation before rejecting it.
pub(crate) fn resolve_legacy_callback(
    val: &Value,
    eg: &ExecutorGlobals,
    lexical_class: Option<&str>,
    called_class: Option<&str>,
    receiver: Option<&Value>,
) -> LegacyCallbackResolution {
    if let Some(name) = val.as_str()
        && let Some((relative, method)) = name.rsplit_once("::")
        && matches!(
            relative.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
    {
        let relative = relative.to_ascii_lowercase();
        let owner = legacy_relative_owner(eg, &relative, lexical_class, called_class);
        let deprecation = owner
            .as_ref()
            .map(|_| format!("Use of \"{relative}\" in callables is deprecated"));
        let resolved = owner.and_then(|owner| {
            resolve_legacy_scoped_method(
                eg,
                &owner,
                method,
                lexical_class,
                called_class,
                receiver,
                false,
            )
        });
        return LegacyCallbackResolution::Legacy {
            resolved,
            deprecation,
        };
    }

    let Some(array) = val.as_array() else {
        return LegacyCallbackResolution::NotLegacy;
    };
    if array.len() != 2 {
        return LegacyCallbackResolution::NotLegacy;
    }
    let Some(first) = array.get_value_at(0) else {
        return LegacyCallbackResolution::NotLegacy;
    };
    let Some(method) = array.get_value_at(1).and_then(Value::as_str) else {
        return LegacyCallbackResolution::NotLegacy;
    };

    if let Some(relative) = first.as_str()
        && matches!(
            relative.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
        && !method.contains("::")
    {
        let relative = relative.to_ascii_lowercase();
        let owner = legacy_relative_owner(eg, &relative, lexical_class, called_class);
        let deprecation = owner
            .as_ref()
            .map(|_| format!("Use of \"{relative}\" in callables is deprecated"));
        let resolved = owner.and_then(|owner| {
            resolve_legacy_scoped_method(
                eg,
                &owner,
                method,
                lexical_class,
                called_class,
                receiver,
                false,
            )
        });
        return LegacyCallbackResolution::Legacy {
            resolved,
            deprecation,
        };
    }

    let Some((qualifier, bare_method)) = method.rsplit_once("::") else {
        return LegacyCallbackResolution::NotLegacy;
    };
    let (receiver_class, callback_receiver, object_form) = if let Some(object) = first.as_object() {
        (object.class_name.to_string(), Some(first), true)
    } else if let Some(class) = first.as_str() {
        let class = class.trim_start_matches('\\').to_string();
        let live_receiver = receiver.filter(|candidate| {
            candidate
                .as_object()
                .is_some_and(|object| eg.class_is_a(&object.class_name, &class))
        });
        (class, live_receiver, false)
    } else {
        return LegacyCallbackResolution::NotLegacy;
    };
    let display = receiver_class.clone();
    let owner = if qualifier.eq_ignore_ascii_case("self") {
        Some(receiver_class.clone())
    } else if qualifier.eq_ignore_ascii_case("parent") {
        eg.find_class(&receiver_class)
            .and_then(|class| class.parent.clone())
    } else if qualifier.eq_ignore_ascii_case("static") {
        called_class.map(str::to_owned)
    } else {
        Some(qualifier.trim_start_matches('\\').to_string())
    };
    let owner = owner.filter(|owner| eg.class_is_a(&receiver_class, owner));
    let deprecation = owner
        .as_ref()
        .map(|_| format!("Callables of the form [\"{display}\", \"{method}\"] are deprecated"));
    let resolved = owner.and_then(|owner| {
        let forwarding_class = called_class
            .filter(|called| eg.class_is_a(called, &owner))
            .or(Some(owner.as_str()));
        resolve_legacy_scoped_method(
            eg,
            &owner,
            bare_method,
            lexical_class,
            forwarding_class,
            callback_receiver,
            object_form,
        )
    });
    LegacyCallbackResolution::Legacy {
        resolved,
        deprecation,
    }
}

#[inline]
fn is_relative_scope_keyword(name: &str) -> bool {
    name.eq_ignore_ascii_case("self")
        || name.eq_ignore_ascii_case("parent")
        || name.eq_ignore_ascii_case("static")
}

#[inline]
pub(crate) fn callback_uses_legacy_scope(val: &Value) -> bool {
    if let Some(name) = val.as_str()
        && let Some((relative, _)) = name.rsplit_once("::")
    {
        return is_relative_scope_keyword(relative);
    }
    let Some(array) = val.as_array() else {
        return false;
    };
    if array.len() != 2 {
        return false;
    }
    array
        .get_value_at(0)
        .and_then(Value::as_str)
        .is_some_and(is_relative_scope_keyword)
        || array
            .get_value_at(1)
            .and_then(Value::as_str)
            .is_some_and(|method| method.contains("::"))
}

/// Explain a matched legacy callback that could not be resolved. Callback
/// wrappers prepend their own function/argument wording; this helper owns the
/// class-scope-specific tail so compiler-lowered and internal paths agree.
pub(crate) fn legacy_callback_invalid_reason(
    val: &Value,
    eg: &ExecutorGlobals,
    lexical_class: Option<&str>,
    called_class: Option<&str>,
    receiver: Option<&Value>,
) -> String {
    let inaccessible_or_missing = |owner: &str, method: &str| {
        if let Some((visibility, is_static, _, defining)) =
            find_method_in_class_hierarchy(eg, owner, method)
        {
            if !eg.check_visibility(lexical_class, defining, visibility) {
                let visibility = match visibility {
                    Visibility::Private => "private",
                    Visibility::Protected => "protected",
                    Visibility::Public => "public",
                };
                return format!("cannot access {visibility} method {owner}::{method}()");
            }
            if !is_static
                && !receiver.is_some_and(|receiver| {
                    receiver
                        .as_object()
                        .is_some_and(|object| eg.class_is_a(&object.class_name, owner))
                })
            {
                return format!(
                    "non-static method {owner}::{method}() cannot be called statically"
                );
            }
        }
        format!("class {owner} does not have a method \"{method}\"")
    };

    if let Some(name) = val.as_str()
        && let Some((relative, method)) = name.rsplit_once("::")
        && matches!(
            relative.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
    {
        return legacy_relative_owner(eg, relative, lexical_class, called_class).map_or_else(
            || {
                format!(
                    "cannot access \"{}\" when no class scope is active",
                    relative.to_ascii_lowercase()
                )
            },
            |owner| inaccessible_or_missing(&owner, method),
        );
    }

    let Some(array) = val.as_array() else {
        return "no array or string given".to_string();
    };
    let Some(first) = array.get_value_at(0) else {
        return "no array or string given".to_string();
    };
    let Some(method) = array.get_value_at(1).and_then(Value::as_str) else {
        return "no array or string given".to_string();
    };
    if let Some(relative) = first.as_str()
        && matches!(
            relative.to_ascii_lowercase().as_str(),
            "self" | "parent" | "static"
        )
        && !method.contains("::")
    {
        return legacy_relative_owner(eg, relative, lexical_class, called_class).map_or_else(
            || {
                format!(
                    "cannot access \"{}\" when no class scope is active",
                    relative.to_ascii_lowercase()
                )
            },
            |owner| inaccessible_or_missing(&owner, method),
        );
    }
    let receiver_class = first
        .as_object()
        .map(|object| object.class_name.to_string())
        .or_else(|| {
            first
                .as_str()
                .map(|class| class.trim_start_matches('\\').to_string())
        });
    let Some(receiver_class) = receiver_class else {
        return "first array member is not a valid class name or object".to_string();
    };
    let Some((qualifier, bare_method)) = method.rsplit_once("::") else {
        return "no array or string given".to_string();
    };
    let owner = if qualifier.eq_ignore_ascii_case("self") {
        Some(receiver_class)
    } else if qualifier.eq_ignore_ascii_case("parent") {
        eg.find_class(&receiver_class)
            .and_then(|class| class.parent.clone())
    } else if qualifier.eq_ignore_ascii_case("static") {
        called_class.map(str::to_owned)
    } else {
        Some(qualifier.trim_start_matches('\\').to_string())
    };
    owner.map_or_else(
        || {
            format!(
                "cannot access \"{}\" when no class scope is active",
                qualifier.to_ascii_lowercase()
            )
        },
        |owner| inaccessible_or_missing(&owner, bare_method),
    )
}

#[cold]
#[inline(never)]
pub(crate) fn closure_is_magic_call(closure: &PhpClosure, eg: &ExecutorGlobals) -> bool {
    if closure.captures.len() != 1 || closure.captures[0].as_str().is_none() {
        return false;
    }
    let Some(class) = eg.declaring_class_of(closure.func) else {
        return false;
    };
    ["__call", "__callStatic"].into_iter().any(|method| {
        eg.find_function(&format!("{class}::{method}"))
            .is_some_and(|function| std::ptr::eq(function, closure.func))
    })
}

/// Resolve a callback value to a function pointer.
/// Supports: string (function name), array [func_name, use_vars...] (closure),
/// array [object, "method"], and objects with __invoke.
/// `caller_class` is the class scope of the call site — used to allow
/// private/protected method callbacks when called from the declaring class.
#[inline]
pub(crate) fn is_property_hook_method_name(method: &str) -> bool {
    method
        .strip_prefix('$')
        .and_then(|name| name.rsplit_once("::"))
        .is_some_and(|(property, hook)| {
            !property.is_empty()
                && (hook.eq_ignore_ascii_case("get") || hook.eq_ignore_ascii_case("set"))
        })
}

/// PHP accepts one leading namespace separator on a string function callable,
/// but keeps empty names and names beginning with multiple separators invalid.
pub(crate) fn dynamic_function_lookup_name(name: &str) -> &str {
    match name.as_bytes() {
        [b'\\', next, ..] if *next != b'\\' => &name[1..],
        _ => name,
    }
}

fn resolve_callback(
    val: &Value,
    eg: &ExecutorGlobals,
    caller_class: Option<&str>,
) -> Option<ResolvedCallback> {
    match val.value_type() {
        ValueType::Closure => {
            let closure = val.as_closure().unwrap();
            // An anonymous closure created in class scope has a declaring
            // class for visibility, but it still has no hidden method `$this`
            // parameter. Only an actual method signature reserves that slot.
            let resolved = ResolvedCallback {
                func_ptr: closure.func,
                prepend_args: vec![],
                use_vars: closure.clone_captures(),
                called_scope_class_id: closure.called_scope_class_id,
                bound_this: closure.bound_this.clone(),
                closure_static_vars: closure.static_vars.clone(),
                is_magic_call: closure_is_magic_call(closure, eg),
            };
            let prepend_args = if resolved.signature().this_offset == 1 {
                vec![closure.bound_this.clone().unwrap_or_else(Value::null)]
            } else {
                vec![]
            };
            Some(ResolvedCallback {
                prepend_args,
                ..resolved
            })
        }
        ValueType::String => {
            let name = val.as_str().unwrap();
            if let Some((class_name, method_name)) = name.rsplit_once("::") {
                let class_name = class_name.trim_start_matches('\\');
                let Some((visibility, is_static, func_ptr, _)) =
                    find_method_in_class_hierarchy(eg, class_name, method_name)
                else {
                    return resolve_magic_callback(
                        eg,
                        class_name,
                        method_name,
                        "__callStatic",
                        None,
                    );
                };
                if visibility != Visibility::Public || !is_static {
                    return resolve_magic_callback(
                        eg,
                        class_name,
                        method_name,
                        "__callStatic",
                        None,
                    );
                }
                return Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![Value::null()],
                    use_vars: vec![],
                    called_scope_class_id: eg.find_class(class_name)?.class_id,
                    bound_this: None,
                    closure_static_vars: None,
                    is_magic_call: false,
                });
            }
            eg.find_function(dynamic_function_lookup_name(name))
                .map(|ptr| ResolvedCallback {
                    func_ptr: ptr,
                    prepend_args: vec![],
                    use_vars: vec![],
                    called_scope_class_id: 0,
                    bound_this: None,
                    closure_static_vars: None,
                    is_magic_call: false,
                })
        }
        ValueType::Array => {
            let arr = val.as_array()?;
            if arr.is_empty() {
                return None;
            }

            // Case 1: Closure descriptor array [func_name_string, use_val1, ...]
            if let Some(func_name) = arr.get_value_at(0)?.as_str() {
                if func_name.starts_with("__closure_") {
                    let func_ptr = eg.find_function(func_name)?;
                    let use_vars: Vec<Value> = arr.values().skip(1).cloned().collect();
                    return Some(ResolvedCallback {
                        func_ptr,
                        prepend_args: vec![],
                        use_vars,
                        called_scope_class_id: 0,
                        bound_this: None,
                        closure_static_vars: None,
                        is_magic_call: false,
                    });
                }
            }

            // Case 2: Method callback [object_or_class, "method_name"]
            if arr.len() != 2 {
                return None;
            }
            let obj_val = arr.get_value_at(0)?;
            let method_val = arr.get_value_at(1)?;
            let method_name = method_val.as_str()?;
            // A qualified array method (`self::m`, `parent::m`, `A::m`)
            // belongs to PHP 8.5's deprecated legacy-callable grammar. Leave
            // it unresolved here so the consuming boundary can diagnose and
            // resolve it with lexical/called scope instead of treating it as
            // an ordinary magic method name.
            if method_name.contains("::") && !is_property_hook_method_name(method_name) {
                return None;
            }
            if obj_val.value_type() == ValueType::Closure
                && method_name.eq_ignore_ascii_case("__invoke")
            {
                return resolve_callback(obj_val, eg, caller_class);
            }
            if let Some(obj) = obj_val.as_object() {
                // Instance method: [$obj, "method"]
                // Public: always callable. Private/protected: only from declaring scope.
                let class_name = obj.class_name.to_string();
                let called_scope_class_id = obj.class_id;
                drop(obj);
                if is_property_hook_method_name(method_name) {
                    return resolve_magic_callback(
                        eg,
                        &class_name,
                        method_name,
                        "__call",
                        Some(obj_val),
                    );
                }
                let Some((visibility, _, func_ptr, declaring)) =
                    find_method_in_class_hierarchy(eg, &class_name, method_name)
                else {
                    return resolve_magic_callback(
                        eg,
                        &class_name,
                        method_name,
                        "__call",
                        Some(obj_val),
                    );
                };
                match visibility {
                    Visibility::Public => {}
                    Visibility::Protected => {
                        // Protected: caller must be in the same hierarchy
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(&class_name, cc) || eg.class_is_a(cc, &class_name)
                        });
                        if !allowed {
                            return resolve_magic_callback(
                                eg,
                                &class_name,
                                method_name,
                                "__call",
                                Some(obj_val),
                            );
                        }
                    }
                    Visibility::Private => {
                        // Private: caller must be exactly the declaring class
                        let allowed =
                            caller_class.map_or(false, |cc| cc.eq_ignore_ascii_case(declaring));
                        if !allowed {
                            return resolve_magic_callback(
                                eg,
                                &class_name,
                                method_name,
                                "__call",
                                Some(obj_val),
                            );
                        }
                    }
                }
                Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![obj_val.clone()],
                    use_vars: vec![],
                    called_scope_class_id,
                    bound_this: None,
                    closure_static_vars: None,
                    is_magic_call: false,
                })
            } else if let Some(class_str) = obj_val.as_str() {
                // Static method: ["ClassName", "method"] — must be static; visibility depends on scope
                if is_property_hook_method_name(method_name) {
                    return resolve_magic_callback(
                        eg,
                        class_str,
                        method_name,
                        "__callStatic",
                        None,
                    );
                }
                let Some((visibility, is_static, func_ptr, declaring)) =
                    find_method_in_class_hierarchy(eg, class_str, method_name)
                else {
                    return resolve_magic_callback(
                        eg,
                        class_str,
                        method_name,
                        "__callStatic",
                        None,
                    );
                };
                if !is_static {
                    return resolve_magic_callback(
                        eg,
                        class_str,
                        method_name,
                        "__callStatic",
                        None,
                    );
                }
                match visibility {
                    Visibility::Public => {}
                    Visibility::Protected => {
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(class_str, cc) || eg.class_is_a(cc, class_str)
                        });
                        if !allowed {
                            return resolve_magic_callback(
                                eg,
                                class_str,
                                method_name,
                                "__callStatic",
                                None,
                            );
                        }
                    }
                    Visibility::Private => {
                        let allowed =
                            caller_class.map_or(false, |cc| cc.eq_ignore_ascii_case(declaring));
                        if !allowed {
                            return resolve_magic_callback(
                                eg,
                                class_str,
                                method_name,
                                "__callStatic",
                                None,
                            );
                        }
                    }
                }
                Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![Value::null()],
                    use_vars: vec![],
                    called_scope_class_id: eg.find_class(class_str)?.class_id,
                    bound_this: None,
                    closure_static_vars: None,
                    is_magic_call: false,
                })
            } else {
                None
            }
        }
        ValueType::Object => {
            let obj = val.as_object()?;
            let (_, _, func_ptr, _) =
                find_method_in_class_hierarchy(eg, &obj.class_name, "__invoke")?;
            drop(obj);
            Some(ResolvedCallback {
                func_ptr,
                prepend_args: vec![val.clone()],
                use_vars: vec![],
                called_scope_class_id: 0,
                bound_this: None,
                closure_static_vars: None,
                is_magic_call: false,
            })
        }
        _ => None,
    }
}

/// Produce the PHP-facing reason why a first-class callable could not be
/// created. The ordinary callback resolver intentionally returns `Option` for
/// its many legacy callers; this cold diagnostic path preserves the richer
/// error contract without adding work to successful callback dispatch.
pub(crate) fn first_class_callable_error(
    val: &Value,
    eg: &ExecutorGlobals,
    caller_class: Option<&str>,
) -> String {
    let inaccessible_method =
        |visibility: Visibility, defining: &str, method: &str| -> Option<String> {
            if visibility == Visibility::Public
                || eg.check_visibility(caller_class, defining, visibility)
            {
                return None;
            }
            let visibility = match visibility {
                Visibility::Private => "private",
                Visibility::Protected => "protected",
                Visibility::Public => unreachable!(),
            };
            let scope = caller_class.unwrap_or("global");
            let suffix = if caller_class.is_some() {
                format!("scope {scope}")
            } else {
                "global scope".to_string()
            };
            Some(format!(
                "Call to {visibility} method {defining}::{method}() from {suffix}"
            ))
        };

    let method_error = |class_name: &str, method: &str, require_static: bool| {
        let class_name = class_name.trim_start_matches('\\');
        let Some(class) = find_class_case_insensitive(eg, class_name) else {
            return format!("Class \"{class_name}\" not found");
        };
        let Some((visibility, is_static, _, defining)) =
            find_method_in_class_hierarchy(eg, &class.name, method)
        else {
            return format!("Call to undefined method {}::{method}()", class.name);
        };
        if let Some(error) = inaccessible_method(visibility, defining, method) {
            return error;
        }
        if require_static && !is_static {
            return format!(
                "Non-static method {}::{method}() cannot be called statically",
                class.name
            );
        }
        "Failed to create closure from callable".to_string()
    };

    match val.value_type() {
        ValueType::String => {
            let name = val.as_str().unwrap_or("");
            if let Some((class_name, method)) = name.rsplit_once("::") {
                method_error(class_name, method, true)
            } else {
                format!("Call to undefined function {name}()")
            }
        }
        ValueType::Array => {
            let Some(array) = val.as_array() else {
                return "Failed to create closure from callable".to_string();
            };
            if array.len() != 2 {
                return "Failed to create closure from callable".to_string();
            }
            let Some(receiver) = array.get_value_at(0) else {
                return "Failed to create closure from callable".to_string();
            };
            let Some(method) = array.get_value_at(1).and_then(Value::as_str) else {
                return "Failed to create closure from callable".to_string();
            };
            if let Some(object) = receiver.as_object() {
                method_error(&object.class_name, method, false)
            } else if let Some(class_name) = receiver.as_str() {
                method_error(class_name, method, true)
            } else {
                "Failed to create closure from callable".to_string()
            }
        }
        ValueType::Object => {
            let object = val.as_object().unwrap();
            format!("Object of type {} is not callable", object.class_name)
        }
        _ => format!(
            "Value of type {} is not callable",
            val.dereferenced().type_name()
        ),
    }
}

/// Return the otherwise-unused DoFcall inline-cache entry belonging to the PHP
/// instruction that entered the current internal callback helper.
#[inline(always)]
fn callback_cache_slot(ed: *mut ExecuteData) -> Option<*mut InlineCache> {
    if ed.is_null() {
        return None;
    }
    // SAFETY: an internal handler receives its live frame. Its saved user
    // caller, opline and compiler-owned cache table remain live until the
    // synchronous handler returns; all bounds and opcode checks precede the
    // returned entry pointer.
    unsafe {
        let caller = (*ed).prev_execute_data;
        if caller.is_null() {
            return None;
        }

        let func = (*caller).func;
        if func.is_null() || (*func).fn_type != FunctionType::User {
            return None;
        }

        let op_array = (*caller).op_array();
        let opline = (*caller).opline;
        let base = op_array.instructions.as_ptr();
        let byte_offset = (opline as usize).checked_sub(base as usize)?;
        if byte_offset % std::mem::size_of::<crate::vm::instruction::Instruction>() != 0 {
            return None;
        }
        let ip = byte_offset / std::mem::size_of::<crate::vm::instruction::Instruction>();
        if ip >= op_array.instructions.len() || (*opline).opcode != OpCode::DoFcall {
            return None;
        }

        Some(op_array.cache.as_ptr().add(ip) as *mut InlineCache)
    }
}

/// Resolve a plain string callback through the call-site cache. The retained
/// key makes a later mutation COW-detach, and the content comparison also
/// handles an equal callback string coming from a different allocation.
#[inline]
fn resolve_cached_string_callback(
    val: &Value,
    cache_slot: *mut InlineCache,
) -> Option<ResolvedCallback> {
    let name = val.as_str()?;
    let cached_name_ptr = unsafe { (*cache_slot).callback_string() };
    if cached_name_ptr.is_null() {
        return None;
    }
    let current_name_ptr = val.string_rc_ptr()?;
    if current_name_ptr != cached_name_ptr && unsafe { &*cached_name_ptr }.as_str() != name {
        return None;
    }
    let func_ptr = unsafe { (*cache_slot).func };
    if func_ptr.is_null() {
        return None;
    }
    Some(ResolvedCallback::plain_function(func_ptr))
}

#[inline]
fn cache_resolved_string_callback(
    val: &Value,
    resolved: &ResolvedCallback,
    cache_slot: *mut InlineCache,
) {
    let Some(name_ptr) = val.string_rc_ptr() else {
        return;
    };
    let old_ptr = unsafe { (*cache_slot).callback_string() };
    if old_ptr != name_ptr {
        unsafe { Value::retain_cached_string(name_ptr) };
        if !old_ptr.is_null() {
            unsafe { Value::release_cached_string(old_ptr) };
        }
    }
    unsafe { (*cache_slot).set_callback_string(name_ptr, resolved.func_ptr) };
}

#[inline(never)]
fn resolve_literal_string_callback_cache_miss(
    val: &Value,
    eg: &ExecutorGlobals,
    cache_slot: *mut InlineCache,
) -> Option<*const FunctionCommon> {
    let name = val.as_str()?;
    let name_ptr = val.string_rc_ptr()?;
    let cached_name_ptr = unsafe { (*cache_slot).callback_string() };
    if !cached_name_ptr.is_null() {
        if cached_name_ptr == name_ptr || unsafe { &*cached_name_ptr }.as_str() == name {
            let func_ptr = unsafe { (*cache_slot).func };
            return (!func_ptr.is_null()).then_some(func_ptr);
        }

        // Defensive parity with the general resolver. Compiler-proven literal
        // sites are monomorphic, but a mismatched pre-existing cache must not
        // retain an old key or be overwritten repeatedly.
        unsafe { Value::release_cached_string(cached_name_ptr) };
        unsafe { (*cache_slot).disable_callback_string_cache() };
    }

    let func_ptr = eg.find_function(name)?;
    if !unsafe { (*cache_slot).callback_string_cache_disabled() } {
        unsafe { Value::retain_cached_string(name_ptr) };
        unsafe { (*cache_slot).set_callback_string(name_ptr, func_ptr) };
    }
    Some(func_ptr)
}

/// Resolve a compiler-proven immutable String callback through the same
/// DoFcall cache as the canonical collection builtin. The literal identity is
/// monomorphic, so the repeated path stays at one pointer comparison; only
/// first resolution or a defensive mismatch enters the larger resolver.
#[inline(always)]
pub(crate) fn resolve_literal_string_callback_with_cache(
    val: &Value,
    eg: &ExecutorGlobals,
    cache_slot: *mut InlineCache,
) -> Option<*const FunctionCommon> {
    let name_ptr = val.string_rc_ptr()?;
    if unsafe { (*cache_slot).callback_string() } == name_ptr {
        let func_ptr = unsafe { (*cache_slot).func };
        return (!func_ptr.is_null()).then_some(func_ptr);
    }
    resolve_literal_string_callback_cache_miss(val, eg, cache_slot)
}

/// Resolve a callback using an optional monomorphic string cache owned by the
/// PHP instruction that performs the call.
#[inline]
pub(crate) fn resolve_callback_with_cache(
    val: &Value,
    eg: &ExecutorGlobals,
    caller_class: Option<&str>,
    cache_slot: Option<*mut InlineCache>,
) -> Option<ResolvedCallback> {
    // A `Class::method` string carries a hidden method slot and late-static
    // scope. The compact plain-function cache stores neither, so only cache
    // actual function names here.
    let mut cache_slot = if val.value_type() == ValueType::String
        && val.as_str().is_some_and(|name| !name.contains("::"))
    {
        cache_slot
    } else {
        None
    };
    if let Some(slot) = cache_slot {
        if unsafe { (*slot).callback_string_cache_disabled() } {
            cache_slot = None;
        } else {
            let cached_name = unsafe { (*slot).callback_string() };
            if !cached_name.is_null() {
                if let Some(resolved) = resolve_cached_string_callback(val, slot) {
                    return Some(resolved);
                }

                // A second callback name at one instruction makes this site
                // polymorphic. Do not thrash the monomorphic cache forever.
                unsafe { Value::release_cached_string(cached_name) };
                unsafe { (*slot).disable_callback_string_cache() };
                cache_slot = None;
            }
        }
    }

    let resolution_scope = if val.value_type() == ValueType::Array {
        caller_class
    } else {
        None
    };
    let resolved = resolve_callback(val, eg, resolution_scope);

    if let (Some(slot), Some(resolved)) = (cache_slot, resolved.as_ref()) {
        cache_resolved_string_callback(val, resolved, slot);
    }
    resolved
}

/// Resolve a callback from the legacy stdlib wrapper. Only array callbacks
/// need the caller's lexical class for visibility checks.
pub(crate) fn resolve_live_scoped_instance_callback(
    val: &Value,
    eg: &ExecutorGlobals,
    lexical_class: Option<&str>,
    receiver: Option<&Value>,
) -> Option<ResolvedCallback> {
    let receiver = receiver?;
    let (class_name, method_name) = if let Some(name) = val.as_str() {
        let (class, method) = name.rsplit_once("::")?;
        (class.trim_start_matches('\\'), method)
    } else {
        let array = val.as_array()?;
        if array.len() != 2 {
            return None;
        }
        (
            array.get_value_at(0)?.as_str()?.trim_start_matches('\\'),
            array.get_value_at(1)?.as_str()?,
        )
    };
    if method_name.contains("::") && !is_property_hook_method_name(method_name) {
        return None;
    }
    if is_relative_scope_keyword(class_name) {
        return None;
    }
    let object = receiver.as_object()?;
    if !eg.class_is_a(&object.class_name, class_name) {
        return None;
    }
    let called_scope_class_id = object.class_id;
    drop(object);
    let Some((visibility, is_static, func_ptr, declaring)) =
        find_method_in_class_hierarchy(eg, class_name, method_name)
    else {
        let mut resolved =
            resolve_magic_callback(eg, class_name, method_name, "__call", Some(receiver))?;
        resolved.called_scope_class_id = called_scope_class_id;
        resolved.bound_this = Some(receiver.clone());
        return Some(resolved);
    };
    if is_static || !eg.check_visibility(lexical_class, declaring, visibility) {
        return None;
    }
    Some(ResolvedCallback {
        func_ptr,
        prepend_args: vec![receiver.clone()],
        use_vars: vec![],
        called_scope_class_id,
        bound_this: Some(receiver.clone()),
        closure_static_vars: None,
        is_magic_call: false,
    })
}

#[inline]
pub(super) fn resolve_callback_at_callsite(
    val: &Value,
    eg: &ExecutorGlobals,
    ed: *mut ExecuteData,
) -> Option<ResolvedCallback> {
    let needs_scope = val.value_type() == ValueType::Array
        || val.as_str().is_some_and(|name| name.contains("::"));
    if !needs_scope {
        if val.value_type() == ValueType::String
            && let Some(slot) = callback_cache_slot(ed)
            && let Some(func_ptr) = resolve_literal_string_callback_with_cache(val, eg, slot)
        {
            return Some(ResolvedCallback::plain_function(func_ptr));
        }
        return resolve_callback_with_cache(val, eg, None, callback_cache_slot(ed));
    }
    let lexical_class = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed);
    let visibility_scope = (val.value_type() == ValueType::Array)
        .then_some(lexical_class.as_deref())
        .flatten();
    let ordinary = resolve_callback_with_cache(val, eg, visibility_scope, callback_cache_slot(ed));
    if ordinary
        .as_ref()
        .is_some_and(|resolved| !resolved.is_magic_call)
    {
        return ordinary;
    }
    let receiver = crate::vm::execute::receiver_for_internal_call(ed);
    resolve_live_scoped_instance_callback(val, eg, lexical_class.as_deref(), receiver.as_ref())
        .or(ordinary)
}

/// Resolve a callback at an internal-function boundary and deliver the PHP
/// 8.5 deprecation attached to legacy relative spellings. Ordinary callbacks
/// retain the existing cache-backed resolver and do not allocate scope data.
pub(super) fn resolve_callback_at_callsite_checked(
    val: &Value,
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
) -> Result<Option<ResolvedCallback>, VmError> {
    let mut ordinary = resolve_callback_at_callsite(val, eg, ed);
    if ordinary.is_none() && ensure_callback_class_loaded(val, eg)? {
        ordinary = resolve_callback_at_callsite(val, eg, ed);
    }
    if ordinary.is_some() {
        return Ok(ordinary);
    }
    if eg.exception.is_some() {
        return Ok(None);
    }
    if !callback_uses_legacy_scope(val) {
        return Ok(ordinary);
    }
    let lexical_class = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed);
    let receiver = crate::vm::execute::receiver_for_internal_call(ed);
    let called_class = receiver
        .as_ref()
        .and_then(Value::as_object)
        .map(|object| object.class_name.to_string())
        .or_else(|| {
            crate::vm::execute::called_class_name_for_internal_call(eg, ed).map(str::to_owned)
        });
    match resolve_legacy_callback(
        val,
        eg,
        lexical_class.as_deref(),
        called_class.as_deref(),
        receiver.as_ref(),
    ) {
        LegacyCallbackResolution::NotLegacy => Ok(ordinary),
        LegacyCallbackResolution::Legacy {
            resolved,
            deprecation,
        } => {
            if let Some(deprecation) = deprecation {
                report_internal_deprecation(eg, ed, &deprecation)?;
            }
            if eg.exception.is_some() {
                Ok(None)
            } else {
                Ok(resolved)
            }
        }
    }
}

#[cold]
#[inline(never)]
fn call_magic_resolved_with_array(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    arguments: PhpArray,
) -> Result<Value, VmError> {
    let method = resolved
        .use_vars
        .first()
        .cloned()
        .unwrap_or_else(Value::null);
    let mut target = resolved.clone();
    target.is_magic_call = false;
    target.use_vars.clear();
    call_resolved_with_values(eg, &target, &[method, Value::array(arguments)])
}

/// Invoke a resolved callback with positional values from a PHP array.
/// Plain functions over packed arrays use the backing Value slice directly;
/// receivers, captures and hash arrays keep the general segmented iterator.
#[inline]
fn call_resolved_with_array(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    args: &PhpArray,
) -> Result<Value, VmError> {
    if resolved.is_magic_call {
        return call_magic_resolved_with_array(eg, resolved, args.clone());
    }
    if let Some(values) = args.packed_values() {
        return call_resolved_with_values(eg, resolved, values);
    }

    let num_args = resolved.prepend_args.len() + args.len() + resolved.use_vars.len();
    call_resolved_iter(
        eg,
        resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .chain(args.values())
            .chain(resolved.use_vars.iter()),
    )
}

#[inline]
pub(super) fn call_resolved_iter<'a, I>(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = &'a Value>,
{
    if resolved.is_magic_call {
        let values: Vec<_> = args.collect();
        let start = resolved.prepend_args.len();
        let end = values.len().saturating_sub(resolved.use_vars.len());
        let mut arguments = PhpArray::with_packed_capacity(end.saturating_sub(start));
        for value in &values[start..end] {
            arguments.push((*value).clone());
        }
        return call_magic_resolved_with_array(eg, resolved, arguments);
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }
    if !resolved.has_context()
        && resolved.use_vars.is_empty()
        && resolved.closure_static_vars.is_none()
    {
        call_function_iter(eg, resolved.func_ptr, num_args, args)
    } else {
        call_function_iter_with_context(
            eg,
            resolved.func_ptr,
            num_args,
            args,
            resolved.called_scope_class_id,
            resolved.bound_this.as_ref(),
            resolved.use_vars.len(),
            resolved.closure_static_vars.clone(),
        )
    }
}

#[inline]
pub(super) fn call_resolved_owned_iter<I>(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    if resolved.is_magic_call {
        let mut values: Vec<_> = args.collect();
        let start = resolved.prepend_args.len();
        let end = values.len().saturating_sub(resolved.use_vars.len());
        let mut arguments = PhpArray::with_packed_capacity(end.saturating_sub(start));
        for value in values.drain(start..end) {
            arguments.push(value);
        }
        return call_magic_resolved_with_array(eg, resolved, arguments);
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }
    if !resolved.has_context()
        && resolved.use_vars.is_empty()
        && resolved.closure_static_vars.is_none()
    {
        call_function_owned_iter(eg, resolved.func_ptr, num_args, args)
    } else {
        let capture_start = num_args.saturating_sub(resolved.use_vars.len());
        let args = args.enumerate().map(|(index, value)| {
            if index >= capture_start {
                resolved.use_vars[index - capture_start].clone_closure_capture()
            } else {
                value
            }
        });
        call_function_owned_iter_with_context(
            eg,
            resolved.func_ptr,
            num_args,
            args,
            resolved.called_scope_class_id,
            resolved.bound_this.clone(),
            resolved.use_vars.len(),
            resolved.closure_static_vars.clone(),
        )
    }
}

#[inline]
fn call_array_walk_resolved_owned_iter<I>(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    if resolved.is_magic_call
        || resolved.common().fn_type != FunctionType::User
        || reject_scope_introspection_callback(eg, resolved)
    {
        return call_resolved_owned_iter(eg, resolved, num_args, args);
    }
    let capture_start = num_args.saturating_sub(resolved.use_vars.len());
    let args = args.enumerate().map(|(index, value)| {
        if index >= capture_start {
            resolved.use_vars[index - capture_start].clone_closure_capture()
        } else {
            value
        }
    });
    crate::vm::execute::call_function_owned_iter_with_context_from(
        eg,
        ed,
        resolved.func_ptr,
        num_args,
        args,
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
    )
}

fn call_resolved_owned_iter_with_named<I>(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
    named_variadic: Vec<(String, Value)>,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    if resolved.is_magic_call {
        let mut values: Vec<_> = args.collect();
        let start = resolved.prepend_args.len();
        let end = values.len().saturating_sub(resolved.use_vars.len());
        let mut arguments =
            PhpArray::with_packed_capacity(end.saturating_sub(start) + named_variadic.len());
        for value in values.drain(start..end) {
            arguments.push(value);
        }
        for (name, value) in named_variadic {
            arguments.set_str(&name, value);
        }
        return call_magic_resolved_with_array(eg, resolved, arguments);
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }
    let capture_start = num_args.saturating_sub(resolved.use_vars.len());
    let args = args.enumerate().map(|(index, value)| {
        if index >= capture_start {
            resolved.use_vars[index - capture_start].clone_closure_capture()
        } else {
            value
        }
    });
    call_function_owned_iter_with_context_and_named(
        eg,
        resolved.func_ptr,
        num_args,
        args,
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
        named_variadic,
    )
}

fn call_resolved_owned_iter_with_named_from<I>(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
    named_variadic: Vec<(String, Value)>,
    logical_caller: *mut ExecuteData,
    file: &str,
    line: usize,
) -> Result<Value, VmError>
where
    I: Iterator<Item = Value>,
{
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }
    let capture_start = num_args.saturating_sub(resolved.use_vars.len());
    let args = args.enumerate().map(|(index, value)| {
        if index >= capture_start {
            resolved.use_vars[index - capture_start].clone_closure_capture()
        } else {
            value
        }
    });
    crate::vm::execute::call_function_owned_iter_with_context_and_named_from(
        eg,
        logical_caller,
        resolved.func_ptr,
        num_args,
        args,
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
        named_variadic,
        (file.to_string(), line),
        None,
        true,
    )
}

pub(crate) fn internal_variadic_forwards_named_arguments(function_name: &str) -> bool {
    matches!(
        function_name.to_ascii_lowercase().as_str(),
        "call_user_func"
            | "closure::__invoke"
            | "closure::call"
            | "reflectionfunction::invoke"
            | "reflectionclass::newinstance"
            | "reflectionmethod::invoke"
    )
}

#[inline]
pub(super) fn call_resolved_owned_iter_readback_arg0<I>(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    num_args: usize,
    args: I,
) -> Result<(Value, Value), VmError>
where
    I: Iterator<Item = Value>,
{
    if resolved.is_magic_call {
        let mut values: Vec<_> = args.collect();
        let start = resolved.prepend_args.len();
        let end = values.len().saturating_sub(resolved.use_vars.len());
        let readback = values.get(start).cloned().unwrap_or_else(Value::undef);
        let mut packed = PhpArray::with_packed_capacity(end.saturating_sub(start));
        for value in values.drain(start..end) {
            packed.push(value);
        }
        let result = call_magic_resolved_with_array(eg, resolved, packed)?;
        return Ok((result, readback));
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok((Value::null(), Value::null()));
    }
    let capture_start = num_args.saturating_sub(resolved.use_vars.len());
    let args = args.enumerate().map(|(index, value)| {
        if index >= capture_start {
            resolved.use_vars[index - capture_start].clone_closure_capture()
        } else {
            value
        }
    });
    call_function_owned_iter_readback_arg0_with_context(
        eg,
        resolved.func_ptr,
        num_args,
        args,
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
    )
}

/// Invoke a resolved callback from a contiguous argument slice. Plain user
/// functions can enter the guarded scalar callback ABI, while internal
/// handlers retain their direct slice ABI and every other callable shape uses
/// the canonical receiver/capture-aware frame path.
#[inline(always)]
fn try_execute_resolved_scalar_long_callback<'a, I>(
    resolved: &ResolvedCallback,
    public_num_args: usize,
    arguments: I,
) -> Option<i64>
where
    I: IntoIterator<Item = &'a Value>,
{
    // SAFETY: callback resolution only publishes a request-owned immutable
    // function pointer that remains live for the descriptor's lifetime. The
    // guarded ABI validates the function kind, arity and every argument before
    // reading the concrete user-function plan.
    unsafe { try_execute_scalar_long_callback(resolved.func_ptr, public_num_args, arguments) }
}

#[inline]
pub(crate) fn call_resolved_with_values(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    args: &[Value],
) -> Result<Value, VmError> {
    if resolved.is_magic_call {
        let mut arguments = PhpArray::with_packed_capacity(args.len());
        for value in args {
            arguments.push(value.clone());
        }
        return call_magic_resolved_with_array(eg, resolved, arguments);
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }
    if resolved.prepend_args.is_empty()
        && resolved.use_vars.is_empty()
        && !resolved.has_context()
        && resolved.closure_static_vars.is_none()
    {
        if let Some(result) =
            try_execute_resolved_scalar_long_callback(resolved, args.len(), args.iter())
        {
            return Ok(Value::long(result));
        }
        return call_function(eg, resolved.func_ptr, args);
    }

    let num_args = resolved.prepend_args.len() + args.len() + resolved.use_vars.len();
    call_resolved_iter(
        eg,
        resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .chain(args.iter())
            .chain(resolved.use_vars.iter()),
    )
}

/// Invoke a user callback beneath the live internal-function activation. This
/// preserves Zend's `[internal function]` boundary in stored Throwable traces;
/// scalar-proven callbacks retain their frame-free fast path.
#[inline]
fn call_resolved_with_values_from_internal(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    args: &[Value],
    publish_live_trace_caller: bool,
) -> Result<Value, VmError> {
    if resolved.is_magic_call
        || resolved.common().fn_type != FunctionType::User
        || reject_scope_introspection_callback(eg, resolved)
    {
        return call_resolved_with_values(eg, resolved, args);
    }
    if resolved.prepend_args.is_empty()
        && resolved.use_vars.is_empty()
        && !resolved.has_context()
        && resolved.closure_static_vars.is_none()
    {
        if let Some(result) =
            try_execute_resolved_scalar_long_callback(resolved, args.len(), args.iter())
        {
            return Ok(Value::long(result));
        }
    }

    let num_args = resolved.prepend_args.len() + args.len() + resolved.use_vars.len();
    crate::vm::execute::call_function_owned_iter_with_context_from_mode(
        eg,
        ed,
        resolved.func_ptr,
        num_args,
        resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(args.iter().cloned())
            .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
        publish_live_trace_caller,
    )
}

/// Invoke a resolved callback from a synthetic PHP call site while retaining
/// the live logical caller for traces and global-scope synchronization. This is
/// reserved for cold engine callbacks such as error and exception handlers;
/// ordinary PHP calls keep their opcode-owned fast paths.
fn call_resolved_with_values_from(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    args: &[Value],
    logical_caller: *mut ExecuteData,
    file: &str,
    line: usize,
    capture_preentry_error_origin: bool,
) -> Result<Value, VmError> {
    if resolved.is_magic_call {
        let method = resolved
            .use_vars
            .first()
            .cloned()
            .unwrap_or_else(Value::null);
        let mut arguments = PhpArray::with_packed_capacity(args.len());
        for argument in args {
            arguments.push(argument.clone());
        }
        let mut target = resolved.clone();
        target.is_magic_call = false;
        target.use_vars.clear();
        return call_resolved_with_values_from(
            eg,
            &target,
            &[method, Value::array(arguments)],
            logical_caller,
            file,
            line,
            capture_preentry_error_origin,
        );
    }
    if capture_preentry_error_origin
        && let Some(error) = scope_introspection_callback_error(resolved)
    {
        let num_args = resolved.prepend_args.len() + args.len() + resolved.use_vars.len();
        crate::vm::execute::call_function_owned_iter_with_context_and_named_from(
            eg,
            logical_caller,
            resolved.func_ptr,
            num_args,
            resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(args.iter().cloned())
                .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
            resolved.called_scope_class_id,
            resolved.bound_this.clone(),
            resolved.use_vars.len(),
            resolved.closure_static_vars.clone(),
            Vec::new(),
            (file.to_string(), line),
            Some(&error),
            false,
        )?;
        eg.exception = Some(error);
        return Ok(Value::null());
    }
    if reject_scope_introspection_callback(eg, resolved) {
        return Ok(Value::null());
    }

    let num_args = resolved.prepend_args.len() + args.len() + resolved.use_vars.len();
    crate::vm::execute::call_function_owned_iter_with_context_and_named_from(
        eg,
        logical_caller,
        resolved.func_ptr,
        num_args,
        resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(args.iter().cloned())
            .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
        resolved.use_vars.len(),
        resolved.closure_static_vars.clone(),
        Vec::new(),
        (file.to_string(), line),
        None,
        capture_preentry_error_origin,
    )
}

/// Invoke an already-resolved callback with PHP 8 call_user_func_array
/// positional/named argument semantics.
fn call_resolved_with_php_array_at(
    eg: &mut ExecutorGlobals,
    resolved: ResolvedCallback,
    args: &PhpArray,
    preserve_reference_aliases: bool,
    call_origin: Option<(*mut ExecuteData, &str, usize)>,
) -> Result<Value, VmError> {
    if resolved.is_magic_call {
        return call_magic_resolved_with_array(eg, &resolved, args.clone());
    }
    // SAFETY: callback resolution returns `func_ptr` from ExecutorGlobals'
    // registered immutable function table, which retains the descriptor for
    // this complete synchronous detached invocation.
    let (sig, function_type) = unsafe { (&(*resolved.func_ptr).sig, (*resolved.func_ptr).fn_type) };
    let prepare_argument = |index: usize, value: &Value| {
        let reference_index = if index < sig.public_arity() as usize {
            index
        } else if sig.is_variadic {
            sig.public_arity() as usize
        } else {
            index
        };
        if preserve_reference_aliases
            && sig.is_param_by_ref(reference_index as u32)
            && value.is_owned_reference()
        {
            value.clone_owned_reference_alias()
        } else if preserve_reference_aliases
            && sig.is_param_by_ref(reference_index as u32)
            && value.is_reference()
        {
            // SAFETY: the source argument array remains live until the
            // synchronous detached callback returns.
            Value::reference(unsafe { value.as_ref_ptr() })
        } else {
            value.clone()
        }
    };
    if !args.has_string_keys() {
        let normalized = args
            .values()
            .enumerate()
            .map(|(index, value)| prepare_argument(index, value))
            .collect::<Vec<_>>();
        let num_args = resolved.prepend_args.len() + normalized.len() + resolved.use_vars.len();
        let captures_preentry_error = normalized.len() < sig.required_num_args as usize
            || (function_type == FunctionType::Internal
                && !sig.is_variadic
                && normalized.len() > sig.public_arity() as usize);
        if captures_preentry_error && let Some((logical_caller, file, line)) = call_origin {
            return call_resolved_owned_iter_with_named_from(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(normalized)
                    .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
                Vec::new(),
                logical_caller,
                file,
                line,
            );
        }
        return call_resolved_owned_iter(
            eg,
            &resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(normalized)
                .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
        );
    }

    let param_names = &sig.param_names;
    let num_params = sig.public_arity() as usize;
    let required = sig.required_num_args as usize;

    let mut positional = vec![Value::undef(); num_params];
    let mut extra_positional: Vec<Value> = Vec::new();
    let mut named_variadic: Vec<(String, Value)> = Vec::new();
    let mut pos_cursor = 0usize;
    let mut seen_named = false;

    for (key, val) in args.iter() {
        match key {
            ArrayKey::String(name) => {
                seen_named = true;
                if let Some(idx) = param_names.iter().position(|p| p == name.as_str()) {
                    if idx < num_params {
                        if !positional[idx].is_undef() {
                            eg.exception = Some(crate::value::make_error_value(
                                "Error",
                                &format!("Named parameter ${} overwrites previous argument", name),
                            ));
                            return Ok(Value::null());
                        }
                        positional[idx] = prepare_argument(idx, val);
                    } else {
                        extra_positional.push(prepare_argument(idx, val));
                    }
                } else if sig.is_variadic {
                    named_variadic.push((
                        name.clone(),
                        prepare_argument(sig.public_arity() as usize, val),
                    ));
                } else {
                    eg.exception = Some(crate::value::make_error_value(
                        "Error",
                        &format!("Unknown named parameter ${}", name),
                    ));
                    return Ok(Value::null());
                }
            }
            ArrayKey::Int(_) => {
                if seen_named {
                    eg.exception = Some(crate::value::make_error_value(
                        "Error",
                        "Cannot use positional argument after named argument",
                    ));
                    return Ok(Value::null());
                }
                if pos_cursor < num_params {
                    positional[pos_cursor] = prepare_argument(pos_cursor, val);
                    pos_cursor += 1;
                } else {
                    extra_positional.push(prepare_argument(pos_cursor, val));
                    pos_cursor += 1;
                }
            }
        }
    }

    for i in 0..required {
        if positional[i].is_undef() {
            if sig.is_variadic && !named_variadic.is_empty() {
                continue;
            }
            let name = param_names.get(i).map(|s| s.as_str()).unwrap_or("?");
            let function = crate::vm::execute::displayed_function_name(eg, resolved.func_ptr);
            eg.exception = Some(crate::value::make_error_value(
                "ArgumentCountError",
                &format!("{function}(): Argument #{} (${name}) not passed", i + 1),
            ));
            return Ok(Value::null());
        }
    }

    let mut normalized: Vec<Value> = positional.into_iter().collect();
    while normalized.last().map_or(false, |v| v.is_undef()) {
        normalized.pop();
    }
    normalized.extend(extra_positional);

    let num_args = resolved.prepend_args.len() + normalized.len() + resolved.use_vars.len();
    let internal_rejects_named_variadic = function_type == FunctionType::Internal
        && sig.is_variadic
        && !named_variadic.is_empty()
        && !internal_variadic_forwards_named_arguments(
            &crate::vm::execute::displayed_function_name(eg, resolved.func_ptr),
        );
    let captures_preentry_error = internal_rejects_named_variadic
        || normalized.len() < required
        || (function_type == FunctionType::Internal
            && !sig.is_variadic
            && normalized.len() > num_params);
    if captures_preentry_error && let Some((logical_caller, file, line)) = call_origin {
        return call_resolved_owned_iter_with_named_from(
            eg,
            &resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(normalized)
                .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
            named_variadic,
            logical_caller,
            file,
            line,
        );
    }
    call_resolved_owned_iter_with_named(
        eg,
        &resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(normalized)
            .chain(resolved.use_vars.iter().cloned()),
        named_variadic,
    )
}

fn call_resolved_with_php_array(
    eg: &mut ExecutorGlobals,
    resolved: ResolvedCallback,
    args: &PhpArray,
    preserve_reference_aliases: bool,
) -> Result<Value, VmError> {
    call_resolved_with_php_array_at(eg, resolved, args, preserve_reference_aliases, None)
}

fn source_unpack_argument(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    function_name: &str,
    public_index: usize,
    value: &Value,
    source_file: &str,
    strict_types: bool,
) -> Result<Option<Value>, VmError> {
    let signature = resolved.signature();
    let reference_index = if public_index < signature.public_arity() as usize {
        public_index
    } else if signature.is_variadic {
        signature.public_arity() as usize
    } else {
        public_index
    };
    let mut prepared = if !signature.is_param_by_ref(reference_index as u32) {
        value.clone()
    } else if value.is_traversable_unpack_value() {
        eg.write_output(
            format!(
                "\nWarning: Cannot pass by-reference argument {} of {}() by unpacking a Traversable, passing by-value instead in {} on line 0\n",
                public_index + 1,
                function_name,
                source_file,
            )
            .as_bytes(),
        );
        value.dereferenced().clone()
    } else if value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else if value.is_reference() {
        // SAFETY: source argument lists remain alive through the synchronous
        // detached call, so a borrowed alias cannot outlive its target.
        Value::reference(unsafe { value.as_ref_ptr() })
    } else {
        let parameter = signature
            .diagnostic_parameter_name(public_index as u32)
            .map(|name| format!(" (${name})"))
            .unwrap_or_default();
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!(
                "{}(): Argument #{}{} could not be passed by reference",
                function_name,
                public_index + 1,
                parameter,
            ),
        ));
        return Ok(None);
    };

    if let Some(hint) = signature.param_type_hints.get(reference_index)
        && !matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed)
    {
        let original = prepared.dereferenced().clone();
        let callee_class = eg.declaring_class_of(resolved.func_ptr).map(str::to_string);
        match prepare_call_argument(&original, hint, eg, strict_types, callee_class.as_deref())? {
            CallArgumentPreparation::Exact => {}
            CallArgumentPreparation::Coerced(value) => {
                if prepared.is_reference() {
                    prepared.assign_dereferenced(value);
                } else {
                    prepared = value;
                }
            }
            CallArgumentPreparation::Invalid => {
                let parameter = if signature.is_variadic
                    && reference_index == signature.public_arity() as usize
                {
                    String::new()
                } else {
                    signature
                        .param_names
                        .get(reference_index)
                        .map(|name| format!(" (${name})"))
                        .unwrap_or_default()
                };
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "{}(): Argument #{}{parameter} must be of type {}, {} given, called in {} on line 0",
                        function_name,
                        public_index + 1,
                        hint.diagnostic_display_name(),
                        original.diagnostic_type_name(),
                        source_file,
                    ),
                ));
                return Ok(None);
            }
        }
    }
    Ok(Some(prepared))
}

fn call_resolved_with_source_unpack(
    eg: &mut ExecutorGlobals,
    resolved: ResolvedCallback,
    args: &PhpArray,
    source_file: &str,
    strict_types: bool,
) -> Result<Value, VmError> {
    let signature = resolved.signature();
    let function_name = displayed_function_name(eg, resolved.func_ptr);
    let fixed_count = signature.public_arity() as usize;
    let required = signature.required_num_args as usize;
    let is_variadic = signature.is_variadic;
    let param_names = signature.param_names.clone();
    let mut fixed = vec![Value::undef(); fixed_count];
    let mut positional_extras = Vec::new();
    let mut named_extras = Vec::new();
    let mut positional_cursor = 0usize;
    let mut highest_fixed = 0usize;

    for (key, value) in args.iter() {
        match key {
            ArrayKey::Int(_) => {
                let public_index = positional_cursor;
                let Some(value) = source_unpack_argument(
                    eg,
                    &resolved,
                    &function_name,
                    public_index,
                    value,
                    source_file,
                    strict_types,
                )?
                else {
                    return Ok(Value::null());
                };
                if public_index < fixed_count {
                    if !fixed[public_index].is_undef() {
                        eg.exception = Some(crate::value::make_error_value(
                            "Error",
                            &format!(
                                "Named parameter ${} overwrites previous argument",
                                param_names
                                    .get(public_index)
                                    .map(String::as_str)
                                    .unwrap_or("unknown")
                            ),
                        ));
                        return Ok(Value::null());
                    }
                    fixed[public_index] = value;
                    highest_fixed = highest_fixed.max(public_index + 1);
                } else {
                    positional_extras.push(value);
                }
                positional_cursor += 1;
            }
            ArrayKey::String(name) => {
                if let Some(index) = param_names.iter().position(|parameter| parameter == &name) {
                    if index < fixed_count {
                        if !fixed[index].is_undef() {
                            eg.exception = Some(crate::value::make_error_value(
                                "Error",
                                &format!("Named parameter ${name} overwrites previous argument"),
                            ));
                            return Ok(Value::null());
                        }
                        let Some(value) = source_unpack_argument(
                            eg,
                            &resolved,
                            &function_name,
                            index,
                            value,
                            source_file,
                            strict_types,
                        )?
                        else {
                            return Ok(Value::null());
                        };
                        fixed[index] = value;
                        highest_fixed = highest_fixed.max(index + 1);
                        positional_cursor = positional_cursor.max(index + 1);
                    } else if is_variadic {
                        if named_extras.iter().any(|(existing, _)| existing == &name) {
                            eg.exception = Some(crate::value::make_error_value(
                                "Error",
                                &format!("Named parameter ${name} overwrites previous argument"),
                            ));
                            return Ok(Value::null());
                        }
                        let public_index = fixed_count + positional_extras.len();
                        let Some(value) = source_unpack_argument(
                            eg,
                            &resolved,
                            &function_name,
                            public_index,
                            value,
                            source_file,
                            strict_types,
                        )?
                        else {
                            return Ok(Value::null());
                        };
                        named_extras.push((name, value));
                    }
                } else if is_variadic {
                    if named_extras.iter().any(|(existing, _)| existing == &name) {
                        eg.exception = Some(crate::value::make_error_value(
                            "Error",
                            &format!("Named parameter ${name} overwrites previous argument"),
                        ));
                        return Ok(Value::null());
                    }
                    let public_index = fixed_count + positional_extras.len();
                    let Some(value) = source_unpack_argument(
                        eg,
                        &resolved,
                        &function_name,
                        public_index,
                        value,
                        source_file,
                        strict_types,
                    )?
                    else {
                        return Ok(Value::null());
                    };
                    named_extras.push((name, value));
                } else {
                    eg.exception = Some(crate::value::make_error_value(
                        "Error",
                        &format!("Unknown named parameter ${name}"),
                    ));
                    return Ok(Value::null());
                }
            }
        }
    }

    for index in 0..required {
        if fixed.get(index).is_none_or(Value::is_undef) {
            let parameter = param_names
                .get(index)
                .map(String::as_str)
                .unwrap_or("unknown");
            eg.exception = Some(crate::value::make_error_value(
                "ArgumentCountError",
                &format!(
                    "{}(): Argument #{} (${}): not passed",
                    function_name,
                    index + 1,
                    parameter,
                ),
            ));
            return Ok(Value::null());
        }
    }

    let mut normalized = fixed;
    normalized.truncate(highest_fixed.max(required));
    normalized.extend(positional_extras);
    let num_args = resolved.prepend_args.len() + normalized.len() + resolved.use_vars.len();
    call_resolved_owned_iter_with_named(
        eg,
        &resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(normalized)
            .chain(resolved.use_vars.iter().map(Value::clone_closure_capture)),
        named_extras,
    )
}

/// VM entry for PHP source-level argument unpacking. Unlike
/// call_user_func_array(), arrays retain element aliases and Traversables use
/// their iterator keys plus the dedicated by-reference warning contract.
pub(crate) fn invoke_source_unpacked_call(
    callback: &Value,
    args_value: &Value,
    eg: &mut ExecutorGlobals,
    caller_class: Option<&str>,
    cache_slot: Option<*mut InlineCache>,
    source_file: &str,
    strict_types: bool,
) -> Result<Value, VmError> {
    let Some(args) = args_value.as_array() else {
        return Err(VmError::Fatal(
            "Compiler-owned unpack argument list is not an array".to_string(),
        ));
    };
    let Some(resolved) = resolve_callback_with_cache(callback, eg, caller_class, cache_slot) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Call to undefined function {}()", callback.echo_to_string()),
        ));
        return Ok(Value::null());
    };
    call_resolved_with_source_unpack(eg, resolved, args, source_file, strict_types)
}

pub(crate) fn invoke_resolved_source_unpacked_call(
    resolved: ResolvedCallback,
    args_value: &Value,
    eg: &mut ExecutorGlobals,
    source_file: &str,
    strict_types: bool,
) -> Result<Value, VmError> {
    let Some(args) = args_value.as_array() else {
        return Err(VmError::Fatal(
            "Compiler-owned unpack argument list is not an array".to_string(),
        ));
    };
    call_resolved_with_source_unpack(eg, resolved, args, source_file, strict_types)
}

/// VM entry for compiler-lowered call_user_func_array. It shares all callback
/// and named-argument semantics with the public stdlib function but skips its
/// variadic call frame entirely.
pub(crate) fn invoke_call_user_func_array(
    callback: &Value,
    args_value: &Value,
    eg: &mut ExecutorGlobals,
    caller_class: Option<&str>,
    cache_slot: Option<*mut InlineCache>,
) -> Result<Value, VmError> {
    let args = match args_value.as_array() {
        Some(args) => args,
        None => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func_array(): Argument #2 ($args) must be of type array, {} given",
                    args_value.dereferenced().type_name()
                ),
            ));
            return Ok(Value::null());
        }
    };

    let resolved = match resolve_callback_with_cache(callback, eg, caller_class, cache_slot) {
        Some(resolved) => resolved,
        None => {
            let desc = callback.echo_to_string();
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func_array(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found or not callable",
                    desc
                ),
            ));
            return Ok(Value::null());
        }
    };

    let caller = eg.current_execute_data.get();
    let strict = user_execute_data_is_strict(caller);
    with_detached_strict_call(caller, strict, || {
        call_resolved_with_php_array(eg, resolved, args, true)
    })
}

/// Invoke a callback already resolved at the consuming source boundary. This
/// keeps legacy-scope deprecation delivery outside the ordinary cache-backed
/// resolver while sharing call_user_func_array argument semantics.
pub(crate) fn invoke_resolved_call_user_func_array(
    resolved: ResolvedCallback,
    args_value: &Value,
    eg: &mut ExecutorGlobals,
) -> Result<Value, VmError> {
    let Some(args) = args_value.as_array() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "call_user_func_array(): Argument #2 ($args) must be of type array, {} given",
                args_value.dereferenced().type_name()
            ),
        ));
        return Ok(Value::null());
    };
    call_resolved_with_php_array(eg, resolved, args, true)
}

/// Invoke a compiler-lowered `call_user_func_array()` callback while retaining
/// its PHP source boundary for pre-entry errors and Throwable stack traces.
pub(crate) fn invoke_resolved_call_user_func_array_from(
    resolved: ResolvedCallback,
    args_value: &Value,
    eg: &mut ExecutorGlobals,
    logical_caller: *mut ExecuteData,
    source_file: &str,
    source_line: usize,
) -> Result<Value, VmError> {
    let Some(args) = args_value.as_array() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "call_user_func_array(): Argument #2 ($args) must be of type array, {} given",
                args_value.dereferenced().type_name()
            ),
        ));
        return Ok(Value::null());
    };
    let strict = user_execute_data_is_strict(logical_caller);
    with_detached_strict_call(logical_caller, strict, || {
        call_resolved_with_php_array_at(
            eg,
            resolved,
            args,
            true,
            Some((logical_caller, source_file, source_line)),
        )
    })
}

/// call_user_func($callback, ...$args)
/// CV 0 = callback, CV 1 = variadic array of extra args
fn fn_call_user_func(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    // Variadic args packed at CV(1)
    let variadic_val = arg!(ed, 1);

    let resolved = match resolve_callback_at_callsite_checked(callback, eg, ed)? {
        Some(r) => r,
        None => {
            if eg.exception.is_some() {
                return Ok(());
            }
            let reason = ordinary_callback_invalid_reason(callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func(): Argument #1 ($callback) must be a valid callback, {reason}"
                ),
            ));
            return Ok(());
        }
    };

    // A discarded wrapper result semantically discards the resolved callback
    // result. Detached callbacks always own a temporary return slot, so carry
    // that source-level fact across the engine boundary explicitly.
    let discarded = rv.is_null() || eg.detached_return_discarded();
    let previous_discarded = eg.replace_detached_return_discarded(discarded);

    // Stream prepend args (e.g. $this), variadic values and closure captures
    // directly into the callback frame. No intermediate argument vectors.
    let result = if let Some(arr) = variadic_val.as_array() {
        if callback_has_hard_reference_parameters(&resolved) {
            let callback_name = callable_display_name(callback, eg);
            if !report_callback_reference_warnings(eg, ed, &resolved, arr, true, &callback_name)? {
                eg.replace_detached_return_discarded(previous_discarded);
                return Ok(());
            }
        }
        if arr.has_string_keys() {
            call_resolved_with_php_array(eg, resolved.clone(), arr, false)
        } else {
            call_resolved_with_array(eg, &resolved, arr)
        }
    } else if variadic_val.value_type() != ValueType::Undef {
        let num_args = resolved.prepend_args.len() + 1 + resolved.use_vars.len();
        call_resolved_iter(
            eg,
            &resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .chain(std::iter::once(variadic_val))
                .chain(resolved.use_vars.iter()),
        )
    } else {
        let num_args = resolved.prepend_args.len() + resolved.use_vars.len();
        call_resolved_iter(
            eg,
            &resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .chain(std::iter::empty())
                .chain(resolved.use_vars.iter()),
        )
    };
    eg.replace_detached_return_discarded(previous_discarded);
    let result = result?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, result);
}

pub(crate) fn callable_display_name(value: &Value, eg: &ExecutorGlobals) -> String {
    match value.value_type() {
        ValueType::String => value.as_str().unwrap_or_default().to_string(),
        ValueType::Closure => value.as_closure().map_or_else(String::new, |closure| {
            crate::vm::execute::displayed_function_name(eg, closure.func)
        }),
        ValueType::Array => {
            let Some(array) = value.as_array() else {
                return "Array".to_string();
            };
            if array.len() != 2 {
                return "Array".to_string();
            }
            let Some(method) = array.get_value_at(1).and_then(Value::as_str) else {
                return "Array".to_string();
            };
            let Some(owner) = array.get_value_at(0) else {
                return "Array".to_string();
            };
            let class = if owner.value_type() == ValueType::Closure {
                "Closure".to_string()
            } else if let Some(object) = owner.as_object() {
                let internal = object.class_name.to_string();
                drop(object);
                eg.find_class(&internal)
                    .and_then(|definition| definition.anonymous_public_name())
                    .unwrap_or(internal)
            } else if let Some(class) = owner.as_str() {
                class.trim_start_matches('\\').to_string()
            } else {
                return "Array".to_string();
            };
            format!("{class}::{method}")
        }
        ValueType::Object => value.as_object().map_or_else(String::new, |object| {
            let internal = object.class_name.to_string();
            drop(object);
            let class = eg
                .find_class(&internal)
                .and_then(|definition| definition.anonymous_public_name())
                .unwrap_or(internal);
            format!("{class}::__invoke")
        }),
        _ => value.echo_to_string(),
    }
}

pub(crate) fn callback_reference_warning_messages(
    resolved: &ResolvedCallback,
    arguments: &PhpArray,
    force_by_value: bool,
    display_name: &str,
) -> Vec<String> {
    let signature = resolved.signature();
    let public_arity = signature.public_arity() as usize;
    let mut positional_index = 0usize;
    let mut warnings = Vec::new();
    for (key, value) in arguments.iter() {
        let parameter_index = match key {
            ArrayKey::Int(_) => {
                let index = positional_index;
                positional_index += 1;
                index
            }
            ArrayKey::String(name) => signature
                .param_names
                .iter()
                .position(|parameter| parameter == name.as_str())
                .unwrap_or(public_arity),
        };
        let reference_index = if parameter_index < public_arity {
            parameter_index
        } else if signature.is_variadic {
            public_arity
        } else {
            parameter_index
        };
        if !signature.is_param_by_ref(reference_index as u32)
            || signature.is_param_prefer_ref(reference_index as u32)
            || (!force_by_value && (value.is_reference() || value.is_owned_reference()))
        {
            continue;
        }
        let parameter = signature
            .diagnostic_parameter_name(reference_index as u32)
            .map(|name| format!(" (${name})"))
            .unwrap_or_default();
        warnings.push(format!(
            "{display_name}(): Argument #{}{parameter} must be passed by reference, value given",
            parameter_index + 1,
        ));
    }
    warnings
}

#[inline]
pub(crate) fn callback_has_hard_reference_parameters(resolved: &ResolvedCallback) -> bool {
    let signature = resolved.signature();
    signature.ref_args != 0 && signature.ref_args != signature.prefer_ref_args
}

fn report_callback_reference_warnings(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    resolved: &ResolvedCallback,
    arguments: &PhpArray,
    force_by_value: bool,
    display_name: &str,
) -> Result<bool, VmError> {
    if !callback_has_hard_reference_parameters(resolved) {
        return Ok(true);
    }
    for message in
        callback_reference_warning_messages(resolved, arguments, force_by_value, display_name)
    {
        report_internal_diagnostic(eg, ed, 2, "Warning", &message)?;
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn callable_has_valid_syntax(value: &Value, eg: &ExecutorGlobals) -> bool {
    match value.value_type() {
        ValueType::String | ValueType::Closure => true,
        ValueType::Array => value.as_array().is_some_and(|array| {
            array.len() == 2
                && array.get_value_at(0).is_some_and(|owner| {
                    owner.value_type() == ValueType::Closure
                        || owner.as_object().is_some()
                        || owner.as_str().is_some()
                })
                && array.get_value_at(1).and_then(Value::as_str).is_some()
        }),
        ValueType::Object => value.as_object().is_some_and(|object| {
            method_declared_in_class_hierarchy(eg, &object.class_name, "__invoke")
        }),
        _ => false,
    }
}

/// is_callable($value, $syntax_only = false, &$callable_name = null)
fn fn_is_callable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let val = arg!(ed, 0);
    let name = callable_display_name(val, eg);
    // Fixed internal frames always own every declared CV. If the optional
    // output was omitted this writes only the handler-local Undef slot; if it
    // was supplied, arg_mut! follows the caller's live reference.
    arg_mut!(ed, 2, Value::string(name));
    let syntax_only = arg_opt!(ed, 1).is_some_and(Value::is_truthy);
    let callable = if syntax_only {
        callable_has_valid_syntax(val, eg)
    } else {
        resolve_callback_at_callsite_checked(val, eg, ed)?.is_some()
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(callable));
}

fn forwarded_static_callback(
    callback: &Value,
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    wrapper: &str,
) -> Result<Option<ResolvedCallback>, VmError> {
    let Some(lexical_class) = crate::vm::execute::lexical_class_name_for_internal_call(eg, ed)
    else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Cannot call {wrapper}() when no class scope is active"),
        ));
        return Ok(None);
    };
    let called_class =
        crate::vm::execute::called_class_name_for_internal_call(eg, ed).map(str::to_owned);
    let Some(mut resolved) = resolve_callback_at_callsite_checked(callback, eg, ed)? else {
        if eg.exception.is_none() {
            let reason = ordinary_callback_invalid_reason(callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!("{wrapper}(): Argument #1 ($callback) must be a valid callback, {reason}"),
            ));
        }
        return Ok(None);
    };
    let target_scope = eg
        .declaring_class_of(resolved.func_ptr)
        .or_else(|| {
            callback
                .as_array()
                .and_then(|array| array.get_value_at(0))
                .and_then(Value::as_str)
                .map(|class| class.trim_start_matches('\\'))
        })
        .unwrap_or(lexical_class.as_str());
    if let Some(called_class) = called_class
        && eg.class_is_a(&called_class, target_scope)
    {
        resolved.called_scope_class_id = eg.class_id_of(&called_class);
    }
    Ok(Some(resolved))
}

/// forward_static_call($callback, ...$args)
fn fn_forward_static_call(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let Some(resolved) = forwarded_static_callback(callback, eg, ed, "forward_static_call")? else {
        return Ok(());
    };
    let arguments = arg!(ed, 1);
    let result = if let Some(arguments) = arguments.as_array() {
        call_resolved_with_array(eg, &resolved, arguments)?
    } else if arguments.value_type() == ValueType::Undef {
        call_resolved_with_values(eg, &resolved, &[])?
    } else {
        call_resolved_with_values(eg, &resolved, std::slice::from_ref(arguments))?
    };
    if eg.exception.is_none() {
        ret!(rv, result);
    }
    Ok(())
}

/// forward_static_call_array($callback, $args)
fn fn_forward_static_call_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let Some(resolved) = forwarded_static_callback(callback, eg, ed, "forward_static_call_array")?
    else {
        return Ok(());
    };
    let arguments = arg!(ed, 1);
    let Some(arguments) = arguments.as_array() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "forward_static_call_array(): Argument #2 ($args) must be of type array, {} given",
                arguments.dereferenced().type_name()
            ),
        ));
        return Ok(());
    };
    let result = call_resolved_with_php_array(eg, resolved, arguments, true)?;
    if eg.exception.is_none() {
        ret!(rv, result);
    }
    Ok(())
}

// ============================================================================
// Time functions
// ============================================================================

/// microtime(bool $as_float = false): string|float
/// Returns current Unix timestamp with microsecond precision.
fn fn_microtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let as_float = arg_opt!(ed, 0).map(|v| v.is_truthy()).unwrap_or(false);
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    if as_float {
        let secs = dur.as_secs() as f64 + dur.subsec_nanos() as f64 / 1_000_000_000.0;
        ret!(rv, Value::double(secs));
    } else {
        let usec = dur.subsec_micros();
        let sec = dur.as_secs();
        ret!(rv, Value::string(format!("0.{:06} {}", usec, sec)));
    }
}

/// hrtime(bool $as_nanoseconds = false): array|int
/// Returns high-resolution monotonic time.
/// hrtime(true) → int nanoseconds
/// hrtime(false) → [seconds, nanoseconds]
fn fn_hrtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::time::Instant;
    // Use a lazy-initialized epoch for monotonic timing
    use std::sync::OnceLock;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    let elapsed = epoch.elapsed();

    let as_ns = arg_opt!(ed, 0).map(|v| v.is_truthy()).unwrap_or(false);
    if as_ns {
        ret!(rv, Value::long(elapsed.as_nanos() as i64));
    } else {
        let mut arr = crate::value::PhpArray::new();
        arr.push(Value::long(elapsed.as_secs() as i64));
        arr.push(Value::long(elapsed.subsec_nanos() as i64));
        ret!(rv, Value::array(arr));
    }
}

/// time(): int — current Unix timestamp
fn fn_time(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    ret!(rv, Value::long(secs as i64));
}

// ============================================================================
// exit / die
// ============================================================================

/// exit($status = 0) / die($status = 0)
/// PHP 8.4+ exposes this language construct through an ordinary `string|int`
/// internal-call contract while retaining exit's process-level result.
fn fn_exit(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(status) = arg_opt!(ed, 0) else {
        return Err(VmError::Exit(0));
    };
    let status = status.dereferenced();
    let function = crate::vm::execute::displayed_frame_function_name(eg, ed);
    let strict = internal_call_is_strict(ed);

    let reject = |eg: &mut ExecutorGlobals| {
        let actual = match status.value_type() {
            ValueType::True => "true".to_string(),
            ValueType::False => "false".to_string(),
            _ => status.diagnostic_type_name().into_owned(),
        };
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{function}(): Argument #1 ($status) must be of type string|int, {actual} given"
            ),
        ));
        Ok(())
    };

    match status.value_type() {
        ValueType::Long => Err(VmError::Exit(status.as_long().unwrap_or(0) as i32)),
        ValueType::String => {
            print!("{}", status.as_str().unwrap_or(""));
            Err(VmError::Exit(0))
        }
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #1 ($status) of type string|int is deprecated"
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            Err(VmError::Exit(0))
        }
        ValueType::True | ValueType::False if !strict => {
            Err(VmError::Exit(i32::from(status.is_truthy())))
        }
        ValueType::Double if !strict => {
            let number = status.as_double().unwrap();
            let upper_exclusive = -(i64::MIN as f64);
            if number.is_finite() && number >= i64::MIN as f64 && number < upper_exclusive {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            status.echo_to_string_with_precision(-1)
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                }
                return Err(VmError::Exit(integer as i32));
            }
            if number.is_nan() {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to string",
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
            print!("{}", status.echo_to_string_with_precision(eg.precision));
            Err(VmError::Exit(0))
        }
        ValueType::Object if !strict => {
            let converted = crate::vm::execute::call_object_string_conversion(eg, status)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            let Some(converted) = converted else {
                return reject(eg);
            };
            let Some(rendered) = converted.as_str() else {
                let class_name = status.diagnostic_type_name();
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!("{class_name}::__toString(): Return value must be of type string"),
                ));
                return Ok(());
            };
            print!("{rendered}");
            Err(VmError::Exit(0))
        }
        _ => reject(eg),
    }
}

// ============================================================================
// Missing common array functions
// ============================================================================

/// Resolve callback consumers once, before entering their iteration loop.
fn resolve_callback_or_fatal(
    eg: &ExecutorGlobals,
    cb_val: &Value,
    ed: *mut ExecuteData,
) -> Result<ResolvedCallback, VmError> {
    resolve_callback_at_callsite(cb_val, eg, ed).ok_or_else(|| {
        let desc = cb_val.echo_to_string();
        VmError::Fatal(format!(
            "Callback must be a valid callable, function \"{}\" not found",
            desc
        ))
    })
}

/// array_reduce($array, $callback, $initial = null): mixed
fn fn_array_reduce(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_val = arg!(ed, 0);
    let callback = arg!(ed, 1).clone();
    let initial = arg_opt!(ed, 2).cloned().unwrap_or(Value::null());

    if let Some(arr) = arr_val.as_array() {
        let items: Vec<Value> = arr.values().cloned().collect();
        let resolved = resolve_callback_or_fatal(eg, &callback, ed)?;
        let mut carry = initial;
        for item in items {
            if resolved.prepend_args.is_empty()
                && resolved.use_vars.is_empty()
                && !resolved.has_context()
                && let Some(result) = unsafe {
                    try_execute_scalar_long_callback(
                        resolved.func_ptr,
                        2,
                        [&carry, &item].into_iter(),
                    )
                }
            {
                carry = Value::long(result);
                continue;
            }
            // Carry and item are already owned: move both straight into the
            // callback frame while cloning only persistent receiver/captures.
            let num_args = resolved.prepend_args.len() + 2 + resolved.use_vars.len();
            carry = call_resolved_owned_iter(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(std::iter::once(carry))
                    .chain(std::iter::once(item))
                    .chain(resolved.use_vars.iter().cloned()),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        ret!(rv, carry);
    }
    ret!(rv, initial);
}

/// usort(&$array, $callback): bool
#[inline(never)]
unsafe fn try_usort_scalar_long(
    items: &mut [Value],
    resolved: &ResolvedCallback,
) -> Result<bool, ()> {
    if !resolved.prepend_args.is_empty()
        || !resolved.use_vars.is_empty()
        || resolved.has_context()
        || resolved.closure_static_vars.is_some()
    {
        return Ok(false);
    }
    let Some(callback) = prepare_scalar_long_callback(resolved.func_ptr, 2) else {
        return Ok(false);
    };
    if items
        .iter()
        .any(|value| value.value_type() != ValueType::Long || value.is_reference())
    {
        return Ok(false);
    }

    if let Some(order) = callback.exact_sort_order() {
        let mut completed_calls = 0u64;
        items.sort_by(|left, right| {
            completed_calls += 1;
            let ordering = left.raw_long().cmp(&right.raw_long());
            match order {
                ScalarLongSortOrder::Ascending => ordering,
                ScalarLongSortOrder::Descending => ordering.reverse(),
            }
        });
        callback.record_calls(completed_calls);
        return Ok(true);
    }

    let mut completed_calls = 0u64;
    for i in 1..items.len() {
        let mut j = i;
        while j > 0 {
            let comparison = callback
                .evaluate_longs(&[items[j - 1].raw_long(), items[j].raw_long()])
                .ok_or(())?;
            completed_calls += 1;
            if comparison <= 0 {
                break;
            }
            items.swap(j - 1, j);
            j -= 1;
        }
    }
    callback.record_calls(completed_calls);
    Ok(true)
}

struct UserSortCallbackState {
    reference_warning_name: Option<String>,
    warned_bool_return: bool,
}

impl UserSortCallbackState {
    fn new(callback: &Value, resolved: &ResolvedCallback, eg: &ExecutorGlobals) -> Self {
        Self {
            reference_warning_name: callback_has_hard_reference_parameters(resolved)
                .then(|| callable_display_name(callback, eg)),
            warned_bool_return: false,
        }
    }
}

fn report_user_sort_reference_warnings(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    display_name: &str,
) -> Result<bool, VmError> {
    let signature = resolved.signature();
    for index in 0..2u32 {
        if !signature.is_param_by_ref(index) || signature.is_param_prefer_ref(index) {
            continue;
        }
        let parameter = signature
            .param_names
            .get(index as usize)
            .map(String::as_str)
            .unwrap_or("unknown");
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "{display_name}(): Argument #{} (${parameter}) must be passed by reference, value given",
                index + 1
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn call_user_sort_callback_once(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    state: &UserSortCallbackState,
    left: &Value,
    right: &Value,
) -> Result<Option<Value>, VmError> {
    if let Some(display_name) = state.reference_warning_name.as_deref()
        && !report_user_sort_reference_warnings(ed, eg, resolved, display_name)?
    {
        return Ok(None);
    }
    let num_args = resolved.prepend_args.len() + 2 + resolved.use_vars.len();
    let result = call_resolved_iter(
        eg,
        resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .chain(std::iter::once(left))
            .chain(std::iter::once(right))
            .chain(resolved.use_vars.iter()),
    )?;
    if eg.exception.is_some() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

fn user_sort_comparison(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    state: &mut UserSortCallbackState,
    function_name: &str,
    left: &Value,
    right: &Value,
) -> Result<Option<std::cmp::Ordering>, VmError> {
    let Some(result) = call_user_sort_callback_once(ed, eg, resolved, state, left, right)? else {
        return Ok(None);
    };
    match result.value_type() {
        ValueType::True => {
            if !state.warned_bool_return {
                state.warned_bool_return = true;
                report_internal_deprecation(
                    eg,
                    ed,
                    &format!(
                        "{function_name}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero"
                    ),
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            Ok(Some(std::cmp::Ordering::Greater))
        }
        ValueType::False => {
            if !state.warned_bool_return {
                state.warned_bool_return = true;
                report_internal_deprecation(
                    eg,
                    ed,
                    &format!(
                        "{function_name}(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero"
                    ),
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            let Some(reverse) = call_user_sort_callback_once(ed, eg, resolved, state, right, left)?
            else {
                return Ok(None);
            };
            Ok(Some(reverse.to_long_val().cmp(&0).reverse()))
        }
        _ => Ok(Some(result.to_long_val().cmp(&0))),
    }
}

fn user_sort_result_value(value: Value) -> Value {
    if value.is_owned_reference() {
        array_projection_value(&value)
    } else {
        value
    }
}

fn fn_usort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // Save raw pointer to array BEFORE any call_function.
    // call_function may push/pop VM stack frames but the by-ref pointer target
    // lives in the CALLER's frame (which is below us on the stack and stays valid).
    let arr_ptr: *mut Value = arg_mut!(ed, 0);
    let callback = arg!(ed, 1).clone();

    let items = {
        let arr = unsafe { &*arr_ptr };
        match arr.as_array() {
            Some(a) => a
                .values()
                .map(array_sort_snapshot_value)
                .collect::<Vec<Value>>(),
            None => {
                ret!(rv, Value::bool(false));
            }
        }
    };
    let resolved = match resolve_callback_at_callsite(&callback, eg, ed) {
        Some(r) => r,
        None => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "usort(): Argument #2 ($callback) must be a valid callback",
            ));
            return Ok(());
        }
    };
    let mut items = items;

    match unsafe { try_usort_scalar_long(&mut items, &resolved) } {
        Ok(true) => {
            let mut new_arr = PhpArray::new();
            for value in items {
                new_arr.push(user_sort_result_value(value));
            }
            unsafe {
                *arr_ptr = Value::array(new_arr);
            }
            ret!(rv, Value::bool(true));
        }
        Ok(false) => {}
        Err(()) => {
            // The scalar callback is pure and its counters are unpublished, so
            // an arithmetic side exit can restart from the untouched array.
            items = unsafe { &*arr_ptr }
                .as_array()
                .expect("usort array changed before canonical fallback")
                .values()
                .map(array_sort_snapshot_value)
                .collect();
        }
    }

    let mut state = UserSortCallbackState::new(&callback, &resolved, eg);
    if items.len() < 6 {
        let completed = stable_sort_small_optional_checked(&mut items, |left, right| {
            user_sort_comparison(ed, eg, &resolved, &mut state, "usort", left, right)
        })?;
        if !completed {
            return Ok(());
        }
    } else {
        // Larger callback sorts retain their established insertion schedule.
        let len = items.len();
        for i in 1..len {
            let mut j = i;
            while j > 0 {
                let Some(ordering) = user_sort_comparison(
                    ed,
                    eg,
                    &resolved,
                    &mut state,
                    "usort",
                    &items[j - 1],
                    &items[j],
                )?
                else {
                    return Ok(());
                };
                if ordering != std::cmp::Ordering::Greater {
                    break;
                }
                items.swap(j - 1, j);
                j -= 1;
            }
        }
    }
    let mut new_arr = PhpArray::new();
    for v in items {
        new_arr.push(user_sort_result_value(v));
    }
    // Write back using saved raw pointer (stable across call_function calls).
    unsafe {
        *arr_ptr = Value::array(new_arr);
    }
    ret!(rv, Value::bool(true));
}

fn array_key_value(key: &ArrayKey, external_byte_keys: bool) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(*value),
        ArrayKey::String(value) if external_byte_keys => {
            Value::binary_string_from_storage(value.clone())
        }
        ArrayKey::String(value) => Value::string(value.clone()),
    }
}

#[inline(always)]
fn array_key_into_value(key: ArrayKey) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(value),
        ArrayKey::String(value) => Value::string(value),
    }
}

fn fn_user_key_preserving_sort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    compare_keys: bool,
    function_name: &str,
) -> Result<(), VmError> {
    let callback = arg!(ed, 1).clone();
    let external_byte_keys = arg!(ed, 0)
        .as_array()
        .is_some_and(PhpArray::has_external_byte_keys);
    let utf8_text_keys = arg!(ed, 0)
        .as_array()
        .is_some_and(PhpArray::has_utf8_text_keys);
    let mut pairs = match arg!(ed, 0).as_array() {
        Some(array) => array
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect::<Vec<_>>(),
        None => ret!(rv, Value::bool(false)),
    };
    let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{function_name}(): Argument #2 ($callback) must be a valid callback"),
        ));
        return Ok(());
    };
    let mut state = UserSortCallbackState::new(&callback, &resolved, eg);

    if pairs.len() < 6 {
        let completed = stable_sort_small_optional_checked(&mut pairs, |left, right| {
            let keys = compare_keys.then(|| {
                [
                    array_key_value(&left.0, external_byte_keys),
                    array_key_value(&right.0, external_byte_keys),
                ]
            });
            let (left, right) = keys
                .as_ref()
                .map_or_else(|| (&left.1, &right.1), |keys| (&keys[0], &keys[1]));
            user_sort_comparison(ed, eg, &resolved, &mut state, function_name, left, right)
        })?;
        if !completed {
            return Ok(());
        }
    } else {
        for index in 1..pairs.len() {
            let mut current = index;
            while current > 0 {
                let keys = compare_keys.then(|| {
                    [
                        array_key_value(&pairs[current - 1].0, external_byte_keys),
                        array_key_value(&pairs[current].0, external_byte_keys),
                    ]
                });
                let (left, right) = keys.as_ref().map_or_else(
                    || (&pairs[current - 1].1, &pairs[current].1),
                    |keys| (&keys[0], &keys[1]),
                );
                let Some(ordering) = user_sort_comparison(
                    ed,
                    eg,
                    &resolved,
                    &mut state,
                    function_name,
                    left,
                    right,
                )?
                else {
                    return Ok(());
                };
                if ordering != std::cmp::Ordering::Greater {
                    break;
                }
                pairs.swap(current - 1, current);
                current -= 1;
            }
        }
    }

    let mut sorted = PhpArray::new();
    for (key, value) in pairs {
        sorted.set(key, user_sort_result_value(value));
    }
    if external_byte_keys {
        sorted.mark_external_byte_keys();
    }
    if utf8_text_keys {
        sorted.mark_utf8_text_keys();
    }
    arg_mut!(ed, 0, Value::array(sorted));
    ret!(rv, Value::bool(true));
}

fn fn_uasort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_user_key_preserving_sort(ed, rv, eg, false, "uasort")
}

fn fn_uksort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_user_key_preserving_sort(ed, rv, eg, true, "uksort")
}

fn report_array_walk_userdata_reference_warning(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    callback: &Value,
    userdata_supplied: bool,
) -> Result<bool, VmError> {
    if !userdata_supplied
        || !resolved.signature().is_param_by_ref(2)
        || resolved.signature().is_param_prefer_ref(2)
    {
        return Ok(true);
    }
    let display_name = callable_display_name(callback, eg);
    let parameter = resolved
        .signature()
        .param_names
        .get(2)
        .map(String::as_str)
        .unwrap_or("unknown");
    report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "{display_name}(): Argument #3 (${parameter}) must be passed by reference, value given"
        ),
    )?;
    Ok(eg.exception.is_none())
}

#[inline]
fn array_walk_declared_property_key(definition: &PropertyDefinition) -> Value {
    let key = match definition.visibility {
        Visibility::Public => definition.name.clone(),
        Visibility::Protected => format!("\0*\0{}", definition.name),
        Visibility::Private => format!("\0{}\0{}", definition.declaring_class, definition.name),
    };
    Value::string(key)
}

#[inline]
fn replace_array_walk_property_value(property: &mut Value, replacement: Value) {
    property.assign_dereferenced(replacement);
}

/// Locate the reference cell used as the live array-walk cursor. The common
/// no-mutation path validates the previous ordered position in O(1); only a
/// callback that structurally edits the array pays for the linear recovery.
#[inline]
fn array_walk_reference_position(
    array: &PhpArray,
    reference_identity: usize,
    position_hint: usize,
) -> Option<usize> {
    if array
        .get_value_at(position_hint)
        .and_then(Value::reference_identity)
        == Some(reference_identity)
    {
        return Some(position_hint);
    }
    array
        .values()
        .position(|value| value.reference_identity() == Some(reference_identity))
}

#[inline]
fn array_walk_key_position(array: &PhpArray, key: &ArrayKey) -> Option<usize> {
    array.iter().position(|(candidate, _)| candidate == *key)
}

/// Promote the current member to a stable reference cell for the duration of
/// one callback. This mirrors Zend's live HashTable cursor: deleting the
/// current member can be distinguished from replacing a value under the same
/// key, while an array replacement is detected through its COW identity.
fn array_walk_live_entry(
    owner: &mut Value,
    position: usize,
    reusable_reference: &mut Option<Value>,
) -> Option<(usize, ArrayKey, usize, Value, bool)> {
    let (_, key) = owner.as_array()?.get_at(position)?;
    let was_reference = owner
        .as_array()?
        .get_value_at(position)
        .is_some_and(Value::is_reference);
    let mut binding = if !was_reference {
        if let Some(mut binding) = reusable_reference.take() {
            let alias = binding.clone_owned_reference_alias();
            let value = owner.as_array_mut()?.replace_value_at(position, alias)?;
            drop(binding.replace_dereferenced(value));
            binding
        } else {
            owner
                .as_array_mut()?
                .argument_unpack_reference_at(position)?
        }
    } else {
        owner
            .as_array_mut()?
            .argument_unpack_reference_at(position)?
    };
    let reference_identity = binding.reference_identity()?;
    if binding.is_owned_reference() {
        binding.mark_internal_reference_alias();
    }
    Some((
        owner.array_identity()?,
        key,
        reference_identity,
        binding,
        was_reference,
    ))
}

/// Advance a live array-walk cursor after arbitrary callback-side structural
/// mutation. `None` means the walked variable ceased to be an array.
fn advance_array_walk_cursor(
    owner: &Value,
    walked_identity: usize,
    position_hint: usize,
    key: ArrayKey,
    reference_identity: usize,
    anchors: &mut Vec<ArrayKey>,
) -> Option<(usize, usize, Option<usize>)> {
    let array = owner.as_array()?;
    let current_identity = owner.array_identity()?;
    if current_identity != walked_identity {
        anchors.clear();
        return Some((current_identity, 0, None));
    }

    if let Some(position) = array_walk_reference_position(array, reference_identity, position_hint)
    {
        anchors.push(key);
        return Some((current_identity, position + 1, Some(position)));
    }

    while let Some(anchor) = anchors.last() {
        if let Some(position) = array_walk_key_position(array, anchor) {
            return Some((current_identity, position + 1, None));
        }
        anchors.pop();
    }
    Some((current_identity, 0, None))
}

/// By-value callbacks must not leave implementation-only reference wrappers
/// behind (PHP bug #42850). Preserve a wrapper only when it predated the walk
/// or another PHP-visible alias was retained during the callback.
fn release_array_walk_cursor_reference(
    owner: &mut Value,
    walked_identity: usize,
    position: Option<usize>,
    mut binding: Value,
    was_reference: bool,
    reusable_reference: &mut Option<Value>,
) -> usize {
    if was_reference || binding.owned_reference_is_aliased() {
        return owner.array_identity().unwrap_or(walked_identity);
    }
    // Detaching here is an internal cleanup, not a user-visible replacement;
    // return the possibly new COW identity so iteration does not restart.
    let replacement = binding.replace_dereferenced(Value::null());
    if let Some(position) = position
        && owner.array_identity() == Some(walked_identity)
        && let Some(array) = owner.as_array_mut()
        && array
            .get_value_at(position)
            .and_then(Value::reference_identity)
            == binding.reference_identity()
    {
        let _ = array.set_value_at(position, replacement);
    }
    let identity = owner.array_identity().unwrap_or(walked_identity);
    *reusable_reference = Some(binding);
    identity
}

/// Detach an implementation-only cursor wrapper after a callback exception
/// without clearing its target. The retained exception trace may own a COW
/// snapshot of the internal-call arguments; preserving the reference target
/// keeps that snapshot observationally equal to the array at throw time.
fn release_array_walk_cursor_reference_after_exception(
    owner: &mut Value,
    walked_identity: usize,
    position: Option<usize>,
    binding: Value,
    was_reference: bool,
) {
    if was_reference || binding.owned_reference_is_aliased() {
        return;
    }
    let replacement = binding.dereferenced().clone();
    if let Some(position) = position
        && owner.array_identity() == Some(walked_identity)
        && let Some(array) = owner.as_array_mut()
        && array
            .get_value_at(position)
            .and_then(Value::reference_identity)
            == binding.reference_identity()
    {
        let _ = array.set_value_at(position, replacement);
    }
}

#[inline]
fn report_invalidated_array_walk_owner(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        "Iterated value is no longer an array or object",
    ));
}

/// Execute the proven scalar prefix of a by-reference walk without temporary
/// reference cells or callback frames. The returned position is the first
/// untouched member that needs canonical replay; `array.len()` means the
/// complete stable array was handled.
fn execute_array_walk_scalar_long_reference_mutations(
    owner: &mut Value,
    callback: &ScalarLongReferenceMutationCallback,
    captures: &[Value],
) -> Option<usize> {
    let array = owner.as_array_mut()?;
    let len = array.len();
    for position in 0..len {
        let mut arguments = [0i64; 8];
        match array.get_at(position) {
            Some((value, ArrayKey::Int(key)))
                if value.dereferenced().value_type() == ValueType::Long =>
            {
                arguments[0] = value
                    .dereferenced()
                    .as_long()
                    .expect("guarded scalar walk member must remain Long");
                arguments[1] = key;
            }
            _ => {
                callback.record_calls(position as u64);
                return Some(position);
            }
        }
        for (index, capture) in captures.iter().enumerate() {
            let Some(capture) = capture.dereferenced().as_long() else {
                callback.record_calls(position as u64);
                return Some(position);
            };
            arguments[index + 2] = capture;
        }
        let Some(result) = callback.evaluate_longs(&arguments[..captures.len() + 2]) else {
            callback.record_calls(position as u64);
            return Some(position);
        };
        if !array.assign_dereferenced_at(position, Value::long(result)) {
            callback.record_calls(position as u64);
            return Some(position);
        }
    }
    callback.record_calls(len as u64);
    Some(len)
}

fn execute_array_walk_scalar_long_reference_mutation_at(
    owner: &mut Value,
    position: usize,
    callback: &ScalarLongReferenceMutationCallback,
    captures: &[Value],
) -> bool {
    let mut arguments = [0i64; 8];
    let Some((value, ArrayKey::Int(key))) =
        owner.as_array().and_then(|array| array.get_at(position))
    else {
        return false;
    };
    let Some(value) = value.dereferenced().as_long() else {
        return false;
    };
    arguments[0] = value;
    arguments[1] = key;
    for (index, capture) in captures.iter().enumerate() {
        let Some(capture) = capture.dereferenced().as_long() else {
            return false;
        };
        arguments[index + 2] = capture;
    }
    let Some(result) = callback.evaluate_longs(&arguments[..captures.len() + 2]) else {
        return false;
    };
    owner
        .as_array_mut()
        .is_some_and(|array| array.assign_dereferenced_at(position, Value::long(result)))
}

/// array_walk(&$array, $callback, $arg = null): true
/// Supports by-ref callbacks: function (&$val, $key) { $val *= 2; }
#[inline(never)]
unsafe fn try_array_walk_scalar_long(arr: &PhpArray, resolved: &ResolvedCallback) -> Option<()> {
    if !resolved.prepend_args.is_empty()
        || !resolved.use_vars.is_empty()
        || resolved.has_context()
        || resolved.closure_static_vars.is_some()
    {
        return None;
    }
    let callback = prepare_scalar_long_callback(resolved.func_ptr, 2)?;
    let values = arr.packed_values()?;
    for (key, value) in values.iter().enumerate() {
        if value.value_type() != ValueType::Long || value.is_reference() {
            return None;
        }
        callback.evaluate_longs(&[value.raw_long(), i64::try_from(key).ok()?])?;
    }
    callback.record_calls(values.len() as u64);
    Some(())
}

fn fn_array_walk(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 1).clone();
    let userdata = arg_opt!(ed, 2).cloned();
    let source_owner = arg!(ed, 0).dereferenced().clone();
    if source_owner.as_array().is_none() && source_owner.as_object().is_none() {
        typed_internal_argument_error(eg, "array_walk", &source_owner, 1, "array", "array");
        return Ok(());
    }
    let arr_ptr: *mut Value = arg_mut!(ed, 0);
    let initialized_object = if eg.is_uninitialized_lazy_object(&source_owner) {
        Some(reflection::initialize_lazy_object(eg, &source_owner)?)
    } else {
        eg.lazy_proxy_instance(&source_owner)
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let object_target = initialized_object.as_ref().unwrap_or(&source_owner);
    if let Some(object) = object_target.as_object() {
        let class_id = object.class_id;
        let class_name = object.class_name.to_string();
        // array_walk() uses the object-to-array projection: inaccessible
        // declared properties remain present under visibility-mangled keys.
        let declared = eg
            .class_by_id(class_id)
            .map(|class| {
                class
                    .properties
                    .iter()
                    .enumerate()
                    .filter(|(_, definition)| !definition.is_virtual_hook_property())
                    .map(|(slot, definition)| (slot, definition.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dynamic_names = {
            let mut names = Vec::new();
            object.for_each_dynamic_property(|name, value| {
                if !value.is_undef()
                    && !declared
                        .iter()
                        .any(|(_, definition)| definition.name == name)
                {
                    names.push(name.to_string());
                }
            });
            names
        };
        drop(object);

        let resolved = match resolve_callback_at_callsite_checked(&callback, eg, ed)? {
            Some(resolved) => resolved,
            None => {
                if eg.exception.is_none() {
                    let reason = ordinary_callback_invalid_reason(&callback, eg);
                    eg.exception = Some(crate::value::make_error_value(
                        "TypeError",
                        &format!(
                            "array_walk(): Argument #2 ($callback) must be a valid callback, {reason}"
                        ),
                    ));
                }
                return Ok(());
            }
        };
        let callback_arg0_by_ref = resolved.signature().is_param_by_ref(0);
        for (slot, definition) in declared {
            let argument = if callback_arg0_by_ref {
                let mut object = object_target
                    .as_object_mut()
                    .expect("array_walk object target must remain live");
                let property = object
                    .get_property_slot_mut(slot)
                    .expect("visible array_walk property must remain addressable");
                if property.is_undef() {
                    continue;
                }
                let binding = if property.is_owned_reference() {
                    property.clone_owned_reference_alias()
                } else {
                    let current = std::mem::replace(property, Value::undef());
                    let binding = Value::owned_reference(current.dereferenced().clone());
                    *property = binding.clone_owned_reference_alias();
                    binding
                };
                let owner = object.instance_property_reference_owner(slot);
                drop(object);
                if definition.is_typed() {
                    binding.add_reference_property_constraint(
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
                binding
            } else {
                let Some(value) = object_target
                    .as_object()
                    .and_then(|object| object.get_property_slot(slot).cloned())
                else {
                    continue;
                };
                if value.is_undef() {
                    continue;
                }
                value
            };
            let key = array_walk_declared_property_key(&definition);
            if !report_array_walk_userdata_reference_warning(
                ed,
                eg,
                &resolved,
                &callback,
                userdata.is_some(),
            )? {
                return Ok(());
            }
            let public_args = 2 + usize::from(userdata.is_some());
            let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
            call_resolved_owned_iter(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(std::iter::once(argument))
                    .chain(std::iter::once(key))
                    .chain(userdata.iter().cloned())
                    .chain(resolved.use_vars.iter().cloned()),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        for name in dynamic_names {
            let argument = object_target
                .as_object()
                .and_then(|object| {
                    object
                        .get_dynamic_property_with_position(&name)
                        .map(|(value, _)| value.clone())
                })
                .unwrap_or_else(Value::null);
            let key = Value::string(name.clone());
            if !report_array_walk_userdata_reference_warning(
                ed,
                eg,
                &resolved,
                &callback,
                userdata.is_some(),
            )? {
                return Ok(());
            }
            let public_args = 2 + usize::from(userdata.is_some());
            let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
            let arguments = resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(std::iter::once(argument))
                .chain(std::iter::once(key))
                .chain(userdata.iter().cloned())
                .chain(resolved.use_vars.iter().cloned());
            if callback_arg0_by_ref {
                let (_, modified) =
                    call_resolved_owned_iter_readback_arg0(eg, &resolved, num_args, arguments)?;
                if let Some(mut object) = object_target.as_object_mut() {
                    object.set_dynamic_property(&name, modified);
                }
            } else {
                call_resolved_owned_iter(eg, &resolved, num_args, arguments)?;
            }
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        ret!(rv, Value::bool(true));
    }

    // The validation snapshot is needed only by the object/lazy-object path.
    // Releasing it before an array walk preserves callback-time destructor
    // order when a live member is removed.
    drop(initialized_object);
    drop(source_owner);

    // SAFETY: `arr_ptr` targets the caller's by-reference argument cell. The
    // caller frame outlives this internal call; no Rust reference to the cell
    // is retained across a PHP callback, which may replace its contained
    // Value. Each temporary borrow ends before callback dispatch.
    unsafe {
        let arr = (&*arr_ptr)
            .as_array()
            .expect("validated array_walk source must remain an array before callbacks");

        let resolved = match resolve_callback_at_callsite_checked(&callback, eg, ed)? {
            Some(r) => r,
            None => {
                if eg.exception.is_none() {
                    let reason = ordinary_callback_invalid_reason(&callback, eg);
                    eg.exception = Some(crate::value::make_error_value(
                        "TypeError",
                        &format!(
                            "array_walk(): Argument #2 ($callback) must be a valid callback, {reason}"
                        ),
                    ));
                }
                return Ok(());
            }
        };

        // A pure by-value callback cannot observe the discarded return values or
        // mutate the walked array. Packed Long members and integer keys can use
        // the shared scalar callback ABI without cloning a snapshot or frames.
        if userdata.is_none() && try_array_walk_scalar_long(arr, &resolved).is_some() {
            ret!(rv, Value::bool(true));
        }

        // Check if callback's first parameter is declared by-reference.
        let cb_arg0_by_ref = resolved.signature().is_param_by_ref(0);
        let mut position = if cb_arg0_by_ref
            && userdata.is_none()
            && resolved.prepend_args.is_empty()
            && !resolved.has_context()
            && resolved.closure_static_vars.is_none()
            && !resolved.is_magic_call
            && !reject_scope_introspection_callback(eg, &resolved)
            && let Some(callback) = prepare_scalar_long_reference_mutation_callback(
                resolved.func_ptr,
                resolved.use_vars.len(),
            ) {
            let position = execute_array_walk_scalar_long_reference_mutations(
                &mut *arr_ptr,
                &callback,
                &resolved.use_vars,
            )
            .unwrap_or(0);
            if (&*arr_ptr)
                .as_array()
                .is_some_and(|array| position == array.len())
            {
                ret!(rv, Value::bool(true));
            }
            position
        } else {
            0
        };
        let mut expected_identity = (&*arr_ptr)
            .array_identity()
            .expect("validated array_walk source must remain an array before callbacks");
        let mut anchors = Vec::new();
        let mut reusable_reference = None;
        loop {
            let Some(current_identity) = (&*arr_ptr).array_identity() else {
                report_invalidated_array_walk_owner(eg);
                return Ok(());
            };
            if current_identity != expected_identity {
                position = 0;
                anchors.clear();
            }
            let Some((walked_identity, key, reference_identity, binding, was_reference)) =
                array_walk_live_entry(&mut *arr_ptr, position, &mut reusable_reference)
            else {
                break;
            };
            let key_value = match &key {
                ArrayKey::Int(key) => Value::long(*key),
                ArrayKey::String(key) => Value::string(key.clone()),
            };
            if !report_array_walk_userdata_reference_warning(
                ed,
                eg,
                &resolved,
                &callback,
                userdata.is_some(),
            )? {
                return Ok(());
            }
            let public_args = 2 + usize::from(userdata.is_some());
            let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
            let argument = if cb_arg0_by_ref {
                binding.clone_closure_capture()
            } else {
                binding.dereferenced().clone()
            };
            call_array_walk_resolved_owned_iter(
                ed,
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(std::iter::once(argument))
                    .chain(std::iter::once(key_value))
                    .chain(userdata.iter().cloned())
                    .chain(resolved.use_vars.iter().cloned()),
            )?;

            if eg.exception.is_some() {
                if (&*arr_ptr).array_identity() == Some(walked_identity) {
                    let member_position = (&*arr_ptr).as_array().and_then(|array| {
                        array_walk_reference_position(array, reference_identity, position)
                    });
                    release_array_walk_cursor_reference_after_exception(
                        &mut *arr_ptr,
                        walked_identity,
                        member_position,
                        binding,
                        was_reference,
                    );
                }
                return Ok(());
            }

            let Some((mut next_identity, next_position, member_position)) =
                advance_array_walk_cursor(
                    &*arr_ptr,
                    walked_identity,
                    position,
                    key,
                    reference_identity,
                    &mut anchors,
                )
            else {
                report_invalidated_array_walk_owner(eg);
                return Ok(());
            };
            if next_identity == walked_identity {
                next_identity = release_array_walk_cursor_reference(
                    &mut *arr_ptr,
                    walked_identity,
                    member_position,
                    binding,
                    was_reference,
                    &mut reusable_reference,
                );
            }
            expected_identity = next_identity;
            position = next_position;
        }
        ret!(rv, Value::bool(true));
    }
}

fn walk_array_recursive_snapshot(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    callback: &Value,
    array: &PhpArray,
    userdata: Option<&Value>,
    callback_arg0_by_ref: bool,
) -> Result<PhpArray, VmError> {
    let pairs = array
        .iter()
        .map(|(key, value)| (key, value.clone()))
        .collect::<Vec<_>>();
    // Start from a complete snapshot so an exception preserves both every
    // committed mutation and all untouched entries after the failing leaf.
    let mut result = array.clone();
    for (key, value) in pairs {
        let value = if let Some(nested) = value.as_array() {
            Value::array(walk_array_recursive_snapshot(
                ed,
                eg,
                resolved,
                callback,
                nested,
                userdata,
                callback_arg0_by_ref,
            )?)
        } else {
            let key_value = match &key {
                ArrayKey::Int(key) => Value::long(*key),
                ArrayKey::String(key) => Value::string(key.clone()),
            };
            if !report_array_walk_userdata_reference_warning(
                ed,
                eg,
                resolved,
                callback,
                userdata.is_some(),
            )? {
                return Ok(result);
            }
            let public_args = 2 + usize::from(userdata.is_some());
            let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
            let arguments = resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(std::iter::once(value.clone()))
                .chain(std::iter::once(key_value))
                .chain(userdata.into_iter().cloned())
                .chain(resolved.use_vars.iter().cloned());
            if callback_arg0_by_ref {
                let (_, modified) =
                    call_resolved_owned_iter_readback_arg0(eg, resolved, num_args, arguments)?;
                modified
            } else {
                call_resolved_owned_iter(eg, resolved, num_args, arguments)?;
                value
            }
        };
        result.set(key, value);
        if eg.exception.is_some() {
            return Ok(result);
        }
    }
    Ok(result)
}

fn walk_array_recursive_live(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    callback: &Value,
    owner: *mut Value,
    userdata: Option<&Value>,
    callback_arg0_by_ref: bool,
    scalar_callback: Option<&ScalarLongReferenceMutationCallback>,
    scalar_completed: &mut u64,
) -> Result<(), VmError> {
    // SAFETY: `owner` is either the live by-reference argument cell or a
    // target kept alive by an owned PHP reference in the parent invocation.
    // Temporary Rust borrows end before callback dispatch, which may mutate
    // or detach any ancestor array while the owned reference preserves this
    // recursive target.
    unsafe {
        let Some(mut expected_identity) = (&*owner).array_identity() else {
            report_invalidated_array_walk_owner(eg);
            return Ok(());
        };
        let mut position = 0usize;
        let mut anchors = Vec::new();
        let mut reusable_reference = None;
        loop {
            let Some(current_identity) = (&*owner).array_identity() else {
                report_invalidated_array_walk_owner(eg);
                return Ok(());
            };
            if current_identity != expected_identity {
                position = 0;
                anchors.clear();
            }
            if let Some(scalar_callback) = scalar_callback
                && execute_array_walk_scalar_long_reference_mutation_at(
                    &mut *owner,
                    position,
                    scalar_callback,
                    &resolved.use_vars,
                )
            {
                *scalar_completed = scalar_completed.saturating_add(1);
                expected_identity = (&*owner)
                    .array_identity()
                    .expect("scalar recursive mutation must retain its array owner");
                position += 1;
                continue;
            }
            let Some((walked_identity, key, reference_identity, binding, was_reference)) =
                array_walk_live_entry(&mut *owner, position, &mut reusable_reference)
            else {
                break;
            };
            let nested = binding.dereferenced().as_array().is_some();
            if nested {
                // The owned cursor alias keeps the nested member live even when a
                // callback removes its parent from an ancestor array.
                let nested_owner = binding.as_ref_ptr();
                walk_array_recursive_live(
                    ed,
                    eg,
                    resolved,
                    callback,
                    nested_owner,
                    userdata,
                    callback_arg0_by_ref,
                    scalar_callback,
                    scalar_completed,
                )?;
            } else {
                let key_value = match &key {
                    ArrayKey::Int(key) => Value::long(*key),
                    ArrayKey::String(key) => Value::string(key.clone()),
                };
                if !report_array_walk_userdata_reference_warning(
                    ed,
                    eg,
                    resolved,
                    callback,
                    userdata.is_some(),
                )? {
                    return Ok(());
                }
                let public_args = 2 + usize::from(userdata.is_some());
                let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
                let argument = if callback_arg0_by_ref {
                    binding.clone_closure_capture()
                } else {
                    binding.dereferenced().clone()
                };
                call_array_walk_resolved_owned_iter(
                    ed,
                    eg,
                    resolved,
                    num_args,
                    resolved
                        .prepend_args
                        .iter()
                        .cloned()
                        .chain(std::iter::once(argument))
                        .chain(std::iter::once(key_value))
                        .chain(userdata.into_iter().cloned())
                        .chain(resolved.use_vars.iter().cloned()),
                )?;
            }

            if eg.exception.is_some() {
                if (&*owner).array_identity() == Some(walked_identity) {
                    let member_position = (&*owner).as_array().and_then(|array| {
                        array_walk_reference_position(array, reference_identity, position)
                    });
                    release_array_walk_cursor_reference_after_exception(
                        &mut *owner,
                        walked_identity,
                        member_position,
                        binding,
                        was_reference,
                    );
                }
                return Ok(());
            }

            let Some((mut next_identity, next_position, member_position)) =
                advance_array_walk_cursor(
                    &*owner,
                    walked_identity,
                    position,
                    key,
                    reference_identity,
                    &mut anchors,
                )
            else {
                report_invalidated_array_walk_owner(eg);
                return Ok(());
            };
            if next_identity == walked_identity {
                next_identity = release_array_walk_cursor_reference(
                    &mut *owner,
                    walked_identity,
                    member_position,
                    binding,
                    was_reference,
                    &mut reusable_reference,
                );
            }
            expected_identity = next_identity;
            position = next_position;
        }
        Ok(())
    }
}

fn walk_object_recursive(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    callback: &Value,
    object_target: &Value,
    userdata: Option<&Value>,
    callback_arg0_by_ref: bool,
) -> Result<(), VmError> {
    let Some(object) = object_target.as_object() else {
        return Ok(());
    };
    let class_id = object.class_id;
    let class_name = object.class_name.to_string();
    let declared = eg
        .class_by_id(class_id)
        .map(|class| {
            class
                .properties
                .iter()
                .enumerate()
                .filter(|(_, definition)| !definition.is_virtual_hook_property())
                .map(|(slot, definition)| (slot, definition.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dynamic_names = {
        let mut names = Vec::new();
        object.for_each_dynamic_property(|name, value| {
            if !value.is_undef()
                && !declared
                    .iter()
                    .any(|(_, definition)| definition.name == name)
            {
                names.push(name.to_string());
            }
        });
        names
    };
    drop(object);

    for (slot, definition) in declared {
        let Some(value) = object_target
            .as_object()
            .and_then(|object| object.get_property_slot(slot).cloned())
        else {
            continue;
        };
        if value.is_undef() {
            continue;
        }
        if let Some(nested) = value.dereferenced().as_array() {
            let walked = walk_array_recursive_snapshot(
                ed,
                eg,
                resolved,
                callback,
                nested,
                userdata,
                callback_arg0_by_ref,
            )?;
            if let Some(mut object) = object_target.as_object_mut() {
                if let Some(property) = object.get_property_slot_mut(slot) {
                    replace_array_walk_property_value(property, Value::array(walked));
                }
            }
            if eg.exception.is_some() {
                return Ok(());
            }
            continue;
        }

        if !report_array_walk_userdata_reference_warning(
            ed,
            eg,
            resolved,
            callback,
            userdata.is_some(),
        )? {
            return Ok(());
        }
        let argument = if callback_arg0_by_ref {
            let mut object = object_target
                .as_object_mut()
                .expect("recursive walk object target must remain live");
            let property = object
                .get_property_slot_mut(slot)
                .expect("recursive walk property must remain addressable");
            let binding = if property.is_owned_reference() {
                property.clone_owned_reference_alias()
            } else {
                let current = std::mem::replace(property, Value::undef());
                let binding = Value::owned_reference(current.dereferenced().clone());
                *property = binding.clone_owned_reference_alias();
                binding
            };
            let owner = object.instance_property_reference_owner(slot);
            drop(object);
            if definition.is_typed() {
                binding.add_reference_property_constraint(
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
            binding
        } else {
            value
        };
        let public_args = 2 + usize::from(userdata.is_some());
        let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
        call_resolved_owned_iter(
            eg,
            resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(std::iter::once(argument))
                .chain(std::iter::once(array_walk_declared_property_key(
                    &definition,
                )))
                .chain(userdata.into_iter().cloned())
                .chain(resolved.use_vars.iter().cloned()),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }

    for name in dynamic_names {
        let Some(value) = object_target.as_object().and_then(|object| {
            object
                .get_dynamic_property_with_position(&name)
                .map(|(value, _)| value.clone())
        }) else {
            continue;
        };
        if let Some(nested) = value.dereferenced().as_array() {
            let walked = walk_array_recursive_snapshot(
                ed,
                eg,
                resolved,
                callback,
                nested,
                userdata,
                callback_arg0_by_ref,
            )?;
            if let Some(mut object) = object_target.as_object_mut() {
                if let Some(property) = object.get_dynamic_property_mut(&name) {
                    replace_array_walk_property_value(property, Value::array(walked));
                }
            }
            if eg.exception.is_some() {
                return Ok(());
            }
            continue;
        }

        if !report_array_walk_userdata_reference_warning(
            ed,
            eg,
            resolved,
            callback,
            userdata.is_some(),
        )? {
            return Ok(());
        }
        let argument = if callback_arg0_by_ref {
            let mut object = object_target
                .as_object_mut()
                .expect("recursive walk object target must remain live");
            let property = object
                .get_dynamic_property_mut(&name)
                .expect("recursive walk dynamic property must remain addressable");
            if property.is_owned_reference() {
                property.clone_owned_reference_alias()
            } else {
                let current = std::mem::replace(property, Value::undef());
                let binding = Value::owned_reference(current.dereferenced().clone());
                *property = binding.clone_owned_reference_alias();
                binding
            }
        } else {
            value
        };
        let public_args = 2 + usize::from(userdata.is_some());
        let num_args = resolved.prepend_args.len() + public_args + resolved.use_vars.len();
        call_resolved_owned_iter(
            eg,
            resolved,
            num_args,
            resolved
                .prepend_args
                .iter()
                .cloned()
                .chain(std::iter::once(argument))
                .chain(std::iter::once(Value::string(name)))
                .chain(userdata.into_iter().cloned())
                .chain(resolved.use_vars.iter().cloned()),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    Ok(())
}

fn fn_array_walk_recursive(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 1).clone();
    let userdata = arg_opt!(ed, 2).cloned();
    let source_owner = arg!(ed, 0).dereferenced().clone();
    let source_ptr: *mut Value = arg_mut!(ed, 0);
    if source_owner.as_array().is_none() && source_owner.as_object().is_none() {
        typed_internal_argument_error(
            eg,
            "array_walk_recursive",
            &source_owner,
            1,
            "array",
            "array",
        );
        return Ok(());
    }
    let initialized_object = if eg.is_uninitialized_lazy_object(&source_owner) {
        Some(reflection::initialize_lazy_object(eg, &source_owner)?)
    } else {
        eg.lazy_proxy_instance(&source_owner)
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    let object_target = initialized_object.as_ref().unwrap_or(&source_owner);
    let resolved = match resolve_callback_at_callsite_checked(&callback, eg, ed)? {
        Some(resolved) => resolved,
        None => {
            if eg.exception.is_none() {
                let reason = ordinary_callback_invalid_reason(&callback, eg);
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "array_walk_recursive(): Argument #2 ($callback) must be a valid callback, {reason}"
                    ),
                ));
            }
            return Ok(());
        }
    };
    let callback_arg0_by_ref = resolved.signature().is_param_by_ref(0);
    if object_target.as_object().is_some() {
        walk_object_recursive(
            ed,
            eg,
            &resolved,
            &callback,
            object_target,
            userdata.as_ref(),
            callback_arg0_by_ref,
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, Value::bool(true));
    }
    drop(initialized_object);
    drop(source_owner);
    let scalar_callback = (callback_arg0_by_ref
        && userdata.is_none()
        && resolved.prepend_args.is_empty()
        && !resolved.has_context()
        && resolved.closure_static_vars.is_none()
        && !resolved.is_magic_call
        && !reject_scope_introspection_callback(eg, &resolved))
    .then(|| {
        prepare_scalar_long_reference_mutation_callback(resolved.func_ptr, resolved.use_vars.len())
    })
    .flatten();
    let mut scalar_completed = 0u64;
    let walk_result = walk_array_recursive_live(
        ed,
        eg,
        &resolved,
        &callback,
        source_ptr,
        userdata.as_ref(),
        callback_arg0_by_ref,
        scalar_callback.as_ref(),
        &mut scalar_completed,
    );
    if let Some(callback) = scalar_callback.as_ref() {
        callback.record_calls(scalar_completed);
    }
    walk_result?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, Value::bool(true));
}

/// asort(&$array): bool — sort by value, preserve keys
fn fn_asort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let external_byte_keys = php_arr.has_external_byte_keys();
        let utf8_text_keys = php_arr.has_utf8_text_keys();
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect();
        if !sort_direct_long_entries(&mut pairs, flags, false, |(_, value)| value)
            && !sort_direct_total_scalar_entries(
                &mut pairs,
                flags,
                false,
                eg.precision,
                |(_, value)| value,
            )
        {
            stable_sort_checked(&mut pairs, |(_, left), (_, right)| {
                sort_value_order_runtime(ed, eg, left, right, flags)
            })?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        let mut new_arr = PhpArray::new();
        for (key, value) in pairs {
            new_arr.set(key, array_projection_value(&value));
        }
        if external_byte_keys {
            new_arr.mark_external_byte_keys();
        }
        if utf8_text_keys {
            new_arr.mark_utf8_text_keys();
        }
        *arr = Value::array(new_arr);
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

/// arsort(&$array): bool — reverse sort by value, preserve keys
fn fn_arsort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let external_byte_keys = php_arr.has_external_byte_keys();
        let utf8_text_keys = php_arr.has_utf8_text_keys();
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect();
        if !sort_direct_long_entries(&mut pairs, flags, true, |(_, value)| value)
            && !sort_direct_total_scalar_entries(
                &mut pairs,
                flags,
                true,
                eg.precision,
                |(_, value)| value,
            )
        {
            stable_sort_checked(&mut pairs, |(_, left), (_, right)| {
                sort_value_order_runtime(ed, eg, left, right, flags)
                    .map(std::cmp::Ordering::reverse)
            })?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
        let mut new_arr = PhpArray::new();
        for (key, value) in pairs {
            new_arr.set(key, array_projection_value(&value));
        }
        if external_byte_keys {
            new_arr.mark_external_byte_keys();
        }
        if utf8_text_keys {
            new_arr.mark_utf8_text_keys();
        }
        *arr = Value::array(new_arr);
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

/// ksort(&$array): bool — sort by key
fn fn_ksort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let external_byte_keys = php_arr.has_external_byte_keys();
        let utf8_text_keys = php_arr.has_utf8_text_keys();
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect();
        stable_sort_checked(&mut pairs, |(left, _), (right, _)| {
            Ok::<_, VmError>(
                sort_key_order(left, right, flags, eg.precision)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })?;
        let mut new_arr = PhpArray::new();
        for (key, value) in pairs {
            new_arr.set(key, array_projection_value(&value));
        }
        if external_byte_keys {
            new_arr.mark_external_byte_keys();
        }
        if utf8_text_keys {
            new_arr.mark_utf8_text_keys();
        }
        *arr = Value::array(new_arr);
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

/// krsort(&$array): bool — reverse sort by key
fn fn_krsort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let external_byte_keys = php_arr.has_external_byte_keys();
        let utf8_text_keys = php_arr.has_utf8_text_keys();
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(key, value)| (key, array_sort_snapshot_value(value)))
            .collect();
        stable_sort_checked(&mut pairs, |(left, _), (right, _)| {
            Ok::<_, VmError>(
                sort_key_order(left, right, flags, eg.precision)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .reverse(),
            )
        })?;
        let mut new_arr = PhpArray::new();
        for (key, value) in pairs {
            new_arr.set(key, array_projection_value(&value));
        }
        if external_byte_keys {
            new_arr.mark_external_byte_keys();
        }
        if utf8_text_keys {
            new_arr.mark_utf8_text_keys();
        }
        *arr = Value::array(new_arr);
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

// ============================================================================
// Missing math functions
// ============================================================================

#[inline(always)]
fn direct_sin(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().sin()))
}

fn fn_sin(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_sin(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
fn fn_cos(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).cos()));
}
#[inline(always)]
fn direct_tan(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().tan()))
}
fn fn_tan(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_tan(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
#[inline(always)]
fn direct_asin(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().asin()))
}
fn fn_asin(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_asin(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
#[inline(always)]
fn direct_acos(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().acos()))
}
fn fn_acos(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_acos(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
#[inline(always)]
fn direct_atan(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().atan()))
}
fn fn_atan(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_atan(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
fn fn_atan2(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::double(arg_float!(ed, 0).atan2(arg_float!(ed, 1)))
    );
}
#[inline(always)]
fn direct_exp(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::double(direct_arg(args, 0).to_float_val().exp()))
}
fn fn_exp(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_exp(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}
fn fn_sinh(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).sinh()));
}
fn fn_cosh(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).cosh()));
}
fn fn_tanh(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).tanh()));
}
fn fn_deg2rad(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).to_radians()));
}
fn fn_rad2deg(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).to_degrees()));
}
fn fn_hypot(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::double(arg_float!(ed, 0).hypot(arg_float!(ed, 1)))
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BaseConvertNumber {
    Integer(i64),
    Float(f64),
}

fn base_convert_digit(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(byte - b'A') + 10),
        _ => None,
    }
}

/// Parse PHP's deliberately permissive base-conversion input. Leading and
/// trailing ASCII whitespace and a matching 0b/0o/0x prefix are admitted;
/// every other invalid byte is ignored and reported once by the caller.
fn parse_base_convert_number(bytes: &[u8], base: u32) -> (BaseConvertNumber, bool) {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    let bytes = &bytes[start..end];
    let prefix = match base {
        2 => [b'0', b'b'],
        8 => [b'0', b'o'],
        16 => [b'0', b'x'],
        _ => [0, 0],
    };
    let offset = usize::from(
        prefix[0] != 0
            && bytes.len() >= 2
            && bytes[0] == prefix[0]
            && bytes[1].eq_ignore_ascii_case(&prefix[1]),
    ) * 2;

    let mut integer = 0_i64;
    let mut float = None;
    let mut invalid = false;
    for &byte in &bytes[offset..] {
        let Some(digit) = base_convert_digit(byte).filter(|digit| *digit < base) else {
            invalid = true;
            continue;
        };
        if let Some(number) = &mut float {
            *number = *number * f64::from(base) + f64::from(digit);
        } else if let Some(number) = integer
            .checked_mul(i64::from(base))
            .and_then(|number| number.checked_add(i64::from(digit)))
        {
            integer = number;
        } else {
            float = Some(integer as f64 * f64::from(base) + f64::from(digit));
        }
    }
    (
        float.map_or(
            BaseConvertNumber::Integer(integer),
            BaseConvertNumber::Float,
        ),
        invalid,
    )
}

fn format_base_convert_number(number: BaseConvertNumber, base: u32) -> Option<String> {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut output = Vec::new();
    match number {
        BaseConvertNumber::Integer(mut number) => {
            while number >= 1 {
                output.push(DIGITS[(number % i64::from(base)) as usize]);
                number /= i64::from(base);
            }
        }
        BaseConvertNumber::Float(mut number) => {
            if !number.is_finite() {
                return None;
            }
            while number >= 1.0 {
                let digit = (number % f64::from(base)) as usize;
                output.push(DIGITS[digit]);
                number = (number / f64::from(base)).floor();
            }
        }
    }
    if output.is_empty() {
        output.push(b'0');
    } else {
        output.reverse();
    }
    Some(String::from_utf8(output).expect("base conversion digits are ASCII"))
}

fn base_convert_type_error(
    eg: &mut ExecutorGlobals,
    argument: &Value,
    position: usize,
    parameter: &str,
    expected: &str,
) {
    let actual = match argument.value_type() {
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        _ => argument.diagnostic_type_name().into_owned(),
    };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "base_convert(): Argument #{position} (${parameter}) must be of type {expected}, {actual} given"
        ),
    ));
}

fn base_convert_string_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<Option<String>, VmError> {
    let argument = owned_argument(ed, 0);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::String => Some(argument.as_str().unwrap_or("").to_string()),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                "base_convert(): Passing null to parameter #1 ($num) of type string is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(String::new())
        }
        ValueType::False if !strict => Some(String::new()),
        ValueType::True if !strict => Some("1".to_string()),
        ValueType::Long | ValueType::Double if !strict => {
            if argument.as_double().is_some_and(f64::is_nan) {
                report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "unexpected NAN value was coerced to string",
                )?;
                if eg.exception.is_some() {
                    return Ok(None);
                }
            }
            Some(argument.echo_to_string_with_precision(eg.precision))
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(rendered) = rendered else {
                base_convert_type_error(eg, argument, 1, "num", "string");
                return Ok(None);
            };
            let Some(rendered) = rendered.as_str() else {
                let class_name = argument.diagnostic_type_name();
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!("{class_name}::__toString(): Return value must be of type string"),
                ));
                return Ok(None);
            };
            Some(rendered.to_string())
        }
        _ => {
            base_convert_type_error(eg, argument, 1, "num", "string");
            None
        }
    };
    Ok(converted)
}

fn typed_internal_int_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<i64>, VmError> {
    typed_internal_int_argument_expected(ed, eg, function, index, parameter, "int")
}

fn typed_internal_int_argument_expected(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
    expected: &str,
) -> Result<Option<i64>, VmError> {
    let argument = owned_argument(ed, index);
    let argument = argument.dereferenced();
    let strict = internal_call_is_strict(ed);
    let converted = match argument.value_type() {
        ValueType::Long => argument.as_long(),
        ValueType::Null if !strict => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "{function}(): Passing null to parameter #{} (${parameter}) of type int is deprecated",
                    index + 1
                ),
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            Some(0)
        }
        ValueType::True | ValueType::False if !strict => Some(i64::from(argument.is_truthy())),
        ValueType::Double if !strict => {
            let number = argument.as_double().unwrap_or(f64::NAN);
            let upper_exclusive = -(i64::MIN as f64);
            if !number.is_finite() || number < i64::MIN as f64 || number >= upper_exclusive {
                None
            } else {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float {} to int loses precision",
                            argument.echo_to_string_with_precision(-1)
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                Some(integer)
            }
        }
        ValueType::String if !strict => {
            let source = argument.as_str().unwrap_or("");
            let Some(number) = php_numeric_string_to_float(source) else {
                typed_internal_argument_error(
                    eg,
                    function,
                    argument,
                    index as usize + 1,
                    parameter,
                    expected,
                );
                return Ok(None);
            };
            let upper_exclusive = -(i64::MIN as f64);
            if !number.is_finite() || number < i64::MIN as f64 || number >= upper_exclusive {
                None
            } else {
                let integer = number as i64;
                if integer as f64 != number {
                    report_internal_deprecation(
                        eg,
                        ed,
                        &format!(
                            "Implicit conversion from float-string \"{source}\" to int loses precision"
                        ),
                    )?;
                    if eg.exception.is_some() {
                        return Ok(None);
                    }
                }
                Some(integer)
            }
        }
        _ => None,
    };
    if converted.is_none() && eg.exception.is_none() {
        typed_internal_argument_error(
            eg,
            function,
            argument,
            index as usize + 1,
            parameter,
            expected,
        );
    }
    Ok(converted)
}

fn fn_base_convert(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = base_convert_string_argument(ed, eg)? else {
        return Ok(());
    };
    let Some(from_base) = typed_internal_int_argument(ed, eg, "base_convert", 1, "from_base")?
    else {
        return Ok(());
    };
    let Some(to_base) = typed_internal_int_argument(ed, eg, "base_convert", 2, "to_base")? else {
        return Ok(());
    };
    if !(2..=36).contains(&from_base) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "base_convert(): Argument #2 ($from_base) must be between 2 and 36 (inclusive)",
        ));
        return Ok(());
    }
    if !(2..=36).contains(&to_base) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "base_convert(): Argument #3 ($to_base) must be between 2 and 36 (inclusive)",
        ));
        return Ok(());
    }

    let (number, invalid) = parse_base_convert_number(
        &php_string_to_bytes(&number),
        u32::try_from(from_base).unwrap(),
    );
    if invalid {
        report_internal_deprecation(
            eg,
            ed,
            "Invalid characters passed for attempted conversion, these have been ignored",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    let to_base = u32::try_from(to_base).unwrap();
    let Some(output) = format_base_convert_number(number, to_base) else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!("An infinite value cannot be converted to base {to_base}"),
        ));
        return Ok(());
    };
    ret!(rv, Value::string(output));
}

fn fn_decbin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = typed_internal_int_argument(ed, eg, "decbin", 0, "num")? else {
        return Ok(());
    };
    ret!(rv, Value::string(format!("{:b}", number as u64)));
}

fn fn_dechex(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = typed_internal_int_argument(ed, eg, "dechex", 0, "num")? else {
        return Ok(());
    };
    ret!(rv, Value::string(format!("{:x}", number as u64)));
}

#[cfg(test)]
mod base_convert_tests {
    use super::{BaseConvertNumber, format_base_convert_number, parse_base_convert_number};

    fn convert(input: &str, from: u32, to: u32) -> (String, bool) {
        let (number, invalid) = parse_base_convert_number(input.as_bytes(), from);
        (format_base_convert_number(number, to).unwrap(), invalid)
    }

    #[test]
    fn supports_bases_prefixes_whitespace_and_ignored_bytes() {
        assert_eq!(
            convert("a37334", 16, 2),
            ("101000110111001100110100".into(), false)
        );
        assert_eq!(convert("\t0Xff\n", 16, 10), ("255".into(), false));
        assert_eq!(convert("0b101", 2, 10), ("5".into(), false));
        assert_eq!(convert("0o77", 8, 10), ("63".into(), false));
        assert_eq!(convert("&4#2", 10, 10), ("42".into(), true));
        assert_eq!(convert("12304560", 2, 10), ("4".into(), true));
        assert_eq!(convert("", 36, 2), ("0".into(), false));
    }

    #[test]
    fn preserves_php_integer_and_float_conversion_boundaries() {
        assert_eq!(
            convert("9223372036854775807", 10, 16),
            ("7fffffffffffffff".into(), false)
        );
        assert_eq!(
            convert("9223372036854775808", 10, 10),
            ("9223372036854776028".into(), false)
        );
        assert_eq!(
            convert("ffffffffffffffff", 16, 10),
            ("18446744073709552046".into(), false)
        );
        assert_eq!(
            parse_base_convert_number(&vec![b'1'; 2000], 2).0,
            BaseConvertNumber::Float(f64::INFINITY)
        );
    }
}

// ============================================================================
// Date/Time functions
// ============================================================================

/// date($format, $timestamp = time()): string
fn fn_date(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let fmt = arg_str!(ed, 0);
    let ts = match arg_opt!(ed, 1) {
        Some(v) if !v.is_undef() => v.to_long_val(),
        _ => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    };
    ret!(rv, Value::string(format_php_date(&fmt, ts, "UTC")));
}

/// gmdate($format, $timestamp = time()): string
fn fn_gmdate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let fmt = arg_str!(ed, 0);
    let ts = match arg_opt!(ed, 1) {
        Some(v) if !v.is_undef() => v.to_long_val(),
        _ => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    };
    ret!(rv, Value::string(format_php_date(&fmt, ts, "GMT")));
}

/// Format a Unix timestamp according to PHP date() format characters
fn format_php_date(fmt: &str, ts: i64, timezone_abbreviation: &str) -> String {
    // Break timestamp into components using manual calculation (no chrono dependency)
    let (year, month, day, hour, min, sec, wday, yday) = unix_to_parts(ts);
    let mut out = String::new();
    let mut escape = false;
    for c in fmt.chars() {
        if escape {
            out.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' => {
                escape = true;
            }
            'Y' => out.push_str(&format!("{:04}", year)),
            'y' => out.push_str(&format!("{:02}", year % 100)),
            'm' => out.push_str(&format!("{:02}", month)),
            'n' => out.push_str(&format!("{}", month)),
            'd' => out.push_str(&format!("{:02}", day)),
            'j' => out.push_str(&format!("{}", day)),
            'H' => out.push_str(&format!("{:02}", hour)),
            'G' => out.push_str(&format!("{}", hour)),
            'i' => out.push_str(&format!("{:02}", min)),
            's' => out.push_str(&format!("{:02}", sec)),
            'g' => {
                let h = if hour == 0 {
                    12
                } else if hour > 12 {
                    hour - 12
                } else {
                    hour
                };
                out.push_str(&format!("{}", h));
            }
            'h' => {
                let h = if hour == 0 {
                    12
                } else if hour > 12 {
                    hour - 12
                } else {
                    hour
                };
                out.push_str(&format!("{:02}", h));
            }
            'A' => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            'a' => out.push_str(if hour < 12 { "am" } else { "pm" }),
            'N' => out.push_str(&format!("{}", if wday == 0 { 7 } else { wday })),
            'w' => out.push_str(&format!("{}", wday)),
            'z' => out.push_str(&format!("{}", yday)),
            'U' => out.push_str(&format!("{}", ts)),
            'T' => out.push_str(timezone_abbreviation),
            'D' => out.push_str(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][wday as usize]),
            'l' => out.push_str(
                [
                    "Sunday",
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                ][wday as usize],
            ),
            'F' => out.push_str(
                [
                    "",
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December",
                ][month as usize],
            ),
            'M' => out.push_str(
                [
                    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
                    "Nov", "Dec",
                ][month as usize],
            ),
            't' => {
                let days = days_in_month(year, month);
                out.push_str(&format!("{}", days));
            }
            'L' => out.push_str(if is_leap_year(year) { "1" } else { "0" }),
            _ => out.push(c),
        }
    }
    out
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Convert Unix timestamp to (year, month, day, hour, min, sec, weekday, yearday)
fn unix_to_parts(ts: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let sec = ((ts % 60) + 60) % 60;
    let total_min = if ts < 0 { (ts - 59) / 60 } else { ts / 60 };
    let min = ((total_min % 60) + 60) % 60;
    let total_hours = if total_min < 0 {
        (total_min - 59) / 60
    } else {
        total_min / 60
    };
    let hour = ((total_hours % 24) + 24) % 24;
    let mut days = if total_hours < 0 {
        (total_hours - 23) / 24
    } else {
        total_hours / 24
    };

    // weekday: 1970-01-01 was Thursday (4)
    let wday = ((days % 7 + 4) % 7 + 7) % 7;

    // Civil date from days since epoch
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    days += 719468; // shift to 0000-03-01
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    // Day of year
    let month_days: [i64; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut yday = d - 1;
    for i in 0..(m - 1) as usize {
        yday += month_days[i];
    }

    (year, m, d, hour, min, sec, wday, yday)
}

/// mktime($hour, $minute, $second, $month, $day, $year): int|false
fn fn_mktime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hour = arg_long!(ed, 0);
    let min = arg_opt!(ed, 1).map(|v| v.to_long_val()).unwrap_or(0);
    let sec = arg_opt!(ed, 2).map(|v| v.to_long_val()).unwrap_or(0);
    let month = arg_opt!(ed, 3).map(|v| v.to_long_val()).unwrap_or(1);
    let day = arg_opt!(ed, 4).map(|v| v.to_long_val()).unwrap_or(1);
    let year = arg_opt!(ed, 5).map(|v| v.to_long_val()).unwrap_or(1970);

    ret!(
        rv,
        Value::long(parts_to_unix(year, month, day, hour, min, sec))
    );
}

fn parts_to_unix(year: i64, month: i64, day: i64, hour: i64, min: i64, sec: i64) -> i64 {
    // Days from 1970-01-01 to the given date
    let m = if month > 2 { month } else { month + 12 };
    let y = if month > 2 { year } else { year - 1 };
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + day - 719469;
    days * 86400 + hour * 3600 + min * 60 + sec
}

// ============================================================================
// Misc missing functions
// ============================================================================

/// getenv($name): string|false
fn fn_getenv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    match std::env::var(name.as_ref()) {
        Ok(val) => ret!(rv, Value::string(val)),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// putenv($assignment): bool
fn fn_putenv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    if let Some(pos) = s.find('=') {
        let (key, val) = s.split_at(pos);
        unsafe {
            std::env::set_var(key, &val[1..]);
        }
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

/// php_uname($mode = "a"): string
fn fn_php_uname(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mode = arg_opt!(ed, 0)
        .map(|v| v.echo_to_string().chars().next().unwrap_or('a'))
        .unwrap_or('a');
    let result = match mode {
        's' => std::env::consts::OS.to_string(),
        'r' => "rphp".to_string(),
        'm' => std::env::consts::ARCH.to_string(),
        _ => format!(
            "{} {} {}",
            std::env::consts::OS,
            "rphp",
            std::env::consts::ARCH
        ),
    };
    ret!(rv, Value::string(result));
}

/// php_sapi_name(): string
fn fn_php_sapi_name(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string("cli".to_string()));
}

/// zend_version(): string
fn fn_zend_version(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::string(format!(
            "4.{}.{}",
            crate::PHP_COMPAT_MINOR_VERSION,
            crate::PHP_COMPAT_RELEASE_VERSION
        ))
    );
}

/// phpversion(): string
fn fn_phpversion(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg_opt!(ed, 0).is_some() {
        ret!(rv, Value::bool(false));
    }
    ret!(rv, Value::string(crate::PHP_COMPAT_VERSION));
}

fn normalize_version(version: &str) -> Vec<String> {
    let mut normalized = String::with_capacity(version.len() * 2);
    let mut previous: Option<char> = None;
    for mut ch in version.chars() {
        if matches!(ch, '-' | '_' | '+') {
            ch = '.';
        }
        if let Some(prev) = previous {
            if ch != '.' && prev != '.' && ch.is_ascii_digit() != prev.is_ascii_digit() {
                normalized.push('.');
            }
        }
        if ch == '.' {
            if !normalized.ends_with('.') {
                normalized.push('.');
            }
        } else {
            normalized.push(ch);
        }
        previous = Some(ch);
    }
    normalized
        .trim_matches('.')
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn version_special_rank(part: &str) -> i8 {
    match part.to_ascii_lowercase().as_str() {
        "dev" => -6,
        "alpha" | "a" => -5,
        "beta" | "b" => -4,
        "rc" => -3,
        "#" => 0,
        "pl" | "p" => 1,
        _ => -7,
    }
}

fn compare_numeric_version_parts(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len()
        .cmp(&right.len())
        .then_with(|| left.as_bytes().cmp(right.as_bytes()))
}

fn compare_version_parts(left: Option<&str>, right: Option<&str>) -> std::cmp::Ordering {
    let left = left.unwrap_or("0");
    let right = right.unwrap_or("0");
    match (
        left.bytes().all(|byte| byte.is_ascii_digit()),
        right.bytes().all(|byte| byte.is_ascii_digit()),
    ) {
        (true, true) => compare_numeric_version_parts(left, right),
        (true, false) => version_special_rank("#").cmp(&version_special_rank(right)),
        (false, true) => version_special_rank(left).cmp(&version_special_rank("#")),
        (false, false) => version_special_rank(left).cmp(&version_special_rank(right)),
    }
}

fn php_version_compare(left: &str, right: &str) -> std::cmp::Ordering {
    let left = normalize_version(left);
    let right = normalize_version(right);
    let count = left.len().max(right.len());
    for index in 0..count {
        let ordering = compare_version_parts(
            left.get(index).map(String::as_str),
            right.get(index).map(String::as_str),
        );
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

fn fn_version_compare(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let left = arg_str!(ed, 0);
    let right = arg_str!(ed, 1);
    let ordering = php_version_compare(&left, &right);
    let Some(operator) = arg_opt!(ed, 2) else {
        let result = match ordering {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        ret!(rv, Value::long(result));
    };
    let operator = operator.echo_to_string();
    let result = match operator.as_str() {
        "<" | "lt" => ordering.is_lt(),
        "<=" | "le" => ordering.is_le(),
        ">" | "gt" => ordering.is_gt(),
        ">=" | "ge" => ordering.is_ge(),
        "=" | "==" | "eq" => ordering.is_eq(),
        "!=" | "<>" | "ne" => ordering.is_ne(),
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "version_compare(): Argument #3 ($operator) must be a valid comparison operator",
            ));
            ret!(rv, Value::null());
        }
    };
    ret!(rv, Value::bool(result));
}

/// Portable locale subset. Unsupported host locales return false, allowing
/// callers and PHPT setup sections to detect the unavailable capability.
enum SetLocaleCandidate {
    Query,
    Name(Value),
}

enum SetLocaleArgument {
    Scalar(SetLocaleCandidate),
    Array(Vec<Value>),
}

fn setlocale_scalar_candidate(
    eg: &mut ExecutorGlobals,
    argument: &Value,
    strict: bool,
    position: usize,
    parameter: &str,
) -> Result<Option<SetLocaleCandidate>, VmError> {
    let argument = argument.dereferenced();
    let converted = match argument.value_type() {
        ValueType::Null => return Ok(Some(SetLocaleCandidate::Query)),
        ValueType::String => argument.clone(),
        ValueType::False if !strict => Value::string(String::new()),
        ValueType::True if !strict => Value::string("1"),
        ValueType::Long | ValueType::Double if !strict => {
            Value::string(argument.echo_to_string_with_precision(eg.precision))
        }
        ValueType::Object if !strict => {
            let rendered = crate::vm::execute::call_object_string_conversion(eg, argument)?;
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(rendered) = rendered else {
                typed_internal_argument_error(
                    eg,
                    "setlocale",
                    argument,
                    position,
                    parameter,
                    "array|string|null",
                );
                return Ok(None);
            };
            let rendered = rendered.dereferenced();
            match rendered.value_type() {
                ValueType::String => rendered.clone(),
                ValueType::Long | ValueType::Double | ValueType::True | ValueType::False => {
                    Value::string(rendered.echo_to_string_with_precision(eg.precision))
                }
                _ => {
                    let class_name = argument.diagnostic_type_name();
                    let actual = rendered.diagnostic_type_name();
                    eg.exception = Some(crate::value::make_error_value(
                        "TypeError",
                        &format!(
                            "{class_name}::__toString(): Return value must be of type string, {actual} returned"
                        ),
                    ));
                    return Ok(None);
                }
            }
        }
        _ => {
            typed_internal_argument_error(
                eg,
                "setlocale",
                argument,
                position,
                parameter,
                "array|string|null",
            );
            return Ok(None);
        }
    };
    let bytes = converted.php_string_bytes().unwrap_or_default();
    if bytes.as_ref() == b"0" {
        Ok(Some(SetLocaleCandidate::Query))
    } else {
        Ok(Some(SetLocaleCandidate::Name(converted)))
    }
}

fn setlocale_try_name(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    locale: &Value,
) -> Result<Option<Value>, VmError> {
    let bytes = locale.php_string_bytes().unwrap_or_default();
    if bytes.len() >= 255 {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "setlocale(): Specified locale name is too long",
        )?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        return Ok(None);
    }
    if bytes.as_ref() == b"C" || bytes.eq_ignore_ascii_case(b"POSIX") {
        return Ok(Some(Value::string("C")));
    }
    Ok(None)
}

fn setlocale_normalize_argument(
    eg: &mut ExecutorGlobals,
    argument: &Value,
    strict: bool,
    position: usize,
    parameter: &str,
) -> Result<Option<SetLocaleArgument>, VmError> {
    if let Some(locales) = argument.dereferenced().as_array() {
        return Ok(Some(SetLocaleArgument::Array(
            locales.iter().map(|(_, locale)| locale.clone()).collect(),
        )));
    }

    let Some(candidate) = setlocale_scalar_candidate(eg, argument, strict, position, parameter)?
    else {
        return Ok(None);
    };
    Ok(Some(SetLocaleArgument::Scalar(candidate)))
}

fn setlocale_try_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    argument: &SetLocaleArgument,
    position: usize,
    parameter: &str,
) -> Result<Option<Value>, VmError> {
    match argument {
        SetLocaleArgument::Scalar(SetLocaleCandidate::Query) => Ok(Some(Value::string("C"))),
        SetLocaleArgument::Scalar(SetLocaleCandidate::Name(locale)) => {
            setlocale_try_name(ed, eg, locale)
        }
        SetLocaleArgument::Array(locales) => {
            for locale in locales {
                let Some(candidate) =
                    setlocale_scalar_candidate(eg, locale, false, position, parameter)?
                else {
                    return Ok(None);
                };
                match candidate {
                    SetLocaleCandidate::Query => return Ok(Some(Value::string("C"))),
                    SetLocaleCandidate::Name(locale) => {
                        if let Some(result) = setlocale_try_name(ed, eg, &locale)? {
                            return Ok(Some(result));
                        }
                        if eg.exception.is_some() {
                            return Ok(None);
                        }
                    }
                }
            }
            Ok(None)
        }
    }
}

fn fn_setlocale(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if arg!(ed, 0).value_type() == ValueType::Long {
        let first = arg!(ed, 1);
        if arg!(ed, 2).as_array().is_some_and(PhpArray::is_empty) {
            if first.value_type() == ValueType::Null {
                ret!(rv, Value::string("C"));
            }
            if first.value_type() == ValueType::String {
                let bytes = first.php_string_bytes().unwrap_or_default();
                if bytes.as_ref() == b"0"
                    || bytes.as_ref() == b"C"
                    || bytes.eq_ignore_ascii_case(b"POSIX")
                {
                    ret!(rv, Value::string("C"));
                }
            }
        }
    } else if typed_internal_int_argument(ed, eg, "setlocale", 0, "category")?.is_none() {
        return Ok(());
    }

    let strict = internal_call_is_strict(ed);
    let first = owned_argument(ed, 1);
    let Some(first) = setlocale_normalize_argument(eg, &first, strict, 2, "locales")? else {
        return Ok(());
    };

    let mut normalized = Vec::new();
    let rest = owned_argument(ed, 2);
    if let Some(rest) = rest.as_array() {
        let rest = rest
            .iter()
            .map(|(_, locale)| locale.clone())
            .collect::<Vec<_>>();
        for (index, locale) in rest.iter().enumerate() {
            let Some(locale) = setlocale_normalize_argument(eg, locale, strict, index + 3, "")?
            else {
                return Ok(());
            };
            normalized.push(locale);
        }
    }

    if let Some(result) = setlocale_try_argument(ed, eg, &first, 2, "locales")? {
        ret!(rv, result);
    }
    if eg.exception.is_some() {
        return Ok(());
    }
    for (index, locale) in normalized.iter().enumerate() {
        if let Some(result) = setlocale_try_argument(ed, eg, locale, index + 3, "")? {
            ret!(rv, result);
        }
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::bool(false));
}

/// RPHP does not advertise an extension until its compatibility contract is
/// separately admitted. Composer can therefore reject unsupported packages
/// instead of selecting code for a partially implemented extension.
fn fn_extension_loaded(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(false));
}

/// CLI execution has no response-header transport. These minimal contracts
/// support Composer's generated platform failure path without pretending to
/// publish HTTP headers.
fn fn_headers_sent(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(false));
}

fn fn_header(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}

/// Resolve the last startup value exactly once before compiling the request.
/// Invalid values retain PHP's ordinary enabled default until their separate
/// startup-diagnostic contract is admitted.
pub fn startup_zend_assertions(settings: &[(String, String)]) -> i8 {
    settings
        .iter()
        .rev()
        .find(|(name, _)| name.eq_ignore_ascii_case("zend.assertions"))
        .and_then(|(_, value)| value.parse::<i8>().ok())
        .filter(|value| (-1..=1).contains(value))
        .unwrap_or(1)
}

/// Resolve the request-startup precision before source compilation. Constant
/// comparisons use this value, while later `ini_set()` calls affect only
/// expressions that remain dynamic at runtime.
pub fn startup_precision(settings: &[(String, String)]) -> i32 {
    settings
        .iter()
        .rev()
        .find(|(name, _)| name.eq_ignore_ascii_case("precision"))
        .and_then(|(_, value)| parse_precision_ini(value))
        .unwrap_or(14)
}

/// Apply the admitted request-startup INI subset after compilation. Unknown
/// CLI definitions remain accepted by the CLI but are not published through
/// `ini_get()` until their observable runtime contract is implemented.
pub fn apply_startup_ini_settings(eg: &mut ExecutorGlobals, settings: &[(String, String)]) {
    let zend_assertions = startup_zend_assertions(settings);
    eg.assertion_state.startup_mode = zend_assertions;
    eg.assertion_state.active = zend_assertions > 0;

    for (name, value) in settings {
        let normalized = name.to_ascii_lowercase();
        match normalized.as_str() {
            "zend.assertions" => {
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, zend_assertions.to_string());
            }
            "assert.exception" => {
                eg.assertion_state.exception = ini_boolean(value);
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, value.clone());
            }
            "error_reporting" => {
                let (published, level) = normalize_error_reporting_ini(value);
                eg.set_error_reporting(level);
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, published);
            }
            "precision" => {
                if let Some(precision) = parse_precision_ini(value) {
                    eg.precision = precision;
                    eg.ini_overrides
                        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                        .insert(normalized, value.clone());
                } else {
                    eg.precision = 14;
                    eg.ini_overrides
                        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                        .insert(normalized, "14".to_string());
                }
            }
            "serialize_precision" => {
                if let Some(precision) = parse_precision_ini(value) {
                    eg.serialize_precision = precision;
                    eg.ini_overrides
                        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                        .insert(normalized, value.clone());
                } else {
                    eg.serialize_precision = -1;
                    eg.ini_overrides
                        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                        .insert(normalized, "-1".to_string());
                }
            }
            "zend.exception_ignore_args" => {
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, normalize_ini_boolean_value(value));
            }
            "zend.exception_string_param_max_len" => {
                let published = normalize_exception_string_param_max_len(value);
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, published);
            }
            "highlight.string" | "highlight.comment" | "highlight.keyword"
            | "highlight.default" | "highlight.html" => {
                eg.ini_overrides
                    .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
                    .insert(normalized, value.clone());
            }
            _ => {}
        }
    }
}

fn normalize_error_reporting_ini(value: &str) -> (String, i64) {
    let value = value.trim();
    if let Some(level) = parse_ini::evaluate_ini_integer_expression(value) {
        return (level.to_string(), level);
    }
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" => ("1".to_string(), 1),
        "false" | "off" | "no" | "none" | "" => (String::new(), 0),
        _ => (value.to_string(), 0),
    }
}

fn parse_precision_ini(value: &str) -> Option<i32> {
    let value = value.trim_start().as_bytes();
    let (negative, digits) = match value {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, value),
    };
    let mut parsed = 0i64;
    for digit in digits.iter().copied().take_while(u8::is_ascii_digit) {
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(i64::from(digit - b'0'));
    }
    if negative {
        parsed = -parsed;
    }
    if parsed < -1 {
        return None;
    }
    i32::try_from(parsed).ok()
}

fn normalize_ini_boolean_value(value: &str) -> String {
    let value = value.trim();
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" => "1".to_string(),
        "false" | "off" | "no" | "none" => String::new(),
        _ => value.to_string(),
    }
}

fn normalize_exception_string_param_max_len(value: &str) -> String {
    let value = normalize_ini_boolean_value(value);
    match value.parse::<i64>() {
        Ok(length) if (0..=1_000_000).contains(&length) => length.to_string(),
        Ok(_) => "15".to_string(),
        Err(_) => value,
    }
}

fn fn_ini_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg_str!(ed, 0);
    let normalized = option.to_ascii_lowercase();
    if let Some(value) = eg
        .ini_overrides
        .as_deref()
        .and_then(|overrides| overrides.get(&normalized))
    {
        ret!(rv, Value::string(value.clone()));
    }
    if option.eq_ignore_ascii_case("display_errors") {
        ret!(rv, Value::string("1"));
    }
    if option.eq_ignore_ascii_case("zend.enable_gc") {
        ret!(rv, Value::string(if eg.gc_enabled { "1" } else { "0" }));
    }
    if option.eq_ignore_ascii_case("precision") {
        ret!(rv, Value::string(eg.precision.to_string()));
    }
    if option.eq_ignore_ascii_case("serialize_precision") {
        ret!(rv, Value::string(eg.serialize_precision.to_string()));
    }
    ret!(rv, Value::bool(false));
}

pub(crate) fn ini_default(eg: &ExecutorGlobals, option: &str) -> Option<String> {
    if let Some(value) = eg
        .ini_overrides
        .as_deref()
        .and_then(|overrides| overrides.get(option))
    {
        return Some(value.clone());
    }
    Some(match option {
        "display_errors" | "report_memleaks" => "1".to_string(),
        "zend.assertions" => eg.assertion_state.startup_mode.to_string(),
        "assert.exception" => if eg.assertion_state.exception {
            "1"
        } else {
            "0"
        }
        .to_string(),
        "zend.exception_ignore_args" => "0".to_string(),
        "precision" => eg.precision.to_string(),
        "serialize_precision" => eg.serialize_precision.to_string(),
        "zend.enable_gc" => if eg.gc_enabled { "1" } else { "0" }.to_string(),
        "memory_limit" => "-1".to_string(),
        "zend.exception_string_param_max_len" => "15".to_string(),
        "fiber.stack_size" => "2097152".to_string(),
        "highlight.string" => "#DD0000".to_string(),
        "highlight.comment" => "#FF8000".to_string(),
        "highlight.keyword" => "#007700".to_string(),
        "highlight.default" => "#0000BB".to_string(),
        "highlight.html" => "#000000".to_string(),
        _ => return None,
    })
}

pub(crate) fn ini_boolean(value: &str) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" => true,
        "" | "false" | "off" | "no" | "none" => false,
        value => value.parse::<i64>().is_ok_and(|value| value != 0),
    }
}

pub(crate) fn exception_string_param_max_len(eg: &ExecutorGlobals) -> usize {
    ini_default(eg, "zend.exception_string_param_max_len")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value <= 1_000_000)
        .unwrap_or(0)
}

fn fn_ini_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg_str!(ed, 0).to_ascii_lowercase();
    let value = arg!(ed, 1).echo_to_string_with_precision(eg.precision);
    let Some(previous) = ini_default(eg, &option) else {
        ret!(rv, Value::bool(false));
    };

    if option == "zend.assertions" {
        let Some(requested) = value
            .parse::<i8>()
            .ok()
            .filter(|value| (-1..=1).contains(value))
        else {
            ret!(rv, Value::bool(false));
        };
        if (eg.assertion_state.startup_mode < 0) != (requested < 0) {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "zend.assertions may be completely enabled or disabled only in php.ini",
            )?;
            ret!(rv, Value::bool(false));
        }
        eg.assertion_state.active = requested > 0;
        eg.ini_overrides
            .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
            .insert(option, requested.to_string());
        ret!(rv, Value::string(previous));
    }

    if option == "precision" {
        let Some(precision) = parse_precision_ini(&value) else {
            ret!(rv, Value::bool(false));
        };
        eg.precision = precision;
        eg.ini_overrides
            .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
            .insert(option, value);
        ret!(rv, Value::string(previous));
    }

    if option == "serialize_precision" {
        let Some(precision) = parse_precision_ini(&value) else {
            ret!(rv, Value::bool(false));
        };
        eg.serialize_precision = precision;
        eg.ini_overrides
            .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
            .insert(option, value);
        ret!(rv, Value::string(previous));
    }

    if option == "zend.exception_string_param_max_len"
        && !value
            .parse::<i64>()
            .is_ok_and(|value| (0..=1_000_000).contains(&value))
    {
        ret!(rv, Value::bool(false));
    }
    if option == "fiber.stack_size" && value.parse::<i64>().is_ok_and(|value| value < 0) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "fiber.stack_size must be a positive number",
        )?;
        ret!(rv, Value::bool(false));
    }
    if option == "zend.enable_gc" {
        eg.gc_enabled = ini_boolean(&value);
    }
    if option == "assert.exception" {
        eg.assertion_state.exception = ini_boolean(&value);
    }
    eg.ini_overrides
        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
        .insert(option, value);
    ret!(rv, Value::string(previous));
}

fn fn_gc_enabled(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(eg.gc_enabled));
}

fn fn_gc_enable(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.gc_enabled = true;
    Ok(())
}

fn fn_gc_disable(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.gc_enabled = false;
    Ok(())
}

fn fn_gc_collect_cycles(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let collected = eg.collect_cycles()?;
    ret!(rv, Value::long(collected as i64));
}

fn fn_gc_status(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let status = crate::value::cycle_collection_status();
    let mut result = PhpArray::new();
    result.set_str("running", Value::bool(status.running));
    result.set_str("protected", Value::bool(false));
    result.set_str("full", Value::bool(false));
    result.set_str(
        "runs",
        Value::long(i64::try_from(status.runs).unwrap_or(i64::MAX)),
    );
    result.set_str(
        "collected",
        Value::long(i64::try_from(status.collected).unwrap_or(i64::MAX)),
    );
    result.set_str("threshold", Value::long(10_001));
    result.set_str("buffer_size", Value::long(16_384));
    result.set_str(
        "roots",
        Value::long(i64::try_from(status.roots).unwrap_or(i64::MAX)),
    );
    result.set_str("application_time", Value::double(status.application_time));
    result.set_str("collector_time", Value::double(status.collector_time));
    result.set_str("destructor_time", Value::double(status.destructor_time));
    result.set_str("free_time", Value::double(status.free_time));
    ret!(rv, Value::array(result));
}

/// PHP_INT_SIZE, PHP_INT_MAX etc. are handled as constants.
fn fn_set_time_limit(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(seconds) = typed_internal_int_argument(ed, eg, "set_time_limit", 0, "seconds")? else {
        return Ok(());
    };
    ret!(rv, Value::bool(eg.set_execution_time_limit(seconds)));
}

/// sleep($seconds): int
fn fn_sleep(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let secs = arg_long!(ed, 0);
    if secs > 0 {
        std::thread::sleep(std::time::Duration::from_secs(secs as u64));
    }
    ret!(rv, Value::long(0));
}

/// usleep($microseconds): void
fn fn_usleep(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let us = arg_long!(ed, 0);
    if us > 0 {
        std::thread::sleep(std::time::Duration::from_micros(us as u64));
    }
    Ok(())
}

/// array_key_first($array): int|string|null
fn fn_array_key_first(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_val = arg!(ed, 0);
    if let Some(arr) = arr_val.as_array() {
        if let Some((k, _)) = arr.iter().next() {
            match k {
                ArrayKey::Int(i) => ret!(rv, Value::long(i)),
                ArrayKey::String(s) => ret!(rv, Value::string(s.clone())),
            }
        }
    }
    ret!(rv, Value::null());
}

fn array_cursor_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    select: impl FnOnce(&PhpArray) -> Option<&Value>,
) -> Result<(), VmError> {
    let value = arg!(ed, 0)
        .as_array()
        .and_then(select)
        .cloned()
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, value);
}

#[derive(Clone, Copy)]
enum ObjectCursorOperation {
    Reset,
    End,
    Current,
    Next,
    Prev,
    Key,
}

fn object_cursor_entries(eg: &ExecutorGlobals, value: &Value) -> Vec<(String, Value)> {
    let object = value
        .as_object()
        .expect("object cursor is entered only for an object");
    let mut entries = Vec::new();
    if let Some(class) = eg.class_by_id(object.class_id) {
        for slot in eg.instance_property_slots_in_iteration_order(object.class_id) {
            let Some(value) = object.get_property_slot(slot) else {
                continue;
            };
            if value.value_type() == ValueType::Undef {
                continue;
            }
            let definition = &class.properties[slot];
            let key = match definition.visibility {
                Visibility::Public => definition.name.clone(),
                Visibility::Protected => format!("\0*\0{}", definition.name),
                Visibility::Private => {
                    format!("\0{}\0{}", definition.declaring_class, definition.name)
                }
            };
            entries.push((key, value.dereferenced().clone()));
        }
    }
    object.for_each_dynamic_property(|key, value| {
        if value.value_type() != ValueType::Undef {
            entries.push((key.to_string(), value.dereferenced().clone()));
        }
    });
    entries
}

fn object_cursor_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    operation: ObjectCursorOperation,
) -> Result<(), VmError> {
    report_internal_deprecation(
        eg,
        ed,
        &format!("{function}(): Calling {function}() on an object is deprecated"),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }

    let argument = arg!(ed, 0);
    let entries = object_cursor_entries(eg, argument);
    let state = argument
        .as_object()
        .and_then(|object| object.object_cursor());
    let current = match state {
        None => (!entries.is_empty()).then_some(0),
        Some(position) => position.filter(|position| *position < entries.len()),
    };
    let selected = match operation {
        ObjectCursorOperation::Reset => (!entries.is_empty()).then_some(0),
        ObjectCursorOperation::End => entries.len().checked_sub(1),
        ObjectCursorOperation::Current | ObjectCursorOperation::Key => current,
        ObjectCursorOperation::Next => current
            .and_then(|position| position.checked_add(1))
            .filter(|position| *position < entries.len()),
        ObjectCursorOperation::Prev => current.and_then(|position| position.checked_sub(1)),
    };
    if let Some(mut object) = argument.as_object_mut() {
        object.set_object_cursor(selected);
    }

    if matches!(operation, ObjectCursorOperation::Key) {
        if let Some((key, _)) = selected.and_then(|position| entries.get(position)) {
            ret!(rv, Value::string(key));
        }
        ret!(rv, Value::null());
    }
    let value = selected
        .and_then(|position| entries.get(position))
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, value);
}

fn cursor_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    operation: ObjectCursorOperation,
    select: impl FnOnce(&PhpArray) -> Option<&Value>,
) -> Result<(), VmError> {
    let argument = arg!(ed, 0);
    if argument.as_array().is_some() {
        return array_cursor_value(ed, rv, select);
    }
    if argument.as_object().is_some() {
        return object_cursor_value(ed, rv, eg, function, operation);
    }
    typed_internal_argument_error(eg, function, argument, 1, "array", "array|object");
    Ok(())
}

fn fn_reset(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(
        ed,
        rv,
        eg,
        "reset",
        ObjectCursorOperation::Reset,
        PhpArray::cursor_reset,
    )
}

fn fn_end(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(
        ed,
        rv,
        eg,
        "end",
        ObjectCursorOperation::End,
        PhpArray::cursor_end,
    )
}

fn fn_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    cursor_value(
        ed,
        rv,
        eg,
        "current",
        ObjectCursorOperation::Current,
        PhpArray::cursor_current,
    )
}

fn fn_next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(
        ed,
        rv,
        eg,
        "next",
        ObjectCursorOperation::Next,
        PhpArray::cursor_next,
    )
}

fn fn_prev(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(
        ed,
        rv,
        eg,
        "prev",
        ObjectCursorOperation::Prev,
        PhpArray::cursor_prev,
    )
}

/// key($array): int|string|null for the array's current internal cursor.
fn fn_key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(key) = arg!(ed, 0).as_array().and_then(PhpArray::cursor_key) {
        match key {
            ArrayKey::Int(key) => ret!(rv, Value::long(key)),
            ArrayKey::String(key) => ret!(rv, Value::string(key)),
        }
    }
    if arg!(ed, 0).as_object().is_some() {
        return object_cursor_value(ed, rv, eg, "key", ObjectCursorOperation::Key);
    }
    if arg!(ed, 0).as_array().is_none() {
        typed_internal_argument_error(eg, "key", arg!(ed, 0), 1, "array", "array|object");
        return Ok(());
    }
    ret!(rv, Value::null());
}

/// array_key_last($array): int|string|null
fn fn_array_key_last(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_val = arg!(ed, 0);
    if let Some(arr) = arr_val.as_array() {
        if let Some((k, _)) = arr.iter().last() {
            match k {
                ArrayKey::Int(i) => ret!(rv, Value::long(i)),
                ArrayKey::String(s) => ret!(rv, Value::string(s.clone())),
            }
        }
    }
    ret!(rv, Value::null());
}

/// ctype_alpha($text): bool
fn fn_ctype_alpha(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic()))
    );
}

/// ctype_digit($text): bool
fn fn_ctype_digit(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    );
}

/// ctype_alnum($text): bool
fn fn_ctype_alnum(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
    );
}

/// ctype_space($text): bool
fn fn_ctype_space(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_whitespace()))
    );
}

/// ctype_upper($text): bool
fn fn_ctype_upper(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase()))
    );
}

/// ctype_lower($text): bool
fn fn_ctype_lower(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(!s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase()))
    );
}

// ============================================================================
// Compatibility pack: dynamic dispatch, type guards, URL/query, regex trio
// ============================================================================

/// call_user_func_array($callback, $args): mixed
fn fn_call_user_func_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let args_val = arg!(ed, 1);
    let resolved = match resolve_callback_at_callsite_checked(callback, eg, ed)? {
        Some(resolved) => resolved,
        None => {
            if eg.exception.is_some() {
                return Ok(());
            }
            let reason = ordinary_callback_invalid_reason(callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func_array(): Argument #1 ($callback) must be a valid callback, {reason}"
                ),
            ));
            return Ok(());
        }
    };
    if callback_has_hard_reference_parameters(&resolved)
        && let Some(arguments) = args_val.as_array()
    {
        let callback_name = callable_display_name(callback, eg);
        if !report_callback_reference_warnings(eg, ed, &resolved, arguments, false, &callback_name)?
        {
            return Ok(());
        }
    }
    let discarded = rv.is_null() || eg.detached_return_discarded();
    let previous_discarded = eg.replace_detached_return_discarded(discarded);
    let strict = internal_call_is_strict(ed);
    let result = with_detached_strict_call(ed, strict, || {
        invoke_resolved_call_user_func_array(resolved, args_val, eg)
    });
    eg.replace_detached_return_discarded(previous_discarded);
    let result = result?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, result);
}

fn user_execute_data_is_strict(ed: *mut ExecuteData) -> bool {
    if ed.is_null() {
        return false;
    }
    // SAFETY: callers supply the currently executing source frame or the
    // compiler-lowered call_user_func_array opcode's live logical caller.
    unsafe {
        if (*ed).func.is_null() {
            return false;
        }
        let function = Function::from_common_ptr((*ed).func);
        function.fn_type() == FunctionType::User && function.as_user().op_array.strict_types
    }
}

fn with_detached_strict_call<T>(
    caller: *mut ExecuteData,
    strict: bool,
    callback: impl FnOnce() -> Result<T, VmError>,
) -> Result<T, VmError> {
    if caller.is_null() {
        return callback();
    }
    // SAFETY: the caller activation remains live across this synchronous
    // callback dispatch. Restore its pre-existing call-kind bit afterwards so
    // adjacent engine callbacks keep their ordinary weak-call contract.
    unsafe {
        let previous = (*caller).is_detached_strict_call();
        (*caller).set_detached_strict_call(strict);
        let result = callback();
        (*caller).set_detached_strict_call(previous);
        result
    }
}

/// function_exists($name): bool
fn fn_function_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    let name = name.strip_prefix('\\').unwrap_or(&name);
    let exists = eg.find_function(name).is_some();
    ret!(rv, Value::bool(exists));
}

/// is_scalar($value): bool — true for int, float, string, bool
fn fn_is_scalar(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let val = arg!(ed, 0);
    let scalar = matches!(
        val.value_type(),
        ValueType::Long
            | ValueType::Double
            | ValueType::String
            | ValueType::True
            | ValueType::False
    );
    ret!(rv, Value::bool(scalar));
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedUrl<'a> {
    scheme: Option<&'a str>,
    host: Option<&'a str>,
    port: Option<i64>,
    user: Option<&'a str>,
    pass: Option<&'a str>,
    path: Option<&'a str>,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

#[inline]
fn parse_url_port(port: &str) -> Option<i64> {
    if port.len() > 5 {
        return None;
    }
    let bytes = port.as_bytes();
    let mut position = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let negative = match bytes.get(position) {
        Some(b'+') => {
            position += 1;
            false
        }
        Some(b'-') => {
            position += 1;
            true
        }
        _ => false,
    };
    let start = position;
    let mut value = 0i64;
    while let Some(digit @ b'0'..=b'9') = bytes.get(position).copied() {
        value = value * 10 + i64::from(digit - b'0');
        position += 1;
    }
    (position > start && !negative && value <= 65_535).then_some(value)
}

/// PHP treats `name:123/path` as a schemeless authority when the decimal
/// field has at most five digits. A five-digit overflow is an invalid URL,
/// while six or more digits remain an opaque scheme path.
fn schemeless_port(after_colon: &str) -> Option<Result<i64, ()>> {
    let digit_count = after_colon.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 || digit_count > 5 {
        return None;
    }
    if !matches!(after_colon.as_bytes().get(digit_count), None | Some(b'/')) {
        return None;
    }
    Some(parse_url_port(&after_colon[..digit_count]).ok_or(()))
}

fn parse_url_parts(input: &str) -> Option<ParsedUrl<'_>> {
    let mut parsed = ParsedUrl::default();
    let mut rest = input;
    let has_authority;

    if let Some(protocol_relative) = rest.strip_prefix("//") {
        rest = protocol_relative;
        has_authority = true;
    } else if let Some(colon) = rest.find(':') {
        let candidate = &rest[..colon];
        let after_colon = &rest[colon + 1..];
        if let Some(port) = schemeless_port(after_colon) {
            port.ok()?;
            has_authority = true;
        } else if !candidate.is_empty()
            && candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
        {
            parsed.scheme = Some(candidate);
            if let Some(authority) = after_colon.strip_prefix("//") {
                rest = authority;
                has_authority = true;
            } else {
                rest = after_colon;
                has_authority = false;
            }
        } else {
            has_authority = false;
        }
    } else {
        has_authority = false;
    }

    if let Some(index) = rest.find('#') {
        parsed.fragment = Some(&rest[index + 1..]);
        rest = &rest[..index];
    }
    if let Some(index) = rest.find('?') {
        parsed.query = Some(&rest[index + 1..]);
        rest = &rest[..index];
    }

    if !has_authority {
        if !rest.is_empty() || input.is_empty() {
            parsed.path = Some(rest);
        }
        return Some(parsed);
    }

    let (authority, path) = rest
        .find('/')
        .map_or((rest, None), |index| (&rest[..index], Some(&rest[index..])));
    parsed.path = path.map(|path| {
        if parsed.scheme == Some("file")
            && path.as_bytes().first() == Some(&b'/')
            && path.as_bytes().get(2) == Some(&b':')
        {
            &path[1..]
        } else {
            path
        }
    });

    let (userinfo, hostport) = authority.rfind('@').map_or((None, authority), |index| {
        (Some(&authority[..index]), &authority[index + 1..])
    });
    if let Some(userinfo) = userinfo {
        if let Some(index) = userinfo.find(':') {
            parsed.user = Some(&userinfo[..index]);
            parsed.pass = Some(&userinfo[index + 1..]);
        } else {
            parsed.user = Some(userinfo);
        }
    }

    if let Some(bracketed) = hostport.strip_prefix('[') {
        let close = bracketed.find(']')? + 1;
        parsed.host = Some(&hostport[..=close]);
        let trailing = &hostport[close + 1..];
        if let Some(port) = trailing.strip_prefix(':') {
            if !port.is_empty() {
                parsed.port = Some(parse_url_port(port)?);
            }
        } else if !trailing.is_empty() {
            return None;
        }
    } else if let Some(index) = hostport.rfind(':') {
        let host = &hostport[..index];
        let port = &hostport[index + 1..];
        if host.is_empty() {
            return None;
        }
        parsed.host = Some(host);
        if !port.is_empty() {
            parsed.port = Some(parse_url_port(port)?);
        }
    } else if !hostport.is_empty() {
        parsed.host = Some(hostport);
    }

    if parsed.host.is_none()
        && !(parsed.scheme == Some("file") && parsed.path.is_some() && userinfo.is_none())
    {
        return None;
    }
    Some(parsed)
}

#[cfg(test)]
mod parse_url_contract_tests {
    use super::{ParsedUrl, parse_url_parts};

    #[test]
    fn separates_empty_paths_schemeless_ports_and_opaque_schemes() {
        assert_eq!(
            parse_url_parts(""),
            Some(ParsedUrl {
                path: Some(""),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(
            parse_url_parts("host:80/path"),
            Some(ParsedUrl {
                host: Some("host"),
                port: Some(80),
                path: Some("/path"),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(
            parse_url_parts("host:999999"),
            Some(ParsedUrl {
                scheme: Some("host"),
                path: Some("999999"),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(parse_url_parts("host:65536/path"), None);
    }

    #[test]
    fn strips_empty_ports_and_validates_authorities() {
        assert_eq!(
            parse_url_parts("http://1.2.3.4:/path"),
            Some(ParsedUrl {
                scheme: Some("http"),
                host: Some("1.2.3.4"),
                path: Some("/path"),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(parse_url_parts("http://host:65536/path"), None);
        assert_eq!(
            parse_url_parts("x://::6.5"),
            Some(ParsedUrl {
                scheme: Some("x"),
                host: Some(":"),
                port: Some(6),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(parse_url_parts("http:///path"), None);
        assert_eq!(
            parse_url_parts("file:///path"),
            Some(ParsedUrl {
                scheme: Some("file"),
                path: Some("/path"),
                ..ParsedUrl::default()
            })
        );
        assert_eq!(
            parse_url_parts("file:///a:/"),
            Some(ParsedUrl {
                scheme: Some("file"),
                path: Some("a:/"),
                ..ParsedUrl::default()
            })
        );
    }
}

/// parse_url($url, $component = -1): mixed
fn fn_parse_url(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let url = arg_str!(ed, 0);
    let component = arg_opt!(ed, 1).map(|v| v.to_long_val()).unwrap_or(-1);
    let Some(parsed) = parse_url_parts(&url) else {
        ret!(rv, Value::bool(false));
    };
    if component > 7 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "parse_url(): Argument #2 ($component) must be a valid URL component identifier, {component} given"
            ),
        ));
        return Ok(());
    }
    let ParsedUrl {
        scheme,
        host,
        port,
        user,
        pass,
        path,
        query,
        fragment,
    } = parsed;

    // PHP_URL_* constants
    const PHP_URL_SCHEME: i64 = 0;
    const PHP_URL_HOST: i64 = 1;
    const PHP_URL_PORT: i64 = 2;
    const PHP_URL_USER: i64 = 3;
    const PHP_URL_PASS: i64 = 4;
    const PHP_URL_PATH: i64 = 5;
    const PHP_URL_QUERY: i64 = 6;
    const PHP_URL_FRAGMENT: i64 = 7;

    if component >= 0 {
        let val = match component {
            PHP_URL_SCHEME => scheme.map(Value::string),
            PHP_URL_HOST => host.map(Value::string),
            PHP_URL_PORT => port.map(Value::long),
            PHP_URL_USER => user.map(Value::string),
            PHP_URL_PASS => pass.map(Value::string),
            PHP_URL_PATH => path.map(Value::string),
            PHP_URL_QUERY => query.map(Value::string),
            PHP_URL_FRAGMENT => fragment.map(Value::string),
            _ => None,
        };
        ret!(rv, val.unwrap_or(Value::null()));
    }

    // Return associative array
    let mut arr = PhpArray::new();
    if let Some(v) = scheme {
        arr.set_str("scheme", Value::string(v));
    }
    if let Some(v) = host {
        arr.set_str("host", Value::string(v));
    }
    if let Some(v) = port {
        arr.set_str("port", Value::long(v));
    }
    if let Some(v) = user {
        arr.set_str("user", Value::string(v));
    }
    if let Some(v) = pass {
        arr.set_str("pass", Value::string(v));
    }
    if let Some(v) = path {
        arr.set_str("path", Value::string(v));
    }
    if let Some(v) = query {
        arr.set_str("query", Value::string(v));
    }
    if let Some(v) = fragment {
        arr.set_str("fragment", Value::string(v));
    }
    ret!(rv, Value::array(arr));
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

#[inline]
fn push_percent_escape(out: &mut String, byte: u8) {
    out.push('%');
    out.push(HEX_UPPER[(byte >> 4) as usize] as char);
    out.push(HEX_UPPER[(byte & 0x0f) as usize] as char);
}

#[inline]
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_php_bytes(bytes: &[u8], plus_as_space: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if plus_as_space && bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((high << 4) | low);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn percent_decode_bytes(value: &str, plus_as_space: bool) -> String {
    let decoded = percent_decode_php_bytes(value.as_bytes(), plus_as_space);
    match String::from_utf8(decoded) {
        Ok(decoded) => decoded,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

fn parse_str_normalize_key(key: &[u8], malformed_bracket: bool) -> Vec<u8> {
    let key = &key[key.iter().take_while(|byte| **byte == b' ').count()..];
    key.iter()
        .map(|byte| {
            if matches!(*byte, b'.' | b' ') || malformed_bracket && *byte == b'[' {
                b'_'
            } else {
                *byte
            }
        })
        .collect()
}

fn parse_str_key(bytes: &[u8]) -> ArrayKey {
    let storage = bytes_to_php_string(bytes);
    crate::value::canonical_decimal_array_key(&storage)
        .map_or_else(|| ArrayKey::String(storage), ArrayKey::Int)
}

/// Parse PHP's query-variable key boundary. A first unmatched `[` is a
/// top-level underscore, while an unmatched later segment is ignored after the
/// last complete segment. Bytes after a complete segment that do not start the
/// next adjacent segment are suffix data and are ignored.
fn parse_str_brackets(full_key: &[u8]) -> Option<(ArrayKey, Vec<Option<ArrayKey>>)> {
    let full_key = full_key.split(|byte| *byte == 0).next().unwrap_or_default();
    let leading_spaces = full_key.iter().take_while(|byte| **byte == b' ').count();
    let full_key = &full_key[leading_spaces..];
    if full_key.is_empty() {
        return None;
    }

    let Some(first_bracket) = full_key.iter().position(|byte| *byte == b'[') else {
        let key = parse_str_normalize_key(full_key, false);
        return (!key.is_empty()).then(|| (parse_str_key(&key), Vec::new()));
    };
    let base = parse_str_normalize_key(&full_key[..first_bracket], false);
    if base.is_empty() {
        return None;
    }

    let mut segments = Vec::new();
    let mut position = first_bracket;
    while position < full_key.len() && full_key[position] == b'[' {
        let Some(relative_close) = full_key[position + 1..]
            .iter()
            .position(|byte| *byte == b']')
        else {
            if segments.is_empty() {
                let key = parse_str_normalize_key(full_key, true);
                return Some((parse_str_key(&key), Vec::new()));
            }
            break;
        };
        let close = position + 1 + relative_close;
        let inner = &full_key[position + 1..close];
        segments.push(if inner.is_empty() {
            None
        } else {
            Some(parse_str_key(inner))
        });
        position = close + 1;
    }
    Some((parse_str_key(&base), segments))
}

fn parse_str_array_get<'a>(array: &'a PhpArray, key: &ArrayKey) -> Option<&'a Value> {
    match key {
        ArrayKey::Int(key) => array.get_int(*key),
        ArrayKey::String(key) => array.get_str(key),
    }
}

fn parse_str_array_set(array: &mut PhpArray, key: &ArrayKey, value: Value) {
    match key {
        ArrayKey::Int(key) => array.set_int(*key, value),
        ArrayKey::String(key) => {
            array.mark_external_byte_keys();
            array.set_str(key, value);
        }
    }
}

/// Recursively set a value in a nested PhpArray given a chain of bracket segments.
fn parse_str_set_nested(arr: &mut PhpArray, segments: &[Option<ArrayKey>], val: Value) {
    if segments.is_empty() {
        // Should not happen — caller handles the leaf case
        return;
    }
    let seg = &segments[0];
    let remaining = &segments[1..];

    if remaining.is_empty() {
        // Leaf: set or push
        match seg {
            None => {
                arr.push(val);
            }
            Some(key) => parse_str_array_set(arr, key, val),
        }
    } else {
        // Intermediate: get-or-create sub-array, then recurse
        match seg {
            None => {
                // [] at intermediate level: append a new sub-array entry
                let mut sub = PhpArray::new();
                parse_str_set_nested(&mut sub, remaining, val);
                arr.push(Value::array(sub));
            }
            Some(key) => {
                let mut sub = if let Some(existing) = parse_str_array_get(arr, key) {
                    existing.as_array().cloned().unwrap_or_else(PhpArray::new)
                } else {
                    PhpArray::new()
                };
                parse_str_set_nested(&mut sub, remaining, val);
                parse_str_array_set(arr, key, Value::array(sub));
            }
        }
    }
}

/// parse_str($string, &$result): void
/// Parses a URL-encoded query string into variables.
/// Supports recursive nesting (a[b][c]=1) and PHP key normalization (dots/spaces → _).
fn fn_parse_str(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "parse_str", 0, "string", "string")?
    else {
        return Ok(());
    };
    let input = input.php_string_bytes().unwrap_or_default();
    let out_ptr = arg_mut!(ed, 1);

    let mut arr = PhpArray::new();
    if !input.is_empty() {
        for pair in input.as_ref().split(|byte| *byte == b'&') {
            if pair.is_empty() {
                continue;
            }
            let (raw_key, value) = if let Some(index) = pair.iter().position(|byte| *byte == b'=') {
                (
                    percent_decode_php_bytes(&pair[..index], true),
                    percent_decode_php_bytes(&pair[index + 1..], true),
                )
            } else {
                (percent_decode_php_bytes(pair, true), Vec::new())
            };

            let Some((base, segments)) = parse_str_brackets(&raw_key) else {
                continue;
            };
            let value = php_byte_result(value, false);
            if segments.is_empty() {
                parse_str_array_set(&mut arr, &base, value);
            } else {
                let mut sub = if let Some(existing) = parse_str_array_get(&arr, &base) {
                    existing.as_array().cloned().unwrap_or_else(PhpArray::new)
                } else {
                    PhpArray::new()
                };
                parse_str_set_nested(&mut sub, &segments, value);
                parse_str_array_set(&mut arr, &base, Value::array(sub));
            }
        }
    }

    unsafe {
        std::ptr::drop_in_place(out_ptr);
        out_ptr.write(Value::array(arr));
    }
    ret!(rv, Value::null());
}

/// Helper: percent-encode a string for URL query
fn percent_encode_query(s: &str) -> String {
    let extra_bytes = s
        .bytes()
        .filter(|b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b' '))
        .count()
        * 2;
    let mut out = String::with_capacity(s.len() + extra_bytes);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => push_percent_escape(&mut out, b),
        }
    }
    out
}

/// http_build_query($data, $numeric_prefix = "", $arg_separator = "&"): string
fn fn_http_build_query(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let data = arg!(ed, 0);
    let prefix = arg_opt!(ed, 1)
        .map(|v| v.echo_to_string())
        .unwrap_or_default();
    let sep = arg_opt!(ed, 2)
        .map(|v| v.echo_to_string())
        .unwrap_or_else(|| "&".to_string());

    fn build_pairs(arr: &PhpArray, parent_key: &str, prefix: &str, pairs: &mut Vec<String>) {
        for (key, val) in arr.iter() {
            // PHP: null values are omitted entirely
            if val.value_type() == ValueType::Null {
                continue;
            }
            let key_str = match &key {
                ArrayKey::Int(i) => {
                    if parent_key.is_empty() {
                        format!("{}{}", prefix, i)
                    } else {
                        format!("{}[{}]", parent_key, i)
                    }
                }
                ArrayKey::String(s) => {
                    if parent_key.is_empty() {
                        s.clone()
                    } else {
                        format!("{}[{}]", parent_key, s)
                    }
                }
            };
            if let Some(sub_arr) = val.as_array() {
                build_pairs(&sub_arr, &key_str, prefix, pairs);
            } else {
                // PHP: booleans serialize as "1" / "0", not "1" / ""
                let v = match val.value_type() {
                    ValueType::True => "1".to_string(),
                    ValueType::False => "0".to_string(),
                    _ => val.echo_to_string(),
                };
                pairs.push(format!(
                    "{}={}",
                    percent_encode_query(&key_str),
                    percent_encode_query(&v)
                ));
            }
        }
    }

    if let Some(arr) = data.as_array() {
        let mut pairs = Vec::new();
        build_pairs(&arr, "", &prefix, &mut pairs);
        ret!(rv, Value::string(pairs.join(&sep)));
    }

    ret!(rv, Value::string(String::new()));
}

/// preg_match_all($pattern, $subject, &$matches = null, $flags = 0, $offset = 0): int
fn fn_preg_match_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);
    let flags = arg_opt!(ed, 3).map(|v| v.to_long_val()).unwrap_or(0);
    let offset_capture = flags & 256 != 0;

    let has_matches = {
        let raw = unsafe { (*ed).cv(2) };
        !raw.is_undef()
    };

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_) => {
            ret!(rv, Value::long(0));
        }
    };

    if !has_matches {
        ret!(rv, Value::long(re.count_matches(&subject) as i64));
    }

    if flags & 2 != 0 {
        // PREG_SET_ORDER — each top-level element represents one match and
        // contains the full match, capture groups, and named aliases.
        let mut out = PhpArray::new();
        let count: Result<usize, std::convert::Infallible> =
            re.try_visit_captures(&subject, |caps| {
                let mut row = PhpArray::new();
                // PHP omits trailing unmatched groups in PREG_SET_ORDER rows,
                // while retaining empty placeholders before a later match.
                let last_capture = (1..caps.len())
                    .rev()
                    .find(|&index| caps.get(index).is_some())
                    .unwrap_or(0);
                for index in 0..=last_capture {
                    let capture = match caps.get(index) {
                        Some(capture) if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(capture.as_str(&subject)));
                            pair.push(Value::long(capture.start as i64));
                            Value::array(pair)
                        }
                        Some(capture) => Value::string(capture.as_str(&subject)),
                        None if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(""));
                            pair.push(Value::long(-1));
                            Value::array(pair)
                        }
                        None => Value::string(""),
                    };
                    row.push(capture);
                }
                for (name, &index) in caps.named_groups() {
                    if index > last_capture {
                        continue;
                    }
                    let capture = match caps.get(index) {
                        Some(capture) if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(capture.as_str(&subject)));
                            pair.push(Value::long(capture.start as i64));
                            Value::array(pair)
                        }
                        Some(capture) => Value::string(capture.as_str(&subject)),
                        None if offset_capture => {
                            let mut pair = PhpArray::with_packed_capacity(2);
                            pair.push(Value::string(""));
                            pair.push(Value::long(-1));
                            Value::array(pair)
                        }
                        None => Value::string(""),
                    };
                    row.set_str(name, capture);
                }
                if let Some(mark) = caps.mark() {
                    row.set_str("MARK", Value::string(mark));
                }
                out.push(Value::array(row));
                Ok(true)
            });
        let count = count.unwrap();

        let matches_ptr = arg_mut!(ed, 2);
        unsafe {
            std::ptr::drop_in_place(matches_ptr);
            matches_ptr.write(Value::array(out));
        }
        ret!(rv, Value::long(count as i64));
    }

    // PHP default: PREG_PATTERN_ORDER — matches[0] contains every full
    // match, matches[1] every group 1 match, and so on. Fill those arrays
    // directly while the regex visitor lends each reusable capture buffer.
    let mut result_arrays: Option<Vec<PhpArray>> = None;
    let mut named_arrays: Vec<(String, usize, PhpArray)> = Vec::new();
    let mut marks = PhpArray::new();
    let count: Result<usize, std::convert::Infallible> = re.try_visit_captures(&subject, |caps| {
        if result_arrays.is_none() {
            result_arrays = Some((0..caps.len()).map(|_| PhpArray::new()).collect());
            named_arrays.extend(
                caps.named_groups()
                    .iter()
                    .map(|(name, &index)| (name.clone(), index, PhpArray::new())),
            );
        }

        let arrays = result_arrays.as_mut().unwrap();
        for (index, array) in arrays.iter_mut().enumerate() {
            let capture = match caps.get(index) {
                Some(capture) if offset_capture => {
                    let mut pair = PhpArray::with_packed_capacity(2);
                    pair.push(Value::string(capture.as_str(&subject)));
                    pair.push(Value::long(capture.start as i64));
                    Value::array(pair)
                }
                Some(capture) => Value::string(capture.as_str(&subject)),
                None if offset_capture => {
                    let mut pair = PhpArray::with_packed_capacity(2);
                    pair.push(Value::string(""));
                    pair.push(Value::long(-1));
                    Value::array(pair)
                }
                None => Value::string(""),
            };
            array.push(capture);
        }
        for (_, index, array) in &mut named_arrays {
            let capture = match caps.get(*index) {
                Some(capture) if offset_capture => {
                    let mut pair = PhpArray::with_packed_capacity(2);
                    pair.push(Value::string(capture.as_str(&subject)));
                    pair.push(Value::long(capture.start as i64));
                    Value::array(pair)
                }
                Some(capture) => Value::string(capture.as_str(&subject)),
                None if offset_capture => {
                    let mut pair = PhpArray::with_packed_capacity(2);
                    pair.push(Value::string(""));
                    pair.push(Value::long(-1));
                    Value::array(pair)
                }
                None => Value::string(""),
            };
            array.push(capture);
        }
        if let Some(mark) = caps.mark() {
            marks.push(Value::string(mark));
        }
        Ok(true)
    });
    let count = count.unwrap();

    let mut out = PhpArray::new();
    for array in result_arrays.unwrap_or_else(|| vec![PhpArray::new()]) {
        out.push(Value::array(array));
    }
    for (name, _, array) in named_arrays {
        out.set_str(&name, Value::array(array));
    }
    if !marks.is_empty() {
        out.set_str("MARK", Value::array(marks));
    }
    let matches_ptr = arg_mut!(ed, 2);
    unsafe {
        std::ptr::drop_in_place(matches_ptr);
        matches_ptr.write(Value::array(out));
    }

    ret!(rv, Value::long(count as i64));
}

/// preg_split($pattern, $subject, $limit = -1, $flags = 0): array|false
fn fn_preg_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);
    let limit = arg_opt!(ed, 2).map(|v| v.to_long_val()).unwrap_or(-1);
    let flags = arg_opt!(ed, 3).map(Value::to_long_val).unwrap_or(0);

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_) => {
            ret!(rv, Value::bool(false));
        }
    };

    let mut arr = PhpArray::new();
    let no_empty = flags & 1 != 0;
    let capture_delimiters = flags & 2 != 0;
    let capture_offsets = flags & 4 != 0;
    let mut push_part = |part: &str, offset: i64| {
        if no_empty && part.is_empty() {
            return;
        }
        if capture_offsets {
            let mut value = PhpArray::with_packed_capacity(2);
            value.push(Value::string(part));
            value.push(Value::long(offset));
            arr.push(Value::array(value));
        } else {
            arr.push(Value::string(part));
        }
    };

    let split_limit = if limit <= 0 { i64::MAX } else { limit };
    let mut cursor = 0usize;
    let mut splits = 0i64;
    for captures in re.captures_iter(&subject) {
        if splits + 1 >= split_limit {
            break;
        }
        let Some(delimiter) = captures.get(0) else {
            continue;
        };
        if delimiter.start < cursor {
            continue;
        }
        push_part(&subject[cursor..delimiter.start], cursor as i64);
        if capture_delimiters {
            for group in 1..captures.len() {
                if let Some(group) = captures.get(group) {
                    push_part(group.as_str(&subject), group.start as i64);
                } else {
                    push_part("", -1);
                }
            }
        }
        cursor = delimiter.end;
        splits += 1;
    }
    push_part(&subject[cursor..], cursor as i64);
    ret!(rv, Value::array(arr));
}

/// preg_replace_callback($pattern, $callback, $subject, $limit, &$count, $flags): string|null
fn fn_preg_replace_callback(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let callback = arg!(ed, 1).clone();
    let resolved = match resolve_callback_at_callsite_checked(&callback, eg, ed)? {
        Some(resolved) => resolved,
        None => {
            if eg.exception.is_some() {
                return Ok(());
            }
            let reason = ordinary_callback_invalid_reason(&callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "preg_replace_callback(): Argument #2 ($callback) must be a valid callback, {reason}"
                ),
            ));
            return Ok(());
        }
    };
    let subject = if arg!(ed, 2).as_array().is_some() {
        arg_str!(ed, 2).into_owned()
    } else {
        let Some(subject) = typed_internal_string_argument_expected(
            ed,
            eg,
            "preg_replace_callback",
            2,
            "subject",
            "array|string",
        )?
        else {
            return Ok(());
        };
        subject
    };
    let limit = arg_opt!(ed, 3).map_or(-1, Value::to_long_val);
    let limit = if limit < 0 {
        usize::MAX
    } else {
        usize::try_from(limit).unwrap_or(usize::MAX)
    };
    let has_count = arg_opt!(ed, 4).is_some();
    let flags = arg_opt!(ed, 5).map_or(0, Value::to_long_val);

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_) => {
            ret!(rv, Value::null());
        }
    };

    let Some((result, replacements)) =
        regex_callback::replace(&re, subject, &resolved, limit, flags & 512 != 0, eg)?
    else {
        return Ok(());
    };

    if has_count {
        arg_mut!(ed, 4, Value::long(replacements as i64));
    }

    ret!(rv, Value::string(result));
}

#[cfg(any(
    feature = "file-contents",
    feature = "file-write",
    feature = "file-lines"
))]
mod file_contents;
#[path = "resource.rs"]
pub(crate) mod resource;
#[path = "stream.rs"]
mod stream;
mod streams;
