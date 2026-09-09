//! Iterator consumers share their advancement protocol, but count/apply never
//! fetch current/key and keyless projection never invokes key(). Native array
//! cursors use the same live primitive as foreach, without PHP call frames.
use super::*;
use crate::value::make_error_value;

fn source(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    allow_array: bool,
) -> Option<Value> {
    let source = owned_argument(ed, 0);
    if (allow_array && source.as_array().is_some())
        || source
            .as_object()
            .is_some_and(|o| eg.class_is_a(&o.class_name, "Traversable"))
    {
        return Some(source);
    }
    typed_internal_argument_error(
        eg,
        function,
        &source,
        1,
        "iterator",
        if allow_array {
            "Traversable|array"
        } else {
            "Traversable"
        },
    );
    None
}

fn resolve_iterator(source: &Value, eg: &mut ExecutorGlobals) -> Result<Option<Value>, VmError> {
    let mut iterator = source.clone();
    let mut seen = Vec::new();
    loop {
        let Some(object) = iterator.as_object() else {
            return Ok(None);
        };
        let class = object.class_name.to_string();
        drop(object);
        if !eg.class_is_a(&class, "IteratorAggregate") {
            return Ok(Some(iterator));
        }
        let identity = iterator.object_identity().expect("object iterator");
        if seen.contains(&identity) {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(None);
        }
        seen.push(identity);
        let next =
            call_object_protocol_method(eg, &iterator, "IteratorAggregate", "getIterator", &[])?;
        if eg.exception.is_some() {
            return Ok(None);
        }
        let Some(next) = next.filter(|value| {
            value
                .as_object()
                .is_some_and(|o| eg.class_is_a(&o.class_name, "Traversable"))
        }) else {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(None);
        };
        iterator = next;
    }
}

fn protocol(eg: &mut ExecutorGlobals, iterator: &Value, method: &str) -> Result<Value, VmError> {
    Ok(
        call_object_protocol_method(eg, iterator, "Iterator", method, &[])?
            .unwrap_or_else(Value::null),
    )
}

// Keep one advancement/error-order body for all consumers. A borrowed visitor
// needs neither allocation nor three copies of the public protocol loop.
#[inline(never)]
fn walk(
    eg: &mut ExecutorGlobals,
    source: &Value,
    values: bool,
    keys: bool,
    consume: &mut dyn FnMut(&mut ExecutorGlobals, Value, Value) -> Result<bool, VmError>,
) -> Result<(), VmError> {
    let Some(iterator) = resolve_iterator(source, eg)? else {
        return Ok(());
    };
    let native = uses_native_iterator_protocol(&iterator, eg);
    if !native {
        protocol(eg, &iterator, "rewind")?;
    }
    let mut first = true;
    let projection = match (keys, values) {
        (false, false) => NativeIteratorProjection::None,
        (true, false) => NativeIteratorProjection::Key,
        (false, true) => NativeIteratorProjection::Value,
        (true, true) => NativeIteratorProjection::Both,
    };
    while eg.exception.is_none() {
        let (key, value) = if native {
            let movement = if first {
                NativeIteratorMove::Rewind
            } else {
                NativeIteratorMove::Next
            };
            let Some(entry) = native_iterator_projected_entry(&iterator, movement, projection, eg)
            else {
                break;
            };
            // count/apply do not fetch current/key in the PHP protocol.
            // Do not create aliases that could delay release in the callback.
            entry
        } else {
            if !first {
                protocol(eg, &iterator, "next")?;
            }
            if eg.exception.is_some() {
                break;
            }
            let valid = protocol(eg, &iterator, "valid")?;
            if eg.exception.is_some() || !valid.is_truthy() {
                break;
            }
            let value = if values {
                protocol(eg, &iterator, "current")?
            } else {
                Value::null()
            };
            if eg.exception.is_some() {
                break;
            }
            let key = if keys {
                protocol(eg, &iterator, "key")?
            } else {
                Value::null()
            };
            if eg.exception.is_some() {
                break;
            }
            (key, value)
        };
        first = false;
        if !consume(eg, key, value)? {
            break;
        }
    }
    Ok(())
}

