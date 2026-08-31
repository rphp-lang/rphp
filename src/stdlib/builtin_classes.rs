//! Built-in Throwable, Closure, iterator, enum and SPL class registrations.
//!
//! This module owns the class handlers and their deterministic class-ID order.
//! It is declared after the parent handler macros so their frame access
//! contracts remain shared without introducing a runtime abstraction.

use super::*;
use crate::compiler::compile::ClassDef;
use crate::value::make_error_value;
use crate::vm::instruction::{
    FETCH_DIM_COMPOUND, FETCH_DIM_EMPTY, FETCH_DIM_MUTABLE, FETCH_DIM_UNSET,
};

const ROUNDING_MODE_CLASS: &str = "RoundingMode";
const ROUNDING_MODE_CASES: [&str; 8] = [
    "HalfAwayFromZero",
    "HalfTowardsZero",
    "HalfEven",
    "HalfOdd",
    "TowardsZero",
    "AwayFromZero",
    "NegativeInfinity",
    "PositiveInfinity",
];

static NULLABLE_THROWABLE_HINT: std::sync::LazyLock<ParamTypeHint> =
    std::sync::LazyLock::new(|| {
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName("Throwable".to_string())))
    });
static NULLABLE_STRING_HINT: std::sync::LazyLock<ParamTypeHint> =
    std::sync::LazyLock::new(|| ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)));
static NULLABLE_INT_HINT: std::sync::LazyLock<ParamTypeHint> =
    std::sync::LazyLock::new(|| ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)));

#[inline]
fn throwable_property_key<'a>(
    eg: &ExecutorGlobals,
    object: &'a PhpObject,
    property: &str,
) -> &'static str {
    crate::runtime::throwable_private_property_key(eg, object, property)
}

enum PreparedThrowableArgument {
    Missing,
    Value(Value),
    Invalid,
}

fn prepare_throwable_argument(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function_name: &str,
    callee_class: &str,
    public_index: usize,
    parameter: &str,
    hint: &ParamTypeHint,
) -> Result<PreparedThrowableArgument, VmError> {
    let Some(argument) = arg_opt!(ed, public_index as u32 + 1) else {
        return Ok(PreparedThrowableArgument::Missing);
    };
    let argument = argument.dereferenced().clone();
    // Throwable construction is hot during throw/catch loops. Preserve the
    // compact internal call ABI and make exact arguments a tag-only guard;
    // only weak scalar or Stringable conversion enters the canonical helper.
    let exact = match hint {
        ParamTypeHint::String => argument.value_type() == ValueType::String,
        ParamTypeHint::Int => argument.value_type() == ValueType::Long,
        ParamTypeHint::Nullable(inner) => {
            argument.value_type() == ValueType::Null
                || matches!(
                    (argument.value_type(), inner.as_ref()),
                    (ValueType::String, ParamTypeHint::String)
                        | (ValueType::Long, ParamTypeHint::Int)
                )
        }
        _ => false,
    };
    if exact {
        return Ok(PreparedThrowableArgument::Value(argument));
    }
    let prepared =
        crate::vm::execute::prepare_call_argument(&argument, hint, eg, false, Some(callee_class))?;
    match prepared {
        crate::vm::execute::CallArgumentPreparation::Exact => {
            Ok(PreparedThrowableArgument::Value(argument))
        }
        crate::vm::execute::CallArgumentPreparation::Coerced(value, _diagnostic) => {
            Ok(PreparedThrowableArgument::Value(value))
        }
        crate::vm::execute::CallArgumentPreparation::Invalid => {
            let actual = match argument.value_type() {
                ValueType::True => "true".to_string(),
                ValueType::False => "false".to_string(),
                _ => argument.diagnostic_type_name().into_owned(),
            };
            let error = make_error_value(
                "TypeError",
                &format!(
                    "{function_name}(): Argument #{} (${parameter}) must be of type {}, {actual} given",
                    public_index + 1,
                    hint.diagnostic_display_name(),
                ),
            );
            eg.exception = Some(error);
            Ok(PreparedThrowableArgument::Invalid)
        }
    }
}

fn rounding_mode_case(name: &str) -> PropertyDefinition {
    let mut properties = std::collections::HashMap::with_capacity(1);
    properties.insert("name".to_string(), Value::string(name));
    PropertyDefinition::new(
        name.to_string(),
        Some(Value::object(PhpObject::dynamic(
            ROUNDING_MODE_CLASS.to_string(),
            0,
            properties,
        ))),
        Visibility::Public,
        ROUNDING_MODE_CLASS.to_string(),
    )
}

fn fn_rounding_mode_cases(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let definition = eg
        .find_class(ROUNDING_MODE_CLASS)
        .expect("registered RoundingMode enum is available");
    let class_id = definition.class_id;
    let mut cases = PhpArray::with_packed_capacity(definition.static_properties.len());
    for index in 0..definition.static_properties.len() {
        let storage_slot = eg
            .static_property_storage_slot(class_id, index)
            .expect("registered RoundingMode case owns a static storage slot");
        let value = eg
            .static_property_value(storage_slot)
            .expect("registered RoundingMode case storage remains live")
            .clone();
        cases.push(value);
    }
    super::write_return_value(rv, Value::array(cases));
    Ok(())
}

