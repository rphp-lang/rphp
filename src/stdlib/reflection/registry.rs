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
    attribute_construct, attribute_get_arguments, attribute_get_name, attribute_get_target,
    attribute_is_repeated, attribute_new_instance, attribute_to_string, class_constant_construct,
    class_construct, class_debug_info, class_file_name, class_get_attributes, class_get_constant,
    class_get_constants, class_get_constructor, class_get_default_properties,
    class_get_interface_names, class_get_interfaces, class_get_lazy_initializer, class_get_method,
    class_get_methods, class_get_name, class_get_parent, class_get_properties, class_get_property,
    class_get_reflection_constant, class_get_reflection_constants, class_get_trait_aliases,
    class_get_trait_names, class_get_traits, class_has_method, class_implements_interface,
    class_initialize_lazy_object, class_is_abstract, class_is_final, class_is_instantiable,
    class_is_interface, class_is_internal, class_is_readonly, class_is_subclass_of, class_is_trait,
    class_is_uninitialized_lazy_object, class_is_user_defined,
    class_mark_lazy_object_as_initialized, class_new_instance, class_new_instance_args,
    class_new_instance_without_constructor, class_new_lazy_ghost, class_new_lazy_proxy,
    class_reset_as_lazy_ghost, class_reset_as_lazy_proxy, class_to_string, constant_construct,
    constant_get_value, deprecated_construct, enum_backed_case_construct,
    enum_case_get_backing_value, enum_case_get_enum, enum_case_get_value, enum_construct,
    enum_get_backing_type, enum_get_case, enum_get_cases, enum_has_case, enum_is_backed_reflection,
    enum_unit_case_construct, function_construct, function_get_closure,
    function_get_closure_called_class, function_get_closure_scope_class, function_get_closure_this,
    function_get_namespace_name, function_get_number_of_parameters,
    function_get_number_of_required_parameters, function_get_parameters, function_get_return_type,
    function_get_short_name, function_get_tentative_return_type, function_has_return_type,
    function_has_tentative_return_type, function_in_namespace, function_invoke,
    function_invoke_args, function_is_anonymous, function_is_closure, function_is_deprecated,
    function_returns_reference, function_to_string, generic_arguments, generic_runtime_modes,
    method_construct, method_create_from_method_name, method_file_name, method_get_closure,
    method_get_modifiers, method_get_prototype, method_has_prototype, method_invoke,
    method_invoke_args, method_invoke_raw, method_is_abstract, method_is_constructor,
    method_is_destructor, method_is_final, method_is_private, method_is_protected,
    method_is_public, method_is_static, method_to_string, no_discard_construct, object_construct,
    override_construct, parameter_allows_null, parameter_construct, parameter_get_attributes,
    parameter_get_class, parameter_get_declaring_class, parameter_get_default_value,
    parameter_get_name, parameter_get_type, parameter_has_type, parameter_is_array,
    parameter_is_callable, parameter_is_default_available, parameter_is_optional,
    parameter_is_passed_by_reference, parameter_is_variadic, parameter_to_string,
    property_construct, property_get_default_value, property_get_hook, property_get_hooks,
    property_get_modifiers, property_get_raw_value, property_get_value, property_has_default_value,
    property_has_hook, property_hook_type_cases, property_hook_type_from,
    property_hook_type_try_from, property_is_abstract, property_is_default, property_is_final,
    property_is_initialized, property_is_lazy, property_is_private, property_is_protected,
    property_is_public, property_is_readonly, property_is_static, property_is_virtual,
    property_set_raw_value, property_set_raw_value_without_lazy_initialization, property_set_value,
    property_skip_lazy_initialization, property_to_string, reflection_compound_types,
    reflection_get_doc_comment, reflection_reference_construct, reflection_reference_debug_info,
    reflection_reference_from_array_element, reflection_reference_get_id,
    reflection_type_allows_null, reflection_type_generic_arguments,
    reflection_type_has_generic_arguments, reflection_type_is_builtin, reflection_type_name,
    reflection_type_to_string, sensitive_parameter_construct,
};
use crate::compiler::compile::{ClassConstantDefinition, ClassDef, PropertyDefinition};
use crate::compiler::{
    make_internal_method, make_internal_method_variadic, make_internal_method_variadic_raw,
};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{PhpObject, Value};
use crate::vm::function::{
    AttributeArgument, AttributeDefinition, AttributeEvaluationScope, ParamTypeHint,
};
use crate::vm::function::{FunctionCommon, InternalFunction, InternalFunctionHandler};

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
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: parent.map(str::to_owned),
        implements: implements.iter().map(|name| (*name).to_string()).collect(),
        is_interface: false,
        is_abstract,
        is_final,
        is_trait: false,
        is_enum,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
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
                attributes: Vec::new(),
                name: constant.to_string(),
                value: Value::long(value),
                source_file: String::new(),
                evaluation_error: None,
                source_expression: None,
                evaluation_scope: None,
                value_is_deferred: false,
                visibility: Visibility::Public,
                declaring_class: name.to_string(),
                type_hint: ParamTypeHint::Int,
                is_final: false,
            })
            .collect()
        } else if name == "ReflectionClass" {
            [
                ("SKIP_INITIALIZATION_ON_SERIALIZE", 8),
                ("SKIP_DESTRUCTOR", 16),
            ]
            .into_iter()
            .map(|(constant, value)| ClassConstantDefinition {
                attributes: Vec::new(),
                name: constant.to_string(),
                value: Value::long(value),
                source_file: String::new(),
                evaluation_error: None,
                source_expression: None,
                evaluation_scope: None,
                value_is_deferred: false,
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
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();
}

fn register_reflection_interface(eg: &mut ExecutorGlobals, name: &str) {
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Stringable".to_string()],
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
        attributes: Vec::new(),
        name: "ReflectionGenericVariance".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: true,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: properties,
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

fn property_hook_type_case(name: &str, value: &str) -> PropertyDefinition {
    let properties = [
        ("name".to_string(), Value::string(name)),
        ("value".to_string(), Value::string(value)),
    ]
    .into_iter()
    .collect();
    PropertyDefinition::new(
        name.to_string(),
        Some(Value::object(PhpObject::dynamic(
            "PropertyHookType".to_string(),
            0,
            properties,
        ))),
        Visibility::Public,
        "PropertyHookType".to_string(),
    )
}

fn register_property_hook_type(eg: &mut ExecutorGlobals) {
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "PropertyHookType".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        // BackedEnum already extends UnitEnum. Keeping only the direct edge
        // avoids presenting the inherited contract as a duplicate during
        // later hierarchy validation; Reflection expands both interfaces.
        implements: vec!["BackedEnum".to_string()],
        is_interface: false,
        is_abstract: false,
        // PHP's internal enums do not publish ReflectionClass::isFinal().
        // The enum-parent guard still prevents inheritance.
        is_final: false,
        is_trait: false,
        is_enum: true,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: [
            ("name", ParamTypeHint::String),
            ("value", ParamTypeHint::String),
        ]
        .into_iter()
        .map(|(name, hint)| {
            PropertyDefinition::declared(
                name.to_string(),
                None,
                Visibility::Public,
                "PropertyHookType".to_string(),
                hint,
                true,
                false,
            )
        })
        .collect(),
        static_properties: vec![
            property_hook_type_case("Get", "get"),
            property_hook_type_case("Set", "set"),
        ],
        constants: Vec::new(),
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec!["name".to_string(), "value".to_string()],
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("PropertyHookType registration is unique");
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
            let registered_name = format!("{}::{}", $class, $method);
            eg.function_table.insert(
                registered_name.to_ascii_lowercase(),
                pointer,
            );
            eg.register_internal_function_display_name(pointer, registered_name);
            eg.method_declaring_class
                .insert(pointer, $class.to_string());
            functions.push(function);
        }};
    }

    macro_rules! register_static_method {
        ($class:expr, $method:expr, $handler:expr, $num_args:expr, $min_args:expr, [$($name:expr),*]) => {{
            // Static method calls reserve CV 0 for the called-class slot just
            // like user methods do; public arguments therefore begin at CV 1.
            let function = Box::new(make_internal_method(
                $handler,
                $num_args + 1,
                $min_args,
                vec![$($name.to_string()),*],
            ));
            let pointer = &function.common as *const FunctionCommon;
            let registered_name = format!("{}::{}", $class, $method);
            eg.function_table.insert(registered_name.to_ascii_lowercase(), pointer);
            eg.register_internal_function_display_name(pointer, registered_name);
            eg.method_declaring_class.insert(pointer, $class.to_string());
            functions.push(function);
        }};
    }

    macro_rules! register_variadic_method {
        ($class:expr, $method:expr, $handler:expr, $required:expr, [$($name:expr),*]) => {{
            let function = Box::new(make_internal_method_variadic(
                $handler,
                $required,
                vec![$($name.to_string()),*],
            ));
            let pointer = &function.common as *const FunctionCommon;
            let registered_name = format!("{}::{}", $class, $method);
            eg.function_table
                .insert(registered_name.to_ascii_lowercase(), pointer);
            eg.register_internal_function_display_name(pointer, registered_name);
            eg.method_declaring_class
                .insert(pointer, $class.to_string());
            functions.push(function);
        }};
    }

    macro_rules! register_variadic_method_raw {
        ($class:expr, $method:expr, $handler:expr, $raw_handler:expr, $required:expr, [$($name:expr),*]) => {{
            let function = Box::new(make_internal_method_variadic_raw(
                $handler,
                $raw_handler,
                $required,
                vec![$($name.to_string()),*],
            ));
            let pointer = &function.common as *const FunctionCommon;
            let registered_name = format!("{}::{}", $class, $method);
            eg.function_table
                .insert(registered_name.to_ascii_lowercase(), pointer);
            eg.register_internal_function_display_name(pointer, registered_name);
            eg.method_declaring_class
                .insert(pointer, $class.to_string());
            functions.push(function);
        }};
    }

    register_property_hook_type(eg);
    register_static_method!(
        "PropertyHookType",
        "cases",
        property_hook_type_cases,
        0,
        0,
        []
    );
    functions
        .last_mut()
        .expect("PropertyHookType::cases was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
    register_static_method!(
        "PropertyHookType",
        "from",
        property_hook_type_from,
        1,
        1,
        ["value"]
    );
    functions
        .last_mut()
        .expect("PropertyHookType::from was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::String];
    functions
        .last_mut()
        .expect("PropertyHookType::from was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::ClassName("PropertyHookType".to_string());
    register_static_method!(
        "PropertyHookType",
        "tryFrom",
        property_hook_type_try_from,
        1,
        1,
        ["value"]
    );
    functions
        .last_mut()
        .expect("PropertyHookType::tryFrom was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::String];
    functions
        .last_mut()
        .expect("PropertyHookType::tryFrom was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "PropertyHookType".to_string(),
    )));

    register_reflection_interface(eg, "Reflector");
    eg.register_class(ClassDef {
        name: "Attribute".to_string(),
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
            "flags".to_string(),
            Some(Value::long(127)),
            Visibility::Public,
            "Attribute".to_string(),
            ParamTypeHint::Int,
            false,
            false,
        )],
        static_properties: vec![],
        constants: [
            ("TARGET_CLASS", 1),
            ("TARGET_FUNCTION", 2),
            ("TARGET_METHOD", 4),
            ("TARGET_PROPERTY", 8),
            ("TARGET_CLASS_CONSTANT", 16),
            ("TARGET_PARAMETER", 32),
            ("TARGET_CONSTANT", 64),
            ("TARGET_ALL", 127),
            ("IS_REPEATABLE", 128),
        ]
        .into_iter()
        .map(|(name, value)| ClassConstantDefinition {
            name: name.to_string(),
            value: Value::long(value),
            source_file: String::new(),
            evaluation_error: None,
            source_expression: None,
            evaluation_scope: None,
            value_is_deferred: false,
            visibility: Visibility::Public,
            declaring_class: "Attribute".to_string(),
            type_hint: ParamTypeHint::Int,
            is_final: false,
            attributes: Vec::new(),
        })
        .collect(),
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
        attributes: vec![AttributeDefinition {
            name: "Attribute".to_string(),
            arguments: vec![AttributeArgument {
                name: None,
                value: Ok(Value::long(1)),
                deferred_expression: None,
            }],
            evaluation_scope: std::rc::Rc::new(AttributeEvaluationScope::default()),
            target: 1,
            source_file: String::new(),
            source_line: 0,
            strict_types: false,
        }],
    })
    .unwrap();
    register_method!(
        "Attribute",
        "__construct",
        attribute_construct,
        2,
        0,
        ["flags"]
    );
    functions
        .last_mut()
        .expect("Attribute constructor was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Int];

    eg.register_class(ClassDef {
        name: "Override".to_string(),
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
        attributes: vec![AttributeDefinition {
            name: "Attribute".to_string(),
            arguments: vec![AttributeArgument {
                name: None,
                value: Ok(Value::long(12)),
                deferred_expression: None,
            }],
            evaluation_scope: std::rc::Rc::new(AttributeEvaluationScope::default()),
            target: 1,
            source_file: String::new(),
            source_line: 0,
            strict_types: false,
        }],
    })
    .unwrap();
    register_method!("Override", "__construct", override_construct, 1, 0, []);

    eg.register_class(ClassDef {
        name: "SensitiveParameter".to_string(),
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
        attributes: vec![AttributeDefinition {
            name: "Attribute".to_string(),
            arguments: vec![AttributeArgument {
                name: None,
                value: Ok(Value::long(32)),
                deferred_expression: None,
            }],
            evaluation_scope: std::rc::Rc::new(AttributeEvaluationScope::default()),
            target: 1,
            source_file: String::new(),
            source_line: 0,
            strict_types: false,
        }],
    })
    .unwrap();
    register_method!(
        "SensitiveParameter",
        "__construct",
        sensitive_parameter_construct,
        1,
        0,
        []
    );

    eg.register_class(ClassDef {
        name: "Deprecated".to_string(),
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
        properties: ["message", "since"]
            .into_iter()
            .map(|name| {
                PropertyDefinition::declared_with_set_visibility(
                    name.to_string(),
                    None,
                    Visibility::Public,
                    Some(Visibility::Protected),
                    "Deprecated".to_string(),
                    ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
                    true,
                    false,
                )
            })
            .collect(),
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec!["message".to_string(), "since".to_string()],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
        // PHP exposes the marker as Attribute::TARGET_CLASS |
        // TARGET_FUNCTION | TARGET_METHOD | TARGET_CLASS_CONSTANT |
        // TARGET_CONSTANT. Core applies the narrower declaration rules.
        attributes: vec![AttributeDefinition {
            name: "Attribute".to_string(),
            arguments: vec![AttributeArgument {
                name: None,
                value: Ok(Value::long(87)),
                deferred_expression: None,
            }],
            evaluation_scope: std::rc::Rc::new(AttributeEvaluationScope::default()),
            target: 1,
            source_file: String::new(),
            source_line: 0,
            strict_types: false,
        }],
    })
    .unwrap();
    register_method!(
        "Deprecated",
        "__construct",
        deprecated_construct,
        3,
        0,
        ["message", "since"]
    );
    functions
        .last_mut()
        .expect("Deprecated constructor was just registered")
        .common
        .sig
        .param_type_hints = vec![
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
    ];
    eg.register_class(ClassDef {
        name: "NoDiscard".to_string(),
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
        properties: vec![PropertyDefinition::declared_with_set_visibility(
            "message".to_string(),
            None,
            Visibility::Public,
            Some(Visibility::Protected),
            "NoDiscard".to_string(),
            ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            true,
            false,
        )],
        static_properties: vec![],
        constants: vec![],
        property_layout: std::rc::Rc::new(crate::value::ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: vec!["message".to_string()],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
        attributes: vec![AttributeDefinition {
            name: "Attribute".to_string(),
            arguments: vec![AttributeArgument {
                name: None,
                value: Ok(Value::long(6)),
                deferred_expression: None,
            }],
            evaluation_scope: std::rc::Rc::new(AttributeEvaluationScope::default()),
            target: 1,
            source_file: String::new(),
            source_line: 0,
            strict_types: false,
        }],
    })
    .unwrap();
    register_method!(
        "NoDiscard",
        "__construct",
        no_discard_construct,
        2,
        0,
        ["message"]
    );
    functions
        .last_mut()
        .expect("NoDiscard constructor was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))];
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionFunctionAbstract",
        None,
        true,
        false,
        &["Reflector"],
    );
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ReflectionAttribute".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Reflector".to_string()],
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
        constants: vec![ClassConstantDefinition {
            attributes: Vec::new(),
            name: "IS_INSTANCEOF".to_string(),
            value: Value::long(2),
            source_file: String::new(),
            evaluation_error: None,
            source_expression: None,
            evaluation_scope: None,
            value_is_deferred: false,
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
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .unwrap();
    register_method!(
        "ReflectionAttribute",
        "getname",
        attribute_get_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionAttribute",
        "getarguments",
        attribute_get_arguments,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionAttribute",
        "gettarget",
        attribute_get_target,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionAttribute",
        "isrepeated",
        attribute_is_repeated,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionAttribute",
        "newInstance",
        attribute_new_instance,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionAttribute",
        "__tostring",
        attribute_to_string,
        1,
        0,
        []
    );
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
    register_method!("ReflectionClass", "__debugInfo", class_debug_info, 1, 0, []);
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: "ReflectionProperty".to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Reflector".to_string()],
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
        constants: [
            ("IS_PUBLIC", 1),
            ("IS_PROTECTED", 2),
            ("IS_PRIVATE", 4),
            ("IS_STATIC", 16),
            ("IS_FINAL", 32),
            ("IS_ABSTRACT", 64),
            ("IS_READONLY", 128),
            ("IS_VIRTUAL", 512),
        ]
        .into_iter()
        .map(|(name, value)| ClassConstantDefinition {
            attributes: Vec::new(),
            name: name.to_string(),
            value: Value::long(value),
            source_file: String::new(),
            evaluation_error: None,
            source_expression: None,
            evaluation_scope: None,
            value_is_deferred: false,
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
        enum_backing_error: None,
        deferred_instance_defaults: None,
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
    register_reflection_class_with_interfaces(
        eg,
        "ReflectionConstant",
        None,
        false,
        false,
        &["Reflector"],
    );
    register_reflection_class(eg, "ReflectionReference", None, false, true);
    register_reflection_class(eg, "ReflectionEnum", Some("ReflectionClass"), false, false);
    register_reflection_class(
        eg,
        "ReflectionEnumUnitCase",
        Some("ReflectionClassConstant"),
        false,
        false,
    );
    register_reflection_class(
        eg,
        "ReflectionEnumBackedCase",
        Some("ReflectionEnumUnitCase"),
        false,
        false,
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
        "__tostring",
        function_to_string,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionReference",
        "__construct",
        reflection_reference_construct,
        1,
        0,
        []
    );
    register_static_method!(
        "ReflectionReference",
        "fromArrayElement",
        reflection_reference_from_array_element,
        2,
        2,
        ["array", "key"]
    );
    functions
        .last_mut()
        .expect("ReflectionReference::fromArrayElement was just registered")
        .common
        .sig
        .param_type_hints = vec![
        ParamTypeHint::Array,
        ParamTypeHint::Union(vec![ParamTypeHint::Int, ParamTypeHint::String]),
    ];
    functions
        .last_mut()
        .expect("ReflectionReference::fromArrayElement was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "ReflectionReference".to_string(),
    )));
    register_method!(
        "ReflectionReference",
        "getId",
        reflection_reference_get_id,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionReference::getId was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::String;
    register_method!(
        "ReflectionReference",
        "__debugInfo",
        reflection_reference_debug_info,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionReference::__debugInfo was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;

    register_method!(
        "ReflectionEnum",
        "__construct",
        enum_construct,
        2,
        1,
        ["objectOrClass"]
    );
    register_method!("ReflectionEnum", "hasCase", enum_has_case, 2, 1, ["name"]);
    functions
        .last_mut()
        .expect("ReflectionEnum::hasCase was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::String];
    functions
        .last_mut()
        .expect("ReflectionEnum::hasCase was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Bool;
    register_method!("ReflectionEnum", "getCase", enum_get_case, 2, 1, ["name"]);
    functions
        .last_mut()
        .expect("ReflectionEnum::getCase was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::String];
    functions
        .last_mut()
        .expect("ReflectionEnum::getCase was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::ClassName("ReflectionEnumUnitCase".to_string());
    register_method!("ReflectionEnum", "getCases", enum_get_cases, 1, 0, []);
    functions
        .last_mut()
        .expect("ReflectionEnum::getCases was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
    register_method!(
        "ReflectionEnum",
        "isBacked",
        enum_is_backed_reflection,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionEnum::isBacked was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Bool;
    register_method!(
        "ReflectionEnum",
        "getBackingType",
        enum_get_backing_type,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionEnum::getBackingType was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "ReflectionNamedType".to_string(),
    )));

    for (class, constructor) in [
        (
            "ReflectionEnumUnitCase",
            enum_unit_case_construct as InternalFunctionHandler,
        ),
        (
            "ReflectionEnumBackedCase",
            enum_backed_case_construct as InternalFunctionHandler,
        ),
    ] {
        register_method!(
            class,
            "__construct",
            constructor,
            3,
            2,
            ["class", "constant"]
        );
    }
    register_method!(
        "ReflectionEnumUnitCase",
        "getEnum",
        enum_case_get_enum,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionEnumUnitCase::getEnum was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::ClassName("ReflectionEnum".to_string());
    register_method!(
        "ReflectionEnumUnitCase",
        "getValue",
        enum_case_get_value,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionEnumUnitCase::getValue was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::ClassName("UnitEnum".to_string());
    register_method!(
        "ReflectionEnumBackedCase",
        "getBackingValue",
        enum_case_get_backing_value,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionEnumBackedCase::getBackingValue was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Union(vec![ParamTypeHint::Int, ParamTypeHint::String]);
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
    register_method!(
        "ReflectionFunctionAbstract",
        "getClosureScopeClass",
        function_get_closure_scope_class,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionFunctionAbstract",
        "getShortName",
        function_get_short_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionFunctionAbstract",
        "getNamespaceName",
        function_get_namespace_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionFunctionAbstract",
        "inNamespace",
        function_in_namespace,
        1,
        0,
        []
    );
    register_variadic_method!("ReflectionFunction", "invoke", function_invoke, 0, ["args"]);
    register_method!(
        "ReflectionFunction",
        "invokeArgs",
        function_invoke_args,
        2,
        1,
        ["args"]
    );
    functions
        .last_mut()
        .expect("ReflectionFunction::invokeArgs was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Array];
    register_method!(
        "ReflectionFunction",
        "getclosure",
        function_get_closure,
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
        register_method!(class, "isdeprecated", function_is_deprecated, 1, 0, []);
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
        "__construct",
        parameter_construct,
        3,
        2,
        ["function", "param"]
    );
    functions
        .last_mut()
        .expect("ReflectionParameter::__construct was just registered")
        .common
        .sig
        .param_type_hints = vec![
        ParamTypeHint::None,
        ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Int]),
    ];
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
        "__tostring",
        parameter_to_string,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionConstant",
        "__construct",
        constant_construct,
        2,
        1,
        ["name"]
    );
    register_method!(
        "ReflectionConstant",
        "getname",
        parameter_get_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionConstant",
        "getvalue",
        constant_get_value,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionConstant",
        "getattributes",
        class_get_attributes,
        3,
        0,
        ["name", "flags"]
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
        "getclass",
        parameter_get_class,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionParameter::getClass was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "ReflectionClass".to_string(),
    )));
    register_method!(
        "ReflectionParameter",
        "isarray",
        parameter_is_array,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionParameter::isArray was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Bool;
    register_method!(
        "ReflectionParameter",
        "iscallable",
        parameter_is_callable,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionParameter::isCallable was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Bool;
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
    for class in ["ReflectionClass", "ReflectionObject"] {
        register_method!(class, "__tostring", class_to_string, 1, 0, []);
    }
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
        "gettraitaliases",
        class_get_trait_aliases,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionClass::getTraitAliases was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
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
        "getconstant",
        class_get_constant,
        2,
        1,
        ["name"]
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
        "getreflectionconstant",
        class_get_reflection_constant,
        2,
        1,
        ["name"]
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
        "getproperty",
        class_get_property,
        2,
        1,
        ["name"]
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
        "newlazyproxy",
        class_new_lazy_proxy,
        3,
        1,
        ["factory", "options"]
    );
    register_method!(
        "ReflectionClass",
        "initializelazyobject",
        class_initialize_lazy_object,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionClass",
        "isuninitializedlazyobject",
        class_is_uninitialized_lazy_object,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionClass",
        "marklazyobjectasinitialized",
        class_mark_lazy_object_as_initialized,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionClass",
        "getlazyinitializer",
        class_get_lazy_initializer,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionClass",
        "resetaslazyghost",
        class_reset_as_lazy_ghost,
        4,
        2,
        ["object", "initializer", "options"]
    );
    register_method!(
        "ReflectionClass",
        "resetaslazyproxy",
        class_reset_as_lazy_proxy,
        4,
        2,
        ["object", "factory", "options"]
    );
    register_method!(
        "ReflectionClass",
        "newinstancewithoutconstructor",
        class_new_instance_without_constructor,
        1,
        0,
        []
    );
    register_variadic_method!(
        "ReflectionClass",
        "newinstance",
        class_new_instance,
        0,
        ["args"]
    );
    register_method!(
        "ReflectionClass",
        "newinstanceargs",
        class_new_instance_args,
        2,
        0,
        ["args"]
    );
    register_method!(
        "ReflectionMethod",
        "__construct",
        method_construct,
        3,
        2,
        ["class", "method"]
    );
    register_static_method!(
        "ReflectionMethod",
        "createfrommethodname",
        method_create_from_method_name,
        1,
        1,
        ["method"]
    );
    functions
        .last_mut()
        .expect("ReflectionMethod::createFromMethodName was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::String];
    functions
        .last_mut()
        .expect("ReflectionMethod::createFromMethodName was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::ClassName("static".to_string());
    functions
        .last_mut()
        .expect("ReflectionMethod::createFromMethodName was just registered")
        .common
        .plan
        .set_needs_late_static_scope(true);
    register_method!(
        "ReflectionMethod",
        "getdeclaringclass",
        parameter_get_declaring_class,
        1,
        0,
        []
    );
    register_method!("ReflectionMethod", "__tostring", method_to_string, 1, 0, []);
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
    register_variadic_method_raw!(
        "ReflectionMethod",
        "invoke",
        method_invoke,
        method_invoke_raw,
        1,
        ["object", "args"]
    );
    functions
        .last_mut()
        .expect("ReflectionMethod::invoke was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "object".to_string(),
    )))];
    register_method!(
        "ReflectionMethod",
        "invokeArgs",
        method_invoke_args,
        3,
        2,
        ["object", "args"]
    );
    functions
        .last_mut()
        .expect("ReflectionMethod::invokeArgs was just registered")
        .common
        .sig
        .param_type_hints = vec![
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName("object".to_string()))),
        ParamTypeHint::Array,
    ];
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
        "__construct",
        class_constant_construct,
        3,
        2,
        ["class", "constant"]
    );
    register_method!(
        "ReflectionClassConstant",
        "getname",
        parameter_get_name,
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
        "getdeclaringclass",
        parameter_get_declaring_class,
        1,
        0,
        []
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
        "getname",
        parameter_get_name,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "__tostring",
        property_to_string,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "hasdefaultvalue",
        property_has_default_value,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "getdefaultvalue",
        property_get_default_value,
        1,
        0,
        []
    );
    register_method!("ReflectionProperty", "isfinal", property_is_final, 1, 0, []);
    register_method!(
        "ReflectionProperty",
        "isabstract",
        property_is_abstract,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "isvirtual",
        property_is_virtual,
        1,
        0,
        []
    );
    register_method!(
        "ReflectionProperty",
        "gethook",
        property_get_hook,
        2,
        1,
        ["type"]
    );
    functions
        .last_mut()
        .expect("ReflectionProperty::getHook was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::ClassName("PropertyHookType".to_string())];
    functions
        .last_mut()
        .expect("ReflectionProperty::getHook was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(
        "ReflectionMethod".to_string(),
    )));
    register_method!(
        "ReflectionProperty",
        "gethooks",
        property_get_hooks,
        1,
        0,
        []
    );
    functions
        .last_mut()
        .expect("ReflectionProperty::getHooks was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Array;
    register_method!(
        "ReflectionProperty",
        "hashook",
        property_has_hook,
        2,
        1,
        ["type"]
    );
    functions
        .last_mut()
        .expect("ReflectionProperty::hasHook was just registered")
        .common
        .sig
        .param_type_hints = vec![ParamTypeHint::ClassName("PropertyHookType".to_string())];
    functions
        .last_mut()
        .expect("ReflectionProperty::hasHook was just registered")
        .common
        .sig
        .return_type_hint = ParamTypeHint::Bool;
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
        "getrawvalue",
        property_get_raw_value,
        2,
        1,
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
    register_method!(
        "ReflectionProperty",
        "setrawvalue",
        property_set_raw_value,
        3,
        2,
        ["object", "value"]
    );
    register_method!(
        "ReflectionProperty",
        "setrawvaluewithoutlazyinitialization",
        property_set_raw_value_without_lazy_initialization,
        3,
        2,
        ["object", "value"]
    );
    register_method!(
        "ReflectionProperty",
        "skiplazyinitialization",
        property_skip_lazy_initialization,
        2,
        1,
        ["object"]
    );
    register_method!(
        "ReflectionProperty",
        "islazy",
        property_is_lazy,
        2,
        1,
        ["object"]
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
