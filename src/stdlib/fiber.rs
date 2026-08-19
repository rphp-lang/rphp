//! PHP 8.5 core Fiber and FiberError API.

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::{make_internal_method, make_internal_method_variadic};
use crate::runtime::ExecutorGlobals;
use crate::runtime::fiber::{FiberInput, FiberReturnState, FiberStatus};
use crate::value::{ObjectLayout, Value, ValueType, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction};

use super::{owned_argument, resolve_callback_at_callsite, write_return_value};

fn argument(execute_data: *mut ExecuteData, index: u32) -> Value {
    owned_argument(execute_data, index)
}

fn write_result(return_value: *mut Value, value: Value) {
    write_return_value(return_value, value);
}

fn fiber_error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(make_error_value("FiberError", message));
}

fn receiver_identity(execute_data: *mut ExecuteData) -> usize {
    argument(execute_data, 0)
        .object_identity()
        .expect("Fiber instance method requires an object receiver")
}

fn fiber_construct(
    execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = argument(execute_data, 0);
    let callback = argument(execute_data, 1);
    let Some(callback) = resolve_callback_at_callsite(&callback, eg, execute_data) else {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "Fiber::__construct(): Argument #1 ($callback) must be of type callable, {} given",
                callback.dereferenced().type_name()
            ),
        ));
        return Ok(());
    };
    if !callback.supports_suspended_root() {
        fiber_error(
            eg,
            "The selected Fiber callback cannot be suspended by this runtime",
        );
        return Ok(());
    }
    if !eg.register_fiber_object(&receiver, callback) {
        fiber_error(eg, "Cannot call constructor twice");
    }
    Ok(())
}

fn fiber_start(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = receiver_identity(execute_data);
    if eg.fiber_status(identity) != Some(FiberStatus::Created) {
        fiber_error(eg, "Cannot start a fiber that has already been started");
        return Ok(());
    }
    let arguments = argument(execute_data, 1)
        .as_array()
        .map(|array| array.values().cloned().collect())
        .unwrap_or_default();
    let outcome = eg.run_fiber(identity, FiberInput::Start(arguments), execute_data)?;
    write_result(return_value, outcome.value);
    if let Some(exception) = outcome.failure {
        eg.exception = Some(exception);
    }
    Ok(())
}

fn fiber_resume(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = receiver_identity(execute_data);
    if eg.fiber_status(identity) != Some(FiberStatus::Suspended) {
        fiber_error(eg, "Cannot resume a fiber that is not suspended");
        return Ok(());
    }
    let value = argument(execute_data, 1);
    let value = if value.value_type() == ValueType::Undef {
        Value::null()
    } else {
        value
    };
    let outcome = eg.run_fiber(identity, FiberInput::Resume(value), execute_data)?;
    write_result(return_value, outcome.value);
    if let Some(exception) = outcome.failure {
        eg.exception = Some(exception);
    }
    Ok(())
}

fn fiber_throw(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = receiver_identity(execute_data);
    if eg.fiber_status(identity) != Some(FiberStatus::Suspended) {
        fiber_error(eg, "Cannot resume a fiber that is not suspended");
        return Ok(());
    }
    let exception = argument(execute_data, 1);
    let throwable = exception
        .as_object()
        .is_some_and(|object| eg.class_is_a(&object.class_name, "Throwable"));
    if !throwable {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "Fiber::throw(): Argument #1 ($exception) must be of type Throwable, {} given",
                exception.dereferenced().type_name()
            ),
        ));
        return Ok(());
    }
    let outcome = eg.run_fiber(identity, FiberInput::Throw(exception), execute_data)?;
    write_result(return_value, outcome.value);
    if let Some(exception) = outcome.failure {
        eg.exception = Some(exception);
    }
    Ok(())
}

fn fiber_get_return(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = receiver_identity(execute_data);
    match eg.fiber_returned(identity) {
        Ok(value) => write_result(return_value, value),
        Err(FiberReturnState::NotStarted) => fiber_error(
            eg,
            "Cannot get fiber return value: The fiber has not been started",
        ),
        Err(FiberReturnState::NotReturned) => fiber_error(
            eg,
            "Cannot get fiber return value: The fiber has not returned",
        ),
        Err(FiberReturnState::Threw) => fiber_error(
            eg,
            "Cannot get fiber return value: The fiber threw an exception",
        ),
        Err(FiberReturnState::Fatal) => fiber_error(
            eg,
            "Cannot get fiber return value: The fiber exited with a fatal error",
        ),
    }
    Ok(())
}

fn fiber_is_started(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let started = !matches!(
        eg.fiber_status(receiver_identity(execute_data)),
        None | Some(FiberStatus::Created)
    );
    write_result(return_value, Value::bool(started));
    Ok(())
}

fn fiber_is_running(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        Value::bool(eg.fiber_status(receiver_identity(execute_data)) == Some(FiberStatus::Running)),
    );
    Ok(())
}

