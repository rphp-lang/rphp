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

use crate::compiler::compile::{ClassConstantDefinition, PropertyDefinition};
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
    prepare_scalar_long_callback, try_execute_scalar_long_callback, values_identical,
};
use crate::vm::frame::ExecuteData;
use crate::vm::function::InternalFunction;
use crate::vm::function::{Function, FunctionCommon, FunctionType, ParamTypeHint};
use crate::vm::instruction::InlineCache;
use crate::vm::opcode::OpCode;

#[cfg(feature = "include-path")]
pub(crate) mod include_path;
mod json_decode;
mod reflection;
mod regex_callback;
mod serialization;
mod tokenizer;

const BUILTIN_EXCEPTION_SUBCLASSES: &[(&str, &str)] = &[
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
    reg!(
        "in_array",
        fn_in_array,
        3,
        2,
        "needle",
        "haystack",
        "strict"
    );
    reg!("array_reverse", fn_array_reverse, 1, 1, "array");
    reg!("array_is_list", fn_array_is_list, 1, 1, "array");
    reg_var!("array_merge", fn_array_merge, 0, "arrays");
    reg_var!("array_replace", fn_array_replace, 1, "array");
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
    reg_ref!("sort", fn_sort, 2, 1, 0b1, "array", "flags");
    reg_ref!("rsort", fn_rsort, 2, 1, 0b1, "array", "flags");
    reg!(
        "array_search",
        fn_array_search,
        3,
        2,
        "needle",
        "haystack",
        "strict"
    );
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
    // S3 exposes xxh128, including the raw-output path used by Symfony's
    // deterministic service identifiers. The wider algorithm catalogue stays
    // explicit compatibility work rather than returning invented digests.
    reg!("hash", fn_hash, 3, 2, "algo", "data", "binary");
    reg!("hash_init", fn_hash_init, 1, 1, "algo");
    reg!("hash_update", fn_hash_update, 2, 2, "context", "data");
    reg!("hash_final", fn_hash_final, 2, 1, "context", "binary");
    reg!("serialize", serialization::serialize, 1, 1, "value");
    reg!(
        "unserialize",
        serialization::unserialize,
        2,
        1,
        "data",
        "options"
    );
    reg!(
        "token_get_all",
        tokenizer::token_get_all,
        2,
        1,
        "code",
        "flags"
    );
    reg!("substr", fn_substr, 3, 2, "string", "offset", "length");
    reg!("strnatcmp", fn_strnatcmp, 2, 2, "string1", "string2");
    reg!(
        "substr_compare",
        fn_substr_compare,
        5,
        3,
        "haystack",
        "needle",
        "offset",
        "length",
        "case_insensitive"
    );
    reg!("strpos", fn_strpos, 3, 2, "haystack", "needle", "offset");
    reg!("strrpos", fn_strrpos, 2, 2, "haystack", "needle");
    reg!("strrchr", fn_strrchr, 2, 2, "haystack", "needle");
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
    reg!("addcslashes", fn_addcslashes, 2, 2, "string", "characters");
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
    reg!("trim", fn_trim, 2, 1, "string", "characters");
    reg!("rtrim", fn_rtrim, 2, 1, "string", "characters");
    reg!("ltrim", fn_ltrim, 2, 1, "string", "characters");
    reg!("explode", fn_explode, 3, 2, "separator", "string", "limit");
    reg!("implode", fn_implode, 2, 2, "separator", "array");
    reg!("join", fn_implode, 2, 2, "separator", "array");
    reg!("str_repeat", fn_str_repeat, 2, 2, "string", "times");
    reg!("substr_count", fn_substr_count, 2, 2, "haystack", "needle");
    reg!(
        "strspn",
        fn_strspn,
        4,
        2,
        "string",
        "characters",
        "offset",
        "length"
    );
    reg!(
        "strcspn",
        fn_strcspn,
        4,
        2,
        "string",
        "characters",
        "offset",
        "length"
    );
    reg!("strpbrk", fn_strpbrk, 2, 2, "string", "characters");
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
    reg!("ucwords", fn_ucwords, 2, 1, "string", "separators");
    reg!("str_word_count", fn_str_word_count, 1, 1, "string");
    reg!(
        "levenshtein",
        fn_levenshtein,
        5,
        2,
        "string1",
        "string2",
        "insertion_cost",
        "replacement_cost",
        "deletion_cost"
    );
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
    reg!("vsprintf", fn_vsprintf, 2, 2, "format", "values");

    // --- Regex functions ---
    reg_ref!(
        "preg_match",
        fn_preg_match,
        5,
        2,
        0b100,
        "pattern",
        "subject",
        "matches",
        "flags",
        "offset"
    );
    reg_ref!(
        "preg_replace",
        fn_preg_replace,
        5,
        3,
        0b1_0000,
        "pattern",
        "replacement",
        "subject",
        "limit",
        "count"
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
    reg!("is_iterable", fn_is_iterable, 1, 1, "value");
    reg!("gettype", fn_gettype, 1, 1, "value");
    reg!("get_debug_type", fn_get_debug_type, 1, 1, "value");

    // --- Reflection / class introspection ---
    reg!("get_class", fn_get_class, 1, 0, "object");
    reg!("get_included_files", fn_get_included_files, 0, 0);
    reg!("get_required_files", fn_get_included_files, 0, 0);
    reg!("get_declared_classes", fn_get_declared_classes, 0, 0);
    reg!("get_declared_interfaces", fn_get_declared_interfaces, 0, 0);
    reg!("get_declared_traits", fn_get_declared_traits, 0, 0);
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
    reg!(
        "class_implements",
        fn_class_implements,
        2,
        1,
        "object_or_class",
        "autoload"
    );
    reg!(
        "class_parents",
        fn_class_parents,
        2,
        1,
        "object_or_class",
        "autoload"
    );
    reg!(
        "class_uses",
        fn_class_uses,
        2,
        1,
        "object_or_class",
        "autoload"
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
    reg!("print_r", fn_print_r, 2, 1, "value", "return");
    reg!("var_export", fn_var_export, 2, 1, "value", "return");

    // --- Constants ---
    reg!("define", fn_define, 2, 2, "constant_name", "value");
    reg!("defined", fn_defined, 1, 1, "constant_name");
    reg!("constant", fn_constant, 1, 1, "name");

    // --- JSON ---
    reg!("json_encode", fn_json_encode, 2, 1, "value", "flags");
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
    reg!(
        "set_error_handler",
        fn_set_error_handler,
        2,
        1,
        "callback",
        "error_levels"
    );
    reg!("restore_error_handler", fn_restore_error_handler, 0, 0);
    reg!("get_error_handler", fn_get_error_handler, 0, 0);
    reg!(
        "trigger_error",
        fn_trigger_error,
        2,
        1,
        "message",
        "error_level"
    );
    reg!(
        "user_error",
        fn_trigger_error,
        2,
        1,
        "message",
        "error_level"
    );
    reg!(
        "set_exception_handler",
        fn_set_exception_handler,
        1,
        1,
        "callback"
    );
    reg!(
        "restore_exception_handler",
        fn_restore_exception_handler,
        0,
        0
    );
    reg!("get_exception_handler", fn_get_exception_handler, 0, 0);
    reg_var!(
        "register_shutdown_function",
        fn_register_shutdown_function,
        1,
        "callback"
    );
    reg!("error_reporting", fn_error_reporting, 1, 0, "error_level");
    reg!(
        "error_log",
        fn_error_log,
        4,
        1,
        "message",
        "message_type",
        "destination",
        "additional_headers"
    );
    reg!("ob_get_level", fn_ob_get_level, 0, 0);
    reg!("gc_mem_caches", fn_gc_mem_caches, 0, 0);
    reg!("func_num_args", fn_func_num_args, 0, 0);
    reg!("func_get_arg", fn_func_get_arg, 1, 1, "position");
    reg!("func_get_args", fn_func_get_args, 0, 0);
    reg!(
        "debug_backtrace",
        fn_debug_backtrace,
        2,
        0,
        "options",
        "limit"
    );

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
    reg!("gmdate", fn_gmdate, 2, 1, "format", "timestamp");
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
    reg!("filemtime", fn_filemtime, 1, 1, "filename");
    reg!("is_file", fn_is_file, 1, 1, "filename");
    reg!("is_dir", fn_is_dir, 1, 1, "filename");
    reg!("is_link", fn_is_link, 1, 1, "filename");
    reg!("chmod", fn_chmod, 2, 2, "filename", "permissions");
    reg!("fileperms", fn_fileperms, 1, 1, "filename");
    reg!("umask", fn_umask, 1, 0, "mask");
    reg!("is_readable", fn_is_readable, 1, 1, "filename");
    reg!("is_writable", fn_is_writable, 1, 1, "filename");
    reg!("is_writeable", fn_is_writable, 1, 1, "filename");
    reg!("dirname", fn_dirname, 2, 1, "path", "levels");
    reg!("basename", fn_basename, 2, 1, "path", "suffix");
    reg!("realpath", fn_realpath, 1, 1, "path");
    reg!("pathinfo", fn_pathinfo, 2, 1, "path", "flags");
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
        5,
        2,
        0b100,
        "pattern",
        "subject",
        "matches",
        "flags",
        "offset"
    );
    reg!(
        "preg_split",
        fn_preg_split,
        4,
        2,
        "pattern",
        "subject",
        "limit",
        "flags"
    );
    reg_ref!(
        "preg_replace_callback",
        fn_preg_replace_callback,
        6,
        3,
        0b1_0000,
        "pattern",
        "callback",
        "subject",
        "limit",
        "count",
        "flags"
    );
    reg!("preg_quote", fn_preg_quote, 2, 1, "string", "delimiter");

    // --- String encoding ---
    reg!(
        "htmlspecialchars",
        fn_htmlspecialchars,
        3,
        1,
        "string",
        "flags",
        "encoding"
    );
    reg!(
        "htmlspecialchars_decode",
        fn_htmlspecialchars_decode,
        2,
        1,
        "string",
        "flags"
    );
    reg!(
        "htmlentities",
        fn_htmlentities,
        3,
        1,
        "string",
        "flags",
        "encoding"
    );
    reg!(
        "html_entity_decode",
        fn_html_entity_decode,
        3,
        1,
        "string",
        "flags",
        "encoding"
    );
    reg!("urlencode", fn_urlencode, 1, 1, "string");
    reg!("urldecode", fn_urldecode, 1, 1, "string");
    reg!("rawurlencode", fn_rawurlencode, 1, 1, "string");
    reg!("rawurldecode", fn_rawurldecode, 1, 1, "string");
    reg!("base64_encode", fn_base64_encode, 1, 1, "data");
    reg!("base64_decode", fn_base64_decode, 1, 1, "data");
    reg!(
        "filter_var",
        fn_filter_var,
        3,
        2,
        "value",
        "filter",
        "options"
    );

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
    reg_ref!("uasort", fn_uasort, 2, 2, 0b1, "array", "callback");
    reg_ref!("uksort", fn_uksort, 2, 2, 0b1, "array", "callback");
    reg!("array_diff", fn_array_diff, 2, 2, "array1", "array2");
    reg_var!("array_diff_key", fn_array_diff_key, 2, "array", "arrays");
    reg_var!(
        "array_intersect_key",
        fn_array_intersect_key,
        2,
        "array",
        "arrays"
    );
    reg!(
        "array_intersect",
        fn_array_intersect,
        2,
        2,
        "array1",
        "array2"
    );
    reg_ref!("array_walk", fn_array_walk, 2, 2, 0b1, "array", "callback");
    reg_ref!(
        "array_walk_recursive",
        fn_array_walk_recursive,
        3,
        2,
        0b1,
        "array",
        "callback",
        "arg"
    );
    reg_ref!("asort", fn_asort, 2, 1, 0b1, "array", "flags");
    reg_ref!("arsort", fn_arsort, 2, 1, 0b1, "array", "flags");
    reg_ref!("ksort", fn_ksort, 2, 1, 0b1, "array", "flags");
    reg_ref!("krsort", fn_krsort, 2, 1, 0b1, "array", "flags");
    reg!("array_key_first", fn_array_key_first, 1, 1, "array");
    reg_ref!("reset", fn_reset, 1, 1, 0b1, "array");
    reg_ref!("end", fn_end, 1, 1, 0b1, "array");
    reg!("current", fn_current, 1, 1, "array");
    reg_ref!("next", fn_next, 1, 1, 0b1, "array");
    reg_ref!("prev", fn_prev, 1, 1, 0b1, "array");
    reg!("key", fn_key, 1, 1, "array");
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
    reg!("phpversion", fn_phpversion, 1, 0, "extension");
    reg!("extension_loaded", fn_extension_loaded, 1, 1, "extension");
    reg!("headers_sent", fn_headers_sent, 2, 0, "filename", "line");
    reg!(
        "header",
        fn_header,
        3,
        1,
        "header",
        "replace",
        "response_code"
    );
    reg!("ini_get", fn_ini_get, 1, 1, "option");
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

    eg.seal_internal_class_ids();
    funcs
}

// ============================================================================
// Built-in exception classes (Throwable hierarchy)
// ============================================================================

/// Internal handler for Error/Exception
/// __construct($message = "", $code = 0, $previous = null).
/// CV 0 = $this, CV 1..3 = explicit parameters.
fn fn_throwable_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    let message = arg_opt!(ed, 1);
    let code = arg_opt!(ed, 2);
    let previous = arg_opt!(ed, 3);
    if let Some(mut obj) = this_val.as_object_mut() {
        let msg = match message {
            Some(v) => v.clone(),
            None => Value::string(""),
        };
        obj.set_property("message", msg);
        obj.set_property("code", code.cloned().unwrap_or_else(|| Value::long(0)));
        let previous_key = eg
            .find_property_visibility(&obj.class_name, "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring_class)| {
                    crate::runtime::mangle_private_prop(&declaring_class, "previous")
                },
            );
        obj.set_property(&previous_key, previous.cloned().unwrap_or_else(Value::null));
    }
    Ok(())
}

