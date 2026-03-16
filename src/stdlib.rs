/// Standard library — built-in PHP functions.
/// Each function follows the InternalFunctionHandler signature.

use crate::value::{Value, ValueType, PhpArray, ArrayKey};
use crate::vm::frame::ExecuteData;
use crate::vm::function::FunctionCommon;
use crate::runtime::ExecutorGlobals;
use crate::compiler::{make_internal_function};
use crate::vm::function::InternalFunction;

/// Register all stdlib functions into the executor globals.
/// The returned Vec must live as long as the EG (owns the InternalFunction structs).
pub fn register_stdlib(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut funcs: Vec<Box<InternalFunction>> = Vec::new();

    macro_rules! reg {
        ($name:expr, $handler:expr, $max_args:expr, $min_args:expr) => {{
            let f = Box::new(make_internal_function($handler, $max_args, $min_args));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    // --- Array functions ---
    reg!("count", fn_count, 1, 1);
    reg!("array_push", fn_array_push, 2, 2);
    reg!("array_pop", fn_array_pop, 1, 1);
    reg!("array_shift", fn_array_shift, 1, 1);
    reg!("array_key_exists", fn_array_key_exists, 2, 2);
    reg!("in_array", fn_in_array, 2, 2);
    reg!("array_reverse", fn_array_reverse, 1, 1);
    reg!("array_merge", fn_array_merge, 2, 2);

    // --- String functions ---
    reg!("strlen", fn_strlen, 1, 1);
    reg!("substr", fn_substr, 3, 2);
    reg!("strpos", fn_strpos, 2, 2);
    reg!("str_replace", fn_str_replace, 3, 3);
    reg!("strtolower", fn_strtolower, 1, 1);
    reg!("strtoupper", fn_strtoupper, 1, 1);
    reg!("trim", fn_trim, 1, 1);
    reg!("explode", fn_explode, 2, 2);
    reg!("implode", fn_implode, 2, 2);
    reg!("str_repeat", fn_str_repeat, 2, 2);
    reg!("substr_count", fn_substr_count, 2, 2);
    reg!("str_contains", fn_str_contains, 2, 2);
    reg!("str_starts_with", fn_str_starts_with, 2, 2);
    reg!("str_ends_with", fn_str_ends_with, 2, 2);

    // --- Type functions ---
    reg!("intval", fn_intval, 1, 1);
    reg!("strval", fn_strval, 1, 1);
    reg!("floatval", fn_floatval, 1, 1);
    reg!("is_array", fn_is_array, 1, 1);
    reg!("is_string", fn_is_string, 1, 1);
    reg!("is_int", fn_is_int, 1, 1);
    reg!("is_null", fn_is_null, 1, 1);
    reg!("is_bool", fn_is_bool, 1, 1);
    reg!("is_numeric", fn_is_numeric, 1, 1);
    reg!("gettype", fn_gettype, 1, 1);

    // --- Math functions ---
    reg!("abs", fn_abs, 1, 1);
    reg!("max", fn_max, 2, 2);
    reg!("min", fn_min, 2, 2);
    reg!("floor", fn_floor, 1, 1);
    reg!("ceil", fn_ceil, 1, 1);
    reg!("round", fn_round, 1, 1);
    reg!("pow", fn_pow, 2, 2);
    reg!("sqrt", fn_sqrt, 1, 1);

    // --- Output ---
    reg!("var_dump", fn_var_dump, 1, 1);
    reg!("print_r", fn_print_r, 1, 1);

    // --- Constants ---
    reg!("define", fn_define, 2, 2);
    reg!("defined", fn_defined, 1, 1);
    reg!("constant", fn_constant, 1, 1);

    funcs
}

// ============================================================================
// Array functions
// ============================================================================

fn fn_count(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let count = match arg.as_array() {
        Some(arr) => arr.len() as i64,
        None => match arg.value_type() {
            ValueType::Null => 0,
            _ => 1,
        },
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::long(count)) };
    }
}

fn fn_array_push(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arr_ptr = unsafe { (*execute_data).cv_mut(0) as *mut Value };
    let val = unsafe { (*execute_data).cv(1) }.clone();
    let arr = unsafe { &mut *arr_ptr };
    if let Some(php_arr) = arr.as_array_mut() {
        php_arr.push(val);
        let new_count = php_arr.len() as i64;
        if !return_value.is_null() {
            unsafe { return_value.write(Value::long(new_count)) };
        }
    } else {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::null()) };
        }
    }
}

