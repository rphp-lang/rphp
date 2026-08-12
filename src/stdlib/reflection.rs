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
use crate::vm::function::{FunctionCommon, ParamTypeHint};

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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let (kind, owner) = reflection_function_target(&target)?;
    let function = target
        .as_closure()
        .map(|closure| closure.func)
        .or_else(|| eg.find_function(&owner));
    let closure_this = target
        .as_closure()
        .and_then(|closure| closure.bound_this.clone())
        .unwrap_or_else(Value::null);
    let called_class = target.as_closure().and_then(|closure| {
        (closure.called_scope_class_id != 0)
            .then(|| eg.class_by_id(closure.called_scope_class_id))
            .flatten()
            .map(|class| class.name.clone())
            .or_else(|| {
                closure
                    .bound_this
                    .as_ref()
                    .and_then(Value::as_object)
                    .map(|object| object.class_name.to_string())
            })
    });
    set_target(ed, kind, owner.clone());
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property(
                "name",
                Value::string(
                    owner
                        .rsplit_once("::")
                        .map_or(owner.as_str(), |(_, name)| name),
                ),
            );
            object.set_property("__reflection_closure_this", closure_this);
            object.set_property(
                "__reflection_closure_called_class",
                called_class.map_or_else(Value::null, Value::string),
            );
            object.set_property(
                "__reflection_function_pointer",
                Value::long(function.map_or(0, |function| function as usize as i64)),
            );
        }
    });
    Ok(())
}

fn reflected_function(ed: *mut ExecuteData) -> Option<&'static FunctionCommon> {
    let pointer = reflected_property(ed, "__reflection_function_pointer")?.as_long()? as usize
        as *const FunctionCommon;
    // SAFETY: constructors store only registered FunctionCommon pointers;
    // ExecutorGlobals owns those allocations for the full request lifetime.
    (!pointer.is_null()).then(|| unsafe { &*pointer })
}

fn hint_metadata(hint: &ParamTypeHint) -> (&'static str, String, bool) {
    match hint {
        ParamTypeHint::Nullable(inner) => ("named", inner.display_name(), true),
        ParamTypeHint::Union(parts) => (
            "union",
            hint.display_name(),
            parts.iter().any(|part| {
                matches!(part, ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("null"))
            }),
        ),
        ParamTypeHint::Intersection(_) => ("intersection", hint.display_name(), false),
        _ => ("named", hint.display_name(), false),
    }
}

fn function_get_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let fixed = function.sig.public_arity();
    let count = fixed + u32::from(function.sig.is_variadic);
    let declaring_class = eg.declaring_class_of(function as *const FunctionCommon);
    let mut parameters = PhpArray::with_packed_capacity(count as usize);
    for index in 0..count {
        let name = function
            .sig
            .param_names
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("arg{}", index + 1));
        let hint = function
            .sig
            .param_type_hints
            .get(index as usize)
            .unwrap_or(&ParamTypeHint::None);
        let (type_kind, type_name, allows_null) = hint_metadata(hint);
        let has_type = !matches!(hint, ParamTypeHint::None);
        let is_variadic = function.sig.is_variadic && index == fixed;
        let has_default = !is_variadic && index >= function.sig.required_num_args;
        parameters.push(object_value(
            "ReflectionParameter",
            [
                ("name", Value::string(name)),
                ("__reflection_position", Value::long(index as i64)),
                ("__reflection_has_type", Value::bool(has_type)),
                ("__reflection_type_kind", Value::string(type_kind)),
                ("__reflection_type_name", Value::string(type_name)),
                ("__reflection_allows_null", Value::bool(allows_null)),
                ("__reflection_variadic", Value::bool(is_variadic)),
                ("__reflection_has_default", Value::bool(has_default)),
                (
                    "__reflection_declaring_class",
                    declaring_class.map_or_else(Value::null, Value::string),
                ),
            ],
        ));
    }
    return_value(rv, Value::array(parameters))
}

fn function_is_anonymous(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let anonymous = reflected_property(ed, "__generic_kind")
        .and_then(|kind| kind.as_str().map(|kind| kind == "closure"))
        .unwrap_or(false)
        && reflected_property(ed, "__generic_owner")
            .and_then(|owner| owner.as_str().map(|owner| owner.starts_with("__closure_")))
            .unwrap_or(false);
    return_value(rv, Value::bool(anonymous))
}

fn function_get_closure_this(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__reflection_closure_this").unwrap_or_else(Value::null),
    )
}

fn function_get_closure_called_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = reflected_property(ed, "__reflection_closure_called_class")
        .and_then(|name| name.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(name.clone())),
                ("name", Value::string(name)),
            ],
        ),
    )
}

fn parameter_property_bool(ed: *mut ExecuteData, name: &str) -> bool {
    reflected_property(ed, name)
        .map(|value| value.is_truthy())
        .unwrap_or(false)
}

fn parameter_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    )
}

fn parameter_get_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !parameter_property_bool(ed, "__reflection_has_type") {
        return return_value(rv, Value::null());
    }
    let kind = reflected_property(ed, "__reflection_type_kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "named".to_string());
    let name = reflected_property(ed, "__reflection_type_name")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let class = match kind.as_str() {
        "union" => "ReflectionUnionType",
        "intersection" => "ReflectionIntersectionType",
        _ => "ReflectionNamedType",
    };
    return_value(
        rv,
        object_value(
            class,
            [
                ("__generic_name", Value::string(name.clone())),
                ("__generic_string", Value::string(name)),
                (
                    "__reflection_allows_null",
                    Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
                ),
            ],
        ),
    )
}

fn parameter_is_variadic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_variadic")),
    )
}

