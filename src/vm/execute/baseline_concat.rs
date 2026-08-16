// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[inline(never)]
fn op_concat(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
) -> Result<(), VmError> {
    let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
    let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
    // Operators consume the value stored in a PHP reference cell, not the
    // reference wrapper itself. This matters after `=&` publishes a returned
    // reference and the referenced CV is subsequently used by `.=`.
    let op1 = op1.dereferenced();
    let op2 = op2.dereferenced();
    let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

    // Fast path: both operands are strings — avoid echo_to_string() heap allocation.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::String {
        let s1 = op1.as_str().unwrap();
        let s2 = op2.as_str().unwrap();
        let mut concatenated = String::with_capacity(s1.len() + s2.len());
        concatenated.push_str(s1);
        concatenated.push_str(s2);
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Fast path: string . int — avoids echo_to_string heap alloc for the int.
    if op1.value_type() == ValueType::String && op2.value_type() == ValueType::Long {
        let s1 = op1.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(s1.len() + 20);
        concatenated.push_str(s1);
        write!(concatenated, "{}", unsafe { op2.raw_long() }).unwrap();
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Fast path: int . string
    if op1.value_type() == ValueType::Long && op2.value_type() == ValueType::String {
        let s2 = op2.as_str().unwrap();
        use std::fmt::Write;
        let mut concatenated = String::with_capacity(20 + s2.len());
        write!(concatenated, "{}", unsafe { op1.raw_long() }).unwrap();
        concatenated.push_str(s2);
        unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
        return Ok(());
    }

    // Slow path: at least one operand is non-string/non-int (object, float, etc).
    // Stringify each, then concatenate with pre-allocated capacity.
    let s1 = if op1.value_type() == ValueType::Object {
        if let Some(result) = call_magic_method(eg, op1, "__tostring", &[])? {
            result.echo_to_string()
        } else {
            op1.echo_to_string()
        }
    } else {
        op1.echo_to_string()
    };
    let s2 = if op2.value_type() == ValueType::Object {
        if let Some(result) = call_magic_method(eg, op2, "__tostring", &[])? {
            result.echo_to_string()
        } else {
            op2.echo_to_string()
        }
    } else {
        op2.echo_to_string()
    };
    let mut concatenated = String::with_capacity(s1.len() + s2.len());
    concatenated.push_str(&s1);
    concatenated.push_str(&s2);
    unsafe { frame_tmp_set(frame, result_ptr, Value::string(concatenated)) };
    Ok(())
}
