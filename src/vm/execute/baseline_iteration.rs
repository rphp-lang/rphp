// Kept in the execute module through include! so this structural split does not change visibility or code generation.

/// A protocol consumer is an object-store owner distinct from its public
/// Iterator. Its existing foreach TMP supplies all normal/abrupt cleanup
/// edges. One private, pooled slot keeps its edge visible to release/GC without
/// allocating dynamic properties, a native sidecar or a name per traversal.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn foreach_iterator_owner(
    iterator: &Value,
    eg: &ExecutorGlobals,
    already_has_native_consumer: bool,
) -> Value {
    thread_local! {
        static LAYOUT: std::rc::Rc<crate::value::ObjectLayout> = std::rc::Rc::new(
            crate::value::ObjectLayout::new("", vec!["\0iterator".into()])
        );
    }
    LAYOUT.with(|layout| {
        let owner = PhpObject::with_layout_from_defaults(
            0, std::rc::Rc::clone(layout), std::slice::from_ref(iterator),
        );
        // A native cursor materialized specifically for this traversal, or a
        // registered weak iterator cursor, already is the PHP-visible
        // consumer. The private ownership envelope must not publish a second
        // object-store handle of its own.
        if already_has_native_consumer || eg.weak_iterator_allows_references(iterator) {
            Value::deferred_object(owner)
        } else {
            Value::object(owner)
        }
    })
}

#[inline(always)]
fn foreach_owned_iterator(value: &Value) -> *const Value {
    let object = value.as_object().expect("protocol consumer owns an object");
    object.get_property_slot(0).expect("protocol consumer retains its Iterator") as *const Value
}

/// The private envelope has exactly one edge and cannot have PHP hooks.
/// Prove a bounded callback-free child shape, independently of Rc counts:
/// multiple consumers can retire together and hold every remaining alias.
#[inline]
fn foreach_owner_has_plain_source(object: &PhpObject, eg: &ExecutorGlobals) -> bool {
    object.class_name.is_empty()
        && object.get_property_slot(0)
            .is_some_and(|source| foreach_source_is_plain(source, eg, 4, &mut 32))
}

/// Composite cursors may retain a shallow list of other scalar-backed cursors.
/// Bound both depth and visited values; flat scalar arrays need no traversal.
/// This read-only proof allocates no graph bookkeeping. References, closures,
/// deep/cyclic graphs and every callback-capable value retain the complete
/// alias-aware planner, even when they currently have other owners.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn foreach_source_is_plain(source: &Value, eg: &ExecutorGlobals, depth: u8, remaining: &mut u8) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    match source.value_type() {
        ValueType::Object => {
            if depth == 0 || value_requires_vm_release(eg, source) {
                return false;
            }
            source.as_object().is_some_and(|object| {
                let mut plain = true;
                object.for_each_owned_value(|value| {
                    plain = plain && foreach_source_is_plain(value, eg, depth - 1, remaining);
                });
                plain
            })
        }
        ValueType::Array => source.as_array().is_some_and(|array| {
            !array.may_require_nested_release()
                || depth != 0 && array.values().all(|value| foreach_source_is_plain(value, eg, depth - 1, remaining))
        }),
        ValueType::Reference | ValueType::Closure => false,
        ValueType::Resource => !source.needs_vm_resource_release(),
        _ => true,
    }
}

#[inline]
fn assign_foreach_cv(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    cv: u32,
    value: Value,
) -> Result<(), VmError> {
    // SAFETY: `cv` is compiler-allocated in the active frame. Assignment may
    // follow a reference target outside the frame, so only direct CV writes use
    // frame bitmap bookkeeping.
    unsafe {
        let slot = (*frame).cv_mut(cv) as *mut Value;
        let target = if (*slot).is_reference() {
            (*slot).as_ref_ptr()
        } else {
            slot
        };
        let op_array = (*frame).op_array();
        let mirrored_global_name = (target == slot)
            .then(|| {
                let root_frame = (*frame).prev_execute_data.is_null();
                let mirrored_variables = if root_frame {
                    &op_array.main_scope_vars
                } else {
                    &op_array.global_vars
                };
                mirrored_variables
                    .iter()
                    .find(|(candidate, _)| *candidate == cv)
                    .and_then(|(_, name)| {
                        eg.globals
                            .get(name)
                            .filter(|global| {
                                !global.is_reference()
                                    && global.weak_object_identity()
                                        == (&*target).weak_object_identity()
                            })
                            .map(|_| name.as_str())
                    })
            })
            .flatten();
        let replaced_references = 1 + usize::from(mirrored_global_name.is_some());
        // Scalar/string replacement has no PHP release work. The slot write
        // still performs Rust/bitmap cleanup; references and container owners
        // must retain the canonical destructor/lifecycle planner.
        let destructor = if ((*target).value_type() as u8) >= ValueType::Array as u8 {
            prepare_replaced_value_destructor_with_references(eg, &*target, replaced_references)
        } else { None };
        if target == slot {
            frame_slot_set(frame, slot, value);
        } else {
            slot_set(target, value);
        }
        if let Some(global_name) = mirrored_global_name {
            globals_set(&mut eg.globals, global_name, (&*target).clone());
        }
        if destructor.is_some() {
            run_prepared_value_destructor(eg, destructor)?;
        }
    }
    Ok(())
}

fn unpack_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    _is_root_frame: bool,
    class: &str,
    message: &str,
) -> Result<ColdResult<'a>, VmError> {
    let error = make_error_value(class, message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    Ok(match throw_in_frame(eg, frame, error)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

fn unpack_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    is_root_frame: bool,
    message: &str,
) -> Result<ColdResult<'a>, VmError> {
    unpack_throw(
        eg,
        frame,
        op_array,
        instruction_index,
        is_root_frame,
        "Error",
        message,
    )
}

fn append_call_unpack_entry(
    target: &mut PhpArray,
    key: ArrayKey,
    value: Value,
    external_byte_key: bool,
    seen_named_in_source: &mut bool,
) -> Result<(), String> {
    match key {
        ArrayKey::Int(_) => {
            if *seen_named_in_source {
                return Err(
                    "Cannot use positional argument after named argument during unpacking"
                        .to_string(),
                );
            }
            target.push(value);
        }
        ArrayKey::String(name) => {
            let name = if external_byte_key && !name.is_ascii() {
                let source = Value::binary_string_from_storage(name.clone());
                match target.prepare_string_key_for_write(ArrayKey::String(name), &source) {
                    ArrayKey::String(name) => name,
                    ArrayKey::Int(_) => {
                        unreachable!("non-ASCII named-argument keys cannot canonicalize to int")
                    }
                }
            } else {
                name
            };
            *seen_named_in_source = true;
            if target.get_str(&name).is_some() {
                return Err(format!(
                    "Named parameter ${name} overwrites previous argument"
                ));
            }
            target.set_str(&name, value);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TraversableUnpackKind {
    Arguments,
    Array,
}

impl TraversableUnpackKind {
    fn value(self, value: Value) -> Value {
        match self {
            Self::Arguments => Value::traversable_unpack_value(value),
            Self::Array => value,
        }
    }

    fn key_error(self) -> &'static str {
        match self {
            Self::Arguments => "Keys must be of type int|string during argument unpacking",
            Self::Array => "Keys must be of type int|string during array unpacking",
        }
    }
}

fn traversable_unpack_key(
    value: &Value,
    kind: TraversableUnpackKind,
) -> Result<(ArrayKey, bool), String> {
    let value = value.dereferenced();
    if let Some(key) = value.as_long() {
        Ok((ArrayKey::Int(key), false))
    } else if let Some(key) = value.as_str() {
        // Traversable keys do not pass through PhpArray::set(), so apply the
        // same canonical decimal-key rule explicitly before array/call unpack.
        // PHP treats a Generator key such as "100" as a positional argument,
        // while a non-canonical decimal spelling remains a named key.
        Ok(match canonical_decimal_array_key(key) {
            Some(key) => (ArrayKey::Int(key), false),
            None => (
                ArrayKey::String(key.to_string()),
                value.is_binary_string(),
            ),
        })
    } else {
        Err(kind.key_error().to_string())
    }
}

fn collect_generator_unpack(
    eg: &mut ExecutorGlobals,
    value: &Value,
    kind: TraversableUnpackKind,
) -> Result<Vec<(ArrayKey, Value, bool)>, VmError> {
    let generator = value
        .as_object_rc()
        .and_then(|object| object.borrow().generator.clone())
        .ok_or_else(|| VmError::Fatal("Generator object has no generator state".to_string()))?;
    let mut entries = Vec::new();

    loop {
        let state = generator.borrow().state;
        if state == crate::vm::generator::GeneratorState::Created
            || state == crate::vm::generator::GeneratorState::Suspended
        {
            if state == crate::vm::generator::GeneratorState::Suspended
                && generator.borrow().rewindable
            {
                generator.borrow_mut().rewindable = false;
            }
            match resume_generator(eg, &generator, Value::null())? {
                GeneratorResumeOutcome::Advanced => {}
                GeneratorResumeOutcome::Threw(exception) => {
                    eg.exception = Some(exception);
                    return Ok(entries);
                }
            }
        }

        let data = generator.borrow();
        if data.state == crate::vm::generator::GeneratorState::Completed {
            break;
        }
        let (key, external_byte_key) = match traversable_unpack_key(&data.key, kind) {
            Ok(key) => key,
            Err(message) => {
                eg.exception = Some(make_error_value("Error", &message));
                return Ok(entries);
            }
        };
        entries.push((
            key,
            kind.value(data.value.dereferenced().clone()),
            external_byte_key,
        ));
    }
    Ok(entries)
}

fn collect_unpack_traversable(
    eg: &mut ExecutorGlobals,
    source: &Value,
    kind: TraversableUnpackKind,
) -> Result<Option<Vec<(ArrayKey, Value, bool)>>, VmError> {
    let Some(object) = source.as_object() else {
        return Ok(None);
    };
    let mut class_name = object.class_name.to_string();
    drop(object);
    if !eg.class_is_a(&class_name, "Traversable") {
        return Ok(None);
    }

    let mut iterable = source.clone();
    let mut aggregate_identities = Vec::new();
    while eg.class_is_a(&class_name, "IteratorAggregate") {
        let identity = iterable.object_identity().unwrap_or(0);
        if aggregate_identities.contains(&identity) {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class_name}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(Some(Vec::new()));
        }
        aggregate_identities.push(identity);
        let Some(next) = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "IteratorAggregate",
            "getIterator",
            &[],
        )? else {
            return Err(VmError::Fatal(format!(
                "Call to undefined method {class_name}::getIterator()"
            )));
        };
        if eg.exception.is_some() {
            return Ok(Some(Vec::new()));
        }
        iterable = next;
        let Some(object) = iterable.as_object() else {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class_name}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(Some(Vec::new()));
        };
        class_name = object.class_name.to_string();
        drop(object);
    }

    if class_name == "Generator" {
        return collect_generator_unpack(eg, &iterable, kind).map(Some);
    }

    if !eg.class_is_a(&class_name, "Iterator") {
        return Ok(None);
    }
    let iterable = crate::stdlib::prepare_native_deque_consumer(&iterable, eg).unwrap_or(iterable);
    let _ = crate::stdlib::call_object_protocol_method(
        eg,
        &iterable,
        "Iterator",
        "rewind",
        &[],
    )?;
    if eg.exception.is_some() {
        return Ok(Some(Vec::new()));
    }
    let mut entries = Vec::new();
    loop {
        let valid = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "Iterator",
            "valid",
            &[],
        )?
        .unwrap_or_else(|| Value::bool(false));
        if eg.exception.is_some() || !valid.is_truthy() {
            break;
        }
        let key = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "Iterator",
            "key",
            &[],
        )?
        .unwrap_or_else(Value::null);
        let value = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "Iterator",
            "current",
            &[],
        )?
        .unwrap_or_else(Value::null);
        if eg.exception.is_some() {
            break;
        }
        let (key, external_byte_key) = match traversable_unpack_key(&key, kind) {
            Ok(key) => key,
            Err(message) => {
                eg.exception = Some(make_error_value("Error", &message));
                break;
            }
        };
        entries.push((
            key,
            kind.value(value.dereferenced().clone()),
            external_byte_key,
        ));
        let _ = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "Iterator",
            "next",
            &[],
        )?;
        if eg.exception.is_some() {
            break;
        }
    }
    Ok(Some(entries))
}


