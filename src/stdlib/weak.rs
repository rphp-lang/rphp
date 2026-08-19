//! PHP 8.5 WeakReference, WeakMap and InternalIterator API.

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::make_internal_method;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpObject, Value, ValueType, make_error_value};
use crate::vm::execute::{
    VmError, prepare_replaced_value_destructor, run_prepared_value_destructor,
};
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction};

use super::{owned_argument, write_return_value};

fn argument(execute_data: *mut ExecuteData, index: u32) -> Value {
    owned_argument(execute_data, index)
}

fn write_result(return_value: *mut Value, value: Value) {
    write_return_value(return_value, value);
}

fn internal_object(eg: &ExecutorGlobals, class_name: &str) -> Value {
    let class = eg
        .find_class(class_name)
        .unwrap_or_else(|| panic!("registered internal class {class_name} must exist"));
    Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.as_ref(),
    ))
}

fn weak_reference_construct(
    _execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(make_error_value(
        "Error",
        "Direct instantiation of WeakReference is not allowed, use WeakReference::create instead",
    ));
    Ok(())
}

fn weak_reference_create(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = argument(execute_data, 1);
    if target.weak_object_identity().is_none() {
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "WeakReference::create(): Argument #1 ($object) must be of type object, {} given",
                target.dereferenced().type_name()
            ),
        ));
        return Ok(());
    }
    if let Some(reference) = eg.existing_weak_reference(&target) {
        write_result(return_value, reference);
        return Ok(());
    }
    let reference = internal_object(eg, "WeakReference");
    if !eg.register_weak_reference(&reference, &target) {
        return Err(VmError::Fatal(
            "Failed to register WeakReference state".to_string(),
        ));
    }
    write_result(return_value, reference);
    Ok(())
}

fn weak_reference_get(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        eg.weak_reference_target(&argument(execute_data, 0))
            .unwrap_or_else(Value::null),
    );
    Ok(())
}

fn weak_map_key_error(eg: &mut ExecutorGlobals) {
    eg.exception = Some(make_error_value(
        "TypeError",
        "WeakMap key must be an object",
    ));
}

fn weak_map_missing_error(eg: &mut ExecutorGlobals, key: &Value) {
    let class_name = key
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "Closure".to_string());
    let handle = key.object_handle().unwrap_or(0);
    eg.exception = Some(make_error_value(
        "Error",
        &format!("Object {class_name}#{handle} not contained in WeakMap"),
    ));
}

fn validate_weak_map_key(eg: &mut ExecutorGlobals, key: &Value) -> bool {
    if key.weak_object_identity().is_some() {
        true
    } else {
        weak_map_key_error(eg);
        false
    }
}

fn weak_map_get_value(
    eg: &mut ExecutorGlobals,
    map: &Value,
    key: &Value,
    preserve_reference: bool,
) -> Value {
    if !validate_weak_map_key(eg, key) {
        return Value::null();
    }
    let Some(value) = eg.weak_map_value(map, key) else {
        weak_map_missing_error(eg, key);
        return Value::null();
    };
    if preserve_reference {
        value.clone_owned_reference_alias()
    } else {
        value.dereferenced().clone()
    }
}

fn weak_map_set_value(
    eg: &mut ExecutorGlobals,
    map: &Value,
    key: &Value,
    value: &Value,
    append: bool,
) -> Result<(), VmError> {
    if append {
        eg.exception = Some(make_error_value("Error", "Cannot append to WeakMap"));
        return Ok(());
    }
    if !validate_weak_map_key(eg, key) {
        return Ok(());
    }
    let release = eg
        .weak_map_value(map, key)
        .and_then(|old| prepare_replaced_value_destructor(eg, old.dereferenced()));
    if !eg.set_weak_map_value(map, key, value) {
        return Err(VmError::Fatal("Failed to update WeakMap state".to_string()));
    }
    run_prepared_value_destructor(eg, release)
}

