//! Associative and user-key-comparator array set operations.
//!
//! All six functions preserve the first array's keys and insertion order. The
//! inputs are snapshotted before a user comparator can re-enter PHP; referenced
//! element cells stay live, while structural mutation detaches normally.

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

fn value_matches(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Value,
    right: &Value,
) -> Result<bool, VmError> {
    let Some(left) = super::internal_value_to_string(execute_data, eg, left)? else {
        return Ok(false);
    };
    if eg.exception.is_some() {
        return Ok(false);
    }
    let Some(right) = super::internal_value_to_string(execute_data, eg, right)? else {
        return Ok(false);
    };
    Ok(eg.exception.is_none() && left == right)
}

fn entry_matches(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    entry: &Entry,
    candidates: &[Entry],
    callback: Option<&super::ResolvedCallback>,
    value_mode: ValueMode,
) -> Result<bool, VmError> {
    for candidate in candidates {
        let keys_equal = if let Some(callback) = callback {
            let comparison = super::call_resolved_with_values(
                eg,
                callback,
                &[key_value(&entry.key), key_value(&candidate.key)],
            )?;
            if eg.exception.is_some() {
                return Ok(false);
            }
            comparison.to_long_val() == 0
        } else {
            entry.key == candidate.key
        };
        if !keys_equal {
            continue;
        }
        if matches!(value_mode, ValueMode::Ignore)
            || value_matches(execute_data, eg, &entry.value, &candidate.value)?
        {
            return Ok(true);
        }
        if eg.exception.is_some() {
            return Ok(false);
        }
    }
    Ok(false)
}

fn user_entry_order(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    left: &Entry,
    right: &Entry,
    callback: &super::ResolvedCallback,
    value_mode: ValueMode,
) -> Result<Option<Ordering>, VmError> {
    let comparison = super::call_resolved_with_values(
        eg,
        callback,
        &[key_value(&left.key), key_value(&right.key)],
    )?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let comparison = comparison.to_long_val();
    if comparison != 0 {
        return Ok(Some(comparison.cmp(&0)));
    }
    if matches!(value_mode, ValueMode::Ignore) {
        return Ok(Some(Ordering::Equal));
    }

    let Some(left) = super::internal_value_to_string(execute_data, eg, &left.value)? else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(right) = super::internal_value_to_string(execute_data, eg, &right.value)? else {
        return Ok(None);
    };
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(Some(left.as_bytes().cmp(right.as_bytes())))
}

/// Use the PHP 8.5 small-input comparison schedule for the common two-to-five
/// entry cases, then a deterministic stable merge baseline for larger
/// arrays. Both paths sort only the structural snapshot.
fn sort_user_entries(
    execute_data: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    entries: &mut Vec<Entry>,
    callback: &super::ResolvedCallback,
    value_mode: ValueMode,
) -> Result<bool, VmError> {
    fn compare_at(
        execute_data: *mut ExecuteData,
        eg: &mut ExecutorGlobals,
        entries: &[Entry],
        left: usize,
        right: usize,
        callback: &super::ResolvedCallback,
        value_mode: ValueMode,
    ) -> Result<Option<Ordering>, VmError> {
        user_entry_order(
            execute_data,
            eg,
            &entries[left],
            &entries[right],
            callback,
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
                        callback,
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
    let Some(first) = compare_at(execute_data, eg, entries, 0, 1, callback, value_mode)? else {
        return Ok(false);
    };
    if first == Ordering::Greater {
        entries.swap(0, 1);
    }
    if entries.len() == 2 {
        return Ok(true);
    }

    if first == Ordering::Greater {
        let Some(third_to_first) =
            compare_at(execute_data, eg, entries, 2, 0, callback, value_mode)?
        else {
            return Ok(false);
        };
        if third_to_first == Ordering::Less {
            entries.swap(1, 2);
            entries.swap(0, 1);
        } else {
            let Some(second_to_third) =
                compare_at(execute_data, eg, entries, 1, 2, callback, value_mode)?
            else {
                return Ok(false);
            };
            if second_to_third == Ordering::Greater {
                entries.swap(1, 2);
            }
        }
    } else {
        let Some(second_to_third) =
            compare_at(execute_data, eg, entries, 1, 2, callback, value_mode)?
        else {
            return Ok(false);
        };
        if second_to_third == Ordering::Greater {
            entries.swap(1, 2);
            let Some(first_to_second) =
                compare_at(execute_data, eg, entries, 0, 1, callback, value_mode)?
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
                callback,
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
    callback: &super::ResolvedCallback,
    kind: SetKind,
    value_mode: ValueMode,
) -> Result<Option<Vec<bool>>, VmError> {
    if !sort_user_entries(execute_data, eg, source, callback, value_mode)? {
        return Ok(None);
    }
    for comparison in comparisons.iter_mut() {
        if !sort_user_entries(execute_data, eg, comparison, callback, value_mode)? {
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
                            callback,
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
                            callback,
                            value_mode,
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

fn execute_set_operation(
    execute_data: *mut ExecuteData,
    return_pointer: *mut Value,
    eg: &mut ExecutorGlobals,
    function: &str,
    kind: SetKind,
    value_mode: ValueMode,
    first: Value,
    comparison_values: Vec<Value>,
    callback: Option<super::ResolvedCallback>,
) -> Result<(), VmError> {
    let Some((mut source, mut comparisons)) =
        validated_snapshots(&first, &comparison_values, eg, function)
    else {
        return Ok(());
    };

    if let Some(callback) = callback.as_ref() {
        let Some(keep) = user_operation_keep_set(
            execute_data,
            eg,
            &mut source,
            &mut comparisons,
            callback,
            kind,
            value_mode,
        )?
        else {
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
                    if entry_matches(execute_data, eg, &entry, candidates, None, value_mode)? {
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
                    if !entry_matches(execute_data, eg, &entry, candidates, None, value_mode)? {
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
        first,
        variadic_values(&rest),
        None,
    )
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
    let Some(resolved) = super::resolve_callback_at_callsite_checked(&callback, eg, execute_data)?
    else {
        if eg.exception.is_none() {
            let reason = super::ordinary_callback_invalid_reason(&callback, eg);
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "{function}(): Argument #{callback_position} must be a valid callback, {reason}"
                ),
            ));
        }
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
        first,
        values,
        Some(resolved),
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
