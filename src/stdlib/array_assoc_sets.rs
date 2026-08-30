//! Associative and user-comparator array set operations.
//!
//! All functions preserve the first array's keys and insertion order. The
//! inputs are snapshotted before a user comparator can re-enter PHP;
//! referenced element cells stay live, while structural mutation detaches
//! normally.

use crate::runtime::ExecutorGlobals;
use crate::value::{
    ArrayKey, PhpArray, Value, ValueType, php_byte_string_bytes, php_byte_string_from_bytes,
};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use std::borrow::Cow;
use std::cmp::Ordering;

#[derive(Clone, Copy)]
enum SetKind {
    Difference,
    Intersection,
}

#[derive(Clone, Copy)]
enum ValueMode {
    Ignore,
    CompareAsString,
    User,
}

struct Entry {
    ordinal: usize,
    key: ArrayKey,
    value: Value,
    external_byte_key: bool,
    utf8_text_key: bool,
}

/// Retain a live reference cell while keeping the implementation-only snapshot
/// out of PHP's observable alias count.
fn snapshot_entry_value(value: &Value) -> Value {
    if value.is_owned_reference() {
        let mut alias = value.clone_owned_reference_alias();
        alias.mark_internal_reference_alias();
        alias
    } else {
        value.clone()
    }
}

/// PHP array set functions preserve a source reference only while that cell
/// still has another PHP-visible owner; a lone wrapper is returned by value.
fn result_entry_value(value: &Value) -> Value {
    if value.is_owned_reference() && value.owned_reference_is_aliased() {
        value.clone_owned_reference_alias()
    } else {
        value.clone()
    }
}

fn variadic_values(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flat_map(|array| array.values().cloned())
        .collect()
}

fn snapshot(array: &PhpArray) -> Vec<Entry> {
    let external_byte_key = array.has_external_byte_keys();
    let utf8_text_key = array.has_utf8_text_keys();
    array
        .iter()
        .enumerate()
        .map(|(ordinal, (key, value))| Entry {
            ordinal,
            key,
            value: snapshot_entry_value(value),
            external_byte_key,
            utf8_text_key,
        })
        .collect()
}

fn array_type_error(eg: &mut ExecutorGlobals, function: &str, position: usize, value: &Value) {
    let parameter = if position == 1 { " ($array)" } else { "" };
    let value = value.dereferenced();
    let actual = match value.value_type() {
        ValueType::True => "true".into(),
        ValueType::False => "false".into(),
        _ => value.diagnostic_type_name(),
    };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{position}{parameter} must be of type array, {} given",
            actual
        ),
    ));
}

fn validated_snapshots(
    first: &Value,
    comparison_values: &[Value],
    eg: &mut ExecutorGlobals,
    function: &str,
) -> Option<(Vec<Entry>, Vec<Vec<Entry>>)> {
    let Some(first_array) = first.dereferenced().as_array() else {
        array_type_error(eg, function, 1, first);
        return None;
    };
    let source = snapshot(first_array);
    let mut comparisons = Vec::with_capacity(comparison_values.len());
    for (index, value) in comparison_values.iter().enumerate() {
        let Some(array) = value.dereferenced().as_array() else {
            array_type_error(eg, function, index + 2, value);
            return None;
        };
        comparisons.push(snapshot(array));
    }
    Some((source, comparisons))
}

fn key_value(entry: &Entry) -> Value {
    match &entry.key {
        ArrayKey::Int(value) => Value::long(*value),
        ArrayKey::String(value) if entry.external_byte_key => {
            Value::binary_string_from_storage(value.clone())
        }
        ArrayKey::String(value) => Value::string(value.clone()),
    }
}

fn string_key_bytes(value: &str, external_byte_key: bool) -> Cow<'_, [u8]> {
    if external_byte_key {
        Cow::Owned(php_byte_string_bytes(value))
    } else {
        Cow::Borrowed(value.as_bytes())
    }
}

fn entry_keys_equal(left: &Entry, right: &Entry) -> bool {
    let left_external_byte_key = left.external_byte_key;
    let right_external_byte_key = right.external_byte_key;
    match (&left.key, &right.key) {
        (ArrayKey::Int(left), ArrayKey::Int(right)) => left == right,
        (ArrayKey::String(left), ArrayKey::String(right)) => {
            string_key_bytes(left, left_external_byte_key)
                == string_key_bytes(right, right_external_byte_key)
        }
        _ => false,
    }
}

fn array_contains_key(array: &PhpArray, key: &ArrayKey, source_external_byte_key: bool) -> bool {
    match key {
        ArrayKey::Int(index) => array.get_int(*index).is_some(),
        ArrayKey::String(name) if array.has_external_byte_keys() == source_external_byte_key => {
            array.get_str(name).is_some()
        }
        ArrayKey::String(name) if array.has_external_byte_keys() => {
            let storage = php_byte_string_from_bytes(name.as_bytes().iter().copied());
            array.get_str(&storage).is_some()
        }
        ArrayKey::String(name) => {
            let bytes = php_byte_string_bytes(name);
            std::str::from_utf8(&bytes)
                .ok()
                .is_some_and(|text| array.get_str(text).is_some())
        }
    }
}

fn user_order(
    eg: &mut ExecutorGlobals,
    callback: &super::ResolvedCallback,
    left: Value,
    right: Value,
) -> Result<Option<Ordering>, VmError> {
    let comparison = super::call_resolved_with_values(eg, callback, &[left, right])?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let comparison = comparison.dereferenced();
    let comparison = comparison.as_array().map_or_else(
        || comparison.to_long_val(),
        |array| i64::from(!array.is_empty()),
    );
    Ok(Some(comparison.cmp(&0)))
}

