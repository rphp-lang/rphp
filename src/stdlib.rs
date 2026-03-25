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

use crate::regex::{Regex, parse_php_regex};

use crate::value::{Value, ValueType, PhpArray, ArrayKey};
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::runtime::ExecutorGlobals;
use crate::compiler::{make_internal_function, make_internal_function_ref, make_internal_function_variadic, make_internal_method};
use crate::vm::function::InternalFunction;
use crate::vm::execute::{call_function, VmError};
use crate::parser::Visibility;

// ============================================================================
// Helper macros — zero-cost abstractions for stdlib handlers
// ============================================================================

/// Read CV(n) as &Value — follows references transparently
#[allow(unused_unsafe)]
macro_rules! arg {
    ($ed:expr, $n:expr) => {{
        let v = unsafe { (*$ed).cv($n) };
        if v.is_reference() { unsafe { &*v.as_ref_ptr() } } else { v }
    }};
}

/// Read CV(n) as *mut Value — follows references (returns pointer to original)
#[allow(unused_unsafe)]
macro_rules! arg_mut {
    ($ed:expr, $n:expr) => {{
        let ptr = unsafe { (*$ed).cv_mut($n) as *mut Value };
        if unsafe { (*ptr).is_reference() } { unsafe { (*ptr).as_ref_ptr() } } else { ptr }
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
        if v.value_type() == ValueType::Undef { None } else { Some(v) }
    }};
}

/// Write value to return_value pointer (with null guard) and return Ok(()).
/// SAFETY: rv must be a valid pointer or null.
macro_rules! ret {
    ($rv:expr, $val:expr) => {{
        if !$rv.is_null() {
            unsafe { $rv.write($val) };
        }
        return Ok(());
    }};
}

// ============================================================================
// Registration
// ============================================================================

/// Register all stdlib functions into the executor globals.
/// The returned Vec must live as long as the EG (owns the InternalFunction structs).
pub fn register_stdlib(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut funcs: Vec<Box<InternalFunction>> = Vec::with_capacity(80);

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
    reg_ref!("array_unshift", fn_array_unshift, 2, 2, 0b1, "array", "value");
    reg!("array_key_exists", fn_array_key_exists, 2, 2, "key", "array");
    reg!("in_array", fn_in_array, 2, 2, "needle", "haystack");
    reg!("array_reverse", fn_array_reverse, 1, 1, "array");
    reg!("array_merge", fn_array_merge, 2, 2, "array1", "array2");
    reg!("array_keys", fn_array_keys, 1, 1, "array");
    reg!("array_values", fn_array_values, 1, 1, "array");
    reg!("array_slice", fn_array_slice, 3, 2, "array", "offset", "length");
    reg!("array_unique", fn_array_unique, 1, 1, "array");
    reg!("array_flip", fn_array_flip, 1, 1, "array");
    reg!("array_combine", fn_array_combine, 2, 2, "keys", "values");
    reg!("array_sum", fn_array_sum, 1, 1, "array");
    reg!("array_product", fn_array_product, 1, 1, "array");
    reg!("array_count_values", fn_array_count_values, 1, 1, "array");
    reg!("array_fill", fn_array_fill, 3, 3, "start_index", "count", "value");
    reg!("array_pad", fn_array_pad, 3, 3, "array", "length", "value");
    reg!("array_chunk", fn_array_chunk, 2, 2, "array", "length");
    reg!("array_column", fn_array_column, 2, 2, "array", "column_key");
    reg_ref!("sort", fn_sort, 1, 1, 0b1, "array");
    reg_ref!("rsort", fn_rsort, 1, 1, 0b1, "array");
    reg!("array_search", fn_array_search, 2, 2, "needle", "haystack");
    reg!("range", fn_range, 2, 2, "start", "end");
    reg_ref!("array_splice", fn_array_splice, 4, 2, 0b1, "array", "offset", "length", "replacement");
    reg!("array_rand", fn_array_rand, 1, 1, "array");
    reg_ref!("shuffle", fn_shuffle, 1, 1, 0b1, "array");
    reg!("array_map", fn_array_map, 2, 2, "callback", "array");
    reg!("array_filter", fn_array_filter, 2, 1, "array", "callback");
    // compact() requires caller scope access (not yet implemented) — intentionally not registered

    // --- String functions ---
    reg!("strlen", fn_strlen, 1, 1, "string");
    reg!("substr", fn_substr, 3, 2, "string", "offset", "length");
    reg!("strpos", fn_strpos, 2, 2, "haystack", "needle");
    reg!("strrpos", fn_strrpos, 2, 2, "haystack", "needle");
    reg!("str_replace", fn_str_replace, 3, 3, "search", "replace", "subject");
    reg!("strtolower", fn_strtolower, 1, 1, "string");
    reg!("strtoupper", fn_strtoupper, 1, 1, "string");
    reg!("trim", fn_trim, 1, 1, "string");
    reg!("rtrim", fn_rtrim, 1, 1, "string");
    reg!("ltrim", fn_ltrim, 1, 1, "string");
    reg!("explode", fn_explode, 2, 2, "separator", "string");
    reg!("implode", fn_implode, 2, 2, "separator", "array");
    reg!("join", fn_implode, 2, 2, "separator", "array");
    reg!("str_repeat", fn_str_repeat, 2, 2, "string", "times");
    reg!("substr_count", fn_substr_count, 2, 2, "haystack", "needle");
    reg!("str_contains", fn_str_contains, 2, 2, "haystack", "needle");
    reg!("str_starts_with", fn_str_starts_with, 2, 2, "haystack", "needle");
    reg!("str_ends_with", fn_str_ends_with, 2, 2, "haystack", "needle");
    reg!("str_pad", fn_str_pad, 3, 2, "string", "length", "pad_string");
    reg!("str_split", fn_str_split, 2, 1, "string", "length");
    reg!("ucfirst", fn_ucfirst, 1, 1, "string");
    reg!("lcfirst", fn_lcfirst, 1, 1, "string");
    reg!("str_word_count", fn_str_word_count, 1, 1, "string");
    reg!("wordwrap", fn_wordwrap, 4, 1, "string", "width", "break_str", "cut_long_words");
    reg!("nl2br", fn_nl2br, 1, 1, "string");
    reg!("str_rev", fn_str_rev, 1, 1, "string");
    reg!("number_format", fn_number_format, 4, 1, "num", "decimals", "decimal_separator", "thousands_separator");
    reg!("ord", fn_ord, 1, 1, "character");
    reg!("chr", fn_chr, 1, 1, "codepoint");
    reg_var!("sprintf", fn_sprintf, 1, "format");

    // --- Regex functions ---
    reg_ref!("preg_match", fn_preg_match, 3, 2, 0b100, "pattern", "subject", "matches");
    reg!("preg_replace", fn_preg_replace, 3, 3, "pattern", "replacement", "subject");

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
    reg!("class_exists", fn_class_exists, 2, 1, "class_name", "autoload");
    reg!("method_exists", fn_method_exists, 2, 2, "object_or_class", "method");

    // --- Math functions ---
    reg!("abs", fn_abs, 1, 1, "num");
    reg!("max", fn_max, 2, 2, "value1", "value2");
    reg!("min", fn_min, 2, 2, "value1", "value2");
    reg!("floor", fn_floor, 1, 1, "num");
    reg!("ceil", fn_ceil, 1, 1, "num");
    reg!("round", fn_round, 2, 1, "num", "precision");
    reg!("pow", fn_pow, 2, 2, "base", "exponent");
    reg!("sqrt", fn_sqrt, 1, 1, "num");
    reg!("intdiv", fn_intdiv, 2, 2, "dividend", "divisor");
    reg!("fmod", fn_fmod, 2, 2, "x", "y");
    reg!("log", fn_log, 1, 1, "num");
    reg!("log10", fn_log10, 1, 1, "num");
    reg!("log2", fn_log2, 1, 1, "num");
    reg!("pi", fn_pi, 0, 0);
    reg!("rand", fn_rand, 2, 0, "min", "max");
    reg!("mt_rand", fn_rand, 2, 0, "min", "max");

    // --- Output ---
    reg!("var_dump", fn_var_dump, 1, 1, "value");
    reg!("print_r", fn_print_r, 1, 1, "value");
    reg!("var_export", fn_var_export, 2, 1, "value", "return");

    // --- Constants ---
    reg!("define", fn_define, 2, 2, "constant_name", "value");
    reg!("defined", fn_defined, 1, 1, "constant_name");
    reg!("constant", fn_constant, 1, 1, "name");

    // --- JSON ---
    reg!("json_encode", fn_json_encode, 1, 1, "value");
    reg!("json_decode", fn_json_decode, 2, 1, "json", "associative");

    // --- Misc ---
    reg!("isset_func", fn_isset_func, 1, 1, "value");
    reg!("empty_func", fn_empty_func, 1, 1, "value");
    reg!("unset_func", fn_unset_func, 1, 1, "value");

    // --- Callable functions ---
    reg_var!("call_user_func", fn_call_user_func, 1, "callback");
    reg!("is_callable", fn_is_callable, 1, 1, "value");

    // --- Time functions ---
    reg!("microtime", fn_microtime, 1, 0, "as_float");
    reg!("hrtime", fn_hrtime, 1, 0, "as_nanoseconds");
    reg!("time", fn_time, 0, 0);

    funcs
}

