use super::{DynamicPropertyMap, DynamicPropertyStorage, ObjectLayout, PhpObject, Value};
use std::rc::Rc;

#[test]
fn declared_properties_use_shared_slots() {
    let layout = Rc::new(ObjectLayout::new("Counter", vec!["count".to_string()]));
    let mut object = PhpObject::with_layout(7, layout.clone(), vec![Value::long(1)]);

    assert_eq!(object.set_property("count", Value::long(2)), Some(0));
    assert_eq!(
        object.get_property("count").and_then(Value::as_long),
        Some(2)
    );
    assert!(object.dynamic_properties.is_none());
    assert!(Rc::ptr_eq(&object.property_layout, &layout));
}

#[test]
fn dynamic_properties_are_allocated_lazily() {
    let mut object = PhpObject::with_layout(
        8,
        Rc::new(ObjectLayout::new("Dynamic", Vec::new())),
        Vec::new(),
    );

    assert!(object.dynamic_properties.is_none());
    assert_eq!(object.set_property("extra", Value::long(9)), None);
    assert_eq!(
        object.get_property("extra").and_then(Value::as_long),
        Some(9)
    );
    assert!(object.dynamic_properties.is_some());
}

#[test]
fn decoded_std_classes_share_immutable_metadata() {
    let first = PhpObject::std_class(std::collections::HashMap::new());
    let second = PhpObject::std_class(std::collections::HashMap::new());

    assert_eq!(first.class_name.as_ref(), "stdClass");
    assert!(Rc::ptr_eq(&first.class_name, &second.class_name));
    assert!(Rc::ptr_eq(&first.property_layout, &second.property_layout));
    assert!(first.dynamic_properties.is_none());
    assert!(second.dynamic_properties.is_none());
}

#[test]
fn dynamic_property_map_promotes_small_to_linear_then_indexed() {
    assert!(std::mem::size_of::<DynamicPropertyMap>() <= 176);
    assert_eq!(
        std::mem::size_of::<Option<Box<DynamicPropertyMap>>>(),
        std::mem::size_of::<usize>()
    );

    let mut properties = DynamicPropertyMap::with_capacity(0);
    for (position, key) in ["a", "b", "c"].into_iter().enumerate() {
        properties.insert_owned(key.to_string(), Value::long(position as i64 + 1));
    }
    properties.insert_owned("b".to_string(), Value::long(20));
    assert_eq!(properties.len(), 3);
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Small(_)
    ));

    let mut keys = Vec::new();
    properties.for_each(|key, _| keys.push(key.to_string()));
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(properties.get("b").and_then(Value::as_long), Some(20));

    let cloned = properties.clone();
    assert!(matches!(cloned.storage, DynamicPropertyStorage::Small(_)));

    properties.insert_owned("d".to_string(), Value::long(4));
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Linear(_)
    ));
    for (position, key) in ["e", "f", "g", "h"].into_iter().enumerate() {
        properties.insert_owned(key.to_string(), Value::long(position as i64 + 5));
    }
    assert_eq!(properties.len(), 8);
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Linear(_)
    ));
    assert_eq!(
        properties
            .get_with_position("h")
            .map(|(value, position)| (value.as_long(), position)),
        Some((Some(8), Some(7)))
    );

    let mut keys = Vec::new();
    properties.for_each(|key, _| keys.push(key.to_string()));
    assert_eq!(keys, ["a", "b", "c", "d", "e", "f", "g", "h"]);

    let cloned = properties.clone();
    assert!(matches!(cloned.storage, DynamicPropertyStorage::Linear(_)));
    assert_eq!(cloned.get("h").and_then(Value::as_long), Some(8));

    properties.insert_owned("i".to_string(), Value::long(9));
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Indexed(_)
    ));
    assert_eq!(properties.get("b").and_then(Value::as_long), Some(20));
    assert_eq!(properties.get("i").and_then(Value::as_long), Some(9));
    assert_eq!(
        properties
            .get_with_position("i")
            .map(|(value, position)| (value.as_long(), position)),
        Some((Some(9), Some(8)))
    );

    let DynamicPropertyStorage::Indexed(indexed) = &properties.storage else {
        unreachable!();
    };
    let entry_key = &indexed.entries[8].0;
    let index_key = indexed
        .index
        .keys()
        .find(|key| key.as_ref() == "i")
        .unwrap();
    assert!(Rc::ptr_eq(&entry_key.0, &index_key.0));

    let mut keys = Vec::new();
    properties.for_each(|key, _| keys.push(key.to_string()));
    assert_eq!(keys, ["a", "b", "c", "d", "e", "f", "g", "h", "i"]);

    properties.insert_owned("b".to_string(), Value::long(200));
    *properties.get_mut("b").unwrap() = Value::long(201);
    assert_eq!(
        properties
            .get_with_position("b")
            .map(|(value, position)| (value.as_long(), position)),
        Some((Some(201), Some(1)))
    );
    let cloned = properties.clone();
    assert!(matches!(cloned.storage, DynamicPropertyStorage::Indexed(_)));
    assert_eq!(cloned.get("b").and_then(Value::as_long), Some(201));
    let mut cloned_keys = Vec::new();
    cloned.for_each(|key, _| cloned_keys.push(key.to_string()));
    assert_eq!(cloned_keys, keys);

    assert!(matches!(
        DynamicPropertyMap::with_capacity(4).storage,
        DynamicPropertyStorage::Linear(_)
    ));
    assert!(matches!(
        DynamicPropertyMap::with_capacity(9).storage,
        DynamicPropertyStorage::Indexed(_)
    ));

    let direct = DynamicPropertyMap::from_hash_map(
        (0..9)
            .map(|index| (format!("key{index}"), Value::long(index)))
            .collect(),
    );
    assert!(matches!(direct.storage, DynamicPropertyStorage::Indexed(_)));
    assert_eq!(direct.len(), 9);
    assert_eq!(
        direct
            .get_with_position("key8")
            .map(|(value, position)| (value.as_long(), position.is_some())),
        Some((Some(8), true))
    );
}