fn append_array_unpack_entry(
    target: &mut PhpArray,
    key: ArrayKey,
    value: Value,
    external_byte_key: bool,
) -> Result<(), &'static str> {
    match key {
        ArrayKey::Int(_) => {
            if !target.try_push(value) {
                return Err("Cannot add element to the array as the next element is already occupied");
            }
        }
        ArrayKey::String(key) => {
            let source = if external_byte_key {
                Value::binary_string_from_storage(key.clone())
            } else {
                Value::string(key.clone())
            };
            let key = match target.prepare_string_key_for_write(ArrayKey::String(key), &source) {
                ArrayKey::String(key) => key,
                ArrayKey::Int(_) => {
                    if !target.try_push(value) {
                        return Err(
                            "Cannot add element to the array as the next element is already occupied",
                        );
                    }
                    return Ok(());
                }
            };
            if canonical_decimal_array_key(&key).is_some() {
                if !target.try_push(value) {
                    return Err(
                        "Cannot add element to the array as the next element is already occupied",
                    );
                }
            } else {
                target.set_owned_str(key, value);
            }
        }
    }
    Ok(())
}

#[inline(never)]
fn op_add_array_unpack<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: `opline` and op2 belong to `op_array` and the active frame. Array
    // literal unpack reads the operand before mutating the separate op1 TMP.
    let (source, instruction_index, is_root_frame) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize,
            (*frame).prev_execute_data.is_null(),
        )
    };
    let source = source.dereferenced();
    let entries = if let Some(source) = source.as_array() {
        let external_byte_keys = source.has_external_byte_keys();
        Some(
            source
                .iter()
                .map(|(key, value)| (key, value.dereferenced().clone(), external_byte_keys))
                .collect::<Vec<_>>(),
        )
    } else if opline._pad & ARRAY_UNPACK_CONSTANT_EXPRESSION != 0 {
        return Ok(unpack_error(
            eg,
            frame,
            op_array,
            instruction_index,
            is_root_frame,
            "Only arrays can be unpacked in constant expression",
        )?);
    } else {
        collect_unpack_traversable(eg, source, TraversableUnpackKind::Array)?
    };
    let Some(entries) = entries else {
        let given = source
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| source.type_name().to_string());
        return Ok(unpack_error(
            eg,
            frame,
            op_array,
            instruction_index,
            is_root_frame,
            &format!("Only arrays and Traversables can be unpacked, {given} given"),
        )?);
    };
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    // SAFETY: op1 is the compiler-owned array-literal temporary and remains
    // live for the rest of the expression.
    let target = unsafe { &mut *(*frame).get_op_mut(opline.op1 as u32, opline.op1_type) }
        .as_array_mut()
        .ok_or_else(|| VmError::Fatal("AddArrayUnpack target is not an array".to_string()))?;
    for (key, value, external_byte_key) in entries {
        if let Err(message) =
            append_array_unpack_entry(target, key, value, external_byte_key)
        {
            return Ok(unpack_error(
                eg,
                frame,
                op_array,
                instruction_index,
                is_root_frame,
                message,
            )?);
        }
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_add_call_argument<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: every operand is compiler-allocated in this live frame. CV
    // promotion writes through the same tracked slot, and the returned target
    // borrow is consumed before the opcode advances the frame.
    let (value, target) = unsafe {
        let value = if opline.op2_type == OpType::Cv {
            let source = (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize);
            materialize_reference_alias(frame, source)
        } else {
            (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone()
        };
        let target = &mut *(*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
        (value, target)
    };
    let key = if opline.result_type == OpType::Const {
        op_array
            .literals
            .get(opline.result as usize)
            .and_then(Value::as_str)
            .map(|key| ArrayKey::String(key.to_string()))
            .unwrap_or(ArrayKey::Int(0))
    } else {
        ArrayKey::Int(0)
    };
    let target = target
        .as_array_mut()
        .ok_or_else(|| VmError::Fatal("AddCallArgument target is not an array".to_string()))?;
    if let ArrayKey::String(name) = key {
        if target.get_str(&name).is_some() {
            return Ok(unpack_error(
                eg,
                frame,
                op_array,
                usize::MAX,
                false,
                &format!("Named parameter ${name} overwrites previous argument"),
            )?);
        }
        target.set_str(&name, value);
    } else {
        target.push(value);
    }
    Ok(ColdResult::Done)
}

#[inline(never)]
fn op_add_call_unpack<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let collect_source = |eg: &mut ExecutorGlobals,
                          source: &mut Value|
     -> Result<Option<Vec<(ArrayKey, Value, bool)>>, VmError> {
        if let Some(source) = source.as_array_mut() {
            let external_byte_keys = source.has_external_byte_keys();
            let keys: Vec<_> = source.iter().map(|(key, _)| key).collect();
            return keys
                .into_iter()
                .enumerate()
                .map(|(position, key)| {
                    source
                        .argument_unpack_reference_at(position)
                        .map(|value| (key, value, external_byte_keys))
                        .ok_or_else(|| {
                            VmError::Fatal(
                                "Argument unpack source changed during iteration".to_string(),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some);
        }
        collect_unpack_traversable(eg, source, TraversableUnpackKind::Arguments)
    };

    // SAFETY: op2 is a compiler-allocated live operand. Non-constant operands
    // are mutable for the duration of this opcode, and any followed reference
    // target is owned by a still-live frame or reference cell.
    // SAFETY: Reading its type before leaving this block cannot outlive or alias
    // the operand mutation.
    let (entries, invalid_given) = unsafe {
        if opline.op2_type == OpType::Const {
            let mut source =
                (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone();
            let entries = collect_source(eg, &mut source)?;
            let given = entries.is_none().then(|| {
                source
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| source.type_name().to_string())
            });
            (entries, given)
        } else {
            let source_ptr = (*frame).get_op_mut(opline.op2 as u32, opline.op2_type);
            let source_ptr = if (*source_ptr).is_reference() {
                (*source_ptr).as_ref_ptr()
            } else {
                source_ptr
            };
            let source = &mut *source_ptr;
            let entries = collect_source(eg, source)?;
            let given = entries.is_none().then(|| {
                source.as_object().map_or_else(
                    || {
                        if source.value_type() == ValueType::Undef {
                            "null".to_string()
                        } else {
                            source.type_name().to_string()
                        }
                    },
                    |object| object.class_name.to_string(),
                )
            });
            (entries, given)
        }
    };

    let entries = match entries {
        Some(entries) => entries,
        None => {
            let given = invalid_given.expect("invalid unpack source type");
            return Ok(unpack_throw(
                eg,
                frame,
                op_array,
                usize::MAX,
                false,
                "TypeError",
                &format!("Only arrays and Traversables can be unpacked, {given} given"),
            )?);
        }
    };
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
        });
    }

    // SAFETY: op1 is the compiler-owned mutable argument-list temporary and
    // remains live until the later call opcode consumes it.
    let target = unsafe { &mut *(*frame).get_op_mut(opline.op1 as u32, opline.op1_type) }
    .as_array_mut()
    .ok_or_else(|| VmError::Fatal("AddCallUnpack target is not an array".to_string()))?;
    let mut seen_named = false;
    for (key, value, external_byte_key) in entries {
        if let Err(message) =
            append_call_unpack_entry(target, key, value, external_byte_key, &mut seen_named)
        {
            return Ok(unpack_error(
                eg,
                frame,
                op_array,
                usize::MAX,
                false,
                &message,
            )?);
        }
    }
    Ok(ColdResult::Done)
}

#[inline]
fn bind_foreach_value_cv(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    cv: u32,
    value: Value,
) -> Result<(), VmError> {
    // SAFETY: `cv` is compiler-allocated in the active frame. A by-reference
    // foreach value rebinds this CV itself, so the destination remains a frame
    // slot and must use frame bitmap bookkeeping.
    unsafe {
        let slot = (*frame).cv_mut(cv);
        let destructor = if (slot.value_type() as u8) >= ValueType::Array as u8 {
            prepare_replaced_value_destructor(eg, &*slot)
        } else { None };
        frame_slot_set(frame, slot, value);
        if destructor.is_some() {
            run_prepared_value_destructor(eg, destructor)?;
        }
    }
    Ok(())
}

#[inline]
fn clone_foreach_value<const BY_REFERENCE_LOOP: bool>(value: &Value) -> Value {
    if BY_REFERENCE_LOOP && value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else if BY_REFERENCE_LOOP && value.is_reference() {
        // SAFETY: the detached foreach array retains the borrowed target for
        // the lifetime of the loop-bound alias.
        Value::reference(value.dereferenced() as *const Value as *mut Value)
    } else {
        value.clone()
    }
}

#[inline]
fn foreach_diagnostic_type_name(value: &Value) -> std::borrow::Cow<'_, str> {
    match value.dereferenced().value_type() {
        ValueType::True => std::borrow::Cow::Borrowed("true"),
        ValueType::False => std::borrow::Cow::Borrowed("false"),
        _ => value.dereferenced().diagnostic_type_name(),
    }
}

#[inline]
fn materialize_foreach_array_key(key: ArrayKey, external_byte_keys: bool) -> Value {
    match key {
        ArrayKey::Int(key) => Value::long(key),
        ArrayKey::String(key) if external_byte_keys => Value::binary_string_from_storage(key),
        ArrayKey::String(key) => Value::string(key),
    }
}

#[inline]
fn set_foreach_object_entry(array: &mut PhpArray, name: &str, value: Value) {
    if let Some(key) = canonical_decimal_array_key(name) {
        array.set_int(key, value);
    } else {
        array.set_str(name, value);
    }
}

#[inline]
fn object_uses_direct_property_iteration(value: &Value, _eg: &ExecutorGlobals) -> bool {
    value.as_object().is_some()
}

fn materialize_foreach_object(
    value: &Value,
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<Value, VmError> {
    let (class_id, dynamic_len) = value
        .as_object()
        .map(|object| {
            (
                object.class_id,
                object
                    .dynamic_properties
                    .as_ref()
                    .map_or(0, |properties| properties.len()),
            )
        })
        .expect("object foreach materialization requires an object");
    let caller_class = get_caller_class(frame, eg);
    let slots = eg.visible_instance_property_slots(class_id, caller_class.as_deref());
    let mut array = PhpArray::with_hash_capacity(slots.len() + dynamic_len);
    let mut declared_names = std::collections::HashSet::new();
    for slot in slots {
        let definition = eg
            .instance_property_definition(class_id, slot)
            .expect("visible property slot must retain its definition")
            .clone();
        declared_names.insert(definition.name.clone());
        let property = if definition.has_get_hook {
            call_object_property_get_hook(
                eg,
                value,
                &definition.name,
                &definition.declaring_class,
            )?
                .map(|value| value.dereferenced().clone())
        } else {
            value.as_object().and_then(|object| {
                object
                    .get_property_slot(slot)
                    .filter(|property| !property.is_undef())
                    .cloned()
            })
        };
        if eg.exception.is_some() {
            return Ok(Value::array(array));
        }
        if let Some(property) = property {
            set_foreach_object_entry(&mut array, &definition.name, property);
        }
    }
    if let Some(object) = value.as_object() {
        object.for_each_dynamic_property(|name, property| {
            if !property.is_undef() && !declared_names.contains(name) {
                set_foreach_object_entry(&mut array, name, property.clone());
            }
        });
    }
    Ok(Value::array(array))
}

#[inline]
fn visible_foreach_object_property_name(name: &str) -> (&str, Option<&'static str>) {
    let Some(mangled) = name.strip_prefix('\0') else {
        return (name, None);
    };
    let Some((scope, _)) = mangled.split_once('\0') else {
        return (name, Some("Illegal member variable name"));
    };
    if scope.is_empty() {
        return (name, Some("Illegal member variable name"));
    }
    let visible = name
        .rsplit_once('\0')
        .map(|(_, visible)| visible)
        .unwrap_or(name);
    if visible.is_empty() {
        (name, Some("Corrupt member variable name"))
    } else {
        (visible, None)
    }
}

#[inline]
fn materialize_foreach_object_key(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    storage_name: &str,
) -> Result<Value, VmError> {
    let (visible_name, diagnostic) = visible_foreach_object_property_name(storage_name);
    if let Some(diagnostic) = diagnostic {
        report_php_notice(eg, frame, op_array, opline, diagnostic)?;
    }
    Ok(Value::string(visible_name.to_string()))
}

fn promote_foreach_property_reference(property: &mut Value) -> Value {
    if property.is_owned_reference() {
        return property.clone_owned_reference_alias();
    }
    let current = std::mem::replace(property, Value::undef());
    let current = if current.is_reference() {
        current.dereferenced().clone()
    } else {
        current
    };
    let binding = Value::owned_reference(current);
    *property = binding.clone_owned_reference_alias();
    binding
}

#[inline]
fn set_foreach_iteration_state(
    frame: *mut ExecuteData,
    opline: &Instruction,
    iterable: Option<Value>,
    position: i64,
) {
    // SAFETY: ForeachInit's source, result, and position operands are
    // compiler-allocated live TMP slots in this frame and no pointer escapes
    // this helper. `None` selects the source's unique-consumer move form.
    unsafe {
        let iterable = iterable.unwrap_or_else(|| {
            debug_assert!(matches!(opline.op1_type, OpType::Tmp | OpType::Var));
            let source = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
            frame_tmp_take!(frame, source)
        });
        let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        frame_result_set(frame, result, opline.result_type, iterable);
        let cursor = (*frame).get_op_mut(opline.extended_value, OpType::Tmp);
        frame_tmp_set_long(frame, cursor, position);
    }
}

/// Translate every active by-reference foreach position that observes the
/// mutated reference cell. `frame` is the first user frame whose loops may be
/// live; callers reached through an internal array mutator start at its parent.
fn adjust_live_foreach_reference_positions(
    eg: &ExecutorGlobals,
    mut frame: *mut ExecuteData,
    target_reference: Option<usize>,
    target_array: Option<usize>,
    start: usize,
    removed: usize,
    inserted: usize,
) {
    // SAFETY: the active frame chain remains live throughout synchronous array
    // mutation. User frame metadata owns every instruction and slot inspected
    // below; only Long foreach-position TMPs are updated.
    unsafe {
        let removed_end = start.saturating_add(removed);
        while !frame.is_null() {
            let function = (*frame).func;
            if !function.is_null() && (*function).fn_type == FunctionType::User {
                let user = &*(function as *const UserFunction);
                let op_array = &user.op_array;
                if (*function).plan.has_reference_foreach() {
                    let current = (*frame)
                        .opline
                        .offset_from(op_array.instructions.as_ptr())
                        as usize;
                    for (init_index, init) in op_array.instructions.iter().enumerate() {
                        if init.opcode != OpCode::ForeachInit {
                            continue;
                        }
                        let Some(next) = op_array.instructions.get(init_index + 1) else {
                            continue;
                        };
                        let Some(exit) = op_array.instructions.get(init_index + 2) else {
                            continue;
                        };
                        if next.opcode != OpCode::ForeachNextRef
                            || exit.opcode != OpCode::JmpZ
                            || current <= init_index + 2
                            || current >= exit.op2 as usize
                        {
                            continue;
                        }
                        let iteration_state = &*(*frame).get_op_ptr(
                            next.op1 as u32,
                            next.op1_type,
                            op_array,
                        );
                        if iteration_state.reference_identity() != target_reference
                            && iteration_state.dereferenced().array_identity() != target_array
                        {
                            continue;
                        }
                        let position = &*(*frame).get_op_ptr(
                            next.op2 as u32,
                            next.op2_type,
                            op_array,
                        );
                        let Some(position) = position
                            .as_long()
                            .and_then(|position| usize::try_from(position).ok())
                        else {
                            continue;
                        };
                        if start >= position {
                            continue;
                        }
                        let removed_before_position = removed_end.min(position) - start;
                        let adjusted = position
                            .saturating_sub(removed_before_position)
                            .saturating_add(inserted);
                        let position_slot =
                            (*frame).get_op_mut(next.op2 as u32, next.op2_type);
                        frame_tmp_set_long(
                            frame,
                            position_slot,
                            i64::try_from(adjusted).unwrap_or(i64::MAX),
                        );
                    }
                }
            }
            let physical = (*frame).prev_execute_data;
            let caller = eg.trace_caller(frame as usize, physical);
            if caller == frame {
                break;
            }
            frame = caller;
        }
    }
}

/// Keep the next-position counter of every active by-reference foreach stable
/// across an array splice performed by an internal function.
pub(crate) fn adjust_live_foreach_reference_positions_for_splice(
    eg: &ExecutorGlobals,
    internal_frame: *mut ExecuteData,
    argument_index: u32,
    start: usize,
    removed: usize,
    inserted: usize,
) {
    if internal_frame.is_null() {
        return;
    }
    // SAFETY: the internal activation and its argument remain live for the
    // complete synchronous call.
    unsafe {
        let argument = (*internal_frame).cv(argument_index);
        let target_reference = argument.reference_identity();
        let target_array = argument.dereferenced().array_identity();
        if target_reference.is_none() && target_array.is_none() {
            return;
        }
        adjust_live_foreach_reference_positions(
            eg,
            (*internal_frame).prev_execute_data,
            target_reference,
            target_array,
            start,
            removed,
            inserted,
        );
    }
}

/// Apply the same iterator translation to a structural mutation executed in a
/// user frame, including every independently nested loop over the same cell.
#[inline]
fn adjust_live_foreach_reference_positions_for_direct_splice(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    target_reference: Option<usize>,
    target_array: Option<usize>,
    start: usize,
    removed: usize,
    inserted: usize,
) {
    if target_reference.is_some() || target_array.is_some() {
        adjust_live_foreach_reference_positions(
            eg,
            frame,
            target_reference,
            target_array,
            start,
            removed,
            inserted,
        );
    }
}

#[inline]
fn take_foreach_protocol_exception<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Result<Option<ColdResult<'a>>, VmError> {
    let Some(exception) = eg.exception.take() else {
        return Ok(None);
    };
    Ok(Some(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
    }))
}

