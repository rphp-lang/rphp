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
    class_construct, class_file_name, class_get_attributes, class_get_constants,
    class_get_constructor, class_get_default_properties, class_get_interface_names,
    class_get_interfaces, class_get_method, class_get_methods, class_get_name, class_get_parent,
    class_get_properties, class_get_reflection_constants, class_get_trait_names, class_get_traits,
    class_has_method, class_implements_interface, class_is_abstract, class_is_final,
    class_is_instantiable, class_is_interface, class_is_internal, class_is_readonly,
    class_is_subclass_of, class_is_trait, class_is_user_defined,
    class_new_instance_without_constructor, class_new_lazy_ghost, function_construct,
    function_get_closure_called_class, function_get_closure_this,
    function_get_number_of_parameters, function_get_number_of_required_parameters,
    function_get_parameters, function_get_return_type, function_get_tentative_return_type,
    function_has_return_type, function_has_tentative_return_type, function_is_anonymous,
    function_is_closure, function_returns_reference, generic_arguments, generic_runtime_modes,
    method_construct, method_file_name, method_get_closure, method_get_modifiers,
    method_get_prototype, method_has_prototype, method_invoke, method_is_abstract,
    method_is_constructor, method_is_destructor, method_is_final, method_is_private,
    method_is_protected, method_is_public, method_is_static, object_construct,
    parameter_allows_null, parameter_get_attributes, parameter_get_declaring_class,
    parameter_get_default_value, parameter_get_name, parameter_get_type, parameter_has_type,
    parameter_is_default_available, parameter_is_optional, parameter_is_passed_by_reference,
    parameter_is_variadic, property_construct, property_get_modifiers, property_get_value,
    property_is_default, property_is_initialized, property_is_private, property_is_protected,
    property_is_public, property_is_readonly, property_is_static, property_set_value,
    reflection_compound_types, reflection_get_doc_comment, reflection_type_allows_null,
    reflection_type_generic_arguments, reflection_type_has_generic_arguments,
    reflection_type_is_builtin, reflection_type_name, reflection_type_to_string,
};
use crate::compiler::compile::{ClassConstantDefinition, ClassDef, PropertyDefinition};
use crate::compiler::make_internal_method;
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::Value;
use crate::vm::function::ParamTypeHint;
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
        source_file: None,
        parent: parent.map(str::to_owned),
        implements: implements.iter().map(|name| (*name).to_string()).collect(),
        is_interface: false,
        is_abstract,
        is_final,
        is_trait: false,
        is_enum,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: if name == "ReflectionMethod" {
            [
                ("IS_PUBLIC", 1),
                ("IS_PROTECTED", 2),
                ("IS_PRIVATE", 4),
                ("IS_STATIC", 16),
                ("IS_FINAL", 32),
                ("IS_ABSTRACT", 64),
            ]
            .into_iter()
            .map(|(constant, value)| ClassConstantDefinition {
                name: constant.to_string(),
                value: Value::long(value),
                evaluation_error: None,
                visibility: Visibility::Public,
                declaring_class: name.to_string(),
                type_hint: ParamTypeHint::Int,
                is_final: false,
            })
            .collect()
        } else {
            vec![]
        },
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();
}

