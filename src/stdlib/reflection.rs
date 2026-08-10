//! Generic Reflection built-ins.
//!
//! Reflection consumes the permanent interned generic graph. Keeping these
//! cold handlers outside the main stdlib unit prevents metadata-facing API
//! growth from obscuring unrelated built-ins or entering their hot paths.

use crate::compiler::compile::ClassDef;
use crate::compiler::make_internal_method;
use crate::generics::{GenericDeclarationKind, GenericRuntimeCapabilities, GenericVariance};
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction};

fn argument<'a>(ed: *mut ExecuteData, index: u32) -> &'a Value {
    let value = unsafe { (*ed).cv(index) };
    if value.is_reference() {
        unsafe { &*value.as_ref_ptr() }
    } else {
        value
    }
}

fn argument_string(ed: *mut ExecuteData, index: u32) -> String {
    let value = argument(ed, index);
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.echo_to_string())
}

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    if !rv.is_null() {
        unsafe { rv.write(value) };
    }
    Ok(())
}

fn set_target(ed: *mut ExecuteData, kind: &str, owner: String) {
    if let Some(mut object) = argument(ed, 0).as_object_mut() {
        object.set_property("__generic_kind", Value::string(kind));
        object.set_property("__generic_owner", Value::string(owner));
    }
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
    let target = argument(ed, 1).clone();
    let owner = target
        .as_object()
        .map(|object| object.class_name.to_string())
        .ok_or_else(|| VmError::Fatal("ReflectionObject expects an object".into()))?;
    set_target(ed, "class", owner);
    if let Some(mut object) = argument(ed, 0).as_object_mut() {
        object.set_property("__generic_object", target);
    }
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
    let object = argument(ed, 0).as_object()?;
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
    let target = argument(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("__generic_object").cloned());
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

    for class in [
        "ReflectionFunction",
        "ReflectionClass",
        "ReflectionObject",
        "ReflectionMethod",
    ] {
        eg.register_class(ClassDef {
            name: class.to_string(),
            parent: None,
            implements: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
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

    functions
}