pub(super) fn to_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = source(ed, eg, "iterator_to_array", true) else {
        ret!(rv, Value::null());
    };
    let preserve = if arg_opt!(ed, 1).is_some() {
        let Some(value) =
            typed_internal_bool_argument(ed, eg, "iterator_to_array", 1, "preserve_keys")?
        else {
            ret!(rv, Value::null());
        };
        value
    } else {
        true
    };
    if let Some(array) = source.as_array() {
        if preserve {
            ret!(rv, Value::array(array.clone()));
        }
    } else {
        let Some(iterator) = resolve_iterator(&source, eg)? else {
            ret!(rv, Value::null());
        };
        if uses_native_iterator_protocol(&iterator, eg)
            && let Some((_, Some(array))) =
                consume_native_iterator_array(&iterator, eg, Some(preserve))
        {
            ret!(rv, Value::array(array));
        }
        return to_array_from_iterator(ed, rv, eg, iterator, preserve);
    }
    to_array_from_iterator(ed, rv, eg, source, preserve)
}

fn to_array_from_iterator(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    source: Value,
    preserve: bool,
) -> Result<(), VmError> {
    let mut result = PhpArray::new();
    let mut insert = |eg: &mut ExecutorGlobals, key: Value, value: Value| {
        let value = value.clone_for_php_storage();
        if preserve {
            match crate::vm::execute::value_to_array_key(&key) {
                Ok(index) => {
                    let index = result.prepare_string_key_for_write(index, &key);
                    result.set(index, value);
                }
                Err(_) => {
                    eg.exception = Some(make_error_value("TypeError", "Illegal offset type"));
                }
            }
        } else {
            result.push(value);
        }
        Ok(eg.exception.is_none())
    };
    if let Some(array) = source.as_array() {
        for (key, value) in array.iter() {
            let key = match key {
                ArrayKey::Int(key) => Value::long(key),
                ArrayKey::String(key) if array.has_external_byte_keys() => {
                    Value::binary_string_from_storage(key)
                }
                ArrayKey::String(key) => Value::string(key),
            };
            if !insert(eg, key, value.clone())? {
                break;
            }
        }
    } else {
        walk(eg, &source, true, preserve, &mut insert)?;
    }
    if eg.exception.is_some() {
        ret!(rv, Value::null());
    }
    ret!(rv, Value::array(result));
}

pub(super) fn count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = source(ed, eg, "iterator_count", true) else {
        ret!(rv, Value::null());
    };
    let mut count = 0;
    if let Some(array) = source.as_array() {
        count = array.len() as i64;
    } else {
        let Some(iterator) = resolve_iterator(&source, eg)? else {
            ret!(rv, Value::null());
        };
        if uses_native_iterator_protocol(&iterator, eg)
            && let Some((count, _)) = consume_native_iterator_array(&iterator, eg, None)
        {
            ret!(rv, Value::long(count as i64));
        }
        walk(eg, &iterator, false, false, &mut |_, _, _| {
            count += 1;
            Ok(true)
        })?;
    }
    ret!(rv, Value::long(count));
}

pub(super) fn apply(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(source) = source(ed, eg, "iterator_apply", false) else {
        ret!(rv, Value::null());
    };
    let callback = owned_argument(ed, 1);
    let Some(resolved) = resolve_callback_at_callsite_checked(&callback, eg, ed)? else {
        if eg.exception.is_none() {
            eg.exception = Some(make_error_value(
                "TypeError",
                &format!(
                    "iterator_apply(): Argument #2 ($callback) must be a valid callback, {}",
                    ordinary_callback_invalid_reason(&callback, eg)
                ),
            ));
        }
        ret!(rv, Value::null());
    };
    let arguments = arg_opt!(ed, 2)
        .map(|v| v.dereferenced().clone())
        .unwrap_or_else(Value::null);
    if arguments.value_type() != ValueType::Null && arguments.as_array().is_none() {
        typed_internal_argument_error(eg, "iterator_apply", &arguments, 3, "args", "?array");
        ret!(rv, Value::null());
    }
    let empty = PhpArray::new();
    let arguments = arguments.as_array().unwrap_or(&empty);
    let mut count = 0;
    walk(eg, &source, false, false, &mut |eg, _, _| {
        let result =
            call_resolved_borrowed_with_php_array_at(eg, &resolved, arguments, true, None)?;
        if eg.exception.is_some() {
            return Ok(false);
        }
        count += 1;
        Ok(result.is_truthy())
    })?;
    ret!(rv, Value::long(count));
}