#[test]
fn dynamic_property_pair_validates_positions_across_all_storage_tiers() {
    let mut properties = DynamicPropertyMap::with_capacity(0);
    properties.insert_owned("name".to_string(), Value::long(5));
    properties.insert_owned("value".to_string(), Value::long(11));
    properties.insert_owned("extra".to_string(), Value::long(17));

    // Both cached positions intentionally describe the opposite insertion
    // order. Name fallback must resolve each property independently.
    let pair = properties.get_pair_at_positions(["value", "name"], [Some(0), Some(1)]);
    assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
    assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

    let missing = properties.get_pair_at_positions(["value", "missing"], [Some(1), Some(2)]);
    assert!(!missing[0].is_null());
    assert!(missing[1].is_null());

    properties.insert_owned("fourth".to_string(), Value::long(23));
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Linear(_)
    ));
    let pair = properties.get_pair_at_positions(["value", "name"], [Some(99), Some(99)]);
    assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
    assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

    for (key, value) in [
        ("fifth", 29),
        ("sixth", 31),
        ("seventh", 37),
        ("eighth", 41),
    ] {
        properties.insert_owned(key.to_string(), Value::long(value));
    }
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Linear(_)
    ));
    let pair = properties.get_pair_at_positions(["value", "name"], [Some(1), Some(0)]);
    assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
    assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));

    properties.insert_owned("ninth".to_string(), Value::long(43));
    assert!(matches!(
        properties.storage,
        DynamicPropertyStorage::Indexed(_)
    ));
    let pair = properties.get_pair_at_positions(["value", "name"], [Some(8), Some(7)]);
    assert_eq!(unsafe { (*pair[0]).as_long() }, Some(11));
    assert_eq!(unsafe { (*pair[1]).as_long() }, Some(5));
}