fn fn_array_pop(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arr_ptr = unsafe { (*execute_data).cv_mut(0) as *mut Value };
    let arr = unsafe { &mut *arr_ptr };
    if let Some(php_arr) = arr.as_array_mut() {
        let popped = php_arr.pop();
        if !return_value.is_null() {
            unsafe { return_value.write(popped.unwrap_or(Value::null())) };
        }
    } else {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::null()) };
        }
    }
}

fn fn_array_shift(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arr_ptr = unsafe { (*execute_data).cv_mut(0) as *mut Value };
    let arr = unsafe { &mut *arr_ptr };
    if let Some(php_arr) = arr.as_array_mut() {
        let shifted = php_arr.shift();
        if !return_value.is_null() {
            unsafe { return_value.write(shifted.unwrap_or(Value::null())) };
        }
    } else {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::null()) };
        }
    }
}

fn fn_array_key_exists(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let key = unsafe { (*execute_data).cv(0) };
    let arr = unsafe { (*execute_data).cv(1) };
    let exists = if let Some(php_arr) = arr.as_array() {
        match key.value_type() {
            ValueType::Long => php_arr.get_int(key.as_long().unwrap()).is_some(),
            ValueType::String => php_arr.get_str(key.as_str().unwrap()).is_some(),
            _ => false,
        }
    } else {
        false
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(exists)) };
    }
}

fn fn_in_array(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let needle = unsafe { (*execute_data).cv(0) };
    let haystack = unsafe { (*execute_data).cv(1) };
    let found = if let Some(arr) = haystack.as_array() {
        arr.entries().iter().any(|(_, v)| values_equal(needle, v))
    } else {
        false
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(found)) };
    }
}

fn fn_array_reverse(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if let Some(arr) = arg.as_array() {
        let mut new_arr = PhpArray::new();
        // PHP array_reverse re-indexes integer keys, preserves string keys
        for (key, val) in arr.entries().iter().rev() {
            match key {
                ArrayKey::Int(_) => new_arr.push(val.clone()),
                ArrayKey::String(k) => new_arr.set_str(k, val.clone()),
            }
        }
        if !return_value.is_null() {
            unsafe { return_value.write(Value::array(new_arr)) };
        }
    } else {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::null()) };
        }
    }
}

fn fn_array_merge(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arr1 = unsafe { (*execute_data).cv(0) };
    let arr2 = unsafe { (*execute_data).cv(1) };
    if let (Some(a1), Some(a2)) = (arr1.as_array(), arr2.as_array()) {
        let mut merged = PhpArray::new();
        // Integer keys get re-indexed, string keys preserved
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
        if !return_value.is_null() {
            unsafe { return_value.write(Value::array(merged)) };
        }
    } else {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::null()) };
        }
    }
}

// ============================================================================
// String functions
// ============================================================================

fn fn_strlen(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let len = match arg.as_str() {
        Some(s) => s.len() as i64,
        None => arg.echo_to_string().len() as i64,
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::long(len)) };
    }
}

fn fn_substr(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let str_arg = unsafe { (*execute_data).cv(0) };
    let start_arg = unsafe { (*execute_data).cv(1) };
    let s = str_arg.as_str().map(|s| s.to_string()).unwrap_or_else(|| str_arg.echo_to_string());
    let bytes = s.as_bytes();
    let len = bytes.len() as i64;
    let start_raw = start_arg.as_long().unwrap_or(0);
    let start = if start_raw < 0 {
        (len + start_raw).max(0) as usize
    } else {
        start_raw as usize
    };

    // Check if 3rd argument (length) was passed
    let length_arg = unsafe { (*execute_data).cv(2) };
    let end = if length_arg.value_type() != ValueType::Undef {
        let l = length_arg.as_long().unwrap_or(0);
        if l < 0 {
            ((len + l) as usize).max(start)
        } else {
            (start + l as usize).min(bytes.len())
        }
    } else {
        bytes.len()
    };

    let result = if start >= bytes.len() {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes[start..end]).to_string()
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(result)) };
    }
}

fn fn_strpos(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let haystack = unsafe { (*execute_data).cv(0) };
    let needle = unsafe { (*execute_data).cv(1) };
    let h = haystack.as_str().map(|s| s.to_string()).unwrap_or_else(|| haystack.echo_to_string());
    let n = needle.as_str().map(|s| s.to_string()).unwrap_or_else(|| needle.echo_to_string());
    let result = match h.find(&n) {
        Some(pos) => Value::long(pos as i64),
        None => Value::bool(false),
    };
    if !return_value.is_null() {
        unsafe { return_value.write(result) };
    }
}

