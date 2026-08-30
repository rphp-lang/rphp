// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[cold]
fn publish_concat_conversion_error(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    class_name: &str,
    message: &str,
) {
    let instruction_index = op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, opline))
        .expect("active concat instruction belongs to its op array");
    let error = make_error_value(class_name, message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    eg.exception = Some(error);
}

/// Convert one concatenation operand in PHP source order. `None` means either
/// a diagnostic handler or object conversion installed the pending exception.
#[cold]
fn prepare_concat_operand_value(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    value: &Value,
    report_array_diagnostic: bool,
) -> Result<Option<Value>, VmError> {
    let value = value.dereferenced();
    if value.value_type() == ValueType::Array {
        if report_array_diagnostic {
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                "Array to string conversion",
                false,
            )?;
            if eg.exception.is_some() {
                return Ok(None);
            }
        }
        return Ok(Some(Value::string("Array")));
    }

    if matches!(value.value_type(), ValueType::Object | ValueType::Closure) {
        let class_name = if value.value_type() == ValueType::Closure {
            "Closure".to_string()
        } else {
            value
                .as_object()
                .map(|object| object.class_name.to_string())
                .unwrap_or_else(|| "object".to_string())
        };
        let conversion = if value.value_type() == ValueType::Closure {
            None
        } else {
            call_magic_method_from_current_site(eg, value, "__tostring", &[])?
        };
        if eg.exception.is_some() {
            return Ok(None);
        }
        let Some(conversion) = conversion else {
            publish_concat_conversion_error(
                eg,
                frame,
                op_array,
                opline,
                "Error",
                &format!("Object of class {class_name} could not be converted to string"),
            );
            return Ok(None);
        };
        let conversion = conversion.dereferenced();
        if conversion.value_type() == ValueType::String {
            return Ok(Some(conversion.clone()));
        }

        // `__toString()` has an implicit string return contract even when the
        // declaration omits `: string`. PHP weakly coerces scalar returns in a
        // weak method file, while strict method files reject the same values.
        let method_name = format!("{}::__tostring", class_name.to_lowercase());
        // SAFETY: find_function returns a live immutable function-table entry;
        // the discriminant is checked before reading the UserFunction tail.
        let weak_method = eg.find_function(&method_name).is_some_and(|function| unsafe {
            (*function).fn_type == FunctionType::User
                && !(*(function as *const UserFunction)).op_array.strict_types
        });
        if weak_method
            && matches!(
                conversion.value_type(),
                ValueType::Long | ValueType::Double | ValueType::True | ValueType::False
            )
        {
            return Ok(Some(Value::string(
                conversion.echo_to_string_with_precision(eg.precision),
            )));
        }

        {
            let returned_type = match conversion.value_type() {
                ValueType::True => "true".into(),
                ValueType::False => "false".into(),
                _ => conversion.diagnostic_type_name(),
            };
            let outcome = format!("{returned_type} returned");
            publish_concat_conversion_error(
                eg,
                frame,
                op_array,
                opline,
                "TypeError",
                &format!(
                    "{class_name}::__toString(): Return value must be of type string, {outcome}"
                ),
            );
            return Ok(None);
        }
    }

    Ok(Some(if value.value_type() == ValueType::String {
        value.clone()
    } else {
        Value::string(value.echo_to_string_with_precision(eg.precision))
    }))
}

#[inline]
fn concatenate_string_values(left: &Value, right: &Value) -> Value {
    if !left.is_binary_string() && !right.is_binary_string() {
        let left = left.as_str();
        let right = right.as_str();
        debug_assert!(left.is_some() && right.is_some());
        let left = left.unwrap_or_default();
        let right = right.unwrap_or_default();
        let mut concatenated = String::with_capacity(left.len() + right.len());
        concatenated.push_str(left);
        concatenated.push_str(right);
        return Value::string(concatenated);
    }

    let left = left.php_string_bytes();
    let right = right.php_string_bytes();
    debug_assert!(left.is_some() && right.is_some());
    let left = left.unwrap_or_default();
    let right = right.unwrap_or_default();
    let mut concatenated = Vec::with_capacity(left.len() + right.len());
    concatenated.extend_from_slice(left.as_ref());
    concatenated.extend_from_slice(right.as_ref());
    Value::binary_string(&concatenated)
}

#[inline]
fn scalar_concat_operand_value(value: &Value, precision: i32) -> Value {
    let value = value.dereferenced();
    if value.value_type() == ValueType::String {
        value.clone()
    } else {
        Value::string(value.echo_to_string_with_precision(precision))
    }
}

