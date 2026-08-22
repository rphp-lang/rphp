//! Associative and user-comparator array set operations.
//!
//! All functions preserve the first array's keys and insertion order. The
//! inputs are snapshotted before a user comparator can re-enter PHP;
//! referenced element cells stay live, while structural mutation detaches
//! normally.

use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
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
    array
        .iter()
        .enumerate()
        .map(|(ordinal, (key, value))| Entry {
            ordinal,
            key,
            value: snapshot_entry_value(value),
        })
        .collect()
}

fn array_type_error(eg: &mut ExecutorGlobals, function: &str, position: usize, value: &Value) {
    let parameter = if position == 1 { " ($array)" } else { "" };
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{function}(): Argument #{position}{parameter} must be of type array, {} given",
            value.dereferenced().diagnostic_type_name()
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

fn key_value(key: &ArrayKey) -> Value {
    match key {
        ArrayKey::Int(value) => Value::long(*value),
        ArrayKey::String(value) => Value::string(value.clone()),
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
            let Some(left) = super::internal_value_to_string(execute_data, eg, left)? else {
                return Ok(None);
            };
            if eg.exception.is_some() {
                return Ok(None);
            }
            let Some(right) = super::internal_value_to_string(execute_data, eg, right)? else {
                return Ok(None);
            };
            if eg.exception.is_some() {
                return Ok(None);
            }
            Ok(Some(left.as_bytes().cmp(right.as_bytes())))
        }
    }
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
        if exact_keys && entry.key != candidate.key {
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
        let Some(ordering) = user_order(eg, callback, key_value(&left.key), key_value(&right.key))?
        else {
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
        let mut result = PhpArray::new();
        for entry in source {
            if keep[entry.ordinal] {
                result.set(entry.key, result_entry_value(&entry.value));
            }
        }
        super::write_return_value(return_pointer, Value::array(result));
        return Ok(());
    }

    let mut result = PhpArray::new();
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
            result.set(entry.key, result_entry_value(&entry.value));
        }
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

    let contains = |array: &PhpArray, key: &ArrayKey| match key {
        ArrayKey::Int(index) => array.get_int(*index).is_some(),
        ArrayKey::String(name) => array.get_str(name).is_some(),
    };
    let mut result = PhpArray::new();
    for (key, value) in first_array.iter() {
        let keep = match kind {
            SetKind::Difference => !comparisons.iter().any(|array| contains(array, &key)),
            SetKind::Intersection => comparisons.iter().all(|array| contains(array, &key)),
        };
        if keep {
            result.set(key, result_entry_value(value));
        }
    }
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
