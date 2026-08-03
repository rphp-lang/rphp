// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
fn op_nullsafe_check(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<bool, VmError> {
    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let is_null = val.value_type() == ValueType::Null;
    let is_non_object = !is_null && val.as_object().is_none();

    if is_null {
        // null ?-> anything  =>  null (short-circuit)
        let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
        unsafe { slot_set(result_ptr, Value::null()) };
        let target = opline.op2 as usize;
        unsafe {
            (*frame).opline = op_array.instructions.as_ptr().add(target);
        }
        return Ok(true); // continue
    } else if is_non_object {
        // extended_value: 0 = property access (warning + null), 1 = method call (fatal)
        if opline.extended_value == 1 {
            // Method call on scalar: fatal error (like PHP)
            return Err(VmError::Fatal(
                "Call to a member function on a non-object".into()
            ));
        } else {
            // Property access on scalar: warning + null (like PHP)
            eg.write_output(b"Warning: Attempt to read property on non-object\n");
            let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
            unsafe { slot_set(result_ptr, Value::null()) };
            let target = opline.op2 as usize;
            unsafe {
                (*frame).opline = op_array.instructions.as_ptr().add(target);
            }
            return Ok(true); // continue
        }
    }
    Ok(false)
}

#[inline(never)]
fn op_clone_obj<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<ColdResult<'a>, VmError> {
    let src_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    if src_val.value_type() != ValueType::Object {
        return Err(VmError::Fatal(
            "__clone method called on non-object".into()
        ));
    }

    // Enum cases are singletons — cloning is forbidden
    {
        let obj = src_val.as_object().unwrap();
        if let Some(class_def) = eg.class_table.get(obj.class_name.as_ref()) {
            if class_def.is_enum {
                let err = make_error_value("Error", &format!(
                    "Trying to clone an uncloneable object of class {}", obj.class_name
                ));
                drop(obj);
                match throw_in_frame(eg, frame, err) {
                    ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
                    ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
                }
            }
        }
    }

    let cloned_obj = {
        let obj = src_val.as_object().unwrap();
        PhpObject {
            class_name: obj.class_name.clone(),
            class_id: obj.class_id,
            property_layout: obj.property_layout.clone(),
            property_values: obj.property_values.clone(),
            dynamic_properties: obj.dynamic_properties.clone(),
            generator: None,
        }
    };
    let cloned_val = Value::object(cloned_obj);

    let _ = call_magic_method(eg, &cloned_val, "__clone", &[])?;

    // If __clone threw an exception, propagate it
    if let Some(exc) = eg.exception.take() {
        match throw_in_frame(eg, frame, exc) {
            ThrowResult::Handled(nf, no) => { return Ok(ColdResult::NewFrame(nf, no)); }
            ThrowResult::Unhandled(t) => { return Ok(ColdResult::Unhandled(t)); }
        }
    }

    unsafe { slot_set(result_ptr, cloned_val) };
    Ok(ColdResult::Done)
}

