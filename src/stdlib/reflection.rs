//! Generic Reflection built-ins.
//!
//! Reflection consumes the permanent interned generic graph. Keeping these
//! cold handlers outside the main stdlib unit prevents metadata-facing API
//! growth from obscuring unrelated built-ins or entering their hot paths.

mod ancestry;
mod functions;
mod generic_parameters;
mod registry;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ancestry::reflected_arguments;
use functions::reflection_function_target;

use crate::compiler::compile::PropertyDefinition;
use crate::generics::{GenericDeclarationKind, GenericRuntimeCapabilities};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, PhpClosure, PhpObject, Value, ValueType, make_error_value};
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

fn reflected_signature_type(hint: &ParamTypeHint) -> Value {
    match hint {
        ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => {
            let mut types = PhpArray::with_packed_capacity(parts.len());
            for part in parts {
                types.push(reflected_signature_type(part));
            }
            object_value(
                if matches!(hint, ParamTypeHint::Union(_)) {
                    "ReflectionUnionType"
                } else {
                    "ReflectionIntersectionType"
                },
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(hint.display_name())),
                    ("__reflection_allows_null", Value::bool(false)),
                ],
            )
        }
        ParamTypeHint::Nullable(inner) => object_value(
            "ReflectionNamedType",
            [
                ("__generic_name", Value::string(inner.display_name())),
                ("__generic_arguments", Value::array(PhpArray::new())),
                ("__generic_string", Value::string(hint.display_name())),
                ("__reflection_allows_null", Value::bool(true)),
            ],
        ),
        _ => object_value(
            "ReflectionNamedType",
            [
                ("__generic_name", Value::string(hint.display_name())),
                ("__generic_arguments", Value::array(PhpArray::new())),
                ("__generic_string", Value::string(hint.display_name())),
                ("__reflection_allows_null", Value::bool(false)),
            ],
        ),
    }
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
                (
                    "__reflection_passed_by_reference",
                    Value::bool(function.sig.is_param_by_ref(index)),
                ),
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

fn function_get_number_of_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = reflected_function(ed).map_or(0, |function| {
        function.sig.public_arity() + u32::from(function.sig.is_variadic)
    });
    return_value(rv, Value::long(i64::from(count)))
}

fn function_get_number_of_required_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = reflected_function(ed).map_or(0, |function| function.sig.required_num_args);
    return_value(rv, Value::long(i64::from(count)))
}

fn function_returns_reference(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(reflected_function(ed).is_some_and(|function| function.sig.returns_reference)),
    )
}

fn function_is_closure(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let is_closure = reflected_property(ed, "__generic_kind")
        .and_then(|value| value.as_str().map(|kind| kind == "closure"))
        .unwrap_or(false);
    return_value(rv, Value::bool(is_closure))
}

fn function_has_return_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(
            reflected_function(ed).is_some_and(|function| {
                !matches!(function.sig.return_type_hint, ParamTypeHint::None)
            }),
        ),
    )
}

fn function_get_return_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        return return_value(rv, Value::null());
    };
    if matches!(function.sig.return_type_hint, ParamTypeHint::None) {
        return return_value(rv, Value::null());
    }
    return_value(rv, reflected_signature_type(&function.sig.return_type_hint))
}

fn function_has_tentative_return_type(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(false))
}

fn function_get_tentative_return_type(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::null())
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

fn parameter_has_type(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_has_type")),
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

fn parameter_is_passed_by_reference(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(
            ed,
            "__reflection_passed_by_reference",
        )),
    )
}

fn parameter_is_optional(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(
            parameter_property_bool(ed, "__reflection_has_default")
                || parameter_property_bool(ed, "__reflection_variadic"),
        ),
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

fn class_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "name").unwrap_or_else(|| Value::string("")),
    )
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

