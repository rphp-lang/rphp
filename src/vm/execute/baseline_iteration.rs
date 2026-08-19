// Kept in the execute module through include! so this structural split does not change visibility or code generation.

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
        let destructor = prepare_replaced_value_destructor_with_references(
            eg,
            &*target,
            replaced_references,
        );
        if target == slot {
            frame_slot_set(frame, slot, value);
        } else {
            slot_set(target, value);
        }
        if let Some(global_name) = mirrored_global_name {
            globals_set(&mut eg.globals, global_name, (&*target).clone());
        }
        run_prepared_value_destructor(eg, destructor)?;
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
) -> ColdResult<'a> {
    let error = make_error_value(class, message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }
}

fn unpack_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    is_root_frame: bool,
    message: &str,
) -> ColdResult<'a> {
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
) -> Result<ArrayKey, String> {
    let value = value.dereferenced();
    if let Some(key) = value.as_long() {
        Ok(ArrayKey::Int(key))
    } else if let Some(key) = value.as_str() {
        Ok(ArrayKey::String(key.to_string()))
    } else {
        Err(kind.key_error().to_string())
    }
}

fn collect_generator_unpack(
    eg: &mut ExecutorGlobals,
    value: &Value,
    kind: TraversableUnpackKind,
) -> Result<Vec<(ArrayKey, Value)>, VmError> {
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
        let key = match traversable_unpack_key(&data.key, kind) {
            Ok(key) => key,
            Err(message) => {
                eg.exception = Some(make_error_value("Error", &message));
                return Ok(entries);
            }
        };
        entries.push((
            key,
            kind.value(data.value.dereferenced().clone()),
        ));
    }
    Ok(entries)
}

