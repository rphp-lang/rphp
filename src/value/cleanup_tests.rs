use super::{PhpArray, PhpClosure, PhpObject, Value, ValueType};
use std::collections::HashMap;
use std::rc::Rc;

// Derive the verdict from the payload representation, independently of the
// constructor-owned cleanup bit. A reference clone may read its target, so
// check each resulting value's representation rather than the original tag.
fn assert_cleanup(value: &Value) {
    let expected = match value.value_type() {
        ValueType::String | ValueType::Array | ValueType::Object | ValueType::Closure => true,
        ValueType::Reference => value.is_owned_reference(),
        ValueType::Resource => cfg!(feature = "resource-lifetime"),
        _ => false,
    };
    assert_eq!(value.needs_cleanup(), expected, "{value:?}");
}

#[test]
fn constructors_and_copies_preserve_payload_ownership() {
    let mut target = Value::string("borrowed target");
    let array = Value::array(PhpArray::new());
    let values = [
        Value::undef(),
        Value::explicitly_unset_property(),
        Value::releasing_unset_property(),
        Value::null(),
        Value::bool(false),
        Value::bool(true),
        Value::long(i64::MIN),
        Value::double(f64::NAN),
        Value::string("ordinary"),
        Value::shared_string(Rc::new("shared".into())),
        Value::binary_string(&[0, 128, 255]),
        Value::binary_string_from_storage("\u{ff}".into()),
        Value::interned_binary_string_from_storage("\u{ff}".into()),
        Value::interned_string("interned"),
        Value::array(PhpArray::new()),
        Value::array_from_storage_copy(PhpArray::new(), array.as_array().unwrap()),
        Value::object(PhpObject::dynamic("Owner".into(), 0, HashMap::new())),
        Value::deferred_object(PhpObject::dynamic("Deferred".into(), 0, HashMap::new())),
        Value::closure(PhpClosure {
            allocation: crate::request_memory::Allocation::default(),
            object_handle: 0,
            func: std::ptr::null(),
            called_scope_class_id: 0,
            lexical_scope_class_id: 0,
            trait_scope_class_id: 0,
            is_static: true,
            bound_this: None,
            captures: vec![Value::string("capture")],
            static_vars: None,
            has_heap_captures: true,
            scope_is_dummy: false,
        }),
        Value::reference(&mut target),
        Value::owned_reference(Value::long(3)),
        Value::owned_reference(Value::string("owned target")),
        Value::traversable_unpack_value(Value::long(5)),
        Value::reference_foreach_cursor(&array),
    ];
    for mut value in values {
        assert_cleanup(&value);
        assert_cleanup(&value.clone());
        assert_cleanup(&value.clone_for_php_storage());
        if let Some(weak) = value.weak_cycle_handle() {
            assert_cleanup(&weak.upgrade().unwrap());
        }
        if let Some(weak) = value.weak_object_owner() {
            assert_cleanup(&weak.upgrade().unwrap());
        }
        value.mark_internal_argument_snapshot();
        assert_cleanup(&value);
        assert_cleanup(&value.clone());
        value.clear_internal_argument_snapshot();
        value.mark_indirect_property_modification_result();
        assert_cleanup(&value);
        if value.is_owned_reference() {
            value.mark_static_initializer_in_progress();
            value.mark_internal_reference_alias();
            assert_cleanup(&value);
            assert_cleanup(&value.clone_owned_reference_alias());
            assert_cleanup(&value.clone_closure_capture());
            value.unmark_internal_reference_alias();
            value.clear_static_initializer_in_progress();
            assert_cleanup(&value);
        }
    }
    assert_eq!(target.as_str(), Some("borrowed target"));
    assert_eq!(std::mem::size_of::<Value>(), 16);
}

#[test]
fn literal_cow_and_scalar_replacement_preserve_cleanup() {
    let mut literal = Value::array(PhpArray::new());
    literal.mark_immutable_array_literal();
    let mut detached = literal.clone();
    detached
        .as_array_mut()
        .unwrap()
        .push(Value::string("new element"));
    assert_cleanup(&literal);
    assert_cleanup(&detached);
    assert!(literal.as_array().unwrap().is_empty());
    literal.demote_nested_immutable_array_owner();
    assert_cleanup(&literal);

    let mut cell = Value::owned_reference(detached);
    for next in [Value::null(), Value::string("new"), Value::long(7)] {
        cell.assign_dereferenced(next);
        assert_cleanup(&cell);
        assert_cleanup(cell.dereferenced());
        assert_cleanup(&cell.clone());
    }
}

#[test]
fn resource_cleanup_follows_the_active_representation() {
    #[cfg(feature = "resource-lifetime")]
    let value = Value::resource_owner(Rc::new(crate::resource_handle::ResourceHandle::new(
        1,
        7,
        |_, _| {},
    )));
    #[cfg(not(feature = "resource-lifetime"))]
    let value = Value::resource(7);
    assert_cleanup(&value);
    assert_cleanup(&value.clone());
    assert_eq!(value.as_resource_id(), Some(7));
    #[cfg(feature = "resource-lifetime")]
    assert_cleanup(&value.weak_resource().unwrap().upgrade().unwrap());
}
