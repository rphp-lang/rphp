//! Registration of built-in Reflection classes and methods.
//!
//! This is cold, request-runtime setup. Keeping the declarative surface away
//! from the handlers makes it possible to audit class hierarchy, finality and
//! method exposure independently from metadata traversal.

use super::ancestry::{
    generic_arguments_for_parent_class, generic_arguments_for_parent_interface,
    generic_arguments_for_used_trait,
};
use super::generic_parameters::{
    generic_parameter_bound, generic_parameter_declaring_entity, generic_parameter_default,
    generic_parameter_has_bound, generic_parameter_has_default, generic_parameter_name,
    generic_parameter_position, generic_parameter_variance, generic_parameters,
    generic_variance_cases, is_generic, type_parameter_reference_parameter,
};
use super::{
    class_construct, function_construct, generic_arguments, generic_runtime_modes,
    method_construct, object_construct, reflection_compound_types,
    reflection_type_generic_arguments, reflection_type_has_generic_arguments, reflection_type_name,
    reflection_type_to_string,
};
use crate::compiler::compile::ClassDef;
use crate::compiler::make_internal_method;
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::vm::function::{FunctionCommon, InternalFunction};

fn register_reflection_class(
    eg: &mut ExecutorGlobals,
    name: &str,
    parent: Option<&str>,
    is_abstract: bool,
    is_final: bool,
) {
    register_reflection_class_kind(eg, name, parent, is_abstract, is_final, false, &[]);
}

fn register_reflection_class_with_interfaces(
    eg: &mut ExecutorGlobals,
    name: &str,
    parent: Option<&str>,
    is_abstract: bool,
    is_final: bool,
    implements: &[&str],
) {
    register_reflection_class_kind(eg, name, parent, is_abstract, is_final, false, implements);
}

fn register_reflection_class_kind(
    eg: &mut ExecutorGlobals,
    name: &str,
    parent: Option<&str>,
    is_abstract: bool,
    is_final: bool,
    is_enum: bool,
    implements: &[&str],
) {
    eg.register_class(ClassDef {
        name: name.to_string(),
        parent: parent.map(str::to_owned),
        implements: implements.iter().map(|name| (*name).to_string()).collect(),
        is_interface: false,
        is_abstract,
        is_final,
        is_trait: false,
        is_enum,
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

fn register_reflection_interface(eg: &mut ExecutorGlobals, name: &str) {
    eg.register_class(ClassDef {
        name: name.to_string(),
        parent: None,
        implements: vec![],
        is_interface: true,
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

fn register_generic_variance(eg: &mut ExecutorGlobals) {
    let cases = generic_variance_cases();
    let properties = ["Invariant", "Covariant", "Contravariant"]
        .into_iter()
        .zip(cases)
        .map(|(name, value)| {
            (
                name.to_string(),
                Some(value),
                Visibility::Public,
                "ReflectionGenericVariance".to_string(),
            )
        })
        .collect();
    eg.register_class(ClassDef {
        name: "ReflectionGenericVariance".to_string(),
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: true,
        uses: vec![],
        properties,
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        class_id: 0,
    })
    .unwrap();
}

pub(in crate::stdlib) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
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

    register_reflection_interface(eg, "Reflector");
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionFunctionAbstract",
        None,
        true,
        false,
        &["Reflector"],
    );
    for class in ["ReflectionFunction", "ReflectionMethod"] {
        register_reflection_class(eg, class, Some("ReflectionFunctionAbstract"), false, false);
    }
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionClass",
        None,
        false,
        false,
        &["Reflector"],
    );
    register_reflection_class(
        eg,
        "ReflectionObject",
        Some("ReflectionClass"),
        false,
        false,
    );
    register_reflection_class(eg, "ReflectionException", Some("Exception"), false, false);
    register_reflection_class(eg, "ReflectionType", None, true, false);
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionGenericTypeParameter",
        None,
        false,
        true,
        &["Reflector"],
    );
    register_generic_variance(eg);
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
        "ReflectionTypeParameterReference",
        "gettypeparameter",
        type_parameter_reference_parameter,
        1,
        0,
        []
    );
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
    register_method!(
        "ReflectionGenericTypeParameter",
        "getname",
        generic_parameter_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "getposition",
        generic_parameter_position,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "getvariance",
        generic_parameter_variance,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "hasbound",
        generic_parameter_has_bound,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "getbound",
        generic_parameter_bound,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "hasdefault",
        generic_parameter_has_default,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "getdefault",
        generic_parameter_default,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "getdeclaringentity",
        generic_parameter_declaring_entity,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionGenericTypeParameter",
        "__tostring",
        reflection_type_to_string,
        1,
        0,
        []
    );

    functions
}