// ============================================================================
// Built-in exception classes (Throwable hierarchy)
// ============================================================================

/// Internal handler for Error/Exception __construct($message = "")
/// CV 0 = $this, CV 1 = $message
fn fn_throwable_construct(ed: *mut ExecuteData, _rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    let message = arg_opt!(ed, 1);
    if let Some(mut obj) = this_val.as_object_mut() {
        let msg = match message {
            Some(v) => v.clone(),
            None => Value::string(""),
        };
        obj.properties.insert("message".to_string(), msg);
    }
    Ok(())
}

/// Internal handler for Error/Exception getMessage()
/// CV 0 = $this
fn fn_throwable_get_message(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        let msg = obj.properties.get("message").cloned().unwrap_or(Value::string(""));
        ret!(rv, msg);
    }
    ret!(rv, Value::string(""));
}

/// Register Throwable, Error, TypeError, Exception classes with
/// __construct and getMessage methods.
pub fn register_builtin_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use crate::compiler::compile::ClassDef;
    use crate::parser::Visibility;

    let mut funcs: Vec<Box<InternalFunction>> = Vec::new();

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
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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
        properties: vec![("message".to_string(), Some(Value::string("")), Visibility::Protected, "Exception".to_string())],
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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
        properties: vec![("message".to_string(), Some(Value::string("")), Visibility::Protected, "Error".to_string())],
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

    // Register __construct and getMessage for each throwable class
    // num_args = 2 for __construct (CV 0 = $this, CV 1 = $message)
    // num_args = 1 for getMessage (CV 0 = $this)
    for class in &["Throwable", "Exception", "Error", "TypeError", "ArgumentCountError", "UnhandledMatchError"] {
        // __construct: num_args=2 (CV 0=$this, CV 1=$message), required=0 ($message is optional)
        reg_method!(class, "__construct", fn_throwable_construct, 2, 0, "message");
        // getMessage: num_args=1 (CV 0=$this), required=0 (no explicit args)
        reg_method!(class, "getmessage", fn_throwable_get_message, 1, 0);
    }

    // Generator class — implements Iterator
    eg.register_class(ClassDef {
        name: "Generator".to_string(),
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    }).unwrap();

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

fn fn_count(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let n = match v.as_array() {
        Some(arr) => arr.len() as i64,
        None => if v.value_type() == ValueType::Null { 0 } else { 1 },
    };
    ret!(rv, Value::long(n));
}

