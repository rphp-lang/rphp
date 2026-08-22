//! Cold registration of built-in functions and classes.
//!
//! The handlers remain owned by their semantic modules. This module owns only
//! their deterministic request-startup registration order, signatures and
//! reference metadata.

use super::array_assoc_sets::*;
use super::array_traversal::*;
use super::filesystem::*;
use super::process::*;
use super::recursive_arrays::*;
use super::source_filters::*;
use super::strings::*;
use super::*;

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

    macro_rules! reg_var_prefer_ref {
        ($name:expr, $handler:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_variadic_prefer_ref(
                $handler,
                $min_args,
                pn![$($pnames),*],
            ));
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
    reg!("array_find", fn_array_find, 2, 2, "array", "callback");
    reg!(
        "array_find_key",
        fn_array_find_key,
        2,
        2,
        "array",
        "callback"
    );
    reg!("array_any", fn_array_any, 2, 2, "array", "callback");
    reg!("array_all", fn_array_all, 2, 2, "array", "callback");
    reg!("array_first", fn_array_first, 1, 1, "array");
    reg!("array_last", fn_array_last, 1, 1, "array");
    reg_var!("array_merge", fn_array_merge, 0, "arrays");
    reg_var!(
        "array_merge_recursive",
        fn_array_merge_recursive,
        0,
        "arrays"
    );
    reg_var!("array_replace", fn_array_replace, 1, "array");
    reg_var!(
        "array_replace_recursive",
        fn_array_replace_recursive,
        1,
        "array"
    );
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
    reg!("array_fill_keys", fn_array_fill_keys, 2, 2, "keys", "value");
    reg!("array_pad", fn_array_pad, 3, 3, "array", "length", "value");
    reg!("array_chunk", fn_array_chunk, 2, 2, "array", "length");
    reg!("array_column", fn_array_column, 2, 2, "array", "column_key");
    reg_ref!("sort", fn_sort, 2, 1, 0b1, "array", "flags");
    reg_ref!("rsort", fn_rsort, 2, 1, 0b1, "array", "flags");
    reg_var_prefer_ref!("array_multisort", fn_array_multisort, 1, "array", "rest");
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
    reg_var!("array_map", fn_array_map, 2, "callback", "array");
    reg!("array_filter", fn_array_filter, 2, 1, "array", "callback");
    reg!(
        "iterator_to_array",
        fn_iterator_to_array,
        2,
        1,
        "iterator",
        "preserve_keys"
    );
    // compact() requires caller scope access (not yet implemented) — intentionally not registered

    // --- String functions ---
    reg!("strlen", fn_strlen, 1, 1, "string");
    reg!("strtok", fn_strtok, 2, 1, "string", "token");
    reg!("str_shuffle", fn_str_shuffle, 1, 1, "string");
    reg!("random_bytes", fn_random_bytes, 1, 1, "length");
    reg!("bin2hex", fn_bin2hex, 1, 1, "string");
    reg!("hex2bin", fn_hex2bin, 1, 1, "string");
    reg!("md5", fn_md5, 2, 1, "string", "binary");
    // S3 exposes md5, xxh128 and crc32, including binary output. The wider
    // algorithm catalogue stays explicit compatibility work rather than
    // returning invented digests.
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
    reg!("strcmp", fn_strcmp, 2, 2, "string1", "string2");
    reg!("strncmp", fn_strncmp, 3, 3, "string1", "string2", "length");
    reg!("strcasecmp", fn_strcasecmp, 2, 2, "string1", "string2");
    reg!(
        "strncasecmp",
        fn_strncasecmp,
        3,
        3,
        "string1",
        "string2",
        "length"
    );
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
    reg!(
        "strstr",
        fn_strstr,
        3,
        2,
        "haystack",
        "needle",
        "before_needle"
    );
    reg!(
        "stristr",
        fn_stristr,
        3,
        2,
        "haystack",
        "needle",
        "before_needle"
    );
    reg!("strrpos", fn_strrpos, 2, 2, "haystack", "needle");
    reg!("strrchr", fn_strrchr, 2, 2, "haystack", "needle");
    reg!("strtr", fn_strtr, 3, 2, "string", "from", "to");
    reg_ref!(
        "str_replace",
        fn_str_replace,
        4,
        3,
        0b1000,
        "search",
        "replace",
        "subject",
        "count"
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
    reg!("implode", fn_implode, 2, 1, "separator", "array");
    reg!("join", fn_join, 2, 1, "separator", "array");
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
    reg_ref!(
        "similar_text",
        fn_similar_text,
        3,
        2,
        0b100,
        "string1",
        "string2",
        "percent"
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
    reg!("strrev", fn_strrev, 1, 1, "string");
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
    reg!("ord", fn_ord, 1, 1, "character");
    reg!("chr", fn_chr, 1, 1, "codepoint");
    reg_var!("sprintf", fn_sprintf, 1, "format");
    reg!("vsprintf", fn_vsprintf, 2, 2, "format", "values");
    reg_var!("printf", fn_printf, 1, "format");
    reg!("vprintf", fn_vprintf, 2, 2, "format", "values");

    // --- Unix process helpers ---
    reg!("escapeshellarg", fn_escapeshellarg, 1, 1, "arg");
    reg!("escapeshellcmd", fn_escapeshellcmd, 1, 1, "command");
    reg_ref!(
        "exec",
        fn_exec,
        3,
        1,
        0b110,
        "command",
        "output",
        "result_code"
    );
    reg!("shell_exec", fn_shell_exec, 1, 1, "command");

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
    reg!("get_called_class", fn_get_called_class, 0, 0);
    reg!(
        "get_class_methods",
        fn_get_class_methods,
        1,
        1,
        "object_or_class"
    );
    reg!("get_class_vars", fn_get_class_vars, 1, 1, "class");
    reg!("get_object_vars", fn_get_object_vars, 1, 1, "object");
    reg!(
        "get_mangled_object_vars",
        fn_get_mangled_object_vars,
        1,
        1,
        "object"
    );
    reg!(
        "get_parent_class",
        fn_get_parent_class,
        1,
        0,
        "object_or_class"
    );
    reg!("get_included_files", fn_get_included_files, 0, 0);
    reg!("get_required_files", fn_get_included_files, 0, 0);
    reg!(
        "get_defined_functions",
        fn_get_defined_functions,
        1,
        0,
        "exclude_disabled"
    );
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
        "property_exists",
        fn_property_exists,
        2,
        2,
        "object_or_class",
        "property"
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
    reg!("fdiv", fn_fdiv, 2, 2, "num1", "num2");
    reg!("log", fn_log, 1, 1, "num");
    reg!("log10", fn_log10, 1, 1, "num");
    reg!("log2", fn_log2, 1, 1, "num");
    reg!("pi", fn_pi, 0, 0);
    reg!("is_nan", fn_is_nan, 1, 1, "num");
    reg!("is_finite", fn_is_finite, 1, 1, "num");
    reg!("is_infinite", fn_is_infinite, 1, 1, "num");
    reg!("rand", fn_rand, 2, 0, "min", "max");
    reg!("mt_rand", fn_rand, 2, 0, "min", "max");
    reg!("random_int", fn_random_int, 2, 2, "min", "max");

    // --- Output ---
    reg_var!("var_dump", fn_var_dump, 1, "value");
    reg_var!("debug_zval_dump", fn_debug_zval_dump, 1, "value");
    reg!("print_r", fn_print_r, 2, 1, "value", "return");
    reg!("var_export", fn_var_export, 2, 1, "value", "return");
    reg!("spl_object_hash", fn_spl_object_hash, 1, 1, "object");
    reg!("spl_object_id", fn_spl_object_id, 1, 1, "object");

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
    reg!("error_get_last", fn_error_get_last, 0, 0);
    reg!("error_clear_last", fn_error_clear_last, 0, 0);
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
    reg!(
        "ob_start",
        fn_ob_start,
        3,
        0,
        "callback",
        "chunk_size",
        "flags"
    );
    reg!("ob_get_level", fn_ob_get_level, 0, 0);
    reg!("ob_get_contents", fn_ob_get_contents, 0, 0);
    reg!("ob_get_length", fn_ob_get_length, 0, 0);
    reg!("ob_get_clean", fn_ob_get_clean, 0, 0);
    reg!("ob_get_flush", fn_ob_get_flush, 0, 0);
    reg!("ob_clean", fn_ob_clean, 0, 0);
    reg!("ob_flush", fn_ob_flush, 0, 0);
    reg!("ob_end_clean", fn_ob_end_clean, 0, 0);
    reg!("ob_end_flush", fn_ob_end_flush, 0, 0);
    reg!("gc_mem_caches", fn_gc_mem_caches, 0, 0);
    reg!("func_num_args", fn_func_num_args, 0, 0);
    reg!("func_get_arg", fn_func_get_arg, 1, 1, "position");
    reg!("func_get_args", fn_func_get_args, 0, 0);
    reg_ref!("extract", fn_extract, 3, 1, 0b1, "array", "flags", "prefix");
    reg!("get_defined_vars", fn_get_defined_vars, 0, 0);
    reg!(
        "debug_backtrace",
        fn_debug_backtrace,
        2,
        0,
        "options",
        "limit"
    );
    reg!(
        "debug_print_backtrace",
        fn_debug_print_backtrace,
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
    reg!("assert", fn_assert, 2, 1, "assertion", "description");
    reg!("assert_options", fn_assert_options, 2, 1, "what", "value");

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
        3,
        2,
        "filename",
        "data",
        "flags"
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
    reg!("strip_tags", fn_strip_tags, 2, 1, "string", "allowed_tags");
    reg!(
        "highlight_string",
        fn_highlight_string,
        2,
        1,
        "string",
        "return"
    );
    reg!(
        "highlight_file",
        fn_highlight_file,
        2,
        1,
        "filename",
        "return"
    );
    reg!("show_source", fn_show_source, 2, 1, "filename", "return");
    reg!(
        "php_strip_whitespace",
        fn_php_strip_whitespace,
        1,
        1,
        "filename"
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
    reg_var!("array_diff", fn_array_diff, 1, "array", "arrays");
    reg_var!(
        "array_diff_assoc",
        fn_array_diff_assoc,
        1,
        "array",
        "arrays"
    );
    reg_var!(
        "array_diff_uassoc",
        fn_array_diff_uassoc,
        1,
        "array",
        "rest"
    );
    reg_var!("array_diff_ukey", fn_array_diff_ukey, 1, "array", "rest");
    reg_var!("array_diff_key", fn_array_diff_key, 2, "array", "arrays");
    reg_var!(
        "array_intersect_assoc",
        fn_array_intersect_assoc,
        1,
        "array",
        "arrays"
    );
    reg_var!(
        "array_intersect_uassoc",
        fn_array_intersect_uassoc,
        1,
        "array",
        "rest"
    );
    reg_var!(
        "array_intersect_ukey",
        fn_array_intersect_ukey,
        1,
        "array",
        "rest"
    );
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
    reg!(
        "base_convert",
        fn_base_convert,
        3,
        3,
        "num",
        "from_base",
        "to_base"
    );

    // --- Environment / system ---
    reg!("getenv", fn_getenv, 1, 1, "name");
    reg!("putenv", fn_putenv, 1, 1, "assignment");
    reg!("php_uname", fn_php_uname, 1, 0, "mode");
    reg!("php_sapi_name", fn_php_sapi_name, 0, 0);
    reg!("phpversion", fn_phpversion, 1, 0, "extension");
    reg!(
        "version_compare",
        fn_version_compare,
        3,
        2,
        "version1",
        "version2",
        "operator"
    );
    reg_var!("setlocale", fn_setlocale, 2, "category", "locales");
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
    reg!("ini_set", fn_ini_set, 2, 2, "option", "value");
    reg!(
        "ini_parse_quantity",
        parse_ini::fn_ini_parse_quantity,
        1,
        1,
        "shorthand"
    );
    reg!(
        "parse_ini_string",
        parse_ini::fn_parse_ini_string,
        3,
        1,
        "ini_string",
        "process_sections",
        "scanner_mode"
    );
    reg!(
        "parse_ini_file",
        parse_ini::fn_parse_ini_file,
        3,
        1,
        "filename",
        "process_sections",
        "scanner_mode"
    );
    reg!("gc_collect_cycles", fn_gc_collect_cycles, 0, 0);
    reg!("gc_enabled", fn_gc_enabled, 0, 0);
    reg!("gc_enable", fn_gc_enable, 0, 0);
    reg!("gc_disable", fn_gc_disable, 0, 0);
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn returned_descriptors_own_every_published_internal_function() {
        let mut eg = ExecutorGlobals::new();
        let functions = register_stdlib(&mut eg);
        let owned: HashSet<*const FunctionCommon> = functions
            .iter()
            .map(|function| &function.common as *const FunctionCommon)
            .collect();

        assert_eq!(owned.len(), functions.len());
        assert!(
            eg.function_table
                .values()
                .all(|function| owned.contains(function)),
            "every raw function-table pointer must remain owned by the returned descriptors",
        );
        assert!(eg.class_is_internal("Throwable"));
        assert!(eg.class_is_internal("ReflectionClass"));
    }
}