fn weak_map_remove_value(
    eg: &mut ExecutorGlobals,
    map: &Value,
    key: &Value,
) -> Result<(), VmError> {
    if !validate_weak_map_key(eg, key) {
        return Ok(());
    }
    let release = eg
        .weak_map_value(map, key)
        .and_then(|old| prepare_replaced_value_destructor(eg, old.dereferenced()));
    let removed = eg.remove_weak_map_value(map, key);
    if removed.is_some() {
        run_prepared_value_destructor(eg, release)?;
    }
    Ok(())
}

fn weak_map_offset_get(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let map = argument(execute_data, 0);
    let key = argument(execute_data, 1);
    let value = weak_map_get_value(eg, &map, &key, false);
    if eg.exception.is_none() {
        write_result(return_value, value);
    }
    Ok(())
}

fn weak_map_offset_set(
    execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    weak_map_set_value(
        eg,
        &argument(execute_data, 0),
        &argument(execute_data, 1),
        &argument(execute_data, 2),
        false,
    )
}

fn weak_map_offset_exists(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let map = argument(execute_data, 0);
    let key = argument(execute_data, 1);
    if !validate_weak_map_key(eg, &key) {
        return Ok(());
    }
    let exists = eg.weak_map_value(&map, &key).is_some_and(|value| {
        !matches!(
            value.dereferenced().value_type(),
            ValueType::Null | ValueType::Undef
        )
    });
    write_result(return_value, Value::bool(exists));
    Ok(())
}

fn weak_map_offset_unset(
    execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    weak_map_remove_value(eg, &argument(execute_data, 0), &argument(execute_data, 1))
}

fn weak_map_count(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let map = argument(execute_data, 0);
    eg.ensure_weak_map(&map);
    write_result(
        return_value,
        Value::long(eg.weak_map_entries(&map).len() as i64),
    );
    Ok(())
}

fn weak_map_get_iterator(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let map = argument(execute_data, 0);
    eg.ensure_weak_map(&map);
    let iterator = internal_object(eg, "InternalIterator");
    if !eg.register_weak_map_iterator(&iterator, &map) {
        return Err(VmError::Fatal(
            "Failed to register WeakMap iterator state".to_string(),
        ));
    }
    write_result(return_value, iterator);
    Ok(())
}

fn internal_iterator_construct(
    _execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(make_error_value(
        "Error",
        "Cannot directly construct InternalIterator, use getIterator() instead",
    ));
    Ok(())
}

fn internal_iterator_current(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        eg.weak_iterator_value(&argument(execute_data, 0))
            .unwrap_or_else(Value::null),
    );
    Ok(())
}

fn internal_iterator_key(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        eg.weak_iterator_key(&argument(execute_data, 0))
            .unwrap_or_else(Value::null),
    );
    Ok(())
}

fn internal_iterator_next(
    execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.weak_iterator_next(&argument(execute_data, 0));
    Ok(())
}

fn internal_iterator_valid(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write_result(
        return_value,
        Value::bool(eg.weak_iterator_valid(&argument(execute_data, 0))),
    );
    Ok(())
}

fn internal_iterator_rewind(
    execute_data: *mut ExecuteData,
    _return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.weak_iterator_rewind(&argument(execute_data, 0));
    Ok(())
}

pub(super) fn call_map_protocol(
    eg: &mut ExecutorGlobals,
    receiver: &Value,
    method: &str,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match method.to_ascii_lowercase().as_str() {
        "offsetget" => Ok(weak_map_get_value(
            eg,
            receiver,
            arguments.first().unwrap_or(&Value::null()),
            true,
        )),
        "offsetset" | "offsetsetappend" => {
            let key = arguments.first().cloned().unwrap_or_else(Value::null);
            let value = arguments.get(1).cloned().unwrap_or_else(Value::null);
            weak_map_set_value(
                eg,
                receiver,
                &key,
                &value,
                method.eq_ignore_ascii_case("offsetSetAppend"),
            )?;
            Ok(Value::null())
        }
        "offsetexists" => {
            let key = arguments.first().cloned().unwrap_or_else(Value::null);
            if !validate_weak_map_key(eg, &key) {
                return Ok(Value::bool(false));
            }
            Ok(Value::bool(eg.weak_map_value(receiver, &key).is_some_and(
                |value| {
                    !matches!(
                        value.dereferenced().value_type(),
                        ValueType::Null | ValueType::Undef
                    )
                },
            )))
        }
        "offsetunset" => {
            let key = arguments.first().cloned().unwrap_or_else(Value::null);
            weak_map_remove_value(eg, receiver, &key)?;
            Ok(Value::null())
        }
        _ => Err(VmError::Fatal(format!(
            "Unsupported WeakMap protocol method {method}"
        ))),
    }
}