fn reflection_get_doc_comment(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // The frontend deliberately discards comments today. Returning false is
    // PHP's truthful "no retained doc comment" result; an empty string would
    // incorrectly claim that a doc comment exists.
    return_value(rv, Value::bool(false))
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

fn class_is_internal(
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
    return_value(rv, Value::bool(eg.class_is_internal(&owner)))
}

fn class_is_user_defined(
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
    return_value(rv, Value::bool(!eg.class_is_internal(&owner)))
}

fn class_is_subclass_of(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let target = with_argument(ed, 1, |value| {
        value
            .as_object()
            .and_then(|object| object.get_property("name").cloned())
            .and_then(|name| name.as_str().map(str::to_owned))
            .or_else(|| value.as_str().map(str::to_owned))
    });
    let Some(target) = target else {
        return return_value(rv, Value::bool(false));
    };
    if (eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?)
        || (eg.find_class(&target).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &target)?)
    {
        return return_value(rv, Value::bool(false));
    }
    let same_identity = eg
        .find_class(&owner)
        .zip(eg.find_class(&target))
        .is_some_and(|(owner, target)| std::ptr::eq(owner, target));
    return_value(
        rv,
        Value::bool(!same_identity && eg.class_is_a(&owner, &target)),
    )
}

fn class_implements_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let target = argument_string(ed, 1);
    if (eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?)
        || (eg.find_class(&target).is_none()
            && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &target)?)
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(
        rv,
        Value::bool(eg.class_is_interface(&target) && eg.class_is_a(&owner, &target)),
    )
}

fn collect_reflected_interface_names(
    eg: &ExecutorGlobals,
    owner: &str,
    names: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    let Some(class) = eg.find_class(owner) else {
        return;
    };
    let parent = class.parent.clone();
    let interfaces = class.implements.clone();

    // PHP reports interfaces inherited from the parent class first, then the
    // class's own declarations and each interface's extended ancestors.
    if let Some(parent) = parent {
        collect_reflected_interface_names(eg, &parent, names, seen);
    }
    for interface in interfaces {
        let canonical = eg
            .find_class(&interface)
            .map_or(interface, |class| class.name.clone());
        if seen.insert(canonical.to_ascii_lowercase()) {
            names.push(canonical.clone());
            collect_reflected_interface_names(eg, &canonical, names, seen);
        }
    }
}

fn class_get_interface_names(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let mut names = Vec::new();
    collect_reflected_interface_names(eg, &owner, &mut names, &mut HashSet::new());
    let mut result = PhpArray::with_packed_capacity(names.len());
    for name in names {
        result.push(Value::string(name));
    }
    return_value(rv, Value::array(result))
}

fn reflected_class_map(names: impl IntoIterator<Item = String>) -> Value {
    let mut result = PhpArray::new();
    for name in names {
        result.set_str(
            &name,
            object_value(
                "ReflectionClass",
                [
                    ("__generic_kind", Value::string("class")),
                    ("__generic_owner", Value::string(name.clone())),
                    ("name", Value::string(name.clone())),
                ],
            ),
        );
    }
    Value::array(result)
}

fn class_get_interfaces(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let mut names = Vec::new();
    collect_reflected_interface_names(eg, &owner, &mut names, &mut HashSet::new());
    return_value(rv, reflected_class_map(names))
}

fn class_get_trait_names(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut result = PhpArray::with_packed_capacity(class.uses.len());
    for name in &class.uses {
        result.push(Value::string(name.clone()));
    }
    return_value(rv, Value::array(result))
}

fn class_get_traits(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let names = eg
        .find_class(&owner)
        .map(|class| class.uses.clone())
        .unwrap_or_default();
    return_value(rv, reflected_class_map(names))
}

fn class_get_constants(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let filter = with_argument(ed, 1, Value::as_long);
    let mut constants = PhpArray::with_hash_capacity(class.constants.len());
    for constant in &class.constants {
        let visibility = match constant.visibility {
            Visibility::Public => 1,
            Visibility::Protected => 2,
            Visibility::Private => 4,
        };
        let modifiers = visibility | if constant.is_final { 32 } else { 0 };
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        constants.set_str(&constant.name, constant.value.clone());
    }
    return_value(rv, Value::array(constants))
}