fn fn_str_replace(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let search = unsafe { (*execute_data).cv(0) };
    let replace = unsafe { (*execute_data).cv(1) };
    let subject = unsafe { (*execute_data).cv(2) };
    let s = subject.as_str().map(|s| s.to_string()).unwrap_or_else(|| subject.echo_to_string());
    let from = search.as_str().map(|s| s.to_string()).unwrap_or_else(|| search.echo_to_string());
    let to = replace.as_str().map(|s| s.to_string()).unwrap_or_else(|| replace.echo_to_string());
    let result = s.replace(&from, &to);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(result)) };
    }
}

fn fn_strtolower(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let s = arg.as_str().map(|s| s.to_string()).unwrap_or_else(|| arg.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(s.to_lowercase())) };
    }
}

fn fn_strtoupper(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let s = arg.as_str().map(|s| s.to_string()).unwrap_or_else(|| arg.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(s.to_uppercase())) };
    }
}

fn fn_trim(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let s = arg.as_str().map(|s| s.to_string()).unwrap_or_else(|| arg.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(s.trim().to_string())) };
    }
}

fn fn_explode(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let delimiter = unsafe { (*execute_data).cv(0) };
    let string = unsafe { (*execute_data).cv(1) };
    let d = delimiter.as_str().map(|s| s.to_string()).unwrap_or_else(|| delimiter.echo_to_string());
    let s = string.as_str().map(|s| s.to_string()).unwrap_or_else(|| string.echo_to_string());
    let mut arr = PhpArray::new();
    for part in s.split(&d) {
        arr.push(Value::string(part.to_string()));
    }
    if !return_value.is_null() {
        unsafe { return_value.write(Value::array(arr)) };
    }
}

fn fn_implode(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let glue = unsafe { (*execute_data).cv(0) };
    let pieces = unsafe { (*execute_data).cv(1) };
    let g = glue.as_str().map(|s| s.to_string()).unwrap_or_else(|| glue.echo_to_string());
    let result = if let Some(arr) = pieces.as_array() {
        let parts: Vec<String> = arr.entries().iter().map(|(_, v)| v.echo_to_string()).collect();
        parts.join(&g)
    } else {
        String::new()
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(result)) };
    }
}

fn fn_str_repeat(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let times_arg = unsafe { (*execute_data).cv(1) };
    let s = arg.as_str().map(|s| s.to_string()).unwrap_or_else(|| arg.echo_to_string());
    let times = times_arg.as_long().unwrap_or(0).max(0) as usize;
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(s.repeat(times))) };
    }
}

fn fn_substr_count(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let haystack = unsafe { (*execute_data).cv(0) };
    let needle = unsafe { (*execute_data).cv(1) };
    let h = haystack.as_str().map(|s| s.to_string()).unwrap_or_else(|| haystack.echo_to_string());
    let n = needle.as_str().map(|s| s.to_string()).unwrap_or_else(|| needle.echo_to_string());
    let count = if n.is_empty() { 0 } else { h.matches(&n).count() as i64 };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::long(count)) };
    }
}

fn fn_str_contains(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let haystack = unsafe { (*execute_data).cv(0) };
    let needle = unsafe { (*execute_data).cv(1) };
    let h = haystack.as_str().map(|s| s.to_string()).unwrap_or_else(|| haystack.echo_to_string());
    let n = needle.as_str().map(|s| s.to_string()).unwrap_or_else(|| needle.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(h.contains(&n))) };
    }
}

fn fn_str_starts_with(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let haystack = unsafe { (*execute_data).cv(0) };
    let needle = unsafe { (*execute_data).cv(1) };
    let h = haystack.as_str().map(|s| s.to_string()).unwrap_or_else(|| haystack.echo_to_string());
    let n = needle.as_str().map(|s| s.to_string()).unwrap_or_else(|| needle.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(h.starts_with(&n))) };
    }
}

fn fn_str_ends_with(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let haystack = unsafe { (*execute_data).cv(0) };
    let needle = unsafe { (*execute_data).cv(1) };
    let h = haystack.as_str().map(|s| s.to_string()).unwrap_or_else(|| haystack.echo_to_string());
    let n = needle.as_str().map(|s| s.to_string()).unwrap_or_else(|| needle.echo_to_string());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(h.ends_with(&n))) };
    }
}

// ============================================================================
// Type functions
// ============================================================================

fn fn_intval(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let result = match arg.value_type() {
        ValueType::Long => arg.as_long().unwrap(),
        ValueType::Double => arg.as_double().unwrap() as i64,
        ValueType::True => 1,
        ValueType::False | ValueType::Null => 0,
        ValueType::String => {
            let s = arg.as_str().unwrap();
            // PHP intval: parse leading integer portion
            parse_leading_int(s)
        }
        _ => 0,
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::long(result)) };
    }
}

