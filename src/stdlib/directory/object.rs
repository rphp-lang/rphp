//! Directory is a typed, non-constructible view of the existing directory
//! resource. No separate backend or hidden lifetime owner is required.

use std::rc::Rc;

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::{make_internal_function, make_internal_method};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpObject, Value, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(in crate::stdlib) fn register_class(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut class = ClassDef {
        attributes: vec![],
        name: "Directory".into(),
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
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec!["path".into(), "handle".into()],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    };
    for (name, hint) in [
        ("path", ParamTypeHint::String),
        ("handle", ParamTypeHint::Mixed),
    ] {
        let mut property = PropertyDefinition::declared(
            name.into(),
            None,
            Visibility::Public,
            class.name.clone(),
            hint,
            true,
            false,
        );
        property.set_visibility = Some(Visibility::Protected);
        class.properties.push(property);
    }
    eg.register_class(class)
        .expect("unique native Directory class");
    let mut functions = Vec::with_capacity(3);
    for (name, handler, result) in [
        (
            "close",
            fn_close as InternalFunctionHandler,
            ParamTypeHint::Void,
        ),
        ("rewind", fn_rewind, ParamTypeHint::Void),
        (
            "read",
            fn_read,
            ParamTypeHint::Union(vec![
                ParamTypeHint::String,
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
    ] {
        eg.register_internal_method_contract(
            "Directory",
            name,
            false,
            0,
            &[],
            vec![],
            result.clone(),
            &[],
            false,
        );
        let mut function = Box::new(make_internal_method(handler, 1, 0, vec![]));
        function.common.sig.return_type_hint = result;
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(format!("directory::{name}"), pointer);
        eg.method_declaring_class
            .insert(pointer, "Directory".into());
        eg.register_internal_function_reflection_metadata(pointer, vec![], "standard");
        functions.push(function);
    }
    functions
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(in crate::stdlib) fn register_function(eg: &mut ExecutorGlobals) -> Box<InternalFunction> {
    let mut function = Box::new(make_internal_function(
        fn_dir,
        2,
        1,
        vec!["directory".into(), "context".into()],
    ));
    function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::None];
    function.common.sig.return_type_hint = ParamTypeHint::Union(vec![
        ParamTypeHint::ClassName("Directory".into()),
        ParamTypeHint::ClassName("false".into()),
    ]);
    function.handler_validates_types = true;
    let pointer = &function.common as *const FunctionCommon;
    eg.register_function("dir", pointer)
        .expect("unique native dir function");
    eg.register_internal_function_reflection_metadata(
        pointer,
        vec![None, Some(Value::null())],
        "standard",
    );
    function
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn fn_dir(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some((path, handle)) = super::open_directory(ed, eg, "dir")? else {
        ret!(rv, Value::bool(false));
    };
    let class = eg
        .find_class("Directory")
        .expect("native Directory class is registered");
    // The native factory owns initialization. Public writes still go through
    // the ordinary typed/readonly capability checks.
    let object = PhpObject::with_layout(
        class.class_id,
        Rc::clone(&class.property_layout),
        vec![path, handle],
    );
    ret!(rv, Value::object(object));
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn method_handle(ed: *mut ExecuteData, eg: &mut ExecutorGlobals, method: &str) -> Option<Value> {
    // Release the object view before consulting a backend or entering any
    // user callback. Keep the resource Value alive across reentrant calls.
    let handle = arg!(ed, 0)
        .as_object()
        .and_then(|object| object.get_property("handle").cloned());
    let Some(handle) = handle else {
        eg.exception = Some(make_error_value(
            "Error",
            "Internal directory stream has been altered",
        ));
        return None;
    };
    let Some(id) = handle.as_resource_id() else {
        eg.exception = Some(make_error_value(
            "Error",
            "Internal directory stream has been altered",
        ));
        return None;
    };
    if !crate::stdlib::resource::is_open_for_request(eg, id) {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "Directory::{method}(): cannot use Directory resource after it has been closed",
            ),
        ));
        return None;
    }
    if !super::is_directory_resource(eg, id) {
        eg.exception = Some(make_error_value(
            "Error",
            "Internal directory stream has been altered",
        ));
        return None;
    }
    Some(handle)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn fn_read(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(handle) = method_handle(ed, eg, "read") else {
        return Ok(());
    };
    let value = super::read_directory(eg, handle.as_resource_id().unwrap())?;
    ret!(rv, value);
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn fn_rewind(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(handle) = method_handle(ed, eg, "rewind") else {
        return Ok(());
    };
    super::rewind_directory(eg, handle.as_resource_id().unwrap())?;
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code; section placement does not change its ABI.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn fn_close(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(handle) = method_handle(ed, eg, "close") else {
        return Ok(());
    };
    super::close_directory(eg, handle.as_resource_id().unwrap())?;
    ret!(rv, Value::null());
}