fn fn_array_push(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_array_pop(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        ret!(rv, a.pop().unwrap_or(Value::null()));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_shift(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        ret!(rv, a.shift().unwrap_or(Value::null()));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_unshift(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let val = arg!(ed, 1).clone();
    let arr = unsafe { &mut *ptr };
    if let Some(a) = arr.as_array_mut() {
        // Rebuild with val at front
        let mut new = PhpArray::new();
        new.push(val);
        for (key, v) in a.entries().iter() {
            match key {
                ArrayKey::Int(_) => new.push(v.clone()),
                ArrayKey::String(k) => new.set_str(k, v.clone()),
            }
        }
        *arr = Value::array(new);
        ret!(rv, Value::long(arr.as_array().map(|a| a.len()).unwrap_or(0) as i64));
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_key_exists(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_in_array(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1);
    let found = haystack.as_array()
        .map(|a| a.entries().iter().any(|(_, v)| values_equal(needle, v)))
        .unwrap_or(false);
    ret!(rv, Value::bool(found));
}

fn fn_array_reverse(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut new = PhpArray::new();
        for (key, val) in arr.entries().iter().rev() {
            match key {
                ArrayKey::Int(_) => new.push(val.clone()),
                ArrayKey::String(k) => new.set_str(k, val.clone()),
            }
        }
        ret!(rv, Value::array(new));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_merge(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a1 = arg!(ed, 0);
    let a2 = arg!(ed, 1);
    if let (Some(a1), Some(a2)) = (a1.as_array(), a2.as_array()) {
        let mut merged = PhpArray::new();
        for (key, val) in a1.entries() {
            match key {
                ArrayKey::Int(_) => merged.push(val.clone()),
                ArrayKey::String(k) => merged.set_str(k, val.clone()),
            }
        }
        for (key, val) in a2.entries() {
            match key {
                ArrayKey::Int(_) => merged.push(val.clone()),
                ArrayKey::String(k) => merged.set_str(k, val.clone()),
            }
        }
        ret!(rv, Value::array(merged));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_keys(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for (key, _) in arr.entries() {
            match key {
                ArrayKey::Int(k) => result.push(Value::long(*k)),
                ArrayKey::String(k) => result.push(Value::string(k.clone())),
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_values(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for (_, val) in arr.entries() {
            result.push(val.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_slice(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    if let Some(arr) = arr_arg.as_array() {
        let len = arr.len() as i64;
        let raw_offset = arg_long!(ed, 1);
        let start = if raw_offset < 0 { (len + raw_offset).max(0) as usize } else { raw_offset as usize };
        let end = match arg_opt!(ed, 2) {
            Some(v) => {
                let l = v.to_long_val();
                if l < 0 { (len + l).max(start as i64) as usize } else { (start + l as usize).min(arr.len()) }
            }
            None => arr.len(),
        };
        let mut result = PhpArray::new();
        for (_, val) in arr.entries().iter().skip(start).take(end.saturating_sub(start)) {
            result.push(val.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_unique(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        let mut seen: Vec<String> = Vec::with_capacity(arr.len());
        for (key, val) in arr.entries() {
            let s = val.echo_to_string();
            if !seen.contains(&s) {
                seen.push(s);
                result.set(key.clone(), val.clone());
            }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_flip(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut result = PhpArray::new();
        for (key, val) in arr.entries() {
            let new_key = match val.value_type() {
                ValueType::Long => ArrayKey::Int(val.as_long().unwrap()),
                ValueType::String => ArrayKey::String(val.as_str().unwrap().to_string()),
                _ => continue,
            };
            let new_val = match key {
                ArrayKey::Int(k) => Value::long(*k),
                ArrayKey::String(k) => Value::string(k.clone()),
            };
            result.set(new_key, new_val);
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_combine(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let keys_arg = arg!(ed, 0);
    let vals_arg = arg!(ed, 1);
    if let (Some(keys), Some(vals)) = (keys_arg.as_array(), vals_arg.as_array()) {
        let mut result = PhpArray::new();
        for (k, v) in keys.entries().iter().zip(vals.entries().iter()) {
            let key = match &k.1 {
                val if val.as_str().is_some() => ArrayKey::String(val.as_str().unwrap().to_string()),
                val => ArrayKey::Int(val.as_long().unwrap_or(0)),
            };
            result.set(key, v.1.clone());
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_array_sum(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut has_float = false;
        let mut sum_int: i64 = 0;
        let mut sum_float: f64 = 0.0;
        for (_, val) in arr.entries() {
            match val.value_type() {
                ValueType::Long => sum_int = sum_int.wrapping_add(val.as_long().unwrap()),
                ValueType::Double => { has_float = true; sum_float += val.as_double().unwrap(); }
                _ => {}
            }
        }
        ret!(rv, if has_float { Value::double(sum_float + sum_int as f64) } else { Value::long(sum_int) });
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_product(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut has_float = false;
        let mut prod_int: i64 = 1;
        let mut prod_float: f64 = 1.0;
        for (_, val) in arr.entries() {
            match val.value_type() {
                ValueType::Long => prod_int = prod_int.wrapping_mul(val.as_long().unwrap()),
                ValueType::Double => { has_float = true; prod_float *= val.as_double().unwrap(); }
                _ => {}
            }
        }
        ret!(rv, if has_float { Value::double(prod_float * prod_int as f64) } else { Value::long(prod_int) });
    } else {
        ret!(rv, Value::long(0));
    }
}

fn fn_array_count_values(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        let mut counts: Vec<(String, i64)> = Vec::new();
        for (_, val) in arr.entries() {
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

fn fn_array_fill(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let start = arg_long!(ed, 0) as i64;
    let count = arg_long!(ed, 1).max(0) as usize;
    let value = arg!(ed, 2);
    let mut result = PhpArray::new();
    for i in 0..count {
        result.set_int(start + i as i64, value.clone());
    }
    ret!(rv, Value::array(result));
}

fn fn_array_pad(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let size = arg_long!(ed, 1);
    let value = arg!(ed, 2);
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let abs_size = size.unsigned_abs() as usize;
        let pad_count = abs_size.saturating_sub(arr.len());
        if size < 0 {
            for _ in 0..pad_count { result.push(value.clone()); }
        }
        for (_, v) in arr.entries() { result.push(v.clone()); }
        if size >= 0 {
            for _ in 0..pad_count { result.push(value.clone()); }
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_array_chunk(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let size = arg_long!(ed, 1).max(1) as usize;
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let mut chunk = PhpArray::new();
        let mut i = 0;
        for (_, v) in arr.entries() {
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

fn fn_array_column(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr_arg = arg!(ed, 0);
    let col_key = arg!(ed, 1);
    if let Some(arr) = arr_arg.as_array() {
        let mut result = PhpArray::new();
        let key_str = col_key.echo_to_string();
        for (_, row) in arr.entries() {
            if let Some(inner) = row.as_array() {
                // Try string key first, then integer
                let val = inner.get_str(&key_str)
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
        let mut entries: Vec<Value> = a.entries().iter().map(|(_, v)| v.clone()).collect();
        entries.sort_by(|a, b| cmp_val(compare_values(a, b)));
        let mut new = PhpArray::new();
        for v in entries { new.push(v); }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_rsort(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.entries().iter().map(|(_, v)| v.clone()).collect();
        entries.sort_by(|a, b| cmp_val(compare_values(b, a)));
        let mut new = PhpArray::new();
        for v in entries { new.push(v); }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

fn fn_array_search(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let needle = arg!(ed, 0);
    let haystack = arg!(ed, 1);
    if let Some(arr) = haystack.as_array() {
        for (key, val) in arr.entries() {
            if values_equal(needle, val) {
                let result = match key {
                    ArrayKey::Int(k) => Value::long(*k),
                    ArrayKey::String(k) => Value::string(k.clone()),
                };
                ret!(rv, result);
            }
        }
    }
    ret!(rv, Value::bool(false));
}

fn fn_range(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let start = arg_long!(ed, 0);
    let end = arg_long!(ed, 1);
    let mut arr = PhpArray::new();
    if start <= end {
        for i in start..=end { arr.push(Value::long(i)); }
    } else {
        for i in (end..=start).rev() { arr.push(Value::long(i)); }
    }
    ret!(rv, Value::array(arr));
}

fn fn_array_splice(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let offset = arg_long!(ed, 1);
    let arr = unsafe { &mut *ptr };
    if let Some(a) = arr.as_array_mut() {
        let len = a.len() as i64;
        let start = if offset < 0 { (len + offset).max(0) as usize } else { (offset as usize).min(a.len()) };
        let del_count = match arg_opt!(ed, 2) {
            Some(v) => v.to_long_val().max(0) as usize,
            None => a.len() - start,
        };
        let replacement = arg_opt!(ed, 3).and_then(|v| v.as_array());

        let entries: Vec<(ArrayKey, Value)> = a.entries().to_vec();
        let mut removed = PhpArray::new();
        let mut new = PhpArray::new();

        for (i, (_, v)) in entries.iter().enumerate() {
            if i < start || i >= start + del_count {
                new.push(v.clone());
            } else {
                removed.push(v.clone());
                if i == start {
                    if let Some(repl) = replacement {
                        for (_, rv) in repl.entries() { new.push(rv.clone()); }
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

fn fn_array_rand(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if let Some(arr) = v.as_array() {
        if arr.is_empty() {
            ret!(rv, Value::null());
        } else {
            // Simple pseudo-random using wrapping arithmetic
            let idx = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as usize) % arr.len();
            let (key, _) = &arr.entries()[idx];
            let result = match key {
                ArrayKey::Int(k) => Value::long(*k),
                ArrayKey::String(k) => Value::string(k.clone()),
            };
            ret!(rv, result);
        }
    } else {
        ret!(rv, Value::null());
    }
}

fn fn_shuffle(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr = unsafe { &mut *arg_mut!(ed, 0) };
    if let Some(a) = arr.as_array_mut() {
        let mut entries: Vec<Value> = a.entries().iter().map(|(_, v)| v.clone()).collect();
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
        for v in entries { new.push(v); }
        *arr = Value::array(new);
        ret!(rv, Value::bool(true));
    } else {
        ret!(rv, Value::bool(false));
    }
}

/// array_map($callback, $array) — apply callback to each element, return new array
fn fn_array_map(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let callback_name = arg_str!(ed, 0);
    let arr_val = arg!(ed, 1);
    let func_ptr = match eg.find_function(&callback_name) {
        Some(ptr) => ptr,
        None => {
            eg.exception = Some(crate::value::make_error_value("TypeError", &format!(
                "array_map(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found", callback_name
            )));
            return Ok(());
        }
    };
    if let Some(arr) = arr_val.as_array() {
        let mut result = PhpArray::new();
        for (key, val) in arr.entries().iter() {
            let mapped = call_function(eg, func_ptr, &[val.clone()])?;
            if eg.exception.is_some() { return Ok(()); }
            result.set(key.clone(), mapped);
        }
        ret!(rv, Value::array(result));
    } else {
        ret!(rv, Value::null());
    }
}

/// array_filter($array [, $callback]) — filter elements by callback (or truthiness)
fn fn_array_filter(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let arr_val = arg!(ed, 0);
    let callback = arg_opt!(ed, 1);
    if let Some(arr) = arr_val.as_array() {
        let mut result = PhpArray::new();
        match callback {
            Some(cb_val) => {
                let cb_name = match cb_val.as_str() {
                    Some(s) => s.to_string(),
                    None => cb_val.echo_to_string(),
                };
                let func_ptr = match eg.find_function(&cb_name) {
                    Some(ptr) => ptr,
                    None => {
                        eg.exception = Some(crate::value::make_error_value("TypeError", &format!(
                            "array_filter(): Argument #2 ($callback) must be a valid callback, function \"{}\" not found", cb_name
                        )));
                        return Ok(());
                    }
                };
                for (key, val) in arr.entries().iter() {
                    let ret_val = call_function(eg, func_ptr, &[val.clone()])?;
                    if eg.exception.is_some() { return Ok(()); }
                    if ret_val.is_truthy() {
                        result.set(key.clone(), val.clone());
                    }
                }
            }
            None => {
                // No callback — filter by truthiness
                for (key, val) in arr.entries().iter() {
                    if val.is_truthy() {
                        result.set(key.clone(), val.clone());
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

fn fn_strlen(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::long(s.len() as i64));
}

fn fn_substr(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;
    let start_raw = arg_long!(ed, 1);
    let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { start_raw as usize };
    let end = match arg_opt!(ed, 2) {
        Some(v) => {
            let l = v.to_long_val();
            if l < 0 { ((len + l) as usize).max(start) } else { (start + l as usize).min(bytes.len()) }
        }
        None => bytes.len(),
    };
    if start >= bytes.len() {
        ret!(rv, Value::string(""));
    } else {
        ret!(rv, Value::string(String::from_utf8_lossy(&bytes[start..end]).into_owned()));
    }
}

fn fn_strpos(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, match h.find(n.as_ref()) {
        Some(pos) => Value::long(pos as i64),
        None => Value::bool(false),
    });
}

fn fn_strrpos(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, match h.rfind(n.as_ref()) {
        Some(pos) => Value::long(pos as i64),
        None => Value::bool(false),
    });
}

fn fn_str_replace(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let search = arg_str!(ed, 0);
    let replace = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);
    ret!(rv, Value::string(subject.replace(search.as_ref(), replace.as_ref())));
}

fn fn_strtolower(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    // PHP strtolower is ASCII-only — use make_ascii_lowercase for performance
    let mut bytes = s.as_bytes().to_vec();
    bytes.make_ascii_lowercase();
    ret!(rv, Value::string(unsafe { String::from_utf8_unchecked(bytes) }));
}

fn fn_strtoupper(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let mut bytes = s.as_bytes().to_vec();
    bytes.make_ascii_uppercase();
    ret!(rv, Value::string(unsafe { String::from_utf8_unchecked(bytes) }));
}

fn fn_trim(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim()));
}

fn fn_rtrim(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim_end()));
}

fn fn_ltrim(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.trim_start()));
}

fn fn_explode(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let d = arg_str!(ed, 0);
    let s = arg_str!(ed, 1);
    let mut arr = PhpArray::new();
    for part in s.split(d.as_ref()) {
        arr.push(Value::string(part));
    }
    ret!(rv, Value::array(arr));
}

fn fn_implode(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let glue = arg_str!(ed, 0);
    let pieces = arg!(ed, 1);
    if let Some(arr) = pieces.as_array() {
        let parts: Vec<String> = arr.entries().iter().map(|(_, v)| v.echo_to_string()).collect();
        ret!(rv, Value::string(parts.join(glue.as_ref())));
    } else {
        ret!(rv, Value::string(""));
    }
}

fn fn_str_repeat(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let times = arg_long!(ed, 1).max(0) as usize;
    ret!(rv, Value::string(s.repeat(times)));
}

fn fn_substr_count(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    let count = if n.is_empty() { 0 } else { h.matches(n.as_ref()).count() as i64 };
    ret!(rv, Value::long(count));
}

fn fn_str_contains(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.contains(n.as_ref())));
}

fn fn_str_starts_with(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.starts_with(n.as_ref())));
}

fn fn_str_ends_with(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let h = arg_str!(ed, 0);
    let n = arg_str!(ed, 1);
    ret!(rv, Value::bool(h.ends_with(n.as_ref())));
}

fn fn_str_pad(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_str_split(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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
        arr.push(Value::string(String::from_utf8_lossy(&bytes[i..end]).into_owned()));
        i = end;
    }
    if arr.is_empty() { arr.push(Value::string("")); }
    ret!(rv, Value::array(arr));
}

fn fn_ucfirst(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    if s.is_empty() {
        ret!(rv, Value::string(""));
    } else {
        // PHP ucfirst is ASCII-only
        let mut bytes = s.as_bytes().to_vec();
        bytes[0] = bytes[0].to_ascii_uppercase();
        ret!(rv, Value::string(unsafe { String::from_utf8_unchecked(bytes) }));
    }
}

fn fn_lcfirst(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    if s.is_empty() {
        ret!(rv, Value::string(""));
    } else {
        let mut bytes = s.as_bytes().to_vec();
        bytes[0] = bytes[0].to_ascii_lowercase();
        ret!(rv, Value::string(unsafe { String::from_utf8_unchecked(bytes) }));
    }
}

fn fn_str_word_count(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::long(s.split_whitespace().count() as i64));
}

fn fn_wordwrap(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_nl2br(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::string(s.replace('\n', "<br />\n")));
}

fn fn_str_rev(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    // PHP strrev reverses bytes, not Unicode codepoints
    let reversed: String = s.chars().rev().collect();
    ret!(rv, Value::string(reversed));
}

fn fn_number_format(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let num = arg_float!(ed, 0);
    let decimals = match arg_opt!(ed, 1) { Some(v) => v.to_long_val().max(0) as usize, None => 0 };
    let dec_point = match arg_opt!(ed, 2) { Some(v) => v.as_str().unwrap_or(".").to_string(), None => ".".to_string() };
    let thousands_sep = match arg_opt!(ed, 3) { Some(v) => v.as_str().unwrap_or(",").to_string(), None => ",".to_string() };

    let formatted = format!("{:.prec$}", num, prec = decimals);
    let parts: Vec<&str> = formatted.split('.').collect();
    let int_part = parts[0];
    let negative = int_part.starts_with('-');
    let digits = if negative { &int_part[1..] } else { int_part };

    let mut with_sep = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            if let Some(sep_ch) = thousands_sep.chars().next() {
                with_sep.insert(0, sep_ch);
            }
        }
        with_sep.insert(0, ch);
    }
    if negative { with_sep.insert(0, '-'); }

    let result = if decimals > 0 {
        format!("{}{}{}", with_sep, dec_point, parts.get(1).unwrap_or(&""))
    } else {
        with_sep
    };
    ret!(rv, Value::string(result));
}

fn fn_ord(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    ret!(rv, Value::long(s.as_bytes().first().copied().unwrap_or(0) as i64));
}

fn fn_chr(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let code = (arg_long!(ed, 0) & 0xFF) as u8;
    ret!(rv, Value::string(String::from(code as char)));
}

fn fn_sprintf(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let fmt = arg_str!(ed, 0);
    // Variadic: VM packs extra args into an array at CV(1)
    // Read individual values from that array
    let variadic_arr = arg!(ed, 1);
    let args: Vec<Value> = if let Some(arr) = variadic_arr.as_array() {
        arr.entries().iter().map(|(_, v)| v.clone()).collect()
    } else if variadic_arr.value_type() != ValueType::Undef {
        // Single non-array arg (non-variadic call path)
        vec![variadic_arr.clone()]
    } else {
        vec![]
    };

    let mut result = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0;
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => { chars.next(); result.push('%'); }
                Some(&spec) => {
                    chars.next();
                    let arg = args.get(arg_idx);
                    arg_idx += 1;
                    match spec {
                        's' => result.push_str(&arg.map(|a| a.echo_to_string()).unwrap_or_default()),
                        'd' => result.push_str(&arg.map(|a| a.to_long_val().to_string()).unwrap_or("0".into())),
                        'f' => {
                            let d = arg.map(|a| a.to_float_val()).unwrap_or(0.0);
                            result.push_str(&format!("{:.6}", d));
                        }
                        'x' => result.push_str(&format!("{:x}", arg.map(|a| a.to_long_val()).unwrap_or(0))),
                        'X' => result.push_str(&format!("{:X}", arg.map(|a| a.to_long_val()).unwrap_or(0))),
                        'o' => result.push_str(&format!("{:o}", arg.map(|a| a.to_long_val()).unwrap_or(0))),
                        'b' => result.push_str(&format!("{:b}", arg.map(|a| a.to_long_val()).unwrap_or(0))),
                        'c' => {
                            let code = arg.map(|a| a.to_long_val()).unwrap_or(0);
                            result.push((code & 0xFF) as u8 as char);
                        }
                        _ => { result.push('%'); result.push(spec); arg_idx -= 1; }
                    }
                }
                None => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }
    ret!(rv, Value::string(result));
}

// ============================================================================
// Type functions
// ============================================================================

fn fn_intval(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::long(arg!(ed, 0).to_long_val()));
}

fn fn_strval(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::string(arg!(ed, 0).echo_to_string()));
}

fn fn_floatval(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg!(ed, 0).to_float_val()));
}

fn fn_boolval(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).is_truthy()));
}

fn fn_settype(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    let type_name = arg_str!(ed, 1);
    let val = unsafe { &*ptr };
    let new_val = match type_name.as_ref() {
        "int" | "integer" => Value::long(val.to_long_val()),
        "float" | "double" => Value::double(val.to_float_val()),
        "string" => Value::string(val.echo_to_string()),
        "bool" | "boolean" => Value::bool(val.is_truthy()),
        "array" => {
            if val.value_type() == ValueType::Array { val.clone() }
            else { let mut a = PhpArray::new(); a.push(val.clone()); Value::array(a) }
        }
        "null" => Value::null(),
        _ => { ret!(rv, Value::bool(false)); }
    };
    unsafe { std::ptr::drop_in_place(ptr); ptr.write(new_val); }
    ret!(rv, Value::bool(true));
}

fn fn_is_array(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Array));
}

fn fn_is_string(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::String));
}

fn fn_is_int(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Long));
}

fn fn_is_float(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Double));
}

fn fn_is_null(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Null));
}

fn fn_is_bool(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let t = arg!(ed, 0).value_type();
    ret!(rv, Value::bool(t == ValueType::True || t == ValueType::False));
}

fn fn_is_numeric(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_is_object(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(arg!(ed, 0).value_type() == ValueType::Object));
}

fn fn_gettype(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let name = match arg!(ed, 0).value_type() {
        ValueType::Null => "NULL",
        ValueType::True | ValueType::False => "boolean",
        ValueType::Long => "integer",
        ValueType::Double => "double",
        ValueType::String => "string",
        ValueType::Array => "array",
        ValueType::Object => "object",
        ValueType::Resource => "resource",
        _ => "unknown type",
    };
    ret!(rv, Value::string(name));
}

// ============================================================================
// Reflection / class introspection
// ============================================================================

fn fn_get_class(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    if v.value_type() == ValueType::Undef {
        // No argument — return the current class name (deprecated in PHP 8 but still works)
        let caller_class = get_calling_scope_class(ed, eg);
        if let Some(cls) = caller_class {
            eg.write_output(b"Deprecated: Calling get_class() without arguments is deprecated\n");
            ret!(rv, Value::string(&cls));
        }
        // Outside class scope: PHP throws Error
        eg.exception = Some(crate::value::make_error_value(
            "Error", "get_class() without arguments must be called from within a class"
        ));
        return Ok(());
    }
    if let Some(obj) = v.as_object() {
        ret!(rv, Value::string(&obj.class_name));
    }
    ret!(rv, Value::bool(false));
}

fn fn_class_exists(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    let lower = name.to_ascii_lowercase();
    let found = eg.class_table.iter().any(|(k, cd)| {
        k.to_ascii_lowercase() == lower && !cd.is_interface && !cd.is_trait
    });
    ret!(rv, Value::bool(found));
}

fn fn_method_exists(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let first = arg!(ed, 0);
    let method_name = arg_str!(ed, 1);

    // Resolve the class name: from object or string
    let class_name: String = if let Some(obj) = first.as_object() {
        obj.class_name.clone()
    } else if let Some(s) = first.as_str() {
        s.to_string()
    } else {
        ret!(rv, Value::bool(false));
    };

    // Look up method in class hierarchy (own + parent chain + traits)
    let found = find_method_in_class_hierarchy(eg, &class_name, &method_name).is_some();
    ret!(rv, Value::bool(found));
}

// ============================================================================
// Math functions
// ============================================================================

fn fn_abs(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    ret!(rv, match v.value_type() {
        ValueType::Long => Value::long(v.as_long().unwrap().abs()),
        ValueType::Double => Value::double(v.as_double().unwrap().abs()),
        _ => Value::long(0),
    });
}

fn fn_max(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg!(ed, 0);
    let b = arg!(ed, 1);
    ret!(rv, if compare_values(a, b) >= 0 { a.clone() } else { b.clone() });
}

fn fn_min(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg!(ed, 0);
    let b = arg!(ed, 1);
    ret!(rv, if compare_values(a, b) <= 0 { a.clone() } else { b.clone() });
}

fn fn_floor(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).floor()));
}

fn fn_ceil(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).ceil()));
}

fn fn_round(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let d = arg_float!(ed, 0);
    let precision = match arg_opt!(ed, 1) { Some(v) => v.to_long_val(), None => 0 };
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
    ret!(rv, Value::double(base.to_float_val().powf(exp.to_float_val())));
}

fn fn_sqrt(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).sqrt()));
}

fn fn_intdiv(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg_long!(ed, 0);
    let b = arg_long!(ed, 1);
    if b == 0 {
        ret!(rv, Value::bool(false)); // PHP throws DivisionByZeroError
    } else {
        ret!(rv, Value::long(a / b));
    }
}

fn fn_fmod(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let a = arg_float!(ed, 0);
    let b = arg_float!(ed, 1);
    ret!(rv, Value::double(a % b));
}

fn fn_log(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::double(arg_float!(ed, 0).ln()));
}

fn fn_log10(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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
        Some(v) => (v.to_long_val(), match arg_opt!(ed, 1) { Some(v2) => v2.to_long_val(), None => i32::MAX as i64 }),
        None => (0, i32::MAX as i64),
    };
    let range = (hi - lo + 1).max(1);
    let val = lo + (seed as i64 % range);
    ret!(rv, Value::long(val));
}

// ============================================================================
// Output functions
// ============================================================================

fn fn_var_dump(ed: *mut ExecuteData, _rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let output = var_dump_value(v, 0);
    eg.write_output(output.as_bytes());
    Ok(())
}

fn fn_print_r(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let output = print_r_value(v, 0);
    eg.write_output(output.as_bytes());
    ret!(rv, Value::bool(true));
}

fn fn_var_export(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    let return_str = match arg_opt!(ed, 1) { Some(v) => v.is_truthy(), None => false };
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

fn fn_define(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    if name.is_empty() {
        ret!(rv, Value::bool(false));
    }
    let val = arg!(ed, 1).clone();
    let result = eg.define_constant(&name, val);
    ret!(rv, Value::bool(result.is_ok()));
}

fn fn_defined(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    ret!(rv, Value::bool(eg.find_constant(&name).is_some()));
}

fn fn_constant(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let name = arg_str!(ed, 0);
    ret!(rv, eg.find_constant(&name).unwrap_or(Value::null()));
}

// ============================================================================
// JSON functions
// ============================================================================

fn fn_json_encode(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    ret!(rv, Value::string(json_encode_value(v)));
}

fn fn_json_decode(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let s = arg_str!(ed, 0);
    let assoc = match arg_opt!(ed, 1) { Some(v) => v.is_truthy(), None => false };
    ret!(rv, json_decode_string(&s, assoc));
}

// ============================================================================
// Misc functions
// ============================================================================

fn fn_isset_func(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let v = arg!(ed, 0);
    ret!(rv, Value::bool(v.value_type() != ValueType::Null && v.value_type() != ValueType::Undef));
}

fn fn_empty_func(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::bool(!arg!(ed, 0).is_truthy()));
}

fn fn_unset_func(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let ptr = arg_mut!(ed, 0);
    unsafe { std::ptr::drop_in_place(ptr); ptr.write(Value::null()); }
    ret!(rv, Value::null());
}

// ============================================================================
// Helpers
// ============================================================================

#[inline]
fn cmp_val(cmp: i32) -> std::cmp::Ordering {
    if cmp < 0 { std::cmp::Ordering::Less }
    else if cmp > 0 { std::cmp::Ordering::Greater }
    else { std::cmp::Ordering::Equal }
}

fn compare_values(a: &Value, b: &Value) -> i32 {
    let ad = a.to_float_val();
    let bd = b.to_float_val();
    if ad < bd { -1 } else if ad > bd { 1 } else { 0 }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a.value_type(), b.value_type()) {
        (ValueType::Long, ValueType::Long) => a.as_long() == b.as_long(),
        (ValueType::String, ValueType::String) => a.as_str() == b.as_str(),
        (ValueType::Long, ValueType::Double) | (ValueType::Double, ValueType::Long) |
        (ValueType::Double, ValueType::Double) => a.to_double() == b.to_double(),
        (ValueType::Null, ValueType::Null) => true,
        (ValueType::True, ValueType::True) | (ValueType::False, ValueType::False) => true,
        (ValueType::String, ValueType::Long) | (ValueType::Long, ValueType::String) => {
            let (s_val, i_val) = if a.value_type() == ValueType::String { (a, b) } else { (b, a) };
            s_val.as_str().unwrap().parse::<i64>().ok() == i_val.as_long()
        }
        _ => false,
    }
}