fn fiber_is_suspended(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        Value::bool(
            eg.fiber_status(receiver_identity(execute_data)) == Some(FiberStatus::Suspended),
        ),
    );
    Ok(())
}

fn fiber_is_terminated(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        Value::bool(
            eg.fiber_status(receiver_identity(execute_data)) == Some(FiberStatus::Terminated),
        ),
    );
    Ok(())
}

fn fiber_get_current(
    _execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(return_value, eg.current_fiber().unwrap_or_else(Value::null));
    Ok(())
}

fn fiber_suspend(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !eg.has_active_fiber() {
        fiber_error(eg, "Cannot suspend outside of a fiber");
        return Ok(());
    }
    if eg.active_fiber_is_force_closing() {
        fiber_error(eg, "Cannot suspend in a force-closed fiber");
        return Ok(());
    }
    // Generator advancement currently owns a detached VM frame through a
    // synchronous Rust handler. Letting the Fiber unwind that handler would
    // retain a pointer to a frame the generator cleanup has already popped.
    // Reject this explicit follow-up boundary safely until generator
    // continuations participate in the shared suspended-call protocol.
    if eg.active_generator.is_some() {
        fiber_error(
            eg,
            "Suspending a fiber through a generator is not supported by this runtime",
        );
        return Ok(());
    }
    let value = argument(execute_data, 1);
    let value = if value.value_type() == ValueType::Undef {
        Value::null()
    } else {
        value
    };
    eg.suspend_fiber(execute_data, return_value, value)
}

fn fiber_error_construct(
    _execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(make_error_value(
        "Error",
        "The \"FiberError\" class is reserved for internal use and cannot be manually instantiated",
    ));
    Ok(())
}

fn internal_class(name: &str, parent: Option<&str>) -> ClassDef {
    ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: parent.map(str::to_string),
        implements: Vec::new(),
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: Vec::<PropertyDefinition>::new(),
        static_properties: Vec::new(),
        constants: Vec::new(),
        property_layout: std::rc::Rc::new(ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: Vec::new(),
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        class_id: 0,
    }
}

fn register_method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    class: &str,
    name: &str,
    function: InternalFunction,
) {
    let function = Box::new(function);
    let pointer = &function.common as *const FunctionCommon;
    eg.function_table
        .insert(format!("{class}::{name}").to_ascii_lowercase(), pointer);
    eg.method_declaring_class.insert(pointer, class.to_string());
    if class == "Fiber" && matches!(name, "getcurrent" | "suspend") {
        eg.register_internal_static_method(pointer);
    }
    let display_method = match name {
        "getreturn" => "getReturn",
        "isstarted" => "isStarted",
        "isrunning" => "isRunning",
        "issuspended" => "isSuspended",
        "isterminated" => "isTerminated",
        "getcurrent" => "getCurrent",
        other => other,
    };
    eg.register_internal_function_display_name(pointer, format!("{class}::{display_method}"));
    functions.push(function);
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.register_class(internal_class("Fiber", None)).unwrap();
    eg.register_class(internal_class("FiberError", Some("Error")))
        .unwrap();

    let mut functions = Vec::with_capacity(12);
    register_method(
        eg,
        &mut functions,
        "Fiber",
        "__construct",
        make_internal_method(fiber_construct, 2, 1, vec!["callback".to_string()]),
    );
    register_method(
        eg,
        &mut functions,
        "Fiber",
        "start",
        make_internal_method_variadic(fiber_start, 0, vec!["args".to_string()]),
    );
    for (name, handler, parameter) in [
        ("resume", fiber_resume as _, Some("value")),
        ("throw", fiber_throw as _, Some("exception")),
    ] {
        register_method(
            eg,
            &mut functions,
            "Fiber",
            name,
            make_internal_method(
                handler,
                2,
                u32::from(name == "throw"),
                vec![parameter.unwrap().to_string()],
            ),
        );
    }
    for (name, handler) in [
        ("getreturn", fiber_get_return as _),
        ("isstarted", fiber_is_started as _),
        ("isrunning", fiber_is_running as _),
        ("issuspended", fiber_is_suspended as _),
        ("isterminated", fiber_is_terminated as _),
    ] {
        register_method(
            eg,
            &mut functions,
            "Fiber",
            name,
            make_internal_method(handler, 1, 0, Vec::new()),
        );
    }
    register_method(
        eg,
        &mut functions,
        "Fiber",
        "getcurrent",
        make_internal_method(fiber_get_current, 1, 0, Vec::new()),
    );
    register_method(
        eg,
        &mut functions,
        "Fiber",
        "suspend",
        make_internal_method(fiber_suspend, 2, 0, vec!["value".to_string()]),
    );
    register_method(
        eg,
        &mut functions,
        "FiberError",
        "__construct",
        make_internal_method(fiber_error_construct, 1, 0, Vec::new()),
    );
    functions
}