fn value_order(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
    callback: Option<&super::ResolvedCallback>,
    mode: ValueMode,
) -> Result<Option<Ordering>, VmError> {
    match mode {
        ValueMode::Ignore => Ok(Some(Ordering::Equal)),
        ValueMode::User => user_order(
            eg,
            callback.expect("user value comparison retains its callback"),
            left.clone(),
            right.clone(),
        ),
        ValueMode::CompareAsString => {
            let Some(left) = internal_value_to_php_bytes(execute_data, eg, left)? else {
                return Ok(None);
            };
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(right) = internal_value_to_php_bytes(execute_data, eg, right)? else {
                return Ok(None);
            };
            if eg.exception.is_some() {
                return Ok(None);
            }
            Ok(Some(left.cmp(&right)))
        }
    }
}

/// Stable insertion sort for the cold observable-conversion path. The focused
/// PHP oracle fixes the adjacent comparison order for duplicate values; scalar
/// inputs never enter this quadratic path.
fn observable_sort_entries<F>(entries: &mut [Entry], compare: &mut F) -> Result<bool, VmError>
where
    F: FnMut(&Entry, &Entry) -> Result<Option<Ordering>, VmError>,
{
    for current in 1..entries.len() {
        let mut position = current;
        while position > 0 {
            let Some(ordering) = compare(&entries[position - 1], &entries[position])? else {
                return Ok(false);
            };
            if ordering != Ordering::Greater {
                break;
            }
            entries.swap(position - 1, position);
            position -= 1;
        }
    }
    Ok(true)
}
fn write_entry_result(
    return_pointer: *mut Value,
    entries: impl IntoIterator<Item = Entry>,
    keep: Option<&[bool]>,
) {
    let mut result = PhpArray::new();
    let mut external_byte_keys = false;
    let mut utf8_text_keys = false;
    for entry in entries {
        if keep.is_none_or(|keep| keep[entry.ordinal]) {
            external_byte_keys |= entry.external_byte_key;
            utf8_text_keys |= entry.utf8_text_key;
            result.set(entry.key, result_entry_value(&entry.value));
        }
    }
    if external_byte_keys {
        result.mark_external_byte_keys();
    }
    if utf8_text_keys {
        result.mark_utf8_text_keys();
    }
    super::write_return_value(return_pointer, Value::array(result));
}

fn copy_key_provenance(source: &PhpArray, result: &PhpArray) {
    if source.has_external_byte_keys() {
        result.mark_external_byte_keys();
    }
    if source.has_utf8_text_keys() {
        result.mark_utf8_text_keys();
    }
}

fn ordinary_array_diff(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    first: Value,
    comparison_values: Vec<Value>,
) -> Result<(), VmError> {
    let Some(first_array) = first.dereferenced().as_array() else {
        array_type_error(eg, "array_diff", 1, &first);
        return Ok(());
    };
    let source = snapshot(first_array);

    if source.is_empty() {
        for (index, value) in comparison_values.iter().enumerate() {
            if value.dereferenced().as_array().is_none() {
                array_type_error(eg, "array_diff", index + 2, value);
                return Ok(());
            }
        }
        write_entry_result(return_pointer, source, None);
        return Ok(());
    }

    // PHP 8.5 converts the sole source value before it validates later array
    // arguments and stops converting candidates after the first match.
    if source.len() == 1 {
        let Some(search) = internal_value_to_php_bytes(execute_data, eg, &source[0].value)? else {
            return Ok(());
        };
        let mut found = false;
        for (index, value) in comparison_values.iter().enumerate() {
            let Some(array) = value.dereferenced().as_array() else {
                array_type_error(eg, "array_diff", index + 2, value);
                return Ok(());
            };
            if found {
                continue;
            }
            for candidate in array.values() {
                let Some(rendered) = internal_value_to_php_bytes(execute_data, eg, candidate)?
                else {
                    return Ok(());
                };
                if rendered == search {
                    found = true;
                    break;
                }
            }
        }
        if found {
            super::write_return_value(return_pointer, Value::array(PhpArray::new()));
        } else {
            write_entry_result(return_pointer, source, None);
        }
        return Ok(());
    }

    let mut comparisons = Vec::with_capacity(comparison_values.len());
    let mut comparison_count = 0usize;
    for (index, value) in comparison_values.iter().enumerate() {
        let Some(array) = value.dereferenced().as_array() else {
            array_type_error(eg, "array_diff", index + 2, value);
            return Ok(());
        };
        let entries = snapshot(array);
        comparison_count += entries.len();
        comparisons.push(entries);
    }
    if comparison_count == 0 {
        write_entry_result(return_pointer, source, None);
        return Ok(());
    }

    let mut excluded = Vec::with_capacity(comparison_count);
    for candidates in &comparisons {
        for candidate in candidates {
            let Some(rendered) = internal_value_to_php_bytes(execute_data, eg, &candidate.value)?
            else {
                return Ok(());
            };
            excluded.push(rendered);
        }
    }

    let mut keep = vec![true; source.len()];
    for entry in &source {
        let Some(rendered) = internal_value_to_php_bytes(execute_data, eg, &entry.value)? else {
            return Ok(());
        };
        if excluded.iter().any(|candidate| candidate == &rendered) {
            keep[entry.ordinal] = false;
        }
    }
    write_entry_result(return_pointer, source, Some(&keep));
    Ok(())
}

fn has_observable_string_conversion(entries: &[Entry]) -> bool {
    entries.iter().any(|entry| {
        matches!(
            entry.value.dereferenced().value_type(),
            ValueType::Array | ValueType::Object | ValueType::Closure
        )
    })
}

