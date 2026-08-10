//! Generic Reflection built-ins.
//!
//! Reflection consumes the permanent interned generic graph. Keeping these
//! cold handlers outside the main stdlib unit prevents metadata-facing API
//! growth from obscuring unrelated built-ins or entering their hot paths.

use std::collections::HashMap;

use crate::compiler::compile::ClassDef;
use crate::compiler::make_internal_method;
use crate::generics::{
    GenericDeclaration, GenericDeclarationKind, GenericInheritanceKind, GenericMetadata,
    GenericReflectionBinding, GenericRuntimeCapabilities, GenericType, GenericVariance,
};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, PhpObject, Value, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction};

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

fn reflected_type(
    metadata: &GenericMetadata,
    declaration: &GenericDeclaration,
    value: &GenericType,
) -> Value {
    let rendered = metadata.format_type(declaration, value);
    match value {
        GenericType::Parameter(index) => {
            let name = declaration
                .parameters
                .get(*index as usize)
                .and_then(|parameter| metadata.symbol(parameter.name))
                .unwrap_or("?");
            object_value(
                "ReflectionTypeParameterReference",
                [
                    ("__generic_name", Value::string(name)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Union(parts) | GenericType::Intersection(parts) => {
            let mut types = PhpArray::with_packed_capacity(parts.len());
            for part in parts.iter() {
                types.push(reflected_type(metadata, declaration, part));
            }
            let class_name = if matches!(value, GenericType::Union(_)) {
                "ReflectionUnionType"
            } else {
                "ReflectionIntersectionType"
            };
            object_value(
                class_name,
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Named { name, arguments } => {
            let mut reflected_arguments = PhpArray::with_packed_capacity(arguments.len());
            for argument in arguments.iter() {
                reflected_arguments.push(reflected_type(metadata, declaration, argument));
            }
            object_value(
                "ReflectionNamedType",
                [
                    (
                        "__generic_name",
                        Value::string(metadata.symbol(*name).unwrap_or("?")),
                    ),
                    ("__generic_arguments", Value::array(reflected_arguments)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        GenericType::Nullable(inner) => {
            let mut types = PhpArray::with_packed_capacity(2);
            types.push(reflected_type(metadata, declaration, inner));
            types.push(named_reflected_type("null"));
            object_value(
                "ReflectionUnionType",
                [
                    ("__generic_types", Value::array(types)),
                    ("__generic_string", Value::string(rendered)),
                ],
            )
        }
        _ => named_reflected_type(&rendered),
    }
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

fn reflected_arguments(
    metadata: &GenericMetadata,
    declaration: Option<&GenericDeclaration>,
    binding: &GenericReflectionBinding,
) -> PhpArray {
    let Some(declaration) = declaration else {
        return PhpArray::new();
    };
    let mut arguments = PhpArray::with_packed_capacity(binding.arguments.len());
    for argument in binding.arguments.iter() {
        arguments.push(reflected_type(metadata, declaration, argument));
    }
    arguments
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
    set_target(ed, "function", argument_string(ed, 1));
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

fn reflected_class_owner(ed: *mut ExecuteData) -> Option<String> {
    generic_target(ed)
        .and_then(|(kind, owner)| (kind == GenericDeclarationKind::Class).then_some(owner))
}

fn generic_arguments_for_parent_class(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class");
        return Ok(());
    };
    if eg.class_is_interface(&owner) {
        reflection_exception(eg, format!("Interface {owner} has no parent class"));
        return Ok(());
    }
    let Some(binding) = eg.generic_metadata.reflection_direct_binding(
        &owner,
        GenericInheritanceKind::Extends,
        None,
    ) else {
        reflection_exception(eg, format!("Class {owner} has no parent class"));
        return Ok(());
    };
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let arguments = reflected_arguments(&eg.generic_metadata, context, &binding);
    return_value(rv, Value::array(arguments))
}

fn generic_arguments_for_parent_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class or interface");
        return Ok(());
    };
    let ancestor = argument_string(ed, 1);
    if owner.eq_ignore_ascii_case(&ancestor)
        || !eg.class_is_interface(&ancestor)
        || !eg.class_is_a(&owner, &ancestor)
    {
        reflection_exception(
            eg,
            format!("Interface {ancestor} is not an ancestor interface of {owner}"),
        );
        return Ok(());
    }
    let bindings = eg
        .generic_metadata
        .reflection_interface_bindings(&owner, &ancestor);
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let mut result = PhpArray::with_packed_capacity(bindings.len());
    for binding in bindings.iter() {
        result.push(Value::array(reflected_arguments(
            &eg.generic_metadata,
            context,
            binding,
        )));
    }
    return_value(rv, Value::array(result))
}

fn generic_arguments_for_used_trait(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(owner) = reflected_class_owner(ed) else {
        reflection_exception(eg, "Reflection target is not a class");
        return Ok(());
    };
    let trait_name = argument_string(ed, 1);
    let Some(binding) = eg.generic_metadata.reflection_direct_binding(
        &owner,
        GenericInheritanceKind::Uses,
        Some(&trait_name),
    ) else {
        reflection_exception(
            eg,
            format!("Trait {trait_name} is not directly used by {owner}"),
        );
        return Ok(());
    };
    let context = eg
        .generic_metadata
        .find_class_like_index(&owner)
        .and_then(|index| eg.generic_metadata.declarations().get(index as usize));
    let arguments = reflected_arguments(&eg.generic_metadata, context, &binding);
    return_value(rv, Value::array(arguments))
}

fn is_generic(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let found = generic_target(ed)
        .is_some_and(|(kind, owner)| eg.generic_metadata.find(kind, &owner).is_some());
    return_value(rv, Value::bool(found))
}

fn generic_parameters(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((kind, owner)) = generic_target(ed) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let Some(declaration) = eg.generic_metadata.find(kind, &owner) else {
        return return_value(rv, Value::array(PhpArray::new()));
    };
    let mut result = PhpArray::with_packed_capacity(declaration.parameters.len());
    for parameter in declaration.parameters.iter() {
        let mut reflected = PhpArray::with_hash_capacity(4);
        reflected.set_str(
            "name",
            Value::string(eg.generic_metadata.symbol(parameter.name).unwrap_or("?")),
        );
        let variance = match parameter.variance {
            GenericVariance::Invariant => "invariant",
            GenericVariance::Covariant => "covariant",
            GenericVariance::Contravariant => "contravariant",
        };
        reflected.set_str("variance", Value::string(variance));
        reflected.set_str(
            "bound",
            parameter.bound.as_ref().map_or_else(Value::null, |bound| {
                Value::string(eg.generic_metadata.format_type(declaration, bound))
            }),
        );
        reflected.set_str(
            "default",
            parameter
                .default
                .as_ref()
                .map_or_else(Value::null, |default| {
                    Value::string(eg.generic_metadata.format_type(declaration, default))
                }),
        );
        result.push(Value::array(reflected));
    }
    return_value(rv, Value::array(result))
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
        if let Some(rendered) = eg.generic_metadata.format_binding_arguments(binding) {
            let mut arguments = PhpArray::with_packed_capacity(rendered.len());
            for argument in rendered {
                arguments.push(Value::string(argument));
            }
            arguments
        } else {
            PhpArray::new()
        }
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

fn register_reflection_class(
    eg: &mut ExecutorGlobals,
    name: &str,
    parent: Option<&str>,
    is_abstract: bool,
    is_final: bool,
) {
    eg.register_class(ClassDef {
        name: name.to_string(),
        parent: parent.map(str::to_owned),
        implements: vec![],
        is_interface: false,
        is_abstract,
        is_final,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        properties: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    })
    .unwrap();
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::new();

    macro_rules! register_method {
        ($class:expr, $method:expr, $handler:expr, $num_args:expr, $min_args:expr, [$($name:expr),*]) => {{
            let function = Box::new(make_internal_method(
                $handler,
                $num_args,
                $min_args,
                vec![$($name.to_string()),*],
            ));
            let pointer = &function.common as *const FunctionCommon;
            eg.function_table.insert(
                format!("{}::{}", $class, $method).to_lowercase(),
                pointer,
            );
            eg.method_declaring_class
                .insert(pointer, $class.to_string());
            functions.push(function);
        }};
    }

    for class in ["ReflectionFunction", "ReflectionClass", "ReflectionMethod"] {
        register_reflection_class(eg, class, None, false, false);
    }
    register_reflection_class(
        eg,
        "ReflectionObject",
        Some("ReflectionClass"),
        false,
        false,
    );
    register_reflection_class(eg, "ReflectionException", Some("Exception"), false, false);
    register_reflection_class(eg, "ReflectionType", None, true, false);
    for class in [
        "ReflectionNamedType",
        "ReflectionUnionType",
        "ReflectionIntersectionType",
        "ReflectionTypeParameterReference",
    ] {
        register_reflection_class(eg, class, Some("ReflectionType"), false, true);
    }

    register_method!(
        "ReflectionFunction",
        "__construct",
        function_construct,
        2,
        1,
        ["name"]
    );
    register_method!(
        "ReflectionClass",
        "__construct",
        class_construct,
        2,
        1,
        ["name"]
    );
    register_method!(
        "ReflectionObject",
        "__construct",
        object_construct,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionMethod",
        "__construct",
        method_construct,
        3,
        2,
        ["class", "method"]
    );
    for class in [
        "ReflectionFunction",
        "ReflectionClass",
        "ReflectionObject",
        "ReflectionMethod",
    ] {
        register_method!(class, "isgeneric", is_generic, 1, 0, []);
        register_method!(class, "getgenericparameters", generic_parameters, 1, 0, []);
        register_method!(
            class,
            "getgenericruntimemodes",
            generic_runtime_modes,
            1,
            0,
            []
        );
    }
    register_method!(
        "ReflectionObject",
        "getgenericarguments",
        generic_arguments,
        1,
        0,
        []
    );
    for class in ["ReflectionClass", "ReflectionObject"] {
        register_method!(
            class,
            "getgenericargumentsforparentclass",
            generic_arguments_for_parent_class,
            1,
            0,
            []
        );
        register_method!(
            class,
            "getgenericargumentsforparentinterface",
            generic_arguments_for_parent_interface,
            2,
            1,
            ["name"]
        );
        register_method!(
            class,
            "getgenericargumentsforusedtrait",
            generic_arguments_for_used_trait,
            2,
            1,
            ["name"]
        );
    }
    for class in [
        "ReflectionNamedType",
        "ReflectionUnionType",
        "ReflectionIntersectionType",
        "ReflectionTypeParameterReference",
    ] {
        register_method!(class, "__tostring", reflection_type_to_string, 1, 0, []);
    }
    for class in ["ReflectionNamedType", "ReflectionTypeParameterReference"] {
        register_method!(class, "getname", reflection_type_name, 1, 0, []);
    }
    register_method!(
        "ReflectionNamedType",
        "hasgenericarguments",
        reflection_type_has_generic_arguments,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionNamedType",
        "getgenericarguments",
        reflection_type_generic_arguments,
        1,
        0,
        []
    );
    for class in ["ReflectionUnionType", "ReflectionIntersectionType"] {
        register_method!(class, "gettypes", reflection_compound_types, 1, 0, []);
    }

    functions
}
