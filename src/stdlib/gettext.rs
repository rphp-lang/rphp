//! PHP 8.5 gettext extension over the process-native GNU locale catalog.
//!
//! PHP's gettext binding and `setlocale()` share process-global libc state.
//! Keeping that boundary in one cold module preserves the ordinary VM/value
//! layouts while still allowing catalogs, plural rules, domain directories,
//! and locale changes to observe the same native state.

use std::ffi::OsStr;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use super::native_process::{NativeCall, NativeInput, invoke_native};

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

const GETTEXT_MAX_DOMAIN_LENGTH: usize = 1024;
const GETTEXT_MAX_MESSAGE_LENGTH: usize = 4096;
const LC_ALL: i64 = 6;

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn write_bytes(return_value: *mut Value, bytes: Vec<u8>) {
    super::write_return_value(return_value, super::php_byte_result(bytes, false));
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn write_native_string_or_false(return_value: *mut Value, result: Option<Vec<u8>>) {
    match result {
        Some(bytes) => write_bytes(return_value, bytes),
        None => super::write_return_value(return_value, Value::bool(false)),
    }
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn value_error(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        message.as_ref(),
    ));
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn required_string(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Value>, VmError> {
    super::typed_internal_string_value_argument_expected(
        ed, eg, function, index, parameter, "string",
    )
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn optional_nullable_string(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Option<Value>>, VmError> {
    let supplied = super::owned_argument(ed, index);
    if matches!(
        supplied.dereferenced().value_type(),
        ValueType::Undef | ValueType::Null
    ) {
        return Ok(Some(None));
    }
    Ok(required_string(ed, eg, function, index, parameter)?.map(Some))
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn string_bytes(value: &Value) -> Vec<u8> {
    value.php_string_bytes().unwrap_or_default().into_owned()
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn validate_max_length(
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    parameter: &str,
    bytes: &[u8],
    maximum: usize,
) -> bool {
    if bytes.len() <= maximum {
        return true;
    }
    value_error(
        eg,
        format!("{function}(): Argument #{position} (${parameter}) is too long"),
    );
    false
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn validate_domain(
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    bytes: &[u8],
) -> bool {
    if !validate_max_length(
        eg,
        function,
        position,
        "domain",
        bytes,
        GETTEXT_MAX_DOMAIN_LENGTH,
    ) {
        return false;
    }
    if bytes.is_empty() {
        value_error(
            eg,
            format!("{function}(): Argument #{position} ($domain) must not be empty"),
        );
        return false;
    }
    true
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn validate_message(
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    parameter: &str,
    bytes: &[u8],
) -> bool {
    validate_max_length(
        eg,
        function,
        position,
        parameter,
        bytes,
        GETTEXT_MAX_MESSAGE_LENGTH,
    )
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn validate_category(
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    parameter: &str,
    category: i64,
) -> bool {
    if category != LC_ALL {
        return true;
    }
    value_error(
        eg,
        format!("{function}(): Argument #{position} (${parameter}) cannot be LC_ALL"),
    );
    false
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_textdomain(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(domain) = optional_nullable_string(ed, eg, "textdomain", 0, "domain")? else {
        return Ok(());
    };
    let domain_bytes = domain.as_ref().map(string_bytes);
    if let Some(bytes) = domain_bytes.as_deref() {
        if !validate_max_length(
            eg,
            "textdomain",
            1,
            "domain",
            bytes,
            GETTEXT_MAX_DOMAIN_LENGTH,
        ) {
            return Ok(());
        }
        if bytes == b"0" {
            value_error(eg, "textdomain(): Argument #1 ($domain) cannot be zero");
            return Ok(());
        }
        if bytes.is_empty() {
            value_error(eg, "textdomain(): Argument #1 ($domain) must not be empty");
            return Ok(());
        }
    }
    let input = domain_bytes.as_deref().map(NativeInput::new);
    if let Some(bytes) = invoke_native(NativeCall::TextDomain(input.as_ref())) {
        write_bytes(rv, bytes);
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_gettext_named(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Result<(), VmError> {
    let Some(message) = required_string(ed, eg, function, 0, "message")? else {
        return Ok(());
    };
    let message = string_bytes(&message);
    if !validate_message(eg, function, 1, "message", &message) {
        return Ok(());
    }
    let message = NativeInput::new(&message);
    if let Some(bytes) = invoke_native(NativeCall::Gettext(&message)) {
        write_bytes(rv, bytes);
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_gettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_gettext_named(ed, rv, eg, "gettext")
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_underscore(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_gettext_named(ed, rv, eg, "_")
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_dgettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(domain) = required_string(ed, eg, "dgettext", 0, "domain")? else {
        return Ok(());
    };
    let Some(message) = required_string(ed, eg, "dgettext", 1, "message")? else {
        return Ok(());
    };
    let domain = string_bytes(&domain);
    let message = string_bytes(&message);
    if !validate_domain(eg, "dgettext", 1, &domain)
        || !validate_message(eg, "dgettext", 2, "message", &message)
    {
        return Ok(());
    }
    let domain = NativeInput::new(&domain);
    let message = NativeInput::new(&message);
    if let Some(bytes) = invoke_native(NativeCall::DGettext(&domain, &message)) {
        write_bytes(rv, bytes);
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_dcgettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(domain) = required_string(ed, eg, "dcgettext", 0, "domain")? else {
        return Ok(());
    };
    let Some(message) = required_string(ed, eg, "dcgettext", 1, "message")? else {
        return Ok(());
    };
    let Some(category) = super::typed_internal_int_argument(ed, eg, "dcgettext", 2, "category")?
    else {
        return Ok(());
    };
    let domain = string_bytes(&domain);
    let message = string_bytes(&message);
    if !validate_domain(eg, "dcgettext", 1, &domain)
        || !validate_message(eg, "dcgettext", 2, "message", &message)
        || !validate_category(eg, "dcgettext", 3, "category", category)
    {
        return Ok(());
    }
    let domain = NativeInput::new(&domain);
    let message = NativeInput::new(&message);
    if let Some(bytes) = invoke_native(NativeCall::DcGettext(&domain, &message, category as c_int))
    {
        write_bytes(rv, bytes);
    }
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_bindtextdomain(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(domain) = required_string(ed, eg, "bindtextdomain", 0, "domain")? else {
        return Ok(());
    };
    let Some(directory) = optional_nullable_string(ed, eg, "bindtextdomain", 1, "directory")?
    else {
        return Ok(());
    };
    let domain = string_bytes(&domain);
    if !validate_domain(eg, "bindtextdomain", 1, &domain) {
        return Ok(());
    }
    if domain.contains(&0) {
        value_error(
            eg,
            "bindtextdomain(): Argument #1 ($domain) must not contain any null bytes",
        );
        return Ok(());
    }
    let directory = match directory {
        None => None,
        Some(directory) => {
            let directory = string_bytes(&directory);
            if directory.contains(&0) {
                value_error(
                    eg,
                    "bindtextdomain(): Argument #2 ($directory) must not contain any null bytes",
                );
                return Ok(());
            }
            let path = if directory.is_empty() || directory == b"0" {
                std::env::current_dir()
            } else {
                std::fs::canonicalize(OsStr::from_bytes(&directory))
            };
            let Ok(path) = path else {
                super::write_return_value(rv, Value::bool(false));
                return Ok(());
            };
            Some(path.into_os_string().into_vec())
        }
    };
    let domain = NativeInput::new(&domain);
    let directory = directory.as_deref().map(NativeInput::new);
    write_native_string_or_false(
        rv,
        invoke_native(NativeCall::BindTextDomain(&domain, directory.as_ref())),
    );
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_bind_textdomain_codeset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(domain) = required_string(ed, eg, "bind_textdomain_codeset", 0, "domain")? else {
        return Ok(());
    };
    let Some(codeset) = optional_nullable_string(ed, eg, "bind_textdomain_codeset", 1, "codeset")?
    else {
        return Ok(());
    };
    let domain = string_bytes(&domain);
    if !validate_domain(eg, "bind_textdomain_codeset", 1, &domain) {
        return Ok(());
    }
    let codeset = codeset.as_ref().map(string_bytes);
    let domain = NativeInput::new(&domain);
    let codeset = codeset.as_deref().map(NativeInput::new);
    write_native_string_or_false(
        rv,
        invoke_native(NativeCall::BindCodeset(&domain, codeset.as_ref())),
    );
    Ok(())
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_ngettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    plural_call(ed, rv, eg, "ngettext", false, false)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_dngettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    plural_call(ed, rv, eg, "dngettext", true, false)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn fn_dcngettext(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    plural_call(ed, rv, eg, "dcngettext", true, true)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn plural_call(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    has_domain: bool,
    has_category: bool,
) -> Result<(), VmError> {
    let mut index = 0;
    let domain = if has_domain {
        let Some(domain) = required_string(ed, eg, function, index, "domain")? else {
            return Ok(());
        };
        index += 1;
        Some(string_bytes(&domain))
    } else {
        None
    };
    let Some(singular) = required_string(ed, eg, function, index, "singular")? else {
        return Ok(());
    };
    index += 1;
    let Some(plural) = required_string(ed, eg, function, index, "plural")? else {
        return Ok(());
    };
    index += 1;
    let Some(count) = super::typed_internal_int_argument(ed, eg, function, index, "count")? else {
        return Ok(());
    };
    index += 1;
    let category = if has_category {
        let Some(category) =
            super::typed_internal_int_argument(ed, eg, function, index, "category")?
        else {
            return Ok(());
        };
        Some(category)
    } else {
        None
    };

    let singular = string_bytes(&singular);
    let plural = string_bytes(&plural);
    if let Some(domain) = domain.as_deref()
        && !validate_domain(eg, function, 1, domain)
    {
        return Ok(());
    }
    let singular_position = if has_domain { 2 } else { 1 };
    if !validate_message(eg, function, singular_position, "singular", &singular)
        || !validate_message(eg, function, singular_position + 1, "plural", &plural)
    {
        return Ok(());
    }
    if let Some(category) = category
        && !validate_category(eg, function, singular_position + 3, "category", category)
    {
        return Ok(());
    }

    let domain_input = domain.as_deref().map(NativeInput::new);
    let singular = NativeInput::new(&singular);
    let plural = NativeInput::new(&plural);
    let count = count as c_ulong;
    let result = match (domain_input.as_ref(), category) {
        (None, None) => invoke_native(NativeCall::NGettext(&singular, &plural, count)),
        (Some(domain), None) => {
            invoke_native(NativeCall::DnGettext(domain, &singular, &plural, count))
        }
        (Some(domain), Some(category)) => invoke_native(NativeCall::DcnGettext(
            domain,
            &singular,
            &plural,
            count,
            category as c_int,
        )),
        (None, Some(_)) => unreachable!("category is only used by dcngettext"),
    };
    if let Some(bytes) = result {
        write_bytes(rv, bytes);
    }
    Ok(())
}

struct Declaration {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    parameter_types: fn() -> Vec<ParamTypeHint>,
    return_type: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
    diagnostics: Option<&'static [Option<&'static str>]>,
}

fn string_type() -> ParamTypeHint {
    ParamTypeHint::String
}

fn string_or_false_type() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::String,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

fn textdomain_types() -> Vec<ParamTypeHint> {
    vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))]
}

fn bind_types() -> Vec<ParamTypeHint> {
    vec![
        ParamTypeHint::String,
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
    ]
}

fn strings<const N: usize>() -> Vec<ParamTypeHint> {
    vec![ParamTypeHint::String; N]
}

fn strings_and_int<const N: usize>() -> Vec<ParamTypeHint> {
    let mut types = vec![ParamTypeHint::String; N - 1];
    types.push(ParamTypeHint::Int);
    types
}

fn no_defaults<const N: usize>() -> Vec<Option<Value>> {
    vec![None; N]
}

const TEXTDOMAIN_DIAGNOSTICS: &[Option<&str>] = &[Some("null")];
const BIND_DIAGNOSTICS: &[Option<&str>] = &[None, Some("null")];

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        Declaration {
            name: "textdomain",
            handler: fn_textdomain,
            required: 0,
            parameters: &["domain"],
            parameter_types: textdomain_types,
            return_type: string_type,
            defaults: || vec![Some(Value::null())],
            diagnostics: Some(TEXTDOMAIN_DIAGNOSTICS),
        },
        Declaration {
            name: "gettext",
            handler: fn_gettext,
            required: 1,
            parameters: &["message"],
            parameter_types: strings::<1>,
            return_type: string_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "_",
            handler: fn_underscore,
            required: 1,
            parameters: &["message"],
            parameter_types: strings::<1>,
            return_type: string_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "dgettext",
            handler: fn_dgettext,
            required: 2,
            parameters: &["domain", "message"],
            parameter_types: strings::<2>,
            return_type: string_type,
            defaults: no_defaults::<2>,
            diagnostics: None,
        },
        Declaration {
            name: "dcgettext",
            handler: fn_dcgettext,
            required: 3,
            parameters: &["domain", "message", "category"],
            parameter_types: strings_and_int::<3>,
            return_type: string_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "bindtextdomain",
            handler: fn_bindtextdomain,
            required: 1,
            parameters: &["domain", "directory"],
            parameter_types: bind_types,
            return_type: string_or_false_type,
            defaults: || vec![None, Some(Value::null())],
            diagnostics: Some(BIND_DIAGNOSTICS),
        },
        Declaration {
            name: "ngettext",
            handler: fn_ngettext,
            required: 3,
            parameters: &["singular", "plural", "count"],
            parameter_types: strings_and_int::<3>,
            return_type: string_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "dngettext",
            handler: fn_dngettext,
            required: 4,
            parameters: &["domain", "singular", "plural", "count"],
            parameter_types: strings_and_int::<4>,
            return_type: string_type,
            defaults: no_defaults::<4>,
            diagnostics: None,
        },
        Declaration {
            name: "dcngettext",
            handler: fn_dcngettext,
            required: 5,
            parameters: &["domain", "singular", "plural", "count", "category"],
            parameter_types: || {
                vec![
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    ParamTypeHint::Int,
                ]
            },
            return_type: string_type,
            defaults: no_defaults::<5>,
            diagnostics: None,
        },
        Declaration {
            name: "bind_textdomain_codeset",
            handler: fn_bind_textdomain_codeset,
            required: 1,
            parameters: &["domain", "codeset"],
            parameter_types: bind_types,
            return_type: string_or_false_type,
            defaults: || vec![None, Some(Value::null())],
            diagnostics: Some(BIND_DIAGNOSTICS),
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let mut function = Box::new(
            make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.common.sig.param_type_hints = (declaration.parameter_types)();
        function.common.sig.return_type_hint = (declaration.return_type)();
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("gettext function registration is unique");
        let defaults = (declaration.defaults)();
        if let Some(diagnostics) = declaration.diagnostics {
            eg.register_internal_function_reflection_metadata_with_diagnostics(
                pointer,
                defaults,
                diagnostics,
                "gettext",
            );
        } else {
            eg.register_internal_function_reflection_metadata(pointer, defaults, "gettext");
        }
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gettext_limit_matches_the_php_85_observed_boundary() {
        assert_eq!(GETTEXT_MAX_DOMAIN_LENGTH, 1024);
        assert_eq!(GETTEXT_MAX_MESSAGE_LENGTH, 4096);
    }
}