fn scalar_array_intersection(
    return_pointer: *mut Value,
    eg: &ExecutorGlobals,
    source: Vec<Entry>,
    comparisons: &[Vec<Entry>],
) {
    let rendered_comparisons = comparisons
        .iter()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| scalar_string(&entry.value, eg.precision).unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut keep = vec![true; source.len()];
    for entry in &source {
        let rendered = scalar_string(&entry.value, eg.precision).unwrap_or_default();
        keep[entry.ordinal] = rendered_comparisons
            .iter()
            .all(|candidates| candidates.iter().any(|candidate| candidate == &rendered));
    }
    write_entry_result(return_pointer, source, Some(&keep));
}

fn observable_array_intersection(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    mut source: Vec<Entry>,
    mut comparisons: Vec<Vec<Entry>>,
) -> Result<(), VmError> {
    let mut compare = |left: &Entry, right: &Entry| {
        value_order(
            execute_data,
            eg,
            &left.value,
            &right.value,
            None,
            ValueMode::CompareAsString,
        )
    };
    if !observable_sort_entries(&mut source, &mut compare)? {
        return Ok(());
    }
    for entries in &mut comparisons {
        if !observable_sort_entries(entries, &mut compare)? {
            return Ok(());
        }
    }

    let mut positions = vec![0usize; comparisons.len()];
    let mut keep = vec![true; source.len()];
    let mut source_position = 0usize;
    'source_groups: while source_position < source.len() {
        let group_start = source_position;
        let mut present_everywhere = true;
        for (index, candidates) in comparisons.iter().enumerate() {
            let position = &mut positions[index];
            let mut matched = false;
            while *position < candidates.len() {
                let Some(ordering) = compare(&source[group_start], &candidates[*position])? else {
                    return Ok(());
                };
                match ordering {
                    Ordering::Greater => *position += 1,
                    Ordering::Equal => {
                        matched = true;
                        *position += 1;
                        break;
                    }
                    Ordering::Less => break,
                }
            }
            if !matched {
                present_everywhere = false;
            }
            if !matched && *position == candidates.len() {
                for entry in &source[group_start..] {
                    keep[entry.ordinal] = false;
                }
                break 'source_groups;
            }
            if !matched {
                break;
            }
        }

        loop {
            if !present_everywhere {
                keep[source[source_position].ordinal] = false;
            }
            let previous = source_position;
            source_position += 1;
            if source_position == source.len() {
                break 'source_groups;
            }
            let Some(ordering) = compare(&source[previous], &source[source_position])? else {
                return Ok(());
            };
            if ordering != Ordering::Equal {
                break;
            }
        }
    }

    source.sort_by_key(|entry| entry.ordinal);
    write_entry_result(return_pointer, source, Some(&keep));
    Ok(())
}

fn ordinary_array_intersect(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    first: Value,
    comparison_values: Vec<Value>,
) -> Result<(), VmError> {
    let Some(first_array) = first.dereferenced().as_array() else {
        array_type_error(eg, "array_intersect", 1, &first);
        return Ok(());
    };
    let source = snapshot(first_array);
    let mut comparisons = Vec::with_capacity(comparison_values.len());
    for (index, value) in comparison_values.iter().enumerate() {
        let Some(array) = value.dereferenced().as_array() else {
            array_type_error(eg, "array_intersect", index + 2, value);
            return Ok(());
        };
        comparisons.push(snapshot(array));
    }

    let observable = has_observable_string_conversion(&source)
        || comparisons
            .iter()
            .any(|entries| has_observable_string_conversion(entries));
    if observable {
        observable_array_intersection(execute_data, return_pointer, eg, source, comparisons)
    } else {
        scalar_array_intersection(return_pointer, eg, source, &comparisons);
        Ok(())
    }
}

fn internal_value_to_php_bytes(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    value: &Value,
) -> Result<Option<Vec<u8>>, VmError> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::String {
        return Ok(value.php_string_bytes().map(Cow::into_owned));
    }
    Ok(super::internal_value_to_string(execute_data, eg, value)?.map(String::into_bytes))
}

#[inline(always)]
fn scalar_string(value: &Value, precision: i32) -> Option<Vec<u8>> {
    let value = if value.value_type() == ValueType::Reference {
        value.dereferenced()
    } else {
        value
    };
    if matches!(
        value.value_type(),
        ValueType::Array | ValueType::Object | ValueType::Closure
    ) {
        None
    } else if value.value_type() == ValueType::String {
        value.php_string_bytes().map(Cow::into_owned)
    } else if precision == 14 {
        Some(value.echo_to_string().into_bytes())
    } else {
        Some(value.echo_to_string_with_precision(precision).into_bytes())
    }
}

fn try_scalar_array_diff(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<bool, VmError> {
    let first = arg!(execute_data, 0);
    let Some(source) = first.as_array() else {
        array_type_error(eg, "array_diff", 1, first);
        return Ok(true);
    };
    let Some(arguments) = arg!(execute_data, 1).as_array() else {
        return Ok(false);
    };

    let mut excluded = Vec::new();
    for (index, argument) in arguments.values().enumerate() {
        let Some(array) = argument.dereferenced().as_array() else {
            array_type_error(eg, "array_diff", index + 2, argument);
            return Ok(true);
        };
        for value in array.values() {
            let Some(rendered) = scalar_string(value, eg.precision) else {
                return Ok(false);
            };
            excluded.push(rendered);
        }
    }

    if arguments.is_empty() {
        super::write_return_value(return_pointer, Value::array(source.clone()));
        return Ok(true);
    }

    let mut result = PhpArray::new();
    for (key, value) in source.iter() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        if !excluded.iter().any(|candidate| candidate == &rendered) {
            result.set(key, result_entry_value(value));
        }
    }
    copy_key_provenance(source, &result);
    super::write_return_value(return_pointer, Value::array(result));
    Ok(true)
}