fn register_rounding_mode(eg: &mut ExecutorGlobals) -> Box<InternalFunction> {
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: ROUNDING_MODE_CLASS.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["UnitEnum".to_string()],
        is_interface: false,
        is_abstract: false,
        // Internal enums omit ReflectionClass's final modifier. The enum
        // parent guard still rejects every attempt to extend them.
        is_final: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        is_trait: false,
        is_enum: true,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: vec![PropertyDefinition::declared(
            "name".to_string(),
            None,
            Visibility::Public,
            ROUNDING_MODE_CLASS.to_string(),
            ParamTypeHint::String,
            true,
            false,
        )],
        static_properties: ROUNDING_MODE_CASES
            .iter()
            .map(|name| rounding_mode_case(name))
            .collect(),
        constants: Vec::new(),
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec!["name".to_string()],
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("RoundingMode registration is unique");

    let mut cases_method = Box::new(make_internal_function(
        fn_rounding_mode_cases,
        0,
        0,
        Vec::new(),
    ));
    cases_method.common.sig.return_type_hint = ParamTypeHint::Array;
    let cases_pointer = &cases_method.common as *const FunctionCommon;
    eg.function_table
        .insert("roundingmode::cases".to_string(), cases_pointer);
    eg.method_declaring_class
        .insert(cases_pointer, ROUNDING_MODE_CLASS.to_string());
    cases_method
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
    let (function_name, callee_class) =
        this_val
            .as_object()
            .map_or(("Error::__construct", "Error"), |object| {
                if eg.class_is_a(&object.class_name, "Exception") {
                    ("Exception::__construct", "Exception")
                } else {
                    ("Error::__construct", "Error")
                }
            });
    let message = prepare_throwable_argument(
        ed,
        eg,
        function_name,
        callee_class,
        0,
        "message",
        &ParamTypeHint::String,
    )?;
    let code = prepare_throwable_argument(
        ed,
        eg,
        function_name,
        callee_class,
        1,
        "code",
        &ParamTypeHint::Int,
    )?;
    let previous = prepare_throwable_argument(
        ed,
        eg,
        function_name,
        callee_class,
        2,
        "previous",
        &NULLABLE_THROWABLE_HINT,
    )?;
    if matches!(
        (&message, &code, &previous),
        (PreparedThrowableArgument::Invalid, _, _)
            | (_, PreparedThrowableArgument::Invalid, _)
            | (_, _, PreparedThrowableArgument::Invalid)
    ) {
        return Ok(());
    }
    if let Some(mut obj) = this_val.as_object_mut() {
        let msg = match message {
            PreparedThrowableArgument::Value(value) => value,
            PreparedThrowableArgument::Missing => Value::string(""),
            PreparedThrowableArgument::Invalid => unreachable!(),
        };
        obj.set_property("message", msg);
        obj.set_property(
            "code",
            match code {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::long(0),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );
        let previous_key = throwable_property_key(eg, &obj, "previous");
        obj.set_property(
            &previous_key,
            match previous {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::null(),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );
    }
    Ok(())
}

/// Internal handler for ErrorException::__construct(). The object's creation
/// site has already initialized file/line before this method runs. Nullable
/// overrides only replace that origin when PHP's constructor contract says so.
fn fn_error_exception_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    let message = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        0,
        "message",
        &ParamTypeHint::String,
    )?;
    let code = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        1,
        "code",
        &ParamTypeHint::Int,
    )?;
    let severity = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        2,
        "severity",
        &ParamTypeHint::Int,
    )?;
    let filename = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        3,
        "filename",
        &NULLABLE_STRING_HINT,
    )?;
    let line = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        4,
        "line",
        &NULLABLE_INT_HINT,
    )?;
    let previous = prepare_throwable_argument(
        ed,
        eg,
        "ErrorException::__construct",
        "ErrorException",
        5,
        "previous",
        &NULLABLE_THROWABLE_HINT,
    )?;
    if [&message, &code, &severity, &filename, &line, &previous]
        .into_iter()
        .any(|argument| matches!(argument, PreparedThrowableArgument::Invalid))
    {
        return Ok(());
    }
    if let Some(mut object) = this_val.as_object_mut() {
        object.set_property(
            "message",
            match message {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::string(""),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );
        object.set_property(
            "code",
            match code {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::long(0),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );
        object.set_property(
            "severity",
            match severity {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::long(1),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );

        if let PreparedThrowableArgument::Value(filename) = filename
            && filename.value_type() != ValueType::Null
        {
            object.set_property("file", filename);
            object.set_property(
                "line",
                match line {
                    PreparedThrowableArgument::Value(line)
                        if line.value_type() != ValueType::Null =>
                    {
                        line
                    }
                    _ => Value::long(0),
                },
            );
        } else if let PreparedThrowableArgument::Value(line) = line
            && line.value_type() != ValueType::Null
        {
            object.set_property("line", line);
        }

        let previous_key = throwable_property_key(eg, &object, "previous");
        object.set_property(
            &previous_key,
            match previous {
                PreparedThrowableArgument::Value(value) => value,
                PreparedThrowableArgument::Missing => Value::null(),
                PreparedThrowableArgument::Invalid => unreachable!(),
            },
        );
    }
    Ok(())
}

fn fn_error_exception_get_severity(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let severity = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("severity").cloned())
        .unwrap_or_else(|| Value::long(1));
    ret!(rv, severity);
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
        let previous_key = throwable_property_key(eg, &obj, "previous");
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let this_val = arg!(ed, 0);
    if let Some(obj) = this_val.as_object() {
        let msg = obj
            .get_property("message")
            .cloned()
            .unwrap_or(Value::string(""));
        drop(obj);
        let Some(message) = super::internal_value_to_string_value(ed, eg, &msg)? else {
            return Ok(());
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        ret!(rv, message);
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0).as_object().map_or_else(
        || Value::array(PhpArray::new()),
        |object| {
            let key = throwable_property_key(eg, &object, "trace");
            object
                .get_property(&key)
                .cloned()
                .unwrap_or_else(|| Value::array(PhpArray::new()))
        },
    );
    ret!(rv, value);
}

fn fn_throwable_get_trace_as_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let trace = arg!(ed, 0)
        .as_object()
        .map_or_else(PhpArray::new, |object| {
            let key = throwable_property_key(eg, &object, "trace");
            object
                .get_property(&key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(PhpArray::new)
        });
    let (rendered, warnings) = crate::vm::trace::format_throwable_trace_checked(
        &trace,
        exception_string_param_max_len(eg),
        eg,
    );
    for warning in warnings {
        super::report_internal_diagnostic(eg, ed, 2, "Warning", &warning)?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    ret!(rv, Value::string(rendered));
}

fn fn_throwable_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let thrown = arg!(ed, 0).clone();
    let exact_terminal_message = thrown.as_object().is_some_and(|object| {
        let previous_key = throwable_property_key(eg, &object, "previous");
        object
            .get_property("message")
            .map(Value::dereferenced)
            .is_none_or(|message| message.value_type() == ValueType::String)
            && object.get_property(&previous_key).is_none_or(|previous| {
                previous
                    .dereferenced()
                    .as_object()
                    .is_none_or(|previous| !eg.class_is_a(&previous.class_name, "Throwable"))
            })
    });
    if exact_terminal_message {
        let object = thrown
            .as_object()
            .expect("terminal Throwable string formatting requires an object");
        let trace_key = throwable_property_key(eg, &object, "trace");
        let message = object
            .get_property("message")
            .map(Value::dereferenced)
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut rendered = object.class_name.to_string();
        if !message.is_empty() {
            rendered.push_str(": ");
            rendered.push_str(message);
        }
        if let Some(((file, line), trace)) = object
            .get_property("file")
            .map(Value::dereferenced)
            .and_then(Value::as_str)
            .zip(
                object
                    .get_property("line")
                    .map(Value::dereferenced)
                    .and_then(Value::as_long),
            )
            .zip(
                object
                    .get_property(&trace_key)
                    .map(Value::dereferenced)
                    .and_then(Value::as_array),
            )
        {
            if object.class_name.as_ref().eq_ignore_ascii_case("TypeError")
                && message.contains(", called in ")
                && message.contains(" on line ")
            {
                rendered.push_str(" and defined in ");
            } else {
                rendered.push_str(" in ");
            }
            rendered.push_str(file);
            rendered.push(':');
            rendered.push_str(&line.to_string());
            rendered.push_str("\nStack trace:\n");
            rendered.push_str(&crate::vm::trace::format_throwable_trace(
                trace,
                exception_string_param_max_len(eg),
                eg,
            ));
        }
        drop(object);
        if let Some(mut object) = arg!(ed, 0).as_object_mut() {
            let key = throwable_property_key(eg, &object, "string");
            object.set_property(&key, Value::string(&rendered));
        }
        ret!(rv, Value::string(rendered));
    }
    let mut messages = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut current = thrown.clone();
    loop {
        let Some(identity) = current.object_identity() else {
            break;
        };
        if !seen.insert(identity) {
            break;
        }
        let (message, previous) = {
            let Some(object) = current.as_object() else {
                break;
            };
            let previous_key = throwable_property_key(eg, &object, "previous");
            (
                object
                    .get_property("message")
                    .cloned()
                    .unwrap_or_else(|| Value::string("")),
                object.get_property(&previous_key).cloned(),
            )
        };
        let Some(message) = super::internal_value_to_string_value(ed, eg, &message)? else {
            return Ok(());
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        messages.insert(identity, message.as_str().unwrap_or_default().to_string());
        let Some(previous) = previous.filter(|previous| {
            previous
                .dereferenced()
                .as_object()
                .is_some_and(|object| eg.class_is_a(&object.class_name, "Throwable"))
        }) else {
            break;
        };
        current = previous.dereferenced().clone();
    }
    let rendered =
        crate::vm::execute::format_throwable_string_with_messages(eg, &thrown, &messages);
    if let Some(mut object) = arg!(ed, 0).as_object_mut() {
        let key = throwable_property_key(eg, &object, "string");
        object.set_property(&key, Value::string(&rendered));
    }
    ret!(rv, Value::string(rendered));
}

fn bind_closure_value(
    ed: *mut ExecuteData,
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
    if let Some(scope) = scope
        && !matches!(
            scope.value_type(),
            ValueType::Null | ValueType::String | ValueType::Object | ValueType::Closure
        )
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "{api}(): Argument #{scope_argument} ($newScope) must be of type object|string|null, {} given",
                scope.dereferenced().type_name()
            ),
        ));
        return Ok(());
    }
    let mut rebound = source.clone();

    if new_this.value_type() == ValueType::Null && source.bound_this.is_some() {
        let uses_this = source.user_function().is_some_and(|function| {
            function
                .op_array
                .all_cvs
                .iter()
                .any(|(_, name)| name == "this")
        });
        if uses_this {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "Cannot unbind $this of closure using $this, this will be an error in PHP 9",
            )?;
            ret!(rv, Value::null());
        }
    }

    rebound.bound_this = match new_this.value_type() {
        ValueType::Null => None,
        ValueType::Object | ValueType::Closure if rebound.is_static => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "Cannot bind an instance to a static closure, this will be an error in PHP 9",
            )?;
            ret!(rv, Value::null());
        }
        ValueType::Object | ValueType::Closure => Some(new_this.clone()),
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{api}(): Argument #{new_this_argument} ($newThis) must be of type ?object, {} given",
                    new_this.dereferenced().type_name()
                ),
            ));
            return Ok(());
        }
    };

    if let Some(function) = source
        .user_function()
        .filter(|function| function.common.sig.this_offset == 1)
        && let Some(declaring_class) = eg.declaring_class_of(source.func).map(str::to_owned)
    {
        let method = function
            .op_array
            .name
            .rsplit_once("::")
            .map_or(function.op_array.name.as_str(), |(_, method)| method)
            .to_string();
        if let Some(receiver_class) = rebound.bound_this.as_ref().map(|receiver| {
            receiver.as_object().map_or_else(
                || "Closure".to_string(),
                |object| object.class_name.to_string(),
            )
        }) && !eg.class_is_a(&receiver_class, &declaring_class)
        {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!(
                    "Cannot bind method {declaring_class}::{method}() to object of class {receiver_class}, this will be an error in PHP 9"
                ),
            )?;
            ret!(rv, Value::null());
        }
        if let Some(scope) = scope {
            let preserves_method_scope = match scope.value_type() {
                ValueType::String
                    if scope
                        .as_str()
                        .is_some_and(|scope| scope.eq_ignore_ascii_case("static")) =>
                {
                    true
                }
                ValueType::String => scope
                    .as_str()
                    .and_then(|scope| eg.find_class(scope))
                    .is_some_and(|class| class.name.eq_ignore_ascii_case(&declaring_class)),
                ValueType::Object => scope
                    .as_object()
                    .is_some_and(|object| object.class_name.eq_ignore_ascii_case(&declaring_class)),
                _ => false,
            };
            if !preserves_method_scope {
                super::report_internal_diagnostic(
                    eg,
                    ed,
                    2,
                    "Warning",
                    "Cannot rebind scope of closure created from method, this will be an error in PHP 9",
                )?;
                ret!(rv, Value::null());
            }
        }
    }

    if let Some(scope) = scope {
        rebound.called_scope_class_id = match scope.value_type() {
            ValueType::Null => {
                rebound.scope_is_dummy = rebound.bound_this.is_some();
                0
            }
            ValueType::String
                if scope
                    .as_str()
                    .is_some_and(|scope| scope.eq_ignore_ascii_case("static")) =>
            {
                rebound.scope_is_dummy = rebound.bound_this.is_some()
                    && (source.scope_is_dummy || source.called_scope_class_id == 0);
                source.called_scope_class_id
            }
            ValueType::String => {
                let name = scope.as_str().unwrap_or_default();
                if eg.find_class(name).is_none()
                    && !super::autoload::ensure_symbol_loaded(eg, name)?
                {
                    super::report_internal_diagnostic(
                        eg,
                        ed,
                        2,
                        "Warning",
                        &format!("Class \"{name}\" not found"),
                    )?;
                    ret!(rv, Value::null());
                }
                let Some(class) = eg.find_class(name) else {
                    ret!(rv, Value::null());
                };
                let class_name = class.name.clone();
                let class_id = class.class_id;
                if eg.class_is_internal(&class_name) {
                    super::report_internal_diagnostic(
                        eg,
                        ed,
                        2,
                        "Warning",
                        &format!(
                            "Cannot bind closure to scope of internal class {}, this will be an error in PHP 9",
                            class_name
                        ),
                    )?;
                    ret!(rv, Value::null());
                }
                rebound.scope_is_dummy = false;
                class_id
            }
            ValueType::Object | ValueType::Closure => {
                let class_name = scope.as_object().map_or_else(
                    || "Closure".to_string(),
                    |object| object.class_name.to_string(),
                );
                if eg.class_is_internal(&class_name) {
                    super::report_internal_diagnostic(
                        eg,
                        ed,
                        2,
                        "Warning",
                        &format!(
                            "Cannot bind closure to scope of internal class {class_name}, this will be an error in PHP 9"
                        ),
                    )?;
                    ret!(rv, Value::null());
                }
                rebound.scope_is_dummy = false;
                eg.find_class(&class_name).map_or(0, |class| class.class_id)
            }
            _ => {
                eg.exception = Some(crate::value::make_error_value(
                    "TypeError",
                    &format!(
                        "{api}(): Argument #{scope_argument} ($newScope) must be of type object|string|null, {} given",
                        scope.dereferenced().type_name()
                    ),
                ));
                return Ok(());
            }
        };
    } else if rebound.bound_this.is_some() && source.called_scope_class_id == 0 {
        // An omitted scope preserves an existing lexical scope. An unscoped
        // closure bound to an object receives Closure's dummy lexical scope
        // rather than the receiver class's private visibility. The bound
        // receiver carrier supplies its separate late-static called class.
        rebound.scope_is_dummy = true;
    }

    ret!(rv, Value::closure(rebound));
}

