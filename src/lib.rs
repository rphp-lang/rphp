#![allow(unsafe_op_in_unsafe_fn)]
#![allow(unused_imports)]

pub mod base64;
pub mod builtin_metadata;
pub mod compiler;
pub mod generics;
#[cfg(feature = "jit-prototype")]
pub mod jit;
pub mod lexer;
pub mod parser;
pub mod regex;
#[cfg(feature = "resource-lifetime")]
mod resource_handle;
pub mod runtime;
#[allow(unused_unsafe)]
pub mod stdlib;
pub mod value;
pub mod vm;

/// Public compatibility identity used by PHP constants, phpversion(), and
/// dependency platform checks. Newer experimental syntax does not change this
/// contract until its full version gate is promoted.
pub const PHP_COMPAT_VERSION: &str = "8.2.0";
pub const PHP_COMPAT_VERSION_ID: i64 = 80_200;
pub const PHP_COMPAT_MAJOR_VERSION: i64 = 8;
pub const PHP_COMPAT_MINOR_VERSION: i64 = 2;
pub const PHP_COMPAT_RELEASE_VERSION: i64 = 0;

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

        // Public PHP compatibility contract.
        "PHP_MAJOR_VERSION" => Some(value::Value::long(PHP_COMPAT_MAJOR_VERSION)),
        "PHP_MINOR_VERSION" => Some(value::Value::long(PHP_COMPAT_MINOR_VERSION)),
        "PHP_RELEASE_VERSION" => Some(value::Value::long(PHP_COMPAT_RELEASE_VERSION)),
        "PHP_VERSION_ID" => Some(value::Value::long(PHP_COMPAT_VERSION_ID)),
        "PHP_VERSION" => Some(value::Value::string(PHP_COMPAT_VERSION)),
        "PHP_SAPI" => Some(value::Value::string("cli")),

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

        // Common extension constants used by portable framework code.
        "ENT_QUOTES" => Some(value::Value::long(3)),
        "ENT_SUBSTITUTE" => Some(value::Value::long(8)),
        "CASE_LOWER" => Some(value::Value::long(0)),
        "E_WARNING" => Some(value::Value::long(2)),
        "PATHINFO_EXTENSION" => Some(value::Value::long(4)),
        "PHP_QUERY_RFC3986" => Some(value::Value::long(2)),
        "PHP_URL_PATH" => Some(value::Value::long(5)),
        "PREG_SET_ORDER" => Some(value::Value::long(2)),
        "PREG_SPLIT_NO_EMPTY" => Some(value::Value::long(1)),
        "JSON_ERROR_NONE" => Some(value::Value::long(0)),
        "JSON_BIGINT_AS_STRING" => Some(value::Value::long(2)),
        "JSON_NUMERIC_CHECK" => Some(value::Value::long(32)),
        "JSON_THROW_ON_ERROR" => Some(value::Value::long(4_194_304)),
        "FILTER_VALIDATE_INT" => Some(value::Value::long(257)),
        "FILTER_VALIDATE_BOOL" => Some(value::Value::long(258)),
        "FILTER_VALIDATE_BOOLEAN" => Some(value::Value::long(258)),
        "FILTER_VALIDATE_IP" => Some(value::Value::long(275)),
        "FILTER_DEFAULT" => Some(value::Value::long(516)),
        "FILTER_CALLBACK" => Some(value::Value::long(1024)),
        "FILTER_FLAG_IPV4" => Some(value::Value::long(1_048_576)),
        "FILTER_FLAG_IPV6" => Some(value::Value::long(2_097_152)),
        "FILTER_REQUIRE_ARRAY" => Some(value::Value::long(16_777_216)),
        "FILTER_REQUIRE_SCALAR" => Some(value::Value::long(33_554_432)),
        "FILTER_FORCE_ARRAY" => Some(value::Value::long(67_108_864)),
        "FILTER_NULL_ON_FAILURE" => Some(value::Value::long(134_217_728)),
        "PHP_SESSION_ACTIVE" => Some(value::Value::long(2)),
        "PHP_OUTPUT_HANDLER_CLEANABLE" => Some(value::Value::long(16)),
        "PHP_OUTPUT_HANDLER_FLUSHABLE" => Some(value::Value::long(32)),
        "PHP_OUTPUT_HANDLER_REMOVABLE" => Some(value::Value::long(64)),
        "UPLOAD_ERR_OK" => Some(value::Value::long(0)),
        "UPLOAD_ERR_INI_SIZE" => Some(value::Value::long(1)),
        "UPLOAD_ERR_FORM_SIZE" => Some(value::Value::long(2)),
        "UPLOAD_ERR_PARTIAL" => Some(value::Value::long(3)),
        "UPLOAD_ERR_NO_FILE" => Some(value::Value::long(4)),
        "UPLOAD_ERR_NO_TMP_DIR" => Some(value::Value::long(6)),
        "UPLOAD_ERR_CANT_WRITE" => Some(value::Value::long(7)),
        "UPLOAD_ERR_EXTENSION" => Some(value::Value::long(8)),
        "DATE_RFC2822" => Some(value::Value::string("D, d M Y H:i:s O")),

        // Streams
        "SEEK_SET" => Some(value::Value::long(0)),
        "SEEK_CUR" => Some(value::Value::long(1)),
        "SEEK_END" => Some(value::Value::long(2)),
        #[cfg(any(feature = "file-write", feature = "file-lines"))]
        "FILE_USE_INCLUDE_PATH" => Some(value::Value::long(1)),
        #[cfg(feature = "file-write")]
        "LOCK_EX" => Some(value::Value::long(2)),
        #[cfg(feature = "file-write")]
        "FILE_APPEND" => Some(value::Value::long(8)),
        #[cfg(feature = "file-lines")]
        "FILE_IGNORE_NEW_LINES" => Some(value::Value::long(2)),
        #[cfg(feature = "file-lines")]
        "FILE_SKIP_EMPTY_LINES" => Some(value::Value::long(4)),

        _ => None,
    }
}