fn try_scalar_array_intersect(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<bool, VmError> {
    let first = arg!(execute_data, 0);
    let Some(source) = first.as_array() else {
        array_type_error(eg, "array_intersect", 1, first);
        return Ok(true);
    };
    let Some(arguments) = arg!(execute_data, 1).as_array() else {
        return Ok(false);
    };

    let mut rendered_comparisons = Vec::with_capacity(arguments.len());
    for (index, argument) in arguments.values().enumerate() {
        let Some(array) = argument.dereferenced().as_array() else {
            array_type_error(eg, "array_intersect", index + 2, argument);
            return Ok(true);
        };
        let mut rendered = Vec::with_capacity(array.len());
        for value in array.values() {
            let Some(value) = scalar_string(value, eg.precision) else {
                return Ok(false);
            };
            rendered.push(value);
        }
        rendered_comparisons.push(rendered);
    }

    let mut result = PhpArray::new();
    for (key, value) in source.iter() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        if rendered_comparisons
            .iter()
            .all(|candidates| candidates.iter().any(|candidate| candidate == &rendered))
        {
            result.set(key, result_entry_value(value));
        }
    }
    copy_key_provenance(source, &result);
    super::write_return_value(return_pointer, Value::array(result));
    Ok(true)
}

fn try_scalar_array_diff_raw(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<bool, VmError> {
    let first = arg!(execute_data, 0);
    let Some(source) = first.as_array() else {
        array_type_error(eg, "array_diff", 1, first);
        return Ok(true);
    };
    if supplied_num_args == 1 {
        if source
            .values()
            .any(|value| scalar_string(value, eg.precision).is_none())
        {
            return Ok(false);
        }
        super::write_return_value(return_pointer, Value::array(source.clone()));
        return Ok(true);
    }

    let second = arg!(execute_data, 1);
    let Some(comparison) = second.as_array() else {
        array_type_error(eg, "array_diff", 2, second);
        return Ok(true);
    };
    let mut excluded = Vec::with_capacity(comparison.len());
    for value in comparison.values() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        excluded.push(rendered);
    }
    let mut result = PhpArray::new();
    for (key, value) in source.iter() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        if !excluded.iter().any(|candidate| candidate == &rendered) {
            result.set(key, result_entry_value(value));
        }
    }
    copy_key_provenance(source, &result);
    super::write_return_value(return_pointer, Value::array(result));
    Ok(true)
}

fn try_scalar_array_intersect_raw(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<bool, VmError> {
    let first = arg!(execute_data, 0);
    let Some(source) = first.as_array() else {
        array_type_error(eg, "array_intersect", 1, first);
        return Ok(true);
    };
    if supplied_num_args == 1 {
        if source
            .values()
            .any(|value| scalar_string(value, eg.precision).is_none())
        {
            return Ok(false);
        }
        super::write_return_value(return_pointer, Value::array(source.clone()));
        return Ok(true);
    }

    let second = arg!(execute_data, 1);
    let Some(comparison) = second.as_array() else {
        array_type_error(eg, "array_intersect", 2, second);
        return Ok(true);
    };
    let mut candidates = Vec::with_capacity(comparison.len());
    for value in comparison.values() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        candidates.push(rendered);
    }
    let mut result = PhpArray::new();
    for (key, value) in source.iter() {
        let Some(rendered) = scalar_string(value, eg.precision) else {
            return Ok(false);
        };
        if candidates.iter().any(|candidate| candidate == &rendered) {
            result.set(key, result_entry_value(value));
        }
    }
    copy_key_provenance(source, &result);
    super::write_return_value(return_pointer, Value::array(result));
    Ok(true)
}

pub(super) fn fn_array_diff_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if try_scalar_array_diff(execute_data, return_pointer, eg)? {
        return Ok(());
    }
    let first = super::owned_argument(execute_data, 0);
    let rest = super::owned_argument(execute_data, 1);
    ordinary_array_diff(
        execute_data,
        return_pointer,
        eg,
        first,
        variadic_values(&rest),
    )
}

pub(super) fn fn_array_intersect_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if try_scalar_array_intersect(execute_data, return_pointer, eg)? {
        return Ok(());
    }
    let first = super::owned_argument(execute_data, 0);
    let rest = super::owned_argument(execute_data, 1);
    ordinary_array_intersect(
        execute_data,
        return_pointer,
        eg,
        first,
        variadic_values(&rest),
    )
}

pub(super) fn fn_array_diff_raw_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    if try_scalar_array_diff_raw(execute_data, return_pointer, eg, supplied_num_args)? {
        return Ok(());
    }
    let first = super::owned_argument(execute_data, 0);
    let comparisons = (supplied_num_args > 1)
        .then(|| super::owned_argument(execute_data, 1))
        .into_iter()
        .collect();
    ordinary_array_diff(execute_data, return_pointer, eg, first, comparisons)
}

pub(super) fn fn_array_intersect_raw_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    supplied_num_args: u32,
) -> Result<(), VmError> {
    if try_scalar_array_intersect_raw(execute_data, return_pointer, eg, supplied_num_args)? {
        return Ok(());
    }
    let first = super::owned_argument(execute_data, 0);
    let comparisons = (supplied_num_args > 1)
        .then(|| super::owned_argument(execute_data, 1))
        .into_iter()
        .collect();
    ordinary_array_intersect(execute_data, return_pointer, eg, first, comparisons)
}