fn fn_closure_bind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bind_closure_value(
        ed,
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
        ed,
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

fn existing_closure_callable(callable: &Value) -> Option<Value> {
    if callable.value_type() == ValueType::Closure {
        return Some(callable.clone());
    }
    callable.as_array().and_then(|array| {
        (array.len() == 2
            && array
                .get_value_at(1)
                .and_then(Value::as_str)
                .is_some_and(|method| method.eq_ignore_ascii_case("__invoke")))
        .then(|| array.get_value_at(0))
        .flatten()
        .filter(|receiver| receiver.value_type() == ValueType::Closure)
        .cloned()
    })
}

#[cold]
#[inline(never)]
fn fn_closure_from_callable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let callable = arg!(ed, 1);
    if let Some(closure) = existing_closure_callable(callable) {
        ret!(rv, closure);
    }

    let resolved = resolve_callback_at_callsite_checked(callable, eg, ed)?;
    let Some(resolved) = resolved else {
        if eg.exception.is_some() {
            return Ok(());
        }
        let caller_class = get_calling_scope_class(ed, eg);
        let mut reason = first_class_callable_error(callable, eg, caller_class.as_deref());
        if reason.starts_with("Non-static method ") {
            reason.replace_range(..1, "n");
        }
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!("Failed to create closure from callable: {reason}"),
        ));
        return Ok(());
    };
    ret!(rv, resolved_callback_into_closure(resolved, eg));
}