#[inline]
fn release_temporary_foreach_source<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    init: &Instruction,
) -> Result<Option<ColdResult<'a>>, VmError> {
    debug_assert!(init.opcode == OpCode::ForeachInit);
    debug_assert!(matches!(init.op1_type, OpType::Tmp | OpType::Var));
    release_statement_temps(
        eg,
        frame,
        init.op1 as usize,
        init.op1 as usize + 1,
        STATEMENT_TEMPS_ORDINARY,
        false,
    )?;
    take_foreach_protocol_exception(eg, frame)
}

/// A failed rewind retires the consumer before its temporary source. In
/// particular, the source destructor can already reuse the consumer's handle.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zziterator"))]
fn release_failed_foreach_rewind<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    init: &Instruction,
) -> Result<Option<ColdResult<'a>>, VmError> {
    // The pending rewind exception must not look like a fresh destructor
    // failure to the release planner and prevent it from clearing the TMP.
    let pending = eg.exception.take();
    let mut cleanup = release_statement_temps(
        eg, frame, init.result as usize, init.result as usize + 1,
        STATEMENT_TEMPS_ORDINARY, false,
    );
    if cleanup.is_ok() && eg.exception.is_none()
        && matches!(init.op1_type, OpType::Tmp | OpType::Var) {
        cleanup = release_statement_temps(
            eg, frame, init.op1 as usize, init.op1 as usize + 1,
            STATEMENT_TEMPS_ORDINARY, false,
        );
    }
    if let Some(replacement) = eg.exception.as_ref() {
        if let Some(previous) = pending.as_ref() {
            append_replaced_exception(replacement, previous, eg);
        }
    } else {
        eg.exception = pending;
    }
    cleanup?;
    take_foreach_protocol_exception(eg, frame)
}

