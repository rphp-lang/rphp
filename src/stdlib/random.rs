//! Built-in Random extension declarations admitted by the PHP 8.5 contract.

use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::make_internal_function;
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpArray, PhpObject, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

const INTERVAL_BOUNDARY: &str = "Random\\IntervalBoundary";
const INTERVAL_BOUNDARY_CASES: [&str; 4] = ["ClosedOpen", "ClosedClosed", "OpenClosed", "OpenOpen"];

fn unit_enum_case(class: &str, name: &str) -> PropertyDefinition {
    let mut properties = HashMap::with_capacity(1);
    properties.insert("name".to_string(), Value::string(name));
    PropertyDefinition::new(
        name.to_string(),
        Some(Value::object(PhpObject::dynamic(
            class.to_string(),
            0,
            properties,
        ))),
        Visibility::Public,
        class.to_string(),
    )
}

fn interval_boundary_cases(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let definition = eg
        .find_class(INTERVAL_BOUNDARY)
        .expect("registered Random\\IntervalBoundary enum is available");
    let class_id = definition.class_id;
    let count = definition.static_properties.len();
    let mut cases = PhpArray::with_packed_capacity(count);
    for index in 0..count {
        let storage_slot = eg
            .static_property_storage_slot(class_id, index)
            .expect("registered enum case owns a static storage slot");
        let value = eg
            .static_property_value(storage_slot)
            .expect("registered enum case storage remains live")
            .clone();
        cases.push(value);
    }
    super::write_return_value(rv, Value::array(cases));
    Ok(())
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let cases = INTERVAL_BOUNDARY_CASES
        .into_iter()
        .map(|name| unit_enum_case(INTERVAL_BOUNDARY, name))
        .collect();
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: INTERVAL_BOUNDARY.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["UnitEnum".to_string()],
        is_interface: false,
        is_abstract: false,
        // PHP's internal enum omits the ReflectionClass final modifier. The
        // enum-parent guard still rejects every attempt to extend it.
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
            INTERVAL_BOUNDARY.to_string(),
            ParamTypeHint::String,
            true,
            false,
        )],
        static_properties: cases,
        constants: Vec::new(),
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec!["name".to_string()],
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("Random\\IntervalBoundary registration is unique");

    let mut cases_method = Box::new(make_internal_function(
        interval_boundary_cases,
        0,
        0,
        Vec::new(),
    ));
    cases_method.common.sig.return_type_hint = ParamTypeHint::Array;
    let cases_pointer = &cases_method.common as *const FunctionCommon;
    eg.function_table
        .insert("random\\intervalboundary::cases".to_string(), cases_pointer);
    eg.method_declaring_class
        .insert(cases_pointer, INTERVAL_BOUNDARY.to_string());

    vec![cases_method]
}
