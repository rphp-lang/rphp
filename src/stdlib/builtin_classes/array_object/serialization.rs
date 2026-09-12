//! Native array-wrapper state is projected through the ordinary serializer's
//! reference table. Member properties are separate from the backing view.
use super::*;

const SELF_BACKING: i64 = 1 << 24;

pub(in crate::stdlib) fn owner(receiver: &Value) -> &'static str {
    let object = receiver.as_object().expect("native serialization receiver");
    if native_storage_key(&object) == Some(ARRAY_ITERATOR_STORAGE) {
        "ArrayIterator"
    } else {
        "ArrayObject"
    }
}

#[cold]
#[inline(never)]
pub(in crate::stdlib) fn state(receiver: &Value, eg: &ExecutorGlobals) -> PhpArray {
    let (flags, storage, iterator_class_id) = storage_state(receiver);
    let mut data = PhpArray::with_packed_capacity(4);
    data.push(Value::long(flags));
    data.push(storage);
    data.push(Value::array(member_properties(receiver, eg)));
    data.push(
        eg.class_by_id(iterator_class_id)
            .map_or_else(Value::null, |class| Value::string(&class.name)),
    );
    data
}

pub(in crate::stdlib) fn members(receiver: &Value, eg: &ExecutorGlobals) -> PhpArray {
    member_properties(receiver, eg)
}

#[cold]
#[inline(never)]
pub(in crate::stdlib) fn storage_state(receiver: &Value) -> (i64, Value, u32) {
    let object = receiver.as_object().expect("native serialization receiver");
    let options = object.native_array_options();
    let storage = object.get_property(array_object_storage_key(&object));
    let self_backed = storage.and_then(Value::object_identity) == receiver.object_identity();
    let flags = i64::from(options.flags) | if self_backed { SELF_BACKING } else { 0 };
    let storage = if self_backed {
        Value::null()
    } else {
        storage
            .map(Value::clone_for_php_storage)
            .unwrap_or_else(|| Value::array(PhpArray::new()))
    };
    (flags, storage, options.iterator_class_id)
}

fn ill_typed(eg: &mut ExecutorGlobals) {
    eg.exception = Some(make_error_value(
        "UnexpectedValueException",
        "Incomplete or ill-typed serialization data",
    ));
}