fn parameter_is_default_available(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_has_default")),
    )
}

fn parameter_get_default_value(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Default expressions currently live in function bytecode rather than
    // signature metadata; null is the conservative reflected placeholder.
    return_value(rv, Value::null())
}

fn parameter_allows_null(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
    )
}

fn parameter_get_attributes(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::array(PhpArray::new()))
}

fn parameter_get_declaring_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) = reflected_property(ed, "__reflection_declaring_class")
        .and_then(|value| value.as_str().map(str::to_owned))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(name.clone())),
                ("name", Value::string(name)),
            ],
        ),
    )
}

fn reflection_type_is_builtin(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let builtin = reflected_property(ed, "__generic_name")
        .and_then(|value| value.as_str().map(str::to_ascii_lowercase))
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "int"
                    | "float"
                    | "string"
                    | "bool"
                    | "array"
                    | "callable"
                    | "iterable"
                    | "object"
                    | "mixed"
                    | "null"
                    | "false"
                    | "true"
            )
        });
    return_value(rv, Value::bool(builtin))
}

fn reflection_type_allows_null(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_allows_null")),
    )
}

fn class_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let owner = with_argument(ed, 1, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| argument_string(ed, 1))
    });
    set_target(ed, "class", owner.clone());
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("name", Value::string(owner));
        }
    });
    Ok(())
}

fn class_get_attributes(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Attribute syntax is accepted during S2 compilation, but metadata is not
    // retained yet. The truthful observable view is therefore an empty list.
    return_value(rv, Value::array(PhpArray::new()))
}

fn class_get_parent(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let Some(parent) = eg
        .find_class(&owner)
        .and_then(|class| class.parent.as_ref())
        .cloned()
    else {
        return return_value(rv, Value::bool(false));
    };
    return_value(
        rv,
        object_value(
            "ReflectionClass",
            [
                ("__generic_kind", Value::string("class")),
                ("__generic_owner", Value::string(parent.clone())),
                ("name", Value::string(parent)),
            ],
        ),
    )
}

fn class_new_lazy_ghost(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Err(VmError::Fatal(
            "ReflectionClass::newLazyGhost() requires a reflected class".into(),
        ));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return Err(VmError::Fatal(format!("Class {owner} does not exist")));
    }
    let class = eg
        .find_class(&owner)
        .ok_or_else(|| VmError::Fatal(format!("Class {owner} does not exist")))?;
    if class.is_interface || class.is_abstract || class.is_enum {
        return Err(VmError::Fatal(format!(
            "Class {owner} cannot be instantiated as a lazy ghost"
        )));
    }
    let object = if class.class_id == 0 {
        PhpObject::dynamic(owner, 0, HashMap::new())
    } else {
        PhpObject::with_layout(
            class.class_id,
            class.property_layout.clone(),
            class.property_defaults.as_ref().to_vec(),
        )
    };
    let ghost = Value::object(object);
    let initializer = with_argument(ed, 1, Clone::clone);
    let resolved = crate::stdlib::resolve_callback_at_callsite(&initializer, eg, ed)
        .ok_or_else(|| VmError::Fatal("Lazy ghost initializer must be callable".into()))?;
    crate::stdlib::call_resolved_with_values(eg, &resolved, std::slice::from_ref(&ghost))?;
    return_value(rv, ghost)
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

fn property_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let name = argument_string(ed, 2);
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__reflection_target", target);
            object.set_property("__reflection_property", Value::string(name));
        }
    });
    Ok(())
}

fn reflected_property_target(ed: *mut ExecuteData) -> Option<(Value, String)> {
    with_argument(ed, 0, |value| {
        let object = value.as_object()?;
        let target = object.get_property("__reflection_target")?.clone();
        let property = object
            .get_property("__reflection_property")?
            .as_str()?
            .to_string();
        Some((target, property))
    })
}

fn reflected_property_object(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    with_argument(ed, index, |value| {
        (value.value_type() == crate::value::ValueType::Object).then(|| value.clone())
    })
    .or_else(|| {
        reflected_property_target(ed).and_then(|(target, _)| {
            (target.value_type() == crate::value::ValueType::Object).then_some(target)
        })
    })
}

fn reflection_property_key(eg: &ExecutorGlobals, object: &PhpObject, property: &str) -> String {
    crate::runtime::resolve_property_key(
        eg,
        object.class_name.as_ref(),
        property,
        Some(object.class_name.as_ref()),
    )
}

fn property_is_initialized(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let initialized = reflected_property_object(ed, 1).is_some_and(|target| {
        let Some(object) = target.as_object() else {
            return false;
        };
        let key = reflection_property_key(eg, &object, &property);
        object
            .get_property(&key)
            .is_some_and(|value| !value.is_undef())
    });
    return_value(rv, Value::bool(initialized))
}

fn property_get_value(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::null());
    };
    let value = reflected_property_object(ed, 1)
        .and_then(|target| {
            let object = target.as_object()?;
            let key = reflection_property_key(eg, &object, &property);
            object.get_property(&key).cloned()
        })
        .unwrap_or_else(Value::null);
    return_value(rv, value)
}

fn property_set_value(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed) else {
        return Ok(());
    };
    let value = with_argument(ed, 2, Clone::clone);
    if let Some(target) = reflected_property_object(ed, 1)
        && let Some(mut object) = target.as_object_mut()
    {
        let key = reflection_property_key(eg, &object, &property);
        object.set_property(&key, value);
    }
    Ok(())
}

fn class_file_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    let value = eg
        .find_class(&owner)
        .and_then(|class| class.source_file.as_ref())
        .map_or_else(|| Value::bool(false), |file| Value::string(file.clone()));
    return_value(rv, value)
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