#[cold]
#[inline(never)]
fn take_closure_static_property_caches(source: &PhpClosure) -> Vec<(usize, InlineCache)> {
    let Some(function) = source.user_function() else {
        return vec![];
    };
    let op_array = &function.op_array;
    let mut saved = Vec::new();
    for (index, instruction) in op_array.instructions.iter().enumerate() {
        if !matches!(
            instruction.opcode,
            OpCode::FetchStaticProp
                | OpCode::FetchLateStaticProp
                | OpCode::AssignStaticProp
                | OpCode::AssignLateStaticProp
        ) {
            continue;
        }
        // SAFETY: each instruction owns one cache entry. Closure::call is
        // synchronous in the single-threaded VM, so temporarily replacing
        // only static-property entries cannot race another activation.
        unsafe {
            let slot = op_array.cache.as_ptr().add(index) as *mut InlineCache;
            saved.push((index, *slot));
            slot.write(InlineCache::empty());
        }
    }
    saved
}

#[cold]
#[inline(never)]
fn restore_closure_static_property_caches(source: &PhpClosure, saved: Vec<(usize, InlineCache)>) {
    let Some(function) = source.user_function() else {
        return;
    };
    for (index, cache) in saved {
        // SAFETY: these are the exact entries detached above and the closure's
        // function storage outlives its synchronous invocation.
        unsafe {
            let slot = function.op_array.cache.as_ptr().add(index) as *mut InlineCache;
            slot.write(cache);
        }
    }
}

#[cold]
#[inline(never)]
fn fn_closure_call(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source_value = arg!(ed, 0);
    let Some(source) = source_value.as_closure() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "Closure::call(): receiver must be of type Closure",
        ));
        return Ok(());
    };
    let new_this = arg!(ed, 1);
    let Some(object) = new_this.as_object() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "Closure::call(): Argument #1 ($newThis) must be of type object, {} given",
                new_this.dereferenced().type_name()
            ),
        ));
        return Ok(());
    };
    let object_class_name = object.class_name.to_string();
    drop(object);
    let Some(scope) = eg.find_class(&object_class_name) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Class \"{object_class_name}\" not found"),
        ));
        return Ok(());
    };
    if let Some(declaring_class) = eg.declaring_class_of(source.func)
        && source
            .user_function()
            .is_some_and(|function| function.common.sig.this_offset == 1)
        && !eg.class_is_a(&object_class_name, declaring_class)
    {
        let method = source
            .user_function()
            .map(|function| function.op_array.name.as_str())
            .unwrap_or("unknown")
            .rsplit_once("::")
            .map_or_else(
                || {
                    source
                        .user_function()
                        .map_or("unknown", |function| function.op_array.name.as_str())
                },
                |(_, method)| method,
            );
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "Cannot bind method {declaring_class}::{method}() to object of class {}, this will be an error in PHP 9",
                object_class_name
            ),
        )?;
        ret!(rv, Value::null());
    }
    if source.user_function().is_some() && eg.class_is_internal(&object_class_name) {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "Cannot bind closure to scope of internal class {}, this will be an error in PHP 9",
                object_class_name
            ),
        )?;
        ret!(rv, Value::null());
    }
    if source.is_static {
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "Cannot bind an instance to a static closure, this will be an error in PHP 9",
        )?;
        ret!(rv, Value::null());
    }

    let Some(mut resolved) = resolve_callback(source_value, eg, None) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Failed to invoke closure",
        ));
        return Ok(());
    };
    resolved.bound_this = Some(new_this.clone());
    resolved.called_scope_class_id = scope.class_id;
    if resolved.signature().this_offset == 1 {
        resolved.prepend_args = vec![new_this.clone()];
    }
    let arguments = arg!(ed, 2)
        .as_array()
        .cloned()
        .unwrap_or_else(PhpArray::new);
    let saved_static_caches = take_closure_static_property_caches(source);
    let requires_reference_handling = super::callback_has_hard_reference_parameters(&resolved);
    let result = if arguments.has_string_keys() || requires_reference_handling {
        super::call_resolved_with_php_array(eg, resolved, &arguments, true)
    } else {
        call_resolved_with_array(eg, &resolved, &arguments)
    };
    restore_closure_static_property_caches(source, saved_static_caches);
    let result = result?;
    ret!(rv, result);
}

#[cold]
#[inline(never)]
fn reject_closure_invoke_nonreferenceable_arguments(
    resolved: &ResolvedCallback,
    arguments: &PhpArray,
    eg: &mut ExecutorGlobals,
) -> bool {
    let signature = &resolved.common().sig;
    let public_arity = signature.public_arity() as usize;
    let mut positional_index = 0usize;
    for (key, value) in arguments.iter() {
        let parameter_index = match key {
            ArrayKey::Int(_) => {
                let index = positional_index;
                positional_index += 1;
                index
            }
            ArrayKey::String(name) => signature
                .param_names
                .iter()
                .position(|parameter| parameter == name.as_str())
                .unwrap_or(public_arity),
        };
        let reference_index = if parameter_index < public_arity {
            parameter_index
        } else if signature.is_variadic {
            public_arity
        } else {
            parameter_index
        };
        if !signature.is_param_by_ref(reference_index as u32)
            || signature.is_param_prefer_ref(reference_index as u32)
            || value.is_reference()
            || value.is_owned_reference()
        {
            continue;
        }
        let parameter = signature
            .param_names
            .get(reference_index)
            .map(String::as_str)
            .unwrap_or("unknown");
        eg.exception = Some(make_error_value(
            "Error",
            &format!(
                "Closure::__invoke(): Argument #{} (${parameter}) could not be passed by reference",
                reference_index + 1,
            ),
        ));
        return true;
    }
    false
}

