#![allow(unsafe_op_in_unsafe_fn)]
#![allow(unused_imports)]

pub mod base64;
pub mod builtin_metadata;
mod class_names;
pub mod compiler;
pub mod generics;
#[cfg(feature = "jit-prototype")]
pub mod jit;
pub mod lexer;
pub mod parser;
pub mod path_decomposition;
pub mod quoted_printable;
pub mod regex;
#[cfg(feature = "resource-lifetime")]
mod resource_handle;
pub mod runtime;
#[allow(unused_unsafe)]
pub mod stdlib;
pub mod string_byte_utilities;
pub mod uuencode;
pub mod value;
pub mod vm;

/// Public compatibility identity used by PHP constants, phpversion(), and
/// dependency platform checks. Newer experimental syntax does not change this
/// contract until its full version gate is promoted.
pub const PHP_COMPAT_VERSION: &str = "8.5.0";
pub const PHP_COMPAT_VERSION_ID: i64 = 80_500;
pub const PHP_COMPAT_MAJOR_VERSION: i64 = 8;
pub const PHP_COMPAT_MINOR_VERSION: i64 = 5;
pub const PHP_COMPAT_RELEASE_VERSION: i64 = 0;

fn php_os_family() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Darwin"
    } else if cfg!(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )) {
        "BSD"
    } else if cfg!(target_os = "solaris") {
        "Solaris"
    } else if cfg!(any(target_os = "linux", target_os = "android")) {
        "Linux"
    } else {
        "Unknown"
    }
}

fn php_os_name() -> &'static str {
    if cfg!(windows) {
        "WINNT"
    } else if cfg!(target_os = "freebsd") {
        "FreeBSD"
    } else if cfg!(target_os = "openbsd") {
        "OpenBSD"
    } else if cfg!(target_os = "netbsd") {
        "NetBSD"
    } else if cfg!(target_os = "dragonfly") {
        "DragonFlyBSD"
    } else if cfg!(target_os = "solaris") {
        "SunOS"
    } else {
        php_os_family()
    }
}

