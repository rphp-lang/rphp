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
                bind_foreach_value_cv(frame, val_cv, gen_data.value.clone());
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
                        bind_foreach_value_cv(frame, val_cv, val.clone());
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
                        bind_foreach_value_cv(frame, val_cv, val.clone());
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