#[cold]
#[inline(never)]
fn fn_closure_invoke(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = arg!(ed, 0);
    let Some(resolved) = resolve_callback(source, eg, None) else {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            "Failed to invoke closure",
        ));
        return Ok(());
    };
    let arguments = arg!(ed, 1)
        .as_array()
        .cloned()
        .unwrap_or_else(PhpArray::new);
    let requires_reference_handling = super::callback_has_hard_reference_parameters(&resolved);
    if requires_reference_handling
        && reject_closure_invoke_nonreferenceable_arguments(&resolved, &arguments, eg)
    {
        return Ok(());
    }
    let result = if arguments.has_string_keys() || requires_reference_handling {
        super::call_resolved_with_php_array(eg, resolved, &arguments, true)?
    } else {
        call_resolved_with_array(eg, &resolved, &arguments)?
    };
    ret!(rv, result);
}

const ARRAY_OBJECT_STORAGE: &str = "ArrayObject\0storage";
const ARRAY_ITERATOR_STORAGE: &str = "ArrayIterator\0storage";

#[inline]
fn array_object_storage_key(object: &PhpObject) -> &'static str {
    if object.contains_property(ARRAY_OBJECT_STORAGE) {
        ARRAY_OBJECT_STORAGE
    } else {
        ARRAY_ITERATOR_STORAGE
    }
}

pub(crate) fn array_object_iterable_values(receiver: &Value) -> Option<Value> {
    receiver.as_object().and_then(|object| {
        let storage = array_object_storage_key(&object);
        object.get_property(storage).cloned()
    })
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
        let storage = array_object_storage_key(&object);
        object.set_property(storage, values);
    }
    Ok(())
}

fn fn_array_iterator_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = arg!(ed, 0).as_object().map_or(0, |object| {
        let storage = array_object_storage_key(&object);
        object
            .get_property(storage)
            .and_then(Value::as_array)
            .map_or(0, PhpArray::len)
    });
    ret!(rv, Value::long(count as i64));
}

fn fn_array_object_append(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 1).dereferenced().clone();
    if let Some(mut object) = arg!(ed, 0).as_object_mut()
        && let storage = array_object_storage_key(&object)
        && let Some(values) = object
            .get_property_mut(storage)
            .and_then(Value::as_array_mut)
        && !values.try_push(value)
    {
        eg.exception = Some(make_error_value(
            "Error",
            "Cannot add element to the array as the next element is already occupied",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ArrayObjectOffsetOperation {
    Access,
    Isset,
    Unset,
}

fn array_object_offset_key(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
    operation: ArrayObjectOffsetOperation,
) -> Result<Option<ArrayKey>, VmError> {
    let value = value.dereferenced();
    let key = match value_to_array_key(value) {
        Ok(key) => key,
        Err(ArrayKeyError::DeprecatedNull) => {
            report_internal_deprecation(
                eg,
                ed,
                "Using null as an array offset is deprecated, use an empty string instead",
            )?;
            ArrayKey::String(String::new())
        }
        Err(ArrayKeyError::DeprecatedFloat(integer)) => {
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "Implicit conversion from float {} to int loses precision",
                    value.echo_to_string_with_precision(-1)
                ),
            )?;
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::NonRepresentableFloat {
            integer,
            also_deprecated,
        }) => {
            let rendered = value.echo_to_string_with_precision(-1);
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("The float {rendered} is not representable as an int, cast occurred"),
            )?;
            if eg.exception.is_none() && also_deprecated {
                report_internal_deprecation(
                    eg,
                    ed,
                    &format!("Implicit conversion from float {rendered} to int loses precision"),
                )?;
            }
            ArrayKey::Int(integer)
        }
        Err(ArrayKeyError::Resource(resource)) => {
            report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("Resource ID#{resource} used as offset, casting to integer ({resource})"),
            )?;
            ArrayKey::Int(resource)
        }
        Err(ArrayKeyError::Illegal) => {
            let kind = value.diagnostic_type_name();
            let message = match operation {
                ArrayObjectOffsetOperation::Access => {
                    format!("Cannot access offset of type {kind} on ArrayObject")
                }
                ArrayObjectOffsetOperation::Isset => {
                    format!("Cannot access offset of type {kind} in isset or empty")
                }
                ArrayObjectOffsetOperation::Unset => {
                    format!("Cannot unset offset of type {kind} on ArrayObject")
                }
            };
            eg.exception = Some(make_error_value("TypeError", &message));
            return Ok(None);
        }
    };
    Ok(eg.exception.is_none().then_some(key))
}

#[inline]
fn array_object_value<'a>(array: &'a PhpArray, key: &ArrayKey) -> Option<&'a Value> {
    match key {
        ArrayKey::Int(key) => array.get_int(*key),
        ArrayKey::String(key) => array.get_str(key),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayObjectOffsetGetContext {
    Read,
    Mutable,
    Append,
    AfterExists,
    Unset,
}

fn array_object_offset_get_context(ed: *mut ExecuteData) -> ArrayObjectOffsetGetContext {
    // Internal ArrayObject dimensions are engine handlers rather than user
    // overloads. The current-site protocol bridge links their synchronous
    // activation to the consuming opcode, which is the distinction Zend uses
    // between a read, a writable dimension, and anonymous append fetch.
    let Some((opcode, flags)) = crate::vm::execute::instruction_for_internal_call(ed) else {
        return ArrayObjectOffsetGetContext::Read;
    };
    match opcode {
        OpCode::BindArrayAppendRef => ArrayObjectOffsetGetContext::Append,
        OpCode::BindArrayDimRef => ArrayObjectOffsetGetContext::Mutable,
        OpCode::FetchDimR if flags & FETCH_DIM_UNSET != 0 => ArrayObjectOffsetGetContext::Unset,
        OpCode::FetchDimR if flags & FETCH_DIM_EMPTY != 0 => {
            ArrayObjectOffsetGetContext::AfterExists
        }
        OpCode::FetchDimR if flags & FETCH_DIM_COMPOUND != 0 => ArrayObjectOffsetGetContext::Read,
        OpCode::FetchDimR if flags & FETCH_DIM_MUTABLE != 0 => ArrayObjectOffsetGetContext::Mutable,
        _ => ArrayObjectOffsetGetContext::Read,
    }
}

fn array_object_offset_key_after_exists(value: &Value) -> Option<ArrayKey> {
    match value_to_array_key(value.dereferenced()) {
        Ok(key) => Some(key),
        Err(ArrayKeyError::DeprecatedNull) => Some(ArrayKey::String(String::new())),
        Err(ArrayKeyError::DeprecatedFloat(integer)) | Err(ArrayKeyError::Resource(integer)) => {
            Some(ArrayKey::Int(integer))
        }
        Err(ArrayKeyError::NonRepresentableFloat { integer, .. }) => Some(ArrayKey::Int(integer)),
        Err(ArrayKeyError::Illegal) => None,
    }
}

fn fn_array_object_offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let context = array_object_offset_get_context(ed);
    if context == ArrayObjectOffsetGetContext::Append {
        ret!(rv, Value::null());
    }
    if context == ArrayObjectOffsetGetContext::Unset
        && matches!(
            value_to_array_key(arg!(ed, 1).dereferenced()),
            Err(ArrayKeyError::Illegal)
        )
    {
        ret!(rv, Value::null());
    }
    let key = if context == ArrayObjectOffsetGetContext::AfterExists {
        let Some(key) = array_object_offset_key_after_exists(arg!(ed, 1)) else {
            return Ok(());
        };
        key
    } else {
        let Some(key) =
            array_object_offset_key(ed, eg, arg!(ed, 1), ArrayObjectOffsetOperation::Access)?
        else {
            return Ok(());
        };
        key
    };
    let Some(mut object) = arg!(ed, 0).as_object_mut() else {
        ret!(rv, Value::null());
    };
    let storage = array_object_storage_key(&object);
    let Some(array) = object
        .get_property_mut(storage)
        .and_then(Value::as_array_mut)
    else {
        ret!(rv, Value::null());
    };
    let key = if context == ArrayObjectOffsetGetContext::Mutable {
        array.prepare_string_key_for_write(key, arg!(ed, 1).dereferenced())
    } else {
        array.normalize_string_key(key, arg!(ed, 1).dereferenced())
    };
    if array_object_value(array, &key).is_none() && context == ArrayObjectOffsetGetContext::Mutable
    {
        array.set(key.clone(), Value::null());
    } else if array_object_value(array, &key).is_none() {
        let displayed = match &key {
            ArrayKey::Int(key) => key.to_string(),
            ArrayKey::String(key) => format!("\"{key}\""),
        };
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("Undefined array key {displayed}"),
        )?;
        ret!(rv, Value::null());
    }
    let slot = array
        .get_key_mut(&key)
        .expect("ArrayObject storage key was checked before reference promotion");
    let binding = if slot.is_owned_reference() {
        slot.clone_owned_reference_alias()
    } else if slot.is_reference() {
        // The ArrayObject storage retains the borrowed reference while the
        // protocol result is consumed synchronously by the VM.
        slot.clone_closure_capture()
    } else {
        let value = std::mem::replace(slot, Value::undef());
        let reference = Value::owned_reference(value);
        let binding = reference.clone_owned_reference_alias();
        *slot = reference;
        binding
    };
    ret!(rv, binding);
}

