//! Cold registration of built-in functions and classes.
//!
//! The handlers remain owned by their semantic modules. This module owns only
//! their deterministic request-startup registration order, signatures and
//! reference metadata.

use super::array_assoc_sets::*;
use super::array_traversal::*;
use super::directory::*;
use super::filesystem::*;
#[cfg(feature = "formatted-io")]
use super::formatted_io::*;
use super::hebrew::*;
use super::process::*;
use super::recursive_arrays::*;
use super::source_filters::*;
use super::strings::*;
use super::*;
use crate::vm::function::InternalFunctionDeprecation;

static LEGACY_UTF8_DEPRECATION: InternalFunctionDeprecation = InternalFunctionDeprecation {
    since: "8.2",
    message: "visit the php.net documentation for various alternatives",
};

static LIBXML_ENTITY_LOADER_DEPRECATION: InternalFunctionDeprecation =
    InternalFunctionDeprecation {
        since: "8.0",
        message: "as external entity loading is disabled by default",
    };

// ============================================================================
// Registration
// ============================================================================

/// Register all stdlib functions into the executor globals.
/// The returned Vec must live as long as the EG (owns the InternalFunction structs).
pub fn register_stdlib(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.reserve_stdlib_capacity();
    // The fixed PHP 8.5 surface currently owns fewer than 512 descriptors in
    // every feature configuration. Reserve that stable envelope up front so
    // cold registration does not repeatedly move the raw-pointer owners while
    // growing through the legacy 128- and 256-entry capacities.
    let mut funcs: Vec<Box<InternalFunction>> = Vec::with_capacity(512);

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

    macro_rules! reg_typed {
        (
            $name:expr,
            $handler:expr,
            $max_args:expr,
            $min_args:expr,
            [$($pname:expr),* $(,)?],
            [$($hint:expr),* $(,)?],
            $return_hint:expr
        ) => {{
            let mut function = Box::new(make_internal_function(
                $handler,
                $max_args,
                $min_args,
                pn![$($pname),*],
            ));
            function.common.sig.param_type_hints = vec![$($hint),*];
            function.common.sig.return_type_hint = $return_hint;
            function.handler_validates_types = true;
            let pointer = &function.common as *const FunctionCommon;
            eg.register_function($name, pointer).unwrap();
            funcs.push(function);
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

    macro_rules! reg_typed_ref {
        (
            $name:expr,
            $handler:expr,
            $max_args:expr,
            $min_args:expr,
            $ref_args:expr,
            [$($pname:expr),* $(,)?],
            [$($hint:expr),* $(,)?],
            $return_hint:expr
        ) => {{
            let mut function = Box::new(make_internal_function_ref(
                $handler,
                $max_args,
                $min_args,
                $ref_args,
                pn![$($pname),*],
            ));
            function.common.sig.param_type_hints = vec![$($hint),*];
            function.common.sig.return_type_hint = $return_hint;
            function.handler_validates_types = true;
            let pointer = &function.common as *const FunctionCommon;
            eg.register_function($name, pointer).unwrap();
            funcs.push(function);
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

    macro_rules! reg_var_raw {
        ($name:expr, $handler:expr, $raw_handler:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_variadic_raw(
                $handler,
                $raw_handler,
                $min_args,
                pn![$($pnames),*],
            ));
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

    macro_rules! reg_var_ref {
        ($name:expr, $handler:expr, $raw_handler:expr, $min_args:expr, $ref_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_variadic_ref(
                $handler,
                $raw_handler,
                $min_args,
                $ref_args,
                pn![$($pnames),*],
            ));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    #[cfg(feature = "formatted-io")]
    macro_rules! reg_var_ref_raw_all {
        ($name:expr, $handler:expr, $raw_handler:expr, $min_args:expr, $ref_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_function_variadic_ref_raw_all(
                $handler,
                $raw_handler,
                $min_args,
                $ref_args,
                pn![$($pnames),*],
            ));
            let ptr = &f.common as *const FunctionCommon;
            eg.register_function($name, ptr).unwrap();
            funcs.push(f);
        }};
    }

    // --- Array functions (by-ref: arg 0) ---
    reg!("count", fn_count, 2, 1, "value", "mode");
    reg!("sizeof", fn_sizeof, 2, 1, "value", "mode");
    reg_var_ref!(
        "array_push",
        fn_array_push,
        fn_array_push_raw_variadic,
        1,
        0b1,
        "array",
        "values"
    );
    reg_ref!("array_pop", fn_array_pop, 1, 1, 0b1, "array");
    reg_ref!("array_shift", fn_array_shift, 1, 1, 0b1, "array");
    reg_var_ref!(
        "array_unshift",
        fn_array_unshift,
        fn_array_unshift_raw_variadic,
        1,
        0b1,
        "array",
        "values"
    );
    reg!(
        "array_key_exists",
        fn_array_key_exists,
        2,
        2,
        "key",
        "array"
    );
    reg!("key_exists", fn_key_exists, 2, 2, "key", "array");
    reg!(
        "array_change_key_case",
        fn_array_change_key_case,
        2,
        1,
        "array",
        "case"
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
    reg!(
        "array_reverse",
        fn_array_reverse,
        2,
        1,
        "array",
        "preserve_keys"
    );
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
    for (name, handler) in [
        (
            "array_replace",
            fn_array_replace as crate::vm::function::InternalFunctionHandler,
        ),
        (
            "array_replace_recursive",
            fn_array_replace_recursive as crate::vm::function::InternalFunctionHandler,
        ),
    ] {
        let mut function = Box::new(make_internal_function_variadic(
            handler,
            1,
            pn!["array", "replacements"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::Array, ParamTypeHint::Array];
        function.common.sig.return_type_hint = ParamTypeHint::Array;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        eg.register_internal_function_extension(pointer, "standard");
        funcs.push(function);
    }
    reg!(
        "array_keys",
        fn_array_keys,
        3,
        1,
        "array",
        "filter_value",
        "strict"
    );
    reg!("array_values", fn_array_values, 1, 1, "array");
    reg!(
        "array_slice",
        fn_array_slice,
        4,
        2,
        "array",
        "offset",
        "length",
        "preserve_keys"
    );
    reg!("array_unique", fn_array_unique, 2, 1, "array", "flags");
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
    reg!(
        "array_chunk",
        fn_array_chunk,
        3,
        2,
        "array",
        "length",
        "preserve_keys"
    );
    reg!(
        "array_column",
        fn_array_column,
        3,
        2,
        "array",
        "column_key",
        "index_key"
    );
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
    reg!("range", fn_range, 3, 2, "start", "end", "step");
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
    reg!("array_rand", fn_array_rand, 2, 1, "array", "num");
    reg_ref!("shuffle", fn_shuffle, 1, 1, 0b1, "array");
    reg_var!("array_map", fn_array_map, 2, "callback", "array", "arrays");
    reg!(
        "array_filter",
        fn_array_filter,
        3,
        1,
        "array",
        "callback",
        "mode"
    );
    {
        let mut function = Box::new(make_internal_function(
            fn_iterator_to_array,
            2,
            1,
            pn!["iterator", "preserve_keys"],
        ));
        function.common.sig.param_type_hints = vec![
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName("Traversable".to_string()),
                ParamTypeHint::Array,
            ]),
            ParamTypeHint::Bool,
        ];
        function.common.sig.return_type_hint = ParamTypeHint::Array;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("iterator_to_array", pointer).unwrap();
        funcs.push(function);
    }
    reg_var!("compact", fn_compact, 1, "var_name", "var_names");

    // --- String functions ---
    reg!("strlen", fn_strlen, 1, 1, "string");
    reg!("strtok", fn_strtok, 2, 1, "string", "token");
    reg!("str_shuffle", fn_str_shuffle, 1, 1, "string");
    reg!("random_bytes", fn_random_bytes, 1, 1, "length");
    reg!("bin2hex", fn_bin2hex, 1, 1, "string");
    reg_typed!(
        "hex2bin",
        fn_hex2bin,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    {
        let mut function = Box::new(make_internal_function_variadic(
            fn_pack,
            1,
            pn!["format", "values"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::Mixed];
        function.common.sig.return_type_hint = ParamTypeHint::String;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("pack", pointer).unwrap();
        funcs.push(function);
    }
    {
        let mut function = Box::new(make_internal_function(
            fn_unpack,
            3,
            2,
            pn!["format", "string", "offset"],
        ));
        function.common.sig.param_type_hints = vec![
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int,
        ];
        function.common.sig.return_type_hint = ParamTypeHint::Union(vec![
            ParamTypeHint::Array,
            ParamTypeHint::ClassName("false".to_string()),
        ]);
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("unpack", pointer).unwrap();
        funcs.push(function);
    }
    reg_typed!(
        "md5",
        fn_md5,
        2,
        1,
        ["string", "binary"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::String
    );
    reg_typed!(
        "md5_file",
        fn_md5_file,
        2,
        1,
        ["filename", "binary"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    reg_typed!(
        "sha1",
        fn_sha1,
        2,
        1,
        ["string", "binary"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::String
    );
    reg_typed!(
        "sha1_file",
        fn_sha1_file,
        2,
        1,
        ["filename", "binary"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    reg_typed!(
        "crc32",
        fn_crc32,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::Int
    );
    // S3 exposes md5, xxh128 and crc32, including binary output. The wider
    // algorithm catalogue stays explicit compatibility work rather than
    // returning invented digests.
    reg_typed!(
        "hash",
        fn_hash,
        4,
        2,
        ["algo", "data", "binary", "options"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Bool,
            ParamTypeHint::Array,
        ],
        ParamTypeHint::String
    );
    let hash = eg.find_function("hash").expect("hash was just registered");
    eg.register_internal_function_reflection_metadata(
        hash,
        vec![
            None,
            None,
            Some(Value::bool(false)),
            Some(Value::array(PhpArray::new())),
        ],
        "hash",
    );
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
    reg_typed!(
        "substr",
        fn_substr,
        3,
        2,
        ["string", "offset", "length"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
        ],
        ParamTypeHint::String
    );
    const SUBSTR_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[None, None, Some("null")];
    let substr = eg
        .find_function("substr")
        .expect("substr was just registered");
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        substr,
        vec![None, None, Some(Value::null())],
        SUBSTR_DEFAULT_DIAGNOSTICS,
        "standard",
    );
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
        "strnatcasecmp",
        fn_strnatcasecmp,
        2,
        2,
        "string1",
        "string2"
    );
    reg_typed!(
        "substr_compare",
        fn_substr_compare,
        5,
        3,
        ["haystack", "needle", "offset", "length", "case_insensitive"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::Int
    );
    reg_typed!(
        "strpos",
        fn_strpos,
        3,
        2,
        ["haystack", "needle", "offset"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int
        ],
        ParamTypeHint::Union(vec![
            ParamTypeHint::Int,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    reg_typed!(
        "strstr",
        fn_strstr,
        3,
        2,
        ["haystack", "needle", "before_needle"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
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
    reg!("strrpos", fn_strrpos, 3, 2, "haystack", "needle", "offset");
    reg_typed!(
        "strrchr",
        fn_strrchr,
        3,
        2,
        ["haystack", "needle", "before_needle"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    {
        let mut function = Box::new(make_internal_function(
            fn_strtr,
            3,
            2,
            pn!["string", "from", "to"],
        ));
        function.common.sig.param_type_hints = vec![
            ParamTypeHint::String,
            ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]),
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
        ];
        function.common.sig.return_type_hint = ParamTypeHint::String;
        function.handler_validates_types = true;
        function.exact_arity_diagnostics = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("strtr", pointer).unwrap();
        funcs.push(function);
    }
    {
        let array_or_string =
            || ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]);
        let mut function = Box::new(make_internal_function_ref(
            fn_str_replace,
            4,
            3,
            0b1000,
            pn!["search", "replace", "subject", "count"],
        ));
        function.common.sig.param_type_hints = vec![
            array_or_string(),
            array_or_string(),
            array_or_string(),
            ParamTypeHint::None,
        ];
        function.common.sig.return_type_hint = array_or_string();
        function.handler_validates_types = true;
        function.common.plan.call = crate::vm::function::CallStrategy::Fast;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("str_replace", pointer).unwrap();
        funcs.push(function);
    }
    reg!("addcslashes", fn_addcslashes, 2, 2, "string", "characters");
    reg!("addslashes", fn_addslashes, 1, 1, "string");
    reg!("stripslashes", fn_stripslashes, 1, 1, "string");
    reg!("stripcslashes", fn_stripcslashes, 1, 1, "string");
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
    reg!("str_increment", fn_str_increment, 1, 1, "string");
    reg!("str_decrement", fn_str_decrement, 1, 1, "string");
    reg!("trim", fn_trim, 2, 1, "string", "characters");
    reg!("rtrim", fn_rtrim, 2, 1, "string", "characters");
    reg!("ltrim", fn_ltrim, 2, 1, "string", "characters");
    reg_typed!(
        "explode",
        fn_explode,
        3,
        2,
        ["separator", "string", "limit"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int
        ],
        ParamTypeHint::Array
    );
    reg_typed!(
        "implode",
        fn_implode,
        2,
        1,
        ["separator", "array"],
        [
            ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]),
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Array)),
        ],
        ParamTypeHint::String
    );
    reg_typed!(
        "join",
        fn_join,
        2,
        1,
        ["separator", "array"],
        [
            ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]),
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Array)),
        ],
        ParamTypeHint::String
    );
    reg!("str_repeat", fn_str_repeat, 2, 2, "string", "times");
    reg_typed!(
        "substr_count",
        fn_substr_count,
        4,
        2,
        ["haystack", "needle", "offset", "length"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
        ],
        ParamTypeHint::Int
    );
    reg_typed!(
        "strspn",
        fn_strspn,
        4,
        2,
        ["string", "characters", "offset", "length"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
        ],
        ParamTypeHint::Int
    );
    reg_typed!(
        "strcspn",
        fn_strcspn,
        4,
        2,
        ["string", "characters", "offset", "length"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
        ],
        ParamTypeHint::Int
    );
    reg_typed!(
        "strpbrk",
        fn_strpbrk,
        2,
        2,
        ["string", "characters"],
        [ParamTypeHint::String, ParamTypeHint::String],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
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
        4,
        2,
        "string",
        "length",
        "pad_string",
        "pad_type"
    );
    {
        let mut function = Box::new(make_internal_function(
            fn_str_split,
            2,
            1,
            pn!["string", "length"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::Int];
        function.common.sig.return_type_hint = ParamTypeHint::Array;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("str_split", pointer).unwrap();
        funcs.push(function);
    }
    reg!("ucfirst", fn_ucfirst, 1, 1, "string");
    reg!("lcfirst", fn_lcfirst, 1, 1, "string");
    reg!("ucwords", fn_ucwords, 2, 1, "string", "separators");
    reg_typed!(
        "count_chars",
        fn_count_chars,
        2,
        1,
        ["string", "mode"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String])
    );
    reg_typed!(
        "metaphone",
        fn_metaphone,
        2,
        1,
        ["string", "max_phonemes"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::String
    );
    reg_typed!(
        "quotemeta",
        fn_quotemeta,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg_typed!(
        "soundex",
        fn_soundex,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg_typed!(
        "str_rot13",
        fn_str_rot13,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    for (name, handler) in [
        (
            "utf8_encode",
            fn_utf8_encode as crate::vm::function::InternalFunctionHandler,
        ),
        (
            "utf8_decode",
            fn_utf8_decode as crate::vm::function::InternalFunctionHandler,
        ),
    ] {
        let mut function = Box::new(make_internal_function(handler, 1, 1, pn!["string"]));
        function.common.sig.param_type_hints = vec![ParamTypeHint::String];
        function.common.sig.return_type_hint = ParamTypeHint::String;
        function.handler_validates_types = true;
        function.set_deprecation(&LEGACY_UTF8_DEPRECATION);
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer).unwrap();
        funcs.push(function);
    }
    reg_typed!(
        "str_word_count",
        fn_str_word_count,
        3,
        1,
        ["string", "format", "characters"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))
        ],
        ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::Int])
    );
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
    reg_typed!(
        "wordwrap",
        fn_wordwrap,
        4,
        1,
        ["string", "width", "break", "cut_long_words"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::String,
            ParamTypeHint::Bool
        ],
        ParamTypeHint::String
    );
    {
        let mut function = Box::new(make_internal_function(
            fn_nl2br,
            2,
            1,
            pn!["string", "use_xhtml"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::Bool];
        function.common.sig.return_type_hint = ParamTypeHint::String;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("nl2br", pointer).unwrap();
        funcs.push(function);
    }
    reg!("strrev", fn_strrev, 1, 1, "string");
    reg_typed!(
        "hebrev",
        fn_hebrev,
        2,
        1,
        ["string", "max_chars_per_line"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::String
    );
    let hebrev = eg
        .find_function("hebrev")
        .expect("hebrev was just registered");
    eg.register_internal_function_reflection_metadata(
        hebrev,
        vec![None, Some(Value::long(0))],
        "standard",
    );
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
    reg_typed!(
        "ord",
        fn_ord,
        1,
        1,
        ["character"],
        [ParamTypeHint::String],
        ParamTypeHint::Int
    );
    reg!("chr", fn_chr, 1, 1, "codepoint");
    reg_var!("sprintf", fn_sprintf, 1, "format", "values");
    reg!("vsprintf", fn_vsprintf, 2, 2, "format", "values");
    reg_var!("printf", fn_printf, 1, "format", "values");
    reg!("vprintf", fn_vprintf, 2, 2, "format", "values");
    #[cfg(feature = "formatted-io")]
    {
        reg_var!("fprintf", fn_fprintf, 2, "stream", "format", "values");
        reg!("vfprintf", fn_vfprintf, 3, 3, "stream", "format", "values");
        reg_var_ref_raw_all!(
            "sscanf",
            fn_sscanf,
            fn_sscanf_raw_variadic,
            2,
            u64::MAX << 2,
            "string",
            "format",
            "vars"
        );
        reg_var_ref_raw_all!(
            "fscanf",
            fn_fscanf,
            fn_fscanf_raw_variadic,
            2,
            u64::MAX << 2,
            "stream",
            "format",
            "vars"
        );
    }

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
    reg_typed!(
        "intval",
        fn_intval,
        2,
        1,
        ["value", "base"],
        [ParamTypeHint::Mixed, ParamTypeHint::Int],
        ParamTypeHint::Int
    );
    let intval = eg
        .find_function("intval")
        .expect("intval was just registered");
    eg.register_internal_function_reflection_metadata(
        intval,
        vec![None, Some(Value::long(10))],
        "standard",
    );
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
    reg!(
        "get_defined_constants",
        fn_get_defined_constants,
        1,
        0,
        "categorize"
    );
    funcs
        .last_mut()
        .expect("get_defined_constants was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Bool];
    funcs
        .last_mut()
        .expect("get_defined_constants was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
    reg!("get_declared_classes", fn_get_declared_classes, 0, 0);
    reg!("get_declared_interfaces", fn_get_declared_interfaces, 0, 0);
    reg!("get_declared_traits", fn_get_declared_traits, 0, 0);
    reg_typed!(
        "class_exists",
        autoload::fn_class_exists,
        2,
        1,
        ["class", "autoload"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Bool
    );
    reg_typed!(
        "interface_exists",
        autoload::fn_interface_exists,
        2,
        1,
        ["interface", "autoload"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Bool
    );
    reg_typed!(
        "trait_exists",
        autoload::fn_trait_exists,
        2,
        1,
        ["trait", "autoload"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Bool
    );
    reg_typed!(
        "enum_exists",
        autoload::fn_enum_exists,
        2,
        1,
        ["enum", "autoload"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Bool
    );
    for name in [
        "class_exists",
        "interface_exists",
        "trait_exists",
        "enum_exists",
    ] {
        let function = eg
            .find_function(name)
            .expect("class-like existence predicate was just registered");
        eg.register_internal_function_reflection_metadata(
            function,
            vec![None, Some(Value::bool(true))],
            "Core",
        );
    }
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
    reg_var_ref!("max", fn_max, fn_max_raw_variadic, 1, 0, "value", "values");
    reg_var_ref!("min", fn_min, fn_min_raw_variadic, 1, 0, "value", "values");
    reg_direct!("floor", fn_floor, direct_floor, 1, 1, "num");
    reg!("ceil", fn_ceil, 1, 1, "num");
    reg_typed!(
        "round",
        fn_round,
        3,
        1,
        ["num", "precision", "mode"],
        [
            ParamTypeHint::Union(vec![ParamTypeHint::Int, ParamTypeHint::Float]),
            ParamTypeHint::Int,
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName("RoundingMode".to_string()),
                ParamTypeHint::Int,
            ]),
        ],
        ParamTypeHint::Float
    );
    const ROUND_DEFAULT_DIAGNOSTICS: &[Option<&str>] =
        &[None, Some("0"), Some("RoundingMode::HalfAwayFromZero")];
    let round = eg
        .find_function("round")
        .expect("round was just registered");
    let default_rounding_mode = rounding_mode_case_value(eg, "HalfAwayFromZero")
        .expect("RoundingMode default case is registered before stdlib functions");
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        round,
        vec![None, Some(Value::long(0)), Some(default_rounding_mode)],
        ROUND_DEFAULT_DIAGNOSTICS,
        "standard",
    );
    reg_typed!(
        "pow",
        fn_pow,
        2,
        2,
        ["num", "exponent"],
        [ParamTypeHint::Mixed, ParamTypeHint::Mixed],
        ParamTypeHint::Union(vec![
            ParamTypeHint::ClassName("object".to_string()),
            ParamTypeHint::Int,
            ParamTypeHint::Float,
        ])
    );
    let pow = eg.find_function("pow").expect("pow was just registered");
    eg.register_internal_function_reflection_metadata(pow, vec![None, None], "standard");
    reg_direct!("sqrt", fn_sqrt, direct_sqrt, 1, 1, "num");
    reg_typed!(
        "intdiv",
        fn_intdiv,
        2,
        2,
        ["num1", "num2"],
        [ParamTypeHint::Int, ParamTypeHint::Int],
        ParamTypeHint::Int
    );
    let intdiv = eg
        .find_function("intdiv")
        .expect("intdiv was just registered");
    eg.register_internal_function_reflection_metadata(intdiv, vec![None, None], "standard");
    reg_typed!(
        "fmod",
        fn_fmod,
        2,
        2,
        ["num1", "num2"],
        [ParamTypeHint::Float, ParamTypeHint::Float],
        ParamTypeHint::Float
    );
    let fmod = eg.find_function("fmod").expect("fmod was just registered");
    eg.register_internal_function_reflection_metadata(fmod, vec![None, None], "standard");
    reg!("fdiv", fn_fdiv, 2, 2, "num1", "num2");
    reg_typed!(
        "log",
        fn_log,
        2,
        1,
        ["num", "base"],
        [ParamTypeHint::Float, ParamTypeHint::Float],
        ParamTypeHint::Float
    );
    const LOG_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[None, Some("M_E")];
    let log = eg.find_function("log").expect("log was just registered");
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        log,
        vec![None, Some(Value::double(std::f64::consts::E))],
        LOG_DEFAULT_DIAGNOSTICS,
        "standard",
    );
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
    reg_typed!(
        "json_encode",
        fn_json_encode,
        3,
        1,
        ["value", "flags", "depth"],
        [ParamTypeHint::Mixed, ParamTypeHint::Int, ParamTypeHint::Int],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    let json_encode = eg
        .find_function("json_encode")
        .expect("json_encode was just registered");
    eg.register_internal_function_reflection_metadata(
        json_encode,
        vec![None, Some(Value::long(0)), Some(Value::long(512))],
        "json",
    );
    reg_typed!(
        "json_last_error",
        fn_json_last_error,
        0,
        0,
        [],
        [],
        ParamTypeHint::Int
    );
    let json_last_error = eg
        .find_function("json_last_error")
        .expect("json_last_error was just registered");
    eg.register_internal_function_reflection_metadata(json_last_error, vec![], "json");
    reg_typed!(
        "json_last_error_msg",
        fn_json_last_error_msg,
        0,
        0,
        [],
        [],
        ParamTypeHint::String
    );
    let json_last_error_msg = eg
        .find_function("json_last_error_msg")
        .expect("json_last_error_msg was just registered");
    eg.register_internal_function_reflection_metadata(json_last_error_msg, vec![], "json");
    reg_typed!(
        "json_decode",
        fn_json_decode,
        4,
        1,
        ["json", "associative", "depth", "flags"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Bool)),
            ParamTypeHint::Int,
            ParamTypeHint::Int,
        ],
        ParamTypeHint::Mixed
    );
    let json_decode = eg
        .find_function("json_decode")
        .expect("json_decode was just registered");
    eg.register_internal_function_reflection_metadata(
        json_decode,
        vec![
            None,
            Some(Value::null()),
            Some(Value::long(512)),
            Some(Value::long(0)),
        ],
        "json",
    );

    // --- Misc ---
    reg!("isset_func", fn_isset_func, 1, 1, "value");
    reg!("empty_func", fn_empty_func, 1, 1, "value");
    reg!("unset_func", fn_unset_func, 1, 1, "value");
    reg_typed!(
        "set_error_handler",
        fn_set_error_handler,
        2,
        1,
        ["callback", "error_levels"],
        [
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::Callable)),
            ParamTypeHint::Int
        ],
        ParamTypeHint::None
    );
    let set_error_handler = eg
        .find_function("set_error_handler")
        .expect("set_error_handler was just registered");
    eg.register_internal_function_reflection_metadata(
        set_error_handler,
        vec![None, Some(Value::long(crate::PHP_E_ALL))],
        "Core",
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
    {
        let mut function = Box::new(make_internal_function_variadic(
            fn_register_shutdown_function,
            1,
            pn!["callback", "args"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::Callable, ParamTypeHint::Mixed];
        function.common.sig.return_type_hint = ParamTypeHint::Void;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("register_shutdown_function", pointer)
            .unwrap();
        eg.register_internal_function_extension(pointer, "standard");
        funcs.push(function);
    }
    reg_typed!(
        "error_reporting",
        fn_error_reporting,
        1,
        0,
        ["error_level"],
        [ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int))],
        ParamTypeHint::Int
    );
    let error_reporting = eg
        .find_function("error_reporting")
        .expect("error_reporting was just registered");
    eg.register_internal_function_reflection_metadata(
        error_reporting,
        vec![Some(Value::null())],
        "Core",
    );
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
    reg!("flush", fn_flush, 0, 0);
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
    {
        let mut function = Box::new(make_internal_function_ref(
            fn_extract,
            3,
            1,
            0b1,
            pn!["array", "flags", "prefix"],
        ));
        // PHP exposes &$array through Reflection but accepts a non-lvalue
        // source when no writable storage exists (notably $GLOBALS and array
        // literals). EXTR_REFS still aliases an ordinary lvalue source.
        function.common.sig.prefer_ref_args = 0b1;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("extract", pointer).unwrap();
        funcs.push(function);
    }
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
    reg_var!("call_user_func", fn_call_user_func, 1, "callback", "args");
    reg!(
        "call_user_func_array",
        fn_call_user_func_array,
        2,
        2,
        "callback",
        "args"
    );
    reg_var!(
        "forward_static_call",
        fn_forward_static_call,
        1,
        "callback",
        "args"
    );
    reg!(
        "forward_static_call_array",
        fn_forward_static_call_array,
        2,
        2,
        "callback",
        "args"
    );
    reg_ref!(
        "is_callable",
        fn_is_callable,
        3,
        1,
        0b100,
        "value",
        "syntax_only",
        "callable_name"
    );
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
    reg!("stat", fn_stat, 1, 1, "filename");
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
    reg_typed!(
        "dirname",
        fn_dirname,
        2,
        1,
        ["path", "levels"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::String
    );
    reg_typed!(
        "basename",
        fn_basename,
        2,
        1,
        ["path", "suffix"],
        [ParamTypeHint::String, ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg!("realpath", fn_realpath, 1, 1, "path");
    reg_typed!(
        "pathinfo",
        fn_pathinfo,
        2,
        1,
        ["path", "flags"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String])
    );
    reg!("getcwd", fn_getcwd, 0, 0);
    reg!("chdir", fn_chdir, 1, 1, "directory");
    reg!("opendir", fn_opendir, 2, 1, "directory", "context");
    reg!("readdir", fn_readdir, 1, 0, "dir_handle");
    reg!("rewinddir", fn_rewinddir, 1, 0, "dir_handle");
    reg!("closedir", fn_closedir, 1, 0, "dir_handle");
    reg!(
        "scandir",
        fn_scandir,
        3,
        1,
        "directory",
        "sorting_order",
        "context"
    );
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
    reg_typed!(
        "get_meta_tags",
        meta_tags::fn_get_meta_tags,
        2,
        1,
        ["filename", "use_include_path"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![
            ParamTypeHint::Array,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    let get_meta_tags = eg
        .find_function("get_meta_tags")
        .expect("get_meta_tags was just registered");
    eg.register_internal_function_reflection_metadata(
        get_meta_tags,
        vec![None, Some(Value::bool(false))],
        "standard",
    );
    reg_typed!(
        "mkdir",
        fn_mkdir,
        4,
        1,
        ["directory", "permissions", "recursive", "context"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Bool,
            ParamTypeHint::None,
        ],
        ParamTypeHint::Bool
    );
    let mkdir = eg
        .find_function("mkdir")
        .expect("mkdir was just registered");
    eg.register_internal_function_reflection_metadata(
        mkdir,
        vec![
            None,
            Some(Value::long(0o777)),
            Some(Value::bool(false)),
            Some(Value::null()),
        ],
        "standard",
    );
    reg_typed!(
        "rmdir",
        fn_rmdir,
        2,
        1,
        ["directory", "context"],
        [ParamTypeHint::String, ParamTypeHint::None],
        ParamTypeHint::Bool
    );
    let rmdir = eg
        .find_function("rmdir")
        .expect("rmdir was just registered");
    eg.register_internal_function_reflection_metadata(
        rmdir,
        vec![None, Some(Value::null())],
        "standard",
    );
    reg_typed!(
        "unlink",
        fn_unlink,
        2,
        1,
        ["filename", "context"],
        [ParamTypeHint::String, ParamTypeHint::None],
        ParamTypeHint::Bool
    );
    let unlink = eg
        .find_function("unlink")
        .expect("unlink was just registered");
    eg.register_internal_function_reflection_metadata(
        unlink,
        vec![None, Some(Value::null())],
        "standard",
    );
    reg_typed!(
        "rename",
        fn_rename,
        3,
        2,
        ["from", "to", "context"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::None,
        ],
        ParamTypeHint::Bool
    );
    let rename = eg
        .find_function("rename")
        .expect("rename was just registered");
    eg.register_internal_function_reflection_metadata(
        rename,
        vec![None, None, Some(Value::null())],
        "standard",
    );
    reg_typed!(
        "copy",
        fn_copy,
        3,
        2,
        ["from", "to", "context"],
        [
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::None,
        ],
        ParamTypeHint::Bool
    );
    let copy = eg.find_function("copy").expect("copy was just registered");
    eg.register_internal_function_reflection_metadata(
        copy,
        vec![None, None, Some(Value::null())],
        "standard",
    );
    reg_typed!(
        "tempnam",
        fn_tempnam,
        2,
        2,
        ["directory", "prefix"],
        [ParamTypeHint::String, ParamTypeHint::String],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    let tempnam = eg
        .find_function("tempnam")
        .expect("tempnam was just registered");
    eg.register_internal_function_reflection_metadata(tempnam, vec![None, None], "standard");
    reg!("sys_get_temp_dir", fn_sys_get_temp_dir, 0, 0);
    reg_typed!(
        "glob",
        fn_glob,
        2,
        1,
        ["pattern", "flags"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::Union(vec![
            ParamTypeHint::Array,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    let glob = eg.find_function("glob").expect("glob was just registered");
    eg.register_internal_function_reflection_metadata(
        glob,
        vec![None, Some(Value::long(0))],
        "standard",
    );

    // --- URL / query ---
    reg!("parse_url", fn_parse_url, 2, 1, "url", "component");
    reg_typed_ref!(
        "parse_str",
        fn_parse_str,
        2,
        2,
        0b10,
        ["string", "result"],
        [ParamTypeHint::String],
        ParamTypeHint::Void
    );
    let parse_str = eg
        .find_function("parse_str")
        .expect("parse_str was just registered");
    eg.register_internal_function_extension(parse_str, "standard");
    reg_typed!(
        "http_build_query",
        fn_http_build_query,
        4,
        1,
        ["data", "numeric_prefix", "arg_separator", "encoding_type"],
        [
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName("object".to_string()),
                ParamTypeHint::Array,
            ]),
            ParamTypeHint::String,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            ParamTypeHint::Int,
        ],
        ParamTypeHint::String
    );
    const HTTP_BUILD_QUERY_DEFAULT_DIAGNOSTICS: &[Option<&str>] =
        &[None, None, None, Some("PHP_QUERY_RFC1738")];
    let http_build_query = eg
        .find_function("http_build_query")
        .expect("http_build_query was just registered");
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        http_build_query,
        vec![
            None,
            Some(Value::string("")),
            Some(Value::null()),
            Some(Value::long(1)),
        ],
        HTTP_BUILD_QUERY_DEFAULT_DIAGNOSTICS,
        "standard",
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
    reg_typed!(
        "preg_quote",
        fn_preg_quote,
        2,
        1,
        ["str", "delimiter"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
        ],
        ParamTypeHint::String
    );
    let preg_quote = eg
        .find_function("preg_quote")
        .expect("preg_quote was just registered");
    eg.register_internal_function_reflection_metadata(
        preg_quote,
        vec![None, Some(Value::null())],
        "pcre",
    );

    // --- String encoding ---
    reg_typed!(
        "htmlspecialchars",
        fn_htmlspecialchars,
        4,
        1,
        ["string", "flags", "encoding", "double_encode"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::String
    );
    reg_typed!(
        "htmlspecialchars_decode",
        fn_htmlspecialchars_decode,
        2,
        1,
        ["string", "flags"],
        [ParamTypeHint::String, ParamTypeHint::Int],
        ParamTypeHint::String
    );
    reg_typed!(
        "htmlentities",
        fn_htmlentities,
        4,
        1,
        ["string", "flags", "encoding", "double_encode"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::String
    );
    reg_typed!(
        "html_entity_decode",
        fn_html_entity_decode,
        3,
        1,
        ["string", "flags", "encoding"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
        ],
        ParamTypeHint::String
    );
    reg_typed!(
        "get_html_translation_table",
        fn_get_html_translation_table,
        3,
        0,
        ["table", "flags", "encoding"],
        [
            ParamTypeHint::Int,
            ParamTypeHint::Int,
            ParamTypeHint::String
        ],
        ParamTypeHint::Array
    );
    const HTML_ENCODER_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[
        None,
        Some("ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401"),
        Some("null"),
        Some("true"),
    ];
    const HTML_DECODE_DEFAULT_DIAGNOSTICS: &[Option<&str>] =
        &[None, Some("ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401")];
    const HTML_ENTITY_DECODE_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[
        None,
        Some("ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401"),
        Some("null"),
    ];
    const HTML_TABLE_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[
        Some("HTML_SPECIALCHARS"),
        Some("ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401"),
        Some("\"UTF-8\""),
    ];
    for (function_name, defaults, diagnostics) in [
        (
            "htmlspecialchars",
            vec![
                None,
                Some(Value::long(11)),
                Some(Value::null()),
                Some(Value::bool(true)),
            ],
            HTML_ENCODER_DEFAULT_DIAGNOSTICS,
        ),
        (
            "htmlentities",
            vec![
                None,
                Some(Value::long(11)),
                Some(Value::null()),
                Some(Value::bool(true)),
            ],
            HTML_ENCODER_DEFAULT_DIAGNOSTICS,
        ),
        (
            "htmlspecialchars_decode",
            vec![None, Some(Value::long(11))],
            HTML_DECODE_DEFAULT_DIAGNOSTICS,
        ),
        (
            "html_entity_decode",
            vec![None, Some(Value::long(11)), Some(Value::null())],
            HTML_ENTITY_DECODE_DEFAULT_DIAGNOSTICS,
        ),
        (
            "get_html_translation_table",
            vec![
                Some(Value::long(0)),
                Some(Value::long(11)),
                Some(Value::string("UTF-8")),
            ],
            HTML_TABLE_DEFAULT_DIAGNOSTICS,
        ),
    ] {
        let function = eg
            .find_function(function_name)
            .expect("HTML string function was just registered");
        eg.register_internal_function_reflection_metadata_with_diagnostics(
            function,
            defaults,
            diagnostics,
            "standard",
        );
    }
    reg_typed!(
        "strip_tags",
        fn_strip_tags,
        2,
        1,
        ["string", "allowed_tags"],
        [
            ParamTypeHint::String,
            ParamTypeHint::Union(vec![
                ParamTypeHint::Array,
                ParamTypeHint::String,
                ParamTypeHint::ClassName("null".to_string()),
            ]),
        ],
        ParamTypeHint::String
    );
    reg_typed!(
        "highlight_string",
        fn_highlight_string,
        2,
        1,
        ["string", "return"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("true".to_string()),
        ])
    );
    reg_typed!(
        "highlight_file",
        fn_highlight_file,
        2,
        1,
        ["filename", "return"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Bool])
    );
    reg_typed!(
        "show_source",
        fn_show_source,
        2,
        1,
        ["filename", "return"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Bool])
    );
    reg_typed!(
        "php_strip_whitespace",
        fn_php_strip_whitespace,
        1,
        1,
        ["filename"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    for (function_name, default_value) in [
        ("highlight_string", Value::bool(false)),
        ("highlight_file", Value::bool(false)),
        ("show_source", Value::bool(false)),
        ("strip_tags", Value::null()),
    ] {
        let function = eg
            .find_function(function_name)
            .expect("source-filter function was just registered");
        eg.register_internal_function_reflection_metadata(
            function,
            vec![None, Some(default_value)],
            "standard",
        );
    }
    let strip_whitespace = eg
        .find_function("php_strip_whitespace")
        .expect("php_strip_whitespace was just registered");
    eg.register_internal_function_extension(strip_whitespace, "standard");
    reg!("urlencode", fn_urlencode, 1, 1, "string");
    reg!("urldecode", fn_urldecode, 1, 1, "string");
    reg!("rawurlencode", fn_rawurlencode, 1, 1, "string");
    reg!("rawurldecode", fn_rawurldecode, 1, 1, "string");
    reg_typed!(
        "base64_encode",
        fn_base64_encode,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    let base64_encode = eg
        .find_function("base64_encode")
        .expect("base64_encode was just registered");
    eg.register_internal_function_reflection_metadata(base64_encode, vec![None], "standard");
    reg_typed!(
        "base64_decode",
        fn_base64_decode,
        2,
        1,
        ["string", "strict"],
        [ParamTypeHint::String, ParamTypeHint::Bool],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    reg_typed!(
        "quoted_printable_encode",
        fn_quoted_printable_encode,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg_typed!(
        "quoted_printable_decode",
        fn_quoted_printable_decode,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg_typed!(
        "convert_uuencode",
        fn_convert_uuencode,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::String
    );
    reg_typed!(
        "convert_uudecode",
        fn_convert_uudecode,
        1,
        1,
        ["string"],
        [ParamTypeHint::String],
        ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    reg_typed!(
        "filter_var",
        fn_filter_var,
        3,
        1,
        ["value", "filter", "options"],
        [
            ParamTypeHint::Mixed,
            ParamTypeHint::Int,
            ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::Int]),
        ],
        ParamTypeHint::Mixed
    );
    const FILTER_VAR_DEFAULT_DIAGNOSTICS: &[Option<&str>] = &[None, Some("FILTER_DEFAULT"), None];
    let filter_var = eg
        .find_function("filter_var")
        .expect("filter_var was just registered");
    eg.register_internal_function_reflection_metadata_with_diagnostics(
        filter_var,
        vec![None, Some(Value::long(516)), Some(Value::long(0))],
        FILTER_VAR_DEFAULT_DIAGNOSTICS,
        "filter",
    );

    // --- Case-insensitive string functions ---
    reg!("stripos", fn_stripos, 3, 2, "haystack", "needle", "offset");
    reg!(
        "strripos",
        fn_strripos,
        3,
        2,
        "haystack",
        "needle",
        "offset"
    );
    {
        let array_or_string =
            || ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]);
        let mut function = Box::new(make_internal_function_ref(
            fn_str_ireplace,
            4,
            3,
            0b1000,
            pn!["search", "replace", "subject", "count"],
        ));
        function.common.sig.param_type_hints = vec![
            array_or_string(),
            array_or_string(),
            array_or_string(),
            ParamTypeHint::None,
        ];
        function.common.sig.return_type_hint = array_or_string();
        function.handler_validates_types = true;
        function.common.plan.call = crate::vm::function::CallStrategy::Fast;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("str_ireplace", pointer).unwrap();
        funcs.push(function);
    }
    {
        let array_or_string =
            || ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::String]);
        let mut function = Box::new(make_internal_function(
            fn_substr_replace,
            4,
            3,
            pn!["string", "replace", "offset", "length"],
        ));
        function.common.sig.param_type_hints = vec![
            array_or_string(),
            array_or_string(),
            ParamTypeHint::Union(vec![ParamTypeHint::Array, ParamTypeHint::Int]),
            ParamTypeHint::Union(vec![
                ParamTypeHint::Array,
                ParamTypeHint::Int,
                ParamTypeHint::ClassName("null".to_string()),
            ]),
        ];
        function.common.sig.return_type_hint = array_or_string();
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("substr_replace", pointer).unwrap();
        funcs.push(function);
    }
    {
        let mut function = Box::new(make_internal_function(
            fn_str_getcsv,
            4,
            1,
            pn!["string", "separator", "enclosure", "escape"],
        ));
        function.common.sig.param_type_hints = vec![
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::String,
            ParamTypeHint::String,
        ];
        function.common.sig.return_type_hint = ParamTypeHint::Array;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("str_getcsv", pointer).unwrap();
        funcs.push(function);
    }
    {
        let mut function = Box::new(make_internal_function(
            fn_chunk_split,
            3,
            1,
            pn!["string", "length", "separator"],
        ));
        function.common.sig.param_type_hints = vec![
            ParamTypeHint::String,
            ParamTypeHint::Int,
            ParamTypeHint::String,
        ];
        function.common.sig.return_type_hint = ParamTypeHint::String;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("chunk_split", pointer).unwrap();
        funcs.push(function);
    }

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
    reg_var_raw!(
        "array_diff",
        fn_array_diff_variadic,
        fn_array_diff_raw_variadic,
        1,
        "array",
        "arrays"
    );
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
    reg_var!(
        "array_diff_key",
        fn_array_diff_key_variadic,
        1,
        "array",
        "arrays"
    );
    reg_var!("array_udiff", fn_array_udiff, 1, "array", "rest");
    reg_var!(
        "array_udiff_assoc",
        fn_array_udiff_assoc,
        1,
        "array",
        "rest"
    );
    reg_var!(
        "array_udiff_uassoc",
        fn_array_udiff_uassoc,
        1,
        "array",
        "rest"
    );
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
    reg_var!("array_uintersect", fn_array_uintersect, 1, "array", "rest");
    reg_var!(
        "array_uintersect_assoc",
        fn_array_uintersect_assoc,
        1,
        "array",
        "rest"
    );
    reg_var!(
        "array_uintersect_uassoc",
        fn_array_uintersect_uassoc,
        1,
        "array",
        "rest"
    );
    reg_var!(
        "array_intersect_key",
        fn_array_intersect_key_variadic,
        1,
        "array",
        "arrays"
    );
    reg_var_raw!(
        "array_intersect",
        fn_array_intersect_variadic,
        fn_array_intersect_raw_variadic,
        1,
        "array",
        "arrays"
    );
    reg_ref!(
        "array_walk",
        fn_array_walk,
        3,
        2,
        0b1,
        "array",
        "callback",
        "arg"
    );
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
    reg_ref!("natsort", fn_natsort, 1, 1, 0b1, "array");
    reg_ref!("natcasesort", fn_natcasesort, 1, 1, 0b1, "array");
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
    reg_typed!(
        "decbin",
        fn_decbin,
        1,
        1,
        ["num"],
        [ParamTypeHint::Int],
        ParamTypeHint::String
    );
    reg_typed!(
        "dechex",
        fn_dechex,
        1,
        1,
        ["num"],
        [ParamTypeHint::Int],
        ParamTypeHint::String
    );

    // --- Environment / system ---
    reg_typed!(
        "getenv",
        fn_getenv,
        2,
        0,
        ["name", "local_only"],
        [
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            ParamTypeHint::Bool,
        ],
        ParamTypeHint::Union(vec![
            ParamTypeHint::Array,
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ])
    );
    let getenv = eg
        .find_function("getenv")
        .expect("getenv was just registered");
    eg.register_internal_function_reflection_metadata(
        getenv,
        vec![Some(Value::null()), Some(Value::bool(false))],
        "standard",
    );
    reg!("putenv", fn_putenv, 1, 1, "assignment");
    reg!("php_uname", fn_php_uname, 1, 0, "mode");
    reg!("php_sapi_name", fn_php_sapi_name, 0, 0);
    reg!("zend_version", fn_zend_version, 0, 0);
    funcs
        .last_mut()
        .expect("zend_version was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::String;
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
    {
        let mut function = Box::new(make_internal_function_variadic(
            fn_setlocale,
            2,
            pn!["category", "locales", "rest"],
        ));
        function.common.sig.param_type_hints =
            vec![ParamTypeHint::Int, ParamTypeHint::None, ParamTypeHint::None];
        function.common.sig.return_type_hint = ParamTypeHint::Union(vec![
            ParamTypeHint::String,
            ParamTypeHint::ClassName("false".to_string()),
        ]);
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("setlocale", pointer).unwrap();
        funcs.push(function);
    }
    reg!("extension_loaded", fn_extension_loaded, 1, 1, "extension");
    {
        let mut function = Box::new(make_internal_function_ref(
            fn_headers_sent,
            2,
            0,
            0b11,
            pn!["filename", "line"],
        ));
        function.common.sig.return_type_hint = ParamTypeHint::Bool;
        // Keep the ordinary internal ABI for the common zero-argument call.
        // SendRef/SendVal already enforce the two optional by-reference slots,
        // as they do for the existing str_replace() ref-output registration.
        function.common.plan.call = crate::vm::function::CallStrategy::Fast;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("headers_sent", pointer).unwrap();
        eg.register_internal_function_reflection_metadata(
            pointer,
            vec![Some(Value::null()), Some(Value::null())],
            "standard",
        );
        funcs.push(function);
    }
    {
        let mut function = Box::new(make_internal_function(
            fn_libxml_disable_entity_loader,
            1,
            0,
            pn!["disable"],
        ));
        function.common.sig.param_type_hints = vec![ParamTypeHint::Bool];
        function.common.sig.return_type_hint = ParamTypeHint::Bool;
        function.handler_validates_types = true;
        function.set_deprecation(&LIBXML_ENTITY_LOADER_DEPRECATION);
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function("libxml_disable_entity_loader", pointer)
            .unwrap();
        eg.register_internal_function_reflection_metadata(
            pointer,
            vec![Some(Value::bool(true))],
            "libxml",
        );
        funcs.push(function);
    }
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
    reg!("gc_status", fn_gc_status, 0, 0);
    funcs
        .last_mut()
        .expect("gc_status was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
    reg!("gc_enabled", fn_gc_enabled, 0, 0);
    reg!("gc_enable", fn_gc_enable, 0, 0);
    reg!("gc_disable", fn_gc_disable, 0, 0);
    reg_typed!(
        "set_time_limit",
        fn_set_time_limit,
        1,
        1,
        ["seconds"],
        [ParamTypeHint::Int],
        ParamTypeHint::Bool
    );
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
