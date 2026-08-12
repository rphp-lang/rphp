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

use crate::compiler::compile::PropertyDefinition;
use crate::compiler::{
    make_direct_internal_function, make_internal_function, make_internal_function_ref,
    make_internal_function_variadic, make_internal_method,
};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::{
    ScalarLongSortOrder, VmError, call_function, call_function_iter,
    call_function_iter_with_context, call_function_owned_iter,
    call_function_owned_iter_readback_arg0_with_context, call_function_owned_iter_with_context,
    prepare_scalar_long_callback, try_execute_scalar_long_callback,
};
use crate::vm::frame::ExecuteData;
use crate::vm::function::InternalFunction;
use crate::vm::function::{FunctionCommon, FunctionType};
use crate::vm::instruction::InlineCache;
use crate::vm::opcode::OpCode;

#[cfg(feature = "include-path")]
pub(crate) mod include_path;
mod json_decode;
mod reflection;
mod regex_callback;

// ============================================================================
// Helper macros — zero-cost abstractions for stdlib handlers
// ============================================================================

/// Read CV(n) as &Value — follows references transparently
#[allow(unused_unsafe)]
macro_rules! arg {
    ($ed:expr, $n:expr) => {{
        let v = unsafe { (*$ed).cv($n) };
        if v.is_reference() {
            unsafe { &*v.as_ref_ptr() }
        } else {
            v
        }
    }};
}