fn class_get_reflection_constants(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let filter = with_argument(ed, 1, Value::as_long);
    let mut constants = PhpArray::with_packed_capacity(class.constants.len());
    for constant in &class.constants {
        let visibility = match constant.visibility {
            Visibility::Public => 1,
            Visibility::Protected => 2,
            Visibility::Private => 4,
        };
        let modifiers = visibility | if constant.is_final { 32 } else { 0 };
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        constants.push(object_value(
            "ReflectionClassConstant",
            [
                ("name", Value::string(constant.name.clone())),
                ("class", Value::string(constant.declaring_class.clone())),
                ("__reflection_modifiers", Value::long(modifiers)),
                ("__reflection_value", constant.value.clone()),
            ],
        ));
    }
    return_value(rv, Value::array(constants))
}

fn class_get_default_properties(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut defaults =
        PhpArray::with_hash_capacity(class.properties.len() + class.static_properties.len());
    for property in class
        .static_properties
        .iter()
        .chain(class.properties.iter())
    {
        if let Some(default) = &property.default {
            defaults.set_str(&property.name, default.clone());
        }
    }
    return_value(rv, Value::array(defaults))
}

fn method_modifiers(visibility: Visibility, is_static: bool, is_final: bool) -> i64 {
    let visibility = match visibility {
        Visibility::Public => 1,
        Visibility::Protected => 2,
        Visibility::Private => 4,
    };
    visibility | if is_static { 16 } else { 0 } | if is_final { 32 } else { 0 }
}

fn collect_reflected_methods(
    eg: &ExecutorGlobals,
    owner: &str,
    methods: &mut Vec<(
        String,
        Visibility,
        bool,
        bool,
        *const FunctionCommon,
        String,
    )>,
    seen: &mut HashSet<String>,
) {
    let Some(class) = eg.find_class(owner) else {
        return;
    };
    for (name, visibility, is_static, is_final, function) in &class.methods {
        if class.method_is_abstract(name) || !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }
        methods.push((
            name.clone(),
            *visibility,
            *is_static,
            *is_final,
            &function.common,
            class.name.clone(),
        ));
    }
    for trait_name in &class.uses {
        let Some(trait_class) = eg.find_class(trait_name) else {
            continue;
        };
        for (name, visibility, is_static, is_final, function) in &trait_class.methods {
            if trait_class.method_is_abstract(name) || !seen.insert(name.to_ascii_lowercase()) {
                continue;
            }
            methods.push((
                name.clone(),
                *visibility,
                *is_static,
                *is_final,
                &function.common,
                class.name.clone(),
            ));
        }
    }
    if let Some(parent) = class.parent.clone() {
        collect_reflected_methods(eg, &parent, methods, seen);
    }
}

fn reflected_method_value(
    name: String,
    visibility: Visibility,
    is_static: bool,
    is_final: bool,
    function: *const FunctionCommon,
    declaring_class: String,
) -> Value {
    object_value(
        "ReflectionMethod",
        [
            ("name", Value::string(name)),
            ("class", Value::string(declaring_class.clone())),
            (
                "__reflection_declaring_class",
                Value::string(declaring_class.clone()),
            ),
            ("__reflection_method_class", Value::string(declaring_class)),
            ("__reflection_method_static", Value::bool(is_static)),
            ("__reflection_method_final", Value::bool(is_final)),
            (
                "__reflection_method_visibility",
                Value::long(match visibility {
                    Visibility::Public => 1,
                    Visibility::Protected => 2,
                    Visibility::Private => 4,
                }),
            ),
            (
                "__reflection_function_pointer",
                Value::long(function as usize as i64),
            ),
        ],
    )
}

