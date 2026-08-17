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
            let error = make_error_value(
                "TypeError",
                &format!(
                    "clone(): Argument #1 ($object) must be of type object, {} given",
                    src_val.dereferenced().type_name()
                ),
            );
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

        // Enum cases and Generator instances are engine-owned singletons.
        {
            let obj = src_val.as_object().unwrap();
            let uncloneable = obj.class_name.as_ref() == "Generator"
                || eg
                    .class_table
                    .get(obj.class_name.as_ref())
                    .is_some_and(|class_def| class_def.is_enum);
            if uncloneable {
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

        let cloned_obj = {
            let obj = src_val.as_object().unwrap();
            obj.clone_for_php()
        };
        let cloned_val = Value::object(cloned_obj);
        {
            let cloned = cloned_val.as_object().unwrap();
            for (slot, property) in cloned.property_values.iter().enumerate() {
                let Some(definition) = eg.instance_property_definition(cloned.class_id, slot) else {
                    continue;
                };
                if definition.is_typed() && property.is_owned_reference() {
                    property.add_reference_property_constraint(
                        crate::value::ReferencePropertyConstraint {
                            owner: cloned.instance_property_reference_owner(slot),
                            declaring_class: definition.declaring_class.clone(),
                            property: definition.name.clone(),
                            type_scope: definition.type_scope.clone(),
                            called_class: cloned.class_name.to_string(),
                            type_hint: definition.type_hint.clone(),
                        },
                    );
                }
            }
        }

    #[cfg(feature = "php-generics-reified")]
        if let Some(binding) = eg.reified_object_binding(src_val) {
            eg.bind_reified_object(&cloned_val, binding);
        }

        let clone_identity = cloned_val
            .object_identity()
            .expect("cloned object has stable identity");
        let readonly_properties = {
            let cloned = cloned_val.as_object().unwrap();
            eg.class_table
                .get(cloned.class_name.as_ref())
                .map(|class| class.readonly_props.iter().cloned().collect())
                .unwrap_or_default()
        };
        eg.clone_readonly_reinitialization
            .push((clone_identity, readonly_properties));
        let clone_result = call_magic_method(eg, &cloned_val, "__clone", &[]);
        let popped = eg.clone_readonly_reinitialization.pop();
        debug_assert!(popped.is_some_and(|(identity, _)| identity == clone_identity));
        let _ = clone_result?;

    // If __clone threw an exception, propagate it
        if let Some(exc) = eg.exception.take() {
            match throw_in_frame(eg, frame, exc) {
                ThrowResult::Handled(nf, no) => return Ok(ColdResult::NewFrame(nf, no)),
                ThrowResult::Unhandled(t) => return Ok(ColdResult::Unhandled(t)),
            }
        }

        if opline._pad & CLONE_OBJ_WITH_PROPERTIES != 0 {
            begin_clone_with_readonly_updates(eg, frame, &cloned_val);
        }

        frame_result_set(frame, result_ptr, opline.result_type, cloned_val);
        Ok(ColdResult::Done)
    }
}

#[inline(never)]
fn op_validate_clone_with<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let value = unsafe {
        (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)).dereferenced()
    };
    let error = if let Some(properties) = value.as_array() {
        properties
            .iter()
            .any(|(_, value)| value.owned_reference_is_aliased())
            .then(|| {
                make_error_value(
                    "Error",
                    "Cannot assign by reference when cloning with updated properties",
                )
            })
    } else {
        Some(make_error_value(
            "TypeError",
            &format!(
                "clone(): Argument #2 ($withProperties) must be of type array, {} given",
                value.type_name()
            ),
        ))
    };
    let Some(error) = error else {
        return Ok(ColdResult::Done);
    };
    let instruction_index = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    Ok(match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    })
}

fn begin_clone_with_readonly_updates(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    object: &Value,
) {
    let Some(identity) = object.object_identity() else {
        return;
    };
    let Some(object_ref) = object.as_object() else {
        return;
    };
    let initialized = eg
        .class_table
        .get(object_ref.class_name.as_ref())
        .map(|class| {
            class
                .readonly_props
                .iter()
                .filter(|name| {
                    let key = crate::runtime::resolve_property_key(
                        eg,
                        &object_ref.class_name,
                        name,
                        Some(&object_ref.class_name),
                    );
                    object_ref
                        .get_property(&key)
                        .is_some_and(|value| !value.is_undef())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    eg.clone_with_readonly_updates
        .push((frame as usize, identity, initialized));
}

fn op_end_clone_with(eg: &mut ExecutorGlobals, frame: *mut ExecuteData) {
    if let Some(index) = eg
        .clone_with_readonly_updates
        .iter()
        .rposition(|(owner, _, _)| *owner == frame as usize)
    {
        eg.clone_with_readonly_updates.remove(index);
    }
}