fn register_reflection_interface(eg: &mut ExecutorGlobals, name: &str) {
    eg.register_class(ClassDef {
        name: name.to_string(),
        source_file: None,
        parent: None,
        implements: vec![],
        is_interface: true,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
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
            PropertyDefinition::new(
                name.to_string(),
                Some(value),
                Visibility::Public,
                "ReflectionGenericVariance".to_string(),
            )
        })
        .collect();
    eg.register_class(ClassDef {
        name: "ReflectionGenericVariance".to_string(),
        source_file: None,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: true,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![],
        static_properties: properties,
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
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
    eg.register_class(ClassDef {
        name: "ReflectionAttribute".to_string(),
        source_file: None,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![ClassConstantDefinition {
            name: "IS_INSTANCEOF".to_string(),
            value: Value::long(2),
            evaluation_error: None,
            visibility: Visibility::Public,
            declaring_class: "ReflectionAttribute".to_string(),
            type_hint: ParamTypeHint::Int,
            is_final: false,
        }],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();
    for class in ["ReflectionFunction", "ReflectionMethod"] {
        register_method!(class, "getname", parameter_get_name, 1, 0, []);
        register_reflection_class(eg, class, Some("ReflectionFunctionAbstract"), false, false);
    }
    register_method!(
        "ReflectionFunctionAbstract",
        "getdoccomment",
        reflection_get_doc_comment,
        1,
        0,
        []
    );
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
    eg.register_class(ClassDef {
        name: "ReflectionProperty".to_string(),
        source_file: None,
        parent: None,
        implements: vec!["Reflector".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        uses: vec![],
        trait_aliases: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: [
            ("IS_PUBLIC", 1),
            ("IS_PROTECTED", 2),
            ("IS_PRIVATE", 4),
            ("IS_STATIC", 16),
            ("IS_READONLY", 128),
        ]
        .into_iter()
        .map(|(name, value)| ClassConstantDefinition {
            name: name.to_string(),
            value: Value::long(value),
            evaluation_error: None,
            visibility: Visibility::Public,
            declaring_class: "ReflectionProperty".to_string(),
            type_hint: ParamTypeHint::Int,
            is_final: false,
        })
        .collect(),
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        class_id: 0,
    })
    .unwrap();
    register_reflection_class(eg, "ReflectionException", Some("Exception"), false, false);
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionClassConstant",
        None,
        false,
        false,
        &["Reflector"],
    );
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionParameter",
        None,
        false,
        false,
        &["Reflector"],
    );
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
        "ReflectionFunction",
        "isanonymous",
        function_is_anonymous,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionFunction",
        "getclosurethis",
        function_get_closure_this,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionFunction",
        "getclosurecalledclass",
        function_get_closure_called_class,
        1,
        0,
        []
    );
    for class in ["ReflectionFunction", "ReflectionMethod"] {
        register_method!(class, "getparameters", function_get_parameters, 1, 0, []);
        register_method!(class, "getdoccomment", reflection_get_doc_comment, 1, 0, []);
        register_method!(
            class,
            "returnsreference",
            function_returns_reference,
            1,
            0,
            []
        );
        register_method!(class, "isclosure", function_is_closure, 1, 0, []);
        register_method!(class, "hasreturntype", function_has_return_type, 1, 0, []);
        register_method!(class, "getreturntype", function_get_return_type, 1, 0, []);
        register_method!(
            class,
            "hastentativereturntype",
            function_has_tentative_return_type,
            1,
            0,
            []
        );
        register_method!(
            class,
            "gettentativereturntype",
            function_get_tentative_return_type,
            1,
            0,
            []
        );
        register_method!(
            class,
            "getnumberofparameters",
            function_get_number_of_parameters,
            1,
            0,
            []
        );
        register_method!(
            class,
            "getnumberofrequiredparameters",
            function_get_number_of_required_parameters,
            1,
            0,
            []
        );
        register_method!(
            class,
            "getattributes",
            class_get_attributes,
            3,
            0,
            ["name", "flags"]
        );
    }
    register_method!(
        "ReflectionParameter",
        "getname",
        parameter_get_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "gettype",
        parameter_get_type,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "hastype",
        parameter_has_type,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "isvariadic",
        parameter_is_variadic,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "isoptional",
        parameter_is_optional,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "ispassedbyreference",
        parameter_is_passed_by_reference,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "isdefaultvalueavailable",
        parameter_is_default_available,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "getdefaultvalue",
        parameter_get_default_value,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "allowsnull",
        parameter_allows_null,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionParameter",
        "getattributes",
        parameter_get_attributes,
        3,
        0,
        ["name", "flags"]
    );
    register_method!(
        "ReflectionParameter",
        "getdeclaringclass",
        parameter_get_declaring_class,
        1,
        0,
        []
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
    register_method!("ReflectionClass", "getname", class_get_name, 1, 0, []);
    register_method!(
        "ReflectionClass",
        "getdoccomment",
        reflection_get_doc_comment,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "getattributes",
        class_get_attributes,
        3,
        0,
        ["name", "flags"]
    );
    register_method!(
        "ReflectionClass",
        "getparentclass",
        class_get_parent,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "getconstructor",
        class_get_constructor,
        1,
        0,
        []
    );
    register_method!("ReflectionClass", "isinternal", class_is_internal, 1, 0, []);
    register_method!(
        "ReflectionClass",
        "issubclassof",
        class_is_subclass_of,
        2,
        1,
        ["class"]
    );
    register_method!(
        "ReflectionClass",
        "implementsinterface",
        class_implements_interface,
        2,
        1,
        ["interface"]
    );
    register_method!(
        "ReflectionClass",
        "isuserdefined",
        class_is_user_defined,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "getinterfacenames",
        class_get_interface_names,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "getinterfaces",
        class_get_interfaces,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "gettraitnames",
        class_get_trait_names,
        1,
        0,
        []
    );
    register_method!("ReflectionClass", "gettraits", class_get_traits, 1, 0, []);
    register_method!(
        "ReflectionClass",
        "getconstants",
        class_get_constants,
        2,
        0,
        ["filter"]
    );
    register_method!(
        "ReflectionClass",
        "getreflectionconstants",
        class_get_reflection_constants,
        2,
        0,
        ["filter"]
    );
    register_method!(
        "ReflectionClass",
        "getdefaultproperties",
        class_get_default_properties,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "getproperties",
        class_get_properties,
        2,
        0,
        ["filter"]
    );
    register_method!(
        "ReflectionClass",
        "getmethods",
        class_get_methods,
        2,
        0,
        ["filter"]
    );
    register_method!(
        "ReflectionClass",
        "hasmethod",
        class_has_method,
        2,
        1,
        ["name"]
    );
    register_method!(
        "ReflectionClass",
        "getmethod",
        class_get_method,
        2,
        1,
        ["name"]
    );
    register_method!(
        "ReflectionClass",
        "isinterface",
        class_is_interface,
        1,
        0,
        []
    );
    register_method!("ReflectionClass", "istrait", class_is_trait, 1, 0, []);
    register_method!("ReflectionClass", "isabstract", class_is_abstract, 1, 0, []);
    register_method!("ReflectionClass", "isfinal", class_is_final, 1, 0, []);
    register_method!("ReflectionClass", "isreadonly", class_is_readonly, 1, 0, []);
    register_method!(
        "ReflectionClass",
        "isinstantiable",
        class_is_instantiable,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClass",
        "newlazyghost",
        class_new_lazy_ghost,
        3,
        1,
        ["initializer", "options"]
    );
    register_method!(
        "ReflectionClass",
        "newinstancewithoutconstructor",
        class_new_instance_without_constructor,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "__construct",
        method_construct,
        3,
        2,
        ["class", "method"]
    );
    register_method!(
        "ReflectionMethod",
        "getdeclaringclass",
        parameter_get_declaring_class,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "getmodifiers",
        method_get_modifiers,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "isconstructor",
        method_is_constructor,
        1,
        0,
        []
    );
    register_method!("ReflectionMethod", "ispublic", method_is_public, 1, 0, []);
    register_method!(
        "ReflectionMethod",
        "isprotected",
        method_is_protected,
        1,
        0,
        []
    );
    register_method!("ReflectionMethod", "isprivate", method_is_private, 1, 0, []);
    register_method!("ReflectionMethod", "isstatic", method_is_static, 1, 0, []);
    register_method!("ReflectionMethod", "isfinal", method_is_final, 1, 0, []);
    register_method!(
        "ReflectionMethod",
        "isabstract",
        method_is_abstract,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "isdestructor",
        method_is_destructor,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "hasprototype",
        method_has_prototype,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "getprototype",
        method_get_prototype,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionMethod",
        "invoke",
        method_invoke,
        3,
        1,
        ["object", "args"]
    );
    register_method!(
        "ReflectionMethod",
        "getclosure",
        method_get_closure,
        2,
        0,
        ["object"]
    );
    register_method!(
        "ReflectionMethod",
        "getfilename",
        method_file_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionClassConstant",
        "getattributes",
        class_get_attributes,
        3,
        0,
        ["name", "flags"]
    );
    register_method!(
        "ReflectionClassConstant",
        "getdoccomment",
        reflection_get_doc_comment,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "__construct",
        property_construct,
        3,
        2,
        ["class", "property"]
    );
    register_method!(
        "ReflectionProperty",
        "isinitialized",
        property_is_initialized,
        2,
        0,
        ["object"]
    );
    register_method!(
        "ReflectionProperty",
        "getmodifiers",
        property_get_modifiers,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "gettype",
        parameter_get_type,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "hastype",
        parameter_has_type,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "getdoccomment",
        reflection_get_doc_comment,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "getattributes",
        class_get_attributes,
        3,
        0,
        ["name", "flags"]
    );
    register_method!(
        "ReflectionProperty",
        "isdefault",
        property_is_default,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "ispublic",
        property_is_public,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "isprotected",
        property_is_protected,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "isprivate",
        property_is_private,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "isstatic",
        property_is_static,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "isreadonly",
        property_is_readonly,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "getvalue",
        property_get_value,
        2,
        0,
        ["object"]
    );
    register_method!(
        "ReflectionProperty",
        "setvalue",
        property_set_value,
        3,
        2,
        ["object", "value"]
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
        register_method!(class, "getfilename", class_file_name, 1, 0, []);
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
        "isbuiltin",
        reflection_type_is_builtin,
        1,
        0,
        []
    );
    for class in [
        "ReflectionNamedType",
        "ReflectionUnionType",
        "ReflectionIntersectionType",
    ] {
        register_method!(class, "allowsnull", reflection_type_allows_null, 1, 0, []);
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