fn class_get_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::null());
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::null());
    }
    let mut methods = Vec::new();
    collect_reflected_methods(eg, &owner, &mut methods, &mut HashSet::new());
    let Some((name, visibility, is_static, is_final, function, declaring_class)) = methods
        .into_iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case("__construct"))
    else {
        return return_value(rv, Value::null());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn find_reflected_method(
    eg: &ExecutorGlobals,
    owner: &str,
    method_name: &str,
) -> Option<(
    String,
    Visibility,
    bool,
    bool,
    *const FunctionCommon,
    String,
)> {
    let mut methods = Vec::new();
    collect_reflected_methods(eg, owner, &mut methods, &mut HashSet::new());
    methods
        .into_iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case(method_name))
        .or_else(|| {
            let function = eg.find_function(&format!("{owner}::{method_name}"))?;
            let declaring_class = eg.declaring_class_of(function).unwrap_or(owner).to_string();
            Some((
                method_name.to_string(),
                Visibility::Public,
                unsafe { (*function).sig.this_offset == 0 },
                false,
                function,
                declaring_class,
            ))
        })
}

fn class_has_method(
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
    let method_name = argument_string(ed, 1);
    return_value(
        rv,
        Value::bool(find_reflected_method(eg, &owner, &method_name).is_some()),
    )
}

fn class_get_method(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        reflection_exception(eg, "ReflectionClass has no resolved class");
        return Ok(());
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        reflection_exception(eg, format!("Class \"{owner}\" does not exist"));
        return Ok(());
    }
    let method_name = argument_string(ed, 1);
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, &owner, &method_name)
    else {
        reflection_exception(
            eg,
            format!("Method {owner}::{method_name}() does not exist"),
        );
        return Ok(());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn class_kind_predicate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    predicate: impl FnOnce(&crate::compiler::compile::ClassDef) -> bool,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::bool(false));
    }
    return_value(
        rv,
        Value::bool(eg.find_class(&owner).is_some_and(predicate)),
    )
}

fn class_is_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_interface)
}

fn class_is_trait(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_trait)
}

fn class_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_abstract || class.is_interface)
}

fn class_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_final)
}

fn class_is_readonly(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| class.is_readonly)
}

fn class_is_instantiable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    class_kind_predicate(ed, rv, eg, |class| {
        !class.is_interface && !class.is_trait && !class.is_abstract
    })
}

fn class_get_methods(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let filter = with_argument(ed, 1, Value::as_long);
    let mut methods = Vec::new();
    collect_reflected_methods(eg, &owner, &mut methods, &mut HashSet::new());
    let mut result = PhpArray::with_packed_capacity(methods.len());
    for (name, visibility, is_static, is_final, function, declaring_class) in methods {
        let modifiers = method_modifiers(visibility, is_static, is_final);
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        result.push(reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ));
    }
    return_value(rv, Value::array(result))
}

fn method_get_modifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let visibility = reflected_property(ed, "__reflection_method_visibility")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    let modifiers = visibility
        | if parameter_property_bool(ed, "__reflection_method_static") {
            16
        } else {
            0
        }
        | if parameter_property_bool(ed, "__reflection_method_final") {
            32
        } else {
            0
        };
    return_value(rv, Value::long(modifiers))
}

fn method_is_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let constructor = reflected_property(ed, "name")
        .and_then(|value| {
            value
                .as_str()
                .map(|name| name.eq_ignore_ascii_case("__construct"))
        })
        .unwrap_or(false);
    return_value(rv, Value::bool(constructor))
}

fn method_name_is(ed: *mut ExecuteData, expected: &str) -> bool {
    reflected_property(ed, "name")
        .and_then(|value| {
            value
                .as_str()
                .map(|name| name.eq_ignore_ascii_case(expected))
        })
        .unwrap_or(false)
}

fn method_is_destructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(method_name_is(ed, "__destruct")))
}

fn method_visibility_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    expected: i64,
) -> Result<(), VmError> {
    let visibility = reflected_property(ed, "__reflection_method_visibility")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    return_value(rv, Value::bool(visibility == expected))
}

fn method_is_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 1)
}

fn method_is_protected(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 2)
}

fn method_is_private(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    method_visibility_is(ed, rv, 4)
}

fn method_is_static(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_static")),
    )
}

fn method_is_final(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_final")),
    )
}

fn method_is_abstract(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        Value::bool(parameter_property_bool(ed, "__reflection_method_abstract")),
    )
}