fn internal_class(name: &str, implements: Vec<String>) -> ClassDef {
    ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements,
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: Vec::<PropertyDefinition>::new(),
        static_properties: Vec::new(),
        constants: Vec::new(),
        property_layout: std::rc::Rc::new(ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: Vec::new(),
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        class_id: 0,
    }
}

fn register_method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    class: &str,
    name: &str,
    display_name: &str,
    function: InternalFunction,
    is_static: bool,
) {
    let function = Box::new(function);
    let pointer = &function.common as *const FunctionCommon;
    eg.function_table
        .insert(format!("{class}::{name}").to_ascii_lowercase(), pointer);
    eg.method_declaring_class.insert(pointer, class.to_string());
    if is_static {
        eg.register_internal_static_method(pointer);
    }
    eg.register_internal_function_display_name(pointer, format!("{class}::{display_name}"));
    functions.push(function);
}

pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.register_class(internal_class("WeakReference", Vec::new()))
        .unwrap();
    eg.register_class(internal_class(
        "WeakMap",
        vec![
            "ArrayAccess".to_string(),
            "Countable".to_string(),
            "IteratorAggregate".to_string(),
        ],
    ))
    .unwrap();
    eg.register_class(internal_class(
        "InternalIterator",
        vec!["Iterator".to_string()],
    ))
    .unwrap();

    let mut functions = Vec::with_capacity(15);
    register_method(
        eg,
        &mut functions,
        "WeakReference",
        "__construct",
        "__construct",
        make_internal_method(weak_reference_construct, 1, 0, Vec::new()),
        false,
    );
    register_method(
        eg,
        &mut functions,
        "WeakReference",
        "create",
        "create",
        make_internal_method(weak_reference_create, 2, 1, vec!["object".to_string()]),
        true,
    );
    register_method(
        eg,
        &mut functions,
        "WeakReference",
        "get",
        "get",
        make_internal_method(weak_reference_get, 1, 0, Vec::new()),
        false,
    );

    for (name, display, handler, args, required, parameters) in [
        (
            "offsetget",
            "offsetGet",
            weak_map_offset_get as _,
            2,
            1,
            vec!["object".to_string()],
        ),
        (
            "offsetset",
            "offsetSet",
            weak_map_offset_set as _,
            3,
            2,
            vec!["object".to_string(), "value".to_string()],
        ),
        (
            "offsetexists",
            "offsetExists",
            weak_map_offset_exists as _,
            2,
            1,
            vec!["object".to_string()],
        ),
        (
            "offsetunset",
            "offsetUnset",
            weak_map_offset_unset as _,
            2,
            1,
            vec!["object".to_string()],
        ),
    ] {
        register_method(
            eg,
            &mut functions,
            "WeakMap",
            name,
            display,
            make_internal_method(handler, args, required, parameters),
            false,
        );
    }
    for (name, display, handler) in [
        ("count", "count", weak_map_count as _),
        ("getiterator", "getIterator", weak_map_get_iterator as _),
    ] {
        register_method(
            eg,
            &mut functions,
            "WeakMap",
            name,
            display,
            make_internal_method(handler, 1, 0, Vec::new()),
            false,
        );
    }

    for (name, handler) in [
        ("__construct", internal_iterator_construct as _),
        ("current", internal_iterator_current as _),
        ("key", internal_iterator_key as _),
        ("next", internal_iterator_next as _),
        ("valid", internal_iterator_valid as _),
        ("rewind", internal_iterator_rewind as _),
    ] {
        register_method(
            eg,
            &mut functions,
            "InternalIterator",
            name,
            name,
            make_internal_method(handler, 1, 0, Vec::new()),
            false,
        );
    }
    functions
}