fn var_dump_value(val: &Value, indent: usize) -> String {
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
            for (key, v) in arr.entries() {
                let key_str = match key {
                    ArrayKey::Int(k) => format!("[{}]", k),
                    ArrayKey::String(k) => format!("[\"{}\"]", k),
                };
                out.push_str(&format!("{}  {}=>\n", prefix, key_str));
                out.push_str(&var_dump_value(v, indent + 1));
            }
            out.push_str(&format!("{}}}\n", prefix));
            out
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
            if d == d.floor() && d.abs() < 1e15 { format!("{}", d as i64) } else { format!("{}", d) }
        }
        ValueType::String => val.as_str().unwrap().to_string(),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let prefix = "    ".repeat(indent);
            let inner = "    ".repeat(indent + 1);
            let mut out = "Array\n".to_string();
            out.push_str(&format!("{}(\n", prefix));
            for (key, v) in arr.entries() {
                let key_str = match key {
                    ArrayKey::Int(k) => format!("{}", k),
                    ArrayKey::String(k) => k.clone(),
                };
                out.push_str(&format!("{}[{}] => {}", inner, key_str, print_r_value(v, indent + 1)));
                if v.value_type() != ValueType::Array { out.push('\n'); }
            }
            out.push_str(&format!("{})\n", prefix));
            out
        }
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
        ValueType::String => format!("'{}'", val.as_str().unwrap().replace('\\', "\\\\").replace('\'', "\\'")),
        ValueType::Array => {
            let arr = val.as_array().unwrap();
            let mut out = "array (\n".to_string();
            for (key, v) in arr.entries() {
                let key_str = match key {
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
        ValueType::Long => serde_json::Value::Number(
            serde_json::Number::from(val.as_long().unwrap())
        ),
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
            let is_list = arr.entries().iter().enumerate().all(|(i, (k, _))| {
                matches!(k, ArrayKey::Int(n) if *n == i as i64)
            });
            if is_list {
                serde_json::Value::Array(
                    arr.entries().iter().map(|(_, v)| value_to_json(v)).collect()
                )
            } else {
                let mut map = serde_json::Map::new();
                for (k, v) in arr.entries() {
                    let key = match k {
                        ArrayKey::Int(n) => n.to_string(),
                        ArrayKey::String(s) => s.clone(),
                    };
                    map.insert(key, value_to_json(v));
                }
                serde_json::Value::Object(map)
            }
        }
        ValueType::Object => {
            if let Some(obj) = val.as_object() {
                let mut map = serde_json::Map::new();
                for (k, v) in &obj.properties {
                    map.insert(k.clone(), value_to_json(v));
                }
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

/// Convert serde_json::Value to PHP Value.
fn json_to_value(jv: serde_json::Value, assoc: bool) -> Value {
    match jv {
        serde_json::Value::Null => Value::null(),
        serde_json::Value::Bool(b) => Value::bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::long(i)
            } else {
                Value::double(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::string(s),
        serde_json::Value::Array(items) => {
            let mut arr = PhpArray::new();
            for item in items {
                arr.push(json_to_value(item, assoc));
            }
            Value::array(arr)
        }
        serde_json::Value::Object(map) => {
            if assoc {
                let mut arr = PhpArray::new();
                for (k, v) in map {
                    arr.set_str(&k, json_to_value(v, assoc));
                }
                Value::array(arr)
            } else {
                use crate::value::PhpObject;
                use std::collections::HashMap;
                let mut props = HashMap::new();
                for (k, v) in map {
                    props.insert(k, json_to_value(v, false));
                }
                Value::object(PhpObject {
                    class_name: "stdClass".to_string(),
                    class_id: 0,
                    properties: props,
                    generator: None,
                })
            }
        }
    }
}

fn json_decode_string(s: &str, assoc: bool) -> Value {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(jv) => json_to_value(jv, assoc),
        Err(_) => Value::null(),
    }
}

// ============================================================================
// Generator methods
// ============================================================================

/// Helper: extract GeneratorRef from $this (CV 0)
fn get_generator_ref(ed: *mut ExecuteData) -> Option<crate::vm::generator::GeneratorRef> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        if obj.class_name == "Generator" {
            if let Some(rc) = this_val.as_object_rc() {
                let borrowed = rc.borrow();
                return borrowed.generator.clone();
            }
        }
    }
    None
}

/// Ensure generator is started (first next/send/rewind triggers initial execution)
fn ensure_generator_started(gen_ref: &crate::vm::generator::GeneratorRef, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    use crate::vm::generator::GeneratorState;
    let state = gen_ref.borrow().state;
    if state == GeneratorState::Created {
        crate::vm::execute::resume_generator(eg, gen_ref, Value::null())?;
    }
    Ok(())
}

fn fn_generator_current(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
        let val = gen_ref.borrow().value.clone();
        ret!(rv, val);
    }
    ret!(rv, Value::null());
}

fn fn_generator_key(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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

fn fn_generator_next(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
        // Advance past current yield
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Suspended {
            crate::vm::execute::resume_generator(eg, &gen_ref, Value::null())?;
        }
    }
    ret!(rv, Value::null());
}

fn fn_generator_valid(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        ret!(rv, Value::bool(is_valid));
    }
    ret!(rv, Value::bool(false));
}

