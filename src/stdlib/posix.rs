//! Linux POSIX identity and terminal functions used by PHPUnit's environment
//! projection. Native NSS and descriptor access stays behind `native_process`;
//! this module owns only PHP coercion, diagnostics and value projection.

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const ERROR_STATE: &str = "\0posix";
const LAST_ERROR: &str = "last_error";

fn return_value(pointer: *mut Value, value: Value) -> Result<(), VmError> {
    super::write_return_value(pointer, value);
    Ok(())
}

fn fn_posix_getuid(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::long(i64::from(super::native_process::user_id(false))),
    )
}

fn fn_posix_geteuid(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::long(i64::from(super::native_process::user_id(true))),
    )
}

fn last_error(eg: &ExecutorGlobals) -> i64 {
    eg.static_vars
        .get(ERROR_STATE)
        .and_then(|state| state.get(LAST_ERROR))
        .map(Value::to_long_val)
        .unwrap_or(0)
}

fn set_last_error(eg: &mut ExecutorGlobals, error: i32) {
    eg.static_vars
        .entry(ERROR_STATE.to_string())
        .or_default()
        .insert(LAST_ERROR.to_string(), Value::long(i64::from(error)));
}

fn fn_posix_get_last_error(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::long(last_error(eg)))
}

fn fn_posix_strerror(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(error) =
        super::typed_internal_int_argument(ed, eg, "posix_strerror", 0, "error_code")?
    else {
        return Ok(());
    };
    let message = super::native_process::error_message(error as i32).unwrap_or_default();
    return_value(rv, Value::string(super::bytes_to_php_string(&message)))
}

fn fn_posix_getpwuid(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(uid) = super::typed_internal_int_argument(ed, eg, "posix_getpwuid", 0, "user_id")?
    else {
        return Ok(());
    };
    let Some(record) = super::native_process::password_by_user_id(uid as u32) else {
        return return_value(rv, Value::bool(false));
    };
    let mut result = PhpArray::with_hash_capacity(7);
    result.set_str(
        "name",
        Value::string(super::bytes_to_php_string(&record.name)),
    );
    result.set_str(
        "passwd",
        Value::string(super::bytes_to_php_string(&record.password)),
    );
    result.set_str("uid", Value::long(i64::from(record.uid)));
    result.set_str("gid", Value::long(i64::from(record.gid)));
    result.set_str(
        "gecos",
        Value::string(super::bytes_to_php_string(&record.gecos)),
    );
    result.set_str(
        "dir",
        Value::string(super::bytes_to_php_string(&record.directory)),
    );
    result.set_str(
        "shell",
        Value::string(super::bytes_to_php_string(&record.shell)),
    );
    return_value(rv, Value::array(result))
}

fn posix_isatty_type_warning(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<(), VmError> {
    let actual = match value.value_type() {
        ValueType::True => "true".into(),
        ValueType::False => "false".into(),
        _ => value.diagnostic_type_name(),
    };
    super::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!(
            "posix_isatty(): Argument #1 ($file_descriptor) must be of type int|resource, {actual} given"
        ),
    )?;
    Ok(())
}

fn weak_file_descriptor(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<i64>, VmError> {
    let weakly_convertible = match value.value_type() {
        ValueType::Null | ValueType::False | ValueType::True | ValueType::Long => true,
        ValueType::Double => value.as_double().is_some_and(f64::is_finite),
        ValueType::String => {
            let source = value.as_str().unwrap_or("");
            source
                .trim_matches(|character| {
                    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
                })
                .parse::<i64>()
                .is_ok()
                || super::php_numeric_string_to_float(source).is_some()
        }
        _ => false,
    };
    if !weakly_convertible {
        posix_isatty_type_warning(ed, eg, value)?;
        return Ok(None);
    }
    super::typed_internal_int_value_argument_expected(
        ed,
        eg,
        value,
        "posix_isatty",
        0,
        "file_descriptor",
        "int",
    )
}

fn fn_posix_isatty(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let argument = super::owned_argument(ed, 0);
    let value = argument.dereferenced();
    if let Some(resource) = value.as_resource_id() {
        let terminal = super::streams::with_stream(eg, resource, |stream| stream.is_terminal());
        let Some(terminal) = terminal else {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "posix_isatty(): Could not use stream of type 'user-space'",
            )?;
            return return_value(rv, Value::bool(false));
        };
        if !terminal {
            set_last_error(eg, libc::ENOTTY);
        }
        return return_value(rv, Value::bool(terminal));
    }
    let descriptor = if super::internal_call_is_strict(ed) {
        let Some(descriptor) = value.as_long() else {
            posix_isatty_type_warning(ed, eg, value)?;
            return return_value(rv, Value::bool(false));
        };
        descriptor
    } else {
        let Some(descriptor) = weak_file_descriptor(ed, eg, value)? else {
            return return_value(rv, Value::bool(false));
        };
        descriptor
    };
    let Ok(descriptor) = i32::try_from(descriptor) else {
        set_last_error(eg, libc::EBADF);
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "posix_isatty(): Argument #1 ($file_descriptor) must be between 0 and {}",
                i32::MAX
            ),
        )?;
        return return_value(rv, Value::bool(false));
    };
    if descriptor < 0 {
        set_last_error(eg, libc::EBADF);
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "posix_isatty(): Argument #1 ($file_descriptor) must be between 0 and {}",
                i32::MAX
            ),
        )?;
        return return_value(rv, Value::bool(false));
    }
    let (terminal, error) = super::native_process::descriptor_is_terminal(descriptor);
    if !terminal {
        set_last_error(eg, error);
    }
    return_value(rv, Value::bool(terminal))
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        (
            "posix_getuid",
            fn_posix_getuid as _,
            Vec::new(),
            ParamTypeHint::Int,
        ),
        (
            "posix_geteuid",
            fn_posix_geteuid as _,
            Vec::new(),
            ParamTypeHint::Int,
        ),
        (
            "posix_get_last_error",
            fn_posix_get_last_error as _,
            Vec::new(),
            ParamTypeHint::Int,
        ),
        (
            "posix_errno",
            fn_posix_get_last_error as _,
            Vec::new(),
            ParamTypeHint::Int,
        ),
        (
            "posix_strerror",
            fn_posix_strerror as _,
            vec![ParamTypeHint::Int],
            ParamTypeHint::String,
        ),
        (
            "posix_getpwuid",
            fn_posix_getpwuid as _,
            vec![ParamTypeHint::Int],
            ParamTypeHint::Union(
                vec![
                    ParamTypeHint::Array,
                    ParamTypeHint::ClassName("false".into()),
                ]
                .into(),
            ),
        ),
        (
            "posix_isatty",
            fn_posix_isatty as _,
            vec![ParamTypeHint::None],
            ParamTypeHint::Bool,
        ),
    ];
    let mut functions = Vec::with_capacity(declarations.len());
    for (name, handler, hints, result) in declarations {
        let parameters: &[&str] = match name {
            "posix_getpwuid" => &["user_id"],
            "posix_isatty" => &["file_descriptor"],
            "posix_strerror" => &["error_code"],
            _ => &[],
        };
        let mut function = Box::new(
            make_internal_function(
                handler,
                parameters.len() as u32,
                parameters.len() as u32,
                vec![],
            )
            .with_static_parameter_names(parameters),
        );
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = result;
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer)
            .expect("POSIX functions register once per request");
        eg.register_internal_function_reflection_metadata(
            pointer,
            vec![None; parameters.len()],
            "posix",
        );
        functions.push(function);
    }
    functions
}