fn fn_throwable_get_code(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object()
        && let Some(code) = obj.get_property("code")
    {
        ret!(rv, code.clone());
    }
    ret!(rv, Value::long(0));
}

fn fn_throwable_get_previous(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        let previous_key = eg
            .find_property_visibility(&obj.class_name, "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring_class)| {
                    crate::runtime::mangle_private_prop(&declaring_class, "previous")
                },
            );
        if let Some(previous) = obj.get_property(&previous_key) {
            ret!(rv, previous.clone());
        }
    }
    ret!(rv, Value::null());
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

fn fn_throwable_get_file(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("file").cloned())
        .unwrap_or_else(|| Value::string(""));
    ret!(rv, value);
}

fn fn_throwable_get_line(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("line").cloned())
        .unwrap_or_else(|| Value::long(0));
    ret!(rv, value);
}

fn fn_throwable_get_trace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("trace").cloned())
        .unwrap_or_else(|| Value::array(PhpArray::new()));
    ret!(rv, value);
}

fn fn_throwable_get_trace_as_string(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(""));
}

fn bind_closure_value(
    source_value: &Value,
    new_this: &Value,
    scope: Option<&Value>,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    api: &str,
    new_this_argument: usize,
    scope_argument: usize,
) -> Result<(), VmError> {
    let Some(source) = source_value.as_closure() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("{api}(): receiver must be of type Closure"),
        ));
        return Ok(());
    };
    let mut rebound = source.clone();
    rebound.identity = std::rc::Rc::new(());

    rebound.bound_this = match new_this.value_type() {
        ValueType::Null => None,
        ValueType::Object if rebound.is_static => {
            eg.write_output(
                format!("Warning: {api}(): Cannot bind an instance to a static closure\n")
                    .as_bytes(),
            );
            ret!(rv, Value::null());
        }
        ValueType::Object => Some(new_this.clone()),
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{api}(): Argument #{new_this_argument} ($newThis) must be of type ?object"
                ),
            ));
            return Ok(());
        }
    };

    if let Some(scope) = scope {
        rebound.called_scope_class_id = match scope.value_type() {
            ValueType::Null => 0,
            ValueType::String if scope.as_str() == Some("static") => source.called_scope_class_id,
            ValueType::String => {
                let name = scope.as_str().unwrap_or_default();
                let Some(class) = eg.find_class(name) else {
                    eg.write_output(
                        format!("Warning: {api}(): Class \"{name}\" not found\n").as_bytes(),
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
                    &format!(
                        "{api}(): Argument #{scope_argument} ($newScope) must be of type object|string|null"
                    ),
                ));
                return Ok(());
            }
        };
    }

    ret!(rv, Value::closure(rebound));
}

fn fn_closure_bind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bind_closure_value(
        arg!(ed, 1),
        arg!(ed, 2),
        arg_opt!(ed, 3),
        rv,
        eg,
        "Closure::bind",
        2,
        3,
    )
}

fn fn_closure_bind_to(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bind_closure_value(
        arg!(ed, 0),
        arg!(ed, 1),
        arg_opt!(ed, 2),
        rv,
        eg,
        "Closure::bindTo",
        1,
        2,
    )
}

fn fn_array_iterator_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let values = arg_opt!(ed, 1)
        .filter(|value| value.value_type() == ValueType::Array)
        .cloned()
        .unwrap_or_else(|| Value::array(PhpArray::new()));
    if let Some(mut object) = arg!(ed, 0).as_object_mut() {
        object.set_property("__rphp_iterator_values", values);
    }
    Ok(())
}

const SPL_STORAGE_DATA: &str = "__rphp_spl_storage_data";
const SPL_STORAGE_OBJECTS: &str = "__rphp_spl_storage_objects";
const SPL_STORAGE_ITERATOR: &str = "__rphp_iterator_values";
const SPL_STORAGE_POSITION: &str = "__rphp_spl_storage_position";
const SPL_PRIORITY_ENTRIES: &str = "__rphp_spl_priority_entries";
const SPL_PRIORITY_POSITION: &str = "__rphp_spl_priority_position";
const SPL_PRIORITY_EXTRACT_FLAGS: &str = "__rphp_spl_priority_extract_flags";
const SPL_PRIORITY_EXTR_DATA: i64 = 1;
const SPL_PRIORITY_EXTR_PRIORITY: i64 = 2;
const SPL_PRIORITY_EXTR_BOTH: i64 = 3;

#[inline]
fn spl_storage_array(receiver: &Value, property: &str) -> PhpArray {
    receiver
        .as_object()
        .and_then(|object| object.get_property(property).cloned())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_else(PhpArray::new)
}

#[inline]
fn spl_storage_identity(eg: &mut ExecutorGlobals, object: &Value, method: &str) -> Option<i64> {
    let Some(identity) = object.object_identity() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("SplObjectStorage::{method}(): Argument #1 ($object) must be of type object"),
        ));
        return None;
    };
    Some(identity as i64)
}

fn spl_storage_store(receiver: &Value, identity: i64, object: Value, data: Value) {
    let mut values = spl_storage_array(receiver, SPL_STORAGE_DATA);
    let mut objects = spl_storage_array(receiver, SPL_STORAGE_OBJECTS);
    let is_new = objects.get_int(identity).is_none();
    values.set_int(identity, data);
    objects.set_int(identity, object.clone());

    let iterator = if is_new {
        let mut iterator = spl_storage_array(receiver, SPL_STORAGE_ITERATOR);
        iterator.push(object);
        iterator
    } else {
        spl_storage_array(receiver, SPL_STORAGE_ITERATOR)
    };

    if let Some(mut receiver) = receiver.as_object_mut() {
        receiver.set_property(SPL_STORAGE_DATA, Value::array(values));
        receiver.set_property(SPL_STORAGE_OBJECTS, Value::array(objects));
        receiver.set_property(SPL_STORAGE_ITERATOR, Value::array(iterator));
    }
}