fn entry_matches(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    entry: &Entry,
    candidates: &[Entry],
    exact_keys: bool,
    value_callback: Option<&super::ResolvedCallback>,
    value_mode: ValueMode,
) -> Result<bool, VmError> {
    for candidate in candidates {
        if exact_keys && !entry_keys_equal(entry, candidate) {
            continue;
        }
        let Some(ordering) = value_order(
            execute_data,
            eg,
            &entry.value,
            &candidate.value,
            value_callback,
            value_mode,
        )?
        else {
            return Ok(false);
        };
        if ordering == Ordering::Equal {
            return Ok(true);
        }
    }
    Ok(false)
}

fn user_entry_order(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Entry,
    right: &Entry,
    key_callback: Option<&super::ResolvedCallback>,
    value_callback: Option<&super::ResolvedCallback>,
    value_mode: ValueMode,
) -> Result<Option<Ordering>, VmError> {
    if let Some(callback) = key_callback {
        let Some(ordering) = user_order(eg, callback, key_value(left), key_value(right))? else {
            return Ok(None);
        };
        if ordering != Ordering::Equal {
            return Ok(Some(ordering));
        }
    }
    value_order(
        execute_data,
        eg,
        &left.value,
        &right.value,
        value_callback,
        value_mode,
    )
}

/// Use the PHP 8.5 small-input comparison schedule for the common two-to-five
/// entry cases, then a deterministic stable merge baseline for larger
/// arrays. Both paths sort only the structural snapshot.
fn sort_user_entries(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    entries: &mut Vec<Entry>,
    key_callback: Option<&super::ResolvedCallback>,
    value_callback: Option<&super::ResolvedCallback>,
    value_mode: ValueMode,
) -> Result<bool, VmError> {
    fn compare_at(
        execute_data: *mut ExecuteData,
        eg: &mut ExecutorGlobals,
        entries: &[Entry],
        left: usize,
        right: usize,
        key_callback: Option<&super::ResolvedCallback>,
        value_callback: Option<&super::ResolvedCallback>,
        value_mode: ValueMode,
    ) -> Result<Option<Ordering>, VmError> {
        user_entry_order(
            execute_data,
            eg,
            &entries[left],
            &entries[right],
            key_callback,
            value_callback,
            value_mode,
        )
    }

    if entries.len() < 2 {
        return Ok(true);
    }
    if entries.len() >= 6 {
        let length = entries.len();
        let mut order = (0..length).collect::<Vec<_>>();
        let mut merged = order.clone();
        let mut width = 1;
        while width < length {
            let mut start = 0;
            while start < length {
                let middle = (start + width).min(length);
                let end = (middle + width).min(length);
                let (mut left, mut right, mut output) = (start, middle, start);
                while left < middle && right < end {
                    let Some(ordering) = compare_at(
                        execute_data,
                        eg,
                        entries,
                        order[left],
                        order[right],
                        key_callback,
                        value_callback,
                        value_mode,
                    )?
                    else {
                        return Ok(false);
                    };
                    if ordering == Ordering::Greater {
                        merged[output] = order[right];
                        right += 1;
                    } else {
                        merged[output] = order[left];
                        left += 1;
                    }
                    output += 1;
                }
                while left < middle {
                    merged[output] = order[left];
                    left += 1;
                    output += 1;
                }
                while right < end {
                    merged[output] = order[right];
                    right += 1;
                    output += 1;
                }
                start = end;
            }
            std::mem::swap(&mut order, &mut merged);
            width = width.saturating_mul(2);
        }

        let original = std::mem::take(entries);
        let mut slots = original.into_iter().map(Some).collect::<Vec<_>>();
        entries.reserve(length);
        for index in order {
            entries.push(
                slots[index]
                    .take()
                    .expect("stable sort permutation consumes each entry once"),
            );
        }
        return Ok(true);
    }
    let Some(first) = compare_at(
        execute_data,
        eg,
        entries,
        0,
        1,
        key_callback,
        value_callback,
        value_mode,
    )?
    else {
        return Ok(false);
    };
    if first == Ordering::Greater {
        entries.swap(0, 1);
    }
    if entries.len() == 2 {
        return Ok(true);
    }

    if first == Ordering::Greater {
        let Some(third_to_first) = compare_at(
            execute_data,
            eg,
            entries,
            2,
            0,
            key_callback,
            value_callback,
            value_mode,
        )?
        else {
            return Ok(false);
        };
        if third_to_first == Ordering::Less {
            entries.swap(1, 2);
            entries.swap(0, 1);
        } else {
            let Some(second_to_third) = compare_at(
                execute_data,
                eg,
                entries,
                1,
                2,
                key_callback,
                value_callback,
                value_mode,
            )?
            else {
                return Ok(false);
            };
            if second_to_third == Ordering::Greater {
                entries.swap(1, 2);
            }
        }
    } else {
        let Some(second_to_third) = compare_at(
            execute_data,
            eg,
            entries,
            1,
            2,
            key_callback,
            value_callback,
            value_mode,
        )?
        else {
            return Ok(false);
        };
        if second_to_third == Ordering::Greater {
            entries.swap(1, 2);
            let Some(first_to_second) = compare_at(
                execute_data,
                eg,
                entries,
                0,
                1,
                key_callback,
                value_callback,
                value_mode,
            )?
            else {
                return Ok(false);
            };
            if first_to_second == Ordering::Greater {
                entries.swap(0, 1);
            }
        }
    }

    for index in 3..entries.len() {
        let mut current = index;
        while current > 0 {
            let Some(ordering) = compare_at(
                execute_data,
                eg,
                entries,
                current - 1,
                current,
                key_callback,
                value_callback,
                value_mode,
            )?
            else {
                return Ok(false);
            };
            if ordering != Ordering::Greater {
                break;
            }
            entries.swap(current - 1, current);
            current -= 1;
        }
    }
    Ok(true)
}