/// Release a temporary protocol source after the first validity check. The
/// consumer retains a direct Iterator; an aggregate receiver is no longer
/// needed. Named/aliased variables retain their ordinary PHP lifetime.
#[inline]
fn release_temporary_foreach_aggregate<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    foreach_next: &Instruction,
) -> Result<Option<ColdResult<'a>>, VmError> {
    // SAFETY: the active instruction is borrowed from this op array for the
    // duration of the dispatch call.
    let next_ip = unsafe {
        (foreach_next as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let Some(init) = next_ip
        .checked_sub(1)
        .and_then(|init_ip| op_array.instructions.get(init_ip))
        .filter(|init| init.opcode == OpCode::ForeachInit)
    else {
        return Ok(None);
    };
    if !matches!(init.op1_type, OpType::Tmp | OpType::Var) {
        return Ok(None);
    }

    // SAFETY: ForeachInit's compiler-owned source TMP remains live until the
    // first validity check for both aggregate and direct Iterator sources.
    // release_statement_temps clears only that slot and updates the bitmap.
    let is_protocol_source = unsafe {
        let source = &*(*frame).get_op_ptr(init.op1 as u32, init.op1_type, op_array);
        source
            .dereferenced()
            .as_object()
            .map(|object| object.class_name.to_string())
            .is_some_and(|class_name| {
                eg.class_is_a(&class_name, "IteratorAggregate")
                    || eg.class_is_a(&class_name, "Iterator")
            })
    };
    if !is_protocol_source {
        return Ok(None);
    }

    release_temporary_foreach_source(eg, frame, init)
}

#[inline]
fn uses_user_iterator_protocol(value: &Value, eg: &ExecutorGlobals) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let class_name = object.class_name.to_string();
    drop(object);
    class_name != "Generator" && eg.class_is_a(&class_name, "Iterator")
}

#[inline]
fn flush_foreach_reference_value(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    array_operand: u16,
    array_type: OpType,
    position_operand: u16,
    position_type: OpType,
    value_cv: u32,
) -> Result<(), VmError> {
    // SAFETY: all operands are allocated by the active op-array. The value is
    // written only into the detached iteration array at the preceding valid
    // position, which `ForeachNextRef` advanced after reading an element.
    unsafe {
        let position = (&*(*frame).get_op_ptr(
            position_operand as u32,
            position_type,
            op_array,
        ))
            .as_long()
            .unwrap_or(0);
        if position <= 0 {
            return Ok(());
        }

        let array_ptr = (*frame).get_op_mut(array_operand as u32, array_type);
        let array = &mut *array_ptr;
        if array.is_reference() {
            // A CV-backed by-reference foreach aliases the source array
            // directly. Its element reference cell is updated by ordinary CV
            // assignment, so there is no detached snapshot to flush.
            return Ok(());
        }
        if array.as_object().is_some() {
            // Object slots and yielded generator cells are live aliases.
            // Only detached array snapshots require element writeback.
            return Ok(());
        }
        // Reading through get_op_ptr would erase the CV's reference identity
        // before copying it into the detached source array. Live sources above
        // need neither this snapshot nor its extra reference owner.
        let value = (*frame).cv(value_cv).clone_closure_capture();
        let Some(array) = array.as_array_mut() else {
            return Err(VmError::Fatal(
                "Foreach by-reference source is no longer an array".into(),
            ));
        };
        array.set_value_at((position - 1) as usize, value);
        Ok(())
    }
}

#[inline(never)]
fn op_foreach_init<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: ForeachInit's source operand and promoted array/object alias use a
    // compiler-validated frame slot borrowed only until this opcode finishes.
    // A CV array is promoted to an owned cell before either side mutates it; a
    // reference-returning call already supplies the owned or borrowed cell in
    // its TMP result and must keep that alias instead of detaching its value.
    let (init_ip, by_reference, live_source_alias) = unsafe {
        let init_ip =
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        let by_reference = op_array
            .instructions
            .get(init_ip + 1)
            .is_some_and(|next| next.opcode == OpCode::ForeachNextRef);
        let source = (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let live_source_alias = (by_reference
            && (opline.op1_type == OpType::Cv || (&*source).is_reference())
            && matches!(
                (&*source).dereferenced().value_type(),
                ValueType::Array | ValueType::Object
            ))
        .then(|| {
            let source = if opline.op1_type == OpType::Cv {
                (*frame).cv_mut(opline.op1 as u32)
            } else {
                (*frame).get_op_mut(opline.op1 as u32, opline.op1_type)
            };
            materialize_reference_alias(frame, source)
        });
        (init_ip, by_reference, live_source_alias)
    };
    let raw_source = live_source_alias.as_ref().unwrap_or_else(|| unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    });
    let source = raw_source.dereferenced();
    let lazy_source_owner = eg.lazy_object_state(source).map(|_| source.clone());
    let source = lazy_source_owner.as_ref().unwrap_or(source);
    let initialized_source = if eg.lazy_object_state(source).is_some() {
        Some(crate::stdlib::reflection::resolve_lazy_object_chain(
            eg, source,
        )?)
    } else {
        None
    };
    if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
        return Ok(control);
    }
    let source = initialized_source.as_ref().unwrap_or(source);
    let mut resolved_iterable = None;
    let mut aggregate_identities = Vec::new();
    loop {
        let candidate = resolved_iterable.as_ref().unwrap_or(source);
        let Some(object) = candidate.as_object() else {
            break;
        };
        let class_name = object.class_name.to_string();
        drop(object);
        if !eg.class_is_a(&class_name, "IteratorAggregate") {
            break;
        }
        let identity = candidate.object_identity().unwrap();
        if aggregate_identities.contains(&identity) {
            let error = make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class_name}::getIterator() must be traversable or implement interface Iterator"
                ),
            );
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        aggregate_identities.push(identity);
        let receiver = candidate.clone();
        let next = crate::stdlib::call_object_protocol_method(
            eg,
            &receiver,
            "IteratorAggregate",
            "getIterator",
            &[],
        )?
        .ok_or_else(|| VmError::Fatal(format!("Call to undefined method {class_name}::getIterator()")))?;
        if let Some(exception) = eg.exception.take() {
            return Ok(match throw_in_frame(eg, frame, exception)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        let traversable = next
            .as_object()
            .map(|object| object.class_name.to_string())
            .is_some_and(|returned_class| eg.class_is_a(&returned_class, "Traversable"));
        if !traversable {
            let error = make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class_name}::getIterator() must be traversable or implement interface Iterator"
                ),
            );
            attach_throwable_origin(&error, eg, frame, op_array, init_ip);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        resolved_iterable = Some(next);
    }
    let native_consumer = crate::stdlib::prepare_native_deque_consumer(resolved_iterable.as_ref().unwrap_or(source), eg);
    let arr_val = native_consumer.as_ref().or(resolved_iterable.as_ref()).unwrap_or(source);

    if by_reference && crate::stdlib::date_period_iterator_disallows_references(arr_val) {
        let error = make_error_value(
            "Error",
            "An iterator cannot be used with foreach by reference",
        );
        attach_throwable_origin(&error, eg, frame, op_array, init_ip);
        return Ok(match throw_in_frame(eg, frame, error)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        });
    }

    // Check for Generator object
    let is_generator = if let Some(obj) = arr_val.as_object() {
        obj.class_name.as_ref() == "Generator" && arr_val.as_object_rc().map_or(false, |rc| rc.borrow().generator.is_some())
    } else {
        false
    };

    if is_generator {
        // Start the generator (rewind)
        let gen_ref = arr_val.as_object_rc().unwrap().borrow().generator.clone().unwrap();
        if by_reference && !gen_ref.borrow().yields_by_reference() {
            let error = make_error_value(
                "Exception",
                "You can only iterate a generator by-reference if it declared that it yields by-reference",
            );
            attach_throwable_origin(&error, eg, frame, op_array, init_ip);
            return Ok(match throw_in_frame(eg, frame, error)? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
        {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Created {
                let outcome = resume_generator(eg, &gen_ref, Value::null())?;
                match generator_resume_result(eg, frame, outcome)? {
                    ColdResult::Done => {}
                    control => return Ok(control),
                }
            } else if !gen_ref.borrow().rewindable {
                let message = if state == crate::vm::generator::GeneratorState::Completed {
                    "Cannot traverse an already closed generator"
                } else {
                    "Cannot rewind a generator that was already run"
                };
                let error = make_error_value("Exception", message);
                attach_throwable_origin(&error, eg, frame, op_array, init_ip);
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        }
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        if !is_valid {
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(ColdResult::Continue);
        }
        // Position 0 means the generator was already started and must not be
        // resumed again before its first value is consumed.
        set_foreach_iteration_state(frame, opline, Some(arr_val.clone()), 0);
    } else {
        if uses_user_iterator_protocol(arr_val, eg) {
            if crate::stdlib::uses_native_iterator_protocol(arr_val, eg) {
                set_foreach_iteration_state(
                    frame,
                    opline,
                    Some(foreach_iterator_owner(arr_val, eg, native_consumer.is_some())),
                    i64::MIN,
                );
                crate::stdlib::native_iterator_projected_entry(arr_val, crate::stdlib::NativeIteratorMove::Rewind, crate::stdlib::NativeIteratorProjection::None, eg)?;
                if eg.exception.is_some()
                    && let Some(control) = release_failed_foreach_rewind(eg, frame, opline)? {
                    return Ok(control);
                }
                if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                    return Ok(control);
                }
                return Ok(ColdResult::Done);
            }
            if by_reference && !eg.weak_iterator_allows_references(arr_val) {
                let error = make_error_value(
                    "Error",
                    "An iterator cannot be used with foreach by reference",
                );
                return Ok(match throw_in_frame(eg, frame, error)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
            if by_reference {
                eg.enable_weak_iterator_references(arr_val);
            }
            if !crate::stdlib::validate_recursive_iterator_start(arr_val, eg) {
                if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                    return Ok(control);
                }
            }
            set_foreach_iteration_state(
                frame,
                opline,
                Some(foreach_iterator_owner(arr_val, eg, native_consumer.is_some())),
                -1,
            );
            let _ = crate::stdlib::call_object_protocol_method(
                eg,
                arr_val,
                "Iterator",
                "rewind",
                &[],
            )?;
            if eg.exception.is_some()
                && let Some(control) = release_failed_foreach_rewind(eg, frame, opline)? {
                return Ok(control);
            }
            if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                return Ok(control);
            }
            // Negative cursor values identify the user Iterator protocol. Each
            // successful fetch decrements it, retaining first-vs-next state
            // without a class lookup in the hot ForeachNext path.
            return Ok(ColdResult::Done);
        }
        let object_values = if arr_val.as_object().is_some() {
            let direct_property_iteration = object_uses_direct_property_iteration(arr_val, eg);
            let materialized = if by_reference || direct_property_iteration {
                arr_val.clone()
            } else {
                materialize_foreach_object(arr_val, eg, frame)?
            };
            if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                return Ok(control);
            }
            Some(materialized)
        } else {
            None
        };
        let iterable = object_values.as_ref().unwrap_or(arr_val);
        let is_empty = match iterable.dereferenced().as_array() {
            Some(arr) => arr.is_empty(),
            None if iterable.value_type() == ValueType::Object => false,
            None if iterable.value_type() == ValueType::Closure => true,
            None => {
                let type_name = foreach_diagnostic_type_name(arr_val);
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!(
                        "foreach() argument must be of type array|object, {type_name} given"
                    ),
                    false,
                )?;
                if let Some(exception) = eg.exception.take() {
                    return Ok(match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            ColdResult::NewFrame(new_frame, new_op_array)
                        }
                        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                    });
                }
                true
            }
        };
        if is_empty {
            if matches!(opline.op1_type, OpType::Tmp | OpType::Var)
                && (resolved_iterable.is_some()
                    || matches!(
                        raw_source.value_type(),
                        ValueType::Array | ValueType::Closure
                    ))
                && let Some(control) = release_temporary_foreach_source(eg, frame, opline)?
            {
                return Ok(control);
            }
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(ColdResult::Continue);
        }
        // A direct temporary array has exactly one compiler consumer. Move it
        // into the iteration state instead of cloning and immediately
        // releasing the source slot; CVs, references, objects, and resolved
        // Traversables retain their existing snapshot semantics.
        let temporary_array_source = matches!(opline.op1_type, OpType::Tmp | OpType::Var)
            && raw_source.value_type() == ValueType::Array
            && resolved_iterable.is_none()
            && object_values.is_none();
        let cloned = if let Some(live_source_alias) = live_source_alias.as_ref()
            && resolved_iterable.is_none()
        {
            clone_foreach_value::<true>(live_source_alias)
        } else if temporary_array_source {
            // The helper consumes the unique compiler TMP/VAR and clears its
            // ownership bitmap before publishing the iteration state.
            set_foreach_iteration_state(frame, opline, None, 0);
            return Ok(ColdResult::Done);
        } else {
            iterable.clone()
        };
        set_foreach_iteration_state(frame, opline, Some(cloned), 0);
        if resolved_iterable.is_some()
            && matches!(opline.op1_type, OpType::Tmp | OpType::Var)
            && let Some(control) = release_temporary_foreach_source(eg, frame, opline)?
        {
            return Ok(control);
        }
    }
    Ok(ColdResult::Done)
}