fn fn_strval(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(arg.echo_to_string())) };
    }
}

fn fn_floatval(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let result = match arg.value_type() {
        ValueType::Double => arg.as_double().unwrap(),
        ValueType::Long => arg.as_long().unwrap() as f64,
        ValueType::True => 1.0,
        ValueType::False | ValueType::Null => 0.0,
        ValueType::String => {
            let s = arg.as_str().unwrap();
            s.trim().parse::<f64>().unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(result)) };
    }
}

fn fn_is_array(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(arg.value_type() == ValueType::Array)) };
    }
}

fn fn_is_string(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(arg.value_type() == ValueType::String)) };
    }
}

fn fn_is_int(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(arg.value_type() == ValueType::Long)) };
    }
}

fn fn_is_null(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(arg.value_type() == ValueType::Null)) };
    }
}

fn fn_is_bool(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let is_bool = arg.value_type() == ValueType::True || arg.value_type() == ValueType::False;
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(is_bool)) };
    }
}

fn fn_is_numeric(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let is_num = match arg.value_type() {
        ValueType::Long | ValueType::Double => true,
        ValueType::String => {
            let s = arg.as_str().unwrap().trim();
            s.parse::<i64>().is_ok() || s.parse::<f64>().is_ok()
        }
        _ => false,
    };
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(is_num)) };
    }
}

fn fn_gettype(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let type_name = match arg.value_type() {
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
    if !return_value.is_null() {
        unsafe { return_value.write(Value::string(type_name)) };
    }
}

// ============================================================================
// Math functions
// ============================================================================

fn fn_abs(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let result = match arg.value_type() {
        ValueType::Long => Value::long(arg.as_long().unwrap().abs()),
        ValueType::Double => Value::double(arg.as_double().unwrap().abs()),
        _ => Value::long(0),
    };
    if !return_value.is_null() {
        unsafe { return_value.write(result) };
    }
}

fn fn_max(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let a = unsafe { (*execute_data).cv(0) };
    let b = unsafe { (*execute_data).cv(1) };
    let result = if compare_values(a, b) >= 0 { a.clone() } else { b.clone() };
    if !return_value.is_null() {
        unsafe { return_value.write(result) };
    }
}

fn fn_min(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let a = unsafe { (*execute_data).cv(0) };
    let b = unsafe { (*execute_data).cv(1) };
    let result = if compare_values(a, b) <= 0 { a.clone() } else { b.clone() };
    if !return_value.is_null() {
        unsafe { return_value.write(result) };
    }
}

fn fn_floor(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let d = arg.to_double().unwrap_or(0.0);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(d.floor())) };
    }
}

fn fn_ceil(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let d = arg.to_double().unwrap_or(0.0);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(d.ceil())) };
    }
}

fn fn_round(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let d = arg.to_double().unwrap_or(0.0);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(d.round())) };
    }
}

fn fn_pow(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let base = unsafe { (*execute_data).cv(0) };
    let exp = unsafe { (*execute_data).cv(1) };
    // If both are integers and exponent is non-negative, return integer
    if let (Some(b), Some(e)) = (base.as_long(), exp.as_long()) {
        if e >= 0 {
            if !return_value.is_null() {
                unsafe { return_value.write(Value::long(b.wrapping_pow(e as u32))) };
            }
            return;
        }
    }
    let b = base.to_double().unwrap_or(0.0);
    let e = exp.to_double().unwrap_or(0.0);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(b.powf(e))) };
    }
}

fn fn_sqrt(execute_data: *mut ExecuteData, return_value: *mut Value, _eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let d = arg.to_double().unwrap_or(0.0);
    if !return_value.is_null() {
        unsafe { return_value.write(Value::double(d.sqrt())) };
    }
}

// ============================================================================
// Output functions
// ============================================================================

fn fn_var_dump(execute_data: *mut ExecuteData, _return_value: *mut Value, eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let output = var_dump_value(arg, 0);
    eg.write_output(output.as_bytes());
}