#[cold]
#[inline(never)]
pub(in crate::stdlib) fn restore_storage(
    receiver: &Value,
    flags: i64,
    storage: &Value,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    method: &str,
) -> Result<(), VmError> {
    let mut object = receiver.as_object_mut().expect("native receiver");
    let mut options = object.native_array_options();
    options.flags = flags as u16;
    object.set_native_array_options(options);
    drop(object);
    if flags & SELF_BACKING != 0 {
        return replace_storage(receiver, receiver.clone(), eg);
    }
    if !matches!(storage.value_type(), ValueType::Array | ValueType::Object) {
        eg.exception = Some(make_error_value(
            "InvalidArgumentException",
            "Passed variable is not an array or object",
        ));
        return Ok(());
    }
    if storage.value_type() == ValueType::Object {
        let owner = owner(receiver);
        report_internal_deprecation(
            eg,
            ed,
            &format!(
                "{owner}::{method}(): Using an object as a backing array for {owner} is deprecated, as it allows violating class constraints and invariants"
            ),
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    replace_storage(receiver, storage.clone_for_php_storage(), eg)
}

#[cold]
#[inline(never)]
pub(in crate::stdlib) fn restore_members(
    receiver: &Value,
    members: &PhpArray,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    restore_members_with_policy(receiver, members, ed, eg, true, None)
}

/// Heap/deque native hooks load the same raw member table, but report a
/// rejected readonly slot using their container-specific serialization error.
#[cold]
#[inline(never)]
pub(in crate::stdlib) fn restore_container_members(
    receiver: &Value,
    members: &PhpArray,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    invalid: Option<&str>,
) -> Result<(), VmError> {
    restore_members_with_policy(receiver, members, ed, eg, false, invalid)
}

#[cold]
#[inline(never)]
fn restore_members_with_policy(
    receiver: &Value,
    members: &PhpArray,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    array_wrapper: bool,
    invalid: Option<&str>,
) -> Result<(), VmError> {
    let class_name = receiver
        .as_object()
        .expect("native receiver")
        .class_name
        .to_string();
    for (key, value) in members.iter() {
        let name = match &key {
            ArrayKey::Int(key) => key.to_string(),
            ArrayKey::String(key) => key.clone(),
        };
        let object = receiver.as_object().expect("native receiver");
        let storage_key =
            crate::stdlib::serialization::unserialized_property_storage_key(eg, &object, &name);
        // Native storage is an implementation slot, not a PHP member. A
        // serialized member with the same spelling must not overwrite it.
        let native_collision = array_wrapper && storage_key == array_object_storage_key(&object);
        let old = if native_collision {
            object
                .get_dynamic_property_with_position(&name)
                .map(|(value, _)| value)
        } else {
            object.get_property(&storage_key)
        };
        if !native_collision
            && old.is_some_and(|value| !value.is_undef())
            && let Some(definition) = object
                .property_slot(&storage_key)
                .and_then(|slot| eg.instance_property_definition(object.class_id, slot))
                .filter(|definition| definition.is_readonly)
        {
            let message = format!(
                "Cannot modify readonly property {}::${}",
                definition.declaring_class, definition.name
            );
            drop(object);
            eg.exception = Some(if let Some(invalid) = invalid {
                make_error_value("Exception", invalid)
            } else {
                make_error_value("Error", &message)
            });
            return Ok(());
        }
        let release = old.and_then(|value| release_plan(eg, value));
        let is_new_dynamic = old.is_none()
            && (native_collision || !name.contains('\0'))
            && eg
                .find_class(&class_name)
                .is_some_and(|class| !class.allow_dynamic_properties);
        drop(object);
        if is_new_dynamic {
            let display_name = name.rsplit('\0').next().unwrap_or(&name);
            report_internal_deprecation(
                eg,
                ed,
                &format!(
                    "Creation of dynamic property {class_name}::${display_name} is deprecated"
                ),
            )?;
            if eg.exception.is_some() && array_wrapper {
                return Ok(());
            }
        }
        run_prepared_value_destructor(eg, release)?;
        if eg.exception.is_some() && array_wrapper {
            return Ok(());
        }
        // PHP's native array-wrapper restore copies members directly, unlike
        // ordinary O: property loading. It guards initialized readonly slots,
        // but does not coerce types or attach a new reference type constraint.
        if native_collision {
            receiver
                .as_object_mut()
                .expect("native receiver")
                .set_dynamic_property(&name, value.clone_for_php_storage());
        } else {
            receiver
                .as_object_mut()
                .expect("native receiver")
                .set_property(&storage_key, value.clone_for_php_storage());
        }
        if eg.exception.is_some() {
            // Native container loading commits this member even if its
            // diagnostic/destructor failed. Heap hooks replace that error
            // with their serialization exception; deque hooks propagate it.
            if let Some(invalid) = invalid {
                eg.exception = Some(make_error_value("Exception", invalid));
            }
            return Ok(());
        }
    }
    Ok(())
}

#[cold]
#[inline(never)]
fn modern_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::array(state(arg!(ed, 0), eg)));
}