fn method_has_prototype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(find_method_prototype(ed, eg).is_some()))
}

fn find_method_prototype(
    ed: *mut ExecuteData,
    eg: &ExecutorGlobals,
) -> Option<(
    String,
    Visibility,
    bool,
    bool,
    *const FunctionCommon,
    String,
)> {
    let class_name = reflected_property(ed, "__reflection_method_class")?
        .as_str()?
        .to_string();
    let method_name = reflected_property(ed, "name")?.as_str()?.to_string();
    let class = eg.find_class(&class_name)?;
    if let Some(parent) = &class.parent {
        let mut methods = Vec::new();
        collect_reflected_methods(eg, parent, &mut methods, &mut HashSet::new());
        if let Some(method) = methods
            .into_iter()
            .find(|(name, ..)| name.eq_ignore_ascii_case(&method_name))
        {
            return Some(method);
        }
    }
    for interface in &class.implements {
        let mut methods = Vec::new();
        collect_reflected_methods(eg, interface, &mut methods, &mut HashSet::new());
        if let Some(method) = methods
            .into_iter()
            .find(|(name, ..)| name.eq_ignore_ascii_case(&method_name))
        {
            return Some(method);
        }
    }
    None
}

fn method_get_prototype(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((name, visibility, is_static, is_final, function, declaring_class)) =
        find_method_prototype(ed, eg)
    else {
        reflection_exception(eg, "Method does not have a prototype");
        return Ok(());
    };
    return_value(
        rv,
        reflected_method_value(
            name,
            visibility,
            is_static,
            is_final,
            function,
            declaring_class,
        ),
    )
}

fn method_invoke(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionMethod has no resolved method");
        return Ok(());
    };
    let receiver = with_argument(ed, 1, Clone::clone);
    let arguments = with_argument(ed, 2, |value| {
        if let Some(values) = value.as_array() {
            values.values().cloned().collect::<Vec<_>>()
        } else if matches!(value.value_type(), ValueType::Undef) {
            Vec::new()
        } else {
            vec![value.clone()]
        }
    });
    let mut call_arguments = Vec::with_capacity(arguments.len() + 1);
    if function.sig.this_offset == 1 {
        call_arguments.push(receiver);
    }
    call_arguments.extend(arguments);
    let result = crate::vm::execute::call_function(eg, function, &call_arguments)?;
    return_value(rv, result)
}

fn property_modifiers(property: &PropertyDefinition, is_static: bool) -> i64 {
    let visibility = match property.visibility {
        Visibility::Public => 1,
        Visibility::Protected => 2,
        Visibility::Private => 4,
    };
    visibility | if is_static { 16 } else { 0 } | if property.is_readonly { 128 } else { 0 }
}

fn reflected_property_value(property: &PropertyDefinition, is_static: bool) -> Value {
    let declaring_class = property.declaring_class.clone();
    object_value(
        "ReflectionProperty",
        [
            (
                "__reflection_target",
                Value::string(declaring_class.clone()),
            ),
            (
                "__reflection_property",
                Value::string(property.name.clone()),
            ),
            (
                "__reflection_modifiers",
                Value::long(property_modifiers(property, is_static)),
            ),
            ("name", Value::string(property.name.clone())),
            ("class", Value::string(declaring_class)),
        ],
    )
}