/// Resolve a PHP built-in constant by name.
/// Single source of truth — used by both compiler (property defaults) and runtime (FetchConst).
pub(crate) const BUILTIN_CONSTANT_NAMES: &[&str] = &[
    "PHP_INT_MAX",
    "PHP_INT_MIN",
    "PHP_INT_SIZE",
    "PHP_FLOAT_MAX",
    "PHP_FLOAT_MIN",
    "PHP_FLOAT_EPSILON",
    "PHP_FLOAT_DIG",
    "PHP_FLOAT_INF",
    "PHP_FLOAT_NAN",
    "INF",
    "NAN",
    "M_PI",
    "PHP_MAJOR_VERSION",
    "PHP_MINOR_VERSION",
    "PHP_RELEASE_VERSION",
    "PHP_VERSION_ID",
    "PHP_VERSION",
    "PHP_SAPI",
    "PHP_DEBUG",
    "PHP_OS_FAMILY",
    "ASSERT_ACTIVE",
    "ASSERT_CALLBACK",
    "ASSERT_BAIL",
    "ASSERT_WARNING",
    "ASSERT_EXCEPTION",
    "PHP_EOL",
    "PHP_MAXPATHLEN",
    "DIRECTORY_SEPARATOR",
    "PHP_OS",
    "PATH_SEPARATOR",
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_ALL",
    "SORT_REGULAR",
    "SORT_NUMERIC",
    "SORT_STRING",
    "SORT_LOCALE_STRING",
    "SORT_NATURAL",
    "SORT_FLAG_CASE",
    "SORT_ASC",
    "SORT_DESC",
    "SCANDIR_SORT_ASCENDING",
    "SCANDIR_SORT_DESCENDING",
    "SCANDIR_SORT_NONE",
    "COUNT_NORMAL",
    "COUNT_RECURSIVE",
    "ARRAY_FILTER_USE_BOTH",
    "ARRAY_FILTER_USE_KEY",
    "STR_PAD_RIGHT",
    "STR_PAD_LEFT",
    "STR_PAD_BOTH",
    "HTML_SPECIALCHARS",
    "HTML_ENTITIES",
    "ENT_NOQUOTES",
    "ENT_COMPAT",
    "ENT_QUOTES",
    "ENT_HTML401",
    "ENT_XML1",
    "ENT_XHTML",
    "ENT_HTML5",
    "ENT_IGNORE",
    "ENT_SUBSTITUTE",
    "ENT_DISALLOWED",
    "CASE_LOWER",
    "CASE_UPPER",
    "E_ERROR",
    "E_WARNING",
    "E_PARSE",
    "E_NOTICE",
    "E_CORE_ERROR",
    "E_CORE_WARNING",
    "E_COMPILE_ERROR",
    "E_COMPILE_WARNING",
    "E_USER_ERROR",
    "E_USER_WARNING",
    "E_USER_NOTICE",
    "E_STRICT",
    "E_RECOVERABLE_ERROR",
    "E_DEPRECATED",
    "E_USER_DEPRECATED",
    "E_ALL",
    "DEBUG_BACKTRACE_PROVIDE_OBJECT",
    "DEBUG_BACKTRACE_IGNORE_ARGS",
    "PATHINFO_DIRNAME",
    "PATHINFO_BASENAME",
    "PATHINFO_EXTENSION",
    "PATHINFO_FILENAME",
    "PATHINFO_ALL",
    "INI_SCANNER_NORMAL",
    "INI_SCANNER_RAW",
    "INI_SCANNER_TYPED",
    "EXTR_OVERWRITE",
    "EXTR_SKIP",
    "EXTR_PREFIX_SAME",
    "EXTR_PREFIX_ALL",
    "EXTR_PREFIX_INVALID",
    "EXTR_PREFIX_IF_EXISTS",
    "EXTR_IF_EXISTS",
    "EXTR_REFS",
    "PHP_QUERY_RFC3986",
    "PHP_URL_SCHEME",
    "PHP_URL_HOST",
    "PHP_URL_PORT",
    "PHP_URL_USER",
    "PHP_URL_PASS",
    "PHP_URL_PATH",
    "PHP_URL_QUERY",
    "PHP_URL_FRAGMENT",
    "PREG_SET_ORDER",
    "PREG_OFFSET_CAPTURE",
    "PREG_SPLIT_NO_EMPTY",
    "PREG_SPLIT_DELIM_CAPTURE",
    "PREG_SPLIT_OFFSET_CAPTURE",
    "LOCK_SH",
    "LOCK_EX",
    "LOCK_UN",
    "LOCK_NB",
    "JSON_ERROR_NONE",
    "JSON_BIGINT_AS_STRING",
    "JSON_NUMERIC_CHECK",
    "JSON_UNESCAPED_SLASHES",
    "JSON_UNESCAPED_UNICODE",
    "JSON_PRESERVE_ZERO_FRACTION",
    "JSON_THROW_ON_ERROR",
    "FILTER_VALIDATE_INT",
    "FILTER_VALIDATE_BOOL",
    "FILTER_VALIDATE_BOOLEAN",
    "FILTER_VALIDATE_FLOAT",
    "FILTER_VALIDATE_IP",
    "FILTER_DEFAULT",
    "FILTER_CALLBACK",
    "FILTER_FLAG_IPV4",
    "FILTER_FLAG_IPV6",
    "FILTER_REQUIRE_ARRAY",
    "FILTER_REQUIRE_SCALAR",
    "FILTER_FORCE_ARRAY",
    "FILTER_NULL_ON_FAILURE",
    "PHP_SESSION_ACTIVE",
    "PHP_OUTPUT_HANDLER_START",
    "PHP_OUTPUT_HANDLER_CLEAN",
    "PHP_OUTPUT_HANDLER_FLUSH",
    "PHP_OUTPUT_HANDLER_FINAL",
    "PHP_OUTPUT_HANDLER_END",
    "PHP_OUTPUT_HANDLER_CLEANABLE",
    "PHP_OUTPUT_HANDLER_FLUSHABLE",
    "PHP_OUTPUT_HANDLER_REMOVABLE",
    "UPLOAD_ERR_OK",
    "UPLOAD_ERR_INI_SIZE",
    "UPLOAD_ERR_FORM_SIZE",
    "UPLOAD_ERR_PARTIAL",
    "UPLOAD_ERR_NO_FILE",
    "UPLOAD_ERR_NO_TMP_DIR",
    "UPLOAD_ERR_CANT_WRITE",
    "UPLOAD_ERR_EXTENSION",
    "DATE_RFC2822",
    "TOKEN_PARSE",
    "T_LNUMBER",
    "T_DNUMBER",
    "T_STRING",
    "T_NAME_FULLY_QUALIFIED",
    "T_NAME_RELATIVE",
    "T_NAME_QUALIFIED",
    "T_VARIABLE",
    "T_INLINE_HTML",
    "T_ENCAPSED_AND_WHITESPACE",
    "T_CONSTANT_ENCAPSED_STRING",
    "T_NEW",
    "T_CLASS",
    "T_NAMESPACE",
    "T_DOUBLE_COLON",
    "T_PAAMAYIM_NEKUDOTAYIM",
    "T_COMMENT",
    "T_DOC_COMMENT",
    "T_OPEN_TAG",
    "T_OPEN_TAG_WITH_ECHO",
    "T_CLOSE_TAG",
    "T_WHITESPACE",
    "T_START_HEREDOC",
    "T_END_HEREDOC",
    "T_NS_SEPARATOR",
    "SEEK_SET",
    "SEEK_CUR",
    "SEEK_END",
    "FILE_USE_INCLUDE_PATH",
    "FILE_APPEND",
    "FILE_IGNORE_NEW_LINES",
    "FILE_SKIP_EMPTY_LINES",
];

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
        "M_PI" => Some(value::Value::double(std::f64::consts::PI)),

        // Public PHP compatibility contract.
        "PHP_MAJOR_VERSION" => Some(value::Value::long(PHP_COMPAT_MAJOR_VERSION)),
        "PHP_MINOR_VERSION" => Some(value::Value::long(PHP_COMPAT_MINOR_VERSION)),
        "PHP_RELEASE_VERSION" => Some(value::Value::long(PHP_COMPAT_RELEASE_VERSION)),
        "PHP_VERSION_ID" => Some(value::Value::long(PHP_COMPAT_VERSION_ID)),
        "PHP_VERSION" => Some(value::Value::string(PHP_COMPAT_VERSION)),
        "PHP_SAPI" => Some(value::Value::string("cli")),
        "PHP_DEBUG" => Some(value::Value::bool(false)),
        "PHP_OS_FAMILY" => Some(value::Value::string(php_os_family())),
        "ASSERT_ACTIVE" => Some(value::Value::long(1)),
        "ASSERT_CALLBACK" => Some(value::Value::long(2)),
        "ASSERT_BAIL" => Some(value::Value::long(3)),
        "ASSERT_WARNING" => Some(value::Value::long(4)),
        "ASSERT_EXCEPTION" => Some(value::Value::long(5)),

        // System
        "PHP_EOL" => Some(value::Value::string("\n".to_string())),
        "PHP_MAXPATHLEN" => Some(value::Value::long(1024)),
        "DIRECTORY_SEPARATOR" | "PHP_OS" => {
            if cfg!(windows) {
                match name {
                    "DIRECTORY_SEPARATOR" => Some(value::Value::string("\\".to_string())),
                    "PHP_OS" => Some(value::Value::string(php_os_name())),
                    _ => None,
                }
            } else {
                match name {
                    "DIRECTORY_SEPARATOR" => Some(value::Value::string("/".to_string())),
                    "PHP_OS" => Some(value::Value::string(php_os_name())),
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

        // Locale categories use the POSIX values exposed by PHP on Unix.
        // The current locale implementation admits only the portable C/POSIX locale.
        "LC_CTYPE" => Some(value::Value::long(0)),
        "LC_NUMERIC" => Some(value::Value::long(1)),
        "LC_TIME" => Some(value::Value::long(2)),
        "LC_COLLATE" => Some(value::Value::long(3)),
        "LC_MONETARY" => Some(value::Value::long(4)),
        "LC_MESSAGES" => Some(value::Value::long(5)),
        "LC_ALL" => Some(value::Value::long(6)),

        // Sorting
        "SORT_REGULAR" => Some(value::Value::long(0)),
        "SORT_NUMERIC" => Some(value::Value::long(1)),
        "SORT_STRING" => Some(value::Value::long(2)),
        "SORT_LOCALE_STRING" => Some(value::Value::long(5)),
        "SORT_NATURAL" => Some(value::Value::long(6)),
        "SORT_FLAG_CASE" => Some(value::Value::long(8)),
        "SORT_ASC" => Some(value::Value::long(4)),
        "SORT_DESC" => Some(value::Value::long(3)),

        // Directory
        "SCANDIR_SORT_ASCENDING" => Some(value::Value::long(0)),
        "SCANDIR_SORT_DESCENDING" => Some(value::Value::long(1)),
        "SCANDIR_SORT_NONE" => Some(value::Value::long(2)),

        // Array
        "COUNT_NORMAL" => Some(value::Value::long(0)),
        "COUNT_RECURSIVE" => Some(value::Value::long(1)),
        "ARRAY_FILTER_USE_BOTH" => Some(value::Value::long(1)),
        "ARRAY_FILTER_USE_KEY" => Some(value::Value::long(2)),

        // String
        "STR_PAD_RIGHT" => Some(value::Value::long(1)),
        "STR_PAD_LEFT" => Some(value::Value::long(0)),
        "STR_PAD_BOTH" => Some(value::Value::long(2)),
        "HTML_SPECIALCHARS" => Some(value::Value::long(0)),
        "HTML_ENTITIES" => Some(value::Value::long(1)),

        // Common extension constants used by portable framework code.
        "ENT_NOQUOTES" => Some(value::Value::long(0)),
        "ENT_COMPAT" => Some(value::Value::long(2)),
        "ENT_QUOTES" => Some(value::Value::long(3)),
        "ENT_HTML401" => Some(value::Value::long(0)),
        "ENT_XML1" => Some(value::Value::long(16)),
        "ENT_XHTML" => Some(value::Value::long(32)),
        "ENT_HTML5" => Some(value::Value::long(48)),
        "ENT_IGNORE" => Some(value::Value::long(4)),
        "ENT_SUBSTITUTE" => Some(value::Value::long(8)),
        "ENT_DISALLOWED" => Some(value::Value::long(128)),
        "CASE_LOWER" => Some(value::Value::long(0)),
        "CASE_UPPER" => Some(value::Value::long(1)),
        "E_ERROR" => Some(value::Value::long(1)),
        "E_WARNING" => Some(value::Value::long(2)),
        "E_PARSE" => Some(value::Value::long(4)),
        "E_NOTICE" => Some(value::Value::long(8)),
        "E_CORE_ERROR" => Some(value::Value::long(16)),
        "E_CORE_WARNING" => Some(value::Value::long(32)),
        "E_COMPILE_ERROR" => Some(value::Value::long(64)),
        "E_COMPILE_WARNING" => Some(value::Value::long(128)),
        "E_USER_ERROR" => Some(value::Value::long(256)),
        "E_USER_WARNING" => Some(value::Value::long(512)),
        "E_USER_NOTICE" => Some(value::Value::long(1024)),
        "E_STRICT" => Some(value::Value::long(2048)),
        "E_RECOVERABLE_ERROR" => Some(value::Value::long(4096)),
        "E_DEPRECATED" => Some(value::Value::long(8192)),
        "E_USER_DEPRECATED" => Some(value::Value::long(16_384)),
        "E_ALL" => Some(value::Value::long(32_767)),
        "DEBUG_BACKTRACE_PROVIDE_OBJECT" => Some(value::Value::long(1)),
        "DEBUG_BACKTRACE_IGNORE_ARGS" => Some(value::Value::long(2)),
        "PATHINFO_DIRNAME" => Some(value::Value::long(1)),
        "PATHINFO_BASENAME" => Some(value::Value::long(2)),
        "PATHINFO_EXTENSION" => Some(value::Value::long(4)),
        "PATHINFO_FILENAME" => Some(value::Value::long(8)),
        "PATHINFO_ALL" => Some(value::Value::long(15)),
        "INI_SCANNER_NORMAL" => Some(value::Value::long(0)),
        "INI_SCANNER_RAW" => Some(value::Value::long(1)),
        "INI_SCANNER_TYPED" => Some(value::Value::long(2)),
        "EXTR_OVERWRITE" => Some(value::Value::long(0)),
        "EXTR_SKIP" => Some(value::Value::long(1)),
        "EXTR_PREFIX_SAME" => Some(value::Value::long(2)),
        "EXTR_PREFIX_ALL" => Some(value::Value::long(3)),
        "EXTR_PREFIX_INVALID" => Some(value::Value::long(4)),
        "EXTR_PREFIX_IF_EXISTS" => Some(value::Value::long(5)),
        "EXTR_IF_EXISTS" => Some(value::Value::long(6)),
        "EXTR_REFS" => Some(value::Value::long(256)),
        "PHP_QUERY_RFC3986" => Some(value::Value::long(2)),
        "PHP_URL_SCHEME" => Some(value::Value::long(0)),
        "PHP_URL_HOST" => Some(value::Value::long(1)),
        "PHP_URL_PORT" => Some(value::Value::long(2)),
        "PHP_URL_USER" => Some(value::Value::long(3)),
        "PHP_URL_PASS" => Some(value::Value::long(4)),
        "PHP_URL_PATH" => Some(value::Value::long(5)),
        "PHP_URL_QUERY" => Some(value::Value::long(6)),
        "PHP_URL_FRAGMENT" => Some(value::Value::long(7)),
        "PREG_SET_ORDER" => Some(value::Value::long(2)),
        "PREG_OFFSET_CAPTURE" => Some(value::Value::long(256)),
        "PREG_SPLIT_NO_EMPTY" => Some(value::Value::long(1)),
        "PREG_SPLIT_DELIM_CAPTURE" => Some(value::Value::long(2)),
        "PREG_SPLIT_OFFSET_CAPTURE" => Some(value::Value::long(4)),
        "LOCK_SH" => Some(value::Value::long(1)),
        "LOCK_EX" => Some(value::Value::long(2)),
        "LOCK_UN" => Some(value::Value::long(3)),
        "LOCK_NB" => Some(value::Value::long(4)),
        "JSON_ERROR_NONE" => Some(value::Value::long(0)),
        "JSON_BIGINT_AS_STRING" => Some(value::Value::long(2)),
        "JSON_NUMERIC_CHECK" => Some(value::Value::long(32)),
        "JSON_UNESCAPED_SLASHES" => Some(value::Value::long(64)),
        "JSON_UNESCAPED_UNICODE" => Some(value::Value::long(256)),
        "JSON_PRESERVE_ZERO_FRACTION" => Some(value::Value::long(1024)),
        "JSON_THROW_ON_ERROR" => Some(value::Value::long(4_194_304)),
        "FILTER_VALIDATE_INT" => Some(value::Value::long(257)),
        "FILTER_VALIDATE_BOOL" => Some(value::Value::long(258)),
        "FILTER_VALIDATE_BOOLEAN" => Some(value::Value::long(258)),
        "FILTER_VALIDATE_FLOAT" => Some(value::Value::long(259)),
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
        "PHP_OUTPUT_HANDLER_START" => Some(value::Value::long(1)),
        "PHP_OUTPUT_HANDLER_CLEAN" => Some(value::Value::long(2)),
        "PHP_OUTPUT_HANDLER_FLUSH" => Some(value::Value::long(4)),
        "PHP_OUTPUT_HANDLER_FINAL" => Some(value::Value::long(8)),
        "PHP_OUTPUT_HANDLER_END" => Some(value::Value::long(8)),
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

        // Tokenizer IDs follow PHP 8.2. Their stable numeric identity matters
        // to source scanners that compare token_get_all() output directly.
        "TOKEN_PARSE" => Some(value::Value::long(1)),
        "T_LNUMBER" => Some(value::Value::long(260)),
        "T_DNUMBER" => Some(value::Value::long(261)),
        "T_STRING" => Some(value::Value::long(262)),
        "T_NAME_FULLY_QUALIFIED" => Some(value::Value::long(263)),
        "T_NAME_RELATIVE" => Some(value::Value::long(264)),
        "T_NAME_QUALIFIED" => Some(value::Value::long(265)),
        "T_VARIABLE" => Some(value::Value::long(266)),
        "T_INLINE_HTML" => Some(value::Value::long(267)),
        "T_ENCAPSED_AND_WHITESPACE" => Some(value::Value::long(268)),
        "T_CONSTANT_ENCAPSED_STRING" => Some(value::Value::long(269)),
        "T_NEW" => Some(value::Value::long(284)),
        "T_CLASS" => Some(value::Value::long(336)),
        "T_NAMESPACE" => Some(value::Value::long(342)),
        "T_DOUBLE_COLON" | "T_PAAMAYIM_NEKUDOTAYIM" => Some(value::Value::long(402)),
        "T_COMMENT" => Some(value::Value::long(392)),
        "T_DOC_COMMENT" => Some(value::Value::long(393)),
        "T_OPEN_TAG" => Some(value::Value::long(394)),
        "T_OPEN_TAG_WITH_ECHO" => Some(value::Value::long(395)),
        "T_CLOSE_TAG" => Some(value::Value::long(396)),
        "T_WHITESPACE" => Some(value::Value::long(397)),
        "T_START_HEREDOC" => Some(value::Value::long(398)),
        "T_END_HEREDOC" => Some(value::Value::long(399)),
        "T_NS_SEPARATOR" => Some(value::Value::long(403)),

        // Streams
        "SEEK_SET" => Some(value::Value::long(0)),
        "SEEK_CUR" => Some(value::Value::long(1)),
        "SEEK_END" => Some(value::Value::long(2)),
        #[cfg(any(feature = "file-write", feature = "file-lines"))]
        "FILE_USE_INCLUDE_PATH" => Some(value::Value::long(1)),
        "FILE_APPEND" => Some(value::Value::long(8)),
        #[cfg(feature = "file-lines")]
        "FILE_IGNORE_NEW_LINES" => Some(value::Value::long(2)),
        #[cfg(feature = "file-lines")]
        "FILE_SKIP_EMPTY_LINES" => Some(value::Value::long(4)),

        _ => None,
    }
}

/// Resolve constants exposed by built-in classes before the runtime class
/// registry exists. Constant expressions in declarations (notably attribute
/// flags) use the same values that stdlib publishes at request startup.
pub fn builtin_class_constant(class: &str, constant: &str) -> Option<value::Value> {
    if !class.eq_ignore_ascii_case("Attribute") {
        return None;
    }
    let value = match constant {
        "TARGET_CLASS" => 1,
        "TARGET_FUNCTION" => 2,
        "TARGET_METHOD" => 4,
        "TARGET_PROPERTY" => 8,
        "TARGET_CLASS_CONSTANT" => 16,
        "TARGET_PARAMETER" => 32,
        "TARGET_CONSTANT" => 64,
        "TARGET_ALL" => 127,
        "IS_REPEATABLE" => 128,
        _ => return None,
    };
    Some(value::Value::long(value))
}
