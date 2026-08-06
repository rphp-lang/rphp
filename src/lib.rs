#![allow(unsafe_op_in_unsafe_fn)]
#![allow(unused_imports)]

pub mod base64;
pub mod builtin_metadata;
pub mod compiler;
#[cfg(feature = "jit-prototype")]
pub mod jit;
pub mod lexer;
pub mod parser;
pub mod regex;
pub mod runtime;
#[allow(unused_unsafe)]
pub mod stdlib;
pub mod value;
pub mod vm;

/// Resolve a PHP built-in constant by name.
/// Single source of truth — used by both compiler (property defaults) and runtime (FetchConst).
pub fn builtin_constant(name: &str) -> Option<value::Value> {
    match name {
        // Boolean / null (case-insensitive names handled by parser, but belt-and-suspenders)
        "true" | "TRUE" => Some(value::Value::bool(true)),
        "false" | "FALSE" => Some(value::Value::bool(false)),
        "null" | "NULL" => Some(value::Value::null()),

        // Integer constants
        "PHP_INT_MAX" => Some(value::Value::long(i64::MAX)),
        "PHP_INT_MIN" => Some(value::Value::long(i64::MIN)),
        "PHP_INT_SIZE" => Some(value::Value::long(8)),

        // Float constants
        "PHP_FLOAT_MAX" => Some(value::Value::double(f64::MAX)),
        "PHP_FLOAT_MIN" => Some(value::Value::double(f64::MIN)),
        "PHP_FLOAT_EPSILON" => Some(value::Value::double(f64::EPSILON)),
        "PHP_FLOAT_DIG" => Some(value::Value::long(15)),
        "PHP_FLOAT_INF" => Some(value::Value::double(f64::INFINITY)),
        "PHP_FLOAT_NAN" => Some(value::Value::double(f64::NAN)),
        "INF" => Some(value::Value::double(f64::INFINITY)),
        "NAN" => Some(value::Value::double(f64::NAN)),

        // Version (rphp emulates PHP 8.4)
        "PHP_MAJOR_VERSION" => Some(value::Value::long(8)),
        "PHP_MINOR_VERSION" => Some(value::Value::long(4)),
        "PHP_RELEASE_VERSION" => Some(value::Value::long(0)),
        "PHP_VERSION_ID" => Some(value::Value::long(80400)),
        "PHP_VERSION" => Some(value::Value::string("8.4.0".to_string())),

        // System
        "PHP_EOL" => Some(value::Value::string("\n".to_string())),
        "PHP_MAXPATHLEN" => Some(value::Value::long(1024)),
        "DIRECTORY_SEPARATOR" | "PHP_OS" => {
            if cfg!(windows) {
                match name {
                    "DIRECTORY_SEPARATOR" => Some(value::Value::string("\\".to_string())),
                    "PHP_OS" => Some(value::Value::string("WINNT".to_string())),
                    _ => None,
                }
            } else {
                match name {
                    "DIRECTORY_SEPARATOR" => Some(value::Value::string("/".to_string())),
                    "PHP_OS" => Some(value::Value::string("Darwin".to_string())),
                    _ => None,
                }
            }
        }
        "PATH_SEPARATOR" => {
            if cfg!(windows) {
                Some(value::Value::string(";".to_string()))
            } else {
                Some(value::Value::string(":".to_string()))
            }
        }

        // Sorting
        "SORT_REGULAR" => Some(value::Value::long(0)),
        "SORT_NUMERIC" => Some(value::Value::long(1)),
        "SORT_STRING" => Some(value::Value::long(2)),
        "SORT_ASC" => Some(value::Value::long(4)),
        "SORT_DESC" => Some(value::Value::long(3)),

        // Array
        "ARRAY_FILTER_USE_BOTH" => Some(value::Value::long(1)),
        "ARRAY_FILTER_USE_KEY" => Some(value::Value::long(2)),

        // String
        "STR_PAD_RIGHT" => Some(value::Value::long(1)),
        "STR_PAD_LEFT" => Some(value::Value::long(0)),
        "STR_PAD_BOTH" => Some(value::Value::long(2)),

        _ => None,
    }
}