#[inline]
fn finish_foreach_step<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    opline: &Instruction,
    next_position: Option<i64>,
    has_more: bool,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: both destinations are compiler-allocated operands in the live
    // frame. A throwing release transfers control before the result write.
    unsafe {
        if let Some(next) = next_position {
            let position = (*frame).get_op_mut(opline.op2 as u32, opline.op2_type);
            frame_result_set(frame, position, opline.op2_type, Value::long(next));
        }
        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
            return Ok(control);
        }
        let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        frame_result_set(frame, result, opline.result_type, Value::bool(has_more));
    }
    Ok(ColdResult::Done)
}

// Native protocol work is shared by all three foreach specializations. It
// does not probe Generator/public Iterator callbacks on each native step.
#[inline(never)]
fn next_native_foreach<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    source: &Value,
    first: bool,
    by_reference: bool,
    assign_through_reference: bool,
) -> Result<ColdResult<'a>, VmError> {
    let movement = if first {
        crate::stdlib::NativeIteratorMove::Current
    } else {
        crate::stdlib::NativeIteratorMove::Next
    };
    let valid = if first {
        // Validity precedes aggregate release, but neither a payload alias nor
        // a reference wrapper may exist yet: the destructor can remove it.
        let valid = crate::stdlib::native_iterator_projected_entry(
            source, movement, crate::stdlib::NativeIteratorProjection::None, eg,
        )?.is_some();
        if let Some(control) = release_temporary_foreach_aggregate(eg, frame, op_array, opline)? {
            return Ok(control);
        }
        valid
    } else { true };
    let key_encoded = (opline.extended_value >> 16) as u32;
    let projection = if key_encoded == 0 {
        crate::stdlib::NativeIteratorProjection::Value
    } else {
        crate::stdlib::NativeIteratorProjection::Both
    };
    let entry = if valid {
        crate::stdlib::native_iterator_entry(source, movement, by_reference, projection, eg)?
    } else { None };
    if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
        return Ok(control);
    }
    let has_more = if let Some((key, value)) = entry {
        let value_cv = (opline.extended_value & 0xFFFF) as u32;
        if by_reference || !assign_through_reference {
            bind_foreach_value_cv(eg, frame, value_cv, value)?;
        } else {
            assign_foreach_cv(eg, frame, value_cv, value)?;
        }
        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
            return Ok(control);
        }
        if key_encoded > 0 {
            assign_foreach_cv(eg, frame, key_encoded - 1, key)?;
        }
        true
    } else {
        false
    };
    finish_foreach_step(eg, frame, opline, has_more.then_some(i64::MIN + 1), has_more)
}