fn user_operation_keep_set(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    source: &mut Vec<Entry>,
    comparisons: &mut [Vec<Entry>],
    key_callback: Option<&super::ResolvedCallback>,
    value_callback: Option<&super::ResolvedCallback>,
    kind: SetKind,
    value_mode: ValueMode,
    consume_equal: bool,
) -> Result<Option<Vec<bool>>, VmError> {
    if !sort_user_entries(
        execute_data,
        eg,
        source,
        key_callback,
        value_callback,
        value_mode,
    )? {
        return Ok(None);
    }
    for comparison in comparisons.iter_mut() {
        if !sort_user_entries(
            execute_data,
            eg,
            comparison,
            key_callback,
            value_callback,
            value_mode,
        )? {
            return Ok(None);
        }
    }

    let mut keep = vec![false; source.len()];
    let mut positions = vec![0usize; comparisons.len()];
    for entry in source.iter() {
        let retained = match kind {
            SetKind::Difference => {
                let mut found = false;
                for candidates in comparisons.iter() {
                    let mut position = 0;
                    while position < candidates.len() {
                        let Some(ordering) = user_entry_order(
                            execute_data,
                            eg,
                            entry,
                            &candidates[position],
                            key_callback,
                            value_callback,
                            value_mode,
                        )?
                        else {
                            return Ok(None);
                        };
                        match ordering {
                            Ordering::Less => break,
                            Ordering::Equal => {
                                found = true;
                                break;
                            }
                            Ordering::Greater => position += 1,
                        }
                    }
                    if found {
                        break;
                    }
                }
                !found
            }
            SetKind::Intersection => {
                let mut found_everywhere = true;
                for (index, candidates) in comparisons.iter().enumerate() {
                    let mut found = false;
                    while positions[index] < candidates.len() {
                        let Some(ordering) = user_entry_order(
                            execute_data,
                            eg,
                            entry,
                            &candidates[positions[index]],
                            key_callback,
                            value_callback,
                            value_mode,
                        )?
                        else {
                            return Ok(None);
                        };
                        match ordering {
                            Ordering::Less => break,
                            Ordering::Equal => {
                                if consume_equal {
                                    positions[index] += 1;
                                }
                                found = true;
                                break;
                            }
                            Ordering::Greater => positions[index] += 1,
                        }
                    }
                    if !found {
                        found_everywhere = false;
                        break;
                    }
                }
                found_everywhere
            }
        };
        keep[entry.ordinal] = retained;
    }
    Ok(Some(keep))
}

/// Value-only user comparisons apply one difference decision to an entire
/// equal-value source group. Intersections group only a successful match;
/// unmatched values advance independently. This preserves both duplicate
/// multiplicity and PHP's observable small-array callback schedule.
fn user_value_operation_keep_set(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    source: &mut Vec<Entry>,
    comparisons: &mut [Vec<Entry>],
    callback: &super::ResolvedCallback,
    kind: SetKind,
) -> Result<Option<Vec<bool>>, VmError> {
    if !sort_user_entries(
        execute_data,
        eg,
        source,
        None,
        Some(callback),
        ValueMode::User,
    )? {
        return Ok(None);
    }
    for comparison in comparisons.iter_mut() {
        if !sort_user_entries(
            execute_data,
            eg,
            comparison,
            None,
            Some(callback),
            ValueMode::User,
        )? {
            return Ok(None);
        }
    }

    let mut keep = vec![false; source.len()];
    let mut positions = vec![0usize; comparisons.len()];
    let mut retry_non_less = vec![false; comparisons.len()];
    let mut source_position = 0;
    while source_position < source.len() {
        let retained = match kind {
            SetKind::Difference => {
                let mut found = false;
                for (index, candidates) in comparisons.iter().enumerate() {
                    while positions[index] < candidates.len() {
                        let Some(ordering) = user_entry_order(
                            execute_data,
                            eg,
                            &source[source_position],
                            &candidates[positions[index]],
                            None,
                            Some(callback),
                            ValueMode::User,
                        )?
                        else {
                            return Ok(None);
                        };
                        match ordering {
                            Ordering::Less => break,
                            Ordering::Equal => {
                                positions[index] += 1;
                                found = true;
                                break;
                            }
                            Ordering::Greater => positions[index] += 1,
                        }
                    }
                    if found {
                        break;
                    }
                }
                !found
            }
            SetKind::Intersection => {
                let mut found_everywhere = true;
                for (index, candidates) in comparisons.iter().enumerate() {
                    let mut found = false;
                    while positions[index] < candidates.len() {
                        let Some(mut ordering) = user_entry_order(
                            execute_data,
                            eg,
                            &source[source_position],
                            &candidates[positions[index]],
                            None,
                            Some(callback),
                            ValueMode::User,
                        )?
                        else {
                            return Ok(None);
                        };
                        if retry_non_less[index] && ordering != Ordering::Less {
                            let Some(retried) = user_entry_order(
                                execute_data,
                                eg,
                                &source[source_position],
                                &candidates[positions[index]],
                                None,
                                Some(callback),
                                ValueMode::User,
                            )?
                            else {
                                return Ok(None);
                            };
                            ordering = retried;
                        }
                        retry_non_less[index] = false;
                        match ordering {
                            Ordering::Less => {
                                retry_non_less[index] = true;
                                break;
                            }
                            Ordering::Equal => {
                                positions[index] += 1;
                                found = true;
                                break;
                            }
                            Ordering::Greater => positions[index] += 1,
                        }
                    }
                    if !found {
                        found_everywhere = false;
                        break;
                    }
                }
                found_everywhere
            }
        };

        keep[source[source_position].ordinal] = retained;
        let group_equal_values = matches!(kind, SetKind::Difference) || retained;
        if !group_equal_values {
            source_position += 1;
            continue;
        }

        let group_start = source_position;
        source_position += 1;
        while source_position < source.len() {
            let Some(ordering) = user_entry_order(
                execute_data,
                eg,
                &source[group_start],
                &source[source_position],
                None,
                Some(callback),
                ValueMode::User,
            )?
            else {
                return Ok(None);
            };
            if ordering != Ordering::Equal {
                break;
            }
            keep[source[source_position].ordinal] = retained;
            source_position += 1;
        }
    }
    Ok(Some(keep))
}

