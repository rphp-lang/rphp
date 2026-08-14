// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline]
fn assign_foreach_cv(frame: *mut ExecuteData, cv: u32, value: Value) {
    // SAFETY: `cv` is compiler-allocated in the active frame. Assignment may
    // follow a reference target outside the frame, so only direct CV writes use
    // frame bitmap bookkeeping.
    unsafe {
        let slot = (*frame).cv_mut(cv);
        if (*slot).is_reference() {
            slot_set((*slot).as_ref_ptr(), value);
        } else {
            frame_slot_set(frame, slot, value);
        }
    }
}

fn unpack_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    class: &str,
    message: &str,
) -> ColdResult<'a> {
    let error = make_error_value(class, message);
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
    message: &str,
) -> ColdResult<'a> {
    unpack_throw(eg, frame, "Error", message)
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
    // SAFETY: op2 is a compiler-allocated live operand. Array literal unpack
    // reads it synchronously before mutating the separate op1 temporary.
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
    }
    .dereferenced();
    let entries = if let Some(source) = source.as_array() {
        Some(
            source
                .iter()
                .map(|(key, value)| (key, value.dereferenced().clone()))
                .collect::<Vec<_>>(),
        )
    } else {
        collect_unpack_traversable(eg, source, TraversableUnpackKind::Array)?
    };
    let Some(entries) = entries else {
        return Ok(unpack_error(
            eg,
            frame,
            "Only arrays and Traversables can be unpacked",
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
            return Ok(unpack_error(eg, frame, message));
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
    let entries = unsafe {
        if opline.op2_type == OpType::Const {
            let mut source =
                (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone();
            collect_source(eg, &mut source)?
        } else {
            let source_ptr = (*frame).get_op_mut(opline.op2 as u32, opline.op2_type);
            let source_ptr = if (*source_ptr).is_reference() {
                (*source_ptr).as_ref_ptr()
            } else {
                source_ptr
            };
            collect_source(eg, &mut *source_ptr)?
        }
    };

    let entries = match entries {
        Some(entries) => entries,
        None => {
            return Ok(unpack_throw(
                eg,
                frame,
                "TypeError",
                "Only arrays and Traversables can be unpacked",
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
            return Ok(unpack_error(eg, frame, &message));
        }
    }
    Ok(ColdResult::Done)
}

#[inline]
fn bind_foreach_value_cv(frame: *mut ExecuteData, cv: u32, value: Value) {
    // SAFETY: `cv` is compiler-allocated in the active frame. A by-reference
    // foreach value rebinds this CV itself, so the destination remains a frame
    // slot and must use frame bitmap bookkeeping.
    unsafe {
        let slot = (*frame).cv_mut(cv);
        frame_slot_set(frame, slot, value);
    }
}

#[inline]
fn clone_foreach_value<const BY_REFERENCE_LOOP: bool>(value: &Value) -> Value {
    if BY_REFERENCE_LOOP && value.is_owned_reference() {
        value.clone_owned_reference_alias()
    } else if BY_REFERENCE_LOOP && value.is_reference() {
        // SAFETY: the detached foreach array retains the borrowed target for
        // the lifetime of the loop-bound alias.
        Value::reference(unsafe { value.as_ref_ptr() })
    } else {
        value.clone()
    }
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
    // SAFETY: ForeachInit's source operand is a compiler-validated live-frame
    // slot and remains borrowed only until this opcode finishes.
    let source = unsafe {
        &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
    };
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
            }
        }
        let is_valid = gen_ref.borrow().state != crate::vm::generator::GeneratorState::Completed;
        if !is_valid {
            let target = opline.op2 as usize;
            let base_ptr = op_array.instructions.as_ptr();
            unsafe { (*frame).opline = base_ptr.add(target) };
            return Ok(ColdResult::Continue);
        }
        // Store generator object in result TMP
        let cloned = arr_val.clone();
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, cloned) };
        // Set position TMP to 0 (0 = first iteration, don't call next)
        let pos_ptr = unsafe { (*frame).get_op_mut(opline.extended_value, OpType::Tmp) };
        unsafe { frame_tmp_set_long(frame, pos_ptr, 0) };
    } else {
        let iterator_values = arr_val.as_object().and_then(|object| {
            matches!(
                object.class_name.as_ref(),
                "ArrayIterator" | "ArrayObject" | "SplObjectStorage" | "SplPriorityQueue"
            )
                .then(|| object.get_property("__rphp_iterator_values").cloned())
                .flatten()
        });
        let iterable = iterator_values.as_ref().unwrap_or(arr_val);
        let is_empty = match iterable.as_array() {
            Some(arr) => arr.is_empty(),
            None => {
                eg.write_output(b"\nWarning: foreach() argument must be of type array|object, ");
                let type_name = match arr_val.value_type() {
                    ValueType::Null => "null",
                    ValueType::True | ValueType::False => "bool",
                    ValueType::Long => "int",
                    ValueType::Double => "float",
                    ValueType::String => "string",
                    _ => "unknown",
                };
                eg.write_output(type_name.as_bytes());
                eg.write_output(b" given\n");
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
        let cloned = iterable.clone();
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { frame_result_set(frame, result_ptr, opline.result_type, cloned) };
        // Set position TMP to 0
        let pos_ptr = unsafe { (*frame).get_op_mut(opline.extended_value, OpType::Tmp) };
        unsafe { frame_tmp_set_long(frame, pos_ptr, 0) };
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

    let arr_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };

    // Check for Generator object
    let gen_ref_opt = if let Some(obj) = arr_val.as_object() {
        if obj.class_name.as_ref() == "Generator" {
            arr_val.as_object_rc().and_then(|rc| rc.borrow().generator.clone())
        } else { None }
    } else { None };

    let has_more = if let Some(gen_ref) = gen_ref_opt {
        let pos_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let pos = pos_val.as_long().unwrap_or(0);

        // On first iteration (pos=0), generator is already started by ForeachInit
        // On subsequent iterations, call next()
        if pos > 0 {
            let state = gen_ref.borrow().state;
            if state == crate::vm::generator::GeneratorState::Suspended {
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
                    frame,
                    val_cv,
                    clone_foreach_value::<BY_REFERENCE_LOOP>(&gen_data.value),
                );
            } else {
                assign_foreach_cv(frame, val_cv, gen_data.value.clone());
            }
            // Write key if requested
            if key_encoded > 0 {
                let key_cv = key_encoded - 1;
                assign_foreach_cv(frame, key_cv, gen_data.key.clone());
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
        let pos_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
        let pos = pos_val.as_long().unwrap_or(0) as usize;

        if let Some(arr) = arr_val.as_array() {
            if pos < arr.len() {
                if key_encoded > 0 {
                    // Need both key and value — use get_at()
                    let (val, key) = arr.get_at(pos).unwrap();
                    if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                        bind_foreach_value_cv(
                            frame,
                            val_cv,
                            clone_foreach_value::<BY_REFERENCE_LOOP>(val),
                        );
                    } else {
                        assign_foreach_cv(frame, val_cv, val.clone());
                    }
                    let key_cv = key_encoded - 1;
                    let key_val = match key {
                        ArrayKey::Int(k) => Value::long(k),
                        ArrayKey::String(k) => Value::string(k),
                    };
                    assign_foreach_cv(frame, key_cv, key_val);
                } else {
                    // Only value needed — use get_value_at() (avoids key clone)
                    let val = arr.get_value_at(pos).unwrap();
                    if BY_REFERENCE_LOOP || !ASSIGN_THROUGH_REFERENCE {
                        bind_foreach_value_cv(
                            frame,
                            val_cv,
                            clone_foreach_value::<BY_REFERENCE_LOOP>(val),
                        );
                    } else {
                        assign_foreach_cv(frame, val_cv, val.clone());
                    }
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

        gen_data.send_value = Value::null();

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
                    // Start inner generator if needed
                    {
                        let inner_state: GeneratorState = inner_gen_ref.borrow().state;
                        if inner_state == GeneratorState::Created {
                            match resume_generator(eg, &inner_gen_ref, Value::null())? {
                                GeneratorResumeOutcome::Advanced => {}
                                GeneratorResumeOutcome::Threw(exception) => {
                                    eg.active_generator = Some(gen_ref);
                                    return Ok(match throw_in_frame(eg, frame, exception) {
                                        ThrowResult::Handled(new_frame, new_op_array) => {
                                            ColdResult::NewFrame(new_frame, new_op_array)
                                        }
                                        ThrowResult::Unhandled(exception) => {
                                            ColdResult::Unhandled(exception)
                                        }
                                    });
                                }
                            }
                        }
                    }

                    let inner_state: GeneratorState = inner_gen_ref.borrow().state;
                    if inner_state == GeneratorState::Completed {
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