#[inline(never)]
fn op_foreach_next<'a, const ASSIGN_THROUGH_REFERENCE: bool, const BY_REFERENCE_LOOP: bool>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let val_cv = (opline.extended_value & 0xFFFF) as u32;
    let key_encoded = (opline.extended_value >> 16) as u32;

    if BY_REFERENCE_LOOP {
        flush_foreach_reference_value(
            frame,
            op_array,
            opline.op1,
            opline.op1_type,
            opline.op2,
            opline.op2_type,
            val_cv,
        )?;
    }

    // SAFETY: both operands are compiler-owned slots in this live frame. A
    // negative cursor proves an internal consumer with an immutable slot 0;
    // neither shared borrow is used after an exception transfers control.
    let (iteration_state, cursor, source) = unsafe {
        let iteration_state =
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let cursor = (&*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        ))
            .as_long()
            .unwrap_or(0);
        // The private owner slot never escapes or changes while this TMP
        // is live. End its RefCell guard before invoking callbacks: a thrown
        // exception can retire the TMP. No source read follows that transfer.
        let source = if cursor < 0 {
            &*foreach_owned_iterator(iteration_state)
        } else {
            iteration_state.dereferenced()
        };
        (iteration_state, cursor, source)
    };
    let lazy_source_owner = eg.lazy_object_state(source).map(|_| source.clone());
    let source = lazy_source_owner.as_ref().unwrap_or(source);
    let initialized_source = if eg.lazy_object_state(source).is_some() {
        Some(crate::stdlib::reflection::resolve_lazy_object_chain(
            eg, source,
        )?)
    } else {
        None
    };
    if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
        return Ok(control);
    }
    let arr_val = initialized_source.as_ref().unwrap_or(source);
    if cursor <= i64::MIN + 1 {
        return next_native_foreach(
            eg, frame, op_array, opline, arr_val, cursor == i64::MIN,
            BY_REFERENCE_LOOP, ASSIGN_THROUGH_REFERENCE,
        );
    }
    // Check for Generator object
    let gen_ref_opt = if let Some(obj) = arr_val.as_object() {
        if obj.class_name.as_ref() == "Generator" {
            arr_val.as_object_rc().and_then(|rc| rc.borrow().generator.clone())
        } else { None }
    } else { None };
    let has_more = if cursor < 0 {
        if cursor < -1 {
            let _ = crate::stdlib::call_object_protocol_method(
                eg,
                arr_val,
                "Iterator",
                "next",
                &[],
            )?;
            if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                return Ok(control);
            }
        }
        let valid = crate::stdlib::call_object_protocol_method(
            eg,
            arr_val,
            "Iterator",
            "valid",
            &[],
        )?
        .unwrap_or_else(|| Value::bool(false));
        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
            return Ok(control);
        }
        if cursor == -1
            && let Some(control) =
                release_temporary_foreach_aggregate(eg, frame, op_array, opline)?
        {
            return Ok(control);
        }
        if !valid.is_truthy() {
            false
        } else {
            let value = crate::stdlib::call_object_protocol_method(
                eg,
                arr_val,
                "Iterator",
                "current",
                &[],
            )?
            .unwrap_or_else(Value::null);
            if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                return Ok(control);
            }
            if BY_REFERENCE_LOOP && value.is_owned_reference() {
                bind_foreach_value_cv(eg, frame, val_cv, value.clone_owned_reference_alias())?;
            } else if BY_REFERENCE_LOOP && value.is_reference() {
                bind_foreach_value_cv(
                    eg,
                    frame,
                    val_cv,
                    Value::reference(value.dereferenced() as *const Value as *mut Value),
                )?;
            } else {
                assign_foreach_cv(eg, frame, val_cv, value.dereferenced().clone())?;
            }
            if key_encoded > 0 {
                let key = crate::stdlib::call_object_protocol_method(
                    eg,
                    arr_val,
                    "Iterator",
                    "key",
                    &[],
                )?
                .unwrap_or_else(Value::null);
                if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                    return Ok(control);
                }
                assign_foreach_cv(eg, frame, key_encoded - 1, key.dereferenced().clone())?;
            }
            true
        }
    } else if let Some(gen_ref) = gen_ref_opt {
        let pos = cursor;

        // On first iteration (pos=0), generator is already started by ForeachInit
        // On subsequent iterations, call next()
        if pos > 0 {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Suspended {
                if pos == 1 {
                    mark_generator_not_rewindable(&gen_ref);
                }
                let outcome = resume_generator(eg, &gen_ref, Value::null())?;
                let control = generator_resume_result(eg, frame, outcome)?;
                if !matches!(control, ColdResult::Done) {
                    return Ok(control);
                }
            }
        }

        let gen_data = gen_ref.borrow();
        if gen_data.state != crate::vm::generator::GeneratorState::Completed {
            // Write current value to value_cv
            if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                bind_foreach_value_cv(
                    eg,
                    frame,
                    val_cv,
                    clone_foreach_value::<BY_REFERENCE_LOOP>(&gen_data.value),
                )?;
            } else {
                assign_foreach_cv(eg, frame, val_cv, gen_data.value.clone())?;
            }
            // Write key if requested
            if key_encoded > 0 {
                let key_cv = key_encoded - 1;
                assign_foreach_cv(eg, frame, key_cv, gen_data.key.clone())?;
            }
            drop(gen_data);
            // Increment position
            let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
            unsafe {
                frame_result_set(
                    frame,
                    pos_ptr,
                    opline.op2_type,
                    Value::long(pos + 1),
                )
            };
            true
        } else {
            false
        }
    } else {
        let pos = cursor as usize;

        if let Some(arr) = arr_val.dereferenced().as_array() {
            if pos < arr.len() {
                let external_byte_keys = arr.has_external_byte_keys();
                // SAFETY: the compiler validated all frame operands. The live
                // owned-reference target and current array position remain
                // request-owned throughout this synchronous opcode.
                unsafe {
                    if BY_REFERENCE_LOOP && iteration_state.is_reference() {
                        // ForeachInit created an owned reference alias for this
                        // CV-backed source. Promoting the live entry before the
                        // body makes both mutations observe the same cell.
                        let value = (&mut *iteration_state.as_ref_ptr())
                            .as_array_mut()
                            .and_then(|array| array.argument_unpack_reference_at(pos))
                            .expect("live foreach position must remain addressable");
                        bind_foreach_value_cv(eg, frame, val_cv, value)?;
                        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                            return Ok(control);
                        }
                        if key_encoded > 0 {
                            let key_cv = key_encoded - 1;
                            let key = iteration_state
                                .dereferenced()
                                .as_array()
                                .and_then(|array| array.get_at(pos))
                                .map(|(_, key)| key)
                                .expect("promoted foreach entry must retain its key");
                            let key_value =
                                materialize_foreach_array_key(key, external_byte_keys);
                            assign_foreach_cv(eg, frame, key_cv, key_value)?;
                        }
                    } else if key_encoded > 0 {
                        // Need both key and value — use get_at()
                        let (val, key) = arr.get_at(pos).unwrap();
                        if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                            bind_foreach_value_cv(
                                eg,
                                frame,
                                val_cv,
                                clone_foreach_value::<BY_REFERENCE_LOOP>(val),
                            )?;
                        } else {
                            assign_foreach_cv(eg, frame, val_cv, val.clone())?;
                        }
                        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                            return Ok(control);
                        }
                        let key_cv = key_encoded - 1;
                        let key_val = materialize_foreach_array_key(key, external_byte_keys);
                        assign_foreach_cv(eg, frame, key_cv, key_val)?;
                    } else {
                        // Only value needed — use get_value_at() (avoids key clone)
                        let val = arr.get_value_at(pos).unwrap();
                        if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                            bind_foreach_value_cv(
                                eg,
                                frame,
                                val_cv,
                                clone_foreach_value::<BY_REFERENCE_LOOP>(val),
                            )?;
                        } else {
                            assign_foreach_cv(eg, frame, val_cv, val.clone())?;
                        }
                    }
                    let pos_ptr = (*frame).get_op_mut(opline.op2 as u32, opline.op2_type);
                    frame_result_set(
                        frame,
                        pos_ptr,
                        opline.op2_type,
                        Value::long((pos + 1) as i64),
                    );
                    true
                }
            } else {
                false
            }
        } else if arr_val.value_type() == ValueType::Object
            && (BY_REFERENCE_LOOP || object_uses_direct_property_iteration(arr_val, eg))
        {
            let caller_class = get_caller_class(frame, eg);
            let class_id = arr_val
                .as_object()
                .map(|object| object.class_id)
                .unwrap_or(0);
            let compact_slot_count = {
                let object = arr_val.as_object().unwrap();
                if object.has_detached_property_table()
                    && !eg.class_by_id(class_id).is_some_and(|class| class.properties.iter().any(|p| p.has_get_hook || p.has_set_hook))
                {
                    Some(0)
                } else {
                eg.class_by_id(class_id)
                    .filter(|class| {
                        class.parent.is_none()
                            && class.properties.iter().all(|definition| {
                                definition.visibility == Visibility::Public
                            })
                            && class.properties.iter().enumerate().all(|(slot, definition)| {
                                (!object.property_values[slot].is_undef()
                                    || definition.has_get_hook)
                                    && (!definition.is_virtual_hook_property()
                                        || definition.has_get_hook)
                            })
                    })
                    .map(|class| class.properties.len())
                }
            };
            let slots = compact_slot_count.is_none().then(|| {
                eg.visible_instance_property_slots(class_id, caller_class.as_deref())
                    .into_iter()
                    .filter(|slot| {
                        let definition = eg.instance_property_definition(class_id, *slot);
                        definition.is_none_or(|definition| {
                            !definition.is_virtual_hook_property() || definition.has_get_hook
                        })
                    })
                    .collect::<Vec<_>>()
            });
            let declared_len = compact_slot_count
                .unwrap_or_else(|| slots.as_ref().map_or(0, Vec::len));
            let has_dynamic_properties = arr_val.as_object().is_some_and(|object| {
                object
                    .dynamic_properties
                    .as_ref()
                    .is_some_and(|properties| !properties.is_empty())
            });
            let dynamic_names = has_dynamic_properties.then(|| {
                let object = arr_val.as_object().unwrap();
                let declared_names = if object.has_detached_property_table() {
                    std::collections::HashSet::new()
                } else if let Some(slots) = slots.as_ref() {
                    slots
                        .iter()
                        .filter_map(|slot| eg.instance_property_definition(class_id, *slot))
                        .map(|definition| definition.name.as_str())
                        .collect::<std::collections::HashSet<_>>()
                } else {
                    eg.class_by_id(class_id)
                        .into_iter()
                        .flat_map(|class| class.properties.iter())
                        .map(|definition| definition.name.as_str())
                        .collect::<std::collections::HashSet<_>>()
                };
                let mut names = Vec::new();
                object.for_each_dynamic_property(|name, property| {
                    if !property.is_undef() && !declared_names.contains(name) {
                        names.push(name.to_string());
                    }
                });
                names
            });
            let dynamic_len = dynamic_names.as_ref().map_or_else(
                || {
                    arr_val
                        .as_object()
                        .and_then(|object| {
                            object
                                .dynamic_properties
                                .as_ref()
                                .map(|properties| properties.len())
                        })
                        .unwrap_or(0)
                },
                Vec::len,
            );
            // Declared property slots are stable even after unset(). Keep the
            // foreach cursor in that stable coordinate space and skip empty
            // slots instead of compacting them out of the live view. This is
            // what lets removing a prior/current property preserve the next
            // property while removing a future property hides it.
            let mut iteration_position = pos;
            while iteration_position < declared_len {
                let slot = slots
                    .as_ref()
                    .map_or(iteration_position, |slots| slots[iteration_position]);
                let readable = {
                    let definition = eg.instance_property_definition(class_id, slot);
                    !arr_val
                        .as_object()
                        .expect("foreach source remains an object")
                        .property_values[slot]
                        .is_undef()
                        || definition.is_some_and(|definition| definition.has_get_hook)
                };
                if readable {
                    break;
                }
                iteration_position += 1;
            }
            if iteration_position < declared_len + dynamic_len {
                let slot = (iteration_position < declared_len).then(|| {
                    slots
                        .as_ref()
                        .map_or(iteration_position, |slots| slots[iteration_position])
                });
                let name = if let Some(slot) = slot {
                    eg.instance_property_definition(class_id, slot)
                        .expect("visible property slot must retain its definition")
                        .name
                        .clone()
                } else {
                    let dynamic_position = iteration_position - declared_len;
                    dynamic_names.as_ref().map_or_else(
                        || {
                            arr_val
                                .as_object()
                                .and_then(|object| {
                                    object
                                        .dynamic_property_at(dynamic_position)
                                        .map(|(name, _)| name.to_string())
                                })
                                .expect("dynamic property position must remain readable")
                        },
                        |names| names[dynamic_position].clone(),
                    )
                };
                let key = if key_encoded > 0 {
                    Some(materialize_foreach_object_key(
                        eg, frame, op_array, opline, &name,
                    )?)
                } else {
                    None
                };
                if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                    return Ok(control);
                }

                let value = if let Some(slot) = slot {
                    let (
                        declaring_class,
                        type_scope,
                        type_hint,
                        is_typed,
                        is_readonly,
                        has_get_hook,
                    ) = {
                        let definition = eg
                            .instance_property_definition(class_id, slot)
                            .expect("visible property slot must retain its definition");
                        (
                            definition.declaring_class.clone(),
                            definition.type_scope.clone(),
                            definition.type_hint.clone(),
                            definition.is_typed(),
                            definition.is_readonly,
                            definition.has_get_hook,
                        )
                    };
                    if BY_REFERENCE_LOOP && is_readonly {
                        let error = make_error_value(
                            "Error",
                            &format!(
                                "Cannot acquire reference to readonly property {}::${}",
                                declaring_class, name
                            ),
                        );
                        return Ok(match throw_in_frame(eg, frame, error)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                        });
                    }
                    if BY_REFERENCE_LOOP && has_get_hook {
                        let hook_name = format!("${name}::get");
                        let returned = call_guarded_property_hook_method(
                            eg,
                            arr_val,
                            &name,
                            PROPERTY_GUARD_HOOK_GET,
                            &declaring_class,
                            &hook_name,
                            &[],
                        )?
                        .unwrap_or_else(Value::null);
                        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                            return Ok(control);
                        }
                        if returned.is_owned_reference() {
                            returned.clone_owned_reference_alias()
                        } else if returned.is_reference() {
                            // SAFETY: the getter result retains the referenced target while
                            // the loop CV alias is installed synchronously for this iteration.
                            Value::reference(
                                returned.dereferenced() as *const Value as *mut Value,
                            )
                        } else {
                            let class_name = arr_val
                                .as_object()
                                .map(|object| object.class_name.to_string())
                                .unwrap_or_else(|| "object".to_string());
                            let error = make_error_value(
                                "Error",
                                &format!(
                                    "Cannot create reference to property {class_name}::${name}"
                                ),
                            );
                            return Ok(match throw_in_frame(eg, frame, error)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                            });
                        }
                    } else if BY_REFERENCE_LOOP {
                        let (owner, called_class) = {
                            let object = arr_val.as_object().unwrap();
                            (
                                object.instance_property_reference_owner(slot),
                                object.class_name.to_string(),
                            )
                        };
                        let mut object = arr_val.as_object_mut().unwrap();
                        let binding = promote_foreach_property_reference(
                            object
                                .get_property_slot_mut(slot)
                                .expect("visible property slot must remain addressable"),
                        );
                        drop(object);
                        if is_typed {
                            binding.add_reference_property_constraint(
                                crate::value::ReferencePropertyConstraint {
                                    owner,
                                    declaring_class,
                                    property: name.clone(),
                                    type_scope,
                                    called_class,
                                    type_hint,
                                },
                            );
                        }
                        binding
                    } else if has_get_hook {
                        let returned = call_object_property_get_hook(
                            eg,
                            arr_val,
                            &name,
                            &declaring_class,
                        )?
                            .map(|value| value.dereferenced().clone())
                            .unwrap_or_else(Value::null);
                        if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                            return Ok(control);
                        }
                        returned
                    } else {
                        arr_val
                            .as_object()
                            .and_then(|object| object.get_property_slot(slot).cloned())
                            .expect("visible property slot must remain readable")
                    }
                } else {
                    if BY_REFERENCE_LOOP {
                        let mut object = arr_val.as_object_mut().unwrap();
                        promote_foreach_property_reference(
                            object
                                .get_dynamic_property_mut(&name)
                                .expect("dynamic property must remain addressable"),
                        )
                    } else {
                        arr_val
                            .as_object()
                            .and_then(|object| {
                                object
                                    .get_dynamic_property_with_position(&name)
                                    .map(|(property, _)| property.clone())
                            })
                            .expect("dynamic property must remain readable")
                    }
                };
                if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                    bind_foreach_value_cv(eg, frame, val_cv, value)?;
                } else {
                    assign_foreach_cv(eg, frame, val_cv, value)?;
                }
                if let Some(key) = key {
                    assign_foreach_cv(eg, frame, key_encoded - 1, key)?;
                }
                let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
                unsafe {
                    frame_result_set(
                        frame,
                        pos_ptr,
                        opline.op2_type,
                        Value::long((iteration_position + 1) as i64),
                    )
                };
                true
            } else {
                false
            }
        } else {
            if BY_REFERENCE_LOOP && iteration_state.is_reference() {
                let type_name = foreach_diagnostic_type_name(arr_val);
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!(
                        "foreach() argument must be of type array|object, {type_name} given"
                    ),
                    false,
                )?;
                if let Some(control) = take_foreach_protocol_exception(eg, frame)? {
                    return Ok(control);
                }
            }
            false
        }
    };

    finish_foreach_step(
        eg, frame, opline, (cursor < 0 && has_more).then(|| cursor - 1), has_more,
    )
}