fn fn_array_object_offset_set(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 2).dereferenced().clone();
    let append = arg!(ed, 1).value_type() == ValueType::Null;
    let key = if append {
        None
    } else {
        let Some(key) =
            array_object_offset_key(ed, eg, arg!(ed, 1), ArrayObjectOffsetOperation::Access)?
        else {
            return Ok(());
        };
        Some(key)
    };
    let Some(mut object) = arg!(ed, 0).as_object_mut() else {
        return Ok(());
    };
    let storage = array_object_storage_key(&object);
    let Some(array) = object
        .get_property_mut(storage)
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    if let Some(key) = key {
        let key = array.prepare_string_key_for_write(key, arg!(ed, 1).dereferenced());
        array.set(key, value);
    } else if !array.try_push(value) {
        eg.exception = Some(make_error_value(
            "Error",
            "Cannot add element to the array as the next element is already occupied",
        ));
    }
    Ok(())
}

fn fn_array_object_offset_exists(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) =
        array_object_offset_key(ed, eg, arg!(ed, 1), ArrayObjectOffsetOperation::Isset)?
    else {
        return Ok(());
    };
    let exists = arg!(ed, 0).as_object().is_some_and(|object| {
        let storage = array_object_storage_key(&object);
        object
            .get_property(storage)
            .and_then(Value::as_array)
            .and_then(|array| {
                let key = array.normalize_string_key(key, arg!(ed, 1).dereferenced());
                array_object_value(array, &key)
            })
            .is_some_and(|value| value.dereferenced().value_type() != ValueType::Null)
    });
    ret!(rv, Value::bool(exists));
}

fn fn_array_object_offset_unset(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) =
        array_object_offset_key(ed, eg, arg!(ed, 1), ArrayObjectOffsetOperation::Unset)?
    else {
        return Ok(());
    };
    if let Some(mut object) = arg!(ed, 0).as_object_mut()
        && let storage = array_object_storage_key(&object)
        && let Some(array) = object
            .get_property_mut(storage)
            .and_then(Value::as_array_mut)
    {
        let key = array.normalize_string_key(key, arg!(ed, 1).dereferenced());
        array.remove(&key);
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

pub(super) fn spl_object_storage_internal_properties() -> &'static [&'static str] {
    &[
        SPL_STORAGE_DATA,
        SPL_STORAGE_OBJECTS,
        SPL_STORAGE_ITERATOR,
        SPL_STORAGE_POSITION,
    ]
}

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

pub(super) fn spl_object_storage_wire_entries(receiver: &Value) -> Option<PhpArray> {
    let values = spl_storage_array(receiver, SPL_STORAGE_DATA);
    let iterator = spl_storage_array(receiver, SPL_STORAGE_ITERATOR);
    let mut entries = PhpArray::with_packed_capacity(iterator.len().saturating_mul(2));
    for object in iterator.values() {
        let identity = i64::try_from(object.object_identity()?).ok()?;
        let info = values
            .get_int(identity)
            .cloned()
            .unwrap_or_else(Value::null);
        entries.push(object.clone());
        entries.push(info);
    }
    Some(entries)
}

pub(super) fn restore_spl_object_storage(
    receiver: &Value,
    entries: &PhpArray,
) -> Result<(), &'static str> {
    if entries.len() % 2 != 0 {
        return Err("Odd number of elements");
    }
    let mut decoded = Vec::with_capacity(entries.len() / 2);
    let mut values = entries.values();
    while let Some(object) = values.next() {
        let Some(info) = values.next() else {
            return Err("Odd number of elements");
        };
        let Some(identity) = object.object_identity() else {
            return Err("Non-object key");
        };
        decoded.push((
            i64::try_from(identity).map_err(|_| "Non-object key")?,
            object.clone(),
            info.dereferenced().clone(),
        ));
    }

    if let Some(mut object) = receiver.as_object_mut() {
        object.set_property(SPL_STORAGE_DATA, Value::array(PhpArray::new()));
        object.set_property(SPL_STORAGE_OBJECTS, Value::array(PhpArray::new()));
        object.set_property(SPL_STORAGE_ITERATOR, Value::array(PhpArray::new()));
        object.set_property(SPL_STORAGE_POSITION, Value::long(0));
    }
    for (identity, object, info) in decoded {
        spl_storage_store(receiver, identity, object, info);
    }
    Ok(())
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

const SENSITIVE_PARAMETER_VALUE_CLASS: &str = "SensitiveParameterValue";

#[inline]
fn sensitive_parameter_value_key() -> String {
    crate::runtime::mangle_private_prop(SENSITIVE_PARAMETER_VALUE_CLASS, "value")
}

pub(super) fn sensitive_parameter_value(eg: &ExecutorGlobals, value: Value) -> Value {
    let class = eg
        .find_class(SENSITIVE_PARAMETER_VALUE_CLASS)
        .expect("SensitiveParameterValue must be registered before execution");
    Value::object(PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        vec![value.dereferenced().clone()],
    ))
}

fn fn_sensitive_parameter_value_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 1).dereferenced().clone();
    let key = sensitive_parameter_value_key();
    if let Some(mut object) = arg!(ed, 0).as_object_mut() {
        if object
            .get_property(&key)
            .is_some_and(|stored| !stored.is_undef())
        {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "Cannot modify readonly property SensitiveParameterValue::$value",
            ));
            return Ok(());
        }
        object.set_property(&key, value);
    }
    Ok(())
}