fn fn_generator_rewind(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        ensure_generator_started(&gen_ref, eg)?;
    }
    ret!(rv, Value::null());
}

fn fn_generator_send(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        let send_val = arg!(ed, 1).clone();

        // PHP semantics: ensure_initialized first, then inject send value.
        // If Created: start generator (runs to first yield), THEN resume with send value.
        // If Suspended: resume with send value directly.
        let state = gen_ref.borrow().state;
        if state == crate::vm::generator::GeneratorState::Created {
            // Start generator — runs to first yield, sets up send_target
            crate::vm::execute::resume_generator(eg, &gen_ref, Value::null())?;
            // Now resume with the actual send value (if still suspended)
            let state2 = gen_ref.borrow().state;
            if state2 == crate::vm::generator::GeneratorState::Suspended {
                crate::vm::execute::resume_generator(eg, &gen_ref, send_val)?;
            }
        } else if state == crate::vm::generator::GeneratorState::Suspended {
            crate::vm::execute::resume_generator(eg, &gen_ref, send_val)?;
        }

        // Return current yielded value
        let val = gen_ref.borrow().value.clone();
        ret!(rv, val);
    }
    ret!(rv, Value::null());
}

fn fn_generator_get_return(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(gen_ref) = get_generator_ref(ed) {
        let gen_data = gen_ref.borrow();
        if gen_data.state != crate::vm::generator::GeneratorState::Completed {
            return Err(VmError::Fatal("Cannot get return value of a generator that hasn't returned".into()));
        }
        ret!(rv, gen_data.return_value.clone());
    }
    ret!(rv, Value::null());
}