fn spl_storage_remove(receiver: &Value, identity: i64) {
    let mut values = spl_storage_array(receiver, SPL_STORAGE_DATA);
    let mut objects = spl_storage_array(receiver, SPL_STORAGE_OBJECTS);
    if !objects.remove(&ArrayKey::Int(identity)) {
        return;
    }
    values.remove(&ArrayKey::Int(identity));

    let mut iterator = PhpArray::new();
    for object in objects.values() {
        iterator.push(object.clone());
    }
    if let Some(mut receiver) = receiver.as_object_mut() {
        receiver.set_property(SPL_STORAGE_DATA, Value::array(values));
        receiver.set_property(SPL_STORAGE_OBJECTS, Value::array(objects));
        receiver.set_property(SPL_STORAGE_ITERATOR, Value::array(iterator));
        let position = receiver
            .get_property(SPL_STORAGE_POSITION)
            .and_then(Value::as_long)
            .unwrap_or(0)
            .min(
                receiver
                    .get_property(SPL_STORAGE_ITERATOR)
                    .and_then(Value::as_array)
                    .map_or(0, |values| values.len() as i64),
            );
        receiver.set_property(SPL_STORAGE_POSITION, Value::long(position));
    }
}

fn fn_spl_object_storage_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_STORAGE_DATA, Value::array(PhpArray::new()));
        receiver.set_property(SPL_STORAGE_OBJECTS, Value::array(PhpArray::new()));
        receiver.set_property(SPL_STORAGE_ITERATOR, Value::array(PhpArray::new()));
        receiver.set_property(SPL_STORAGE_POSITION, Value::long(0));
    }
    Ok(())
}

fn fn_spl_object_storage_offset_set(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let object = arg!(ed, 1).clone();
    let Some(identity) = spl_storage_identity(eg, &object, "offsetSet") else {
        return Ok(());
    };
    let data = arg_opt!(ed, 2).cloned().unwrap_or_else(Value::null);
    spl_storage_store(&receiver, identity, object, data);
    Ok(())
}

fn fn_spl_object_storage_offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let object = arg!(ed, 1);
    let Some(identity) = spl_storage_identity(eg, object, "offsetGet") else {
        return Ok(());
    };
    let data = spl_storage_array(arg!(ed, 0), SPL_STORAGE_DATA)
        .get_int(identity)
        .cloned();
    let Some(data) = data else {
        eg.exception = Some(crate::value::make_error_value(
            "UnexpectedValueException",
            "Object not found",
        ));
        return Ok(());
    };
    ret!(rv, data);
}

fn fn_spl_object_storage_offset_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let object = arg!(ed, 1);
    let Some(identity) = spl_storage_identity(eg, object, "offsetExists") else {
        return Ok(());
    };
    ret!(
        rv,
        Value::bool(
            spl_storage_array(arg!(ed, 0), SPL_STORAGE_OBJECTS)
                .get_int(identity)
                .is_some()
        )
    );
}

fn fn_spl_object_storage_offset_unset(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let object = arg!(ed, 1);
    let Some(identity) = spl_storage_identity(eg, object, "offsetUnset") else {
        return Ok(());
    };
    spl_storage_remove(arg!(ed, 0), identity);
    Ok(())
}

fn fn_spl_object_storage_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::long(spl_storage_array(arg!(ed, 0), SPL_STORAGE_OBJECTS).len() as i64)
    );
}

fn fn_spl_object_storage_rewind(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_STORAGE_POSITION, Value::long(0));
    }
    Ok(())
}

fn spl_storage_position(receiver: &Value) -> usize {
    receiver
        .as_object()
        .and_then(|object| {
            object
                .get_property(SPL_STORAGE_POSITION)
                .and_then(Value::as_long)
        })
        .unwrap_or(0)
        .max(0) as usize
}

fn fn_spl_object_storage_valid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ret!(
        rv,
        Value::bool(
            spl_storage_position(receiver)
                < spl_storage_array(receiver, SPL_STORAGE_ITERATOR).len()
        )
    );
}

fn fn_spl_object_storage_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    let value = spl_storage_array(receiver, SPL_STORAGE_ITERATOR)
        .get_value_at(spl_storage_position(receiver))
        .cloned()
        .unwrap_or_else(Value::null);
    ret!(rv, value);
}

fn fn_spl_object_storage_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(spl_storage_position(arg!(ed, 0)) as i64));
}

fn fn_spl_object_storage_next(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let next = spl_storage_position(arg!(ed, 0)).saturating_add(1) as i64;
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_STORAGE_POSITION, Value::long(next));
    }
    Ok(())
}

#[inline]
fn spl_priority_entries(receiver: &Value) -> PhpArray {
    spl_storage_array(receiver, SPL_PRIORITY_ENTRIES)
}

#[inline]
fn spl_priority_position(receiver: &Value) -> usize {
    receiver
        .as_object()
        .and_then(|object| {
            object
                .get_property(SPL_PRIORITY_POSITION)
                .and_then(Value::as_long)
        })
        .unwrap_or(0)
        .max(0) as usize
}

fn spl_priority_compare(left: &Value, right: &Value) -> std::cmp::Ordering {
    match (left.as_array(), right.as_array()) {
        (Some(left), Some(right)) => {
            for (left, right) in left.values().zip(right.values()) {
                let ordering = spl_priority_compare(left, right);
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            return left.len().cmp(&right.len());
        }
        (Some(_), None) => return std::cmp::Ordering::Greater,
        (None, Some(_)) => return std::cmp::Ordering::Less,
        (None, None) => {}
    }

    match (left.value_type(), right.value_type()) {
        (ValueType::Long | ValueType::Double, ValueType::Long | ValueType::Double) => left
            .to_float_val()
            .partial_cmp(&right.to_float_val())
            .unwrap_or(std::cmp::Ordering::Equal),
        (ValueType::String, ValueType::String) => left
            .as_str()
            .unwrap_or_default()
            .cmp(right.as_str().unwrap_or_default()),
        _ => left.echo_to_string().cmp(&right.echo_to_string()),
    }
}

#[inline]
fn spl_priority_entry_part(entry: &Value, index: i64) -> Value {
    entry
        .as_array()
        .and_then(|entry| entry.get_int(index))
        .cloned()
        .unwrap_or_else(Value::null)
}

fn spl_priority_extract_value(receiver: &Value, entry: &Value) -> Value {
    let data = spl_priority_entry_part(entry, 0);
    let priority = spl_priority_entry_part(entry, 1);
    let flags = receiver
        .as_object()
        .and_then(|object| {
            object
                .get_property(SPL_PRIORITY_EXTRACT_FLAGS)
                .and_then(Value::as_long)
        })
        .unwrap_or(SPL_PRIORITY_EXTR_DATA);
    match flags {
        SPL_PRIORITY_EXTR_PRIORITY => priority,
        SPL_PRIORITY_EXTR_BOTH => {
            let mut result = PhpArray::new();
            result.set_str("data", data);
            result.set_str("priority", priority);
            Value::array(result)
        }
        _ => data,
    }
}

fn spl_priority_refresh_iterator(receiver: &Value) {
    let entries = spl_priority_entries(receiver);
    let mut iterator = PhpArray::with_packed_capacity(entries.len());
    for entry in entries.values() {
        iterator.push(spl_priority_extract_value(receiver, entry));
    }
    if let Some(mut receiver) = receiver.as_object_mut() {
        receiver.set_property(SPL_STORAGE_ITERATOR, Value::array(iterator));
    }
}

fn fn_spl_priority_queue_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_PRIORITY_ENTRIES, Value::array(PhpArray::new()));
        receiver.set_property(SPL_PRIORITY_POSITION, Value::long(0));
        receiver.set_property(
            SPL_PRIORITY_EXTRACT_FLAGS,
            Value::long(SPL_PRIORITY_EXTR_DATA),
        );
        receiver.set_property(SPL_STORAGE_ITERATOR, Value::array(PhpArray::new()));
    }
    Ok(())
}

fn fn_spl_priority_queue_insert(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let mut entry = PhpArray::new();
    entry.push(arg!(ed, 1).clone());
    entry.push(arg!(ed, 2).clone());

    let mut entries: Vec<Value> = spl_priority_entries(&receiver).values().cloned().collect();
    entries.push(Value::array(entry));
    entries.sort_by(|left, right| {
        spl_priority_compare(
            &spl_priority_entry_part(right, 1),
            &spl_priority_entry_part(left, 1),
        )
    });

    let mut sorted = PhpArray::with_packed_capacity(entries.len());
    for entry in entries {
        sorted.push(entry);
    }
    if let Some(mut receiver) = receiver.as_object_mut() {
        receiver.set_property(SPL_PRIORITY_ENTRIES, Value::array(sorted));
    }
    spl_priority_refresh_iterator(&receiver);
    ret!(rv, Value::bool(true));
}

fn fn_spl_priority_queue_set_extract_flags(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = arg_long!(ed, 1);
    if !matches!(
        flags,
        SPL_PRIORITY_EXTR_DATA | SPL_PRIORITY_EXTR_PRIORITY | SPL_PRIORITY_EXTR_BOTH
    ) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "SplPriorityQueue::setExtractFlags(): Argument #1 ($flags) must be a valid extract flag",
        ));
        return Ok(());
    }
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_PRIORITY_EXTRACT_FLAGS, Value::long(flags));
    }
    spl_priority_refresh_iterator(arg!(ed, 0));
    Ok(())
}

fn fn_spl_priority_queue_rewind(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_PRIORITY_POSITION, Value::long(0));
    }
    Ok(())
}

fn fn_spl_priority_queue_valid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    ret!(
        rv,
        Value::bool(spl_priority_position(receiver) < spl_priority_entries(receiver).len())
    );
}