fn fn_sensitive_parameter_value_get_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let key = sensitive_parameter_value_key();
    let value = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property(&key).cloned())
        .unwrap_or_else(Value::null);
    ret!(rv, value);
}

fn fn_sensitive_parameter_value_debug_info(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::array(PhpArray::new()));
}

#[cold]
fn register_value_error(eg: &mut ExecutorGlobals) -> [Box<InternalFunction>; 2] {
    use crate::compiler::compile::ClassDef;

    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ValueError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
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

    // Internal static methods use the same hidden CV 0 ABI as instance
    // methods, so retain their dispatch kind in request-owned metadata.
    macro_rules! reg_static_method {
        ($class:expr, $method:expr, $handler:expr, $num_args:expr, $min_args:expr, $($pnames:expr),*) => {{
            let f = Box::new(make_internal_method($handler, $num_args, $min_args, vec![$($pnames.to_string()),*]));
            let ptr = &f.common as *const FunctionCommon;
            let full_name = format!("{}::{}", $class, $method).to_lowercase();
            eg.function_table.insert(full_name, ptr);
            eg.method_declaring_class.insert(ptr, $class.to_string());
            eg.register_internal_static_method(ptr);
            funcs.push(f);
        }};
    }

    // Throwable — proper interface (PHP 8 compatible)
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "Throwable".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec![],
        is_interface: true,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // Exception implements Throwable
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "Exception".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![
            PropertyDefinition::new(
                "message".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::declared(
                "string".to_string(),
                Some(Value::string("")),
                Visibility::Private,
                "Exception".to_string(),
                ParamTypeHint::String,
                false,
                false,
            ),
            PropertyDefinition::new(
                "code".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::new(
                "file".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::new(
                "line".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Exception".to_string(),
            ),
            PropertyDefinition::declared(
                "trace".to_string(),
                Some(Value::array(PhpArray::new())),
                Visibility::Private,
                "Exception".to_string(),
                ParamTypeHint::Array,
                false,
                false,
            ),
            PropertyDefinition::declared(
                "previous".to_string(),
                Some(Value::null()),
                Visibility::Private,
                "Exception".to_string(),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
                    "Throwable".to_string(),
                ))),
                false,
                false,
            ),
        ],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // SPL's logic/runtime exception families share Exception's constructor and
    // properties. Register parents before children so ClassDef inheritance can
    // materialize their layouts immediately.
    for &(name, parent) in BUILTIN_EXCEPTION_SUBCLASSES {
        eg.register_class(ClassDef {
            attributes: Vec::new(),
            name: name.to_string(),
            source_file: None,
            declaration_line: 0,
            parent: Some(parent.to_string()),
            implements: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_trait: false,
            is_enum: false,
            is_readonly: false,
            allow_dynamic_properties: false,
            uses: vec![],
            trait_aliases: vec![],
            trait_precedences: vec![],
            properties: vec![],
            static_properties: vec![],
            constants: vec![],
            property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
            property_defaults: std::rc::Rc::from([]),
            readonly_props: vec![],
            methods: vec![],
            abstract_methods: vec![],
            enum_backing_error: None,
            deferred_instance_defaults: None,
            class_id: 0,
        })
        .unwrap();
    }

    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ErrorException".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("Exception".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![PropertyDefinition::new(
            "severity".to_string(),
            Some(Value::long(1)),
            Visibility::Protected,
            "ErrorException".to_string(),
        )],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // Error implements Throwable
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "Error".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Throwable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![
            PropertyDefinition::new(
                "message".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::declared(
                "string".to_string(),
                Some(Value::string("")),
                Visibility::Private,
                "Error".to_string(),
                ParamTypeHint::String,
                false,
                false,
            ),
            PropertyDefinition::new(
                "code".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::new(
                "file".to_string(),
                Some(Value::string("")),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::new(
                "line".to_string(),
                Some(Value::long(0)),
                Visibility::Protected,
                "Error".to_string(),
            ),
            PropertyDefinition::declared(
                "trace".to_string(),
                Some(Value::array(PhpArray::new())),
                Visibility::Private,
                "Error".to_string(),
                ParamTypeHint::Array,
                false,
                false,
            ),
            PropertyDefinition::declared(
                "previous".to_string(),
                Some(Value::null()),
                Visibility::Private,
                "Error".to_string(),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
                    "Throwable".to_string(),
                ))),
                false,
                false,
            ),
        ],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    for &(name, parent) in BUILTIN_ERROR_SUBCLASSES {
        eg.register_class(ClassDef {
            attributes: Vec::new(),
            name: name.to_string(),
            source_file: None,
            declaration_line: 0,
            parent: Some(parent.to_string()),
            implements: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_trait: false,
            is_enum: false,
            is_readonly: false,
            allow_dynamic_properties: false,
            uses: vec![],
            trait_aliases: vec![],
            trait_precedences: vec![],
            properties: vec![],
            static_properties: vec![],
            constants: vec![],
            property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
            property_defaults: std::rc::Rc::from([]),
            readonly_props: vec![],
            methods: vec![],
            abstract_methods: vec![],
            enum_backing_error: None,
            deferred_instance_defaults: None,
            class_id: 0,
        })
        .unwrap();
    }

    // TypeError extends Error
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "TypeError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // CompileError extends Error
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "CompileError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // ParseError extends CompileError
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ParseError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("CompileError".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // ArgumentCountError extends TypeError
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ArgumentCountError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("TypeError".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();

    // UnhandledMatchError extends Error
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "UnhandledMatchError".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: Some("Error".to_string()),
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
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
            "ErrorException",
            "Error",
            "ArithmeticError",
            "DivisionByZeroError",
            "AssertionError",
            "TypeError",
            "CompileError",
            "ParseError",
            "ArgumentCountError",
            "UnhandledMatchError",
        ]);
    for class in throwable_classes {
        // All explicit constructor parameters are optional.
        if class == "ErrorException" {
            reg_method!(
                class,
                "__construct",
                fn_error_exception_construct,
                7,
                0,
                "message",
                "code",
                "severity",
                "filename",
                "line",
                "previous"
            );
            let constructor = funcs
                .last_mut()
                .expect("ErrorException constructor was just registered");
            constructor.common.sig.param_type_hints = vec![
                ParamTypeHint::String,
                ParamTypeHint::Int,
                ParamTypeHint::Int,
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
                    "Throwable".to_string(),
                ))),
            ];
            constructor.handler_validates_types = true;
            let pointer = &constructor.common as *const FunctionCommon;
            eg.method_declaring_class
                .insert(pointer, "ErrorException".to_string());
            reg_method!(class, "getseverity", fn_error_exception_get_severity, 1, 0);
        } else {
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
            let constructor = funcs
                .last_mut()
                .expect("Throwable constructor was just registered");
            constructor.common.sig.param_type_hints = vec![
                ParamTypeHint::String,
                ParamTypeHint::Int,
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
                    "Throwable".to_string(),
                ))),
            ];
            constructor.handler_validates_types = true;
            let pointer = &constructor.common as *const FunctionCommon;
            let declaring_class = if eg.class_is_a(class, "Exception") {
                "Exception"
            } else if eg.class_is_a(class, "Error") {
                "Error"
            } else {
                class
            };
            eg.method_declaring_class
                .insert(pointer, declaring_class.to_string());
        }
        // getMessage: num_args=1 (CV 0=$this), required=0 (no explicit args)
        reg_method!(class, "getmessage", fn_throwable_get_message, 1, 0);
        reg_method!(class, "getcode", fn_throwable_get_code, 1, 0);
        reg_method!(class, "getprevious", fn_throwable_get_previous, 1, 0);
        reg_method!(class, "getfile", fn_throwable_get_file, 1, 0);
        reg_method!(class, "getline", fn_throwable_get_line, 1, 0);
        reg_method!(class, "gettrace", fn_throwable_get_trace, 1, 0);
        reg_method!(class, "__tostring", fn_throwable_to_string, 1, 0);
        reg_method!(
            class,
            "gettraceasstring",
            fn_throwable_get_trace_as_string,
            1,
            0
        );
    }

    funcs.extend(super::fiber::register(eg));
    funcs.extend(reflection::register(eg));

    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: SENSITIVE_PARAMETER_VALUE_CLASS.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![PropertyDefinition::declared(
            "value".to_string(),
            None,
            Visibility::Private,
            SENSITIVE_PARAMETER_VALUE_CLASS.to_string(),
            ParamTypeHint::Mixed,
            true,
            false,
        )],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec!["value".to_string()],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();
    reg_method!(
        SENSITIVE_PARAMETER_VALUE_CLASS,
        "__construct",
        fn_sensitive_parameter_value_construct,
        2,
        1,
        "value"
    );
    funcs
        .last_mut()
        .expect("SensitiveParameterValue constructor was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Mixed];
    reg_method!(
        SENSITIVE_PARAMETER_VALUE_CLASS,
        "getValue",
        fn_sensitive_parameter_value_get_value,
        1,
        0
    );
    funcs
        .last_mut()
        .expect("SensitiveParameterValue::getValue was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Mixed;
    reg_method!(
        SENSITIVE_PARAMETER_VALUE_CLASS,
        "__debugInfo",
        fn_sensitive_parameter_value_debug_info,
        1,
        0
    );
    funcs
        .last_mut()
        .expect("SensitiveParameterValue::__debugInfo was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;

    funcs.extend(register_value_error(eg));

    let empty_internal_type =
        |name: &str, implements: Vec<String>, is_interface: bool, is_final: bool| ClassDef {
            attributes: Vec::new(),
            name: name.to_string(),
            source_file: None,
            declaration_line: 0,
            parent: None,
            implements,
            is_interface,
            is_abstract: false,
            is_final,
            is_trait: false,
            is_enum: false,
            is_readonly: false,
            allow_dynamic_properties: name.eq_ignore_ascii_case("stdClass"),
            uses: vec![],
            trait_aliases: vec![],
            trait_precedences: vec![],
            properties: vec![],
            static_properties: vec![],
            constants: vec![],
            property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
            property_defaults: std::rc::Rc::from([]),
            readonly_props: vec![],
            methods: vec![],
            abstract_methods: vec![],
            enum_backing_error: None,
            deferred_instance_defaults: None,
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
    reg_static_method!(
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
    reg_static_method!(
        "Closure",
        "fromCallable",
        fn_closure_from_callable,
        2,
        1,
        "callback"
    );
    let mut closure_call = Box::new(make_internal_method_variadic(
        fn_closure_call,
        1,
        vec!["newThis".to_string(), "args".to_string()],
    ));
    closure_call.common.sig.ref_args = u64::MAX << 1;
    closure_call.common.sig.prefer_ref_args = u64::MAX << 1;
    let closure_call_ptr = &closure_call.common as *const FunctionCommon;
    eg.function_table
        .insert("closure::call".to_string(), closure_call_ptr);
    eg.method_declaring_class
        .insert(closure_call_ptr, "Closure".to_string());
    funcs.push(closure_call);
    let mut closure_invoke = Box::new(make_internal_method_variadic(
        fn_closure_invoke,
        0,
        vec!["args".to_string()],
    ));
    closure_invoke.common.sig.ref_args = u64::MAX;
    closure_invoke.common.sig.prefer_ref_args = u64::MAX;
    let closure_invoke_ptr = &closure_invoke.common as *const FunctionCommon;
    eg.function_table
        .insert("closure::__invoke".to_string(), closure_invoke_ptr);
    eg.method_declaring_class
        .insert(closure_invoke_ptr, "Closure".to_string());
    funcs.push(closure_invoke);

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
        ("UnitEnum", vec![]),
        ("BackedEnum", vec!["UnitEnum".to_string()]),
        ("SplObserver", vec![]),
        ("SplSubject", vec![]),
        ("SessionHandlerInterface", vec![]),
        ("SessionUpdateTimestampHandlerInterface", vec![]),
    ] {
        eg.register_class(empty_internal_type(name, parents, true, false))
            .unwrap();
    }
    funcs.push(register_rounding_mode(eg));
    funcs.extend(random::register(eg));
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
    funcs.extend(super::weak::register(eg));
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
            "storage".to_string(),
            Some(Value::array(PhpArray::new())),
            Visibility::Private,
            name.to_string(),
        ));
        class.constants = ["STD_PROP_LIST", "ARRAY_AS_PROPS"]
            .into_iter()
            .map(|constant| ClassConstantDefinition {
                attributes: Vec::new(),
                name: constant.to_string(),
                value: crate::builtin_class_constant(name, constant)
                    .expect("declared ArrayObject flag has a builtin value"),
                source_file: String::new(),
                evaluation_error: None,
                source_expression: None,
                callable_factory: None,
                evaluation_scope: None,
                value_is_deferred: false,
                visibility: Visibility::Public,
                declaring_class: name.to_string(),
                type_hint: ParamTypeHint::Int,
                is_final: false,
            })
            .collect();
        eg.register_class(class).unwrap();
        reg_method!(
            name,
            "__construct",
            fn_array_iterator_construct,
            2,
            0,
            "array"
        );
        reg_method!(name, "count", fn_array_iterator_count, 1, 0);
        reg_method!(name, "append", fn_array_object_append, 2, 1, "value");
        reg_method!(name, "offsetGet", fn_array_object_offset_get, 2, 1, "key");
        reg_method!(
            name,
            "offsetSet",
            fn_array_object_offset_set,
            3,
            2,
            "key",
            "value"
        );
        reg_method!(
            name,
            "offsetExists",
            fn_array_object_offset_exists,
            2,
            1,
            "key"
        );
        reg_method!(
            name,
            "offsetUnset",
            fn_array_object_offset_unset,
            2,
            1,
            "key"
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
        attributes: Vec::new(),
        name: name.to_string(),
        value: Value::long(value),
        source_file: String::new(),
        evaluation_error: None,
        source_expression: None,
        callable_factory: None,
        evaluation_scope: None,
        value_is_deferred: false,
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
    reg_method!("Generator", "throw", fn_generator_throw, 2, 1, "exception");
    reg_method!("Generator", "getreturn", fn_generator_get_return, 1, 0);

    funcs
}