// ============================================================================
// Regex (PCRE) functions
// ============================================================================

/// preg_match($pattern, $subject [, &$matches]) -> int (0 or 1)
fn fn_preg_match(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let subject = arg_str!(ed, 1);

    let has_matches = {
        let raw = unsafe { (*ed).cv(2) };
        !raw.is_undef()
    };

    let (pat, flags) = match parse_php_regex(&pattern_str) {
        Ok(v) => v,
        Err(_e) => {
            // PHP emits a warning and returns false for invalid patterns
            ret!(rv, Value::bool(false));
        }
    };
    let re = match Regex::new(&pat, flags) {
        Ok(r) => r,
        Err(_e) => {
            ret!(rv, Value::bool(false));
        }
    };

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
                unsafe { std::ptr::drop_in_place(matches_ptr); matches_ptr.write(Value::array(arr)); }
            }
            ret!(rv, Value::long(1));
        }
        None => {
            if has_matches {
                let matches_ptr = arg_mut!(ed, 2);
                unsafe { std::ptr::drop_in_place(matches_ptr); matches_ptr.write(Value::array(PhpArray::new())); }
            }
            ret!(rv, Value::long(0));
        }
    }
}

/// preg_replace($pattern, $replacement, $subject) -> string
fn fn_preg_replace(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let pattern_str = arg_str!(ed, 0);
    let replacement = arg_str!(ed, 1);
    let subject = arg_str!(ed, 2);

    let (pat, flags) = match parse_php_regex(&pattern_str) {
        Ok(v) => v,
        Err(_e) => {
            ret!(rv, Value::null());
        }
    };
    let re = match Regex::new(&pat, flags) {
        Ok(r) => r,
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

/// Search for a method in a class hierarchy (own methods + parent chain + trait uses).
/// Returns Some((visibility, is_static)) if found.
/// Returns (visibility, is_static, declaring_class_lower) for a method in the hierarchy.
fn find_method_in_class_hierarchy(
    eg: &ExecutorGlobals,
    class_name: &str,
    method_name: &str,
) -> Option<(Visibility, bool, String)> {
    let method_lower = method_name.to_ascii_lowercase();
    let mut current = Some(class_name.to_ascii_lowercase());
    while let Some(ref cls) = current {
        let entry = eg.class_table.iter().find(|(k, _)| k.to_ascii_lowercase() == *cls);
        if let Some((_, cd)) = entry {
            // Check own methods
            if let Some((_, vis, is_static, _, _)) = cd.methods.iter().find(|(m, _, _, _, _)| m.to_ascii_lowercase() == method_lower) {
                return Some((*vis, *is_static, cls.clone()));
            }
            // Check trait-composed methods
            for trait_name in &cd.uses {
                let trait_lower = trait_name.to_ascii_lowercase();
                if let Some((_, trait_def)) = eg.class_table.iter().find(|(k, _)| k.to_ascii_lowercase() == trait_lower) {
                    if let Some((_, vis, is_static, _, _)) = trait_def.methods.iter().find(|(m, _, _, _, _)| m.to_ascii_lowercase() == method_lower) {
                        return Some((*vis, *is_static, cls.clone()));
                    }
                }
            }
            current = cd.parent.as_ref().map(|p| p.to_ascii_lowercase());
        } else {
            break;
        }
    }
    None
}

/// Result of resolving a callback: func pointer + args to prepend (e.g. $this, use_vars).
struct ResolvedCallback {
    func_ptr: *const FunctionCommon,
    /// Args to prepend before user-supplied args.
    /// For plain functions: empty.
    /// For method calls: [$this].
    /// For closures: use_vars (appended after user args, not prepended).
    prepend_args: Vec<Value>,
    /// Captured use_vars for closures (appended after all params).
    use_vars: Vec<Value>,
}

/// Get the calling scope's class name from an ExecuteData frame.
/// Walks prev_execute_data to find the caller's declaring class.
fn get_calling_scope_class(ed: *mut crate::vm::frame::ExecuteData, eg: &ExecutorGlobals) -> Option<String> {
    if ed.is_null() { return None; }
    // ed is the stdlib function's own frame; the caller is prev_execute_data
    let caller = unsafe { (*ed).prev_execute_data };
    if caller.is_null() { return None; }
    let func = unsafe { (*caller).func };
    if func.is_null() { return None; }
    eg.declaring_class_of(func).map(|s| s.to_string())
}

/// Resolve a callback value to a function pointer.
/// Supports: string (function name), array [func_name, use_vars...] (closure),
/// array [object, "method"], and objects with __invoke.
/// `caller_class` is the class scope of the call site — used to allow
/// private/protected method callbacks when called from the declaring class.
fn resolve_callback(val: &Value, eg: &ExecutorGlobals, caller_class: Option<&str>) -> Option<ResolvedCallback> {
    match val.value_type() {
        ValueType::String => {
            let name = val.as_str().unwrap();
            eg.find_function(name).map(|ptr| ResolvedCallback {
                func_ptr: ptr, prepend_args: vec![], use_vars: vec![],
            })
        }
        ValueType::Array => {
            let arr = val.as_array()?;
            let entries = arr.entries();
            if entries.is_empty() { return None; }

            // Case 1: Closure descriptor array [func_name_string, use_val1, ...]
            if let Some(func_name) = entries[0].1.as_str() {
                if func_name.starts_with("__closure_") {
                    let func_ptr = eg.find_function(func_name)?;
                    let use_vars: Vec<Value> = entries.iter().skip(1).map(|(_, v)| v.clone()).collect();
                    return Some(ResolvedCallback {
                        func_ptr, prepend_args: vec![], use_vars,
                    });
                }
            }

            // Case 2: Method callback [object_or_class, "method_name"]
            if entries.len() != 2 { return None; }
            let obj_val = &entries[0].1;
            let method_val = &entries[1].1;
            let method_name = method_val.as_str()?;
            if let Some(obj) = obj_val.as_object() {
                // Instance method: [$obj, "method"]
                // Public: always callable. Private/protected: only from declaring scope.
                let class_name = obj.class_name.clone();
                drop(obj);
                match find_method_in_class_hierarchy(eg, &class_name, method_name) {
                    Some((Visibility::Public, _, _)) => {}
                    Some((Visibility::Protected, _, _)) => {
                        // Protected: caller must be in the same hierarchy
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(&class_name, cc) || eg.class_is_a(cc, &class_name)
                        });
                        if !allowed { return None; }
                    }
                    Some((Visibility::Private, _, ref declaring)) => {
                        // Private: caller must be exactly the declaring class
                        let allowed = caller_class.map_or(false, |cc| {
                            cc.to_ascii_lowercase() == *declaring
                        });
                        if !allowed { return None; }
                    }
                    _ => return None,
                }
                let full = format!("{}::{}", class_name, method_name).to_lowercase();
                // Try exact class first, then walk parents for function_table lookup
                let func_ptr = eg.function_table.get(&full).copied()
                    .or_else(|| {
                        // Walk parent chain for the function_table entry
                        let mut cur = eg.class_table.iter()
                            .find(|(k, _)| k.to_ascii_lowercase() == class_name.to_ascii_lowercase())
                            .and_then(|(_, cd)| cd.parent.clone());
                        while let Some(ref parent) = cur {
                            let key = format!("{}::{}", parent, method_name).to_lowercase();
                            if let Some(&ptr) = eg.function_table.get(&key) {
                                return Some(ptr);
                            }
                            cur = eg.class_table.iter()
                                .find(|(k, _)| k.to_ascii_lowercase() == parent.to_ascii_lowercase())
                                .and_then(|(_, cd)| cd.parent.clone());
                        }
                        None
                    });
                func_ptr.map(|ptr| ResolvedCallback {
                    func_ptr: ptr, prepend_args: vec![obj_val.clone()], use_vars: vec![],
                })
            } else if let Some(class_str) = obj_val.as_str() {
                // Static method: ["ClassName", "method"] — must be static; visibility depends on scope
                match find_method_in_class_hierarchy(eg, class_str, method_name) {
                    Some((Visibility::Public, true, _)) => {}
                    Some((Visibility::Protected, true, _)) => {
                        let allowed = caller_class.map_or(false, |cc| {
                            eg.class_is_a(class_str, cc) || eg.class_is_a(cc, class_str)
                        });
                        if !allowed { return None; }
                    }
                    Some((Visibility::Private, true, ref declaring)) => {
                        let allowed = caller_class.map_or(false, |cc| {
                            cc.to_ascii_lowercase() == *declaring
                        });
                        if !allowed { return None; }
                    }
                    _ => return None,
                }
                let full = format!("{}::{}", class_str, method_name).to_lowercase();
                let func_ptr = eg.function_table.get(&full).copied()
                    .or_else(|| {
                        let mut cur = eg.class_table.iter()
                            .find(|(k, _)| k.to_ascii_lowercase() == class_str.to_ascii_lowercase())
                            .and_then(|(_, cd)| cd.parent.clone());
                        while let Some(ref parent) = cur {
                            let key = format!("{}::{}", parent, method_name).to_lowercase();
                            if let Some(&ptr) = eg.function_table.get(&key) {
                                return Some(ptr);
                            }
                            cur = eg.class_table.iter()
                                .find(|(k, _)| k.to_ascii_lowercase() == parent.to_ascii_lowercase())
                                .and_then(|(_, cd)| cd.parent.clone());
                        }
                        None
                    });
                func_ptr.map(|ptr| ResolvedCallback {
                    func_ptr: ptr, prepend_args: vec![Value::null()], use_vars: vec![],
                })
            } else {
                None
            }
        }
        ValueType::Object => {
            let obj = val.as_object()?;
            let full = format!("{}::__invoke", obj.class_name).to_lowercase();
            drop(obj);
            eg.function_table.get(&full).map(|&ptr| ResolvedCallback {
                func_ptr: ptr, prepend_args: vec![val.clone()], use_vars: vec![],
            })
        }
        _ => None,
    }
}