fn fn_print_r(execute_data: *mut ExecuteData, return_value: *mut Value, eg: &ExecutorGlobals) {
    let arg = unsafe { (*execute_data).cv(0) };
    let output = print_r_value(arg, 0);
    eg.write_output(output.as_bytes());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(true)) };
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn var_dump_value(val: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match val.value_type() {
        ValueType::Null => format!("{}NULL\n", prefix),
        ValueType::True => format!("{}bool(true)\n", prefix),
        ValueType::False => format!("{}bool(false)\n", prefix),
        ValueType::Long => format!("{}int({})\n", prefix, val.as_long().unwrap()),
        ValueType::Double => {
            let d = val.as_double().unwrap();
            if d == d.floor() && d.abs() < 1e15 {
                format!("{}float({})\n", prefix, d)
            } else {
                format!("{}float({})\n", prefix, d)
            }
        }
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
            for (key, v) in arr.entries() {
                let key_str = match key {
                    ArrayKey::Int(k) => format!("{}", k),
                    ArrayKey::String(k) => k.clone(),
                };
                out.push_str(&format!("{}[{}] => {}", inner, key_str, print_r_value(v, indent + 1)));
                if v.value_type() != ValueType::Array {
                    out.push('\n');
                }
            }
            out.push_str(&format!("{})\n", prefix));
            out
        }
        _ => String::new(),
    }
}

/// Compare two values numerically (returns -1, 0, 1 like <=>)
fn compare_values(a: &Value, b: &Value) -> i32 {
    let ad = a.to_double().unwrap_or(0.0);
    let bd = b.to_double().unwrap_or(0.0);
    if ad < bd { -1 } else if ad > bd { 1 } else { 0 }
}

/// Loose equality comparison for in_array
fn values_equal(a: &Value, b: &Value) -> bool {
    // Simple numeric comparison for now
    match (a.value_type(), b.value_type()) {
        (ValueType::Long, ValueType::Long) => a.as_long() == b.as_long(),
        (ValueType::String, ValueType::String) => a.as_str() == b.as_str(),
        (ValueType::Long, ValueType::Double) | (ValueType::Double, ValueType::Long) |
        (ValueType::Double, ValueType::Double) => {
            a.to_double() == b.to_double()
        }
        (ValueType::Null, ValueType::Null) => true,
        (ValueType::True, ValueType::True) | (ValueType::False, ValueType::False) => true,
        // String vs numeric: coerce string
        (ValueType::String, ValueType::Long) | (ValueType::Long, ValueType::String) => {
            let s_val = if a.value_type() == ValueType::String { a } else { b };
            let i_val = if a.value_type() == ValueType::Long { a } else { b };
            if let Ok(n) = s_val.as_str().unwrap().parse::<i64>() {
                n == i_val.as_long().unwrap()
            } else {
                false
            }
        }
        _ => false,
    }
}

// ============================================================================
// Constant functions
// ============================================================================

fn fn_define(execute_data: *mut ExecuteData, return_value: *mut Value, eg: &ExecutorGlobals) {
    let name_arg = unsafe { (*execute_data).cv(0) };
    let value_arg = unsafe { (*execute_data).cv(1) };
    // PHP requires name to be a string; coerce via string conversion
    let name = match name_arg.as_str() {
        Some(s) => s.to_string(),
        None => name_arg.echo_to_string(),
    };
    if name.is_empty() {
        if !return_value.is_null() {
            unsafe { return_value.write(Value::bool(false)) };
        }
        return;
    }
    let result = eg.define_constant(&name, value_arg.clone());
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(result.is_ok())) };
    }
}

fn fn_defined(execute_data: *mut ExecuteData, return_value: *mut Value, eg: &ExecutorGlobals) {
    let name_arg = unsafe { (*execute_data).cv(0) };
    let name = match name_arg.as_str() {
        Some(s) => s.to_string(),
        None => name_arg.echo_to_string(),
    };
    let exists = eg.find_constant(&name).is_some();
    if !return_value.is_null() {
        unsafe { return_value.write(Value::bool(exists)) };
    }
}

fn fn_constant(execute_data: *mut ExecuteData, return_value: *mut Value, eg: &ExecutorGlobals) {
    let name_arg = unsafe { (*execute_data).cv(0) };
    let name = match name_arg.as_str() {
        Some(s) => s.to_string(),
        None => name_arg.echo_to_string(),
    };
    let value = eg.find_constant(&name).unwrap_or(Value::null());
    if !return_value.is_null() {
        unsafe { return_value.write(value) };
    }
}

/// Parse leading integer from a string (PHP intval behavior)
fn parse_leading_int(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() { return 0; }
    let mut chars = s.chars().peekable();
    let mut negative = false;
    if chars.peek() == Some(&'-') {
        negative = true;
        chars.next();
    } else if chars.peek() == Some(&'+') {
        chars.next();
    }
    let mut n: i64 = 0;
    for c in chars {
        if c.is_ascii_digit() {
            n = n.wrapping_mul(10).wrapping_add((c as i64) - ('0' as i64));
        } else {
            break;
        }
    }
    if negative { -n } else { n }
}