fn execute_set_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
    value_mode: ValueMode,
    exact_keys: bool,
    first: Value,
    comparison_values: Vec<Value>,
    key_callback: Option<super::ResolvedCallback>,
    value_callback: Option<super::ResolvedCallback>,
    sorted_callbacks: bool,
    consume_equal: bool,
    group_user_values: bool,
) -> Result<(), VmError> {
    let Some((mut source, mut comparisons)) =
        validated_snapshots(&first, &comparison_values, eg, function)
    else {
        return Ok(());
    };

    if sorted_callbacks {
        let keep = if group_user_values {
            user_value_operation_keep_set(
                execute_data,
                eg,
                &mut source,
                &mut comparisons,
                value_callback
                    .as_ref()
                    .expect("grouped user-value operation retains its callback"),
                kind,
            )?
        } else {
            user_operation_keep_set(
                execute_data,
                eg,
                &mut source,
                &mut comparisons,
                key_callback.as_ref(),
                value_callback.as_ref(),
                kind,
                value_mode,
                consume_equal,
            )?
        };
        let Some(keep) = keep else {
            return Ok(());
        };
        source.sort_by_key(|entry| entry.ordinal);
        write_entry_result(return_pointer, source, Some(&keep));
        return Ok(());
    }

    let mut result = PhpArray::new();
    let mut external_byte_keys = false;
    let mut utf8_text_keys = false;
    for entry in source {
        let keep = match kind {
            SetKind::Difference => {
                let mut found = false;
                for candidates in &comparisons {
                    if entry_matches(
                        execute_data,
                        eg,
                        &entry,
                        candidates,
                        exact_keys,
                        value_callback.as_ref(),
                        value_mode,
                    )? {
                        found = true;
                        break;
                    }
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                }
                !found
            }
            SetKind::Intersection => {
                let mut present_everywhere = true;
                for candidates in &comparisons {
                    if !entry_matches(
                        execute_data,
                        eg,
                        &entry,
                        candidates,
                        exact_keys,
                        value_callback.as_ref(),
                        value_mode,
                    )? {
                        present_everywhere = false;
                        break;
                    }
                    if eg.exception.is_some() {
                        return Ok(());
                    }
                }
                present_everywhere
            }
        };
        if eg.exception.is_some() {
            return Ok(());
        }
        if keep {
            external_byte_keys |= entry.external_byte_key;
            utf8_text_keys |= entry.utf8_text_key;
            result.set(entry.key, result_entry_value(&entry.value));
        }
    }
    if external_byte_keys {
        result.mark_external_byte_keys();
    }
    if utf8_text_keys {
        result.mark_utf8_text_keys();
    }
    super::write_return_value(return_pointer, Value::array(result));
    Ok(())
}

fn ordinary_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
) -> Result<(), VmError> {
    let first = super::owned_argument(execute_data, 0);
    let rest = super::owned_argument(execute_data, 1);
    execute_set_operation(
        execute_data,
        return_pointer,
        eg,
        function,
        kind,
        ValueMode::CompareAsString,
        true,
        first,
        variadic_values(&rest),
        None,
        None,
        false,
        false,
        false,
    )
}

fn ordinary_key_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
) -> Result<(), VmError> {
    let first = arg!(execute_data, 0);
    let comparison_values = arg!(execute_data, 1)
        .as_array()
        .into_iter()
        .flat_map(|array| array.values())
        .collect::<Vec<_>>();
    let Some(first_array) = first.as_array() else {
        array_type_error(eg, function, 1, first);
        return Ok(());
    };
    let mut comparisons = Vec::with_capacity(comparison_values.len());
    for (index, value) in comparison_values.iter().enumerate() {
        let Some(array) = value.dereferenced().as_array() else {
            array_type_error(eg, function, index + 2, value);
            return Ok(());
        };
        comparisons.push(array);
    }

    let source_external_byte_keys = first_array.has_external_byte_keys();
    let mut result = PhpArray::new();
    for (key, value) in first_array.iter() {
        let keep = match kind {
            SetKind::Difference => !comparisons
                .iter()
                .any(|array| array_contains_key(array, &key, source_external_byte_keys)),
            SetKind::Intersection => comparisons
                .iter()
                .all(|array| array_contains_key(array, &key, source_external_byte_keys)),
        };
        if keep {
            result.set(key, result_entry_value(value));
        }
    }
    copy_key_provenance(first_array, &result);
    super::write_return_value(return_pointer, Value::array(result));
    Ok(())
}

fn resolve_user_callback(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    position: usize,
    callback: &Value,
) -> Result<Option<super::ResolvedCallback>, VmError> {
    let Some(resolved) = super::resolve_callback_at_callsite_checked(callback, eg, execute_data)?
    else {
        if eg.exception.is_none() {
            let reason = super::ordinary_callback_invalid_reason(callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!("{function}(): Argument #{position} must be a valid callback, {reason}"),
            ));
        }
        return Ok(None);
    };
    Ok(Some(resolved))
}