/// call_user_func($callback, ...$args)
/// CV 0 = callback, CV 1 = variadic array of extra args
fn fn_call_user_func(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let callback = arg!(ed, 0);
    let caller_class = get_calling_scope_class(ed, eg);
    // Variadic args packed at CV(1)
    let variadic_val = arg!(ed, 1);
    let extra_args: Vec<Value> = if let Some(arr) = variadic_val.as_array() {
        arr.entries().iter().map(|(_, v)| v.clone()).collect()
    } else if variadic_val.value_type() != ValueType::Undef {
        vec![variadic_val.clone()]
    } else {
        vec![]
    };

    let resolved = match resolve_callback(callback, eg, caller_class.as_deref()) {
        Some(r) => r,
        None => {
            let desc = callback.echo_to_string();
            eg.exception = Some(crate::value::make_error_value("TypeError", &format!(
                "call_user_func(): Argument #1 ($callback) must be a valid callback, function \"{}\" not found or not callable", desc
            )));
            return Ok(());
        }
    };

    // Build args: prepend_args (e.g. $this) + user args + use_vars (closure captures)
    let mut args = Vec::with_capacity(resolved.prepend_args.len() + extra_args.len() + resolved.use_vars.len());
    args.extend(resolved.prepend_args);
    args.extend(extra_args);
    args.extend(resolved.use_vars);

    let result = call_function(eg, resolved.func_ptr, &args)?;
    if eg.exception.is_some() { return Ok(()); }
    ret!(rv, result);
}

/// is_callable($value) — check if value is callable
fn fn_is_callable(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let val = arg!(ed, 0);
    let caller_class = get_calling_scope_class(ed, eg);
    let callable = resolve_callback(val, eg, caller_class.as_deref()).is_some();
    ret!(rv, Value::bool(callable));
}

// ============================================================================
// Time functions
// ============================================================================

/// microtime(bool $as_float = false): string|float
/// Returns current Unix timestamp with microsecond precision.
fn fn_microtime(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let as_float = arg_opt!(ed, 0).map(|v| v.is_truthy()).unwrap_or(false);
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
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
fn fn_hrtime(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
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
fn fn_time(_ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    ret!(rv, Value::long(secs as i64));
}