fn class_get_properties(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    if eg.find_class(&owner).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &owner)?
    {
        return return_value(rv, Value::array(PhpArray::new()));
    }
    let Some(class) = eg.find_class(&owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let filter = with_argument(ed, 1, |value| value.as_long());
    let mut properties =
        PhpArray::with_packed_capacity(class.properties.len() + class.static_properties.len());
    for (property, is_static) in class
        .properties
        .iter()
        .map(|property| (property, false))
        .chain(
            class
                .static_properties
                .iter()
                .map(|property| (property, true)),
        )
    {
        if property.visibility == Visibility::Private
            && !property.declaring_class.eq_ignore_ascii_case(&class.name)
        {
            continue;
        }
        let modifiers = property_modifiers(property, is_static);
        if filter.is_some_and(|filter| modifiers & filter == 0) {
            continue;
        }
        properties.push(reflected_property_value(property, is_static));
    }
    return_value(rv, Value::array(properties))
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

fn class_new_instance_without_constructor(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((GenericDeclarationKind::Class, owner)) = generic_target(ed) else {
        return Err(VmError::Fatal(
            "ReflectionClass::newInstanceWithoutConstructor() requires a reflected class".into(),
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
    if class.is_interface || class.is_trait || class.is_abstract || class.is_enum {
        return Err(VmError::Fatal(format!(
            "Class {owner} cannot be instantiated without invoking its constructor"
        )));
    }
    let object = if class.class_id == 0 {
        PhpObject::dynamic(class.name.clone(), 0, HashMap::new())
    } else {
        PhpObject::with_layout(
            class.class_id,
            class.property_layout.clone(),
            class.property_defaults.as_ref().to_vec(),
        )
    };
    return_value(rv, Value::object(object))
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = with_argument(ed, 1, Clone::clone);
    let name = argument_string(ed, 2);
    let class_name = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .or_else(|| target.as_str().map(str::to_string));
    let metadata = class_name.as_deref().and_then(|class_name| {
        let class = eg.find_class(class_name)?;
        class
            .properties
            .iter()
            .find(|property| property.name == name)
            .map(|property| {
                (
                    property.declaring_class.clone(),
                    property_modifiers(property, false),
                )
            })
            .or_else(|| {
                class
                    .static_properties
                    .iter()
                    .find(|property| property.name == name)
                    .map(|property| {
                        (
                            property.declaring_class.clone(),
                            property_modifiers(property, true),
                        )
                    })
            })
    });
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property("__reflection_target", target);
            object.set_property("__reflection_property", Value::string(name.clone()));
            object.set_property("name", Value::string(name));
            if let Some((declaring_class, modifiers)) = metadata {
                object.set_property("class", Value::string(declaring_class));
                object.set_property("__reflection_modifiers", Value::long(modifiers));
            }
        }
    });
    Ok(())
}

fn property_get_modifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(
        rv,
        reflected_property(ed, "__reflection_modifiers").unwrap_or_else(|| Value::long(0)),
    )
}

fn property_is_static(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    return_value(rv, Value::bool(modifiers & 16 != 0))
}

fn property_is_readonly(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(0);
    return_value(rv, Value::bool(modifiers & 128 != 0))
}

fn property_is_default(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    return_value(rv, Value::bool(true))
}

fn property_visibility_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    expected: i64,
) -> Result<(), VmError> {
    let modifiers = reflected_property(ed, "__reflection_modifiers")
        .and_then(|value| value.as_long())
        .unwrap_or(1);
    return_value(rv, Value::bool(modifiers & 7 == expected))
}

fn property_is_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 1)
}

fn property_is_protected(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 2)
}

fn property_is_private(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    property_visibility_is(ed, rv, 4)
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

fn reflection_property_key(
    eg: &ExecutorGlobals,
    object: &PhpObject,
    reflected_scope: Option<&str>,
    property: &str,
) -> String {
    crate::runtime::resolve_property_key(
        eg,
        object.class_name.as_ref(),
        property,
        Some(reflected_scope.unwrap_or(object.class_name.as_ref())),
    )
}

fn reflected_property_scope(ed: *mut ExecuteData) -> Option<String> {
    reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned))
}

