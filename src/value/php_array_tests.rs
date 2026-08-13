use std::rc::Rc;

use super::{
    ArrayEntryKey, ArrayKey, ArrayStorage, IntIndexValue, NativeLongArraySetContext, PhpArray,
    Value, native_indexed_long_lookup, native_long_array_set, native_long_array_set_deferred,
};

#[test]
fn hash_entry_layout_stays_compact() {
    assert_eq!(std::mem::size_of::<ArrayEntryKey>(), 16);
    assert_eq!(
        std::mem::size_of::<IntIndexValue>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(std::mem::size_of::<(ArrayEntryKey, Value)>(), 32);
    assert_eq!(std::mem::size_of::<Option<(ArrayEntryKey, Value)>>(), 32);
    assert_eq!(std::mem::size_of::<ArrayStorage>(), 112);
    // PHP's reset/current/next/prev/key API requires one persistent cursor per
    // array. Keep the resulting allocation envelope explicit so later fields
    // cannot grow every array unnoticed.
    assert_eq!(std::mem::size_of::<PhpArray>(), 128);
}

#[test]
fn packed_long_chunks_preserve_keys_and_reject_other_storage() {
    let mut packed = PhpArray::new();
    assert!(packed.reserve_packed_long_appends(1_000));
    assert!(matches!(
        &packed.storage,
        ArrayStorage::Packed(values) if values.capacity() >= 1_000
    ));
    assert!(packed.push_packed_long_chunk(&[-2, 0, 7]));
    packed.push(Value::long(9));
    assert_eq!(packed.len(), 4);
    assert_eq!(packed.get_int(0).and_then(Value::as_long), Some(-2));
    assert_eq!(packed.get_int(2).and_then(Value::as_long), Some(7));
    assert_eq!(packed.get_int(3).and_then(Value::as_long), Some(9));

    let mut hashed = PhpArray::new();
    hashed.set_str("key", Value::long(1));
    assert!(!hashed.reserve_packed_long_appends(1_000));
    assert!(!hashed.push_packed_long_chunk(&[2, 3]));
    assert_eq!(hashed.len(), 1);
}

#[test]
fn integer_index_compact_long_payload_stays_exact_across_mutation() {
    let keys = [107, -4, 91, 33, 205, 17, 409, 73, 301];
    let mut array = PhpArray::new();
    for (position, key) in keys.into_iter().enumerate() {
        array.set_int(key, Value::long(position as i64 - 4));
    }

    let ArrayStorage::Hash {
        entries, int_index, ..
    } = &array.storage
    else {
        panic!("irregular integer keys should materialize the canonical index");
    };
    for (position, key) in keys.into_iter().enumerate() {
        let indexed = *int_index.get(&key).unwrap();
        assert_eq!(indexed.position(), position);
        assert_eq!(indexed.cached_long(), Some(position as i64 - 4));
        assert_eq!(entries[position].1.as_long(), indexed.cached_long());
    }

    array.set_int(33, Value::long(i64::MAX));
    let ArrayStorage::Hash { int_index, .. } = &array.storage else {
        unreachable!();
    };
    let wide = *int_index.get(&33).unwrap();
    assert_eq!(wide.position(), 3);
    assert_eq!(wide.cached_long(), None);
    assert_eq!(array.get_indexed_long(33), Some(i64::MAX));

    array.set_int(33, Value::long(IntIndexValue::LONG_MIN));
    let ArrayStorage::Hash { int_index, .. } = &array.storage else {
        unreachable!();
    };
    assert_eq!(
        int_index.get(&33).unwrap().cached_long(),
        Some(IntIndexValue::LONG_MIN)
    );

    *array.get_int_mut(33).unwrap() = Value::long(987_654_321);
    let ArrayStorage::Hash { int_index, .. } = &array.storage else {
        unreachable!();
    };
    assert_eq!(int_index.get(&33).unwrap().cached_long(), None);
    assert_eq!(array.get_indexed_long(33), Some(987_654_321));

    let cloned = array.clone();
    assert_eq!(cloned.get_indexed_long(33), Some(987_654_321));
    assert!(array.remove(&ArrayKey::Int(-4)));
    for key in keys.into_iter().filter(|key| *key != -4) {
        assert_eq!(
            array.get_int(key).and_then(Value::as_long),
            cloned.get_int(key).and_then(Value::as_long)
        );
    }
}

#[test]
fn native_integer_lookup_context_is_exact_and_preserves_failed_output() {
    let keys = [107, -4, 91, 33, 205, 17, 409, 73, 301];
    let mut array = PhpArray::new();
    for (position, key) in keys.into_iter().enumerate() {
        array.set_int(key, Value::long(position as i64 - 4));
    }
    array.set_int(33, Value::long(i64::MAX));
    array.set_int(205, Value::string("not a long"));

    let context = array.native_indexed_long_lookup_context().unwrap();
    let mut output = -999;
    assert_eq!(
        unsafe { native_indexed_long_lookup(&context, 91, &mut output) },
        1
    );
    assert_eq!(output, -2);

    assert_eq!(
        unsafe { native_indexed_long_lookup(&context, 33, &mut output) },
        1
    );
    assert_eq!(output, i64::MAX);

    output = 777;
    assert_eq!(
        unsafe { native_indexed_long_lookup(&context, 999, &mut output) },
        0
    );
    assert_eq!(output, 777);
    assert_eq!(
        unsafe { native_indexed_long_lookup(&context, 205, &mut output) },
        0
    );
    assert_eq!(output, 777);
}

#[test]
fn native_integer_lookup_context_rejects_progression_only_hashes() {
    let mut array = PhpArray::with_hash_capacity(9);
    for key in 0..9 {
        array.set_int(key, Value::long(key));
    }

    assert!(array.native_indexed_long_lookup_context().is_none());
}

#[test]
fn native_integer_store_uses_canonical_structural_mutation() {
    let mut array = PhpArray::new();
    let keys = [107, -4, 91, 33, 205, 17, 409, 73, 301];
    for (position, key) in keys.into_iter().enumerate() {
        assert_eq!(
            unsafe { native_long_array_set(&mut array, key, position as i64 - 4) },
            1
        );
    }
    assert_eq!(
        unsafe { native_long_array_set(&mut array, 33, i64::MAX) },
        1
    );
    for (position, key) in keys.into_iter().enumerate() {
        let expected = if key == 33 {
            i64::MAX
        } else {
            position as i64 - 4
        };
        assert_eq!(array.get_int(key).and_then(Value::as_long), Some(expected));
    }
    assert_eq!(
        unsafe { native_long_array_set(std::ptr::null_mut(), 1, 2) },
        0
    );
}

#[test]
fn native_integer_store_applies_reservation_after_small_hash_promotion() {
    let mut array = PhpArray::new();
    array.set_int(107, Value::long(0));
    let mut context = NativeLongArraySetContext::new(&mut array, 1_000);
    for (position, key) in [-4, 91, 33].into_iter().enumerate() {
        assert_eq!(
            unsafe { native_long_array_set_deferred(&mut context, key, position as i64 + 1) },
            1
        );
    }
    assert_eq!(context.reserve_remaining, 0);
    let ArrayStorage::Hash {
        entries, int_index, ..
    } = &array.storage
    else {
        panic!("four irregular keys should promote to indexed hash storage");
    };
    assert!(entries.capacity() >= entries.len() + 1_000);
    assert!(int_index.capacity() >= int_index.len() + 1_000);
}

#[test]
fn indexed_integer_write_reservation_is_bounded_to_existing_hash_tiers() {
    let mut packed = PhpArray::new();
    assert!(!packed.reserve_indexed_int_writes(1_000));
    assert!(matches!(packed.storage, ArrayStorage::Packed(_)));

    let keys = [107, -4, 91, 33, 205, 17, 409, 73, 301];
    let mut irregular = PhpArray::new();
    for (position, key) in keys.into_iter().enumerate() {
        irregular.set_int(key, Value::long(position as i64));
    }
    assert!(irregular.reserve_indexed_int_writes(1_000));
    let ArrayStorage::Hash {
        entries,
        int_index,
        verified_int_prefix,
        ..
    } = &irregular.storage
    else {
        panic!("irregular integer keys should materialize hash storage");
    };
    assert_eq!(*verified_int_prefix, 0);
    assert!(entries.capacity() >= entries.len() + 1_000);
    assert!(int_index.capacity() >= int_index.len() + 1_000);

    let mut progression = PhpArray::new();
    for position in 0..9 {
        progression.set_int(1_000_000 + position * 7, Value::long(position));
    }
    assert!(progression.reserve_indexed_int_writes(1_000));
    let ArrayStorage::Hash {
        entries,
        int_index,
        verified_int_prefix,
        ..
    } = &progression.storage
    else {
        panic!("wide progression should use hash storage");
    };
    assert_eq!(*verified_int_prefix, entries.len());
    assert!(entries.capacity() >= entries.len() + 1_000);
    assert!(int_index.is_empty());
    assert_eq!(int_index.capacity(), 0);
}

#[test]
fn hash_entry_and_string_index_share_key_allocation() {
    let mut array = PhpArray::with_hash_capacity(9);
    array.set_str("shared", Value::long(7));

    let ArrayStorage::Hash {
        entries, str_index, ..
    } = &array.storage
    else {
        panic!("string key should select hash storage");
    };
    let ArrayEntryKey::String(entry_key) = &entries[0].0 else {
        panic!("entry should retain its string key");
    };
    let index_key = str_index.keys().next().unwrap();
    assert!(Rc::ptr_eq(&entry_key.0, &index_key.0));
}

#[test]
fn small_hash_promotes_to_linear_before_the_general_index() {
    let mut array = PhpArray::with_deferred_hash_capacity(3);
    array.set_str("a", Value::long(1));
    array.set_int(8, Value::long(2));
    array.set_str("c", Value::long(3));
    assert!(matches!(&array.storage, ArrayStorage::SmallHash(_)));

    array.set_str("a", Value::long(10));
    assert!(matches!(&array.storage, ArrayStorage::SmallHash(_)));
    array.set_streamed_owned_str("d".to_string(), Value::long(4));
    assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
    array.set_str("e", Value::long(5));
    array.set_str("f", Value::long(6));
    array.set_str("g", Value::long(7));
    array.set_str("h", Value::long(8));
    assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
    array.set_streamed_owned_str("d".to_string(), Value::long(40));
    assert!(matches!(&array.storage, ArrayStorage::LinearHash(_)));
    array.set_streamed_owned_str("i".to_string(), Value::long(9));
    assert!(matches!(&array.storage, ArrayStorage::Hash { .. }));
    assert_eq!(array.get_str("a").and_then(Value::as_long), Some(10));
    assert_eq!(array.get_int(8).and_then(Value::as_long), Some(2));
    assert_eq!(array.get_str("c").and_then(Value::as_long), Some(3));
    assert_eq!(array.get_str("d").and_then(Value::as_long), Some(40));
    assert_eq!(
        array.iter().map(|(key, _)| key).collect::<Vec<ArrayKey>>(),
        vec![
            ArrayKey::String("a".to_string()),
            ArrayKey::Int(8),
            ArrayKey::String("c".to_string()),
            ArrayKey::String("d".to_string()),
            ArrayKey::String("e".to_string()),
            ArrayKey::String("f".to_string()),
            ArrayKey::String("g".to_string()),
            ArrayKey::String("h".to_string()),
            ArrayKey::String("i".to_string()),
        ]
    );
}

#[test]
fn linear_hash_supports_clone_remove_shift_and_pop() {
    let mut original = PhpArray::with_deferred_hash_capacity(6);
    original.set_str("first", Value::long(1));
    original.set_int(10, Value::long(2));
    original.set_str("third", Value::long(3));
    original.set_int(20, Value::long(4));
    assert!(matches!(&original.storage, ArrayStorage::LinearHash(_)));

    let mut changed = original.clone();
    assert!(changed.remove(&ArrayKey::String("third".to_string())));
    assert_eq!(changed.shift().and_then(|value| value.as_long()), Some(1));
    assert_eq!(changed.pop().and_then(|value| value.as_long()), Some(4));
    assert_eq!(changed.get_int(0).and_then(Value::as_long), Some(2));

    assert_eq!(original.len(), 4);
    assert_eq!(original.get_str("first").and_then(Value::as_long), Some(1));
    assert_eq!(original.get_str("third").and_then(Value::as_long), Some(3));
    assert_eq!(original.get_int(10).and_then(Value::as_long), Some(2));
    assert_eq!(original.get_int(20).and_then(Value::as_long), Some(4));
}

#[test]
fn repeated_linear_string_reads_build_and_mutations_invalidate_the_lazy_index() {
    let mut array = PhpArray::with_deferred_hash_capacity(8);
    for key in ["a", "b", "c", "d", "e", "f", "g", "h"] {
        array.set_str(key, Value::long(1));
    }
    let ArrayStorage::LinearHash(linear) = &array.storage else {
        panic!("eight entries should retain bounded linear storage");
    };
    assert!(linear.str_index.get().is_none());

    for _ in 0..4 {
        assert_eq!(array.get_str("h").and_then(Value::as_long), Some(1));
    }
    let ArrayStorage::LinearHash(linear) = &array.storage else {
        unreachable!();
    };
    assert!(linear.str_index.get().is_some());

    assert!(array.remove(&ArrayKey::String("b".to_string())));
    let ArrayStorage::LinearHash(linear) = &array.storage else {
        unreachable!();
    };
    assert!(linear.str_index.get().is_none());
    assert_eq!(array.get_str("h").and_then(Value::as_long), Some(1));
}

#[test]
fn string_position_hint_validates_layout_changes() {
    let mut array = PhpArray::with_hash_capacity(3);
    array.set_str("first", Value::long(1));
    array.set_str("target", Value::long(2));
    array.set_str("last", Value::long(3));

    let (position, value) = array.get_str_with_position("target").unwrap();
    assert_eq!(position, 1);
    assert_eq!(value.as_long(), Some(2));
    assert_eq!(
        array
            .get_positioned_str("target", position)
            .and_then(Value::as_long),
        Some(2)
    );

    array.remove(&ArrayKey::String("first".to_string()));
    assert!(array.get_positioned_str("target", position).is_none());
    let (new_position, value) = array.get_str_with_position("target").unwrap();
    assert_eq!(new_position, 0);
    assert_eq!(value.as_long(), Some(2));
}

#[test]
fn string_value_key_reuses_source_allocation_and_keeps_cow() {
    let mut key = Value::string("shared");
    let original_ptr = key.string_rc_ptr().unwrap();
    let mut array = PhpArray::with_hash_capacity(1);
    array.set_str_value(&key, Value::long(7));

    let Some((ArrayEntryKey::String(entry_key), _)) = array.hash_entry_at(0) else {
        panic!("entry should retain its string key");
    };
    assert_eq!(Rc::as_ptr(&entry_key.0), original_ptr);

    unsafe { key.as_string_mut().unwrap().push_str("-changed") };
    assert_eq!(key.as_str(), Some("shared-changed"));
    assert_eq!(array.get_str("shared").and_then(Value::as_long), Some(7));
}

#[test]
fn value_iterator_preserves_order_for_packed_and_hash_arrays() {
    let mut packed = PhpArray::new();
    packed.push(Value::long(1));
    packed.push(Value::long(2));
    assert_eq!(
        packed
            .values()
            .filter_map(Value::as_long)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );

    let mut hash = PhpArray::new();
    hash.set_int(7, Value::long(3));
    hash.set_str("name", Value::long(4));
    hash.set_int(-2, Value::long(5));
    assert_eq!(
        hash.values().filter_map(Value::as_long).collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    assert_eq!(hash.values().next_back().and_then(Value::as_long), Some(5));
}

#[test]
fn string_key_detection_uses_array_storage_metadata() {
    let mut packed = PhpArray::new();
    packed.push(Value::long(1));
    assert!(!packed.has_string_keys());

    let mut integer_hash = PhpArray::new();
    integer_hash.set_int(7, Value::long(2));
    assert!(!integer_hash.has_string_keys());

    integer_hash.set_str("name", Value::long(3));
    assert!(integer_hash.has_string_keys());
}

#[test]
fn integer_lookup_routing_distinguishes_contiguous_and_irregular_hashes() {
    let mut contiguous = PhpArray::new();
    contiguous.set_int(100, Value::long(1));
    contiguous.set_int(101, Value::long(2));
    contiguous.set_str("sentinel", Value::long(3));
    assert!(contiguous.prefers_positional_int_lookup());

    let mut irregular = PhpArray::new();
    irregular.set_int(100, Value::long(4));
    irregular.set_int(107, Value::long(5));
    irregular.set_int(-3, Value::long(6));
    assert!(!irregular.prefers_positional_int_lookup());
    assert_eq!(irregular.integer_position_hint(), None);
    assert_eq!(
        irregular.get_indexed_int(107).and_then(Value::as_long),
        Some(5)
    );
    assert_eq!(
        irregular
            .get_positioned_int(107, 100, 7)
            .and_then(Value::as_long),
        Some(5)
    );
    assert_eq!(
        irregular
            .get_positioned_int(-3, 100, 7)
            .and_then(Value::as_long),
        Some(6)
    );
    assert!(irregular.get_positioned_int(101, 100, 7).is_none());
}

#[test]
#[cfg(feature = "quick-loops")]
fn indexed_lookup_position_revalidates_ordered_cursor_after_mutation() {
    let keys = [11, 30, 31, 70, -4, 900, 2, 88, 1234, -90];
    let mut array = PhpArray::new();
    for (position, key) in keys.into_iter().enumerate() {
        array.set_int(key, Value::long(position as i64));
    }

    for (position, key) in keys.into_iter().enumerate() {
        let (indexed_position, value) = array
            .get_indexed_int_with_position(key)
            .expect("indexed key should exist");
        assert_eq!(indexed_position, position);
        assert_eq!(value.as_long(), Some(position as i64));
        assert_eq!(
            array
                .get_ordered_int_at(position, key)
                .and_then(Value::as_long),
            Some(position as i64)
        );
        assert!(array.get_ordered_int_at(position, key + 1).is_none());
    }

    assert!(array.remove(&ArrayKey::Int(70)));
    let (position, value) = array
        .get_indexed_int_with_position(-4)
        .expect("shifted key should retain its rebuilt position");
    assert_eq!(position, 3);
    assert_eq!(value.as_long(), Some(4));
    assert_eq!(
        array
            .get_ordered_int_at(position, -4)
            .and_then(Value::as_long),
        Some(4)
    );
}

#[test]
fn integer_index_handles_offset_and_irregular_hash_keys() {
    let mut array = PhpArray::new();
    for key in 1_000_000..1_000_100 {
        array.set_int(key, Value::long(key * 2));
    }
    array.set_str("separator", Value::long(7));
    array.set_int(-11, Value::long(22));
    array.set_int(9_000_007, Value::long(33));

    assert_eq!(
        array.get_int(1_000_000).and_then(Value::as_long),
        Some(2_000_000)
    );
    assert_eq!(
        array.get_int(1_000_099).and_then(Value::as_long),
        Some(2_000_198)
    );
    assert_eq!(array.get_int(-11).and_then(Value::as_long), Some(22));
    assert_eq!(array.get_int(9_000_007).and_then(Value::as_long), Some(33));
    assert!(array.get_int(1_000_101).is_none());
}

#[test]
fn integer_position_hint_accepts_negative_stride_and_rejects_irregular_layout() {
    let mut descending = PhpArray::new();
    for key in [100, 93, 86, 79, 72, 65, 58, 51] {
        descending.set_int(key, Value::long(key));
    }
    assert_eq!(descending.integer_position_hint(), Some((100, -7)));
    assert_eq!(
        descending
            .get_positioned_int(58, 100, -7)
            .and_then(Value::as_long),
        Some(58)
    );

    let mut irregular = PhpArray::new();
    for key in [10, 30, 31, 70, -4, 900, 2, 88] {
        irregular.set_int(key, Value::long(key));
    }
    assert_eq!(irregular.integer_position_hint(), None);
}

#[test]
fn integer_position_hint_routes_regular_suffix_after_irregular_prefix() {
    let mut array = PhpArray::new();
    for key in [11, 30, 31, 70, -4, 900, 2, 88] {
        array.set_int(key, Value::long(-1));
    }
    for key in [100, 107, 114, 121, 128, 135, 142, 149] {
        array.set_int(key, Value::long(key));
    }

    // The regular suffix starts at entry position 8, so its virtual key
    // at entry position 0 is 100 - 8 * 7 = 44.
    assert_eq!(array.integer_position_hint(), Some((44, 7)));
    assert_eq!(
        array
            .get_positioned_int(100, 44, 7)
            .and_then(Value::as_long),
        Some(100)
    );
    assert_eq!(
        array
            .get_positioned_int(149, 44, 7)
            .and_then(Value::as_long),
        Some(149)
    );
    assert_eq!(
        array.get_positioned_int(30, 44, 7).and_then(Value::as_long),
        Some(-1)
    );

    array.set_int(9_999, Value::long(-1));
    assert_eq!(array.integer_position_hint(), None);
}

#[test]
#[cfg(feature = "quick-loops")]
fn exact_ordered_int_layout_tracks_structural_changes() {
    let mut transitioned = PhpArray::new();
    for value in 1..=16 {
        transitioned.push(Value::long(value));
    }
    transitioned.set_str("sentinel", Value::long(0));
    let layout = transitioned.exact_ordered_int_layout().unwrap();
    for (key, expected) in [(0, 1), (7, 8), (15, 16)] {
        let found = unsafe { layout.positioned_value(key) };
        assert_eq!(
            found.and_then(|value| unsafe { (*value).as_long() }),
            Some(expected)
        );
    }
    assert!(unsafe { layout.positioned_value(16) }.is_none());

    let cloned = transitioned.clone();
    assert!(cloned.exact_ordered_int_layout().is_some());
    transitioned.set_int(7, Value::long(70));
    assert!(transitioned.exact_ordered_int_layout().is_some());
    transitioned.remove(&ArrayKey::Int(3));
    assert!(transitioned.exact_ordered_int_layout().is_none());

    let mut sparse = PhpArray::with_deferred_hash_capacity(0);
    for offset in 0..9 {
        sparse.set_streamed_int(30 + offset * 7, Value::long(offset));
    }
    assert!(sparse.exact_ordered_int_layout().is_none());
    assert_eq!(sparse.integer_position_hint(), Some((30, 7)));
    sparse.set_streamed_int(93, Value::long(9));
    assert_eq!(
        sparse
            .get_positioned_int(93, 30, 7)
            .and_then(Value::as_long),
        Some(9)
    );

    let mut irregular = PhpArray::new();
    for key in [11, 30, 31, 70, -4, 900, 2, 88] {
        irregular.set_int(key, Value::long(-1));
    }
    for key in [100, 107, 114, 121, 128, 135, 142, 149] {
        irregular.set_int(key, Value::long(key));
    }
    assert!(irregular.exact_ordered_int_layout().is_none());
    assert_eq!(irregular.integer_position_hint(), Some((44, 7)));
}

#[test]
fn regular_integer_hash_index_materializes_only_when_needed() {
    let mut regular = PhpArray::new();
    for offset in 0..12 {
        regular.set_int(100 + offset * 7, Value::long(offset));
    }
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &regular.storage
    else {
        panic!("wide regular integer keys should use hash storage");
    };
    assert_eq!(*verified_int_prefix, 12);
    assert!(int_index.is_empty());
    assert_eq!(regular.get_int(149).and_then(Value::as_long), Some(7));
    assert!(regular.get_int(150).is_none());

    regular.set_int(149, Value::long(70));
    regular.set_str("sentinel", Value::long(-1));
    assert_eq!(regular.get_int(149).and_then(Value::as_long), Some(70));
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &regular.storage
    else {
        unreachable!();
    };
    assert_eq!(*verified_int_prefix, 12);
    assert!(int_index.is_empty());

    regular.set_int(184, Value::long(12));
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &regular.storage
    else {
        unreachable!();
    };
    assert_eq!(*verified_int_prefix, 0);
    assert_eq!(int_index.len(), 13);
    assert_eq!(regular.get_int(100).and_then(Value::as_long), Some(0));
    assert_eq!(regular.get_int(184).and_then(Value::as_long), Some(12));

    let mut popped = PhpArray::new();
    for offset in 0..12 {
        popped.set_int(-50 + offset * 3, Value::long(offset));
    }
    assert_eq!(popped.pop().and_then(|value| value.as_long()), Some(11));
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &popped.storage
    else {
        unreachable!();
    };
    assert_eq!(*verified_int_prefix, 11);
    assert!(int_index.is_empty());

    assert_eq!(popped.shift().and_then(|value| value.as_long()), Some(0));
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &popped.storage
    else {
        unreachable!();
    };
    assert_eq!(*verified_int_prefix, 10);
    assert!(int_index.is_empty());
    assert_eq!(popped.get_int(0).and_then(Value::as_long), Some(1));

    assert!(popped.remove(&ArrayKey::Int(4)));
    let ArrayStorage::Hash {
        int_index,
        verified_int_prefix,
        ..
    } = &popped.storage
    else {
        unreachable!();
    };
    assert_eq!(*verified_int_prefix, 0);
    assert_eq!(int_index.len(), 9);
    assert!(popped.get_int(4).is_none());
    assert_eq!(popped.get_int(5).and_then(Value::as_long), Some(6));
}

#[test]
fn integer_index_remains_valid_after_remove_and_clone() {
    let mut array = PhpArray::new();
    for key in [17, 3, 9001, -4, 42] {
        array.set_int(key, Value::long(key));
    }
    assert!(array.remove(&ArrayKey::Int(9001)));

    let cloned = array.clone();
    for key in [17, 3, -4, 42] {
        assert_eq!(cloned.get_int(key).and_then(Value::as_long), Some(key));
    }
    assert!(cloned.get_int(9001).is_none());
}