#[cold]
#[inline(never)]
fn modern_unserialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let input = arg!(ed, 1).dereferenced().clone();
    let Some(data) = input.as_array() else {
        typed_internal_argument_error(
            eg,
            &format!("{}::__unserialize", owner(&receiver)),
            &input,
            1,
            "data",
            "array",
        );
        return Ok(());
    };
    let Some(flags) = data
        .get_int(0)
        .filter(|v| v.value_type() == ValueType::Long)
        .and_then(Value::as_long)
    else {
        ill_typed(eg);
        return Ok(());
    };
    let Some(storage) = data.get_int(1) else {
        ill_typed(eg);
        return Ok(());
    };
    let Some(members) = data
        .get_int(2)
        .filter(|v| v.value_type() == ValueType::Array)
        .and_then(Value::as_array)
    else {
        ill_typed(eg);
        return Ok(());
    };
    let iterator = data.get_int(3);
    if iterator.is_some_and(|v| !matches!(v.value_type(), ValueType::Null | ValueType::String)) {
        ill_typed(eg);
        return Ok(());
    }
    if reject_mutation(&receiver, eg) {
        return Ok(());
    }
    restore_storage(&receiver, flags, storage, ed, eg, "__unserialize")?;
    if eg.exception.is_some() {
        return Ok(());
    }
    restore_members(&receiver, members, ed, eg)?;
    if eg.exception.is_some() {
        return Ok(());
    }
    if let Some(name) = iterator.and_then(Value::as_str) {
        crate::stdlib::autoload::ensure_symbol_loaded(eg, name)?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let class_id = find_class_case_insensitive(eg, name).map(|class| class.class_id);
        let reason = if class_id.is_none() {
            Some("no such class exists")
        } else if !eg.class_is_a(name, "Iterator") {
            Some("this class does not implement the Iterator interface")
        } else {
            None
        };
        if let Some(reason) = reason {
            eg.exception = Some(make_error_value(
                "UnexpectedValueException",
                &format!(
                    "Cannot deserialize {} with iterator class '{name}'; {reason}",
                    owner(&receiver)
                ),
            ));
            return Ok(());
        }
        let mut object = receiver.as_object_mut().expect("native receiver");
        let mut options = object.native_array_options();
        options.iterator_class_id = class_id.expect("validated class");
        object.set_native_array_options(options);
    }
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
fn legacy_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = crate::stdlib::serialization::serialize_array_wrapper(arg!(ed, 0), eg)?;
    ret!(rv, value);
}

#[cold]
#[inline(never)]
fn legacy_unserialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let argument = arg!(ed, 1).dereferenced().clone();
    let Some(input) = typed_internal_string_value_expected(
        ed,
        eg,
        &argument,
        &format!("{}::unserialize", owner(&receiver)),
        0,
        "data",
        "string",
        "string",
    )?
    else {
        return Ok(());
    };
    crate::stdlib::serialization::unserialize_array_wrapper(
        &receiver,
        &input.php_string_bytes().expect("converted payload"),
        ed,
        eg,
    );
    ret!(rv, Value::null());
}

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::new();
    for owner in ["ArrayObject", "ArrayIterator"] {
        for (method, handler, names, hints, result) in [
            (
                "serialize",
                legacy_serialize as InternalFunctionHandler,
                vec![],
                vec![],
                ParamTypeHint::String,
            ),
            (
                "unserialize",
                legacy_unserialize,
                vec!["data"],
                vec![ParamTypeHint::String],
                ParamTypeHint::Void,
            ),
            (
                "__serialize",
                modern_serialize as InternalFunctionHandler,
                vec![],
                vec![],
                ParamTypeHint::Array,
            ),
            (
                "__unserialize",
                modern_unserialize,
                vec!["data"],
                vec![ParamTypeHint::Array],
                ParamTypeHint::Void,
            ),
        ] {
            eg.register_internal_method_contract(
                owner,
                method,
                false,
                names.len() as u32,
                &names,
                hints.clone(),
                result,
                &vec![None; names.len()],
                true,
            );
            let mut function = Box::new(make_internal_method(
                handler,
                names.len() as u32 + 1,
                names.len() as u32,
                names.iter().map(|name| name.to_string()).collect(),
            ));
            function.common.sig.param_type_hints = hints;
            function.handler_validates_types = true;
            let ptr = &function.common as *const FunctionCommon;
            eg.function_table.insert(
                internal_method_display_name(owner, method).to_ascii_lowercase(),
                ptr,
            );
            eg.method_declaring_class.insert(ptr, owner.into());
            eg.register_internal_function_display_name(
                ptr,
                internal_method_display_name(owner, method),
            );
            eg.register_internal_function_reflection_metadata(ptr, vec![None; names.len()], "SPL");
            functions.push(function);
        }
    }
    functions
}
