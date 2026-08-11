//! Generic Reflection built-ins.
//!
//! Reflection consumes the permanent interned generic graph. Keeping these
//! cold handlers outside the main stdlib unit prevents metadata-facing API
//! growth from obscuring unrelated built-ins or entering their hot paths.

mod ancestry;
mod functions;
mod generic_parameters;
mod registry;

use std::collections::HashMap;

use ancestry::reflected_arguments;
use functions::reflection_function_target;

use crate::generics::{GenericDeclarationKind, GenericRuntimeCapabilities};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, PhpObject, Value, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

pub(super) use registry::register;

fn with_argument<R>(ed: *mut ExecuteData, index: u32, visit: impl FnOnce(&Value) -> R) -> R {
    let value = unsafe { (*ed).cv(index) };
    let value = if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    };
    visit(value)
}

fn argument_string(ed: *mut ExecuteData, index: u32) -> String {
    with_argument(ed, index, |value| {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.echo_to_string())
    })
}

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    if !rv.is_null() {
        unsafe { rv.write(value) };
    }
    Ok(())
}

fn object_value(
    class_name: &str,
    properties: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let properties = properties
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect::<HashMap<_, _>>();
    Value::object(PhpObject::dynamic(class_name.to_string(), 0, properties))
}

fn named_reflected_type(name: &str) -> Value {
    object_value(
        "ReflectionNamedType",
        [
            ("__generic_name", Value::string(name)),
            ("__generic_arguments", Value::array(PhpArray::new())),
            ("__generic_string", Value::string(name)),
        ],
    )
}

fn reflected_property(ed: *mut ExecuteData, name: &str) -> Option<Value> {
    with_argument(ed, 0, |value| {
        value.as_object()?.get_property(name).cloned()
    })
}

fn reflection_type_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_name").unwrap_or_else(|| Value::string("")),
    )
}

fn reflection_type_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_string").unwrap_or_else(|| Value::string("")),
    )
}

fn reflection_type_has_generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found = reflected_property(ed, "__generic_arguments")
        .and_then(|arguments| arguments.as_array().map(|arguments| !arguments.is_empty()))
        .unwrap_or(false);
    return_value(rv, Value::bool(found))
}

fn reflection_type_generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_arguments")
            .unwrap_or_else(|| Value::array(PhpArray::new())),
    )
}

fn reflection_compound_types(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__generic_types").unwrap_or_else(|| Value::array(PhpArray::new())),
    )
}

fn reflection_exception(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(make_error_value("ReflectionException", message.as_ref()));
}

fn set_target(ed: *mut ExecuteData, kind: &str, owner: String) {
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__generic_kind", Value::string(kind));
            object.set_property("__generic_owner", Value::string(owner));
        }
    });
}

fn function_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (kind, owner) = with_argument(ed, 1, reflection_function_target)?;
    set_target(ed, kind, owner);
    Ok(())
}

fn class_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_target(ed, "class", argument_string(ed, 1));
    Ok(())
}

fn object_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let owner = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .ok_or_else(|| VmError::Fatal("ReflectionObject expects an object".into()))?;
    set_target(ed, "class", owner);
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__generic_object", target);
        }
    });
    Ok(())
}

fn method_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let owner = format!("{}::{}", argument_string(ed, 1), argument_string(ed, 2));
    set_target(ed, "method", owner);
    Ok(())
}

fn generic_target(ed: *mut ExecuteData) -> Option<(GenericDeclarationKind, String)> {
    with_argument(ed, 0, |value| {
        let object = value.as_object()?;
        let kind = match object.get_property("__generic_kind")?.as_str()? {
            "function" => GenericDeclarationKind::Function,
            "closure" => GenericDeclarationKind::Closure,
            "class" => GenericDeclarationKind::Class,
            "method" => GenericDeclarationKind::Method,
            _ => return None,
        };
        let owner = object
            .get_property("__generic_owner")?
            .as_str()?
            .to_string();
        Some((kind, owner))
    })
}

fn generic_runtime_modes(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let capabilities = GenericRuntimeCapabilities::CONFIGURED;
    let mut modes = PhpArray::with_packed_capacity(
        capabilities.erased as usize + capabilities.reified as usize,
    );
    if capabilities.erased {
        modes.push(Value::string("bound-erased"));
    }
    if capabilities.reified {
        modes.push(Value::string("reified"));
    }
    return_value(rv, Value::array(modes))
}

fn generic_arguments(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 0, |value| {
        value
            .as_object()
            .and_then(|object| object.get_property("__generic_object").cloned())
    });
    #[cfg(feature = "php-generics-reified")]
    let arguments = if let Some(binding) = target
        .as_ref()
        .and_then(|target| eg.reified_object_binding(target))
    {
        let declaration = eg.generic_metadata.declaration(binding);
        eg.generic_metadata
            .reflection_reified_binding(binding)
            .map_or_else(PhpArray::new, |binding| {
                reflected_arguments(&eg.generic_metadata, declaration, &binding)
            })
    } else {
        PhpArray::new()
    };
    #[cfg(not(feature = "php-generics-reified"))]
    let arguments = {
        let _ = (target, eg);
        PhpArray::new()
    };
    return_value(rv, Value::array(arguments))
}