fn fn_spl_priority_queue_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0);
    let entry = spl_priority_entries(receiver)
        .get_value_at(spl_priority_position(receiver))
        .cloned();
    ret!(
        rv,
        entry
            .as_ref()
            .map_or_else(Value::null, |entry| spl_priority_extract_value(
                receiver, entry
            ))
    );
}

fn fn_spl_priority_queue_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(spl_priority_position(arg!(ed, 0)) as i64));
}

fn fn_spl_priority_queue_next(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let next = spl_priority_position(arg!(ed, 0)).saturating_add(1) as i64;
    if let Some(mut receiver) = arg!(ed, 0).as_object_mut() {
        receiver.set_property(SPL_PRIORITY_POSITION, Value::long(next));
    }
    Ok(())
}

fn fn_spl_priority_queue_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::long(spl_priority_entries(arg!(ed, 0)).len() as i64)
    );
}

fn fn_spl_priority_queue_is_empty(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        Value::bool(spl_priority_entries(arg!(ed, 0)).is_empty())
    );
}

#[cfg(feature = "value-errors")]
#[cold]
fn register_value_error(eg: &mut ExecutorGlobals) -> [Box<InternalFunction>; 2] {
    use crate::compiler::compile::ClassDef;

    eg.register_class(ClassDef {
        name: "ValueError".to_string(),
        source_file: None,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        4,
        0,
        vec![
            "message".to_string(),
            "code".to_string(),
            "previous".to_string(),
        ],
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
        source_file: None,
        parent: None,
        implements: vec![],
        is_interface: true,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        source_file: None,
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![
            PropertyDefinition::new(
                "message".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::new(
                "code".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::new(
                "previous".to_string(),
                Some(Value::null()),
                Visibility::Private,
                "Exception".to_string(),
            ),
        ],
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

    // SPL's logic/runtime exception families share Exception's constructor and
    // properties. Register parents before children so ClassDef inheritance can
    // materialize their layouts immediately.
    for &(name, parent) in BUILTIN_EXCEPTION_SUBCLASSES {
        eg.register_class(ClassDef {
            name: name.to_string(),
            source_file: None,
            parent: Some(parent.to_string()),
            implements: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_trait: false,
            is_enum: false,
            is_readonly: false,
            uses: vec![],
            trait_aliases: vec![],
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
    }

    // Error implements Throwable
    eg.register_class(ClassDef {
        name: "Error".to_string(),
        source_file: None,
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![
            PropertyDefinition::new(
                "message".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::new(
                "code".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::new(
                "previous".to_string(),
                Some(Value::null()),
                Visibility::Private,
                "Error".to_string(),
            ),
        ],
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
        source_file: None,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        source_file: None,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        source_file: None,
        parent: Some("CompileError".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        source_file: None,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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
        source_file: None,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
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

    // Register core Throwable methods for each built-in concrete class.
    // num_args = 4 for __construct (CV 0 = $this, CV 1..3 = explicit args)
    // num_args = 1 for getMessage (CV 0 = $this)
    let throwable_classes = ["Throwable", "Exception"]
        .into_iter()
        .chain(BUILTIN_EXCEPTION_SUBCLASSES.iter().map(|&(name, _)| name))
        .chain([
            "Error",
            "TypeError",
            "CompileError",
            "ParseError",
            "ArgumentCountError",
            "UnhandledMatchError",
        ]);
    for class in throwable_classes {
        // All explicit constructor parameters are optional.
        reg_method!(
            class,
            "__construct",
            fn_throwable_construct,
            4,
            0,
            "message",
            "code",
            "previous"
        );
        // getMessage: num_args=1 (CV 0=$this), required=0 (no explicit args)
        reg_method!(class, "getmessage", fn_throwable_get_message, 1, 0);
        reg_method!(class, "getcode", fn_throwable_get_code, 1, 0);
        reg_method!(class, "getprevious", fn_throwable_get_previous, 1, 0);
        reg_method!(class, "getfile", fn_throwable_get_file, 1, 0);
        reg_method!(class, "getline", fn_throwable_get_line, 1, 0);
        reg_method!(class, "gettrace", fn_throwable_get_trace, 1, 0);
        reg_method!(
            class,
            "gettraceasstring",
            fn_throwable_get_trace_as_string,
            1,
            0
        );
    }

    funcs.extend(reflection::register(eg));

    #[cfg(feature = "value-errors")]
    funcs.extend(register_value_error(eg));

    let empty_internal_type =
        |name: &str, implements: Vec<String>, is_interface: bool, is_final: bool| ClassDef {
            name: name.to_string(),
            source_file: None,
            parent: None,
            implements,
            is_interface,
            is_abstract: false,
            is_final,
            is_trait: false,
            is_enum: false,
            is_readonly: false,
            uses: vec![],
            trait_aliases: vec![],
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
    eg.register_class(empty_internal_type("HashContext", vec![], false, true))
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
    reg_method!(
        "Closure",
        "bindTo",
        fn_closure_bind_to,
        3,
        1,
        "newThis",
        "newScope"
    );

    // Canonical iterator hierarchy used by generator return contracts,
    // instanceof and the iterable pseudo-type.
    eg.register_class(empty_internal_type("Traversable", vec![], true, false))
        .unwrap();
    for (name, parents) in [
        ("IteratorAggregate", vec!["Traversable".to_string()]),
        ("Countable", vec![]),
        ("ArrayAccess", vec![]),
        ("Stringable", vec![]),
        ("Serializable", vec![]),
        ("JsonSerializable", vec![]),
        ("SessionHandlerInterface", vec![]),
        ("SessionUpdateTimestampHandlerInterface", vec![]),
    ] {
        eg.register_class(empty_internal_type(name, parents, true, false))
            .unwrap();
    }
    eg.register_class(empty_internal_type(
        "Iterator",
        vec!["Traversable".to_string()],
        true,
        false,
    ))
    .unwrap();
    eg.register_class(empty_internal_type(
        "RecursiveIterator",
        vec!["Iterator".to_string()],
        true,
        false,
    ))
    .unwrap();
    for (name, traversal_interface) in [
        ("ArrayIterator", "Iterator"),
        ("ArrayObject", "IteratorAggregate"),
    ] {
        let mut class = empty_internal_type(
            name,
            vec![
                traversal_interface.to_string(),
                "ArrayAccess".to_string(),
                "Countable".to_string(),
            ],
            false,
            false,
        );
        class.properties.push(PropertyDefinition::new(
            "__rphp_iterator_values".to_string(),
            Some(Value::array(PhpArray::new())),
            Visibility::Private,
            name.to_string(),
        ));
        eg.register_class(class).unwrap();
        reg_method!(
            name,
            "__construct",
            fn_array_iterator_construct,
            2,
            0,
            "array"
        );
    }
    let mut spl_object_storage = empty_internal_type(
        "SplObjectStorage",
        vec![
            "Iterator".to_string(),
            "ArrayAccess".to_string(),
            "Countable".to_string(),
            "Serializable".to_string(),
        ],
        false,
        false,
    );
    for (name, default) in [
        (SPL_STORAGE_DATA, Value::array(PhpArray::new())),
        (SPL_STORAGE_OBJECTS, Value::array(PhpArray::new())),
        (SPL_STORAGE_ITERATOR, Value::array(PhpArray::new())),
        (SPL_STORAGE_POSITION, Value::long(0)),
    ] {
        spl_object_storage.properties.push(PropertyDefinition::new(
            name.to_string(),
            Some(default),
            Visibility::Private,
            "SplObjectStorage".to_string(),
        ));
    }
    eg.register_class(spl_object_storage).unwrap();
    reg_method!(
        "SplObjectStorage",
        "__construct",
        fn_spl_object_storage_construct,
        1,
        0
    );
    reg_method!(
        "SplObjectStorage",
        "offsetset",
        fn_spl_object_storage_offset_set,
        3,
        1,
        "object",
        "info"
    );
    reg_method!(
        "SplObjectStorage",
        "attach",
        fn_spl_object_storage_offset_set,
        3,
        1,
        "object",
        "info"
    );
    reg_method!(
        "SplObjectStorage",
        "offsetget",
        fn_spl_object_storage_offset_get,
        2,
        1,
        "object"
    );
    reg_method!(
        "SplObjectStorage",
        "offsetexists",
        fn_spl_object_storage_offset_exists,
        2,
        1,
        "object"
    );
    reg_method!(
        "SplObjectStorage",
        "contains",
        fn_spl_object_storage_offset_exists,
        2,
        1,
        "object"
    );
    reg_method!(
        "SplObjectStorage",
        "offsetunset",
        fn_spl_object_storage_offset_unset,
        2,
        1,
        "object"
    );
    reg_method!(
        "SplObjectStorage",
        "detach",
        fn_spl_object_storage_offset_unset,
        2,
        1,
        "object"
    );
    reg_method!(
        "SplObjectStorage",
        "count",
        fn_spl_object_storage_count,
        1,
        0
    );
    reg_method!(
        "SplObjectStorage",
        "rewind",
        fn_spl_object_storage_rewind,
        1,
        0
    );
    reg_method!(
        "SplObjectStorage",
        "valid",
        fn_spl_object_storage_valid,
        1,
        0
    );
    reg_method!(
        "SplObjectStorage",
        "current",
        fn_spl_object_storage_current,
        1,
        0
    );
    reg_method!("SplObjectStorage", "key", fn_spl_object_storage_key, 1, 0);
    reg_method!("SplObjectStorage", "next", fn_spl_object_storage_next, 1, 0);

    let mut spl_priority_queue = empty_internal_type(
        "SplPriorityQueue",
        vec!["Iterator".to_string(), "Countable".to_string()],
        false,
        false,
    );
    spl_priority_queue.constants = [
        ("EXTR_DATA", SPL_PRIORITY_EXTR_DATA),
        ("EXTR_PRIORITY", SPL_PRIORITY_EXTR_PRIORITY),
        ("EXTR_BOTH", SPL_PRIORITY_EXTR_BOTH),
    ]
    .into_iter()
    .map(|(name, value)| ClassConstantDefinition {
        name: name.to_string(),
        value: Value::long(value),
        visibility: Visibility::Public,
        declaring_class: "SplPriorityQueue".to_string(),
        type_hint: ParamTypeHint::Int,
        is_final: false,
    })
    .collect();
    for (name, default) in [
        (SPL_PRIORITY_ENTRIES, Value::array(PhpArray::new())),
        (SPL_PRIORITY_POSITION, Value::long(0)),
        (
            SPL_PRIORITY_EXTRACT_FLAGS,
            Value::long(SPL_PRIORITY_EXTR_DATA),
        ),
        (SPL_STORAGE_ITERATOR, Value::array(PhpArray::new())),
    ] {
        spl_priority_queue.properties.push(PropertyDefinition::new(
            name.to_string(),
            Some(default),
            Visibility::Private,
            "SplPriorityQueue".to_string(),
        ));
    }
    eg.register_class(spl_priority_queue).unwrap();
    reg_method!(
        "SplPriorityQueue",
        "__construct",
        fn_spl_priority_queue_construct,
        1,
        0
    );
    reg_method!(
        "SplPriorityQueue",
        "insert",
        fn_spl_priority_queue_insert,
        3,
        2,
        "value",
        "priority"
    );
    reg_method!(
        "SplPriorityQueue",
        "setextractflags",
        fn_spl_priority_queue_set_extract_flags,
        2,
        1,
        "flags"
    );
    reg_method!(
        "SplPriorityQueue",
        "rewind",
        fn_spl_priority_queue_rewind,
        1,
        0
    );
    reg_method!(
        "SplPriorityQueue",
        "valid",
        fn_spl_priority_queue_valid,
        1,
        0
    );
    reg_method!(
        "SplPriorityQueue",
        "current",
        fn_spl_priority_queue_current,
        1,
        0
    );
    reg_method!("SplPriorityQueue", "key", fn_spl_priority_queue_key, 1, 0);
    reg_method!("SplPriorityQueue", "next", fn_spl_priority_queue_next, 1, 0);
    reg_method!(
        "SplPriorityQueue",
        "count",
        fn_spl_priority_queue_count,
        1,
        0
    );
    reg_method!(
        "SplPriorityQueue",
        "isempty",
        fn_spl_priority_queue_is_empty,
        1,
        0
    );
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
    let strict = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    let found = haystack
        .as_array()
        .map(|a| {
            a.values().any(|value| {
                if strict {
                    values_identical(needle, value)
                } else {
                    values_equal(needle, value)
                }
            })
        })
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut merged = PhpArray::new();
    let Some(arrays) = arg!(ed, 0).as_array() else {
        ret!(rv, Value::array(merged));
    };
    for value in arrays.values() {
        let Some(array) = value.as_array() else {
            ret!(rv, Value::null());
        };
        for (key, val) in array.iter() {
            match &key {
                ArrayKey::Int(_) => merged.push(val.clone()),
                ArrayKey::String(k) => merged.set_str(k, val.clone()),
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
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        entries.sort_by(|a, b| sort_value_cmp(a, b, flags));
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
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.values().cloned().collect();
        entries.sort_by(|a, b| sort_value_cmp(b, a, flags));
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
    let strict = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    if let Some(arr) = haystack.as_array() {
        for (key, val) in arr.iter() {
            if if strict {
                values_identical(needle, val)
            } else {
                values_equal(needle, val)
            } {
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

fn fn_hash(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let algorithm = arg_str!(ed, 0);
    let data = arg_str!(ed, 1);
    let binary = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
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
                ((len + l).max(0) as usize).max(start).min(bytes.len())
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

fn natural_compare(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    fn numeric_string(bytes: &[u8]) -> Option<&[u8]> {
        let bytes = bytes.trim_ascii();
        bytes.iter().all(u8::is_ascii_digit).then_some(bytes)
    }

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

    if let (Some(left), Some(right)) = (numeric_string(left), numeric_string(right)) {
        return compare_integer(left, right);
    }

    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() || right_index < right.len() {
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let left = arg_str!(ed, 0);
    let right = arg_str!(ed, 1);
    let result = match natural_compare(left.as_bytes(), right.as_bytes()) {
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
    let haystack = arg_str!(ed, 0);
    let needle = arg_str!(ed, 1);
    let haystack_bytes = haystack.as_bytes();
    let offset = arg_long!(ed, 2);
    let start = if offset < 0 {
        Some(
            haystack_bytes
                .len()
                .saturating_sub(offset.unsigned_abs() as usize),
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
    let length = arg_opt!(ed, 3)
        .filter(|value| !matches!(value.value_type(), ValueType::Null | ValueType::Undef))
        .map(Value::to_long_val);
    if length.is_some_and(|length| length <= 0) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "substr_compare(): Argument #4 ($length) must be greater than 0",
        ));
        return Ok(());
    }
    let compared_length = length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(available)
        .min(available);
    let left = &haystack_bytes[start..start + compared_length];
    let right = &needle.as_bytes()[..length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(needle.len())
        .min(needle.len())];
    let ordering = if arg_opt!(ed, 4).is_some_and(Value::is_truthy) {
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
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    let offset = arg_opt!(ed, 2).map(Value::to_long_val).unwrap_or(0);
    let start = if offset < 0 {
        h.len().checked_sub(offset.unsigned_abs() as usize)
    } else {
        usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= h.len())
    };
    let Some(start) = start else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)",
        ));
        return Ok(());
    };
    ret!(
        rv,
        match h[start..].find(n.as_ref()) {
            Some(pos) => Value::long((start + pos) as i64),
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

fn fn_strrchr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let haystack = php_string_to_bytes(arg_str!(ed, 0).as_ref());
    let needle = php_string_to_bytes(arg_str!(ed, 1).as_ref());
    let Some(needle) = needle.first() else {
        ret!(rv, Value::bool(false));
    };
    let value = haystack
        .iter()
        .rposition(|byte| byte == needle)
        .map_or_else(
            || Value::bool(false),
            |position| Value::string(bytes_to_php_string(&haystack[position..])),
        );
    ret!(rv, value);
}

fn string_span_bounds(ed: *mut ExecuteData, byte_len: usize) -> (usize, usize) {
    let len = byte_len as i64;
    let raw_offset = arg_opt!(ed, 2).map_or(0, Value::to_long_val);
    let start = if raw_offset < 0 {
        (len + raw_offset).max(0)
    } else {
        raw_offset.min(len)
    };
    let end = match arg_opt!(ed, 3) {
        Some(length) => {
            let length = length.to_long_val();
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

fn string_span(ed: *mut ExecuteData, accept_matches: bool) -> i64 {
    let string = arg_str!(ed, 0);
    let characters = arg_str!(ed, 1);
    let (start, end) = string_span_bounds(ed, string.len());
    let mut accepted = [false; 256];
    for byte in characters.bytes() {
        accepted[byte as usize] = true;
    }
    string.as_bytes()[start..end]
        .iter()
        .take_while(|byte| accepted[**byte as usize] == accept_matches)
        .count() as i64
}

fn fn_strspn(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(string_span(ed, true)));
}

fn fn_strcspn(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(string_span(ed, false)));
}

fn fn_strpbrk(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let string = arg_str!(ed, 0);
    let characters = arg_str!(ed, 1);
    let mut accepted = [false; 256];
    for byte in characters.bytes() {
        accepted[byte as usize] = true;
    }
    let position = string
        .as_bytes()
        .iter()
        .position(|byte| accepted[*byte as usize]);
    match position {
        Some(position) => ret!(
            rv,
            Value::string(String::from_utf8_lossy(&string.as_bytes()[position..]).into_owned())
        ),
        None => ret!(rv, Value::bool(false)),
    }
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

fn trim_mask(ed: *mut ExecuteData) -> [bool; 256] {
    let mut mask = [false; 256];
    let default = [0_u8, b'\t', b'\n', 11, b'\r', b' '];
    let characters = arg_opt!(ed, 1).and_then(Value::as_str);
    let bytes = characters.map_or(default.as_slice(), str::as_bytes);
    let mut index = 0;
    while index < bytes.len() {
        if index + 3 < bytes.len() && bytes[index + 1] == b'.' && bytes[index + 2] == b'.' {
            let start = bytes[index];
            let end = bytes[index + 3];
            if start <= end {
                for byte in start..=end {
                    mask[byte as usize] = true;
                }
            } else {
                mask[start as usize] = true;
                mask[end as usize] = true;
            }
            index += 4;
        } else {
            mask[bytes[index] as usize] = true;
            index += 1;
        }
    }
    mask
}

fn fn_addcslashes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let string = php_string_to_bytes(arg_str!(ed, 0).as_ref());
    let characters = php_string_to_bytes(arg_str!(ed, 1).as_ref());
    let mut mask = [false; 256];
    let mut index = 0;
    while index < characters.len() {
        if index + 3 < characters.len()
            && characters[index + 1] == b'.'
            && characters[index + 2] == b'.'
            && characters[index] <= characters[index + 3]
        {
            for byte in characters[index]..=characters[index + 3] {
                mask[byte as usize] = true;
            }
            index += 4;
        } else {
            mask[characters[index] as usize] = true;
            index += 1;
        }
    }

    let mut escaped = Vec::with_capacity(string.len());
    for byte in string {
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
    ret!(rv, Value::string(bytes_to_php_string(&escaped)));
}

fn trim_bytes(ed: *mut ExecuteData, trim_left: bool, trim_right: bool) -> String {
    let string = arg_str!(ed, 0);
    let bytes = string.as_bytes();
    let mask = trim_mask(ed);
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
    String::from_utf8_lossy(&bytes[start..end]).into_owned()
}

fn fn_trim(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::string(trim_bytes(ed, true, true)));
}

fn fn_rtrim(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(trim_bytes(ed, false, true)));
}

fn fn_ltrim(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(trim_bytes(ed, true, false)));
}

fn fn_explode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let d = arg_str!(ed, 0);
    let s = arg_str!(ed, 1);
    if d.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "explode(): Argument #1 ($separator) cannot be empty",
        ));
        ret!(rv, Value::null());
    }
    let limit = arg_opt!(ed, 2).map(Value::to_long_val);
    let mut arr = PhpArray::new();
    match limit {
        None => {
            for part in s.split(d.as_ref()) {
                arr.push(Value::string(part));
            }
        }
        Some(limit) if limit >= 0 => {
            let limit = usize::try_from(limit.max(1)).unwrap_or(usize::MAX);
            for part in s.splitn(limit, d.as_ref()) {
                arr.push(Value::string(part));
            }
        }
        Some(limit) => {
            let parts = s.split(d.as_ref()).collect::<Vec<_>>();
            let retained = parts.len().saturating_sub(limit.unsigned_abs() as usize);
            for part in parts.into_iter().take(retained) {
                arr.push(Value::string(part));
            }
        }
    }
    ret!(rv, Value::array(arr));
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
    // The VM materializes the variadic bucket as an array. Reuse the same
    // zero-copy formatter as vsprintf instead of cloning its values.
    let args = arg!(ed, 1).as_array();
    let result = format_sprintf_values(&fmt, args.as_deref());
    ret!(rv, Value::string(result));
}

fn fn_vsprintf(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let fmt = arg_str!(ed, 0);
    let args = arg!(ed, 1).as_array();
    let result = format_sprintf_values(&fmt, args.as_deref());
    ret!(rv, Value::string(result));
}

fn format_sprintf_values(fmt: &str, args: Option<&PhpArray>) -> String {
    let args_count = args.map_or(0, PhpArray::len);
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
                    let arg = args.and_then(|args| args.get_value_at(arg_idx));
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
    result
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

fn fn_class_implements(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let class_name = if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_class(&class_name).is_none()
        && (!autoload_enabled || !autoload::ensure_symbol_loaded(eg, &class_name)?)
    {
        if eg.exception.is_none() {
            ret!(rv, Value::bool(false));
        }
        return Ok(());
    }

    let mut result = PhpArray::new();
    let mut classes = vec![class_name];
    let mut interfaces = Vec::new();
    while let Some(class_name) = classes.pop() {
        if let Some(class) = eg.find_class(&class_name) {
            interfaces.extend(class.implements.iter().cloned());
            if let Some(parent) = &class.parent {
                classes.push(parent.clone());
            }
        }
    }
    while let Some(interface_name) = interfaces.pop() {
        if result.get_str(&interface_name).is_some() {
            continue;
        }
        result.set_str(&interface_name, Value::string(interface_name.clone()));
        if let Some(interface) = eg.find_class(&interface_name) {
            interfaces.extend(interface.implements.iter().cloned());
        }
    }
    ret!(rv, Value::array(result));
}

fn fn_class_parents(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0);
    let class_name = if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_class(&class_name).is_none()
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
    let class_name = if let Some(object) = value.as_object() {
        object.class_name.to_string()
    } else if let Some(name) = value.as_str() {
        name.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };
    let autoload_enabled = arg_opt!(ed, 1).is_none_or(Value::is_truthy);
    if eg.find_class(&class_name).is_none()
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
    if arg_opt!(ed, 1).is_some_and(Value::is_truthy) {
        ret!(rv, Value::string(output));
    }
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

fn fn_set_error_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = eg.error_handler.clone().unwrap_or_else(Value::null);
    eg.error_handler_stack
        .push((eg.error_handler.take(), eg.error_handler_levels));
    eg.error_handler = arg_opt!(ed, 0)
        .filter(|handler| handler.value_type() != ValueType::Null)
        .cloned();
    eg.error_handler_levels = arg_opt!(ed, 1).map_or(32767, Value::to_long_val);
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

const E_USER_ERROR: i64 = 256;
const E_USER_WARNING: i64 = 512;
const E_USER_NOTICE: i64 = 1024;
const E_USER_DEPRECATED: i64 = 16_384;

/// Raise one of PHP's user-generated diagnostics. Recoverable diagnostics are
/// deliberately quiet when no handler claims them, matching RPHP's existing
/// warning policy while still exposing the observable handler contract.
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

    let handled = if level & eg.error_handler_levels != 0 {
        eg.error_handler.clone().and_then(|callback| {
            let resolved = resolve_callback_at_callsite(&callback, eg, ed)?;
            Some(call_resolved_with_values(
                eg,
                &resolved,
                &[
                    Value::long(level),
                    Value::string(message.clone()),
                    Value::string(""),
                    Value::long(0),
                ],
            ))
        })
    } else {
        None
    };

    if let Some(result) = handled {
        if result?.is_truthy() || eg.exception.is_some() {
            ret!(rv, Value::bool(true));
        }
    }
    if level == E_USER_ERROR {
        return Err(VmError::Fatal(message));
    }
    ret!(rv, Value::bool(true));
}

fn fn_set_exception_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = eg.exception_handler.clone().unwrap_or_else(Value::null);
    eg.exception_handler_stack.push(eg.exception_handler.take());
    eg.exception_handler = arg_opt!(ed, 0)
        .filter(|handler| handler.value_type() != ValueType::Null)
        .cloned();
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

/// The S2 CLI fixture only needs registration to be accepted; invocation at
/// request teardown remains outside this compatibility slice.
fn fn_register_shutdown_function(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}

fn fn_error_reporting(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let previous = eg.error_reporting;
    if let Some(level) = arg_opt!(ed, 0).and_then(Value::as_long) {
        eg.error_reporting = level;
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

/// No output buffer is active until the buffering API itself is implemented.
/// Exposing the observable level lets request kernels record their entry state.
fn fn_ob_get_level(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::long(0));
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
        if let Some(arguments) = eg.function_arguments.get(&(caller as usize)) {
            return arguments.get(index as usize).cloned();
        }
        let function = &*(*caller).func;
        let value = if function.sig.is_variadic && index >= function.sig.public_arity() {
            let offset = index - function.sig.public_arity();
            return (*caller)
                .cv(function.sig.variadic_cv_index)
                .as_array()
                .and_then(|arguments| arguments.get_value_at(offset as usize).cloned());
        } else {
            (*caller).cv(function.sig.param_cv_index(index))
        };
        Some(value.dereferenced().clone())
    }
}

fn caller_num_args(ed: *mut ExecuteData) -> Option<u32> {
    // SAFETY: `ed` is the live internal-function frame for this handler; its
    // predecessor, when non-null, remains allocated until the handler returns.
    unsafe {
        let caller = (*ed).prev_execute_data;
        (!caller.is_null()).then(|| (*caller).num_args)
    }
}

fn fn_func_num_args(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = caller_num_args(ed).map_or(-1, i64::from);
    ret!(rv, Value::long(count));
}

fn fn_func_get_arg(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(index) = arg!(ed, 0)
        .as_long()
        .and_then(|index| u32::try_from(index).ok())
    else {
        ret!(rv, Value::bool(false));
    };
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
    let count = caller_num_args(ed).unwrap_or(0);
    let mut arguments = PhpArray::with_packed_capacity(count as usize);
    for index in 0..count {
        arguments.push(caller_argument(ed, index, eg).unwrap_or_else(Value::null));
    }
    ret!(rv, Value::array(arguments));
}

/// Collect the live PHP call chain behind the internal debug_backtrace frame.
///
/// # Safety
///
/// The frame must be the active internal-function frame supplied to its
/// handler. Every predecessor and function header must remain live for this
/// synchronous walk, as guaranteed by the VM call-frame stack.
unsafe fn collect_debug_backtrace(
    ed: *mut ExecuteData,
    options: i64,
    limit: usize,
    eg: &ExecutorGlobals,
) -> PhpArray {
    let include_object = options & 1 != 0;
    let include_arguments = options & 2 == 0;
    let mut trace = PhpArray::new();
    // The built-in call frame itself is not part of PHP's reported trace;
    // frame zero is the user/internal caller that invoked debug_backtrace().
    let mut frame = (*ed).prev_execute_data;
    while !frame.is_null() && (limit == 0 || trace.len() < limit) {
        let function = Function::from_common_ptr((*frame).func);
        let name = match function.fn_type() {
            FunctionType::User => {
                let name = function.as_user().op_array.name.clone();
                if name.is_empty() {
                    break;
                }
                name
            }
            FunctionType::Internal => {
                let Some(name) = eg
                    .function_table
                    .iter()
                    .find(|(_, candidate)| **candidate == (*frame).func)
                    .map(|(name, _)| name.clone())
                else {
                    break;
                };
                name
            }
            FunctionType::Undef => break,
        };
        let common = &*(*frame).func;
        let mut entry = PhpArray::new();
        if let Some((class, method)) = name.rsplit_once("::") {
            entry.set_str("function", Value::string(method.to_string()));
            entry.set_str("class", Value::string(class.to_string()));
            let is_instance = common.sig.this_offset != 0;
            entry.set_str("type", Value::string(if is_instance { "->" } else { "::" }));
            if include_object && is_instance {
                let object = (*frame).cv(0).dereferenced();
                if object.as_object().is_some() {
                    entry.set_str("object", object.clone());
                }
            }
        } else {
            entry.set_str("function", Value::string(name));
        }
        if include_arguments {
            let count = (*frame).num_args;
            let mut arguments = PhpArray::with_packed_capacity(count as usize);
            for index in 0..count {
                let argument = if let Some(saved) = eg.function_arguments.get(&(frame as usize)) {
                    saved.get(index as usize).cloned()
                } else if common.sig.is_variadic && index >= common.sig.public_arity() {
                    let offset = index - common.sig.public_arity();
                    (*frame)
                        .cv(common.sig.variadic_cv_index)
                        .as_array()
                        .and_then(|values| values.get_value_at(offset as usize).cloned())
                } else {
                    Some(
                        (*frame)
                            .cv(common.sig.param_cv_index(index))
                            .dereferenced()
                            .clone(),
                    )
                };
                if let Some(argument) = argument {
                    arguments.push(argument);
                }
            }
            entry.set_str("args", Value::array(arguments));
        }
        trace.push(Value::array(entry));
        frame = (*frame).prev_execute_data;
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
    let trace = unsafe { collect_debug_backtrace(ed, options, limit, eg) };
    ret!(rv, Value::array(trace));
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

const SORT_NATURAL: i64 = 6;
const SORT_FLAG_CASE: i64 = 8;

fn natural_string_cmp(left: &str, right: &str, case_insensitive: bool) -> std::cmp::Ordering {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut left_index = 0usize;
    let mut right_index = 0usize;
    while left_index < left.len() && right_index < right.len() {
        if left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit() {
            let left_end = left_index
                + left[left_index..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
            let right_end = right_index
                + right[right_index..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
            let left_significant = left_index
                + left[left_index..left_end]
                    .iter()
                    .take_while(|byte| **byte == b'0')
                    .count();
            let right_significant = right_index
                + right[right_index..right_end]
                    .iter()
                    .take_while(|byte| **byte == b'0')
                    .count();
            let left_digits = &left[left_significant..left_end];
            let right_digits = &right[right_significant..right_end];
            let numeric = left_digits
                .len()
                .cmp(&right_digits.len())
                .then_with(|| left_digits.cmp(right_digits));
            if numeric != std::cmp::Ordering::Equal {
                return numeric;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let mut left_byte = left[left_index];
        let mut right_byte = right[right_index];
        if case_insensitive {
            left_byte = left_byte.to_ascii_lowercase();
            right_byte = right_byte.to_ascii_lowercase();
        }
        match left_byte.cmp(&right_byte) {
            std::cmp::Ordering::Equal => {
                left_index += 1;
                right_index += 1;
            }
            ordering => return ordering,
        }
    }
    (left.len() - left_index).cmp(&(right.len() - right_index))
}

fn sort_value_cmp(left: &Value, right: &Value, flags: i64) -> std::cmp::Ordering {
    if flags & !SORT_FLAG_CASE == SORT_NATURAL {
        natural_string_cmp(
            &left.echo_to_string(),
            &right.echo_to_string(),
            flags & SORT_FLAG_CASE != 0,
        )
    } else {
        cmp_val(compare_values(left, right))
    }
}

fn sort_key_cmp(left: &ArrayKey, right: &ArrayKey, flags: i64) -> std::cmp::Ordering {
    if flags & !SORT_FLAG_CASE == SORT_NATURAL {
        let left = match left {
            ArrayKey::Int(value) => value.to_string(),
            ArrayKey::String(value) => value.clone(),
        };
        let right = match right {
            ArrayKey::Int(value) => value.to_string(),
            ArrayKey::String(value) => value.clone(),
        };
        natural_string_cmp(&left, &right, flags & SORT_FLAG_CASE != 0)
    } else {
        match (left, right) {
            (ArrayKey::Int(left), ArrayKey::Int(right)) => left.cmp(right),
            (ArrayKey::String(left), ArrayKey::String(right)) => left.cmp(right),
            (ArrayKey::Int(_), ArrayKey::String(_)) => std::cmp::Ordering::Less,
            (ArrayKey::String(_), ArrayKey::Int(_)) => std::cmp::Ordering::Greater,
        }
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
            return Some((
                Visibility::Public,
                unsafe { (*function).sig.this_offset == 0 },
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
    let Some(object) = receiver.as_object() else {
        return Ok(None);
    };
    let class_name = object.class_name.to_string();
    drop(object);
    if !eg.class_is_a(&class_name, interface) {
        return Ok(None);
    }
    call_object_public_method(eg, receiver, method, args)
}

/// Invoke an ordinary public instance method without constructing a callback
/// descriptor. Serialization hooks and VM protocols share this cold path.
pub(crate) fn call_object_public_method(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    method: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(object) = receiver.as_object() else {
        return Ok(None);
    };
    let class_name = object.class_name.to_string();
    let class_id = object.class_id;
    drop(object);

    let internal_name = format!("{class_name}::{method}");
    let func_ptr = if let Some(function) = eg.find_function(&internal_name) {
        function
    } else {
        let Some((visibility, is_static, function, _)) =
            find_method_in_class_hierarchy(eg, &class_name, method)
        else {
            return Ok(None);
        };
        if visibility != Visibility::Public || is_static {
            return Ok(None);
        }
        function
    };
    let resolved = ResolvedCallback {
        func_ptr,
        prepend_args: vec![receiver.clone()],
        use_vars: vec![],
        called_scope_class_id: class_id,
        bound_this: None,
    };
    call_resolved_with_values(eg, &resolved, args).map(Some)
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

    /// Resolved callback pointers are owned by the request's immutable
    /// function table and remain stable for the lifetime of this descriptor.
    #[inline]
    fn signature(&self) -> &crate::vm::function::SignatureInfo {
        unsafe { &(*self.func_ptr).sig }
    }

    #[inline]
    pub(crate) fn is_method(&self) -> bool {
        self.signature().this_offset == 1
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
            // An anonymous closure created in class scope has a declaring
            // class for visibility, but it still has no hidden method `$this`
            // parameter. Only an actual method signature reserves that slot.
            let resolved = ResolvedCallback {
                func_ptr: closure.func,
                prepend_args: vec![],
                use_vars: closure.captures.clone(),
                called_scope_class_id: closure.called_scope_class_id,
                bound_this: closure.bound_this.clone(),
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
                let (visibility, is_static, func_ptr, _) =
                    find_method_in_class_hierarchy(eg, class_name, method_name)?;
                if visibility != Visibility::Public || !is_static {
                    return None;
                }
                return Some(ResolvedCallback {
                    func_ptr,
                    prepend_args: vec![Value::null()],
                    use_vars: vec![],
                    called_scope_class_id: eg.find_class(class_name)?.class_id,
                    bound_this: None,
                });
            }
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
                let called_scope_class_id = obj.class_id;
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
                    called_scope_class_id,
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
                    called_scope_class_id: eg.find_class(class_str)?.class_id,
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
pub(crate) fn call_resolved_with_values(
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

/// filemtime($filename): int|false
fn fn_filemtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let modified = std::fs::metadata(path.as_ref()).and_then(|metadata| metadata.modified());
    match modified {
        Ok(timestamp) => {
            let seconds = match timestamp.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
                Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
            };
            ret!(rv, Value::long(seconds));
        }
        Err(_) => {
            eg.write_output(
                format!("Warning: filemtime(): stat failed for {}\n", path.as_ref()).as_bytes(),
            );
            ret!(rv, Value::bool(false));
        }
    }
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

/// is_link($filename): bool — lstat semantics also recognize broken links.
fn fn_is_link(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let is_link = std::fs::symlink_metadata(path.as_ref())
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    ret!(rv, Value::bool(is_link));
}

fn fn_chmod(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let mode = arg_long!(ed, 1) as u32;
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path.as_ref(), std::fs::Permissions::from_mode(mode)).is_ok()
    };
    #[cfg(not(unix))]
    let result = false;
    ret!(rv, Value::bool(result));
}

fn fn_fileperms(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path.as_ref()) {
            Ok(metadata) => ret!(rv, Value::long(i64::from(metadata.permissions().mode()))),
            Err(_) => ret!(rv, Value::bool(false)),
        }
    }
    #[cfg(not(unix))]
    ret!(rv, Value::bool(false));
}

fn fn_umask(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn umask(mask: u32) -> u32;
        }
        let supplied = arg_opt!(ed, 0).map(|value| value.to_long_val() as u32);
        let previous = unsafe {
            let previous = umask(supplied.unwrap_or(0));
            if supplied.is_none() {
                umask(previous);
            }
            previous
        };
        ret!(rv, Value::long(i64::from(previous)));
    }
    #[cfg(not(unix))]
    ret!(rv, Value::long(0));
}

/// dirname($path, $levels = 1): string
fn fn_dirname(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = arg_str!(ed, 0);
    let levels = arg_opt!(ed, 1).and_then(Value::as_long).unwrap_or(1);
    if levels < 1 {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "dirname(): Argument #2 ($levels) must be greater than or equal to 1",
        ));
        return Ok(());
    }
    let mut current = std::path::PathBuf::from(path.as_ref());
    for _ in 0..levels {
        let next = current.parent().map(std::path::Path::to_path_buf);
        current = match next {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            Some(_) => std::path::PathBuf::from("."),
            None if current.has_root() => current,
            None => std::path::PathBuf::from("."),
        };
    }
    ret!(rv, Value::string(current.to_string_lossy().into_owned()));
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

    let flags = arg_opt!(ed, 1).map_or(15, Value::to_long_val);
    match flags {
        1 => ret!(rv, Value::string(dirname)),
        2 => ret!(rv, Value::string(basename_str)),
        4 => ret!(rv, Value::string(extension)),
        8 => ret!(rv, Value::string(filename)),
        _ => {}
    }

    let mut arr = PhpArray::new();
    if flags & 1 != 0 {
        arr.set_str("dirname", Value::string(dirname));
    }
    if flags & 2 != 0 {
        arr.set_str("basename", Value::string(basename_str));
    }
    if flags & 4 != 0 {
        arr.set_str("extension", Value::string(extension));
    }
    if flags & 8 != 0 {
        arr.set_str("filename", Value::string(filename));
    }
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
    ret!(rv, Value::string(decode_html_entities(s.as_ref(), false)));
}

fn decode_html_entities(src: &str, decode_numeric: bool) -> String {
    // Single-pass decode to avoid chaining issues (e.g. &amp;lt; → &lt; not <).
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
            } else if src[i..].starts_with("&apos;") {
                out.push('\'');
                i += 6;
            } else if decode_numeric {
                let decoded = src[i + 1..].find(';').and_then(|relative_end| {
                    let entity = &src[i + 1..i + 1 + relative_end];
                    let codepoint = entity
                        .strip_prefix("#x")
                        .or_else(|| entity.strip_prefix("#X"))
                        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                        .or_else(|| {
                            entity
                                .strip_prefix('#')
                                .and_then(|digits| digits.parse::<u32>().ok())
                        });
                    codepoint
                        .and_then(char::from_u32)
                        .map(|character| (character, relative_end + 2))
                });
                if let Some((character, consumed)) = decoded {
                    out.push(character);
                    i += consumed;
                } else {
                    out.push('&');
                    i += 1;
                }
            } else {
                out.push('&');
                i += 1;
            }
        } else {
            let character = src[i..].chars().next().unwrap();
            out.push(character);
            i += character.len_utf8();
        }
    }
    out
}

fn fn_html_entity_decode(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(decode_html_entities(s.as_ref(), true)));
}

fn fn_filter_var(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const FILTER_VALIDATE_INT: i64 = 257;
    const FILTER_VALIDATE_BOOL: i64 = 258;
    const FILTER_VALIDATE_FLOAT: i64 = 259;
    const FILTER_VALIDATE_IP: i64 = 275;
    const FILTER_NULL_ON_FAILURE: i64 = 134_217_728;
    const FILTER_FLAG_IPV4: i64 = 1_048_576;
    const FILTER_FLAG_IPV6: i64 = 2_097_152;

    let value = arg!(ed, 0);
    let filter = arg_long!(ed, 1);
    let options = arg_opt!(ed, 2);
    let flags = options.map_or(0, |options| {
        options
            .as_array()
            .and_then(|array| array.get_str("flags"))
            .unwrap_or(options)
            .to_long_val()
    });
    let invalid = || {
        if flags & FILTER_NULL_ON_FAILURE != 0 {
            Value::null()
        } else {
            Value::bool(false)
        }
    };

    let result = match filter {
        FILTER_VALIDATE_INT => match value.value_type() {
            ValueType::Long => value.clone(),
            ValueType::String => value
                .as_str()
                .and_then(|source| source.parse::<i64>().ok())
                .map_or_else(invalid, Value::long),
            _ => invalid(),
        },
        FILTER_VALIDATE_BOOL => {
            let normalized = value.echo_to_string().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "on" | "yes" => Value::bool(true),
                "" | "0" | "false" | "off" | "no" => Value::bool(false),
                _ => invalid(),
            }
        }
        FILTER_VALIDATE_FLOAT => match value.value_type() {
            ValueType::Double => value.clone(),
            ValueType::Long => Value::double(value.as_long().unwrap_or_default() as f64),
            ValueType::String => value
                .as_str()
                .and_then(|source| source.parse::<f64>().ok())
                .map_or_else(invalid, Value::double),
            _ => invalid(),
        },
        FILTER_VALIDATE_IP => {
            let parsed = value
                .as_str()
                .and_then(|source| source.parse::<std::net::IpAddr>().ok());
            let valid = parsed.is_some_and(|address| {
                (flags & FILTER_FLAG_IPV4 == 0 || address.is_ipv4())
                    && (flags & FILTER_FLAG_IPV6 == 0 || address.is_ipv6())
            });
            if valid { value.clone() } else { invalid() }
        }
        _ => value.clone(),
    };
    ret!(rv, result);
}

fn fn_preg_quote(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg_str!(ed, 0);
    let delimiter = arg_opt!(ed, 1)
        .and_then(Value::as_str)
        .and_then(|value| value.chars().next());
    let mut quoted = String::with_capacity(source.len());
    for character in source.chars() {
        if matches!(
            character,
            '.' | '\\'
                | '+'
                | '*'
                | '?'
                | '['
                | '^'
                | ']'
                | '$'
                | '('
                | ')'
                | '{'
                | '}'
                | '='
                | '!'
                | '<'
                | '>'
                | '|'
                | ':'
                | '-'
                | '#'
        ) || delimiter == Some(character)
        {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    ret!(rv, Value::string(quoted));
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

fn array_key_value(key: &ArrayKey) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(*value),
        ArrayKey::String(value) => Value::string(value.clone()),
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
    let mut pairs = match arg!(ed, 0).as_array() {
        Some(array) => array
            .iter()
            .map(|(key, value)| (key, value.clone()))
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

    for index in 1..pairs.len() {
        let mut current = index;
        while current > 0 {
            let arguments = compare_keys
                .then(|| {
                    [
                        array_key_value(&pairs[current - 1].0),
                        array_key_value(&pairs[current].0),
                    ]
                })
                .unwrap_or_else(|| [pairs[current - 1].1.clone(), pairs[current].1.clone()]);
            let comparison = call_resolved_with_values(eg, &resolved, &arguments)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if comparison.to_long_val() <= 0 {
                break;
            }
            pairs.swap(current - 1, current);
            current -= 1;
        }
    }

    let mut sorted = PhpArray::new();
    for (key, value) in pairs {
        sorted.set(key, value);
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

fn fn_array_diff_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_array_key_membership(ed, rv, false)
}

fn fn_array_intersect_key(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_array_key_membership(ed, rv, true)
}

fn array_contains_key(array: &PhpArray, key: &ArrayKey) -> bool {
    match key {
        ArrayKey::Int(index) => array.get_int(*index).is_some(),
        ArrayKey::String(name) => array.get_str(name).is_some(),
    }
}

fn fn_array_key_membership(
    ed: *mut ExecuteData,
    rv: *mut Value,
    intersect: bool,
) -> Result<(), VmError> {
    let (Some(left), Some(second)) = (arg!(ed, 0).as_array(), arg!(ed, 1).as_array()) else {
        ret!(rv, Value::array(PhpArray::new()));
    };
    let trailing = arg!(ed, 2).as_array();
    let mut result = PhpArray::new();
    for (key, value) in left.iter() {
        let mut matches = array_contains_key(&second, &key) == intersect;
        if matches && let Some(trailing) = trailing.as_deref() {
            for candidate in trailing.values() {
                let Some(candidate) = candidate.as_array() else {
                    ret!(rv, Value::array(PhpArray::new()));
                };
                if array_contains_key(&candidate, &key) != intersect {
                    matches = false;
                    break;
                }
            }
        }
        if matches {
            result.set(key, value.clone());
        };
    }
    ret!(rv, Value::array(result));
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
    let cb_arg0_by_ref = resolved.signature().is_param_by_ref(0);

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

fn walk_array_recursive(
    eg: &mut ExecutorGlobals,
    resolved: &ResolvedCallback,
    array: &PhpArray,
    userdata: Option<&Value>,
    callback_arg0_by_ref: bool,
) -> Result<PhpArray, VmError> {
    let pairs = array
        .iter()
        .map(|(key, value)| (key, value.clone()))
        .collect::<Vec<_>>();
    let mut result = PhpArray::new();
    for (key, value) in pairs {
        let value = if let Some(nested) = value.as_array() {
            Value::array(walk_array_recursive(
                eg,
                resolved,
                nested,
                userdata,
                callback_arg0_by_ref,
            )?)
        } else {
            let key_value = match &key {
                ArrayKey::Int(key) => Value::long(*key),
                ArrayKey::String(key) => Value::string(key.clone()),
            };
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
        if eg.exception.is_some() {
            return Ok(result);
        }
        match key {
            ArrayKey::Int(key) => result.set_int(key, value),
            ArrayKey::String(key) => result.set_str(&key, value),
        }
    }
    Ok(result)
}

fn fn_array_walk_recursive(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callback = arg!(ed, 1).clone();
    let userdata = arg_opt!(ed, 2).cloned();
    let Some(array) = arg!(ed, 0).as_array() else {
        ret!(rv, Value::bool(false));
    };
    let Some(resolved) = resolve_callback_at_callsite(&callback, eg, ed) else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "array_walk_recursive(): Argument #2 ($callback) must be a valid callback",
        ));
        return Ok(());
    };
    let callback_arg0_by_ref = resolved.signature().is_param_by_ref(0);
    let result = walk_array_recursive(
        eg,
        &resolved,
        array,
        userdata.as_ref(),
        callback_arg0_by_ref,
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    arg_mut!(ed, 0, Value::array(result));
    ret!(rv, Value::bool(true));
}

/// asort(&$array): bool — sort by value, preserve keys
fn fn_asort(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(_, a), (_, b)| sort_value_cmp(a, b, flags));
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
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(_, a), (_, b)| sort_value_cmp(b, a, flags));
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
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(a, _), (b, _)| sort_key_cmp(a, b, flags));
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
    let flags = arg_opt!(ed, 1).map_or(0, Value::to_long_val);
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(php_arr) = arr.as_array() {
        let mut pairs: Vec<(ArrayKey, Value)> = php_arr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|(a, _), (b, _)| sort_key_cmp(b, a, flags));
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

fn fn_ini_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg_str!(ed, 0);
    if option.eq_ignore_ascii_case("display_errors") {
        ret!(rv, Value::string("1"));
    }
    ret!(rv, Value::bool(false));
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

fn cursor_value(
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

fn fn_reset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    cursor_value(ed, rv, PhpArray::cursor_reset)
}

fn fn_end(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(ed, rv, PhpArray::cursor_end)
}

fn fn_current(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    cursor_value(ed, rv, PhpArray::cursor_current)
}

fn fn_next(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(ed, rv, PhpArray::cursor_next)
}

fn fn_prev(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    cursor_value(ed, rv, PhpArray::cursor_prev)
}

/// key($array): int|string|null for the array's current internal cursor.
fn fn_key(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(key) = arg!(ed, 0).as_array().and_then(PhpArray::cursor_key) {
        match key {
            ArrayKey::Int(key) => ret!(rv, Value::long(key)),
            ArrayKey::String(key) => ret!(rv, Value::string(key)),
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
    let subject = arg_str!(ed, 2).into_owned();
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
        regex_callback::replace(&re, subject, &callback, limit, flags & 512 != 0, ed, eg)?
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