/// Read CV(n) as *mut Value — follows references (returns pointer to original)
#[allow(unused_unsafe)]
macro_rules! arg_mut {
    ($ed:expr, $n:expr) => {{
        let ptr = unsafe { (*$ed).cv_mut($n) as *mut Value };
        if unsafe { (*ptr).is_reference() } {
            unsafe { (*ptr).as_ref_ptr() }
        } else {
            ptr
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

pub(crate) mod autoload;

/// Read a borrowed argument for the frame-free internal ABI, following PHP
/// references with the same semantics as `arg!` on an ExecuteData frame.
#[inline(always)]
fn direct_arg(args: &[Value], index: usize) -> &Value {
    let value = &args[index];
    if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    }
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
fn direct_arg_opt(args: &[Value], index: usize) -> Option<&Value> {
    let value = args.get(index)?;
    let value = if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    };
    (value.value_type() != ValueType::Undef).then_some(value)
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

/// Dispatch a compiler-identified pure builtin without resolving a runtime
/// FunctionCommon or crossing the generic internal-function ABI.
#[inline(always)]
pub(crate) fn invoke_direct_internal1(
    kind: crate::builtin_metadata::DirectInternalKind,
    argument: &Value,
) -> Result<Value, VmError> {
    use crate::builtin_metadata::DirectInternalKind;

    let args = std::slice::from_ref(argument);
    match kind {
        DirectInternalKind::Strlen => direct_strlen(args),
        DirectInternalKind::Strtolower => direct_strtolower(args),
        DirectInternalKind::Strtoupper => direct_strtoupper(args),
        DirectInternalKind::Ord => direct_ord(args),
        DirectInternalKind::Abs => direct_abs(args),
        DirectInternalKind::Floor => direct_floor(args),
        DirectInternalKind::Sqrt => direct_sqrt(args),
        DirectInternalKind::ChunkSplit => direct_chunk_split(args),
        DirectInternalKind::Sin => direct_sin(args),
        DirectInternalKind::Tan => direct_tan(args),
        DirectInternalKind::Asin => direct_asin(args),
        DirectInternalKind::Acos => direct_acos(args),
        DirectInternalKind::Atan => direct_atan(args),
        DirectInternalKind::Exp => direct_exp(args),
        DirectInternalKind::Intdiv | DirectInternalKind::JsonDecode => Err(VmError::Fatal(
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
) -> Result<Value, VmError> {
    use crate::builtin_metadata::DirectInternalKind;

    match kind {
        DirectInternalKind::Intdiv => direct_intdiv_values(first, second),
        DirectInternalKind::JsonDecode => Ok(json_decode_values(first, Some(second))),
        _ => Err(VmError::Fatal(
            "Invalid binary direct internal handler ID".into(),
        )),
    }
}

// ============================================================================
// Registration
// ============================================================================

/// Register all stdlib functions into the executor globals.
/// The returned Vec must live as long as the EG (owns the InternalFunction structs).
pub fn register_stdlib(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.reserve_stdlib_capacity();
    let mut funcs: Vec<Box<InternalFunction>> = Vec::with_capacity(128);

    // Register built-in exception classes first (Throwable, Error, TypeError, Exception)
    let class_funcs = register_builtin_classes(eg);
    funcs.extend(class_funcs);

    /// Helper to turn a list of &str into Vec<String> for param_names.
    macro_rules! pn {
        ($($name:expr),*) => { vec![$($name.to_string()),*] };
    }

    macro_rules! reg {
        ($name:expr, $handler:expr, $max_args:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function($handler, $max_args, $min_args, pn![$($pnames),*]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
        ($name:expr, $handler:expr, $max_args:expr, $min_args:expr) => {{
            let f = Box::new(make_internal_function($handler, $max_args, $min_args, vec![]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    macro_rules! reg_direct {
        ($name:expr, $handler:expr, $direct:expr, $max_args:expr, $min_args:expr, $($pnames:expr),*) => {{
            debug_assert_eq!(
                crate::builtin_metadata::direct_internal_spec($name)
                    .map(|spec| (spec.max_args, spec.required_args)),
                Some(($max_args, $min_args)),
                "direct builtin metadata must match stdlib registration",
            );
            let f = Box::new(make_direct_internal_function(
                $handler,
                $direct,
                $max_args,
                $min_args,
                pn![$($pnames),*],
            ));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
        ($name:expr, $handler:expr, $direct:expr, $max_args:expr, $min_args:expr) => {{
            debug_assert_eq!(
                crate::builtin_metadata::direct_internal_spec($name)
                    .map(|spec| (spec.max_args, spec.required_args)),
                Some(($max_args, $min_args)),
                "direct builtin metadata must match stdlib registration",
            );
            let f = Box::new(make_direct_internal_function(
                $handler,
                $direct,
                $max_args,
                $min_args,
                vec![],
            ));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    macro_rules! reg_ref {
        ($name:expr, $handler:expr, $max_args:expr, $min_args:expr, $ref_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_ref($handler, $max_args, $min_args, $ref_args, pn![$($pnames),*]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
        ($name:expr, $handler:expr, $max_args:expr, $min_args:expr, $ref_args:expr) => {{
            let f = Box::new(make_internal_function_ref($handler, $max_args, $min_args, $ref_args, vec![]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    macro_rules! reg_var {
        ($name:expr, $handler:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_variadic($handler, $min_args, pn![$($pnames),*]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
        ($name:expr, $handler:expr, $min_args:expr) => {{
            let f = Box::new(make_internal_function_variadic($handler, $min_args, vec![]));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    // --- Array functions (by-ref: arg 0) ---
    reg!("count", fn_count, 1, 1, "value");
    reg!("sizeof", fn_count, 1, 1, "value");
    reg_ref!("array_push", fn_array_push, 2, 2, 0b1, "array", "value");
    reg_ref!("array_pop", fn_array_pop, 1, 1, 0b1, "array");
    reg_ref!("array_shift", fn_array_shift, 1, 1, 0b1, "array");
    reg_ref!(
        "array_unshift",
        fn_array_unshift,
        2,
        2,
        0b1,
        "array",
        "value"
    );
    reg!(
        "array_key_exists",
        fn_array_key_exists,
        2,
        2,
        "key",
        "array"
    );
    reg!("in_array", fn_in_array, 2, 2, "needle", "haystack");
    reg!("array_reverse", fn_array_reverse, 1, 1, "array");
    reg!("array_merge", fn_array_merge, 2, 2, "array1", "array2");
    reg!("array_keys", fn_array_keys, 1, 1, "array");
    reg!("array_values", fn_array_values, 1, 1, "array");
    reg!(
        "array_slice",
        fn_array_slice,
        3,
        2,
        "array",
        "offset",
        "length"
    );
    reg!("array_unique", fn_array_unique, 1, 1, "array");
    reg!("array_flip", fn_array_flip, 1, 1, "array");
    reg!("array_combine", fn_array_combine, 2, 2, "keys", "values");
    reg!("array_sum", fn_array_sum, 1, 1, "array");
    reg!("array_product", fn_array_product, 1, 1, "array");
    reg!("array_count_values", fn_array_count_values, 1, 1, "array");
    reg!(
        "array_fill",
        fn_array_fill,
        3,
        3,
        "start_index",
        "count",
        "value"
    );
    reg!("array_pad", fn_array_pad, 3, 3, "array", "length", "value");
    reg!("array_chunk", fn_array_chunk, 2, 2, "array", "length");
    reg!("array_column", fn_array_column, 2, 2, "array", "column_key");
    reg_ref!("sort", fn_sort, 1, 1, 0b1, "array");
    reg_ref!("rsort", fn_rsort, 1, 1, 0b1, "array");
    reg!("array_search", fn_array_search, 2, 2, "needle", "haystack");
    reg!("range", fn_range, 2, 2, "start", "end");
    reg_ref!(
        "array_splice",
        fn_array_splice,
        4,
        2,
        0b1,
        "array",
        "offset",
        "length",
        "replacement"
    );
    reg!("array_rand", fn_array_rand, 1, 1, "array");
    reg_ref!("shuffle", fn_shuffle, 1, 1, 0b1, "array");
    reg!("array_map", fn_array_map, 2, 2, "callback", "array");
    reg!("array_filter", fn_array_filter, 2, 1, "array", "callback");
    // compact() requires caller scope access (not yet implemented) — intentionally not registered

    // --- String functions ---
    reg_direct!("strlen", fn_strlen, direct_strlen, 1, 1, "string");
    reg!("substr", fn_substr, 3, 2, "string", "offset", "length");
    reg!("strpos", fn_strpos, 2, 2, "haystack", "needle");
    reg!("strrpos", fn_strrpos, 2, 2, "haystack", "needle");
    reg!("strtr", fn_strtr, 3, 2, "string", "from", "to");
    reg!(
        "str_replace",
        fn_str_replace,
        3,
        3,
        "search",
        "replace",
        "subject"
    );
    reg_direct!(
        "strtolower",
        fn_strtolower,
        direct_strtolower,
        1,
        1,
        "string"
    );
    reg_direct!(
        "strtoupper",
        fn_strtoupper,
        direct_strtoupper,
        1,
        1,
        "string"
    );
    reg!("trim", fn_trim, 1, 1, "string");
    reg!("rtrim", fn_rtrim, 1, 1, "string");
    reg!("ltrim", fn_ltrim, 1, 1, "string");
    reg!("explode", fn_explode, 2, 2, "separator", "string");
    reg!("implode", fn_implode, 2, 2, "separator", "array");
    reg!("join", fn_implode, 2, 2, "separator", "array");
    reg!("str_repeat", fn_str_repeat, 2, 2, "string", "times");
    reg!("substr_count", fn_substr_count, 2, 2, "haystack", "needle");
    reg!("str_contains", fn_str_contains, 2, 2, "haystack", "needle");
    reg!(
        "str_starts_with",
        fn_str_starts_with,
        2,
        2,
        "haystack",
        "needle"
    );
    reg!(
        "str_ends_with",
        fn_str_ends_with,
        2,
        2,
        "haystack",
        "needle"
    );
    reg!(
        "str_pad",
        fn_str_pad,
        3,
        2,
        "string",
        "length",
        "pad_string"
    );
    reg!("str_split", fn_str_split, 2, 1, "string", "length");
    reg!("ucfirst", fn_ucfirst, 1, 1, "string");
    reg!("lcfirst", fn_lcfirst, 1, 1, "string");
    reg!("str_word_count", fn_str_word_count, 1, 1, "string");
    reg!(
        "wordwrap",
        fn_wordwrap,
        4,
        1,
        "string",
        "width",
        "break_str",
        "cut_long_words"
    );
    reg!("nl2br", fn_nl2br, 1, 1, "string");
    reg!("str_rev", fn_str_rev, 1, 1, "string");
    reg!(
        "number_format",
        fn_number_format,
        4,
        1,
        "num",
        "decimals",
        "decimal_separator",
        "thousands_separator"
    );
    reg_direct!("ord", fn_ord, direct_ord, 1, 1, "character");
    reg!("chr", fn_chr, 1, 1, "codepoint");
    reg_var!("sprintf", fn_sprintf, 1, "format");

    // --- Regex functions ---
    reg_ref!(
        "preg_match",
        fn_preg_match,
        3,
        2,
        0b100,
        "pattern",
        "subject",
        "matches"
    );
    reg!(
        "preg_replace",
        fn_preg_replace,
        3,
        3,
        "pattern",
        "replacement",
        "subject"
    );

    // --- Type functions ---
    reg!("intval", fn_intval, 1, 1, "value");
    reg!("strval", fn_strval, 1, 1, "value");
    reg!("floatval", fn_floatval, 1, 1, "value");
    reg!("boolval", fn_boolval, 1, 1, "value");
    reg_ref!("settype", fn_settype, 2, 2, 0b1, "var", "type");
    reg!("is_array", fn_is_array, 1, 1, "value");
    reg!("is_string", fn_is_string, 1, 1, "value");
    reg!("is_int", fn_is_int, 1, 1, "value");
    reg!("is_integer", fn_is_int, 1, 1, "value");
    reg!("is_long", fn_is_int, 1, 1, "value");
    reg!("is_float", fn_is_float, 1, 1, "value");
    reg!("is_double", fn_is_float, 1, 1, "value");
    reg!("is_null", fn_is_null, 1, 1, "value");
    reg!("is_bool", fn_is_bool, 1, 1, "value");
    reg!("is_numeric", fn_is_numeric, 1, 1, "value");
    reg!("is_object", fn_is_object, 1, 1, "value");
    reg!("gettype", fn_gettype, 1, 1, "value");

    // --- Reflection / class introspection ---
    reg!("get_class", fn_get_class, 1, 0, "object");
    reg!(
        "class_exists",
        autoload::fn_class_exists,
        2,
        1,
        "class_name",
        "autoload"
    );
    reg!(
        "interface_exists",
        autoload::fn_interface_exists,
        2,
        1,
        "interface",
        "autoload"
    );
    reg!(
        "trait_exists",
        autoload::fn_trait_exists,
        2,
        1,
        "trait",
        "autoload"
    );
    reg!(
        "enum_exists",
        autoload::fn_enum_exists,
        2,
        1,
        "enum",
        "autoload"
    );
    reg!(
        "class_alias",
        autoload::fn_class_alias,
        3,
        2,
        "class",
        "alias",
        "autoload"
    );
    reg!(
        "spl_autoload",
        autoload::fn_spl_autoload,
        2,
        1,
        "class",
        "file_extensions"
    );
    reg!(
        "spl_autoload_extensions",
        autoload::fn_spl_autoload_extensions,
        1,
        0,
        "file_extensions"
    );
    reg!(
        "spl_autoload_call",
        autoload::fn_spl_autoload_call,
        1,
        1,
        "class"
    );
    reg!(
        "spl_autoload_register",
        autoload::fn_spl_autoload_register,
        3,
        0,
        "callback",
        "throw",
        "prepend"
    );
    reg!(
        "spl_autoload_unregister",
        autoload::fn_spl_autoload_unregister,
        1,
        1,
        "callback"
    );
    reg!(
        "spl_autoload_functions",
        autoload::fn_spl_autoload_functions,
        0,
        0
    );
    reg!(
        "method_exists",
        fn_method_exists,
        2,
        2,
        "object_or_class",
        "method"
    );
    reg!(
        "is_a",
        fn_is_a,
        3,
        2,
        "object_or_class",
        "class",
        "allow_string"
    );
    reg!(
        "is_subclass_of",
        fn_is_subclass_of,
        3,
        2,
        "object_or_class",
        "class",
        "allow_string"
    );

    // --- Math functions ---
    reg_direct!("abs", fn_abs, direct_abs, 1, 1, "num");
    reg!("max", fn_max, 2, 2, "value1", "value2");
    reg!("min", fn_min, 2, 2, "value1", "value2");
    reg_direct!("floor", fn_floor, direct_floor, 1, 1, "num");
    reg!("ceil", fn_ceil, 1, 1, "num");
    reg!("round", fn_round, 2, 1, "num", "precision");
    reg!("pow", fn_pow, 2, 2, "base", "exponent");
    reg_direct!("sqrt", fn_sqrt, direct_sqrt, 1, 1, "num");
    reg_direct!(
        "intdiv",
        fn_intdiv,
        direct_intdiv,
        2,
        2,
        "dividend",
        "divisor"
    );
    reg!("fmod", fn_fmod, 2, 2, "x", "y");
    reg!("log", fn_log, 1, 1, "num");
    reg!("log10", fn_log10, 1, 1, "num");
    reg!("log2", fn_log2, 1, 1, "num");
    reg!("pi", fn_pi, 0, 0);
    reg!("rand", fn_rand, 2, 0, "min", "max");
    reg!("mt_rand", fn_rand, 2, 0, "min", "max");

    // --- Output ---
    reg_var!("var_dump", fn_var_dump, 1, "value");
    reg!("print_r", fn_print_r, 1, 1, "value");
    reg!("var_export", fn_var_export, 2, 1, "value", "return");

    // --- Constants ---
    reg!("define", fn_define, 2, 2, "constant_name", "value");
    reg!("defined", fn_defined, 1, 1, "constant_name");
    reg!("constant", fn_constant, 1, 1, "name");

    // --- JSON ---
    reg!("json_encode", fn_json_encode, 1, 1, "value");
    reg_direct!(
        "json_decode",
        fn_json_decode,
        direct_json_decode,
        2,
        1,
        "json",
        "associative"
    );

    // --- Misc ---
    reg!("isset_func", fn_isset_func, 1, 1, "value");
    reg!("empty_func", fn_empty_func, 1, 1, "value");
    reg!("unset_func", fn_unset_func, 1, 1, "value");

    // --- Callable functions ---
    reg_var!("call_user_func", fn_call_user_func, 1, "callback");
    reg!(
        "call_user_func_array",
        fn_call_user_func_array,
        2,
        2,
        "callback",
        "args"
    );
    reg!("is_callable", fn_is_callable, 1, 1, "value");
    reg!("is_scalar", fn_is_scalar, 1, 1, "value");
    reg!("function_exists", fn_function_exists, 1, 1, "function");

    // --- Time functions ---
    reg!("microtime", fn_microtime, 1, 0, "as_float");
    reg!("hrtime", fn_hrtime, 1, 0, "as_nanoseconds");
    reg!("time", fn_time, 0, 0);
    reg!("date", fn_date, 2, 1, "format", "timestamp");
    reg!(
        "mktime", fn_mktime, 6, 1, "hour", "minute", "second", "month", "day", "year"
    );

    // --- exit / die ---
    reg!("exit", fn_exit, 1, 0, "status");
    reg!("die", fn_exit, 1, 0, "status");

    // --- Filesystem ---
    #[cfg(feature = "include-path")]
    reg!("get_include_path", include_path::fn_get_include_path, 0, 0);
    #[cfg(feature = "include-path")]
    reg!(
        "set_include_path",
        include_path::fn_set_include_path,
        1,
        1,
        "include_path"
    );
    #[cfg(feature = "include-path")]
    reg!(
        "stream_resolve_include_path",
        include_path::fn_stream_resolve_include_path,
        1,
        1,
        "filename"
    );
    streams::register(eg, &mut funcs);
    #[cfg(not(feature = "file-contents"))]
    reg!("file_get_contents", fn_file_get_contents, 1, 1, "filename");
    #[cfg(feature = "file-contents")]
    reg!(
        "file_get_contents",
        file_contents::fn_file_get_contents,
        5,
        1,
        "filename",
        "use_include_path",
        "context",
        "offset",
        "length"
    );
    #[cfg(not(feature = "file-write"))]
    reg!(
        "file_put_contents",
        fn_file_put_contents,
        2,
        2,
        "filename",
        "data"
    );
    #[cfg(feature = "file-write")]
    reg!(
        "file_put_contents",
        file_contents::fn_file_put_contents,
        4,
        2,
        "filename",
        "data",
        "flags",
        "context"
    );
    reg!("file_exists", fn_file_exists, 1, 1, "filename");
    reg!("is_file", fn_is_file, 1, 1, "filename");
    reg!("is_dir", fn_is_dir, 1, 1, "filename");
    reg!("is_readable", fn_is_readable, 1, 1, "filename");
    reg!("is_writable", fn_is_writable, 1, 1, "filename");
    reg!("is_writeable", fn_is_writable, 1, 1, "filename");
    reg!("dirname", fn_dirname, 1, 1, "path");
    reg!("basename", fn_basename, 2, 1, "path", "suffix");
    reg!("realpath", fn_realpath, 1, 1, "path");
    reg!("pathinfo", fn_pathinfo, 1, 1, "path");
    reg!("getcwd", fn_getcwd, 0, 0);
    #[cfg(not(feature = "file-lines"))]
    reg!("file", fn_file, 1, 1, "filename");
    #[cfg(feature = "file-lines")]
    reg!(
        "file",
        file_contents::fn_file,
        3,
        1,
        "filename",
        "flags",
        "context"
    );
    reg!("mkdir", fn_mkdir, 3, 1, "pathname", "mode", "recursive");
    reg!("rmdir", fn_rmdir, 1, 1, "dirname");
    reg!("unlink", fn_unlink, 1, 1, "filename");
    reg!("rename", fn_rename, 2, 2, "old", "new");
    reg!("copy", fn_copy, 2, 2, "source", "dest");
    reg!("tempnam", fn_tempnam, 2, 2, "dir", "prefix");
    reg!("sys_get_temp_dir", fn_sys_get_temp_dir, 0, 0);
    reg!("glob", fn_glob, 1, 1, "pattern");

    // --- URL / query ---
    reg!("parse_url", fn_parse_url, 2, 1, "url", "component");
    reg_ref!("parse_str", fn_parse_str, 2, 2, 0b10, "string", "result");
    reg!(
        "http_build_query",
        fn_http_build_query,
        3,
        1,
        "data",
        "numeric_prefix",
        "arg_separator"
    );

    // --- Regex (extended) ---
    reg_ref!(
        "preg_match_all",
        fn_preg_match_all,
        3,
        2,
        0b100,
        "pattern",
        "subject",
        "matches"
    );
    reg!(
        "preg_split",
        fn_preg_split,
        3,
        2,
        "pattern",
        "subject",
        "limit"
    );
    reg!(
        "preg_replace_callback",
        fn_preg_replace_callback,
        3,
        3,
        "pattern",
        "callback",
        "subject"
    );

    // --- String encoding ---
    reg!("htmlspecialchars", fn_htmlspecialchars, 1, 1, "string");
    reg!(
        "htmlspecialchars_decode",
        fn_htmlspecialchars_decode,
        1,
        1,
        "string"
    );
    reg!("htmlentities", fn_htmlentities, 1, 1, "string");
    reg!("urlencode", fn_urlencode, 1, 1, "string");
    reg!("urldecode", fn_urldecode, 1, 1, "string");
    reg!("rawurlencode", fn_rawurlencode, 1, 1, "string");
    reg!("rawurldecode", fn_rawurldecode, 1, 1, "string");
    reg!("base64_encode", fn_base64_encode, 1, 1, "data");
    reg!("base64_decode", fn_base64_decode, 1, 1, "data");

    // --- Case-insensitive string functions ---
    reg!("stripos", fn_stripos, 2, 2, "haystack", "needle");
    reg!("strripos", fn_strripos, 2, 2, "haystack", "needle");
    reg!(
        "str_ireplace",
        fn_str_ireplace,
        3,
        3,
        "search",
        "replace",
        "subject"
    );
    reg!(
        "substr_replace",
        fn_substr_replace,
        4,
        3,
        "string",
        "replacement",
        "start",
        "length"
    );
    reg!(
        "str_getcsv",
        fn_str_getcsv,
        3,
        1,
        "string",
        "separator",
        "enclosure"
    );
    reg_direct!(
        "chunk_split",
        fn_chunk_split,
        direct_chunk_split,
        3,
        1,
        "string",
        "chunklen",
        "end"
    );

    // --- Additional array functions ---
    reg!(
        "array_reduce",
        fn_array_reduce,
        3,
        2,
        "array",
        "callback",
        "initial"
    );
    reg_ref!("usort", fn_usort, 2, 2, 0b1, "array", "callback");
    reg!("array_diff", fn_array_diff, 2, 2, "array1", "array2");
    reg!(
        "array_intersect",
        fn_array_intersect,
        2,
        2,
        "array1",
        "array2"
    );
    reg_ref!("array_walk", fn_array_walk, 2, 2, 0b1, "array", "callback");
    reg_ref!("asort", fn_asort, 1, 1, 0b1, "array");
    reg_ref!("arsort", fn_arsort, 1, 1, 0b1, "array");
    reg_ref!("ksort", fn_ksort, 1, 1, 0b1, "array");
    reg_ref!("krsort", fn_krsort, 1, 1, 0b1, "array");
    reg!("array_key_first", fn_array_key_first, 1, 1, "array");
    reg!("array_key_last", fn_array_key_last, 1, 1, "array");

    // --- Math (trigonometric + friends) ---
    reg_direct!("sin", fn_sin, direct_sin, 1, 1, "num");
    reg!("cos", fn_cos, 1, 1, "num");
    reg_direct!("tan", fn_tan, direct_tan, 1, 1, "num");
    reg_direct!("asin", fn_asin, direct_asin, 1, 1, "num");
    reg_direct!("acos", fn_acos, direct_acos, 1, 1, "num");
    reg_direct!("atan", fn_atan, direct_atan, 1, 1, "num");
    reg!("atan2", fn_atan2, 2, 2, "y", "x");
    reg_direct!("exp", fn_exp, direct_exp, 1, 1, "num");
    reg!("sinh", fn_sinh, 1, 1, "num");
    reg!("cosh", fn_cosh, 1, 1, "num");
    reg!("tanh", fn_tanh, 1, 1, "num");
    reg!("deg2rad", fn_deg2rad, 1, 1, "num");
    reg!("rad2deg", fn_rad2deg, 1, 1, "num");
    reg!("hypot", fn_hypot, 2, 2, "x", "y");

    // --- Environment / system ---
    reg!("getenv", fn_getenv, 1, 1, "name");
    reg!("putenv", fn_putenv, 1, 1, "assignment");
    reg!("php_uname", fn_php_uname, 1, 0, "mode");
    reg!("php_sapi_name", fn_php_sapi_name, 0, 0);
    reg!("phpversion", fn_phpversion, 0, 0);
    reg!("sleep", fn_sleep, 1, 1, "seconds");
    reg!("usleep", fn_usleep, 1, 1, "microseconds");

    // --- ctype ---
    reg!("ctype_alpha", fn_ctype_alpha, 1, 1, "text");
    reg!("ctype_digit", fn_ctype_digit, 1, 1, "text");
    reg!("ctype_alnum", fn_ctype_alnum, 1, 1, "text");
    reg!("ctype_space", fn_ctype_space, 1, 1, "text");
    reg!("ctype_upper", fn_ctype_upper, 1, 1, "text");
    reg!("ctype_lower", fn_ctype_lower, 1, 1, "text");

    // See streams::register_extensions: this append-only Apple path keeps the
    // admitted hot-code layout stable as new cold stream handlers are added.
    #[cfg(target_vendor = "apple")]
    streams::register_extensions(eg, &mut funcs);

    funcs
}

// ============================================================================
// Built-in exception classes (Throwable hierarchy)
// ============================================================================

/// Internal handler for Error/Exception __construct($message = "")
/// CV 0 = $this, CV 1 = $message
fn fn_throwable_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    let message = arg_opt!(ed, 1);
    if let Some(mut obj) = this_val.as_object_mut() {
        let msg = match message {
            Some(v) => v.clone(),
            None => Value::string(""),
        };
        obj.set_property("message", msg);
    }
    Ok(())
}

/// Internal handler for Error/Exception getMessage()
/// CV 0 = $this
fn fn_throwable_get_message(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        let msg = obj
            .get_property("message")
            .cloned()
            .unwrap_or(Value::string(""));
        ret!(rv, msg);
    }
    ret!(rv, Value::string(""));
}

fn fn_closure_bind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = arg!(ed, 1).as_closure() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "Closure::bind(): Argument #1 ($closure) must be of type Closure",
        ));
        return Ok(());
    };
    let mut rebound = source.clone();
    rebound.identity = std::rc::Rc::new(());

    let new_this = arg!(ed, 2);
    rebound.bound_this = match new_this.value_type() {
        ValueType::Null => None,
        ValueType::Object if rebound.is_static => {
            eg.write_output(
                b"Warning: Closure::bind(): Cannot bind an instance to a static closure\n",
            );
            ret!(rv, Value::null());
        }
        ValueType::Object => Some(new_this.clone()),
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "Closure::bind(): Argument #2 ($newThis) must be of type ?object",
            ));
            return Ok(());
        }
    };

    if let Some(scope) = arg_opt!(ed, 3) {
        rebound.called_scope_class_id = match scope.value_type() {
            ValueType::Null => 0,
            ValueType::String if scope.as_str() == Some("static") => source.called_scope_class_id,
            ValueType::String => {
                let name = scope.as_str().unwrap_or_default();
                let Some(class) = eg.find_class(name) else {
                    eg.write_output(
                        format!("Warning: Closure::bind(): Class \"{name}\" not found\n")
                            .as_bytes(),
                    );
                    ret!(rv, Value::null());
                };
                class.class_id
            }
            ValueType::Object => {
                let object = scope.as_object().expect("object value lost its payload");
                eg.find_class(object.class_name.as_ref())
                    .map_or(0, |class| class.class_id)
            }
            _ => {
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    "Closure::bind(): Argument #3 ($newScope) must be of type object|string|null",
                ));
                return Ok(());
            }
        };
    }

    ret!(rv, Value::closure(rebound));
}

#[cfg(feature = "value-errors")]
#[cold]
fn register_value_error(eg: &mut ExecutorGlobals) -> [Box<InternalFunction>; 2] {
    use crate::compiler::compile::ClassDef;

    eg.register_class(ClassDef {
        name: "ValueError".to_string(),
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    let constructor = Box::new(make_internal_method(
        fn_throwable_construct,
        2,
        0,
        vec!["message".to_string()],
    ));
    let constructor_pointer = &constructor.common as *const FunctionCommon;
    eg.function_table
        .insert("valueerror::__construct".to_string(), constructor_pointer);
    eg.method_declaring_class
        .insert(constructor_pointer, "ValueError".to_string());

    let get_message = Box::new(make_internal_method(fn_throwable_get_message, 1, 0, vec![]));
    let get_message_pointer = &get_message.common as *const FunctionCommon;
    eg.function_table
        .insert("valueerror::getmessage".to_string(), get_message_pointer);
    eg.method_declaring_class
        .insert(get_message_pointer, "ValueError".to_string());
    [constructor, get_message]
}

/// Register Throwable, Error, TypeError, Exception classes with
/// __construct and getMessage methods.
pub fn register_builtin_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use crate::compiler::compile::ClassDef;
    use crate::parser::Visibility;

    let mut funcs: Vec<Box<InternalFunction>> = Vec::with_capacity(64);

    // Helper: register an internal method and return its func pointer
    macro_rules! reg_method {
        ($class:expr, $method:expr, $handler:expr, $num_args:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_method($handler, $num_args, $min_args, vec![$($pnames.to_string()),*]));
            let ptr = &f.common as *const FunctionCommon;
            let full_name = format!("{}::{}", $class, $method).to_lowercase();
            eg.function_table.insert(full_name, ptr);
            eg.method_declaring_class.insert(ptr, $class.to_string());
            funcs.push(f);
        }};
        ($class:expr, $method:expr, $handler:expr, $num_args:expr, $min_args:expr) => {{
            let f = Box::new(make_internal_method($handler, $num_args, $min_args, vec![]));
            let ptr = &f.common as *const FunctionCommon;
            let full_name = format!("{}::{}", $class, $method).to_lowercase();
            eg.function_table.insert(full_name, ptr);
            eg.method_declaring_class.insert(ptr, $class.to_string());
            funcs.push(f);
        }};
    }

    // Throwable — proper interface (PHP 8 compatible)
    eg.register_class(ClassDef {
        name: "Throwable".to_string(),
        parent: None,
        implements: vec![],
        is_interface: true,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // Exception implements Throwable
    eg.register_class(ClassDef {
        name: "Exception".to_string(),
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![PropertyDefinition::new(
            "message".to_string(),
            Some(Value::string("")),
            Visibility::Protected,
            "Exception".to_string(),
        )],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // Error implements Throwable
    eg.register_class(ClassDef {
        name: "Error".to_string(),
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![PropertyDefinition::new(
            "message".to_string(),
            Some(Value::string("")),
            Visibility::Protected,
            "Error".to_string(),
        )],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // TypeError extends Error
    eg.register_class(ClassDef {
        name: "TypeError".to_string(),
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // CompileError extends Error
    eg.register_class(ClassDef {
        name: "CompileError".to_string(),
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // ParseError extends CompileError
    eg.register_class(ClassDef {
        name: "ParseError".to_string(),
        parent: Some("CompileError".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // ArgumentCountError extends Error
    eg.register_class(ClassDef {
        name: "ArgumentCountError".to_string(),
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // UnhandledMatchError extends Error
    eg.register_class(ClassDef {
        name: "UnhandledMatchError".to_string(),
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();

    // Register __construct and getMessage for each throwable class
    // num_args = 2 for __construct (CV 0 = $this, CV 1 = $message)
    // num_args = 1 for getMessage (CV 0 = $this)
    for class in &[
        "Throwable",
        "Exception",
        "Error",
        "TypeError",
        "CompileError",
        "ParseError",
        "ArgumentCountError",
        "UnhandledMatchError",
    ] {
        // __construct: num_args=2 (CV 0=$this, CV 1=$message), required=0 ($message is optional)
        reg_method!(
            class,
            "__construct",
            fn_throwable_construct,
            2,
            0,
            "message"
        );
        // getMessage: num_args=1 (CV 0=$this), required=0 (no explicit args)
        reg_method!(class, "getmessage", fn_throwable_get_message, 1, 0);
    }

    funcs.extend(reflection::register(eg));

    #[cfg(feature = "value-errors")]
    funcs.extend(register_value_error(eg));

    let empty_internal_type =
        |name: &str, implements: Vec<String>, is_interface: bool, is_final: bool| ClassDef {
            name: name.to_string(),
            parent: None,
            implements,
            is_interface,
            is_abstract: false,
            is_final,
            is_trait: false,
            is_enum: false,
            uses: vec![],
            properties: vec![],
            static_properties: vec![],
            constants: vec![],
            property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
            property_defaults: std::rc::Rc::from([]),
            readonly_props: vec![],
            methods: vec![],
            abstract_methods: vec![],
            class_id: 0,
        };

    // stdClass has dynamic object storage but still participates in ordinary
    // class_exists(), aliases, type hints and reflection as an internal class.
    eg.register_class(empty_internal_type("stdClass", vec![], false, false))
        .unwrap();

    eg.register_class(empty_internal_type("Closure", vec![], false, true))
        .unwrap();
    // Static methods still reserve the canonical hidden method slot at CV 0;
    // explicit Closure::bind arguments begin at CV 1.
    reg_method!(
        "Closure",
        "bind",
        fn_closure_bind,
        4,
        2,
        "closure",
        "newThis",
        "newScope"
    );

    // Canonical iterator hierarchy used by generator return contracts,
    // instanceof and the iterable pseudo-type.
    eg.register_class(empty_internal_type("Traversable", vec![], true, false))
        .unwrap();
    eg.register_class(empty_internal_type(
        "Iterator",
        vec!["Traversable".to_string()],
        true,
        false,
    ))
    .unwrap();
    eg.register_class(empty_internal_type(
        "Generator",
        vec!["Iterator".to_string()],
        false,
        true,
    ))
    .unwrap();

    // Generator methods: $this is CV 0
    reg_method!("Generator", "current", fn_generator_current, 1, 0);
    reg_method!("Generator", "key", fn_generator_key, 1, 0);
    reg_method!("Generator", "next", fn_generator_next, 1, 0);
    reg_method!("Generator", "valid", fn_generator_valid, 1, 0);
    reg_method!("Generator", "rewind", fn_generator_rewind, 1, 0);
    reg_method!("Generator", "send", fn_generator_send, 2, 1, "value");
    reg_method!("Generator", "getreturn", fn_generator_get_return, 1, 0);

    funcs
}

// ============================================================================
// Array functions
// ============================================================================

fn fn_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let n = match v.as_array() {
        Some(arr) => arr.len() as i64,
        None => {
            if v.value_type() == ValueType::Null {
                0
            } else {
                1
            }
        }
    };
    ret!(rv, Value::long(n));
}

fn fn_array_push(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let val = arg!(ed, 1).clone();
    let arr = unsafe { &mut *ptr };
    if let Some(a) = arr.as_array_mut() {
        a.push(val);
        ret!(rv, Value::long(a.len() as i64));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_pop(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        ret!(rv, a.pop().unwrap_or(Value::null()));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_shift(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        ret!(rv, a.shift().unwrap_or(Value::null()));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_unshift(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let val = arg!(ed, 1).clone();
    let arr = unsafe { &mut *ptr };
    if let Some(a) = arr.as_array_mut() {
        // Rebuild with val at front
        let mut new = PhpArray::new();
        new.push(val);
        for (key, v) in a.iter() {
            match &key {
                ArrayKey::Int(_) => new.push(v.clone()),
                ArrayKey::String(k) => new.set_str(k, v.clone()),
            }
        }
        *arr = Value::array(new);
        ret!(
            rv,
            Value::long(arr.as_array().map(|a| a.len()).unwrap_or(0) as i64)
        );
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_key_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let key = arg!(ed, 0);
    let arr = arg!(ed, 1);
    let exists = if let Some(a) = arr.as_array() {
        match key.value_type() {
            ValueType::Long => a.get_int(key.as_long().unwrap()).is_some(),
            ValueType::String => a.get_str(key.as_str().unwrap()).is_some(),
            _ => false,
        }
    } else {
        false
    };
    ret!(rv, Value::bool(exists));
}

fn fn_in_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1);
    let found = haystack
        .as_array()
        .map(|a| a.values().any(|v| values_equal(needle, v)))
        .unwrap_or(false);
    ret!(rv, Value::bool(found));
}

fn fn_array_reverse(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut new = PhpArray::new();
        let collected: Vec<(ArrayKey, &Value)> = arr.iter().collect();
        for (key, val) in collected.into_iter().rev() {
            match &key {
                ArrayKey::Int(_) => new.push(val.clone()),
                ArrayKey::String(k) => new.set_str(k, val.clone()),
            }
        }
        ret!(rv, Value::array(new));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_merge(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let a1 = arg!(ed, 0);
    let a2 = arg!(ed, 1);
    if let (Some(a1), Some(a2)) = (a1.as_array(), a2.as_array()) {
        let mut merged = PhpArray::new();
        for (key, val) in a1.iter() {
            match &key {
                ArrayKey::Int(_) => merged.push(val.clone()),
                ArrayKey::String(k) => merged.set_str(k, val.clone()),
            }
        }
        for (key, val) in a2.iter() {
            match &key {
                ArrayKey::Int(_) => merged.push(val.clone()),
                ArrayKey::String(k) => merged.set_str(k, val.clone()),
            }
        }
        ret!(rv, Value::array(merged));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_keys(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for (key, _) in arr.iter() {
            match key {
                ArrayKey::Int(k) => result.push(Value::long(k)),
                ArrayKey::String(k) => result.push(Value::string(k)),
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for val in arr.values() {
            result.push(val.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_slice(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    if let Some(arr) = arr_arg.as_array() {
        let len = arr.len() as i64;
        let raw_offset = arg_long!(ed, 1);
        let start = if raw_offset < 0 {
            (len + raw_offset).max(0) as usize
        } else {
            raw_offset as usize
        };
        let end = match arg_opt!(ed, 2) {
            Some(v) => {
                let l = v.to_long_val();
                if l < 0 {
                    (len + l).max(start as i64) as usize
                } else {
                    (start + l as usize).min(arr.len())
                }
            }
            None => arr.len(),
        };
        let mut result = PhpArray::new();
        for val in arr.values().skip(start).take(end.saturating_sub(start)) {
            result.push(val.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_unique(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        let mut seen: Vec<String> = Vec::with_capacity(arr.len());
        for (key, val) in arr.iter() {
            let s = val.echo_to_string();
            if !seen.contains(&s) {
                seen.push(s);
                result.set(key, val.clone());
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_flip(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for (key, val) in arr.iter() {
            let new_key = match val.value_type() {
                ValueType::Long => ArrayKey::Int(val.as_long().unwrap()),
                ValueType::String => ArrayKey::String(val.as_str().unwrap().to_string()),
                _ => continue,
            };
            let new_val = match key {
                ArrayKey::Int(k) => Value::long(k),
                ArrayKey::String(k) => Value::string(k),
            };
            result.set(new_key, new_val);
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_combine(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let keys_arg = arg!(ed, 0);
    let vals_arg = arg!(ed, 1);
    if let (Some(keys), Some(vals)) = (keys_arg.as_array(), vals_arg.as_array()) {
        let mut result = PhpArray::new();
        for (kv, vv) in keys.values().zip(vals.values()) {
            let key = match kv {
                val if val.as_str().is_some() => {
                    ArrayKey::String(val.as_str().unwrap().to_string())
                }
                val => ArrayKey::Int(val.as_long().unwrap_or(0)),
            };
            result.set(key, vv.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_array_sum(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut has_float = false;
        let mut sum_int: i64 = 0;
        let mut sum_float: f64 = 0.0;
        for val in arr.values() {
            match val.value_type() {
                ValueType::Long => sum_int = sum_int.wrapping_add(val.as_long().unwrap()),
                ValueType::Double => {
                    has_float = true;
                    sum_float += val.as_double().unwrap();
                }
                _ => {}
            }
        }
        ret!(
            rv,
            if has_float {
                Value::double(sum_float + sum_int as f64)
            } else {
                Value::long(sum_int)
            }
        );
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_product(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut has_float = false;
        let mut prod_int: i64 = 1;
        let mut prod_float: f64 = 1.0;
        for val in arr.values() {
            match val.value_type() {
                ValueType::Long => prod_int = prod_int.wrapping_mul(val.as_long().unwrap()),
                ValueType::Double => {
                    has_float = true;
                    prod_float *= val.as_double().unwrap();
                }
                _ => {}
            }
        }
        ret!(
            rv,
            if has_float {
                Value::double(prod_float * prod_int as f64)
            } else {
                Value::long(prod_int)
            }
        );
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_count_values(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut counts: Vec<(String, i64)> = Vec::new();
        for val in arr.values() {
            let s = val.echo_to_string();
            if let Some(entry) = counts.iter_mut().find(|(k, _)| k == &s) {
                entry.1 += 1;
            } else {
                counts.push((s, 1));
            }
        }
        let mut result = PhpArray::new();
        for (k, cnt) in counts {
            result.set_str(&k, Value::long(cnt));
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_fill(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let start = arg_long!(ed, 0) as i64;
    let count = arg_long!(ed, 1).max(0) as usize;
    let value = arg!(ed, 2);
    let mut result = PhpArray::new();
    for i in 0..count {
        result.set_int(start + i as i64, value.clone());
    }
    ret!(rv, Value::array(result));
}

fn fn_array_pad(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let size = arg_long!(ed, 1);
    let value = arg!(ed, 2);
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let abs_size = size.unsigned_abs() as usize;
        let pad_count = abs_size.saturating_sub(arr.len());
        if size < 0 {
            for _ in 0..pad_count {
                result.push(value.clone());
            }
        }
        for v in arr.values() {
            result.push(v.clone());
        }
        if size >= 0 {
            for _ in 0..pad_count {
                result.push(value.clone());
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_chunk(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let size = arg_long!(ed, 1).max(1) as usize;
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let mut chunk = PhpArray::new();
        let mut i = 0;
        for v in arr.values() {
            chunk.push(v.clone());
            i += 1;
            if i == size {
                result.push(Value::array(chunk));
                chunk = PhpArray::new();
                i = 0;
            }
        }
        if !chunk.is_empty() {
            result.push(Value::array(chunk));
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_column(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let col_key = arg!(ed, 1);
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let key_str = col_key.echo_to_string();
        for row in arr.values() {
            if let Some(inner) = row.as_array() {
                // Try string key first, then integer
                let val = inner
                    .get_str(&key_str)
                    .or_else(|| col_key.as_long().and_then(|k| inner.get_int(k)));
                if let Some(v) = val {
                    result.push(v.clone());
                }
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_sort(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        entries.sort_by(|a, b| cmp_val(compare_values(a, b)));
        let mut new = PhpArray::new();
        for v in entries {
            new.push(v);
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_rsort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        entries.sort_by(|a, b| cmp_val(compare_values(b, a)));
        let mut new = PhpArray::new();
        for v in entries {
            new.push(v);
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_array_search(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1);
    if let Some(arr) = haystack.as_array() {
        for (key, val) in arr.iter() {
            if values_equal(needle, val) {
                let result = match key {
                    ArrayKey::Int(k) => Value::long(k),
                    ArrayKey::String(k) => Value::string(k),
                };
                ret!(rv, result);
            }
        }
    }
    ret!(rv, Value::bool(false));
}

fn fn_range(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let start = arg_long!(ed, 0);
    let end = arg_long!(ed, 1);
    let mut arr = PhpArray::new();
    if start <= end {
        for i in start..=end {
            arr.push(Value::long(i));
        }
    } else {
        for i in (end..=start).rev() {
            arr.push(Value::long(i));
        }
    }
    ret!(rv, Value::array(arr));
}

fn fn_array_splice(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let offset = arg_long!(ed, 1);
    let arr = unsafe { &mut *ptr };
    if let Some(a) = arr.as_array_mut() {
        let len = a.len() as i64;
        let start = if offset < 0 {
            (len + offset).max(0) as usize
        } else {
            (offset as usize).min(a.len())
        };
        let del_count = match arg_opt!(ed, 2) {
            Some(v) => v.to_long_val().max(0) as usize,
            None => a.len() - start,
        };
        let replacement = arg_opt!(ed, 3).and_then(|v| v.as_array());

        let entries: Vec<(ArrayKey, Value)> = a.iter().map(|(k, v)| (k, v.clone())).collect();
        let mut removed = PhpArray::new();
        let mut new = PhpArray::new();

        for (i, (_, v)) in entries.iter().enumerate() {
            if i < start || i >= start + del_count {
                new.push(v.clone());
            } else {
                removed.push(v.clone());
                if i == start {
                    if let Some(repl) = replacement {
                        for rv in repl.values() {
                            new.push(rv.clone());
                        }
                    }
                }
            }
        }
        *arr = Value::array(new);
        ret!(rv, Value::array(removed));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_rand(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        if arr.is_empty() {
            ret!(rv, Value::null());
        } else {
            // Simple pseudo-random using wrapping arithmetic
            let idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as usize)
                % arr.len();
            let (_, key) = arr.get_at(idx).unwrap();
            let result = match key {
                ArrayKey::Int(k) => Value::long(k),
                ArrayKey::String(k) => Value::string(k),
            };
            ret!(rv, result);
        }
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_shuffle(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        // Fisher-Yates with simple PRNG
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize;
        for i in (1..entries.len()).rev() {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let j = seed % (i + 1);
            entries.swap(i, j);
        }
        let mut new = PhpArray::new();
        for v in entries {
            new.push(v);
        }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

/// array_map($callback, $array) — apply callback to each element, return new array
fn fn_array_map(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let arr_val = arg!(ed, 1);
    let resolved = match resolve_callback_at_callsite(callback, eg, ed) {
        Some(resolved) => resolved,
        None => {
            let description = callback.echo_to_string();
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "array_map(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found",
                    description
                ),
            ));
            return Ok(());
        }
    };
    if let Some(arr) = arr_val.as_array() {
        let mut result = if arr.is_packed() {
            PhpArray::with_packed_capacity(arr.len())
        } else {
            PhpArray::with_deferred_hash_capacity(arr.len())
        };
        for (key, val) in arr.iter() {
            let mapped = call_resolved_with_values(eg, &resolved, std::slice::from_ref(val))?;
            if eg.exception.is_some() {
                return Ok(());
            }
            result.set(key, mapped);
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

/// array_filter($array [, $callback]) — filter elements by callback (or truthiness)
fn fn_array_filter(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr_val = arg!(ed, 0);
    let callback = arg_opt!(ed, 1);
    if let Some(arr) = arr_val.as_array() {
        let mut result = PhpArray::new();
        match callback {
            Some(cb_val) => {
                let resolved = match resolve_callback_at_callsite(cb_val, eg, ed) {
                    Some(resolved) => resolved,
                    None => {
                        let description = cb_val.echo_to_string();
                        eg.exception = Some(crate::value::make_error_value(
                            "TypeError",
                            &format!(
                                "array_filter(): Argument #2 ($callback) must be a valid callback, function \"{}\" not found",
                                description
                            ),
                        ));
                        return Ok(());
                    }
                };
                for (key, val) in arr.iter() {
                    let ret_val =
                        call_resolved_with_values(eg, &resolved, std::slice::from_ref(val))?;
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                    if ret_val.is_truthy() {
                        result.set(key, val.clone());
                    }
                }
            }
            None => {
                // No callback — filter by truthiness
                for (key, val) in arr.iter() {
                    if val.is_truthy() {
                        result.set(key, val.clone());
                    }
                }
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

// compact() intentionally removed — requires caller scope access (not yet implemented)

// ============================================================================
// String functions
// ============================================================================

#[inline(always)]
pub(crate) fn direct_strlen_len(argument: &Value) -> i64 {
    let argument = if argument.is_reference() {
        unsafe { &*argument.as_ref_ptr() }
    } else {
        argument
    };
    match argument.as_str() {
        Some(string) => string.len() as i64,
        None => argument.echo_to_string().len() as i64,
    }
}

#[inline(always)]
fn direct_strlen(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::long(direct_strlen_len(&args[0])))
}

fn fn_strlen(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = direct_strlen(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

fn fn_substr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;
    let start_raw = arg_long!(ed, 1);
    let start = if start_raw < 0 {
        (len + start_raw).max(0) as usize
    } else {
        start_raw as usize
    };
    let end = match arg_opt!(ed, 2) {
        Some(v) => {
            let l = v.to_long_val();
            if l < 0 {
                ((len + l) as usize).max(start)
            } else {
                (start + l as usize).min(bytes.len())
            }
        }
        None => bytes.len(),
    };
    if start >= bytes.len() {
        ret!(rv, Value::string(""));
    } else {
        ret!(
            rv,
            Value::string(String::from_utf8_lossy(&bytes[start..end]).into_owned())
        );
    }
}

fn fn_strpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(
        rv,
        match h.find(n.as_ref()) {
            Some(pos) => Value::long(pos as i64),
            None => Value::bool(false),
        }
    );
}

fn fn_strrpos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(
        rv,
        match h.rfind(n.as_ref()) {
            Some(pos) => Value::long(pos as i64),
            None => Value::bool(false),
        }
    );
}

fn fn_strtr(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let subject = arg_str!(ed, 0);
    let from_or_pairs = arg!(ed, 1);

    if let Some(to_value) = arg_opt!(ed, 2) {
        let from = match from_or_pairs.as_str() {
            Some(value) => Cow::Borrowed(value),
            None if !matches!(
                from_or_pairs.value_type(),
                ValueType::Array | ValueType::Object
            ) =>
            {
                Cow::Owned(from_or_pairs.echo_to_string())
            }
            None => {
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    "strtr(): Argument #2 ($from) must be of type string",
                ));
                return Ok(());
            }
        };
        let to = match to_value.as_str() {
            Some(value) => Cow::Borrowed(value),
            None if !matches!(to_value.value_type(), ValueType::Array | ValueType::Object) => {
                Cow::Owned(to_value.echo_to_string())
            }
            None => {
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    "strtr(): Argument #3 ($to) must be of type string",
                ));
                return Ok(());
            }
        };

        let mut translated = subject.as_bytes().to_vec();
        let from = from.as_bytes();
        let to = to.as_bytes();
        for byte in &mut translated {
            if let Some(position) = from.iter().position(|candidate| candidate == byte)
                && let Some(replacement) = to.get(position)
            {
                *byte = *replacement;
            }
        }
        ret!(
            rv,
            Value::string(String::from_utf8_lossy(&translated).into_owned())
        );
    }

    let Some(pairs) = from_or_pairs.as_array() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "strtr(): Argument #2 ($from) must be of type array, string given",
        ));
        return Ok(());
    };

    let mut replacements = Vec::with_capacity(pairs.len());
    for (key, value) in pairs.iter() {
        let search = match key {
            ArrayKey::Int(value) => value.to_string(),
            ArrayKey::String(value) => value,
        };
        if search.is_empty() {
            eg.write_output(b"Warning: strtr(): Ignoring replacement of empty string\n");
            continue;
        }
        if value.value_type() == ValueType::Array {
            eg.write_output(b"Warning: Array to string conversion\n");
        }
        replacements.push((search.into_bytes(), value.echo_to_string()));
    }
    // PHP selects the longest key at each input position. `sort_by` is stable,
    // so equal-length keys retain their source-array order.
    replacements.sort_by(|(left, _), (right, _)| right.len().cmp(&left.len()));

    let input = subject.as_bytes();
    let mut translated = Vec::with_capacity(input.len());
    let mut position = 0;
    while position < input.len() {
        if let Some((search, replacement)) = replacements
            .iter()
            .find(|(search, _)| input[position..].starts_with(search))
        {
            translated.extend_from_slice(replacement.as_bytes());
            position += search.len();
        } else {
            translated.push(input[position]);
            position += 1;
        }
    }
    ret!(
        rv,
        Value::string(String::from_utf8_lossy(&translated).into_owned())
    );
}

fn fn_str_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let search = arg_str!(ed, 0);
    let replace = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);
    ret!(
        rv,
        Value::string(subject.replace(search.as_ref(), replace.as_ref()))
    );
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

fn fn_trim(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim()));
}

fn fn_rtrim(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim_end()));
}

fn fn_ltrim(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim_start()));
}

fn fn_explode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let d = arg_str!(ed, 0);
    let s = arg_str!(ed, 1);
    let mut arr = PhpArray::new();
    for part in s.split(d.as_ref()) {
        arr.push(Value::string(part));
    }
    ret!(rv, Value::array(arr));
}

fn fn_implode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let glue = arg_str!(ed, 0);
    let pieces = arg!(ed, 1);
    if let Some(arr) = pieces.as_array() {
        let glue_bytes = glue.len().saturating_mul(arr.len().saturating_sub(1));
        let value_bytes = arr.values().map(Value::echo_len_hint).sum::<usize>();
        let mut result = String::with_capacity(glue_bytes.saturating_add(value_bytes));
        for (index, value) in arr.values().enumerate() {
            if index > 0 {
                result.push_str(glue.as_ref());
            }
            value.append_echo_to(&mut result);
        }
        ret!(rv, Value::string(result));
    } else {
        ret!(rv, Value::string(""));
    }
}

fn fn_str_repeat(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let times = arg_long!(ed, 1).max(0) as usize;
    ret!(rv, Value::string(s.repeat(times)));
}

fn fn_substr_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    let count = if n.is_empty() {
        0
    } else {
        h.matches(n.as_ref()).count() as i64
    };
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

fn fn_str_pad(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 0);
    let length = arg_long!(ed, 1) as usize;
    let pad = match arg_opt!(ed, 2) {
        Some(v) => v.as_str().unwrap_or(" ").to_string(),
        None => " ".to_string(),
    };
    if input.len() >= length {
        ret!(rv, Value::string(input.into_owned()));
    } else {
        let diff = length - input.len();
        let padding: String = pad.chars().cycle().take(diff).collect();
        ret!(rv, Value::string(format!("{}{}", input, padding)));
    }
}

fn fn_str_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let chunk = match arg_opt!(ed, 1) {
        Some(v) => v.to_long_val().max(1) as usize,
        None => 1,
    };
    let mut arr = PhpArray::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + chunk).min(bytes.len());
        arr.push(Value::string(
            String::from_utf8_lossy(&bytes[i..end]).into_owned(),
        ));
        i = end;
    }
    if arr.is_empty() {
        arr.push(Value::string(""));
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

fn fn_str_word_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::long(s.split_whitespace().count() as i64));
}

fn fn_wordwrap(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let width = match arg_opt!(ed, 1) {
        Some(v) => v.to_long_val().max(1) as usize,
        None => 75,
    };
    let brk = match arg_opt!(ed, 2) {
        Some(v) => v.as_str().unwrap_or("\n").to_string(),
        None => "\n".to_string(),
    };
    let cut = match arg_opt!(ed, 3) {
        Some(v) => v.is_truthy(),
        None => false,
    };
    let mut result = String::with_capacity(s.len() + s.len() / width);
    let mut line_len = 0;
    for word in s.split(' ') {
        if cut && word.len() > width {
            for ch in word.chars() {
                if line_len >= width {
                    result.push_str(&brk);
                    line_len = 0;
                }
                result.push(ch);
                line_len += 1;
            }
        } else {
            if line_len > 0 && line_len + 1 + word.len() > width {
                result.push_str(&brk);
                line_len = 0;
            } else if line_len > 0 {
                result.push(' ');
                line_len += 1;
            }
            result.push_str(word);
            line_len += word.len();
        }
    }
    ret!(rv, Value::string(result));
}

fn fn_nl2br(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.replace('\n', "<br />\n")));
}

fn fn_str_rev(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    // PHP strrev reverses bytes, not Unicode codepoints
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
    let s = direct_arg_str(args, 0);
    Ok(Value::long(
        s.as_bytes().first().copied().unwrap_or(0) as i64
    ))
}

fn fn_ord(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let result = direct_ord(std::slice::from_ref(arg!(ed, 0)))?;
    ret!(rv, result);
}

fn fn_chr(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let code = (arg_long!(ed, 0) & 0xFF) as u8;
    ret!(rv, Value::string(String::from(code as char)));
}

fn fn_sprintf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let fmt = arg_str!(ed, 0);
    // Variadic: VM packs extra args into an array at CV(1)
    // Read values directly from that array without cloning them into another Vec.
    let variadic_arr = arg!(ed, 1);
    let args = variadic_arr.as_array();
    let args_count = args
        .map(|array| array.len())
        .unwrap_or_else(|| usize::from(variadic_arr.value_type() != ValueType::Undef));

    let mut result = String::with_capacity(fmt.len().saturating_add(args_count * 8));
    let bytes = fmt.as_bytes();
    let mut literal_start = 0usize;
    let mut index = 0usize;
    let mut arg_idx = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            result.push_str(&fmt[literal_start..index]);
            if index + 1 < bytes.len() {
                let spec = bytes[index + 1] as char;
                if spec == '%' {
                    result.push('%');
                } else {
                    let arg = if let Some(args) = args {
                        args.get_value_at(arg_idx)
                    } else if arg_idx == 0 && variadic_arr.value_type() != ValueType::Undef {
                        Some(variadic_arr)
                    } else {
                        None
                    };
                    arg_idx += 1;
                    match spec {
                        's' => {
                            if let Some(arg) = arg {
                                arg.append_echo_to(&mut result);
                            }
                        }
                        'd' => {
                            let _ = write!(
                                result,
                                "{}",
                                arg.map(|value| value.to_long_val()).unwrap_or(0)
                            );
                        }
                        'f' => {
                            let value = arg.map(|value| value.to_float_val()).unwrap_or(0.0);
                            let _ = write!(result, "{value:.6}");
                        }
                        'x' => {
                            let value = arg.map(|value| value.to_long_val()).unwrap_or(0);
                            let _ = write!(result, "{value:x}");
                        }
                        'X' => {
                            let value = arg.map(|value| value.to_long_val()).unwrap_or(0);
                            let _ = write!(result, "{value:X}");
                        }
                        'o' => {
                            let value = arg.map(|value| value.to_long_val()).unwrap_or(0);
                            let _ = write!(result, "{value:o}");
                        }
                        'b' => {
                            let value = arg.map(|value| value.to_long_val()).unwrap_or(0);
                            let _ = write!(result, "{value:b}");
                        }
                        'c' => {
                            let code = arg.map(|value| value.to_long_val()).unwrap_or(0);
                            result.push((code & 0xFF) as u8 as char);
                        }
                        _ => {
                            result.push('%');
                            result.push(spec);
                            arg_idx -= 1;
                        }
                    }
                }
                index += 2;
                literal_start = index;
                continue;
            } else {
                result.push('%');
                index += 1;
                literal_start = index;
                continue;
            }
        }
        index += 1;
    }
    result.push_str(&fmt[literal_start..]);
    ret!(rv, Value::string(result));
}

// ============================================================================
// Type functions
// ============================================================================

fn fn_intval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(arg!(ed, 0).to_long_val()));
}

fn fn_strval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(arg!(ed, 0).echo_to_string()));
}

fn fn_floatval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::double(arg!(ed, 0).to_float_val()));
}

fn fn_boolval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).is_truthy()));
}

fn fn_settype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let type_name = arg_str!(ed, 1);
    let val = unsafe { &*ptr };
    let new_val = match type_name.as_ref() {
        "int" | "integer" => Value::long(val.to_long_val()),
        "float" | "double" => Value::double(val.to_float_val()),
        "string" => Value::string(val.echo_to_string()),
        "bool" | "boolean" => Value::bool(val.is_truthy()),
        "array" => {
            if val.value_type() == ValueType::Array {
                val.clone()
            } else {
                let mut a = PhpArray::new();
                a.push(val.clone());
                Value::array(a)
            }
        }
        "null" => Value::null(),
        _ => {
            ret!(rv, Value::bool(false));
        }
    };
    unsafe {
        std::ptr::drop_in_place(ptr);
        ptr.write(new_val);
    }
    ret!(rv, Value::bool(true));
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
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Null));
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
        Value::bool(arg!(ed, 0).value_type() == ValueType::Object)
    );
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
        ValueType::Object => "object",
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
        // No argument — return the current class name (deprecated in PHP 8 but still works)
        let caller_class = get_calling_scope_class(ed, eg);
        if let Some(cls) = caller_class {
            eg.write_output(b"Deprecated: Calling get_class() without arguments is deprecated\n");
            ret!(rv, Value::string(cls));
        }
        // Outside class scope: PHP throws Error
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "get_class() without arguments must be called from within a class",
        ));
        return Ok(());
    }
    if let Some(obj) = v.as_object() {
        ret!(rv, Value::string(obj.class_name.as_ref()));
    }
    ret!(rv, Value::bool(false));
}

fn fn_method_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = arg!(ed, 0);
    let method_name = arg_str!(ed, 1);

    // Resolve the class name: from object or string
    let (class_name, needs_autoload): (String, bool) = if let Some(obj) = first.as_object() {
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
    let found = method_declared_in_class_hierarchy(eg, &class_name, &method_name);
    ret!(rv, Value::bool(found));
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

fn fn_max(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg!(ed, 0);
    let b = arg!(ed, 1);
    ret!(
        rv,
        if compare_values(a, b) >= 0 {
            a.clone()
        } else {
            b.clone()
        }
    );
}

fn fn_min(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg!(ed, 0);
    let b = arg!(ed, 1);
    ret!(
        rv,
        if compare_values(a, b) <= 0 {
            a.clone()
        } else {
            b.clone()
        }
    );
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

// ============================================================================
// Output functions
// ============================================================================

fn fn_var_dump(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = var_dump_value(arg!(ed, 0), 0, eg);
    eg.write_output(first.as_bytes());
    if let Some(arguments) = arg!(ed, 1).as_array() {
        for value in arguments.values() {
            let output = var_dump_value(value, 0, eg);
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
    let output = print_r_value(v, 0);
    eg.write_output(output.as_bytes());
    ret!(rv, Value::bool(true));
}

fn fn_var_export(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let return_str = match arg_opt!(ed, 1) {
        Some(v) => v.is_truthy(),
        None => false,
    };
    let output = var_export_value(v);
    if return_str {
        ret!(rv, Value::string(output));
    } else {
        eg.write_output(output.as_bytes());
        ret!(rv, Value::null());
    }
}

// ============================================================================
// Constant functions
// ============================================================================

fn fn_define(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    if name.is_empty() {
        ret!(rv, Value::bool(false));
    }
    let val = arg!(ed, 1).clone();
    let result = eg.define_constant(&name, val);
    ret!(rv, Value::bool(result.is_ok()));
}

fn fn_defined(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    ret!(rv, Value::bool(eg.find_constant(&name).is_some()));
}

fn fn_constant(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    ret!(rv, eg.find_constant(&name).unwrap_or(Value::null()));
}

// ============================================================================
// JSON functions
// ============================================================================

fn fn_json_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    ret!(rv, Value::string(json_encode_value(v)));
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

fn compare_values(a: &Value, b: &Value) -> i32 {
    let ad = a.to_float_val();
    let bd = b.to_float_val();
    if ad < bd {
        -1
    } else if ad > bd {
        1
    } else {
        0
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a.value_type(), b.value_type()) {
        (ValueType::Long, ValueType::Long) => a.as_long() == b.as_long(),
        (ValueType::String, ValueType::String) => a.as_str() == b.as_str(),
        (ValueType::Long, ValueType::Double)
        | (ValueType::Double, ValueType::Long)
        | (ValueType::Double, ValueType::Double) => a.to_double() == b.to_double(),
        (ValueType::Null, ValueType::Null) => true,
        (ValueType::True, ValueType::True) | (ValueType::False, ValueType::False) => true,
        (ValueType::Resource, ValueType::Resource) => a.as_resource_id() == b.as_resource_id(),
        (ValueType::String, ValueType::Long) | (ValueType::Long, ValueType::String) => {
            let (s_val, i_val) = if a.value_type() == ValueType::String {
                (a, b)
            } else {
                (b, a)
            };
            s_val.as_str().unwrap().parse::<i64>().ok() == i_val.as_long()
        }
        _ => false,
    }
}

fn var_dump_value(val: &Value, indent: usize, eg: &ExecutorGlobals) -> String {
    var_dump_value_inner(val, indent, eg, &mut std::collections::HashSet::new())
}

fn var_dump_value_inner(
    val: &Value,
    indent: usize,
    eg: &ExecutorGlobals,
    visited_objects: &mut std::collections::HashSet<usize>,
) -> String {
    let prefix = "  ".repeat(indent);
    match val.value_type() {
        ValueType::Null => format!("{}NULL\n", prefix),
        ValueType::True => format!("{}bool(true)\n", prefix),
        ValueType::False => format!("{}bool(false)\n", prefix),
        ValueType::Long => format!("{}int({})\n", prefix, val.as_long().unwrap()),
        ValueType::Double => format!("{}float({})\n", prefix, val.as_double().unwrap()),
        ValueType::String => {
            let s = val.as_str().unwrap();
            format!("{}string({}) \"{}\"\n", prefix, s.len(), s)
        }
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let mut out = format!("{}array({}) {{\n", prefix, arr.len());
            for (key, v) in arr.iter() {
                let key_str = match &key {
                    ArrayKey::Int(k) => format!("[{}]", k),
                    ArrayKey::String(k) => format!("[\"{}\"]", k),
                };
                out.push_str(&format!("{}  {}=>\n", prefix, key_str));
                out.push_str(&var_dump_value_inner(v, indent + 1, eg, visited_objects));
            }
            out.push_str(&format!("{}}}\n", prefix));
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
            let output = if eg
                .class_table
                .get(object.class_name.as_ref())
                .is_some_and(|class| class.is_enum)
            {
                let case = object
                    .get_property("name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!("{}enum({}::{})\n", prefix, object.class_name, case)
            } else {
                let mut properties = Vec::new();
                object.for_each_property(|name, value| {
                    properties.push((name.to_string(), value.clone()));
                });
                let mut out = format!(
                    "{}object({})#1 ({}) {{\n",
                    prefix,
                    object.class_name,
                    properties.len()
                );
                for (name, value) in properties {
                    out.push_str(&format!("{}  [\"{}\"]=>\n", prefix, name));
                    out.push_str(&var_dump_value_inner(
                        &value,
                        indent + 1,
                        eg,
                        visited_objects,
                    ));
                }
                out.push_str(&format!("{}}}\n", prefix));
                out
            };
            visited_objects.remove(&identity);
            output
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

fn print_r_value(val: &Value, indent: usize) -> String {
    match val.value_type() {
        ValueType::Null => String::new(),
        ValueType::True => "1".to_string(),
        ValueType::False => String::new(),
        ValueType::Long => val.as_long().unwrap().to_string(),
        ValueType::Double => {
            let d = val.as_double().unwrap();
            if d == d.floor() && d.abs() < 1e15 {
                format!("{}", d as i64)
            } else {
                format!("{}", d)
            }
        }
        ValueType::String => val.as_str().unwrap().to_string(),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let prefix = "    ".repeat(indent);
            let inner = "    ".repeat(indent + 1);
            let mut out = "Array\n".to_string();
            out.push_str(&format!("{}(\n", prefix));
            for (key, v) in arr.iter() {
                let key_str = match &key {
                    ArrayKey::Int(k) => format!("{}", k),
                    ArrayKey::String(k) => k.clone(),
                };
                out.push_str(&format!(
                    "{}[{}] => {}",
                    inner,
                    key_str,
                    print_r_value(v, indent + 1)
                ));
                if v.value_type() != ValueType::Array {
                    out.push('\n');
                }
            }
            out.push_str(&format!("{})\n", prefix));
            out
        }
        ValueType::Resource => val.echo_to_string(),
        _ => String::new(),
    }
}

fn var_export_value(val: &Value) -> String {
    match val.value_type() {
        ValueType::Null => "NULL".to_string(),
        ValueType::True => "true".to_string(),
        ValueType::False => "false".to_string(),
        ValueType::Long => val.as_long().unwrap().to_string(),
        ValueType::Double => format!("{}", val.as_double().unwrap()),
        ValueType::String => format!(
            "'{}'",
            val.as_str()
                .unwrap()
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
        ),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let mut out = "array (\n".to_string();
            for (key, v) in arr.iter() {
                let key_str = match &key {
                    ArrayKey::Int(k) => format!("{}", k),
                    ArrayKey::String(k) => format!("'{}'", k),
                };
                out.push_str(&format!("  {} => {},\n", key_str, var_export_value(v)));
            }
            out.push(')');
            out
        }
        _ => "NULL".to_string(),
    }
}

/// Simple JSON encoder
/// Convert a PHP Value to serde_json::Value for encoding.
fn value_to_json(val: &Value) -> serde_json::Value {
    match val.value_type() {
        ValueType::Null | ValueType::Undef => serde_json::Value::Null,
        ValueType::True => serde_json::Value::Bool(true),
        ValueType::False => serde_json::Value::Bool(false),
        ValueType::Long => {
            serde_json::Value::Number(serde_json::Number::from(val.as_long().unwrap()))
        }
        ValueType::Double => {
            let d = val.as_double().unwrap();
            if d.is_finite() {
                serde_json::Number::from_f64(d)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        ValueType::String => serde_json::Value::String(val.as_str().unwrap().to_string()),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let is_list = arr
                .iter()
                .enumerate()
                .all(|(i, (k, _))| matches!(k, ArrayKey::Int(n) if n == i as i64));
            if is_list {
                serde_json::Value::Array(arr.values().map(value_to_json).collect())
            } else {
                let mut map = serde_json::Map::new();
                for (k, v) in arr.iter() {
                    let key = match k {
                        ArrayKey::Int(n) => n.to_string(),
                        ArrayKey::String(s) => s,
                    };
                    map.insert(key, value_to_json(v));
                }
                serde_json::Value::Object(map)
            }
        }
        ValueType::Object => {
            if let Some(obj) = val.as_object() {
                let mut map = serde_json::Map::new();
                obj.for_each_property(|key, value| {
                    map.insert(key.to_string(), value_to_json(value));
                });
                serde_json::Value::Object(map)
            } else {
                serde_json::Value::Null
            }
        }
        _ => serde_json::Value::Null,
    }
}

fn json_encode_value(val: &Value) -> String {
    serde_json::to_string(&value_to_json(val)).unwrap_or_else(|_| "null".to_string())
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
    gen_ref: &crate::vm::generator::GeneratorRef,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;
    let state = gen_ref.borrow().state;
    if state == GeneratorState::Created {
        resume_generator_method(eg, gen_ref, Value::null())?;
    }
    Ok(())
}

/// Generator methods execute as internal calls. Preserve an escaped PHP
/// exception in the standard executor sidecar so `execute_full_call` can
/// inject it into the user caller after the handler returns.
fn resume_generator_method(
    eg: &mut ExecutorGlobals,
    gen_ref: &crate::vm::generator::GeneratorRef,
    send_value: Value,
) -> Result<(), VmError> {
    match crate::vm::execute::resume_generator(eg, gen_ref, send_value)? {
        crate::vm::execute::GeneratorResumeOutcome::Advanced => Ok(()),
        crate::vm::execute::GeneratorResumeOutcome::Threw(exception) => {
            eg.exception = Some(exception);
            Ok(())
        }
    }
}

fn fn_generator_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
        let val = gen_ref.borrow().value.clone();
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
        ensure_generator_started(&gen_ref, eg)?;
        let gen_data = gen_ref.borrow();
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
        ensure_generator_started(&gen_ref, eg)?;
        // Advance past current yield
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Suspended {
            resume_generator_method(eg, &gen_ref, Value::null())?;
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
        ensure_generator_started(&gen_ref, eg)?;
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        ret!(rv, Value::bool(is_valid));
    }
    ret!(rv, Value::bool(false));
}

fn fn_generator_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
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
            resume_generator_method(eg, &gen_ref, Value::null())?;
            // Now resume with the actual send value (if still suspended)
            let state2 = gen_ref.borrow().state;
            if state2 == crate::vm::generator::GeneratorState::Suspended {
                resume_generator_method(eg, &gen_ref, send_val)?;
            }
        } else if state == crate::vm::generator::GeneratorState::Suspended {
            resume_generator_method(eg, &gen_ref, send_val)?;
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        let gen_data = gen_ref.borrow();
        if gen_data.state != crate::vm::generator::GeneratorState::Completed {
            return Err(VmError::Fatal(
                "Cannot get return value of a generator that hasn't returned".into(),
            ));
        }
        ret!(rv, gen_data.return_value.clone());
    }
    ret!(rv, Value::null());
}

// ============================================================================
// Regex (PCRE) functions
// ============================================================================

/// preg_match($pattern, $subject [, &$matches]) -> int (0 or 1)
fn fn_preg_match(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);

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
        ret!(rv, Value::long(re.is_match(&subject) as i64));
    }

    match re.captures(&subject) {
        Some(caps) => {
            if has_matches {
                let matches_ptr = arg_mut!(ed, 2);
                let mut arr = PhpArray::new();
                for i in 0..caps.len() {
                    match caps.get(i) {
                        Some(m) => arr.push(Value::string(m.as_str(&subject))),
                        None => arr.push(Value::string("")),
                    }
                }
                // Add named capture groups as string-keyed entries
                for (name, &idx) in caps.named_groups() {
                    if let Some(m) = caps.get(idx) {
                        arr.set_str(name, Value::string(m.as_str(&subject)));
                    }
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

/// preg_replace($pattern, $replacement, $subject) -> string
fn fn_preg_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let replacement = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_e) => {
            ret!(rv, Value::null());
        }
    };

    let result = re.replace_all(&subject, &replacement);
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

        current = class
            .parent
            .as_deref()
            .and_then(|parent| find_class_case_insensitive(eg, parent));
    }
    None
}

/// Result of resolving a callback: func pointer + args to prepend (e.g. $this, use_vars).
#[derive(Clone)]
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
}

impl ResolvedCallback {
    #[inline]
    pub(crate) fn has_context(&self) -> bool {
        self.called_scope_class_id != 0 || self.bound_this.is_some()
    }
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
    // ed is the stdlib function's own frame; the caller is prev_execute_data
    let caller = unsafe { (*ed).prev_execute_data };
    if caller.is_null() {
        return None;
    }
    let func = unsafe { (*caller).func };
    if func.is_null() {
        return None;
    }
    eg.declaring_class_of(func)
}

/// Resolve a callback value to a function pointer.
/// Supports: string (function name), array [func_name, use_vars...] (closure),
/// array [object, "method"], and objects with __invoke.
/// `caller_class` is the class scope of the call site — used to allow
/// private/protected method callbacks when called from the declaring class.
fn resolve_callback(
    val: &Value,
    eg: &ExecutorGlobals,
    caller_class: Option<&str>,
) -> Option<ResolvedCallback> {
    match val.value_type() {
        ValueType::Closure => {
            let closure = val.as_closure().unwrap();
            Some(ResolvedCallback {
                func_ptr: closure.func,
                prepend_args: vec![],
                use_vars: closure.captures.clone(),
                called_scope_class_id: closure.called_scope_class_id,
                bound_this: closure.bound_this.clone(),
            })
        }
        ValueType::String => {
            let name = val.as_str().unwrap();
            eg.find_function(name).map(|ptr| ResolvedCallback {
                func_ptr: ptr,
                prepend_args: vec![],
                use_vars: vec![],
                called_scope_class_id: 0,
                bound_this: None,
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
            if let Some(obj) = obj_val.as_object() {
                // Instance method: [$obj, "method"]
                // Public: always callable. Private/protected: only from declaring scope.
                let class_name = obj.class_name.as_ref();
                let (visibility, _, func_ptr, declaring) =
                    find_method_in_class_hierarchy(eg, class_name, method_name)?;
                match visibility {
                    Visibility::Public => {}
                    Visibility::Protected => {
                        // Protected: caller must be in the same hierarchy
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(class_name, cc) || eg.class_is_a(cc, class_name)
                        });
                        if !allowed {
                            return None;
                        }
                    }
                    Visibility::Private => {
                        // Private: caller must be exactly the declaring class
                        let allowed =
                            caller_class.map_or(false, |cc| cc.eq_ignore_ascii_case(declaring));
                        if !allowed {
                            return None;
                        }
                    }
                }
                drop(obj);
                Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![obj_val.clone()],
                    use_vars: vec![],
                    called_scope_class_id: 0,
                    bound_this: None,
                })
            } else if let Some(class_str) = obj_val.as_str() {
                // Static method: ["ClassName", "method"] — must be static; visibility depends on scope
                let (visibility, is_static, func_ptr, declaring) =
                    find_method_in_class_hierarchy(eg, class_str, method_name)?;
                if !is_static {
                    return None;
                }
                match visibility {
                    Visibility::Public => {}
                    Visibility::Protected => {
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(class_str, cc) || eg.class_is_a(cc, class_str)
                        });
                        if !allowed {
                            return None;
                        }
                    }
                    Visibility::Private => {
                        let allowed =
                            caller_class.map_or(false, |cc| cc.eq_ignore_ascii_case(declaring));
                        if !allowed {
                            return None;
                        }
                    }
                }
                Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![Value::null()],
                    use_vars: vec![],
                    called_scope_class_id: 0,
                    bound_this: None,
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
            })
        }
        _ => None,
    }
}

/// Return the otherwise-unused DoFcall inline-cache entry belonging to the PHP
/// instruction that entered the current internal callback helper.
#[inline(always)]
fn callback_cache_slot(ed: *mut ExecuteData) -> Option<*mut InlineCache> {
    if ed.is_null() {
        return None;
    }
    let caller = unsafe { (*ed).prev_execute_data };
    if caller.is_null() {
        return None;
    }

    let func = unsafe { (*caller).func };
    if func.is_null() || unsafe { (*func).fn_type } != FunctionType::User {
        return None;
    }

    let op_array = unsafe { (*caller).op_array() };
    let opline = unsafe { (*caller).opline };
    let base = op_array.instructions.as_ptr();
    let byte_offset = (opline as usize).checked_sub(base as usize)?;
    if byte_offset % std::mem::size_of::<crate::vm::instruction::Instruction>() != 0 {
        return None;
    }
    let ip = byte_offset / std::mem::size_of::<crate::vm::instruction::Instruction>();
    if ip >= op_array.instructions.len() || unsafe { (*opline).opcode } != OpCode::DoFcall {
        return None;
    }

    Some(unsafe { op_array.cache.as_ptr().add(ip) as *mut InlineCache })
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
    Some(ResolvedCallback {
        func_ptr,
        prepend_args: vec![],
        use_vars: vec![],
        called_scope_class_id: 0,
        bound_this: None,
    })
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
    let mut cache_slot = if val.value_type() == ValueType::String {
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
#[inline]
pub(super) fn resolve_callback_at_callsite(
    val: &Value,
    eg: &ExecutorGlobals,
    ed: *mut ExecuteData,
) -> Option<ResolvedCallback> {
    let caller_class = if val.value_type() == ValueType::Array {
        get_calling_scope_class(ed, eg)
    } else {
        None
    };
    resolve_callback_with_cache(val, eg, caller_class, callback_cache_slot(ed))
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
    if !resolved.has_context() {
        call_function_iter(eg, resolved.func_ptr, num_args, args)
    } else {
        call_function_iter_with_context(
            eg,
            resolved.func_ptr,
            num_args,
            args,
            resolved.called_scope_class_id,
            resolved.bound_this.as_ref(),
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
    if !resolved.has_context() {
        call_function_owned_iter(eg, resolved.func_ptr, num_args, args)
    } else {
        call_function_owned_iter_with_context(
            eg,
            resolved.func_ptr,
            num_args,
            args,
            resolved.called_scope_class_id,
            resolved.bound_this.clone(),
        )
    }
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
    call_function_owned_iter_readback_arg0_with_context(
        eg,
        resolved.func_ptr,
        num_args,
        args,
        resolved.called_scope_class_id,
        resolved.bound_this.clone(),
    )
}

/// Invoke a resolved callback from a contiguous argument slice. Plain user
/// functions can enter the guarded scalar callback ABI, while internal
/// handlers retain their direct slice ABI and every other callable shape uses
/// the canonical receiver/capture-aware frame path.
#[inline]
pub(super) fn call_resolved_with_values(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    args: &[Value],
) -> Result<Value, VmError> {
    if resolved.prepend_args.is_empty() && resolved.use_vars.is_empty() && !resolved.has_context() {
        if let Some(result) =
            unsafe { try_execute_scalar_long_callback(resolved.func_ptr, args.len(), args.iter()) }
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

/// Invoke an already-resolved callback with PHP 8 call_user_func_array
/// positional/named argument semantics.
fn call_resolved_with_php_array(
    eg: &mut ExecutorGlobals,
    resolved: ResolvedCallback,
    args: &PhpArray,
) -> Result<Value, VmError> {
    if !args.has_string_keys() {
        return call_resolved_with_array(eg, &resolved, args);
    }

    let sig = unsafe { &(*resolved.func_ptr).sig };
    let param_names = &sig.param_names;
    let num_params = sig.public_arity() as usize;
    let required = sig.required_num_args as usize;

    let mut positional = vec![Value::undef(); num_params];
    let mut extra_positional: Vec<Value> = Vec::new();
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
                        positional[idx] = val.clone();
                    } else {
                        extra_positional.push(val.clone());
                    }
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
                    positional[pos_cursor] = val.clone();
                    pos_cursor += 1;
                } else {
                    extra_positional.push(val.clone());
                }
            }
        }
    }

    for i in 0..required {
        if positional[i].is_undef() {
            let name = param_names.get(i).map(|s| s.as_str()).unwrap_or("?");
            eg.exception = Some(crate::value::make_error_value(
                "ArgumentCountError",
                &format!(
                    "call_user_func_array(): Argument #{} (${}): not passed",
                    i + 1,
                    name
                ),
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
    call_resolved_owned_iter(
        eg,
        &resolved,
        num_args,
        resolved
            .prepend_args
            .iter()
            .cloned()
            .chain(normalized)
            .chain(resolved.use_vars.iter().cloned()),
    )
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
                "call_user_func_array(): Argument #2 ($args) must be of type array, given non-array",
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

    call_resolved_with_php_array(eg, resolved, args)
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

    let resolved = match resolve_callback_at_callsite(callback, eg, ed) {
        Some(r) => r,
        None => {
            let desc = callback.echo_to_string();
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "call_user_func(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found or not callable",
                    desc
                ),
            ));
            return Ok(());
        }
    };

    // Stream prepend args (e.g. $this), variadic values and closure captures
    // directly into the callback frame. No intermediate argument vectors.
    let result = if let Some(arr) = variadic_val.as_array() {
        call_resolved_with_array(eg, &resolved, arr)?
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
        )?
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
        )?
    };
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, result);
}

/// is_callable($value) — check if value is callable
fn fn_is_callable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let val = arg!(ed, 0);
    let callable = resolve_callback_at_callsite(val, eg, ed).is_some();
    ret!(rv, Value::bool(callable));
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
/// If $status is int → exit with that code.  If string → print it, exit 0.
fn fn_exit(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let status = arg_opt!(ed, 0);
    match status {
        None => Err(VmError::Exit(0)),
        Some(v) if v.value_type() == ValueType::Long => {
            Err(VmError::Exit(v.as_long().unwrap_or(0) as i32))
        }
        Some(v) => {
            // String argument: print it, exit 0
            print!("{}", v.echo_to_string());
            Err(VmError::Exit(0))
        }
    }
}

// ============================================================================
// Filesystem functions
// ============================================================================

/// file_get_contents($filename): string|false
/// PHP strings are byte strings. We use Latin-1 (byte→char 1:1) to preserve raw bytes
/// losslessly inside Rust String, pending a proper byte-string Value backend.
#[cfg(not(feature = "file-contents"))]
fn fn_file_get_contents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    match std::fs::read(path.as_ref()) {
        Ok(bytes) => ret!(rv, Value::string(bytes_to_php_string(&bytes))),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// Convert raw bytes to a Rust String preserving every byte losslessly.
/// Uses Latin-1 encoding: each byte 0x00-0xFF maps to the same Unicode codepoint.
/// This is the standard way to round-trip PHP byte strings through Rust Strings.
fn bytes_to_php_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Convert a PHP-style string back to raw bytes (inverse of bytes_to_php_string).
fn php_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

/// file_put_contents($filename, $data): int|false
/// Writes using Latin-1 byte mapping to preserve binary data round-trip.
#[cfg(not(feature = "file-write"))]
fn fn_file_put_contents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let data = arg_str!(ed, 1);
    let raw_bytes = php_string_to_bytes(data.as_ref());
    match std::fs::write(path.as_ref(), &raw_bytes) {
        Ok(()) => ret!(rv, Value::long(raw_bytes.len() as i64)),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// file_exists($filename): bool
fn fn_file_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).exists())
    );
}

/// is_file($filename): bool
fn fn_is_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).is_file())
    );
}

/// is_dir($filename): bool
fn fn_is_dir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(
        rv,
        Value::bool(std::path::Path::new(path.as_ref()).is_dir())
    );
}

/// dirname($path): string
fn fn_dirname(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    // PHP returns "." for paths without directory component, empty parent → "."
    let dir = if dir.is_empty() { ".".to_string() } else { dir };
    ret!(rv, Value::string(dir));
}

/// basename($path, $suffix = ""): string
fn fn_basename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let suffix = arg_opt!(ed, 1)
        .map(|v| v.echo_to_string())
        .unwrap_or_default();
    let p = std::path::Path::new(path.as_ref());
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result = if !suffix.is_empty() && name.ends_with(&suffix) {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name
    };
    ret!(rv, Value::string(result));
}

/// realpath($path): string|false
fn fn_realpath(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    match std::fs::canonicalize(path.as_ref()) {
        Ok(p) => ret!(rv, Value::string(p.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// getcwd(): string|false
fn fn_getcwd(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    match std::env::current_dir() {
        Ok(p) => ret!(rv, Value::string(p.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// file($filename): array|false — read file into array of lines
/// Uses Latin-1 mapping to preserve binary content losslessly.
#[cfg(not(feature = "file-lines"))]
fn fn_file(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    match std::fs::read(path.as_ref()) {
        Ok(bytes) => {
            let contents = bytes_to_php_string(&bytes);
            let mut arr = PhpArray::new();
            let mut start = 0;
            while start < contents.len() {
                match contents[start..].find('\n') {
                    Some(pos) => {
                        arr.push(Value::string(contents[start..start + pos + 1].to_string()));
                        start += pos + 1;
                    }
                    None => {
                        arr.push(Value::string(contents[start..].to_string()));
                        break;
                    }
                }
            }
            ret!(rv, Value::array(arr));
        }
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// mkdir($pathname, $mode = 0777, $recursive = false): bool
fn fn_mkdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let recursive = arg_opt!(ed, 2).map(|v| v.is_truthy()).unwrap_or(false);
    let result = if recursive {
        std::fs::create_dir_all(path.as_ref())
    } else {
        std::fs::create_dir(path.as_ref())
    };
    ret!(rv, Value::bool(result.is_ok()));
}

/// rmdir($dirname): bool
fn fn_rmdir(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(rv, Value::bool(std::fs::remove_dir(path.as_ref()).is_ok()));
}

/// unlink($filename): bool
fn fn_unlink(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    ret!(rv, Value::bool(std::fs::remove_file(path.as_ref()).is_ok()));
}

/// rename($old, $new): bool
fn fn_rename(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let old = arg_str!(ed, 0);
    let new = arg_str!(ed, 1);
    ret!(
        rv,
        Value::bool(std::fs::rename(old.as_ref(), new.as_ref()).is_ok())
    );
}

/// copy($source, $dest): bool
fn fn_copy(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let src = arg_str!(ed, 0);
    let dst = arg_str!(ed, 1);
    ret!(
        rv,
        Value::bool(std::fs::copy(src.as_ref(), dst.as_ref()).is_ok())
    );
}

/// tempnam($dir, $prefix): string|false
fn fn_tempnam(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = arg_str!(ed, 0);
    let prefix = arg_str!(ed, 1);
    let dir_path = std::path::Path::new(dir.as_ref());
    if !dir_path.is_dir() {
        ret!(rv, Value::bool(false));
    }
    // Generate a unique filename: prefix + pid + atomic counter
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}{}{}", prefix, std::process::id(), seq);
    let path = dir_path.join(&name);
    match std::fs::File::create(&path) {
        Ok(_) => ret!(rv, Value::string(path.to_string_lossy().into_owned())),
        Err(_) => ret!(rv, Value::bool(false)),
    }
}

/// sys_get_temp_dir(): string
fn fn_sys_get_temp_dir(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::string(std::env::temp_dir().to_string_lossy().into_owned())
    );
}

/// pathinfo($path, $flags = PATHINFO_ALL): array|string
fn fn_pathinfo(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    let dirname = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let dirname = if dirname.is_empty() {
        ".".to_string()
    } else {
        dirname
    };
    let basename_str = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = p
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut arr = PhpArray::new();
    arr.set_str("dirname", Value::string(dirname));
    arr.set_str("basename", Value::string(basename_str));
    arr.set_str("extension", Value::string(extension));
    arr.set_str("filename", Value::string(filename));
    ret!(rv, Value::array(arr));
}

/// is_readable($filename): bool
fn fn_is_readable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    // Simple check: file exists and we can open it for reading
    ret!(rv, Value::bool(std::fs::File::open(p).is_ok()));
}

/// is_writable($filename): bool / is_writeable()
fn fn_is_writable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let p = std::path::Path::new(path.as_ref());
    let writable = if p.is_dir() {
        // For directories: try creating a temp file inside
        let probe = p.join(format!(".rphp_writable_probe_{}", std::process::id()));
        match std::fs::File::create(&probe) {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    } else {
        // For files: try opening for append (non-destructive)
        std::fs::OpenOptions::new().append(true).open(p).is_ok()
    };
    ret!(rv, Value::bool(writable));
}

/// glob($pattern): array|false
fn fn_glob(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let pattern = arg_str!(ed, 0);
    let pat = pattern.as_ref();
    let mut arr = PhpArray::new();

    // Split pattern into directory and filename parts
    let (dir, file_pat) = match pat.rfind('/') {
        Some(pos) => (&pat[..pos], &pat[pos + 1..]),
        None => (".", pat),
    };
    let dir = if dir.is_empty() { "/" } else { dir };

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut results: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if glob_match(file_pat, &name) {
                // Return full path (dir + name) when pattern had directory component
                if pat.contains('/') {
                    results.push(format!("{}/{}", dir, name));
                } else {
                    results.push(name);
                }
            }
        }
        results.sort(); // PHP glob returns sorted results
        for r in results {
            arr.push(Value::string(r));
        }
    }
    ret!(rv, Value::array(arr));
}

/// Simple glob matcher for *, ? patterns (no full POSIX glob)
fn glob_match(pattern: &str, text: &str) -> bool {
    let pi: Vec<char> = pattern.chars().collect();
    let ti: Vec<char> = text.chars().collect();
    glob_match_inner(&pi, 0, &ti, 0)
}

fn glob_match_inner(pat: &[char], pi: usize, txt: &[char], ti: usize) -> bool {
    if pi == pat.len() && ti == txt.len() {
        return true;
    }
    if pi == pat.len() {
        return false;
    }
    match pat[pi] {
        '*' => {
            // Match zero or more characters
            for skip in 0..=(txt.len() - ti) {
                if glob_match_inner(pat, pi + 1, txt, ti + skip) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < txt.len() {
                glob_match_inner(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < txt.len() && txt[ti] == c {
                glob_match_inner(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
    }
}

// ============================================================================
// String encoding functions
// ============================================================================

/// htmlspecialchars($string, $flags = ENT_QUOTES|ENT_SUBSTITUTE): string
fn fn_htmlspecialchars(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    ret!(rv, Value::string(out));
}

/// htmlspecialchars_decode($string): string
/// Decodes only one layer — `&amp;lt;` becomes `&lt;`, not `<`.
fn fn_htmlspecialchars_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    // Single-pass decode to avoid chaining issues (e.g. &amp;lt; → &lt; not <)
    let src = s.as_ref();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if src[i..].starts_with("&amp;") {
                out.push('&');
                i += 5;
            } else if src[i..].starts_with("&quot;") {
                out.push('"');
                i += 6;
            } else if src[i..].starts_with("&#039;") {
                out.push('\'');
                i += 6;
            } else if src[i..].starts_with("&lt;") {
                out.push('<');
                i += 4;
            } else if src[i..].starts_with("&gt;") {
                out.push('>');
                i += 4;
            } else {
                out.push('&');
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    ret!(rv, Value::string(out));
}

/// htmlentities($string): string — same as htmlspecialchars for basic usage
fn fn_htmlentities(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_htmlspecialchars(ed, rv, eg)
}

/// urlencode($string): string
fn fn_urlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let extra_bytes = s
        .bytes()
        .filter(
            |b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b' '),
        )
        .count()
        * 2;
    let mut out = String::with_capacity(s.len() + extra_bytes);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => push_percent_escape(&mut out, *b),
        }
    }
    ret!(rv, Value::string(out));
}

/// urldecode($string): string
fn fn_urldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(percent_decode_bytes(&s, true)));
}

/// rawurlencode($string): string — like urlencode but space → %20
fn fn_rawurlencode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let extra_bytes = s
        .bytes()
        .filter(
            |b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'),
        )
        .count()
        * 2;
    let mut out = String::with_capacity(s.len() + extra_bytes);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => push_percent_escape(&mut out, *b),
        }
    }
    ret!(rv, Value::string(out));
}

/// rawurldecode($string): string
fn fn_rawurldecode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(percent_decode_bytes(&s, false)));
}

/// base64_encode($data): string
/// Uses Latin-1 byte mapping to handle binary PHP strings correctly.
fn fn_base64_encode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    use crate::base64;
    let raw = php_string_to_bytes(s.as_ref());
    ret!(rv, Value::string(base64::encode(&raw)));
}

/// base64_decode($data): string|false
fn fn_base64_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    use crate::base64;
    match base64::decode(s.as_ref()) {
        Some(bytes) => ret!(rv, Value::string(bytes_to_php_string(&bytes))),
        None => ret!(rv, Value::bool(false)),
    }
}

// ============================================================================
// Missing common string functions
// ============================================================================

/// stripos($haystack, $needle): int|false
fn fn_stripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hay = arg_str!(ed, 0);
    let needle = arg_str!(ed, 1);
    let hay_lower = hay.to_lowercase();
    let needle_lower = needle.to_lowercase();
    match hay_lower.find(&needle_lower) {
        Some(pos) => {
            // Convert byte offset to char offset for consistency
            let char_pos = hay[..pos].chars().count();
            ret!(rv, Value::long(char_pos as i64));
        }
        None => ret!(rv, Value::bool(false)),
    }
}

/// strripos($haystack, $needle): int|false
fn fn_strripos(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hay = arg_str!(ed, 0);
    let needle = arg_str!(ed, 1);
    let hay_lower = hay.to_lowercase();
    let needle_lower = needle.to_lowercase();
    match hay_lower.rfind(&needle_lower) {
        Some(pos) => ret!(rv, Value::long(hay[..pos].chars().count() as i64)),
        None => ret!(rv, Value::bool(false)),
    }
}

/// str_ireplace($search, $replace, $subject): string
fn fn_str_ireplace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let search = arg_str!(ed, 0);
    let replace = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);
    if search.is_empty() {
        ret!(rv, Value::string(subject.into_owned()));
    }
    // Case-insensitive replace
    let search_lower = search.to_lowercase();
    let mut result = String::with_capacity(subject.len());
    let subject_lower = subject.to_lowercase();
    let mut start = 0;
    while let Some(pos) = subject_lower[start..].find(&search_lower) {
        result.push_str(&subject[start..start + pos]);
        result.push_str(replace.as_ref());
        start += pos + search.len();
    }
    result.push_str(&subject[start..]);
    ret!(rv, Value::string(result));
}

/// substr_replace($string, $replacement, $start, $length = null): string
fn fn_substr_replace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let replacement = arg_str!(ed, 1);
    let start_raw = arg_long!(ed, 2);
    let len = s.len() as i64;
    let start = if start_raw < 0 {
        (len + start_raw).max(0) as usize
    } else {
        start_raw.min(len) as usize
    };
    let length = match arg_opt!(ed, 3) {
        Some(v) if !v.is_undef() => {
            let l = v.to_long_val();
            if l < 0 {
                ((len as i64 - start as i64) + l).max(0) as usize
            } else {
                l as usize
            }
        }
        _ => s.len() - start,
    };
    let end = (start + length).min(s.len());
    let mut result = String::with_capacity(start + replacement.len() + (s.len() - end));
    result.push_str(&s[..start]);
    result.push_str(replacement.as_ref());
    result.push_str(&s[end..]);
    ret!(rv, Value::string(result));
}

/// str_getcsv($string, $separator = ",", $enclosure = "\"", $escape = "\\"): array
fn fn_str_getcsv(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let sep = arg_opt!(ed, 1)
        .map(|v| v.echo_to_string().chars().next().unwrap_or(','))
        .unwrap_or(',');
    let enc = arg_opt!(ed, 2)
        .map(|v| v.echo_to_string().chars().next().unwrap_or('"'))
        .unwrap_or('"');

    let mut arr = PhpArray::new();
    let mut field = String::new();
    let mut in_enclosure = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == enc {
            if in_enclosure {
                if chars.peek() == Some(&enc) {
                    field.push(enc);
                    chars.next();
                } else {
                    in_enclosure = false;
                }
            } else {
                in_enclosure = true;
            }
        } else if c == sep && !in_enclosure {
            arr.push(Value::string(std::mem::take(&mut field)));
        } else {
            field.push(c);
        }
    }
    arr.push(Value::string(field));
    ret!(rv, Value::array(arr));
}

/// chunk_split($string, $chunklen = 76, $end = "\r\n"): string
fn direct_chunk_split(args: &[Value]) -> Result<Value, VmError> {
    let s = direct_arg_str(args, 0);
    let chunklen = direct_arg_opt(args, 1)
        .map(|v| v.to_long_val() as usize)
        .unwrap_or(76);
    let end = direct_arg_opt(args, 2)
        .map(|v| v.echo_to_string())
        .unwrap_or_else(|| "\r\n".to_string());
    if chunklen == 0 {
        return Err(VmError::Fatal(
            "chunk_split(): Argument #2 ($chunklen) must be greater than 0".into(),
        ));
    }
    let mut result = String::new();
    for chunk in s.as_bytes().chunks(chunklen) {
        result.push_str(&String::from_utf8_lossy(chunk));
        result.push_str(&end);
    }
    Ok(Value::string(result))
}

fn fn_chunk_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let args = unsafe { std::slice::from_raw_parts((*ed).cv(0), 3) };
    let result = direct_chunk_split(args)?;
    ret!(rv, result);
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
    if !resolved.prepend_args.is_empty() || !resolved.use_vars.is_empty() || resolved.has_context()
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

fn fn_usort(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    // Save raw pointer to array BEFORE any call_function.
    // call_function may push/pop VM stack frames but the by-ref pointer target
    // lives in the CALLER's frame (which is below us on the stack and stays valid).
    let arr_ptr: *mut Value = arg_mut!(ed, 0);
    let callback = arg!(ed, 1).clone();

    let items = {
        let arr = unsafe { &*arr_ptr };
        match arr.as_array() {
            Some(a) => a.values().cloned().collect::<Vec<Value>>(),
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
                new_arr.push(value);
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
                .cloned()
                .collect();
        }
    }

    // Insertion sort with PHP callback comparison
    let len = items.len();
    for i in 1..len {
        let mut j = i;
        while j > 0 {
            let num_args = resolved.prepend_args.len() + 2 + resolved.use_vars.len();
            let result = call_resolved_iter(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .chain(std::iter::once(&items[j - 1]))
                    .chain(std::iter::once(&items[j]))
                    .chain(resolved.use_vars.iter()),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if result.to_long_val() <= 0 {
                break;
            }
            items.swap(j - 1, j);
            j -= 1;
        }
    }
    let mut new_arr = PhpArray::new();
    for v in items {
        new_arr.push(v);
    }
    // Write back using saved raw pointer (stable across call_function calls).
    unsafe {
        *arr_ptr = Value::array(new_arr);
    }
    ret!(rv, Value::bool(true));
}

/// array_diff($array1, $array2): array
fn fn_array_diff(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr1 = arg!(ed, 0);
    let arr2 = arg!(ed, 1);

    if let (Some(a1), Some(a2)) = (arr1.as_array(), arr2.as_array()) {
        let mut result = PhpArray::new();
        let vals2: Vec<String> = a2.values().map(Value::echo_to_string).collect();
        for (k, v) in a1.iter() {
            let vs = v.echo_to_string();
            if !vals2.iter().any(|v2| *v2 == vs) {
                match k {
                    ArrayKey::Int(i) => result.set_int(i, v.clone()),
                    ArrayKey::String(s) => result.set_str(&s, v.clone()),
                }
            }
        }
        ret!(rv, Value::array(result));
    }
    ret!(rv, Value::array(PhpArray::new()));
}

/// array_intersect($array1, $array2): array
fn fn_array_intersect(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr1 = arg!(ed, 0);
    let arr2 = arg!(ed, 1);

    if let (Some(a1), Some(a2)) = (arr1.as_array(), arr2.as_array()) {
        let mut result = PhpArray::new();
        let vals2: Vec<String> = a2.values().map(Value::echo_to_string).collect();
        for (k, v) in a1.iter() {
            let vs = v.echo_to_string();
            if vals2.iter().any(|v2| *v2 == vs) {
                match k {
                    ArrayKey::Int(i) => result.set_int(i, v.clone()),
                    ArrayKey::String(s) => result.set_str(&s, v.clone()),
                }
            }
        }
        ret!(rv, Value::array(result));
    }
    ret!(rv, Value::array(PhpArray::new()));
}

/// array_walk(&$array, $callback): bool
/// Supports by-ref callbacks: function (&$val, $key) { $val *= 2; }
#[inline(never)]
unsafe fn try_array_walk_scalar_long(arr: &PhpArray, resolved: &ResolvedCallback) -> Option<()> {
    if !resolved.prepend_args.is_empty() || !resolved.use_vars.is_empty() || resolved.has_context()
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
    let arr_ptr: *mut Value = arg_mut!(ed, 0);

    let arr = match unsafe { &*arr_ptr }.as_array() {
        Some(arr) => arr,
        None => {
            ret!(rv, Value::bool(false));
        }
    };

    let resolved = match resolve_callback_at_callsite(&callback, eg, ed) {
        Some(r) => r,
        None => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                "array_walk(): Argument #2 ($callback) must be a valid callback",
            ));
            return Ok(());
        }
    };

    // A pure by-value callback cannot observe the discarded return values or
    // mutate the walked array. Packed Long members and integer keys can use
    // the shared scalar callback ABI without cloning a snapshot or frames.
    if unsafe { try_array_walk_scalar_long(arr, &resolved) }.is_some() {
        ret!(rv, Value::bool(true));
    }

    let pairs = arr
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<Vec<_>>();

    // Check if callback's first parameter is declared by-reference.
    let cb_arg0_by_ref = unsafe { (*resolved.func_ptr).sig.is_param_by_ref(0) };

    if cb_arg0_by_ref {
        // By-ref callback: read back CV(0) after each call and rebuild the array.
        let mut mutations: Vec<(ArrayKey, Value)> = Vec::new();
        for (k, v) in pairs {
            let key_val = match &k {
                ArrayKey::Int(i) => Value::long(*i),
                ArrayKey::String(s) => Value::string(s.clone()),
            };
            let num_args = resolved.prepend_args.len() + 2 + resolved.use_vars.len();
            let (_ret, modified_val) = call_resolved_owned_iter_readback_arg0(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(std::iter::once(v))
                    .chain(std::iter::once(key_val))
                    .chain(resolved.use_vars.iter().cloned()),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            mutations.push((k, modified_val));
        }
        let mut new_arr = PhpArray::new();
        for (k, v) in mutations {
            match k {
                ArrayKey::Int(i) => new_arr.set_int(i, v),
                ArrayKey::String(s) => new_arr.set_str(&s, v),
            }
        }
        unsafe {
            *arr_ptr = Value::array(new_arr);
        }
    } else {
        // By-value callback: call without readback, array stays unchanged.
        for (k, v) in pairs {
            let key_val = match &k {
                ArrayKey::Int(i) => Value::long(*i),
                ArrayKey::String(s) => Value::string(s.clone()),
            };
            let num_args = resolved.prepend_args.len() + 2 + resolved.use_vars.len();
            call_resolved_owned_iter(
                eg,
                &resolved,
                num_args,
                resolved
                    .prepend_args
                    .iter()
                    .cloned()
                    .chain(std::iter::once(v))
                    .chain(std::iter::once(key_val))
                    .chain(resolved.use_vars.iter().cloned()),
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
        }
    }
    ret!(rv, Value::bool(true));
}

/// asort(&$array): bool — sort by value, preserve keys
fn fn_asort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(_, a), (_, b)| cmp_val(compare_values(a, b)));
        let mut new_arr = PhpArray::new();
        for (k, v) in pairs {
            match k {
                ArrayKey::Int(i) => new_arr.set_int(i, v),
                ArrayKey::String(s) => new_arr.set_str(&s, v),
            }
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(_, a), (_, b)| cmp_val(compare_values(b, a)));
        let mut new_arr = PhpArray::new();
        for (k, v) in pairs {
            match k {
                ArrayKey::Int(i) => new_arr.set_int(i, v),
                ArrayKey::String(s) => new_arr.set_str(&s, v),
            }
        }
        *arr = Value::array(new_arr);
        ret!(rv, Value::bool(true));
    }
    ret!(rv, Value::bool(false));
}

/// ksort(&$array): bool — sort by key
fn fn_ksort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(a, _), (b, _)| match (a, b) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => x.cmp(y),
            (ArrayKey::String(x), ArrayKey::String(y)) => x.cmp(y),
            (ArrayKey::Int(_), ArrayKey::String(_)) => std::cmp::Ordering::Less,
            (ArrayKey::String(_), ArrayKey::Int(_)) => std::cmp::Ordering::Greater,
        });
        let mut new_arr = PhpArray::new();
        for (k, v) in pairs {
            match k {
                ArrayKey::Int(i) => new_arr.set_int(i, v),
                ArrayKey::String(s) => new_arr.set_str(&s, v),
            }
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(a, _), (b, _)| match (b, a) {
            (ArrayKey::Int(x), ArrayKey::Int(y)) => x.cmp(y),
            (ArrayKey::String(x), ArrayKey::String(y)) => x.cmp(y),
            (ArrayKey::Int(_), ArrayKey::String(_)) => std::cmp::Ordering::Less,
            (ArrayKey::String(_), ArrayKey::Int(_)) => std::cmp::Ordering::Greater,
        });
        let mut new_arr = PhpArray::new();
        for (k, v) in pairs {
            match k {
                ArrayKey::Int(i) => new_arr.set_int(i, v),
                ArrayKey::String(s) => new_arr.set_str(&s, v),
            }
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
    ret!(rv, Value::string(format_php_date(&fmt, ts)));
}

/// Format a Unix timestamp according to PHP date() format characters
fn format_php_date(fmt: &str, ts: i64) -> String {
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

/// phpversion(): string
fn fn_phpversion(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string("8.4.0".to_string()));
}

/// PHP_INT_SIZE, PHP_INT_MAX etc. are handled as constants.
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

/// compact(...$var_names): array  — can't access caller scope; stub returns empty array
/// (Noted in registration as intentionally limited)

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
    let caller_class = get_calling_scope_class(ed, eg).map(str::to_owned);
    let result = invoke_call_user_func_array(
        callback,
        args_val,
        eg,
        caller_class.as_deref(),
        callback_cache_slot(ed),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    ret!(rv, result);
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

/// parse_url($url, $component = -1): mixed
fn fn_parse_url(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let url = arg_str!(ed, 0);
    let component = arg_opt!(ed, 1).map(|v| v.to_long_val()).unwrap_or(-1);

    // Manual URL parse — matches PHP's parse_url() behavior.
    // Handles:  scheme://[user[:pass]@]host[:port][/path][?query][#fragment]
    //           scheme:opaque_path[?query][#fragment]   (mailto:, tel:, news:, …)
    //           //host/path  (protocol-relative)
    //           /path?query  (relative)
    let s = url.as_ref();
    let mut rest = s;

    // Detect scheme — a sequence of [A-Za-z][A-Za-z0-9+.-]* followed by ':'
    let (scheme, has_authority) = if let Some(colon) = rest.find(':') {
        let candidate = &rest[..colon];
        let valid_scheme = !candidate.is_empty()
            && candidate.as_bytes()[0].is_ascii_alphabetic()
            && candidate
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-');
        if valid_scheme {
            let after_colon = &rest[colon + 1..];
            if after_colon.starts_with("//") {
                // scheme://authority...
                rest = &after_colon[2..];
                (Some(candidate.to_string()), true)
            } else {
                // Opaque scheme (mailto:path, tel:number, etc.)
                rest = after_colon;
                (Some(candidate.to_string()), false)
            }
        } else if rest.starts_with("//") {
            rest = &rest[2..];
            (None, true)
        } else {
            (None, false)
        }
    } else if rest.starts_with("//") {
        rest = &rest[2..];
        (None, true)
    } else {
        (None, false)
    };

    // Fragment (split early — # can appear in query too, but PHP splits on first #)
    let fragment = if let Some(idx) = rest.find('#') {
        let f = rest[idx + 1..].to_string();
        rest = &rest[..idx];
        Some(f)
    } else {
        None
    };

    // Query
    let query = if let Some(idx) = rest.find('?') {
        let q = rest[idx + 1..].to_string();
        rest = &rest[..idx];
        Some(q)
    } else {
        None
    };

    // Authority vs path
    let (user, pass, host, port, path);
    if has_authority {
        // Split authority from path at first /
        let (authority, p) = if let Some(idx) = rest.find('/') {
            (&rest[..idx], Some(rest[idx..].to_string()))
        } else {
            (rest, None)
        };
        path = p;

        // user:pass@host:port
        let (userinfo, hostport) = if let Some(idx) = authority.rfind('@') {
            (Some(&authority[..idx]), &authority[idx + 1..])
        } else {
            (None, authority)
        };

        if let Some(ui) = userinfo {
            if let Some(idx) = ui.find(':') {
                user = Some(ui[..idx].to_string());
                pass = Some(ui[idx + 1..].to_string());
            } else {
                user = Some(ui.to_string());
                pass = None;
            }
        } else {
            user = None;
            pass = None;
        }

        // host[:port]
        if let Some(idx) = hostport.rfind(':') {
            let port_str = &hostport[idx + 1..];
            if let Ok(p) = port_str.parse::<i64>() {
                host = Some(hostport[..idx].to_string());
                port = Some(p);
            } else {
                host = Some(hostport.to_string());
                port = None;
            }
        } else {
            host = if hostport.is_empty() {
                None
            } else {
                Some(hostport.to_string())
            };
            port = None;
        }
    } else {
        // No authority — rest is the path (opaque URI or relative)
        user = None;
        pass = None;
        host = None;
        port = None;
        path = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
    }

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

fn percent_decode_bytes(s: &str, plus_as_space: bool) -> String {
    let bytes = s.as_bytes();
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
    match String::from_utf8(out) {
        Ok(decoded) => decoded,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

/// Helper: decode an application/x-www-form-urlencoded string.
fn percent_decode(s: &str) -> String {
    percent_decode_bytes(s, true)
}

/// PHP normalizes dots and spaces in top-level query variable names to underscores.
fn parse_str_normalize_key(key: &str) -> String {
    key.chars()
        .map(|c| if c == '.' || c == ' ' { '_' } else { c })
        .collect()
}

/// Parse bracket segments from a key like `a[b][c][]`.
/// Returns (base_key, vec_of_segments) where each segment is Some("key") or None for [].
fn parse_str_brackets(full_key: &str) -> (String, Vec<Option<String>>) {
    if let Some(bracket_pos) = full_key.find('[') {
        let base = parse_str_normalize_key(&full_key[..bracket_pos]);
        let rest = &full_key[bracket_pos..];
        let mut segments = Vec::new();
        let mut i = 0;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'[' {
                if let Some(close) = rest[i + 1..].find(']') {
                    let inner = &rest[i + 1..i + 1 + close];
                    if inner.is_empty() {
                        segments.push(None); // []
                    } else {
                        segments.push(Some(inner.to_string()));
                    }
                    i = i + 2 + close;
                } else {
                    // Malformed — no closing bracket; treat rest as literal
                    segments.push(Some(rest[i..].to_string()));
                    break;
                }
            } else {
                i += 1;
            }
        }
        (base, segments)
    } else {
        (parse_str_normalize_key(full_key), vec![])
    }
}

/// Recursively set a value in a nested PhpArray given a chain of bracket segments.
fn parse_str_set_nested(arr: &mut PhpArray, segments: &[Option<String>], val: Value) {
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
            Some(k) => {
                arr.set_str(k, val);
            }
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
            Some(k) => {
                let mut sub = if let Some(existing) = arr.get_str(k) {
                    existing.as_array().cloned().unwrap_or_else(PhpArray::new)
                } else {
                    PhpArray::new()
                };
                parse_str_set_nested(&mut sub, remaining, val);
                arr.set_str(k, Value::array(sub));
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 0);
    let out_ptr = arg_mut!(ed, 1);

    let mut arr = PhpArray::new();
    if !input.is_empty() {
        for pair in input.as_ref().split('&') {
            if pair.is_empty() {
                continue;
            }
            let (raw_key, val) = if let Some(idx) = pair.find('=') {
                (
                    percent_decode(&pair[..idx]),
                    percent_decode(&pair[idx + 1..]),
                )
            } else {
                (percent_decode(pair), String::new())
            };

            let (base, segments) = parse_str_brackets(&raw_key);
            if segments.is_empty() {
                // Simple key — no brackets
                arr.set_str(&base, Value::string(val));
            } else {
                // Nested key — get-or-create the base sub-array, then recurse
                let mut sub = if let Some(existing) = arr.get_str(&base) {
                    existing.as_array().cloned().unwrap_or_else(PhpArray::new)
                } else {
                    PhpArray::new()
                };
                parse_str_set_nested(&mut sub, &segments, Value::string(val));
                arr.set_str(&base, Value::array(sub));
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

/// preg_match_all($pattern, $subject, &$matches = null): int
fn fn_preg_match_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);

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

    // PHP default: PREG_PATTERN_ORDER — matches[0] contains every full
    // match, matches[1] every group 1 match, and so on. Fill those arrays
    // directly while the regex visitor lends each reusable capture buffer.
    let mut result_arrays: Option<Vec<PhpArray>> = None;
    let mut named_arrays: Vec<(String, usize, PhpArray)> = Vec::new();
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
            match caps.get(index) {
                Some(capture) => array.push(Value::string(capture.as_str(&subject))),
                None => array.push(Value::string("")),
            }
        }
        for (_, index, array) in &mut named_arrays {
            match caps.get(*index) {
                Some(capture) => array.push(Value::string(capture.as_str(&subject))),
                None => array.push(Value::string("")),
            }
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
    let matches_ptr = arg_mut!(ed, 2);
    unsafe {
        std::ptr::drop_in_place(matches_ptr);
        matches_ptr.write(Value::array(out));
    }

    ret!(rv, Value::long(count as i64));
}

/// preg_split($pattern, $subject, $limit = -1): array|false
fn fn_preg_split(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);
    let limit = arg_opt!(ed, 2).map(|v| v.to_long_val()).unwrap_or(-1);

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_) => {
            ret!(rv, Value::bool(false));
        }
    };

    let parts = re.split(&subject, limit);
    let mut arr = PhpArray::new();
    for part in parts {
        arr.push(Value::string(part));
    }
    ret!(rv, Value::array(arr));
}

/// preg_replace_callback($pattern, $callback, $subject): string|null
fn fn_preg_replace_callback(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let callback = arg!(ed, 1).clone();
    let subject = arg_str!(ed, 2).into_owned();

    let re = match eg.regex_cache.get_or_compile(&pattern_str) {
        Ok(regex) => regex,
        Err(_) => {
            ret!(rv, Value::null());
        }
    };

    let Some(result) = regex_callback::replace(&re, subject, &callback, ed, eg)? else {
        return Ok(());
    };

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