#[inline(never)]
fn op_foreach_writeback(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    flush_foreach_reference_value(
        frame,
        op_array,
        opline.op1,
        opline.op1_type,
        opline.op2,
        opline.op2_type,
        opline.result as u32,
    )
}

fn generator_resume_result<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    outcome: GeneratorResumeOutcome,
) -> Result<ColdResult<'a>, VmError> {
    Ok(match outcome {
        GeneratorResumeOutcome::Advanced => ColdResult::Done,
        GeneratorResumeOutcome::Threw(exception) => match throw_in_frame(eg, frame, exception)? {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        },
    })
}

#[cold]
#[inline(never)]
fn mark_generator_not_rewindable(gen_ref: &crate::vm::generator::GeneratorRef) {
    gen_ref.borrow_mut().rewindable = false;
}

#[cold]
#[inline(never)]
fn prepare_reference_yield(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<Option<Value>, VmError> {
    let (value, notice) = if opline.extended_value == 1 {
        prepare_user_return_value(frame, op_array, opline, true)
    } else {
        let (value, _) = prepare_user_return_value(frame, op_array, opline, false);
        (Value::owned_reference(value), opline.extended_value == 2)
    };
    if notice {
        report_php_notice(eg, frame, op_array, opline, "Only variable references should be yielded by reference")?;
        // The resume boundary consumes this escaping exception. Leave it in
        // executor state and end the detached activation without entering a
        // generator-local catch/finally or extending the hot dispatch arm.
        if eg.exception.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(value))
}

struct SuspendedGeneratorFrameSnapshot {
    cv_values: Vec<Value>,
    tmp_values: Vec<Value>,
    ip_offset: usize,
    pending_return_after_finally: bool,
}

#[inline]
fn snapshot_suspended_generator_frame(
    frame: *mut ExecuteData,
    known_op_array: Option<&crate::compiler::OpArray>,
    advance_ip: bool,
    mut cv_values: Vec<Value>,
    mut tmp_values: Vec<Value>,
) -> SuspendedGeneratorFrameSnapshot {
    // SAFETY: callers pass the currently active generator frame. The ordinary
    // yield paths also provide its owning immutable op-array; Fiber suspension
    // recovers the same op-array from the materialized frame. CV/TMP bounds are
    // read from that frame, and its opline remains inside the op-array until
    // the snapshot is complete.
    unsafe {
        let frame = &*frame;
        let op_array = match known_op_array {
            Some(op_array) => op_array,
            None => frame.op_array(),
        };
        cv_values.clear();
        cv_values.extend(
            (0..frame.num_cvs).map(|index| frame.cv(index).clone_closure_capture()),
        );
        tmp_values.clear();
        tmp_values.extend(
            (0..frame.num_temps).map(|index| frame.tmp(index).clone_closure_capture()),
        );
        SuspendedGeneratorFrameSnapshot {
            cv_values,
            tmp_values,
            ip_offset: frame.opline.offset_from(op_array.instructions.as_ptr()) as usize
                + usize::from(advance_ip),
            pending_return_after_finally: frame.pending_return_after_finally,
        }
    }
}

#[cold]
#[inline(never)]
fn generator_instruction_index(
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> usize {
    op_array
        .instructions
        .iter()
        .position(|candidate| std::ptr::eq(candidate, opline))
        .expect("generator instruction must belong to its op-array")
}

#[inline(never)]
fn op_yield<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::GeneratorState;

    if eg
        .active_generator
        .as_ref()
        .is_some_and(|generator| generator.borrow().force_closing)
    {
        let error = make_error_value(
            "Error",
            "Cannot yield from finally in a force-closed generator",
        );
        let instruction_index = generator_instruction_index(op_array, opline);
        let origin_index = (0..=instruction_index)
            .rev()
            .find(|index| op_array.source_line(*index).is_some())
            .unwrap_or(instruction_index);
        attach_new_throwable_origin(
            &error,
            eg,
            frame,
            op_array,
            origin_index,
        );
        return throw_yield_from_exception(eg, frame, error);
    }

    let yielded_value = if opline.extended_value != 0 {
        let Some(value) = prepare_reference_yield(eg, frame, op_array, opline)? else {
            return Ok(ColdResult::Return);
        };
        value
    } else if opline.op1_type != OpType::Unused {
        unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }.clone()
    } else {
        Value::null()
    };

    let yielded_key = if opline.op2_type != OpType::Unused {
        Some(unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) }.clone())
    } else {
        None
    };

    if let Some(gen_ref) = eg.active_generator.take() {
        let pending_finally_exceptions = eg
            .finally_exceptions
            .remove(&(frame as usize))
            .unwrap_or_default();
        let mut gen_data = gen_ref.borrow_mut();

        // Set yielded value/key
        gen_data.value = yielded_value;
        if let Some(key) = yielded_key {
            if let Some(explicit) = key.as_long()
                && explicit >= gen_data.implicit_key
            {
                gen_data.implicit_key = explicit.wrapping_add(1);
            }
            gen_data.key = key;
        } else {
            gen_data.key = Value::long(gen_data.implicit_key);
            gen_data.implicit_key += 1;
        }
        gen_data.last_yielded_value = gen_data.value.clone_closure_capture();
        gen_data.last_yielded_key = gen_data.key.clone_closure_capture();

        let snapshot = snapshot_suspended_generator_frame(
            frame,
            Some(op_array),
            true,
            std::mem::take(&mut gen_data.cv_values),
            std::mem::take(&mut gen_data.tmp_values),
        );
        gen_data.cv_values = snapshot.cv_values;
        gen_data.tmp_values = snapshot.tmp_values;
        gen_data.ip_offset = snapshot.ip_offset;
        gen_data.pending_return_after_finally = snapshot.pending_return_after_finally;
        gen_data.pending_finally_exceptions = pending_finally_exceptions;
        gen_data.state = GeneratorState::Suspended;

        drop(gen_data);
        eg.active_generator = Some(gen_ref);
    }

    // Return from generator frame (like OpCode::Return)
    let prev = unsafe { (*frame).prev_execute_data };
    if prev.is_null() {
        return Ok(ColdResult::Return);
    }
    eg.current_execute_data.set(prev);
    unsafe { cleanup_frame_slots(frame) };
    pop_vm_call_frame(eg, frame);
    Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }))
}

#[inline(never)]
fn resolve_yield_from_source(
    eg: &mut ExecutorGlobals,
    source: &Value,
) -> Result<Option<YieldFromSource>, VmError> {
    let Some(object) = source.as_object() else {
        return Ok(source.as_array().map(|array| {
            YieldFromSource::Array(
                array
                    .iter()
                    .map(|(key, value)| (key, value.clone()))
                    .collect(),
                array.has_external_byte_keys(),
            )
        }));
    };
    let mut class_name = object.class_name.to_string();
    if class_name == "Generator" {
        let generator = object.generator.clone();
        drop(object);
        return Ok(generator.map(|generator| {
            YieldFromSource::Generator(
                generator,
                crate::vm::generator::YieldFromGeneratorMode::Direct,
            )
        }));
    }
    drop(object);

    if !eg.class_is_a(&class_name, "Traversable") {
        return Ok(None);
    }

    let mut iterable = source.clone();
    let mut aggregate_identities = Vec::new();
    while eg.class_is_a(&class_name, "IteratorAggregate") {
        let identity = iterable.object_identity().unwrap_or(0);
        if aggregate_identities.contains(&identity) {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {class_name}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(None);
        }
        aggregate_identities.push(identity);
        let aggregate_class = class_name.clone();
        let Some(next) = crate::stdlib::call_object_protocol_method(
            eg,
            &iterable,
            "IteratorAggregate",
            "getIterator",
            &[],
        )? else {
            return Err(VmError::Fatal(format!(
                "Call to undefined method {class_name}::getIterator()"
            )));
        };
        if eg.exception.is_some() {
            return Ok(None);
        }
        iterable = next;
        let Some(object) = iterable.as_object() else {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {aggregate_class}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(None);
        };
        class_name = object.class_name.to_string();
        drop(object);
        if !eg.class_is_a(&class_name, "Traversable") {
            eg.exception = Some(make_error_value(
                "Exception",
                &format!(
                    "Objects returned by {aggregate_class}::getIterator() must be traversable or implement interface Iterator"
                ),
            ));
            return Ok(None);
        }
    }

    if class_name == "Generator" {
        let generator = iterable
            .as_object_rc()
            .and_then(|object| object.borrow().generator.clone());
        return Ok(generator.map(|generator| {
            YieldFromSource::Generator(
                generator,
                crate::vm::generator::YieldFromGeneratorMode::Traversable,
            )
        }));
    }
    if eg.class_is_a(&class_name, "Iterator") {
        return Ok(Some(YieldFromSource::Iterator(crate::stdlib::prepare_native_deque_consumer(&iterable, eg).unwrap_or(iterable))));
    }
    Ok(None)
}