fn user_key_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
    value_mode: ValueMode,
) -> Result<(), VmError> {
    let rest = super::owned_argument(execute_data, 1);
    let mut values = variadic_values(&rest);
    if values.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ArgumentCountError",
            &format!("{function}() expects at least 2 arguments, 1 given"),
        ));
        return Ok(());
    }
    let callback_position = values.len() + 1;
    let callback = values
        .pop()
        .expect("variadic user-key operation retains its required callback")
        .dereferenced()
        .clone();
    let Some(resolved) =
        resolve_user_callback(execute_data, eg, function, callback_position, &callback)?
    else {
        return Ok(());
    };

    let first = super::owned_argument(execute_data, 0);
    execute_set_operation(
        execute_data,
        return_pointer,
        eg,
        function,
        kind,
        value_mode,
        false,
        first,
        values,
        Some(resolved),
        None,
        true,
        true,
        false,
    )
}

fn user_value_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
    exact_keys: bool,
) -> Result<(), VmError> {
    let rest = super::owned_argument(execute_data, 1);
    let mut values = variadic_values(&rest);
    if values.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ArgumentCountError",
            &format!("{function}() expects at least 2 arguments, 1 given"),
        ));
        return Ok(());
    }
    let callback_position = values.len() + 1;
    let callback = values
        .pop()
        .expect("variadic user-value operation retains its required callback")
        .dereferenced()
        .clone();
    let Some(resolved) =
        resolve_user_callback(execute_data, eg, function, callback_position, &callback)?
    else {
        return Ok(());
    };

    let first = super::owned_argument(execute_data, 0);
    execute_set_operation(
        execute_data,
        return_pointer,
        eg,
        function,
        kind,
        ValueMode::User,
        exact_keys,
        first,
        values,
        None,
        Some(resolved),
        !exact_keys,
        false,
        !exact_keys,
    )
}

fn user_value_key_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
) -> Result<(), VmError> {
    let rest = super::owned_argument(execute_data, 1);
    let mut values = variadic_values(&rest);
    if values.len() < 2 {
        let given = values.len() + 1;
        eg.exception = Some(crate::value::make_error_value(
            "ArgumentCountError",
            &format!("{function}() expects at least 3 arguments, {given} given"),
        ));
        return Ok(());
    }

    let key_callback_position = values.len() + 1;
    let value_callback_position = values.len();
    let key_callback = values
        .pop()
        .expect("user value/key operation retains its key callback")
        .dereferenced()
        .clone();
    let value_callback = values
        .pop()
        .expect("user value/key operation retains its value callback")
        .dereferenced()
        .clone();
    let Some(resolved_value) = resolve_user_callback(
        execute_data,
        eg,
        function,
        value_callback_position,
        &value_callback,
    )?
    else {
        return Ok(());
    };
    let Some(resolved_key) = resolve_user_callback(
        execute_data,
        eg,
        function,
        key_callback_position,
        &key_callback,
    )?
    else {
        return Ok(());
    };

    let first = super::owned_argument(execute_data, 0);
    execute_set_operation(
        execute_data,
        return_pointer,
        eg,
        function,
        kind,
        ValueMode::User,
        false,
        first,
        values,
        Some(resolved_key),
        Some(resolved_value),
        true,
        true,
        false,
    )
}

#[cold]
pub(super) fn fn_array_diff_assoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ordinary_operation(
        execute_data,
        return_pointer,
        eg,
        "array_diff_assoc",
        SetKind::Difference,
    )
}

#[cold]
pub(super) fn fn_array_intersect_assoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ordinary_operation(
        execute_data,
        return_pointer,
        eg,
        "array_intersect_assoc",
        SetKind::Intersection,
    )
}

#[cold]
pub(super) fn fn_array_diff_uassoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_diff_uassoc",
        SetKind::Difference,
        ValueMode::CompareAsString,
    )
}

#[cold]
pub(super) fn fn_array_intersect_uassoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_intersect_uassoc",
        SetKind::Intersection,
        ValueMode::CompareAsString,
    )
}

#[cold]
pub(super) fn fn_array_diff_ukey(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_diff_ukey",
        SetKind::Difference,
        ValueMode::Ignore,
    )
}

#[cold]
pub(super) fn fn_array_intersect_ukey(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_intersect_ukey",
        SetKind::Intersection,
        ValueMode::Ignore,
    )
}

#[cold]
pub(super) fn fn_array_diff_key_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ordinary_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_diff_key",
        SetKind::Difference,
    )
}

#[cold]
pub(super) fn fn_array_intersect_key_variadic(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ordinary_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_intersect_key",
        SetKind::Intersection,
    )
}

#[cold]
pub(super) fn fn_array_udiff(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_operation(
        execute_data,
        return_pointer,
        eg,
        "array_udiff",
        SetKind::Difference,
        false,
    )
}

#[cold]
pub(super) fn fn_array_uintersect(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_operation(
        execute_data,
        return_pointer,
        eg,
        "array_uintersect",
        SetKind::Intersection,
        false,
    )
}

#[cold]
pub(super) fn fn_array_udiff_assoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_operation(
        execute_data,
        return_pointer,
        eg,
        "array_udiff_assoc",
        SetKind::Difference,
        true,
    )
}

#[cold]
pub(super) fn fn_array_uintersect_assoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_operation(
        execute_data,
        return_pointer,
        eg,
        "array_uintersect_assoc",
        SetKind::Intersection,
        true,
    )
}

#[cold]
pub(super) fn fn_array_udiff_uassoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_udiff_uassoc",
        SetKind::Difference,
    )
}

#[cold]
pub(super) fn fn_array_uintersect_uassoc(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    user_value_key_operation(
        execute_data,
        return_pointer,
        eg,
        "array_uintersect_uassoc",
        SetKind::Intersection,
    )
}