fn collect_unpack_traversable(
    eg: &mut ExecutorGlobals,
    source: &Value,
    kind: TraversableUnpackKind,
) -> Result<Option<Vec<(ArrayKey, Value)>>, VmError> {
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

    if let Some(values) = iterable.as_object().and_then(|object| {
        matches!(
            object.class_name.as_ref(),
            "ArrayIterator" | "ArrayObject" | "SplObjectStorage" | "SplPriorityQueue"
        )
        .then(|| object.get_property("__rphp_iterator_values").cloned())
        .flatten()
    }) && let Some(values) = values.as_array()
    {
        return Ok(Some(
            values
                .iter()
                .map(|(key, value)| {
                    (
                        key,
                        kind.value(value.dereferenced().clone()),
                    )
                })
                .collect(),
        ));
    }

    if !eg.class_is_a(&class_name, "Iterator") {
        return Ok(None);
    }
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
        let key = match traversable_unpack_key(&key, kind) {
            Ok(key) => key,
            Err(message) => {
                eg.exception = Some(make_error_value("Error", &message));
                break;
            }
        };
        entries.push((key, kind.value(value.dereferenced().clone())));
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

/// Collect one canonical Traversable for stdlib consumers that need the same
/// Generator, IteratorAggregate and Iterator semantics as foreach/unpacking.
pub(crate) fn collect_traversable_entries(
    eg: &mut ExecutorGlobals,
    source: &Value,
) -> Result<Option<Vec<(ArrayKey, Value)>>, VmError> {
    collect_unpack_traversable(eg, source, TraversableUnpackKind::Array)
}

fn append_array_unpack_entry(
    target: &mut PhpArray,
    key: ArrayKey,
    value: Value,
) -> Result<(), &'static str> {
    match key {
        ArrayKey::Int(_) => {
            if !target.try_push(value) {
                return Err("Cannot add element to the array as the next element is already occupied");
            }
        }
        ArrayKey::String(key) => {
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
        Some(
            source
                .iter()
                .map(|(key, value)| (key, value.dereferenced().clone()))
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
        ));
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
        ));
    };
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception) {
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
    for (key, value) in entries {
        if let Err(message) = append_array_unpack_entry(target, key, value) {
            return Ok(unpack_error(
                eg,
                frame,
                op_array,
                instruction_index,
                is_root_frame,
                message,
            ));
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
            ));
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
     -> Result<Option<Vec<(ArrayKey, Value)>>, VmError> {
        if let Some(source) = source.as_array_mut() {
            let keys: Vec<_> = source.iter().map(|(key, _)| key).collect();
            return keys
                .into_iter()
                .enumerate()
                .map(|(position, key)| {
                    source
                        .argument_unpack_reference_at(position)
                        .map(|value| (key, value))
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
                source
                    .as_object()
                    .map(|object| object.class_name.to_string())
                    .unwrap_or_else(|| source.type_name().to_string())
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
            ));
        }
    };
    if let Some(exception) = eg.exception.take() {
        return Ok(match throw_in_frame(eg, frame, exception) {
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
    for (key, value) in entries {
        if let Err(message) = append_call_unpack_entry(target, key, value, &mut seen_named) {
            return Ok(unpack_error(
                eg,
                frame,
                op_array,
                usize::MAX,
                false,
                &message,
            ));
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
        let destructor = prepare_replaced_value_destructor(eg, &*slot);
        frame_slot_set(frame, slot, value);
        run_prepared_value_destructor(eg, destructor)?;
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
fn set_foreach_object_entry(array: &mut PhpArray, name: &str, value: Value) {
    if let Some(key) = canonical_decimal_array_key(name) {
        array.set_int(key, value);
    } else {
        array.set_str(name, value);
    }
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
            call_object_property_get_hook(eg, value, &definition.name)?
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
    iterable: Value,
    position: i64,
) {
    // SAFETY: ForeachInit's result and position operands are compiler-allocated
    // live TMP slots in this frame and neither pointer escapes this helper.
    unsafe {
        let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        frame_result_set(frame, result, opline.result_type, iterable);
        let cursor = (*frame).get_op_mut(opline.extended_value, OpType::Tmp);
        frame_tmp_set_long(frame, cursor, position);
    }
}

#[inline]
fn take_foreach_protocol_exception<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
) -> Option<ColdResult<'a>> {
    let exception = eg.exception.take()?;
    Some(match throw_in_frame(eg, frame, exception) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
    })
}

#[inline]
fn uses_user_iterator_protocol(value: &Value, eg: &ExecutorGlobals) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let class_name = object.class_name.to_string();
    drop(object);
    !matches!(
        class_name.as_str(),
        "Generator" | "ArrayIterator" | "ArrayObject" | "SplObjectStorage" | "SplPriorityQueue"
    ) && eg.class_is_a(&class_name, "Iterator")
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

        let value = (&*(*frame).get_op_ptr(value_cv, OpType::Cv, op_array)).clone();
        let array_ptr = (*frame).get_op_mut(array_operand as u32, array_type);
        let array = &mut *array_ptr;
        if array.is_reference() {
            // A CV-backed by-reference foreach aliases the source array
            // directly. Its element reference cell is updated by ordinary CV
            // assignment, so there is no detached snapshot to flush.
            return Ok(());
        }
        if let Some(object) = array.as_object()
            && object.class_name.as_ref() != "Generator"
        {
            return Ok(());
        }
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
    // A CV array is promoted to an owned cell before either side mutates it.
    let (init_ip, by_reference, live_source_alias) = unsafe {
        let init_ip =
            (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize;
        let by_reference = op_array
            .instructions
            .get(init_ip + 1)
            .is_some_and(|next| next.opcode == OpCode::ForeachNextRef);
        let source = (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let live_source_alias = (by_reference
            && opline.op1_type == OpType::Cv
            && matches!(
                (&*source).dereferenced().value_type(),
                ValueType::Array | ValueType::Object
            ))
        .then(|| materialize_reference_alias(frame, (*frame).cv_mut(opline.op1 as u32)));
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
    if let Some(control) = take_foreach_protocol_exception(eg, frame) {
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
            return Ok(match throw_in_frame(eg, frame, error) {
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
            return Ok(match throw_in_frame(eg, frame, exception) {
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
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        resolved_iterable = Some(next);
    }
    let arr_val = resolved_iterable.as_ref().unwrap_or(source);

    // Check for Generator object
    let is_generator = if let Some(obj) = arr_val.as_object() {
        obj.class_name.as_ref() == "Generator" && arr_val.as_object_rc().map_or(false, |rc| rc.borrow().generator.is_some())
    } else {
        false
    };

    if is_generator {
        // Start the generator (rewind)
        let gen_ref = arr_val.as_object_rc().unwrap().borrow().generator.clone().unwrap();
        {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Created {
                let outcome = resume_generator(eg, &gen_ref, Value::null())?;
                match generator_resume_result(eg, frame, outcome) {
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
                return Ok(match throw_in_frame(eg, frame, error) {
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
        set_foreach_iteration_state(frame, opline, arr_val.clone(), 0);
    } else {
        if uses_user_iterator_protocol(arr_val, eg) {
            if by_reference && !eg.weak_iterator_allows_references(arr_val) {
                let error = make_error_value(
                    "Error",
                    "An iterator cannot be used with foreach by reference",
                );
                return Ok(match throw_in_frame(eg, frame, error) {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
            if by_reference {
                eg.enable_weak_iterator_references(arr_val);
            }
            let _ = crate::stdlib::call_object_protocol_method(
                eg,
                arr_val,
                "Iterator",
                "rewind",
                &[],
            )?;
            if let Some(control) = take_foreach_protocol_exception(eg, frame) {
                return Ok(control);
            }
            // Negative cursor values identify the user Iterator protocol. Each
            // successful fetch decrements it, retaining first-vs-next state
            // without a class lookup in the hot ForeachNext path.
            set_foreach_iteration_state(frame, opline, arr_val.clone(), -1);
            return Ok(ColdResult::Done);
        }
        let iterator_values = arr_val.as_object().and_then(|object| {
            matches!(
                object.class_name.as_ref(),
                "ArrayIterator" | "ArrayObject" | "SplObjectStorage" | "SplPriorityQueue"
            )
                .then(|| object.get_property("__rphp_iterator_values").cloned())
                .flatten()
        });
        let object_values = if iterator_values.is_none() && arr_val.as_object().is_some() {
            let materialized = if by_reference {
                arr_val.clone()
            } else {
                materialize_foreach_object(arr_val, eg, frame)?
            };
            if let Some(control) = take_foreach_protocol_exception(eg, frame) {
                return Ok(control);
            }
            Some(materialized)
        } else {
            None
        };
        let iterable = iterator_values
            .as_ref()
            .or(object_values.as_ref())
            .unwrap_or(arr_val);
        let is_empty = match iterable.dereferenced().as_array() {
            Some(arr) => arr.is_empty(),
            None if iterable.value_type() == ValueType::Object => false,
            None => {
                let type_name = match arr_val.value_type() {
                    ValueType::Null => "null",
                    ValueType::True | ValueType::False => "bool",
                    ValueType::Long => "int",
                    ValueType::Double => "float",
                    ValueType::String => "string",
                    _ => "unknown",
                };
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
                    return Ok(match throw_in_frame(eg, frame, exception) {
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
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(ColdResult::Continue);
        }
        // Copy array to result TMP
        let cloned = if let Some(live_source_alias) = live_source_alias.as_ref()
            && resolved_iterable.is_none()
            && iterator_values.is_none()
        {
            clone_foreach_value::<true>(live_source_alias)
        } else {
            iterable.clone()
        };
        set_foreach_iteration_state(frame, opline, cloned, 0);
    }
    Ok(ColdResult::Done)
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

    // SAFETY: both operands are compiler-allocated slots in this live frame;
    // neither shared borrow escapes this synchronous iteration opcode.
    let (iteration_state, cursor) = unsafe {
        let iteration_state =
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let cursor = (&*(*frame).get_op_ptr(
            opline.op2 as u32,
            opline.op2_type,
            op_array,
        ))
            .as_long()
            .unwrap_or(0);
        (iteration_state, cursor)
    };
    let source = iteration_state.dereferenced();
    let lazy_source_owner = eg.lazy_object_state(source).map(|_| source.clone());
    let source = lazy_source_owner.as_ref().unwrap_or(source);
    let initialized_source = if eg.lazy_object_state(source).is_some() {
        Some(crate::stdlib::reflection::resolve_lazy_object_chain(
            eg, source,
        )?)
    } else {
        None
    };
    if let Some(control) = take_foreach_protocol_exception(eg, frame) {
        return Ok(control);
    }
    let arr_val = initialized_source.as_ref().unwrap_or(source);
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
            if let Some(control) = take_foreach_protocol_exception(eg, frame) {
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
        if let Some(control) = take_foreach_protocol_exception(eg, frame) {
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
            if let Some(control) = take_foreach_protocol_exception(eg, frame) {
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
                if let Some(control) = take_foreach_protocol_exception(eg, frame) {
                    return Ok(control);
                }
                assign_foreach_cv(eg, frame, key_encoded - 1, key.dereferenced().clone())?;
            }
            let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
            unsafe {
                frame_result_set(
                    frame,
                    pos_ptr,
                    opline.op2_type,
                    Value::long(cursor - 1),
                )
            };
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
                let control = generator_resume_result(eg, frame, outcome);
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
                        if key_encoded > 0 {
                            let key_cv = key_encoded - 1;
                            let key = iteration_state
                                .dereferenced()
                                .as_array()
                                .and_then(|array| array.get_at(pos))
                                .map(|(_, key)| key)
                                .expect("promoted foreach entry must retain its key");
                            let key_value = match key {
                                ArrayKey::Int(key) => Value::long(key),
                                ArrayKey::String(key) => Value::string(key),
                            };
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
                        let key_cv = key_encoded - 1;
                        let key_val = match key {
                            ArrayKey::Int(k) => Value::long(k),
                            ArrayKey::String(k) => Value::string(k),
                        };
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
        } else if BY_REFERENCE_LOOP && arr_val.value_type() == ValueType::Object {
            let caller_class = get_caller_class(frame, eg);
            let class_id = arr_val
                .as_object()
                .map(|object| object.class_id)
                .unwrap_or(0);
            let slots = {
                let object = arr_val.as_object().unwrap();
                eg.visible_instance_property_slots(class_id, caller_class.as_deref())
                    .into_iter()
                    .filter(|slot| {
                        let definition = eg.instance_property_definition(class_id, *slot);
                        (!object.property_values[*slot].is_undef()
                            || definition.is_some_and(|definition| definition.has_get_hook))
                            && definition.is_none_or(|definition| {
                                !definition.is_virtual_hook_property() || definition.has_get_hook
                            })
                    })
                    .collect::<Vec<_>>()
            };
            let dynamic_names = {
                let object = arr_val.as_object().unwrap();
                let declared_names = slots
                    .iter()
                    .filter_map(|slot| eg.instance_property_definition(class_id, *slot))
                    .map(|definition| definition.name.as_str())
                    .collect::<std::collections::HashSet<_>>();
                let mut names = Vec::new();
                object.for_each_dynamic_property(|name, property| {
                    if !property.is_undef() && !declared_names.contains(name) {
                        names.push(name.to_string());
                    }
                });
                names
            };
            if pos < slots.len() + dynamic_names.len() {
                let (name, value) = if pos < slots.len() {
                    let slot = slots[pos];
                    let (
                        name,
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
                            definition.name.clone(),
                            definition.declaring_class.clone(),
                            definition.type_scope.clone(),
                            definition.type_hint.clone(),
                            definition.is_typed(),
                            definition.is_readonly,
                            definition.has_get_hook,
                        )
                    };
                    if is_readonly {
                        let error = make_error_value(
                            "Error",
                            &format!(
                                "Cannot acquire reference to readonly property {}::${}",
                                declaring_class, name
                            ),
                        );
                        return Ok(match throw_in_frame(eg, frame, error) {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                        });
                    }
                    let value = if has_get_hook {
                        let hook_name = format!("${name}::get");
                        let returned = call_guarded_property_magic_method(
                            eg,
                            arr_val,
                            &name,
                            PROPERTY_GUARD_GET,
                            &hook_name,
                            &[],
                        )?
                        .unwrap_or_else(Value::null);
                        if let Some(control) = take_foreach_protocol_exception(eg, frame) {
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
                            return Ok(match throw_in_frame(eg, frame, error) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
                            });
                        }
                    } else {
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
                    };
                    (name, value)
                } else {
                    let name = dynamic_names[pos - slots.len()].clone();
                    let mut object = arr_val.as_object_mut().unwrap();
                    let value = promote_foreach_property_reference(
                        object
                            .get_dynamic_property_mut(&name)
                            .expect("dynamic property must remain addressable"),
                    );
                    (name, value)
                };
                bind_foreach_value_cv(eg, frame, val_cv, value)?;
                if key_encoded > 0 {
                    let key_cv = key_encoded - 1;
                    let key = canonical_decimal_array_key(&name)
                        .map(Value::long)
                        .unwrap_or_else(|| Value::string(name));
                    assign_foreach_cv(eg, frame, key_cv, key)?;
                }
                let pos_ptr = unsafe { (*frame).get_op_mut(opline.op2 as u32, opline.op2_type) };
                unsafe {
                    frame_result_set(
                        frame,
                        pos_ptr,
                        opline.op2_type,
                        Value::long((pos + 1) as i64),
                    )
                };
                true
            } else {
                false
            }
        } else {
            false
        }
    };

    if let Some(control) = take_foreach_protocol_exception(eg, frame) {
        return Ok(control);
    }

    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
    unsafe { frame_result_set(frame, result_ptr, opline.result_type, Value::bool(has_more)) };
    Ok(ColdResult::Done)
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
) -> ColdResult<'a> {
    match outcome {
        GeneratorResumeOutcome::Advanced => ColdResult::Done,
        GeneratorResumeOutcome::Threw(exception) => match throw_in_frame(eg, frame, exception) {
            ThrowResult::Handled(new_frame, new_op_array) => {
                ColdResult::NewFrame(new_frame, new_op_array)
            }
            ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
        },
    }
}

#[cold]
#[inline(never)]
fn mark_generator_not_rewindable(gen_ref: &crate::vm::generator::GeneratorRef) {
    gen_ref.borrow_mut().rewindable = false;
}

#[inline(never)]
fn op_yield<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::GeneratorState;

    let yielded_value = if opline.op1_type != OpType::Unused {
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

        // Save frame state back to generator
        let num_cvs = unsafe { (*frame).num_cvs } as usize;
        let num_temps = unsafe { (*frame).num_temps } as usize;
        gen_data.cv_values.clear();
        for i in 0..num_cvs {
            gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
        }
        gen_data.tmp_values.clear();
        for i in 0..num_temps {
            gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
        }

        // Save instruction pointer (advance past yield for resume)
        let base = op_array.instructions.as_ptr();
        gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize + 1 };
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
fn op_yield_from<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    use crate::vm::generator::{GeneratorState, YieldFromDelegate};

    let source_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) }.clone();

    if let Some(gen_ref) = eg.active_generator.take() {
        let result_slot = opline.result as u32;

        // Determine delegate type
        if let Some(obj_data) = source_val.as_object() {
            if obj_data.class_name.as_ref() == "Generator" {
                if let Some(inner_gen_ref) = obj_data.generator.clone() {
                    drop(obj_data);
                    if std::rc::Rc::ptr_eq(&gen_ref, &inner_gen_ref) {
                        eg.active_generator = Some(gen_ref);
                        let error = crate::value::make_error_value(
                            "Error",
                            "Impossible to yield from the Generator being currently run",
                        );
                        return Ok(match throw_in_frame(eg, frame, error) {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                ColdResult::NewFrame(new_frame, new_op_array)
                            }
                            ThrowResult::Unhandled(exception) => {
                                ColdResult::Unhandled(exception)
                            }
                        });
                    }
                    let inner_state: GeneratorState = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
                        if !inner_gen_ref.borrow().has_returned {
                            eg.active_generator = Some(gen_ref);
                            let error = crate::value::make_error_value(
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
                            return Ok(match throw_in_frame(eg, frame, error) {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    ColdResult::NewFrame(new_frame, new_op_array)
                                }
                                ThrowResult::Unhandled(exception) => {
                                    ColdResult::Unhandled(exception)
                                }
                            });
                        }
                        // Sub-generator already done, write return value to result
                        let ret_val = inner_gen_ref.borrow().return_value.clone();
                        eg.active_generator = Some(gen_ref);
                        // Write result to TMP and continue (don't suspend)
                        if opline.result_type != OpType::Unused {
                            let slot = unsafe { (*frame).slot_mut(result_slot) };
                            unsafe { frame_tmp_set(frame, slot as *mut Value, ret_val) };
                        }
                        unsafe { (*frame).opline = (*frame).opline.add(1); }
                        return Ok(ColdResult::Continue);
                    }

                    // Set up delegation
                    {
                        let mut gen_data = gen_ref.borrow_mut();
                        gen_data.delegate = Some(YieldFromDelegate::Generator(inner_gen_ref.clone()));
                        gen_data.yield_from_result_slot = result_slot;

                        // Copy inner generator's current value/key to outer
                        let inner = inner_gen_ref.borrow();
                        gen_data.value = inner.value.clone();
                        gen_data.key = inner.key.clone();

                        // Save frame state
                        let num_cvs = unsafe { (*frame).num_cvs } as usize;
                        let num_temps = unsafe { (*frame).num_temps } as usize;
                        gen_data.cv_values.clear();
                        for i in 0..num_cvs {
                            gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
                        }
                        gen_data.tmp_values.clear();
                        for i in 0..num_temps {
                            gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
                        }
                        let base = op_array.instructions.as_ptr();
                        gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize };
                        gen_data.state = GeneratorState::Suspended;
                    }

                    eg.active_generator = Some(gen_ref);

                    // Pop frame like Yield
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(ColdResult::Return);
                    }
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    pop_vm_call_frame(eg, frame);
                    return Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }));
                }
            }
            drop(obj_data);
            eg.active_generator = Some(gen_ref);
            let err = make_error_value("Error", "Can use \"yield from\" only with arrays and Traversables");
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                }
                ThrowResult::Unhandled(exc) => {
                    return Ok(ColdResult::Unhandled(exc));
                }
            }
        } else if let Some(arr) = source_val.as_array() {
            let entries: Vec<(crate::value::ArrayKey, Value)> = arr.iter().map(|(k, v)| (k, v.clone())).collect();

            if entries.is_empty() {
                // Empty array — result is null, continue
                eg.active_generator = Some(gen_ref);
                if opline.result_type != OpType::Unused {
                    let slot = unsafe { (*frame).slot_mut(result_slot) };
                    unsafe { frame_tmp_set(frame, slot as *mut Value, Value::null()) };
                }
                unsafe { (*frame).opline = (*frame).opline.add(1); }
                return Ok(ColdResult::Continue);
            }

            // Set up array delegation
            {
                let mut gen_data = gen_ref.borrow_mut();
                // Yield first element
                let (ref key, ref val) = entries[0];
                gen_data.value = val.clone();
                gen_data.key = match key {
                    crate::value::ArrayKey::Int(i) => Value::long(*i),
                    crate::value::ArrayKey::String(s) => Value::string(s.clone()),
                };
                gen_data.delegate = Some(YieldFromDelegate::Array(entries, 1)); // position after first
                gen_data.yield_from_result_slot = result_slot;

                // Save frame state
                let num_cvs = unsafe { (*frame).num_cvs } as usize;
                let num_temps = unsafe { (*frame).num_temps } as usize;
                gen_data.cv_values.clear();
                for i in 0..num_cvs {
                    gen_data.cv_values.push(unsafe { (*frame).cv(i as u32) }.clone());
                }
                gen_data.tmp_values.clear();
                for i in 0..num_temps {
                    gen_data.tmp_values.push(unsafe { (*frame).tmp(i as u32) }.clone());
                }
                let base = op_array.instructions.as_ptr();
                gen_data.ip_offset = unsafe { (*frame).opline.offset_from(base) as usize };
                gen_data.state = GeneratorState::Suspended;
            }

            eg.active_generator = Some(gen_ref);

            // Pop frame like Yield
            let prev = unsafe { (*frame).prev_execute_data };
            if prev.is_null() {
                return Ok(ColdResult::Return);
            }
            eg.current_execute_data.set(prev);
            unsafe { cleanup_frame_slots(frame) };
            pop_vm_call_frame(eg, frame);
            return Ok(ColdResult::NewFrame(prev, unsafe { (*prev).op_array() }));
        } else {
            eg.active_generator = Some(gen_ref);
            let err = make_error_value("Error", "Can use \"yield from\" only with arrays and Traversables");
            match throw_in_frame(eg, frame, err) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    return Ok(ColdResult::NewFrame(new_frame, new_op_array));
                }
                ThrowResult::Unhandled(exc) => {
                    return Ok(ColdResult::Unhandled(exc));
                }
            }
        }
    } else {
        return Err(VmError::Fatal("yield from outside generator".into()));
    }
}
