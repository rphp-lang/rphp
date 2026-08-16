// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
fn op_nullsafe_check<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: every operand and jump target belongs to the live `frame` and
    // `op_array`; result publication records ownership for TMP/VAR slots.
    unsafe {
        let val = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        // PHP references are transparent to the nullsafe receiver check. Keep
        // the original operand alive for the following opcode, but classify
        // the value stored in its cell rather than the reference wrapper.
        let receiver = val.dereferenced();
        let is_null = receiver.value_type() == ValueType::Null;
        let is_non_object = !is_null && receiver.as_object().is_none();

        if is_null {
            // null ?-> anything  =>  null (short-circuit)
            let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
            frame_result_set(frame, result_ptr, opline.result_type, Value::null());
            let target = opline.op2 as usize;
            (*frame).opline = op_array.instructions.as_ptr().add(target);
            return Ok(ColdResult::Continue);
        } else if is_non_object && opline.extended_value == 1 {
            // extended_value: 0 = property access (warning + null), 1 = method call (fatal)
            let method = op_array
                .literals
                .get(opline._pad as usize)
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let error = make_error_value(
                "Error",
                &format!(
                    "Call to a member function {method}() on {}",
                    receiver.type_name()
                ),
            );
            let instruction_index = (opline as *const Instruction)
                .offset_from(op_array.instructions.as_ptr())
                as usize;
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }
        Ok(ColdResult::Done)
    }
}

#[inline(never)]
fn op_clone_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: CloneObj's source and result are compiler-owned slots in the live
    // frame; result publication initializes and marks the destination owner.
    unsafe {
        let src_val = &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);

        if src_val.value_type() != ValueType::Object {
            let error = make_error_value("Error", "__clone method called on non-object");
            let instruction_index = (opline as *const Instruction)
                .offset_from(op_array.instructions.as_ptr()) as usize;
            attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
            return Ok(match throw_in_frame(eg, frame, error) {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
            });
        }

    // Enum cases are singletons — cloning is forbidden
        {
            let obj = src_val.as_object().unwrap();
            if let Some(class_def) = eg.class_table.get(obj.class_name.as_ref()) {
                if class_def.is_enum {
                    let err = make_error_value(
                        "Error",
                        &format!(
                            "Trying to clone an uncloneable object of class {}",
                            obj.class_name
                        ),
                    );
                    drop(obj);
                    match throw_in_frame(eg, frame, err) {
                        ThrowResult::Handled(nf, no) => {
                            return Ok(ColdResult::NewFrame(nf, no));
                        }
                        ThrowResult::Unhandled(t) => return Ok(ColdResult::Unhandled(t)),
                    }
                }
            }
        }

        let cloned_obj = {
            let obj = src_val.as_object().unwrap();
            PhpObject {
                class_name: obj.class_name.clone(),
                class_id: obj.class_id,
                lifecycle: 0,
                property_layout: obj.property_layout.clone(),
                property_values: obj.property_values.clone(),
                dynamic_properties: obj.dynamic_properties.clone(),
                generator: None,
            }
        };
        let cloned_val = Value::object(cloned_obj);

    #[cfg(feature = "php-generics-reified")]
        if let Some(binding) = eg.reified_object_binding(src_val) {
            eg.bind_reified_object(&cloned_val, binding);
        }

        let _ = call_magic_method(eg, &cloned_val, "__clone", &[])?;

    // If __clone threw an exception, propagate it
        if let Some(exc) = eg.exception.take() {
            match throw_in_frame(eg, frame, exc) {
                ThrowResult::Handled(nf, no) => return Ok(ColdResult::NewFrame(nf, no)),
                ThrowResult::Unhandled(t) => return Ok(ColdResult::Unhandled(t)),
            }
        }

        frame_result_set(frame, result_ptr, opline.result_type, cloned_val);
        Ok(ColdResult::Done)
    }
}