enum YieldFromSource {
    Generator(
        crate::vm::generator::GeneratorRef,
        crate::vm::generator::YieldFromGeneratorMode,
    ),
    Array(Vec<(crate::value::ArrayKey, Value)>, bool),
    Iterator(Value),
}

fn yield_from_iterator_step(
    eg: &mut ExecutorGlobals,
    iterator: &Value,
    first: bool,
) -> Result<Option<(Value, Value)>, VmError> {
    let method = if first { "rewind" } else { "next" };
    let _ = crate::stdlib::call_object_protocol_method(
        eg,
        iterator,
        "Iterator",
        method,
        &[],
    )?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let valid = crate::stdlib::call_object_protocol_method(
        eg,
        iterator,
        "Iterator",
        "valid",
        &[],
    )?
    .unwrap_or_else(|| Value::bool(false));
    if eg.exception.is_some() || !valid.is_truthy() {
        return Ok(None);
    }
    // Zend observes current() before key() when advancing an Iterator-backed
    // yield-from delegate. Keep this separate from foreach's fetch order.
    let value = crate::stdlib::call_object_protocol_method(
        eg,
        iterator,
        "Iterator",
        "current",
        &[],
    )?
    .unwrap_or_else(Value::null);
    if eg.exception.is_some() {
        return Ok(None);
    }
    let key = crate::stdlib::call_object_protocol_method(
        eg,
        iterator,
        "Iterator",
        "key",
        &[],
    )?
    .unwrap_or_else(Value::null);
    if eg.exception.is_some() {
        return Ok(None);
    }
    Ok(Some((key, value)))
}

fn throw_yield_from_exception<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    exception: Value,
) -> Result<ColdResult<'a>, VmError> {
    Ok(match throw_in_frame(eg, frame, exception)? {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
    })
}

fn suspend_yield_from<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    generator: crate::vm::generator::GeneratorRef,
    delegate: crate::vm::generator::YieldFromDelegate,
    key: Value,
    value: Value,
) -> ColdResult<'a> {
    use crate::vm::generator::GeneratorState;

    let pending_finally_exceptions = eg
        .finally_exceptions
        .remove(&(frame as usize))
        .unwrap_or_default();
    {
        let mut data = generator.borrow_mut();
        data.delegate = Some(delegate);
        data.yield_from_result_slot = opline.result as u32;
        data.value = value;
        data.key = key;
        data.last_yielded_value = data.value.clone_closure_capture();
        data.last_yielded_key = data.key.clone_closure_capture();
        let snapshot = snapshot_suspended_generator_frame(
            frame,
            Some(op_array),
            false,
            std::mem::take(&mut data.cv_values),
            std::mem::take(&mut data.tmp_values),
        );
        data.cv_values = snapshot.cv_values;
        data.tmp_values = snapshot.tmp_values;
        data.ip_offset = snapshot.ip_offset;
        data.pending_return_after_finally = snapshot.pending_return_after_finally;
        data.pending_finally_exceptions = pending_finally_exceptions;
        data.state = GeneratorState::Suspended;
    }

    eg.active_generator = Some(generator);
    // SAFETY: `frame` is still the active generator activation. Its predecessor
    // remains live while this frame is cleaned and popped, and therefore owns
    // a valid immutable op-array for the returned dispatch control.
    let previous = unsafe { (*frame).prev_execute_data };
    if previous.is_null() {
        return ColdResult::Return;
    }
    eg.current_execute_data.set(previous);
    unsafe { cleanup_frame_slots(frame) };
    pop_vm_call_frame(eg, frame);
    ColdResult::NewFrame(previous, unsafe { (*previous).op_array() })
}

#[inline(never)]
fn op_yield_from<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    if eg
        .active_generator
        .as_ref()
        .is_some_and(|generator| generator.borrow().force_closing)
    {
        let error = make_error_value(
            "Error",
            "Cannot use \"yield from\" in a force-closed generator",
        );
        let instruction_index = generator_instruction_index(op_array, opline);
        let origin_index = (0..=instruction_index)
            .rev()
            .find(|index| op_array.source_line(*index).is_some())
            .unwrap_or(instruction_index);
        attach_new_throwable_origin(
            &error,
            eg,
            frame,
            op_array,
            origin_index,
        );
        return throw_yield_from_exception(eg, frame, error);
    }

    let source_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }.clone();

    if let Some(gen_ref) = eg.active_generator.take() {
        let result_slot = opline.result as u32;
        let source = match resolve_yield_from_source(eg, &source_val) {
            Ok(source) => source,
            Err(error) => {
                eg.active_generator = Some(gen_ref);
                return Err(error);
            }
        };
        if let Some(exception) = eg.exception.take() {
            eg.active_generator = Some(gen_ref);
            return Ok(throw_yield_from_exception(eg, frame, exception)?);
        }
        let Some(source) = source else {
            eg.active_generator = Some(gen_ref);
            let error = make_error_value(
                "Error",
                "Can use \"yield from\" only with arrays and Traversables",
            );
            return Ok(throw_yield_from_exception(eg, frame, error)?);
        };

        match source {
            YieldFromSource::Generator(inner, return_mode) => {
                if std::rc::Rc::ptr_eq(&gen_ref, &inner) {
                    eg.active_generator = Some(gen_ref);
                    let error = make_error_value(
                        "Error",
                        "Impossible to yield from the Generator being currently run",
                    );
                    return Ok(throw_yield_from_exception(eg, frame, error)?);
                }
                let inner_state = inner.borrow().state;
                if return_mode == crate::vm::generator::YieldFromGeneratorMode::Traversable {
                    let protocol_error = match inner_state {
                        GeneratorState::Completed => {
                            Some("Cannot traverse an already closed generator")
                        }
                        GeneratorState::Suspended if !inner.borrow().rewindable => {
                            Some("Cannot rewind a generator that was already run")
                        }
                        GeneratorState::Created
                        | GeneratorState::Suspended
                        | GeneratorState::Running => None,
                    };
                    if let Some(message) = protocol_error {
                        eg.active_generator = Some(gen_ref);
                        let exception = make_error_value("Exception", message);
                        return Ok(throw_yield_from_exception(eg, frame, exception)?);
                    }
                }
                if inner_state == GeneratorState::Completed {
                    if !inner.borrow().has_returned {
                        eg.active_generator = Some(gen_ref);
                        let error = make_error_value(
                            "Error",
                            "Generator passed to yield from was aborted without proper return and is unable to continue",
                        );
                        let instruction_index = unsafe {
                            (opline as *const Instruction)
                                .offset_from(op_array.instructions.as_ptr())
                                as usize
                        };
                        attach_throwable_origin(
                            &error,
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                        );
                        return Ok(throw_yield_from_exception(eg, frame, error)?);
                    }
                    let result = if return_mode
                        == crate::vm::generator::YieldFromGeneratorMode::Direct
                    {
                        inner.borrow().return_value.clone()
                    } else {
                        Value::null()
                    };
                    eg.active_generator = Some(gen_ref);
                    if opline.result_type != OpType::Unused {
                        // SAFETY: `result_slot` is compiler-allocated by this
                        // live YieldFrom instruction in the active frame.
                        let slot = unsafe { (*frame).slot_mut(result_slot) };
                        unsafe { frame_tmp_set(frame, slot as *mut Value, result) };
                    }
                    // SAFETY: `opline` is the current instruction inside this
                    // immutable op-array, so its successor is the continuation.
                    unsafe { (*frame).opline = (*frame).opline.add(1) };
                    return Ok(ColdResult::Continue);
                }
                let (key, value) = {
                    let inner_data = inner.borrow();
                    (inner_data.key.clone(), inner_data.value.clone())
                };
                Ok(suspend_yield_from(
                    eg,
                    frame,
                    op_array,
                    opline,
                    gen_ref,
                    YieldFromDelegate::Generator(inner, return_mode),
                    key,
                    value,
                ))
            }
            YieldFromSource::Array(entries, external_byte_keys) => {
                if entries.is_empty() {
                    eg.active_generator = Some(gen_ref);
                    if opline.result_type != OpType::Unused {
                        // SAFETY: `result_slot` is compiler-allocated by this
                        // live YieldFrom instruction in the active frame.
                        let slot = unsafe { (*frame).slot_mut(result_slot) };
                        unsafe { frame_tmp_set(frame, slot as *mut Value, Value::null()) };
                    }
                    // SAFETY: `opline` is the current instruction inside this
                    // immutable op-array, so its successor is the continuation.
                    unsafe { (*frame).opline = (*frame).opline.add(1) };
                    return Ok(ColdResult::Continue);
                }
                let (key, value) = {
                    let (key, value) = &entries[0];
                    let key = match key {
                        crate::value::ArrayKey::Int(key) => Value::long(*key),
                        crate::value::ArrayKey::String(key) if external_byte_keys => {
                            Value::binary_string_from_storage(key.clone())
                        }
                        crate::value::ArrayKey::String(key) => Value::string(key),
                    };
                    (key, value.clone())
                };
                Ok(suspend_yield_from(
                    eg,
                    frame,
                    op_array,
                    opline,
                    gen_ref,
                    YieldFromDelegate::Array(entries, 1, external_byte_keys),
                    key,
                    value,
                ))
            }
            YieldFromSource::Iterator(iterator) => {
                let step = match yield_from_iterator_step(eg, &iterator, true) {
                    Ok(step) => step,
                    Err(error) => {
                        eg.active_generator = Some(gen_ref);
                        return Err(error);
                    }
                };
                if let Some(exception) = eg.exception.take() {
                    eg.active_generator = Some(gen_ref);
                    return Ok(throw_yield_from_exception(eg, frame, exception)?);
                }
                let Some((key, value)) = step else {
                    eg.active_generator = Some(gen_ref);
                    if opline.result_type != OpType::Unused {
                        // SAFETY: `result_slot` is compiler-allocated by this
                        // live YieldFrom instruction in the active frame.
                        let slot = unsafe { (*frame).slot_mut(result_slot) };
                        unsafe { frame_tmp_set(frame, slot as *mut Value, Value::null()) };
                    }
                    // SAFETY: `opline` is the current instruction inside this
                    // immutable op-array, so its successor is the continuation.
                    unsafe { (*frame).opline = (*frame).opline.add(1) };
                    return Ok(ColdResult::Continue);
                };
                Ok(suspend_yield_from(
                    eg,
                    frame,
                    op_array,
                    opline,
                    gen_ref,
                    YieldFromDelegate::Iterator(iterator),
                    key,
                    value,
                ))
            }
        }
    } else {
        Err(VmError::Fatal("yield from outside generator".into()))
    }
}
