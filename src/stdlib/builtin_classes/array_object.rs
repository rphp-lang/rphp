//! Raw backing storage for the native ArrayObject family. Object projection is
//! intentionally separate from ordinary property access: PHP deprecates this
//! API precisely because it bypasses visibility, types, readonly and hooks.

use super::*;
use crate::vm::execute::{
    PreparedValueDestructor, prepare_replaced_value_destructor,
    prepare_replaced_value_tree_destructor_with_references, run_prepared_value_destructor,
};
use crate::vm::function::InternalFunctionHandler;
use std::rc::Rc;

mod sorting;
pub(super) use sorting::reject_mutation;

enum Backing {
    Array(Value, &'static str),
    Object(Value),
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn backing(receiver: &Value) -> Option<Backing> {
    let mut owner = receiver.clone();
    let mut seen = Vec::new();
    loop {
        let object = owner.as_object()?;
        let identity = owner.object_identity()?;
        if seen.contains(&identity) {
            drop(object);
            return Some(Backing::Object(owner));
        }
        seen.push(identity);
        let key = array_object_storage_key(&object);
        let Some(value) = object.get_property(key) else {
            drop(object);
            return Some(Backing::Object(owner));
        };
        if value.value_type() == ValueType::Array {
            drop(object);
            return Some(Backing::Array(owner, key));
        }
        let next = value.clone();
        drop(object);
        owner = next;
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn snapshot(receiver: &Value, eg: &ExecutorGlobals, public_only: bool) -> PhpArray {
    let Some(storage) = backing(receiver) else {
        return PhpArray::new();
    };
    let object_value = match storage {
        Backing::Array(owner, key) => {
            return owner
                .as_object()
                .and_then(|o| o.get_property(key).and_then(Value::as_array).cloned())
                .unwrap_or_else(PhpArray::new);
        }
        Backing::Object(owner) => owner,
    };
    let object = object_value.as_object().expect("resolved object backing");
    let mut result = PhpArray::new();
    for slot in eg.instance_property_slots_in_iteration_order(object.class_id) {
        let definition = eg
            .instance_property_definition(object.class_id, slot)
            .expect("declared slot");
        if definition.is_virtual_hook_property()
            || (public_only && definition.visibility != Visibility::Public)
        {
            continue;
        }
        // Native wrapper metadata is not a property of a self-backed view.
        if definition.name == "storage"
            && matches!(
                definition.declaring_class.as_str(),
                "ArrayObject" | "ArrayIterator"
            )
        {
            continue;
        }
        let Some(value) = object
            .get_property_slot(slot)
            .filter(|v| v.value_type() != ValueType::Undef)
        else {
            continue;
        };
        let key = match definition.visibility {
            Visibility::Public => definition.name.clone(),
            Visibility::Protected => format!("\0*\0{}", definition.name),
            Visibility::Private => format!("\0{}\0{}", definition.declaring_class, definition.name),
        };
        result.set_str(&key, value.clone_for_php_storage());
    }
    object.for_each_dynamic_property(|key, value| {
        if value.value_type() != ValueType::Undef && (!public_only || !key.starts_with('\0')) {
            result.set_str(key, value.clone_for_php_storage());
        }
    });
    result
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn count(receiver: &Value, eg: &ExecutorGlobals) -> usize {
    let Some(storage) = backing(receiver) else {
        return 0;
    };
    let owner = match storage {
        Backing::Array(owner, key) => {
            return owner
                .as_object()
                .and_then(|object| {
                    object
                        .get_property(key)
                        .and_then(Value::as_array)
                        .map(PhpArray::len)
                })
                .unwrap_or(0);
        }
        Backing::Object(owner) => owner,
    };
    let object = owner.as_object().expect("resolved object backing");
    let mut count = 0;
    for (slot, value) in object.property_values.iter().enumerate() {
        if value.value_type() == ValueType::Undef {
            continue;
        }
        let Some(definition) = eg.instance_property_definition(object.class_id, slot) else {
            continue;
        };
        if definition.visibility == Visibility::Public && !definition.is_virtual_hook_property() {
            count += 1;
        }
    }
    object.for_each_dynamic_property(|key, value| {
        if !key.starts_with('\0') && value.value_type() != ValueType::Undef {
            count += 1;
        }
    });
    count
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn append(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: Value,
) -> Result<(), VmError> {
    if matches!(backing(arg!(ed, 0)), Some(Backing::Object(_))) {
        let method = arg!(ed, 0).as_object().map_or("ArrayObject", |object| {
            if array_object_storage_key(&object) == ARRAY_ITERATOR_STORAGE {
                "ArrayIterator"
            } else {
                "ArrayObject"
            }
        });
        eg.exception = Some(make_error_value(
            "Error",
            &format!("Cannot append properties to objects, use {method}::offsetSet() instead"),
        ));
        return Ok(());
    }
    offset_set(ed, eg, None, value)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn release_plan(eg: &ExecutorGlobals, value: &Value) -> Option<PreparedValueDestructor> {
    // These raw bucket operations detach the slot rather than writing through
    // a shared PHP reference. Its other aliases still retain the old payload.
    if value.owned_reference_is_aliased() {
        return None;
    }
    let value = value.dereferenced();
    if value.value_type() == ValueType::Array {
        prepare_replaced_value_tree_destructor_with_references(eg, value, 1)
    } else {
        prepare_replaced_value_destructor(eg, value)
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn validate(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
    method: &str,
    owner: &str,
) -> Result<bool, VmError> {
    if value.value_type() == ValueType::Array {
        return Ok(true);
    }
    let Some(_object) = value.as_object() else {
        typed_internal_argument_error(
            eg,
            &format!("{owner}::{method}"),
            value,
            1,
            "array",
            "array",
        );
        return Ok(false);
    };
    drop(_object);
    report_internal_deprecation(
        eg,
        ed,
        &format!(
            "{owner}::{method}(): Using an object as a backing array for {owner} is deprecated, as it allows violating class constraints and invariants"
        ),
    )?;
    Ok(eg.exception.is_none())
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn replace_storage(
    receiver: &Value,
    value: Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(mut object) = receiver.as_object_mut() else {
        return Ok(());
    };
    let key = array_object_storage_key(&object);
    let release = object
        .get_property(key)
        .and_then(|old| release_plan(eg, old));
    object.set_property(key, value);
    drop(object);
    run_prepared_value_destructor(eg, release)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let owner = arg!(ed, 0).as_object().map_or("ArrayObject", |o| {
        if array_object_storage_key(&o) == ARRAY_ITERATOR_STORAGE {
            "ArrayIterator"
        } else {
            "ArrayObject"
        }
    });
    let value = arg_opt!(ed, 1)
        .map(|v| v.dereferenced().clone())
        .unwrap_or_else(|| Value::array(PhpArray::new()));
    if !validate(ed, eg, &value, "__construct", owner)? {
        return Ok(());
    }
    if reject_mutation(arg!(ed, 0), eg) {
        return Ok(());
    }
    replace_storage(arg!(ed, 0), value, eg)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn exchange(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let value = arg!(ed, 1).dereferenced().clone();
    if !validate(ed, eg, &value, "exchangeArray", "ArrayObject")? {
        return Ok(());
    }
    if reject_mutation(arg!(ed, 0), eg) {
        return Ok(());
    }
    let previous = Value::array(snapshot(arg!(ed, 0), eg, false));
    replace_storage(arg!(ed, 0), value, eg)?;
    write_array_mutator_return(ed, rv, eg, previous)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn copy(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    ret!(rv, Value::array(snapshot(arg!(ed, 0), eg, false)));
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn iterator(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let class = eg
        .find_class("ArrayIterator")
        .expect("native iterator registered");
    let mut object = PhpObject::with_layout(
        class.class_id,
        Rc::clone(&class.property_layout),
        class.property_defaults.to_vec(),
    );
    object.set_property(ARRAY_ITERATOR_STORAGE, arg!(ed, 0).clone());
    ret!(rv, Value::object(object));
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::new();
    for (owner, method, handler, required, names, hints, result) in [
        (
            "ArrayObject",
            "exchangeArray",
            exchange as InternalFunctionHandler,
            1,
            vec!["array"],
            vec![ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName("object".into()),
                ParamTypeHint::Array,
            ])],
            ParamTypeHint::Array,
        ),
        (
            "ArrayObject",
            "getArrayCopy",
            copy,
            0,
            vec![],
            vec![],
            ParamTypeHint::Array,
        ),
        (
            "ArrayIterator",
            "getArrayCopy",
            copy,
            0,
            vec![],
            vec![],
            ParamTypeHint::Array,
        ),
        (
            "ArrayObject",
            "getIterator",
            iterator,
            0,
            vec![],
            vec![],
            ParamTypeHint::ClassName("Iterator".into()),
        ),
    ] {
        eg.register_internal_method_contract(
            owner,
            method,
            false,
            required,
            &names,
            hints.clone(),
            result.clone(),
            &vec![None; names.len()],
            true,
        );
        let mut function = Box::new(make_internal_method(
            handler,
            names.len() as u32 + 1,
            required as u32,
            names.iter().map(|n| n.to_string()).collect(),
        ));
        function.common.sig.param_type_hints = hints;
        function.handler_validates_types = true;
        let ptr = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(format!("{owner}::{method}").to_ascii_lowercase(), ptr);
        eg.method_declaring_class.insert(ptr, owner.into());
        eg.register_internal_function_display_name(ptr, format!("{owner}::{method}"));
        eg.register_internal_function_reflection_metadata(ptr, vec![None; names.len()], "SPL");
        functions.push(function);
    }
    functions.extend(sorting::register(eg));
    functions
}

enum PropertySlot {
    Declared(usize),
    Dynamic(String),
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
fn property_slot(object: &PhpObject, key: &ArrayKey, eg: &ExecutorGlobals) -> PropertySlot {
    let name = match key {
        ArrayKey::Int(key) => key.to_string(),
        ArrayKey::String(key) => key.clone(),
    };
    for slot in 0..object.property_values.len() {
        let Some(definition) = eg.instance_property_definition(object.class_id, slot) else {
            continue;
        };
        if definition.is_virtual_hook_property() {
            continue;
        }
        let matches = match definition.visibility {
            Visibility::Public => name == definition.name,
            Visibility::Protected => name.strip_prefix("\0*\0") == Some(definition.name.as_str()),
            Visibility::Private => name
                .strip_prefix('\0')
                .and_then(|n| n.split_once('\0'))
                .is_some_and(|(owner, prop)| {
                    owner == definition.declaring_class && prop == definition.name
                }),
        };
        if matches {
            return PropertySlot::Declared(slot);
        }
    }
    PropertySlot::Dynamic(name)
}

fn object_slot<'a>(object: &'a mut PhpObject, slot: &PropertySlot) -> Option<&'a mut Value> {
    match slot {
        PropertySlot::Declared(slot) => object.get_property_slot_mut(*slot),
        PropertySlot::Dynamic(name) => object.get_dynamic_property_mut(name),
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    key: ArrayKey,
    context: ArrayObjectOffsetGetContext,
) -> Result<(), VmError> {
    let Some(storage) = backing(arg!(ed, 0)) else {
        ret!(rv, Value::null());
    };
    let (owner, array_key) = match storage {
        Backing::Array(owner, key) => (owner, Some(key)),
        Backing::Object(owner) => (owner, None),
    };
    let mut object = owner.as_object_mut().expect("resolved backing owner");
    let object_key = property_slot(&object, &key, eg);
    let is_object = array_key.is_none();
    let slot = if let Some(storage_key) = array_key {
        let array = object
            .get_property_mut(storage_key)
            .and_then(Value::as_array_mut)
            .expect("array backing");
        let normalized = if context == ArrayObjectOffsetGetContext::Mutable {
            array.prepare_string_key_for_write(key.clone(), arg!(ed, 1))
        } else {
            array.normalize_string_key(key.clone(), arg!(ed, 1))
        };
        if context == ArrayObjectOffsetGetContext::Mutable
            && array.get_key_mut(&normalized).is_none()
        {
            array.set(normalized.clone(), Value::null());
        }
        array.get_key_mut(&normalized)
    } else {
        if context == ArrayObjectOffsetGetContext::Mutable {
            match &object_key {
                PropertySlot::Declared(slot)
                    if object
                        .get_property_slot(*slot)
                        .is_some_and(|v| v.value_type() == ValueType::Undef) =>
                {
                    object.property_values[*slot] = Value::null();
                }
                PropertySlot::Dynamic(name) if object.get_dynamic_property_mut(name).is_none() => {
                    object.set_dynamic_property(name, Value::null())
                }
                _ => {}
            }
        }
        object_slot(&mut object, &object_key).filter(|v| v.value_type() != ValueType::Undef)
    };
    let Some(slot) = slot else {
        drop(object);
        let displayed = match key {
            ArrayKey::Int(key) if !is_object => key.to_string(),
            ArrayKey::Int(key) => format!("\"{key}\""),
            ArrayKey::String(key) => format!("\"{key}\""),
        };
        report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("Undefined array key {displayed}"),
        )?;
        ret!(rv, Value::null());
    };
    let result = if slot.is_owned_reference() {
        slot.clone_owned_reference_alias()
    } else if slot.is_reference() {
        slot.clone_closure_capture()
    } else {
        let value = std::mem::replace(slot, Value::undef());
        *slot = Value::owned_reference(value);
        slot.clone_owned_reference_alias()
    };
    ret!(rv, result);
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn offset_set(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    key: Option<ArrayKey>,
    value: Value,
) -> Result<(), VmError> {
    let Some(storage) = backing(arg!(ed, 0)) else {
        return Ok(());
    };
    let (owner, array_key) = match storage {
        Backing::Array(owner, key) => (owner, Some(key)),
        Backing::Object(owner) => (owner, None),
    };
    let mut object = owner.as_object_mut().expect("resolved backing owner");
    let mut release = None;
    if let Some(storage_key) = array_key {
        let array = object
            .get_property_mut(storage_key)
            .and_then(Value::as_array_mut)
            .expect("array backing");
        if let Some(key) = key {
            let key = array.prepare_string_key_for_write(key, arg!(ed, 1));
            if let Some(slot) = array.get_key_mut(&key) {
                release = release_plan(eg, slot);
                *slot = value;
            } else {
                array.set(key, value);
            }
        } else if !array.try_push(value) {
            eg.exception = Some(make_error_value(
                "Error",
                "Cannot add element to the array as the next element is already occupied",
            ));
        }
    } else {
        let key = key.unwrap_or_else(|| {
            let mut next = 0i64;
            object.for_each_dynamic_property(|name, _| {
                if let Ok(key) = name.parse::<i64>() {
                    next = next.max(key.saturating_add(1));
                }
            });
            ArrayKey::Int(next)
        });
        let target = property_slot(&object, &key, eg);
        if let Some(slot) = object_slot(&mut object, &target) {
            release = release_plan(eg, slot);
            // SPL replaces the raw bucket, including its reference wrapper.
            // Ordinary property assignment would write through the cell (or
            // validate/remove typed-reference sources), which is not this
            // deprecated backing-storage API's observable behavior.
            *slot = value;
        } else if let PropertySlot::Dynamic(name) = target {
            object.set_dynamic_property(&name, value);
        }
    }
    drop(object);
    run_prepared_value_destructor(eg, release)
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn offset_exists(
    receiver: &Value,
    key: &ArrayKey,
    source: &Value,
    eg: &ExecutorGlobals,
) -> bool {
    let Some(storage) = backing(receiver) else {
        return false;
    };
    match storage {
        Backing::Array(owner, key_storage) => owner
            .as_object()
            .and_then(|o| {
                o.get_property(key_storage)
                    .and_then(Value::as_array)
                    .map(|a| {
                        array_object_value(a, &a.normalize_string_key(key.clone(), source))
                            .is_some_and(|v| v.dereferenced().value_type() != ValueType::Null)
                    })
            })
            .unwrap_or(false),
        Backing::Object(owner) => {
            let mut object = owner.as_object_mut().expect("resolved object backing");
            let target = property_slot(&object, key, eg);
            object_slot(&mut object, &target).is_some_and(|v| v.value_type() != ValueType::Undef)
        }
    }
}

#[cold]
#[inline(never)]
// SAFETY: compiler-generated code retains its normal calling convention.
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zdiagnostic"))]
pub(super) fn offset_unset(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    key: ArrayKey,
) -> Result<(), VmError> {
    let Some(storage) = backing(arg!(ed, 0)) else {
        return Ok(());
    };
    let (owner, array_key) = match storage {
        Backing::Array(owner, key) => (owner, Some(key)),
        Backing::Object(owner) => (owner, None),
    };
    let mut object = owner.as_object_mut().expect("resolved backing owner");
    let release;
    if let Some(storage_key) = array_key {
        let array = object
            .get_property_mut(storage_key)
            .and_then(Value::as_array_mut)
            .expect("array backing");
        let key = array.normalize_string_key(key, arg!(ed, 1));
        release = array_object_value(array, &key).and_then(|v| release_plan(eg, v));
        array.remove(&key);
    } else {
        let target = property_slot(&object, &key, eg);
        release = object_slot(&mut object, &target).and_then(|v| release_plan(eg, v));
        match target {
            PropertySlot::Declared(slot) => {
                let name = object
                    .property_name_at_slot(slot)
                    .expect("declared property")
                    .to_owned();
                object.unset_property(&name);
            }
            PropertySlot::Dynamic(name) => {
                object.remove_dynamic_property(&name);
            }
        }
    }
    drop(object);
    run_prepared_value_destructor(eg, release)
}