fn property_is_initialized(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((_, property)) = reflected_property_target(ed) else {
        return return_value(rv, Value::bool(false));
    };
    let reflected_scope = reflected_property_scope(ed);
    let initialized = reflected_property_object(ed, 1).is_some_and(|target| {
        let Some(object) = target.as_object() else {
            return false;
        };
        let key = reflection_property_key(eg, &object, reflected_scope.as_deref(), &property);
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
    let reflected_scope = reflected_property_scope(ed);
    let value = reflected_property_object(ed, 1)
        .and_then(|target| {
            let object = target.as_object()?;
            let key = reflection_property_key(eg, &object, reflected_scope.as_deref(), &property);
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
    let reflected_scope = reflected_property_scope(ed);
    let value = with_argument(ed, 2, Clone::clone);
    if let Some(target) = reflected_property_object(ed, 1)
        && let Some(mut object) = target.as_object_mut()
    {
        let key = reflection_property_key(eg, &object, reflected_scope.as_deref(), &property);
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

fn method_file_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name =
        reflected_property(ed, "class").and_then(|value| value.as_str().map(str::to_owned));
    let value = class_name
        .as_deref()
        .and_then(|class_name| eg.find_class(class_name))
        .and_then(|class| class.source_file.as_ref())
        .map_or_else(|| Value::bool(false), |file| Value::string(file.clone()));
    return_value(rv, value)
}

fn method_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class_name = with_argument(ed, 1, |value| {
        value
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| argument_string(ed, 1))
    });
    let method_name = argument_string(ed, 2);
    if eg.find_class(&class_name).is_none()
        && !crate::stdlib::autoload::ensure_symbol_loaded(eg, &class_name)?
    {
        reflection_exception(eg, format!("Class \"{class_name}\" does not exist"));
        return Ok(());
    }
    let Some((_, visibility, is_static, is_final, function, declaring_class)) =
        find_reflected_method(eg, &class_name, &method_name)
    else {
        reflection_exception(
            eg,
            format!("Method {class_name}::{method_name}() does not exist"),
        );
        return Ok(());
    };
    let owner = format!("{class_name}::{method_name}");
    set_target(ed, "method", owner);
    with_argument(ed, 0, |value| {
        if let Some(mut object) = value.as_object_mut() {
            object.set_property(
                "__reflection_function_pointer",
                Value::long(function as usize as i64),
            );
            object.set_property("__reflection_method_static", Value::bool(is_static));
            object.set_property("__reflection_method_final", Value::bool(is_final));
            object.set_property(
                "__reflection_method_visibility",
                Value::long(match visibility {
                    Visibility::Public => 1,
                    Visibility::Protected => 2,
                    Visibility::Private => 4,
                }),
            );
            object.set_property(
                "__reflection_method_class",
                Value::string(class_name.clone()),
            );
            object.set_property(
                "__reflection_declaring_class",
                Value::string(declaring_class),
            );
            object.set_property("name", Value::string(method_name));
        }
    });
    Ok(())
}

fn method_get_closure(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(function) = reflected_function(ed) else {
        reflection_exception(eg, "ReflectionMethod has no resolved method");
        return Ok(());
    };
    let is_static = parameter_property_bool(ed, "__reflection_method_static");
    let reflected_class = reflected_property(ed, "__reflection_method_class")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let object = with_argument(ed, 1, |value| {
        (!matches!(value.value_type(), ValueType::Undef | ValueType::Null)).then(|| value.clone())
    });
    if !is_static {
        let Some(instance) = object.as_ref().and_then(Value::as_object) else {
            eg.exception = Some(make_error_value(
                "ValueError",
                "ReflectionMethod::getClosure(): Argument #1 ($object) must be provided for non-static methods",
            ));
            return Ok(());
        };
        if !eg.class_is_a(instance.class_name.as_ref(), &reflected_class) {
            eg.exception = Some(make_error_value(
                "ReflectionException",
                "Given object is not an instance of the class this method was declared in",
            ));
            return Ok(());
        }
    }
    let called_scope_class_id = object
        .as_ref()
        .and_then(Value::as_object)
        .map(|object| object.class_id)
        .or_else(|| eg.find_class(&reflected_class).map(|class| class.class_id))
        .unwrap_or(0);
    return_value(
        rv,
        Value::closure(PhpClosure {
            func: function as *const FunctionCommon,
            called_scope_class_id,
            is_static,
            bound_this: (!is_static).then_some(object).flatten(),
            captures: vec![],
            has_heap_captures: false,
        }),
    )
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