#[inline(never)]
fn op_concat(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    report_array_diagnostic: bool,
) -> Result<(), VmError> {
    // SAFETY: execute_ex supplies its live frame and an instruction from this
    // op-array. No re-entry occurs before the fast-path operand borrows end;
    // the raw result pointer names this instruction's stable TMP slot.
    let (op1, op2, result_ptr) = unsafe {
        (
            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
            &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array),
            (*frame).get_op_mut(opline.result as u32, opline.result_type),
        )
    };
    // Operators consume the value stored in a PHP reference cell, not the
    // reference wrapper itself. This matters after `=&` publishes a returned
    // reference and the referenced CV is subsequently used by `.=`.
    let op1 = op1.dereferenced();
    let op2 = op2.dereferenced();
    // Fast path: both operands are strings — avoid echo_to_string() heap allocation.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::String {
        // SAFETY: result_ptr is this instruction's compiler-owned live TMP;
        // the helper returns an owned Value and retains no operand borrow.
        unsafe { frame_tmp_set(frame, result_ptr, concatenate_string_values(op1, op2)) };
        return Ok(());
    }

    // Fast path: string . int — avoids echo_to_string heap alloc for the int.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::Long {
        if op1.is_binary_string() {
            let mut concatenated = op1.php_string_bytes().unwrap_or_default().into_owned();
            concatenated.extend_from_slice(
                op2.as_long()
                    .expect("checked Long operand")
                    .to_string()
                    .as_bytes(),
            );
            // SAFETY: result_ptr is this instruction's live TMP result slot.
            unsafe {
                frame_tmp_set(frame, result_ptr, Value::binary_string(&concatenated))
            };
            return Ok(());
        }
        let s1 = op1.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(s1.len() + 20);
        concatenated.push_str(s1);
        write!(concatenated, "{}", op2.as_long().expect("checked Long operand")).unwrap();
        // SAFETY: result_ptr is this instruction's live TMP result slot.
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Fast path: int . string
    if op1.value_type() == ValueType::Long && op2.value_type() == ValueType::String {
        if op2.is_binary_string() {
            let prefix = op1
                .as_long()
                .expect("checked Long operand")
                .to_string();
            let right = op2.php_string_bytes().unwrap_or_default();
            let mut concatenated = Vec::with_capacity(prefix.len() + right.len());
            concatenated.extend_from_slice(prefix.as_bytes());
            concatenated.extend_from_slice(right.as_ref());
            // SAFETY: result_ptr is this instruction's live TMP result slot.
            unsafe {
                frame_tmp_set(frame, result_ptr, Value::binary_string(&concatenated))
            };
            return Ok(());
        }
        let s2 = op2.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(20 + s2.len());
        write!(concatenated, "{}", op1.as_long().expect("checked Long operand")).unwrap();
        concatenated.push_str(s2);
        // SAFETY: result_ptr is this instruction's live TMP result slot.
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Binary concat preflights both array diagnostics before object conversion.
    // Re-read each operand immediately before its stage: PHP allows a warning
    // handler to mutate a later CV and the operator observes that new value.
    if !report_array_diagnostic {
        for (operand, operand_type) in [
            (opline.op1 as u32, opline.op1_type),
            (opline.op2 as u32, opline.op2_type),
        ] {
            // SAFETY: the named operand is initialized in the live frame. It
            // is cloned before the warning handler can re-enter user code.
            let operand = unsafe {
                (&*(*frame).get_op_ptr(operand, operand_type, op_array)).clone()
            };
            if operand.value_type() == ValueType::Array {
                report_php_warning(
                    eg,
                    frame,
                    op_array,
                    opline,
                    "Array to string conversion",
                    false,
                )?;
                if eg.exception.is_some() {
                    return Ok(());
                }
            }
        }
    }

    // Clone each live value immediately before converting it. Conversion may
    // re-enter PHP and alter a later operand, which is intentionally fetched
    // only after the earlier conversion completes.
    // SAFETY: the live operand is cloned before object conversion can re-enter.
    let op1 = unsafe {
        (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)).clone()
    };
    let Some(s1) = prepare_concat_operand_value(
        eg,
        frame,
        op_array,
        opline,
        &op1,
        report_array_diagnostic,
    )?
    else {
        return Ok(());
    };
    // SAFETY: the later live operand is fetched only after the first conversion
    // completes, then cloned before its own conversion can re-enter.
    let op2 = unsafe {
        (&*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)).clone()
    };
    let Some(s2) = prepare_concat_operand_value(
        eg,
        frame,
        op_array,
        opline,
        &op2,
        report_array_diagnostic,
    )?
    else {
        return Ok(());
    };
    // SAFETY: result_ptr is this instruction's live TMP result slot and no
    // operand borrow remains when the detached value is committed.
    unsafe {
        frame_tmp_set(
            frame,
            result_ptr,
            concatenate_string_values(&s1, &s2),
        )
    };
    Ok(())
}
