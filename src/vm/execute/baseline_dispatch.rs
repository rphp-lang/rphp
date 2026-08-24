// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[cold]
fn return_type_diagnostic_name(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    hint: &ParamTypeHint,
) -> String {
    let Some(class_name) = eg
        .class_by_id(late_static_call_class_id(eg, frame))
        .map(|class| class.name.clone())
    else {
        return hint.diagnostic_display_name();
    };
    fn resolve_static(hint: &mut ParamTypeHint, class_name: &str) {
        match hint {
            ParamTypeHint::ClassName(name) if name.eq_ignore_ascii_case("static") => {
                *name = class_name.to_string();
            }
            ParamTypeHint::Nullable(inner) => resolve_static(inner, class_name),
            ParamTypeHint::Union(parts) | ParamTypeHint::Intersection(parts) => {
                for part in parts {
                    resolve_static(part, class_name);
                }
            }
            _ => {}
        }
    }
    let mut resolved = hint.clone();
    resolve_static(&mut resolved, &class_name);
    resolved.diagnostic_display_name()
}

#[cold]
fn return_type_error_value(
    eg: &ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    hint: &ParamTypeHint,
    outcome: &str,
) -> Value {
    let function_name = displayed_frame_function_name(eg, frame);
    let error = make_error_value(
        "TypeError",
        &format!(
            "{function_name}(): Return value must be of type {}, {outcome}",
            return_type_diagnostic_name(eg, frame, hint)
        ),
    );
    let instruction_index = op_array
        .instructions
        .iter()
        .position(|instruction| std::ptr::eq(instruction, opline))
        .expect("active return instruction belongs to its op array");
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    error
}

#[cold]
#[inline(never)]
fn throw_invalid_dynamic_call_class<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
) -> Result<ThrowResult<'a>, VmError> {
    let error = make_error_value(
        "Error",
        "Class name must be a valid object or a string",
    );
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    throw_in_frame(eg, frame, error)
}

#[cold]
fn is_enum_value(eg: &ExecutorGlobals, value: &Value) -> bool {
    value
        .dereferenced()
        .as_object()
        .and_then(|object| eg.find_class(&object.class_name))
        .is_some_and(|class| class.is_enum)
}

#[cold]
fn enum_comparison_result(eg: &ExecutorGlobals, left: &Value, right: &Value) -> Option<i32> {
    let left = left.dereferenced();
    let right = right.dereferenced();
    let left_is_enum = is_enum_value(eg, left);
    let right_is_enum = is_enum_value(eg, right);
    (left_is_enum || right_is_enum).then(|| {
        if matches!(left.value_type(), ValueType::True | ValueType::False | ValueType::Null)
            || matches!(right.value_type(), ValueType::True | ValueType::False | ValueType::Null)
        {
            match left.is_truthy().cmp(&right.is_truthy()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        } else if left_is_enum && right_is_enum && values_identical(left, right) {
            0
        } else {
            1
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeComparisonMode {
    LooseEquality,
    Ordering,
}

#[inline]
fn comparison_numeric_pair(left: &Value, right: &Value) -> Option<(f64, f64)> {
    let left = left.dereferenced();
    let right = right.dereferenced();
    let uses_boolean_comparison = matches!(
        left.value_type(),
        ValueType::True | ValueType::False | ValueType::Null | ValueType::Undef
    ) || matches!(
        right.value_type(),
        ValueType::True | ValueType::False | ValueType::Null | ValueType::Undef
    );
    (!uses_boolean_comparison)
        .then(|| left.to_double().zip(right.to_double()))
        .flatten()
}

#[cold]
fn comparison_object_string_conversion(
    eg: &mut ExecutorGlobals,
    object: &Value,
) -> Result<Option<Value>, VmError> {
    let class_name = object.diagnostic_type_name();
    let rendered = call_object_string_conversion(eg, object)?;
    if eg.exception.is_none()
        && let Some(rendered) = rendered.as_ref()
        && rendered.as_str().is_none()
    {
        let outcome = if rendered.value_type() == ValueType::Null {
            "none returned".to_string()
        } else {
            format!("{} returned", rendered.diagnostic_type_name())
        };
        eg.exception = Some(make_error_value(
            "TypeError",
            &format!(
                "{class_name}::__toString(): Return value must be of type string, {outcome}"
            ),
        ));
    }
    Ok(rendered)
}

/// Prepare the object-handler cases whose comparison may execute user code.
/// Zend avoids touching lazy state for identity and class-mismatch decisions,
/// while same-class property comparison realizes both distinct operands.
#[cold]
fn prepare_object_comparison_operands(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    left: &Value,
    right: &Value,
) -> Result<Option<(Value, Value, Option<i32>)>, VmError> {
    let left = left.dereferenced();
    let right = right.dereferenced();

    match (left.value_type(), right.value_type()) {
        (ValueType::Object, ValueType::Object) => {
            let mut prepared_left = left.clone();
            let mut prepared_right = right.clone();
            let same_identity = left.object_identity() == right.object_identity();
            let same_class = left
                .as_object()
                .zip(right.as_object())
                .is_some_and(|(left, right)| {
                    if left.class_id != 0 || right.class_id != 0 {
                        left.class_id == right.class_id
                    } else {
                        left.class_name.eq_ignore_ascii_case(&right.class_name)
                    }
                });
            if !same_identity && same_class {
                if eg.lazy_object_state(&prepared_right).is_some() {
                    prepared_right = crate::stdlib::reflection::resolve_lazy_object_chain(
                        eg,
                        &prepared_right,
                    )?;
                }
                if eg.exception.is_none() && eg.lazy_object_state(&prepared_left).is_some() {
                    prepared_left = crate::stdlib::reflection::resolve_lazy_object_chain(
                        eg,
                        &prepared_left,
                    )?;
                }
            }
            Ok(Some((prepared_left, prepared_right, None)))
        }
        (ValueType::Object, ValueType::String) => {
            let rendered = comparison_object_string_conversion(eg, left)?;
            Ok(Some(match rendered {
                Some(rendered) => (rendered, right.clone(), None),
                None => (left.clone(), right.clone(), Some(1)),
            }))
        }
        (ValueType::String, ValueType::Object) => {
            let rendered = comparison_object_string_conversion(eg, right)?;
            Ok(Some(match rendered {
                Some(rendered) => (left.clone(), rendered, None),
                None => (left.clone(), right.clone(), Some(-1)),
            }))
        }
        (ValueType::Object | ValueType::Closure, ValueType::Long | ValueType::Resource)
            if !is_enum_value(eg, left) =>
        {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                &format!(
                    "Object of class {} could not be converted to int",
                    left.diagnostic_type_name()
                ),
            )?;
            Ok(Some((Value::long(1), right.clone(), None)))
        }
        (ValueType::Long | ValueType::Resource, ValueType::Object | ValueType::Closure)
            if !is_enum_value(eg, right) =>
        {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                &format!(
                    "Object of class {} could not be converted to int",
                    right.diagnostic_type_name()
                ),
            )?;
            Ok(Some((left.clone(), Value::long(1), None)))
        }
        (ValueType::Object | ValueType::Closure, ValueType::Double)
            if !is_enum_value(eg, left) =>
        {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                &format!(
                    "Object of class {} could not be converted to float",
                    left.diagnostic_type_name()
                ),
            )?;
            Ok(Some((Value::double(1.0), right.clone(), None)))
        }
        (ValueType::Double, ValueType::Object | ValueType::Closure)
            if !is_enum_value(eg, right) =>
        {
            report_php_notice(
                eg,
                frame,
                op_array,
                opline,
                &format!(
                    "Object of class {} could not be converted to float",
                    right.diagnostic_type_name()
                ),
            )?;
            Ok(Some((left.clone(), Value::double(1.0), None)))
        }
        _ => Ok(None),
    }
}

#[cold]
fn runtime_values_checked(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    left: &Value,
    right: &Value,
    mode: RuntimeComparisonMode,
) -> Result<Result<i32, ()>, VmError> {
    fn ordering(value: std::cmp::Ordering) -> i32 {
        match value {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    fn compare_inner(
        eg: &mut ExecutorGlobals,
        frame: *mut ExecuteData,
        op_array: &crate::compiler::OpArray,
        opline: &Instruction,
        left: &Value,
        right: &Value,
        context: &mut ComparisonContext,
        depth: usize,
        mode: RuntimeComparisonMode,
    ) -> Result<Result<i32, ()>, VmError> {
        let left_owner = left.dereferenced().clone();
        let right_owner = right.dereferenced().clone();
        let left = &left_owner;
        let right = &right_owner;

        if let (Some(left_array), Some(right_array)) = (left.as_array(), right.as_array()) {
            let left_identity = left.array_identity().unwrap();
            let right_identity = right.array_identity().unwrap();
            if left_identity == right_identity {
                return Ok(Ok(0));
            }
            if depth >= MAX_COMPARISON_DEPTH
                || !context.active_left.insert(left_identity)
                || !context.active_right.insert(right_identity)
            {
                return Ok(Err(()));
            }

            let mut result = ordering(left_array.len().cmp(&right_array.len()));
            let mut comparison_error = false;
            if result == 0 {
                for (key, value) in left_array.iter() {
                    let Some(other) = (match key {
                        ArrayKey::Int(key) => right_array.get_int(key),
                        ArrayKey::String(key) => right_array.get_str(&key),
                    }) else {
                        result = 1;
                        break;
                    };
                    match compare_inner(
                        eg,
                        frame,
                        op_array,
                        opline,
                        value,
                        other,
                        context,
                        depth + 1,
                        mode,
                    )? {
                        Ok(comparison) if comparison != 0 => {
                            result = comparison;
                            break;
                        }
                        Ok(_) => {}
                        Err(()) => {
                            comparison_error = true;
                            break;
                        }
                    }
                    if eg.exception.is_some() {
                        break;
                    }
                }
            }
            context.active_left.remove(&left_identity);
            context.active_right.remove(&right_identity);
            return Ok(if comparison_error { Err(()) } else { Ok(result) });
        }

        let prepared = prepare_object_comparison_operands(
            eg, frame, op_array, opline, left, right,
        )?;
        let (left_owner, right_owner, forced_cmp) = match prepared {
            Some((left, right, forced_cmp)) => (Some(left), Some(right), forced_cmp),
            None => (None, None, None),
        };
        let left = left_owner.as_ref().unwrap_or(left);
        let right = right_owner.as_ref().unwrap_or(right);
        if eg.exception.is_some() {
            return Ok(Ok(0));
        }
        if let Some(comparison) = enum_comparison_result(eg, left, right) {
            return Ok(Ok(comparison));
        } else if let Some(comparison) = forced_cmp {
            return Ok(Ok(comparison));
        }

        if let (Some(left_object), Some(right_object)) = (left.as_object(), right.as_object()) {
            let left_identity = left.object_identity().unwrap();
            let right_identity = right.object_identity().unwrap();
            if left_identity == right_identity {
                return Ok(Ok(0));
            }
            let same_class = if left_object.class_id != 0 || right_object.class_id != 0 {
                left_object.class_id == right_object.class_id
            } else {
                left_object
                    .class_name
                    .eq_ignore_ascii_case(&right_object.class_name)
            };
            if !same_class {
                return Ok(Ok(1));
            }
            if depth >= MAX_COMPARISON_DEPTH
                || !context.active_left.insert(left_identity)
                || !context.active_right.insert(right_identity)
            {
                return Ok(Err(()));
            }

            let mut left_count = 0usize;
            left_object.for_each_property(|_, _| left_count += 1);
            let mut right_count = 0usize;
            right_object.for_each_property(|_, _| right_count += 1);
            let mut result = ordering(left_count.cmp(&right_count));
            let mut comparison_error = false;
            let mut execution_error = None;
            if result == 0 {
                left_object.for_each_property(|name, value| {
                    if result != 0
                        || comparison_error
                        || execution_error.is_some()
                        || eg.exception.is_some()
                    {
                        return;
                    }
                    let Some(other) = right_object.get_property(name) else {
                        result = 1;
                        return;
                    };
                    match compare_inner(
                        eg,
                        frame,
                        op_array,
                        opline,
                        value,
                        other,
                        context,
                        depth + 1,
                        mode,
                    ) {
                        Ok(Ok(comparison)) => result = comparison,
                        Ok(Err(())) => comparison_error = true,
                        Err(error) => execution_error = Some(error),
                    }
                });
            }
            context.active_left.remove(&left_identity);
            context.active_right.remove(&right_identity);
            if let Some(error) = execution_error {
                return Err(error);
            }
            return Ok(if comparison_error { Err(()) } else { Ok(result) });
        }

        Ok(match mode {
            RuntimeComparisonMode::LooseEquality => {
                values_equal_checked_with_precision(left, right, eg.precision)
                    .map(|equal| i32::from(!equal))
            }
            RuntimeComparisonMode::Ordering => {
                values_compare_checked_with_precision(left, right, eg.precision)
            }
        })
    }

    compare_inner(
        eg,
        frame,
        op_array,
        opline,
        left,
        right,
        &mut ComparisonContext::default(),
        0,
        mode,
    )
}

#[cold]
fn prepared_comparison_result(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    opcode: OpCode,
    left: &Value,
    right: &Value,
) -> Result<Result<bool, ()>, VmError> {
    let comparison_result = |comparison: i32| match opcode {
        OpCode::IsEqual | OpCode::IsEqual_CvConst => comparison == 0,
        OpCode::IsNotEqual => comparison != 0,
        OpCode::IsSmaller | OpCode::IsSmaller_CvConst => comparison < 0,
        OpCode::IsSmallerOrEqual | OpCode::IsSmallerOrEqual_CvConst => comparison <= 0,
        _ => unreachable!(),
    };

    let mode = if matches!(
        opcode,
        OpCode::IsEqual | OpCode::IsEqual_CvConst | OpCode::IsNotEqual
    ) {
        RuntimeComparisonMode::LooseEquality
    } else {
        RuntimeComparisonMode::Ordering
    };
    runtime_values_checked(eg, frame, op_array, opline, left, right, mode)
        .map(|result| result.map(comparison_result))
}

/// Return the innermost finally block crossed by a non-local jump. The whole
/// try/catch/finally instruction span counts as local: compiler-generated jumps
/// into the block's own finally must not create a continuation.
fn crossed_finally_for_jump(
    op_array: &crate::compiler::OpArray,
    source: u32,
    target: u32,
    target_outside_try: bool,
) -> Option<&crate::compiler::compile::TryEntry> {
    op_array
        .try_entries
        .iter()
        .filter(|entry| {
            entry.finally_start != u32::MAX
                && source >= entry.try_start
                && source < entry.finally_start
                && !(target >= entry.try_start
                    && target < entry.finally_end
                    && !(target_outside_try && target == entry.try_start))
        })
        .min_by_key(|entry| entry.finally_end - entry.try_start)
}

fn finally_jump_cv(op_array: &crate::compiler::OpArray) -> Option<u32> {
    op_array
        .all_cvs
        .iter()
        .find_map(|(cv, name)| (name == "\0finally_jump").then_some(*cv))
}

const FINALLY_JUMP_TARGET_OUTSIDE_TRY: u32 = 1 << 31;

fn finally_jump_target(encoded: u32) -> (u32, bool) {
    (
        encoded & !FINALLY_JUMP_TARGET_OUTSIDE_TRY,
        encoded & FINALLY_JUMP_TARGET_OUTSIDE_TRY != 0,
    )
}

const FINALLY_JUMP_CLEAR: u8 = 0;
const FINALLY_JUMP_RESUME: u8 = 1;
const FINALLY_JUMP_START: u8 = 2;

#[derive(Clone, Copy)]
enum IncDecDiagnostic {
    Warning(&'static str),
    Deprecation(&'static str),
}

#[cold]
fn report_incdec_diagnostic(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    diagnostic: IncDecDiagnostic,
) -> Result<(), VmError> {
    match diagnostic {
        IncDecDiagnostic::Warning(message) => {
            report_php_warning(eg, frame, op_array, opline, message, false)
        }
        IncDecDiagnostic::Deprecation(message) => {
            report_php_deprecation(eg, frame, op_array, opline, message)
        }
    }
}

#[cold]
fn report_return_coercion_diagnostic(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    source: &Value,
    diagnostic: ReturnCoercionDiagnostic,
) -> Result<(), VmError> {
    match diagnostic {
        ReturnCoercionDiagnostic::FloatToInt => report_php_deprecation(
            eg,
            frame,
            op_array,
            opline,
            &format!(
                "Implicit conversion from float {} to int loses precision",
                source.echo_to_string_with_precision(-1)
            ),
        ),
        ReturnCoercionDiagnostic::FloatStringToInt => report_php_deprecation(
            eg,
            frame,
            op_array,
            opline,
            &format!(
                "Implicit conversion from float-string \"{}\" to int loses precision",
                source.as_str().unwrap_or("")
            ),
        ),
        ReturnCoercionDiagnostic::NanTo(target) => report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            &format!("unexpected NAN value was coerced to {target}"),
            false,
        ),
    }
}

fn increment_php_alphanumeric_string(value: &str) -> String {
    if value.is_empty() {
        return "1".to_string();
    }
    let mut bytes = value.as_bytes().to_vec();
    let mut carry = true;
    let mut carry_prefix = None;
    for byte in bytes.iter_mut().rev() {
        if !carry {
            break;
        }
        match *byte {
            b'0'..=b'8' | b'a'..=b'y' | b'A'..=b'Y' => {
                *byte += 1;
                carry = false;
            }
            b'9' => {
                *byte = b'0';
                carry_prefix = Some(b'1');
            }
            b'z' => {
                *byte = b'a';
                carry_prefix = Some(b'a');
            }
            b'Z' => {
                *byte = b'A';
                carry_prefix = Some(b'A');
            }
            _ => {
                // A non-alphanumeric prefix stops carry propagation without
                // discarding changes already made to the trailing suffix.
                // For example, PHP increments `".Z"` to `".A"` rather than
                // restoring the original string or prepending another digit.
                carry = false;
                break;
            }
        }
    }
    if carry {
        bytes.insert(0, carry_prefix.unwrap_or(b'1'));
    }
    String::from_utf8(bytes).expect("ASCII increment preserves UTF-8")
}

fn increment_php_value(value: &Value) -> Option<(Value, Option<IncDecDiagnostic>)> {
    if let Some(number) = value.as_long() {
        return Some((
            number.checked_add(1).map_or_else(
                || Value::double(number as f64 + 1.0),
                Value::long,
            ),
            None,
        ));
    }
    match value.value_type() {
        ValueType::Double => Some((Value::double(value.as_double().unwrap() + 1.0), None)),
        ValueType::Null | ValueType::Undef => Some((Value::long(1), None)),
        ValueType::True | ValueType::False => Some((
            value.clone(),
            Some(IncDecDiagnostic::Warning(
                "Increment on type bool has no effect, this will change in the next major version of PHP",
            )),
        )),
        ValueType::String => {
            let text = value.as_str().unwrap();
            let numeric = text.trim();
            if !numeric.is_empty() {
                if let Ok(number) = numeric.parse::<i64>() {
                    return Some((
                        number.checked_add(1).map_or_else(
                            || Value::double(number as f64 + 1.0),
                            Value::long,
                        ),
                        None,
                    ));
                }
                if let Ok(number) = numeric.parse::<f64>() {
                    return Some((Value::double(number + 1.0), None));
                }
            }
            Some((
                Value::string(increment_php_alphanumeric_string(text)),
                Some(IncDecDiagnostic::Deprecation(
                    "Increment on non-numeric string is deprecated, use str_increment() instead",
                )),
            ))
        }
        _ => None,
    }
}

fn decrement_php_value(value: &Value) -> Option<(Value, Option<IncDecDiagnostic>)> {
    if let Some(number) = value.as_long() {
        return Some((
            number
                .checked_sub(1)
                .map_or_else(|| Value::double(number as f64 - 1.0), Value::long),
            None,
        ));
    }
    match value.value_type() {
        ValueType::Null | ValueType::Undef => Some((
            value.clone(),
            Some(IncDecDiagnostic::Warning(
                "Decrement on type null has no effect, this will change in the next major version of PHP",
            )),
        )),
        ValueType::True | ValueType::False => Some((
            value.clone(),
            Some(IncDecDiagnostic::Warning(
                "Decrement on type bool has no effect, this will change in the next major version of PHP",
            )),
        )),
        ValueType::String => {
            let text = value.as_str().unwrap();
            if text.is_empty() {
                return Some((
                    Value::long(-1),
                    Some(IncDecDiagnostic::Deprecation(
                        "Decrement on empty string is deprecated as non-numeric",
                    )),
                ));
            }
            let numeric = text.trim();
            if !numeric.is_empty() {
                if let Ok(number) = numeric.parse::<i64>() {
                    return Some((
                        number.checked_sub(1).map_or_else(
                            || Value::double(number as f64 - 1.0),
                            Value::long,
                        ),
                        None,
                    ));
                }
                if let Ok(number) = numeric.parse::<f64>() {
                    return Some((Value::double(number - 1.0), None));
                }
            }
            Some((
                value.clone(),
                Some(IncDecDiagnostic::Deprecation(
                    "Decrement on non-numeric string has no effect and is deprecated",
                )),
            ))
        }
        ValueType::Double => Some((Value::double(value.as_double().unwrap() - 1.0), None)),
        _ => None,
    }
}

#[cold]
fn report_integer_operator_diagnostics(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    source: &Value,
    operand: IntegerOperatorOperand,
) -> Result<(), VmError> {
    let source = source.dereferenced();
    if operand.leading_numeric {
        report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            "A non-numeric value encountered",
            false,
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    if operand.non_representable_float {
        report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            &format!(
                "The float {} is not representable as an int, cast occurred",
                source.echo_to_string_with_precision(-1)
            ),
            false,
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
    }
    if operand.loses_precision {
        let message = if operand.float_string {
            format!(
                "Implicit conversion from float-string \"{}\" to int loses precision",
                source.as_str().unwrap_or("")
            )
        } else {
            format!(
                "Implicit conversion from float {} to int loses precision",
                source.echo_to_string_with_precision(-1)
            )
        };
        report_php_deprecation(eg, frame, op_array, opline, &message)?;
    }
    Ok(())
}

#[inline]
fn prepare_integer_operator_operand(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    source: &Value,
) -> Result<Option<i64>, VmError> {
    let Ok(operand) = integer_operator_operand(source) else {
        return Ok(None);
    };
    report_integer_operator_diagnostics(eg, frame, op_array, opline, source, operand)?;
    Ok(Some(operand.value))
}

#[inline]
fn commutative_operator_error_operands<'a>(
    left: &'a Value,
    right: &'a Value,
    left_type: OpType,
    right_type: OpType,
) -> (&'a Value, &'a Value) {
    // Zend preserves source order when both operands are runtime CVs (for
    // example an array_reduce callback's `$carry * $value`). Constant/CV and
    // temporary forms canonicalize certain internal resource/object operands
    // for commutative operators before reporting their type error.
    if left_type == OpType::Cv && right_type == OpType::Cv {
        return (left, right);
    }
    let rank = |value: &Value| match value.dereferenced().value_type() {
        ValueType::Object | ValueType::Closure => 2,
        ValueType::Resource => 1,
        _ => 0,
    };
    if rank(right) > rank(left) {
        (right, left)
    } else {
        (left, right)
    }
}

#[inline]
fn commutative_operator_uses_canonical_validation(left: &Value, right: &Value) -> bool {
    [left, right].into_iter().any(|value| {
        matches!(
            value.dereferenced().value_type(),
            ValueType::Object | ValueType::Closure | ValueType::Resource
        )
    })
}

#[cold]
fn report_arithmetic_operator_diagnostic(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    leading_numeric: bool,
) -> Result<bool, VmError> {
    if leading_numeric {
        report_php_warning(
            eg,
            frame,
            op_array,
            opline,
            "A non-numeric value encountered",
            false,
        )?;
    }
    Ok(eg.exception.is_some())
}

#[cold]
fn prepare_arithmetic_operator_pair(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    left: &Value,
    right: &Value,
) -> Result<Option<(Value, Value)>, VmError> {
    let Ok(left) = arithmetic_operator_operand(left) else {
        return Ok(None);
    };
    if report_arithmetic_operator_diagnostic(
        eg,
        frame,
        op_array,
        opline,
        left.leading_numeric,
    )? {
        return Ok(None);
    }

    let Ok(right) = arithmetic_operator_operand(right) else {
        return Ok(None);
    };
    if report_arithmetic_operator_diagnostic(
        eg,
        frame,
        op_array,
        opline,
        right.leading_numeric,
    )? {
        return Ok(None);
    }
    Ok(Some((left.value, right.value)))
}

#[cold]
fn prepare_commutative_arithmetic_operator_pair(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    opline: &Instruction,
    left: &Value,
    right: &Value,
) -> Result<Option<(Value, Value)>, VmError> {
    let (Ok(left), Ok(right)) = (
        arithmetic_operator_operand(left),
        arithmetic_operator_operand(right),
    ) else {
        return Ok(None);
    };
    if report_arithmetic_operator_diagnostic(
        eg,
        frame,
        op_array,
        opline,
        left.leading_numeric,
    )? || report_arithmetic_operator_diagnostic(
        eg,
        frame,
        op_array,
        opline,
        right.leading_numeric,
    )? {
        return Ok(None);
    }
    Ok(Some((left.value, right.value)))
}

#[cold]
fn prepared_add_result(left: &Value, right: &Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_long(), right.as_long()) {
        left.checked_add(right)
            .map(Value::long)
            .unwrap_or_else(|| Value::double(left as f64 + right as f64))
    } else {
        Value::double(left.to_double().unwrap() + right.to_double().unwrap())
    }
}

#[cold]
fn prepared_sub_result(left: &Value, right: &Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_long(), right.as_long()) {
        left.checked_sub(right)
            .map(Value::long)
            .unwrap_or_else(|| Value::double(left as f64 - right as f64))
    } else {
        Value::double(left.to_double().unwrap() - right.to_double().unwrap())
    }
}

#[cold]
fn prepared_mul_result(left: &Value, right: &Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_long(), right.as_long()) {
        left.checked_mul(right)
            .map(Value::long)
            .unwrap_or_else(|| Value::double(left as f64 * right as f64))
    } else {
        Value::double(left.to_double().unwrap() * right.to_double().unwrap())
    }
}

#[cold]
fn prepared_div_result(left: &Value, right: &Value) -> Option<Value> {
    let left_number = left.to_double().unwrap();
    let right_number = right.to_double().unwrap();
    if right_number == 0.0 {
        return None;
    }
    if let (Some(left), Some(right)) = (left.as_long(), right.as_long())
        && let Some(quotient) = left.checked_div(right)
        && left.checked_rem(right) == Some(0)
    {
        return Some(Value::long(quotient));
    }
    Some(Value::double(left_number / right_number))
}

#[cold]
fn prepared_pow_result(left: &Value, right: &Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_long(), right.as_long())
        && let Ok(exponent) = u32::try_from(right)
        && let Some(value) = left.checked_pow(exponent)
    {
        return Value::long(value);
    }
    Value::double(left.to_double().unwrap().powf(right.to_double().unwrap()))
}

#[inline]
fn split_arithmetic_result(value: Value) -> Result<i64, Value> {
    if let Some(value) = value.as_long() {
        Ok(value)
    } else {
        Err(value)
    }
}

#[cold]
#[inline(never)]
fn finally_jump_state(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    action: u8,
    target: u32,
    target_outside_try: bool,
) -> Option<u32> {
    // SAFETY: the compiler allocated the hidden CV in this op array's live
    // frame, and every redirect is an instruction offset produced by that same
    // compiler. The continuation is always a scalar Long/Undef;
    // frame_slot_set preserves ordinary slot ownership while replacing it.
    unsafe {
        match action {
            FINALLY_JUMP_START => {
                let current_ip = (*frame)
                    .opline
                    .offset_from(op_array.instructions.as_ptr())
                    as u32;
                let entry = crossed_finally_for_jump(
                    op_array,
                    current_ip,
                    target,
                    target_outside_try,
                )
                .expect("JmpFinally must cross a compiled finally range");
                let encoded = target
                    | if target_outside_try {
                        FINALLY_JUMP_TARGET_OUTSIDE_TRY
                    } else {
                        0
                    };
                let cv = finally_jump_cv(op_array)
                    .expect("JmpFinally requires the compiler-owned continuation CV");
                let destination = (*frame).get_op_mut(cv, OpType::Cv);
                frame_slot_set(frame, destination, Value::long(encoded as i64));
                (*frame).opline = op_array
                    .instructions
                    .as_ptr()
                    .add(entry.finally_start as usize);
                None
            }
            FINALLY_JUMP_CLEAR => {
                let cv = finally_jump_cv(op_array)?;
                let destination = (*frame).get_op_mut(cv, OpType::Cv);
                frame_slot_set(frame, destination, Value::undef());
                None
            }
            FINALLY_JUMP_RESUME => {
                let cv = finally_jump_cv(op_array)?;
                let encoded = u32::try_from((*frame).cv(cv).as_long()?).ok()?;
                let current_ip = (*frame)
                    .opline
                    .offset_from(op_array.instructions.as_ptr())
                    as u32;
                let (target, target_outside_try) = finally_jump_target(encoded);
                let next = crossed_finally_for_jump(
                    op_array,
                    current_ip,
                    target,
                    target_outside_try,
                )
                    .map_or(target, |entry| entry.finally_start);
                if next == target {
                    let destination = (*frame).get_op_mut(cv, OpType::Cv);
                    frame_slot_set(frame, destination, Value::undef());
                }
                (*frame).opline = op_array.instructions.as_ptr().add(next as usize);
                Some(target)
            }
            _ => unreachable!("unknown finally jump action"),
        }
    }
}

fn throw_object_as_array<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    receiver: &Value,
) -> Result<ThrowResult<'a>, VmError> {
    let class_name = if receiver.value_type() == ValueType::Closure {
        "Closure".to_string()
    } else {
        receiver
            .as_object()
            .map(|object| object.class_name.to_string())
            .unwrap_or_else(|| "object".to_string())
    };
    throw_array_dimension_error(
        eg,
        frame,
        op_array,
        instruction_index,
        &format!("Cannot use object of type {class_name} as array"),
    )
}

fn throw_array_dimension_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    message: &str,
) -> Result<ThrowResult<'a>, VmError> {
    let error = make_error_value("Error", message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    throw_in_frame(eg, frame, error)
}

fn throw_illegal_offset_type<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    message: &str,
) -> Result<ThrowResult<'a>, VmError> {
    let error = make_error_value("TypeError", message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    throw_in_frame(eg, frame, error)
}

#[cold]
#[inline(never)]
fn throw_operator_error<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    class_name: &str,
    message: &str,
) -> Result<ThrowResult<'a>, VmError> {
    let error = make_error_value(class_name, message);
    // Arithmetic bytecode is commonly emitted before the located statement
    // consumer (Return, call, echo or constant definition). Use that nearest
    // source entry while the failing frame is still live; a preceding entry
    // remains the fallback for a discarded terminal expression.
    let origin_index = if op_array.source_line(instruction_index).is_some() {
        instruction_index
    } else {
        (instruction_index + 1..op_array.instructions.len())
            .find(|index| op_array.source_line(*index).is_some())
            .or_else(|| {
                (0..instruction_index)
                    .rev()
                    .find(|index| op_array.source_line(*index).is_some())
            })
            .or_else(|| op_array.declaration_line().map(|_| u32::MAX as usize))
            .unwrap_or(instruction_index)
    };
    attach_throwable_origin(&error, eg, frame, op_array, origin_index);
    throw_in_frame(eg, frame, error)
}

#[cold]
fn array_access_offset_error(value: &Value, isset_or_empty: bool) -> String {
    if isset_or_empty {
        format!(
            "Cannot access offset of type {} in isset or empty",
            value.diagnostic_type_name()
        )
    } else {
        format!(
            "Cannot access offset of type {} on array",
            value.diagnostic_type_name()
        )
    }
}

#[cold]
#[inline(never)]
fn fetch_dim_after_array_key_diagnostic<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    result_slot: *mut Value,
    source: Value,
    error: ArrayKeyError,
) -> Result<ColdResult<'a>, VmError> {
    // SAFETY: FetchDimR supplied its live operand slot. Retaining the array
    // before invoking user code keeps the allocation valid if that code
    // replaces the operand; the slot itself remains part of the live frame.
    let (array_slot, original_owner_count, array_guard, original_identity, pristine_empty) = unsafe {
        let array_slot = (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
        let array = (&*array_slot).dereferenced();
        let original_owner_count = array.cycle_strong_count().unwrap();
        let array_guard = array.clone();
        let original_identity = array_guard.array_identity().unwrap();
        let pristine_empty = array.as_array().unwrap().is_pristine_empty();
        (
            array_slot,
            original_owner_count,
            array_guard,
            original_identity,
            pristine_empty,
        )
    };

    macro_rules! finish_diagnostic {
        () => {
            if let Some(exception) = eg.exception.take() {
                return Ok(match throw_in_frame(eg, frame, exception)? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        ColdResult::NewFrame(new_frame, new_op_array)
                    }
                    ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
                });
            }
        };
    }

    let key = match error {
        ArrayKeyError::Resource(resource) => {
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &format!(
                    "Resource ID#{resource} used as offset, casting to integer ({resource})"
                ),
                opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
            )?;
            finish_diagnostic!();
            ArrayKey::Int(resource)
        }
        ArrayKeyError::DeprecatedNull => {
            report_php_deprecation(
                eg,
                frame,
                op_array,
                opline,
                "Using null as an array offset is deprecated, use an empty string instead",
            )?;
            finish_diagnostic!();
            ArrayKey::String(String::new())
        }
        ArrayKeyError::DeprecatedFloat(integer) => {
            let rendered = source.echo_to_string_with_precision(-1);
            report_php_deprecation(
                eg,
                frame,
                op_array,
                opline,
                &format!("Implicit conversion from float {rendered} to int loses precision"),
            )?;
            finish_diagnostic!();
            ArrayKey::Int(integer)
        }
        ArrayKeyError::NonRepresentableFloat {
            integer,
            also_deprecated,
        } => {
            let rendered = source.echo_to_string_with_precision(-1);
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &format!("The float {rendered} is not representable as an int, cast occurred"),
                opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
            )?;
            finish_diagnostic!();
            if also_deprecated {
                report_php_deprecation(
                    eg,
                    frame,
                    op_array,
                    opline,
                    &format!("Implicit conversion from float {rendered} to int loses precision"),
                )?;
                finish_diagnostic!();
            }
            ArrayKey::Int(integer)
        }
        ArrayKeyError::Illegal => {
            let instruction_index = unsafe {
                (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
            };
            return Ok(match throw_illegal_offset_type(
                eg,
                frame,
                op_array,
                instruction_index,
                &array_access_offset_error(
                    &source,
                    opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_EMPTY) != 0,
                ),
            )? {
                ThrowResult::Handled(new_frame, new_op_array) => {
                    ColdResult::NewFrame(new_frame, new_op_array)
                }
                ThrowResult::Unhandled(exception) => ColdResult::Unhandled(exception),
            });
        }
    };

    let current_identity = unsafe { (&*array_slot).dereferenced().array_identity() };
    if current_identity == Some(original_identity)
        || original_owner_count > 1
        || pristine_empty
    {
        let array = array_guard.as_array().unwrap();
        let fetched = match &key {
            ArrayKey::Int(key) => array.get_int(*key),
            ArrayKey::String(key) => {
                let cache_ip = unsafe {
                    (opline as *const Instruction)
                        .offset_from(op_array.instructions.as_ptr())
                        as usize
                };
                unsafe { cached_string_array_value(op_array, cache_ip, array, key) }
            }
        };
        if fetched.is_none() && opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_SILENT) == 0 {
            let key = match key {
                ArrayKey::Int(key) => key.to_string(),
                ArrayKey::String(key) => format!("\"{key}\""),
            };
            report_php_warning(
                eg,
                frame,
                op_array,
                opline,
                &format!("Undefined array key {key}"),
                opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
            )?;
            finish_diagnostic!();
        }
        let value = if opline._pad & FETCH_DIM_ISSET != 0 {
            Value::bool(fetched.is_some_and(|value| {
                !matches!(value.value_type(), ValueType::Null | ValueType::Undef)
            }))
        } else {
            fetched.cloned().unwrap_or(Value::null())
        };
        write_fetch_dim_result(frame, result_slot, value);
    } else {
        write_fetch_dim_result(
            frame,
            result_slot,
            if opline._pad & FETCH_DIM_ISSET != 0 {
                Value::bool(false)
            } else {
                Value::null()
            },
        );
    }
    Ok(ColdResult::Done)
}

/// Inner loop for RPHP's authoritative baseline executor.
fn execute_ex(eg: &mut ExecutorGlobals, initial_frame: *mut ExecuteData) -> Result<(), VmError> {
    let mut frame = initial_frame;
    let mut op_array = unsafe { (*frame).op_array() };
    let mut tick: u8 = 255; // First iteration checks immediately (wraps to 0)
    'vm: loop {
        // Batch interrupt check: every 256 opcodes instead of every opcode.
        // Placed at loop top so all `continue` paths also pass through it.
        tick = tick.wrapping_add(1);
        if tick == 0 {
            if eg.vm_interrupt.load(Ordering::Relaxed) {
                handle_interrupt(eg)?;
            }
        }

        // SAFETY: the active frame's opline points into its live op array.
        let (mut opline_ptr, opline) = unsafe {
            let opline_ptr: *const Instruction = (*frame).opline;
            (opline_ptr, &*opline_ptr)
        };
        macro_rules! array_key_or_throw {
            ($conversion:expr, $message:expr) => {
                match $conversion {
                    Ok(key) => key,
                    Err(_) => {
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_illegal_offset_type(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            $message,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                }
            };
        }
        macro_rules! finish_array_key_diagnostic {
            () => {{
                if let Some(exception) = eg.exception.take() {
                    match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue 'vm;
                        }
                        ThrowResult::Unhandled(exception) => {
                            eg.exception = Some(exception);
                            return Ok(());
                        }
                    }
                }
            }};
        }
        macro_rules! array_key_ref_or_throw {
            ($value:expr, $message:expr, $suppressed:expr) => {{
                let source = $value;
                match value_to_array_key_ref(source) {
                    Ok(key) => key,
                    Err(ArrayKeyError::Resource(resource)) => {
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Resource ID#{resource} used as offset, casting to integer ({resource})"
                            ),
                            $suppressed,
                        )?;
                        finish_array_key_diagnostic!();
                        ArrayKeyRef::Int(resource)
                    }
                    Err(ArrayKeyError::DeprecatedNull) => {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            "Using null as an array offset is deprecated, use an empty string instead",
                        )?;
                        finish_array_key_diagnostic!();
                        ArrayKeyRef::String("")
                    }
                    Err(ArrayKeyError::DeprecatedFloat(integer)) => {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Implicit conversion from float {} to int loses precision",
                                source.echo_to_string_with_precision(-1)
                            ),
                        )?;
                        finish_array_key_diagnostic!();
                        ArrayKeyRef::Int(integer)
                    }
                    Err(ArrayKeyError::NonRepresentableFloat {
                        integer,
                        also_deprecated,
                    }) => {
                        let rendered = source.echo_to_string_with_precision(-1);
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "The float {} is not representable as an int, cast occurred",
                                rendered
                            ),
                            $suppressed,
                        )?;
                        finish_array_key_diagnostic!();
                        if also_deprecated {
                            report_php_deprecation(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "Implicit conversion from float {} to int loses precision",
                                    rendered
                                ),
                            )?;
                            finish_array_key_diagnostic!();
                        }
                        ArrayKeyRef::Int(integer)
                    }
                    Err(ArrayKeyError::Illegal) => {
                        array_key_or_throw!(Err::<ArrayKeyRef<'_>, ()>(()), $message)
                    }
                }
            }};
        }
        macro_rules! array_key_owned_or_throw {
            ($value:expr, $message:expr, $suppressed:expr, $report_conversion:expr) => {{
                let source = $value;
                match value_to_array_key(source) {
                    Ok(key) => key,
                    Err(ArrayKeyError::Resource(resource)) => {
                        if $report_conversion {
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "Resource ID#{resource} used as offset, casting to integer ({resource})"
                                ),
                                $suppressed,
                            )?;
                            finish_array_key_diagnostic!();
                        }
                        ArrayKey::Int(resource)
                    }
                    Err(ArrayKeyError::DeprecatedNull) => {
                        if $report_conversion {
                            report_php_deprecation(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "Using null as an array offset is deprecated, use an empty string instead",
                            )?;
                            finish_array_key_diagnostic!();
                        }
                        ArrayKey::String(String::new())
                    }
                    Err(ArrayKeyError::DeprecatedFloat(integer)) => {
                        if $report_conversion {
                            report_php_deprecation(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "Implicit conversion from float {} to int loses precision",
                                    source.echo_to_string_with_precision(-1)
                                ),
                            )?;
                            finish_array_key_diagnostic!();
                        }
                        ArrayKey::Int(integer)
                    }
                    Err(ArrayKeyError::NonRepresentableFloat {
                        integer,
                        also_deprecated,
                    }) => {
                        if $report_conversion {
                            let rendered = source.echo_to_string_with_precision(-1);
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!(
                                    "The float {} is not representable as an int, cast occurred",
                                    rendered
                                ),
                                $suppressed,
                            )?;
                            finish_array_key_diagnostic!();
                            if also_deprecated {
                                report_php_deprecation(
                                    eg,
                                    frame,
                                    op_array,
                                    opline,
                                    &format!(
                                        "Implicit conversion from float {} to int loses precision",
                                        rendered
                                    ),
                                )?;
                                finish_array_key_diagnostic!();
                            }
                        }
                        ArrayKey::Int(integer)
                    }
                    Err(ArrayKeyError::Illegal) => {
                        array_key_or_throw!(Err::<ArrayKey, ()>(()), $message)
                    }
                }
            }};
        }
        macro_rules! prepare_constrained_write {
            ($constraints:expr, $value:expr) => {{
                let value = $value;
                let constraints = $constraints;
                if constraints.is_empty() {
                    value
                } else {
                    match prepare_reference_assignment(
                        value,
                        &constraints,
                        eg,
                        op_array.strict_types,
                    ) {
                        Ok(value) => value,
                        Err(message) => {
                            let instruction_index = (opline_ptr as usize
                                - op_array.instructions.as_ptr() as usize)
                                / std::mem::size_of::<Instruction>();
                            match throw_illegal_offset_type(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &message,
                            )? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }};
        }
        macro_rules! prepare_reference_write {
            ($cv:expr, $value:expr) => {{
                let value = $value;
                let reference = (*frame).cv($cv);
                if reference.is_owned_reference() {
                    let constraints = reference.reference_property_constraints();
                    prepare_constrained_write!(constraints, value)
                } else {
                    value
                }
            }};
        }
        macro_rules! restore_incdec_snapshot_on_exception {
            ($writeback_cv:expr, $old:expr) => {
                if eg.exception.is_some() {
                    let restored = prepare_reference_write!($writeback_cv, $old.clone());
                    let cv_ptr = (*frame).get_op_mut($writeback_cv, OpType::Cv);
                    slot_set(cv_ptr, restored);
                }
            };
        }
        macro_rules! throw_operator {
            ($class_name:expr, $message:expr) => {{
                let instruction_index = (opline_ptr as usize
                    - op_array.instructions.as_ptr() as usize)
                    / std::mem::size_of::<Instruction>();
                match throw_operator_error(
                    eg,
                    frame,
                    op_array,
                    instruction_index,
                    $class_name,
                    $message,
                )? {
                    ThrowResult::Handled(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue 'vm;
                    }
                    ThrowResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                }
            }};
        }
        macro_rules! reject_reference_incdec_overflow {
            ($writeback_cv:expr, $old:expr, $overflow:expr) => {
                if let Some(writeback_cv) = $writeback_cv {
                    let reference = (*frame).cv(writeback_cv);
                    if reference.is_owned_reference()
                        && let Some(message) = reference_incdec_overflow_message(
                            reference,
                            $old,
                            eg,
                            $overflow,
                        )
                    {
                        throw_operator!("TypeError", &message);
                    }
                }
            };
        }
        macro_rules! resume_pending_exception {
            () => {
                if let Some(exception) = eg.exception.take() {
                    match throw_in_frame(eg, frame, exception)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue 'vm;
                        }
                        ThrowResult::Unhandled(exception) => {
                            eg.exception = Some(exception);
                            return Ok(());
                        }
                    }
                }
            };
        }
        macro_rules! report_array_to_string_conversion {
            ($value:expr) => {
                if $value.dereferenced().value_type() == ValueType::Array {
                    report_php_warning(
                        eg,
                        frame,
                        op_array,
                        opline,
                        "Array to string conversion",
                        false,
                    )?;
                    resume_pending_exception!();
                }
            };
        }
        stats::inc_opcode(opline.opcode as usize);

        // Check for pending return or exception after finally block ends
        let frame_pending = unsafe { (*frame).pending_return_after_finally };
        let check_finally = frame_pending
            || eg.exception.is_some()
            || eg.finally_exceptions.contains_key(&(frame as usize));
        if check_finally {
            let current_ip = unsafe {
                (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
            };
            let at_finally_end = op_array.try_entries.iter().any(|e| {
                e.finally_start != 0xFFFFFFFF && current_ip == e.finally_end
            });
            if at_finally_end {
                if frame_pending {
                    unsafe { (*frame).pending_return_after_finally = false; }
                    // Deferred return — pop frame now (return value already written)
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    run_frame_destructors(eg, frame)?;
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    let func_common = unsafe { &*(*frame).func };
                    if func_common.plan.needs_late_static_scope() {
                        eg.discard_late_static_scope(frame as usize);
                    }
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                } else {
                    // Real exception — re-enter throw/unwind to find outer handler
                    let pending = eg.exception.take().or_else(|| {
                        let exceptions = eg.finally_exceptions.get_mut(&(frame as usize))?;
                        let pending = exceptions.pop();
                        if exceptions.is_empty() {
                            eg.finally_exceptions.remove(&(frame as usize));
                        }
                        pending
                    }).unwrap();
                    match throw_in_frame(eg, frame, pending)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ThrowResult::Unhandled(exception) => {
                            eg.exception = Some(exception);
                            return Ok(());
                        }
                    }
                }
            }
        }

        match opline.opcode {
            OpCode::AssignCv | OpCode::BindCvRef => {
                // SAFETY: `frame` is the active VM frame and every operand was
                // allocated by this op-array. Reference binding promotes only
                // a live source CV and rebinds only a live destination CV. The
                // slot helpers preserve the frame cleanup metadata.
                unsafe {
                    if opline.opcode == OpCode::BindCvRef {
                        // `=&` rebinds the destination variable itself, even
                        // when it previously pointed at another reference
                        // cell. The source is promoted once and both CVs retain
                        // aliases to that stable request-owned cell.
                        let source = if opline.op1_type == OpType::Cv {
                            (*frame).cv_mut(opline.op1 as u32) as *mut Value
                        } else {
                            (*frame).get_op_mut(opline.op1 as u32, opline.op1_type)
                        };
                        let nonreferenceable_source =
                            opline.op1_type != OpType::Cv && !(&*source).is_reference();
                        if nonreferenceable_source {
                            report_php_notice(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "Only variables should be assigned by reference",
                            )?;
                        }
                        let mut binding = materialize_reference_alias(frame, source);
                        if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                            binding.mark_internal_reference_alias();
                        }
                        let destination = (*frame).cv_mut(opline.result as u32) as *mut Value;
                        let destructor = (matches!(
                            (&*destination).dereferenced().value_type(),
                            ValueType::Object | ValueType::Closure
                        ))
                            .then(|| prepare_replaced_value_destructor(eg, &*destination))
                            .flatten();
                        let destructor_ran = destructor.is_some();
                        if nonreferenceable_source {
                            run_prepared_value_destructor(eg, destructor)?;
                            frame_slot_set(frame, destination, binding);
                        } else {
                            frame_slot_set(frame, destination, binding);
                            run_prepared_value_destructor(eg, destructor)?;
                        }
                        if destructor_ran {
                            resume_pending_exception!();
                        }
                    } else {
                        // ASSIGN_CV op1=CV(dest), op2=value, result=optional copy
                        // Unused TMP/VAR results are SSA values. When an
                        // assignment does not publish an expression result,
                        // transfer that sole bytecode owner into the
                        // destination instead of retaining a hidden TMP alias
                        // until frame teardown. Besides avoiding one Rc pair,
                        // this makes value lifetime follow PHP variables rather
                        // than compiler storage.
                        let movable_source = opline._pad & ASSIGN_CV_MOVE_SOURCE != 0
                            && opline.result_type == OpType::Unused
                            && matches!(opline.op2_type, OpType::Tmp | OpType::Var);
                        let mut cloned = if movable_source {
                            let source = (*frame)
                                .get_op_mut(opline.op2 as u32, opline.op2_type);
                            if (&*source).is_reference() {
                                // A by-value assignment from a reference-
                                // returning call must read through the cell;
                                // moving the raw VAR would turn the destination
                                // into an observable alias.
                                (&*(*frame).get_op_ptr(
                                    opline.op2 as u32,
                                    opline.op2_type,
                                    op_array,
                                ))
                                    .clone()
                            } else if matches!(
                                (&*source).value_type(),
                                ValueType::Array | ValueType::Object | ValueType::Closure
                            ) {
                                std::mem::replace(&mut *source, Value::undef())
                            } else {
                                // Scalar scratch values are intentionally kept
                                // populated: hot-loop activation may validate
                                // their established slot tags at the backedge.
                                (&*source).clone()
                            }
                        } else {
                            (&*(*frame).get_op_ptr(
                                opline.op2 as u32,
                                opline.op2_type,
                                op_array,
                            ))
                                .clone()
                        };
                        let rebind_destination = opline._pad & ASSIGN_CV_REBIND != 0;
                        let destination_is_reference = !rebind_destination
                            && opline.op1_type == OpType::Cv
                            && (*frame).cv(opline.op1 as u32).is_reference();
                        let dest = if rebind_destination {
                            (*frame).cv_mut(opline.op1 as u32) as *mut Value
                        } else {
                            (*frame).get_op_mut(opline.op1 as u32, opline.op1_type)
                        };
                        let replaced_object = opline.op1_type == OpType::Cv
                            && matches!(
                                (&*dest).dereferenced().value_type(),
                                ValueType::Object | ValueType::Closure
                            );
                        let mirrored_global_name = (replaced_object && !(&*dest).is_reference())
                            .then(|| {
                                let root_frame = (*frame).prev_execute_data.is_null();
                                let mirrored_variables = if root_frame {
                                    &op_array.main_scope_vars
                                } else {
                                    &op_array.global_vars
                                };
                                (root_frame || !mirrored_variables.is_empty()).then(|| {
                                    mirrored_variables
                                    .iter()
                                    .find(|(cv, _)| *cv == u32::from(opline.op1))
                                    .and_then(|(_, name)| {
                                        eg.globals
                                            .get(name)
                                            .filter(|global| {
                                                !global.is_reference()
                                                    && global.weak_object_identity()
                                                        == (&*dest).weak_object_identity()
                                            })
                                            .map(|_| name.as_str())
                                    })
                                })
                                .flatten()
                            })
                            .flatten();
                        let replaced_references = 1 + usize::from(mirrored_global_name.is_some());
                        let destructor = replaced_object
                            .then(|| {
                                prepare_replaced_value_destructor_with_references(
                                    eg,
                                    &*dest,
                                    replaced_references,
                                )
                            })
                            .flatten();
                        let destructor_ran = destructor.is_some();
                        if destination_is_reference {
                            cloned = prepare_reference_write!(opline.op1 as u32, cloned);
                        }
                        if opline.result_type != OpType::Unused {
                            // Need two copies: one for dest, one for result
                            if matches!(opline.op1_type, OpType::Tmp | OpType::Var) {
                                frame_tmp_set(frame, dest, cloned.clone());
                            } else if opline.op1_type == OpType::Cv
                                && (!destination_is_reference || rebind_destination)
                            {
                                frame_slot_set(frame, dest, cloned.clone());
                            } else {
                                slot_set(dest, cloned.clone());
                            }
                            let result_ptr =
                                (*frame).get_op_mut(opline.result as u32, opline.result_type);
                            if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                                frame_tmp_set(frame, result_ptr, cloned);
                            } else {
                                slot_set(result_ptr, cloned);
                            }
                        } else {
                            // Common path: just move the single clone into dest
                            if matches!(opline.op1_type, OpType::Tmp | OpType::Var) {
                                frame_tmp_set(frame, dest, cloned);
                            } else if opline.op1_type == OpType::Cv
                                && (!destination_is_reference || rebind_destination)
                            {
                                frame_slot_set(frame, dest, cloned);
                            } else {
                                slot_set(dest, cloned);
                            }
                        }
                        if let Some(global_name) = mirrored_global_name {
                            globals_set(&mut eg.globals, global_name, (&*dest).clone());
                        }
                        run_prepared_value_destructor(eg, destructor)?;
                        if destructor_ran {
                            resume_pending_exception!();
                        }
                    }
                }
            }

            OpCode::FetchCvR => {
                // Snapshot before invoking the handler: PHP consumes null for
                // this read even when the handler assigns the same CV.
                // SAFETY: both operands belong to the active frame/op-array;
                // the TMP writer owns cleanup bookkeeping, and pending calls
                // are released only while this same frame remains suspended.
                unsafe {
                    let source = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        OpType::Cv,
                        op_array,
                    );
                    if !source.is_undef() {
                        if opline.result_type != OpType::Unused {
                            let result =
                                (*frame).get_op_mut(opline.result as u32, opline.result_type);
                            frame_tmp_set(frame, result, source.clone());
                        }
                    } else {
                        if opline.result_type != OpType::Unused {
                            let result =
                                (*frame).get_op_mut(opline.result as u32, opline.result_type);
                            frame_tmp_set(frame, result, Value::null());
                        }
                        report_undefined_variable_read(
                            eg,
                            frame,
                            op_array,
                            opline,
                            opline.op2,
                            opline._pad & crate::vm::instruction::FETCH_CV_ERROR_SUPPRESS != 0,
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            cleanup_pending_calls(eg, frame);
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }

            OpCode::AssignConcat => {
                // SAFETY: both named operands are initialized in this live
                // frame. Checked values are cloned before user-code re-entry;
                // the ordinary path acquires its own bounded unsafe region.
                let checked_operands = unsafe {
                    let left = (&*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    ))
                    .dereferenced();
                    let right = (&*(*frame).get_op_ptr(
                        opline.op2 as u32,
                        opline.op2_type,
                        op_array,
                    ))
                    .dereferenced();
                    let checked = [left, right].into_iter().any(|value| {
                        matches!(
                            value.value_type(),
                            ValueType::Array | ValueType::Object | ValueType::Closure
                        )
                    });
                    checked.then(|| {
                        let left = left.clone();
                        let right = if opline.op1_type == opline.op2_type
                            && opline.op1 == opline.op2
                        {
                            left.clone()
                        } else {
                            (&*(*frame).get_op_ptr(
                                opline.op2 as u32,
                                opline.op2_type,
                                op_array,
                            ))
                            .clone()
                        };
                        (left, right)
                    })
                };
                if let Some((left, right)) = checked_operands {
                    // Object conversion may re-enter user code and mutate a
                    // source CV. Snapshot both already-evaluated operands before
                    // invoking it, then commit only after both conversions pass.
                    let left = prepare_concat_operand_string(
                        eg, frame, op_array, opline, &left, true,
                    )?;
                    resume_pending_exception!();
                    let Some(left) = left else {
                        unreachable!("failed concat conversion installs an exception")
                    };
                    let right = prepare_concat_operand_string(
                        eg, frame, op_array, opline, &right, true,
                    )?;
                    resume_pending_exception!();
                    let Some(right) = right else {
                        unreachable!("failed concat conversion installs an exception")
                    };
                    // SAFETY: conversion succeeded with detached strings. The
                    // destination remains the live compound-assignment slot;
                    // the constrained write is validated before replacement.
                    unsafe {
                        let prepared = prepare_reference_write!(
                            opline.op1 as u32,
                            Value::string(left + &right)
                        );
                        let destination =
                            (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                        slot_set(destination, prepared);
                    }
                } else {
                // SAFETY: all operands and the optional reference-owning CV
                // belong to this live frame. The ordinary path performs no
                // user-code conversion and preserves its existing COW rules.
                unsafe {
                if opline.op1_type == OpType::Cv
                    && !{
                        (*frame)
                            .cv(opline.op1 as u32)
                            .reference_property_constraints()
                            .is_empty()
                    }
                {
                    let lhs = (&*(*frame).get_op_ptr(opline.op1 as u32, OpType::Cv, op_array))
                    .echo_to_string_with_precision(eg.precision);
                    let rhs = (&*(*frame).get_op_ptr(
                            opline.op2 as u32,
                            opline.op2_type,
                            op_array,
                        ))
                    .echo_to_string_with_precision(eg.precision);
                    let prepared = prepare_reference_write!(
                        opline.op1 as u32,
                        Value::string(lhs + &rhs)
                    );
                    let destination = (*frame).get_op_mut(opline.op1 as u32, OpType::Cv);
                    slot_set(destination, prepared);
                    (*frame).opline = opline_ptr.add(1);
                    continue 'vm;
                }
                // $x .= expr: in-place string append
                // COW: if dest is sole owner, push_str in place (no allocation).
                // If shared, as_string_mut() detaches first.
                // Snapshot an exact self-source before taking the mutable
                // destination. Besides preserving the original RHS, this
                // makes `$x .= $x` obey Rust's aliasing rules while the string
                // COW detach grows the destination.
                let self_source =
                    opline.op1_type == opline.op2_type && opline.op1 == opline.op2;
                let self_rhs;
                // SAFETY: operand slots are initialized for this instruction;
                // an exact self-source is cloned before the destination is
                // mutably accessed, and a distinct slot stays live meanwhile.
                let rhs = {
                    let rhs_ptr = (*frame).get_op_ptr(
                        opline.op2 as u32,
                        opline.op2_type,
                        op_array,
                    );
                    self_rhs = self_source.then(|| (&*rhs_ptr).clone());
                    self_rhs.as_ref().unwrap_or(&*rhs_ptr)
                };
                let dest = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                let dest_ref = &mut *dest;
                if dest_ref.value_type() == ValueType::String {
                    // Fast path: avoid echo_to_string() allocation when RHS is string
                    if rhs.value_type() == ValueType::String {
                        let rhs_s = rhs.as_str().unwrap();
                        let s = dest_ref.as_string_mut().unwrap_unchecked();
                        s.push_str(rhs_s);
                    } else {
                        let rhs_str = rhs.echo_to_string_with_precision(eg.precision);
                        let s = dest_ref.as_string_mut().unwrap_unchecked();
                        s.push_str(&rhs_str);
                    }
                } else {
                    let lhs_str = dest_ref.echo_to_string_with_precision(eg.precision);
                    let rhs_str = if rhs.value_type() == ValueType::String {
                        rhs.as_str().unwrap().to_string()
                    } else {
                        rhs.echo_to_string_with_precision(eg.precision)
                    };
                    let mut new_s = lhs_str;
                    new_s.push_str(&rhs_str);
                    slot_set(dest, Value::string(new_s));
                }
                }
                }
            }

            OpCode::Echo => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let val = val.dereferenced();
                report_array_to_string_conversion!(val);
                if val.value_type() == ValueType::Undef {
                    if opline.op1_type == OpType::Cv && opline.extended_value != 0 {
                        if let Some((_, name)) = op_array
                            .all_cvs
                            .iter()
                            .find(|(index, _)| *index == u32::from(opline.op1))
                        {
                            let file = if op_array.source_file.is_empty() {
                                op_array.name.as_str()
                            } else {
                                op_array.source_file.as_str()
                            };
                            if eg.error_reporting & 2 != 0 {
                                eg.write_output(
                                    format!(
                                        "\nWarning: Undefined variable ${name} in {file} on line {}\n",
                                        opline.extended_value
                                    )
                                    .as_bytes(),
                                );
                            }
                        }
                    }
                } else if val.value_type() == ValueType::String {
                    // Fast path: string → write bytes directly, no allocation
                    eg.write_output(val.as_str().unwrap().as_bytes());
                } else if val.value_type() == ValueType::Long {
                    // Fast path: integer → stack-local write, no heap allocation
                    use std::io::Write;
                    let mut buf = [0u8; 20]; // i64 max is 19 digits + sign
                    let s = {
                        let mut cursor = std::io::Cursor::new(&mut buf[..]);
                        write!(cursor, "{}", unsafe { val.raw_long() }).unwrap();
                        cursor.position() as usize
                    };
                    eg.write_output(&buf[..s]);
                } else if matches!(val.value_type(), ValueType::Object | ValueType::Closure) {
                    let class_name = if val.value_type() == ValueType::Closure {
                        "Closure".to_string()
                    } else {
                        val.as_object()
                            .map(|object| object.class_name.to_string())
                            .unwrap_or_else(|| "object".to_string())
                    };
                    let conversion = if val.value_type() == ValueType::Closure {
                        None
                    } else {
                        call_magic_method(eg, val, "__tostring", &[])?
                    };
                    resume_pending_exception!();
                    if let Some(result) = conversion {
                        let Some(output) = result.as_str() else {
                            throw_operator!(
                                "TypeError",
                                &format!(
                                    "{class_name}::__toString(): Return value must be of type string"
                                )
                            );
                        };
                        eg.write_output(output.as_bytes());
                    } else {
                        throw_operator!(
                            "Error",
                            &format!("Object of class {class_name} could not be converted to string")
                        );
                    }
                } else {
                    let output = val.echo_to_string_with_precision(eg.precision);
                    eg.write_output(output.as_bytes());
                }
            }

            OpCode::Echo_String => {
                let value = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(value.value_type(), ValueType::String);
                let string = unsafe { value.as_str().unwrap_unchecked() };
                eg.write_output(string.as_bytes());
            }

            OpCode::Echo_Long => {
                let value = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(value.value_type(), ValueType::Long);
                use std::io::Write;
                let mut buffer = [0u8; 20];
                let length = {
                    let mut cursor = std::io::Cursor::new(&mut buffer[..]);
                    write!(cursor, "{}", unsafe { value.raw_long() }).unwrap();
                    cursor.position() as usize
                };
                eg.write_output(&buffer[..length]);
            }

            // ── Specialized arithmetic opcodes ──────────────────────────
            // Inline operand access: no get_op_ptr match, no ref check.
            // Fall through to general handler on non-Long operands.

            OpCode::Add_LongLong
            | OpCode::Sub_LongLong
            | OpCode::Mul_LongLong
            | OpCode::Mod_LongLong
            | OpCode::BitwiseXor_LongLong
            | OpCode::BitwiseAnd_LongLong
            | OpCode::BitwiseOr_LongLong => {
                let left = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                let right = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op2, opline.op2_type)
                };
                debug_assert_eq!(left.value_type(), ValueType::Long);
                debug_assert_eq!(right.value_type(), ValueType::Long);
                let lhs = unsafe { left.raw_long() };
                let rhs = unsafe { right.raw_long() };
                let result_ptr = unsafe {
                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                };
                match opline.opcode {
                    OpCode::Add_LongLong => match lhs.checked_add(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 + rhs as f64),
                            )
                        },
                    },
                    OpCode::Sub_LongLong => match lhs.checked_sub(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 - rhs as f64),
                            )
                        },
                    },
                    OpCode::Mul_LongLong => match lhs.checked_mul(rhs) {
                        Some(result) => unsafe { frame_tmp_set_long(frame, result_ptr, result) },
                        None => unsafe {
                            frame_tmp_set(
                                frame,
                                result_ptr,
                                Value::double(lhs as f64 * rhs as f64),
                            )
                        },
                    },
                    OpCode::Mod_LongLong => {
                        if rhs == 0 {
                            throw_operator!("DivisionByZeroError", "Modulo by zero");
                        }
                        let remainder = lhs.checked_rem(rhs).unwrap_or(0);
                        unsafe { frame_tmp_set_long(frame, result_ptr, remainder) };
                    }
                    OpCode::BitwiseXor_LongLong => unsafe {
                        frame_tmp_set_long(frame, result_ptr, lhs ^ rhs)
                    },
                    OpCode::BitwiseAnd_LongLong => unsafe {
                        frame_tmp_set_long(frame, result_ptr, lhs & rhs)
                    },
                    OpCode::BitwiseOr_LongLong => unsafe {
                        frame_tmp_set_long(frame, result_ptr, lhs | rhs)
                    },
                    _ => unreachable!(),
                }
            }

            OpCode::Add_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let op1 = op1.dereferenced();
                let op2 = op2.dereferenced();
                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    if let Some(sum) = l1.checked_add(l2) {
                        // Peek ahead: if next is Return consuming our result TMP,
                        // and frame is FastScalar with no heap — skip TMP write + Return dispatch.
                        // Write sum directly to caller's return_value, pop frame inline.
                        let next = unsafe { &*opline_ptr.add(1) };
                        if next.opcode == OpCode::Return
                            && next.op1_type == OpType::Tmp
                            && next.op1 == opline.result
                            && !unsafe { (*frame).has_heap_slots }
                        {
                            let return_target = unsafe { (*frame).return_value };
                            if !return_target.is_null() {
                                unsafe { frame_return_set_long(frame, return_target, sum) };
                            }
                            stats::inc_return_fast();
                            let prev = unsafe { (*frame).prev_execute_data };
                            if prev.is_null() {
                                return Ok(());
                            }
                            if frame == initial_frame {
                                eg.current_execute_data.set(prev);
                                pop_vm_call_frame(eg, frame);
                                return Ok(());
                            }
                            eg.current_execute_data.set(prev);
                            pop_vm_call_frame(eg, frame);
                            frame = prev;
                            op_array = unsafe { (*frame).op_array() };
            
                            continue;
                        }
                        // Normal path: write to TMP
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe { frame_tmp_set_long(frame, result_ptr, sum) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        };
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else if let (Some(left), Some(right)) = (op1.as_array(), op2.as_array()) {
                    write_array_union_result(frame, opline.result, left, right);
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} + {}",
                                op1.diagnostic_type_name(),
                                op2.diagnostic_type_name()
                            )
                        );
                    };
                    let result = prepared_add_result(&left, &right);
                    // SAFETY: the specialized opcode's TMP result index was
                    // validated with this op-array, and both operand borrows
                    // have ended before the owned result replaces that slot.
                    unsafe {
                        let result_ptr =
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize);
                        frame_tmp_set(frame, result_ptr, result)
                    };
                }
            }

            OpCode::Add_CvTmp => {
                let base = frame as *const Value;
                let cv_ptr = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op1 = cv_ptr.dereferenced();
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let op2 = op2.dereferenced();
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else if let (Some(left), Some(right)) = (op1.as_array(), op2.as_array()) {
                    write_array_union_result(frame, opline.result, left, right);
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} + {}",
                                op1.diagnostic_type_name(),
                                op2.diagnostic_type_name()
                            )
                        );
                    };
                    // SAFETY: `result_ptr` is the validated TMP slot for this
                    // specialized opcode, and the owned result is constructed
                    // before it replaces that slot.
                    unsafe {
                        frame_tmp_set(frame, result_ptr, prepared_add_result(&left, &right))
                    };
                }
            }

            OpCode::Sub_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = op1_cv.dereferenced();
                let op2 = &op_array.literals()[opline.op2 as usize];
                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    // Peek ahead: if next instruction is SendVal consuming our TMP result,
                    // write directly to the call arg slot and skip the SendVal dispatch.
                    let next = unsafe { &*opline_ptr.add(1) };
                    if next.opcode == OpCode::SendVal
                        && next.op1_type == OpType::Tmp
                        && next.op1 == opline.result
                    {
                        let call = unsafe { (*frame).call };
                        let dst = unsafe {
                            (call as *mut Value).add(CALL_FRAME_SLOTS + next.op2 as usize)
                        };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { Value::write_long(dst, diff) },
                            None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                        }
                        // Skip SendVal: advance local ptr +1, loop bottom adds +1 → net +2
                        opline_ptr = unsafe { opline_ptr.add(1) };
                    } else {
                        let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                        match l1.checked_sub(l2) {
                            Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                            None => unsafe {
                                frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                            },
                        }
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} - {}",
                                op1.diagnostic_type_name(),
                                op2.diagnostic_type_name()
                            )
                        );
                    };
                    let result = prepared_sub_result(&left, &right);
                    // SAFETY: the specialized opcode's TMP result index was
                    // validated with this op-array, and both operand borrows
                    // have ended before the owned result replaces that slot.
                    unsafe {
                        let result_ptr =
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize);
                        frame_tmp_set(frame, result_ptr, result)
                    };
                }
            }

            OpCode::Sub_TmpTmp => {
                let base = frame as *const Value;
                let op1 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op1 as usize) };
                let op2 = unsafe { &*base.add(CALL_FRAME_SLOTS + opline.op2 as usize) };
                let result_ptr = unsafe { (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize) };
                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    match l1.checked_sub(l2) {
                        Some(diff) => unsafe { frame_tmp_set_long(frame, result_ptr, diff) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 - l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 - d2)) };
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} - {}",
                                op1.diagnostic_type_name(),
                                op2.diagnostic_type_name()
                            )
                        );
                    };
                    // SAFETY: `result_ptr` is the validated TMP slot for this
                    // specialized opcode, and the owned result is constructed
                    // before it replaces that slot.
                    unsafe {
                        frame_tmp_set(frame, result_ptr, prepared_sub_result(&left, &right))
                    };
                }
            }

            OpCode::IsSmaller_CvConst | OpCode::JmpZ_Lt_CvConst | OpCode::JmpNZ_Lt_CvConst => {
                // SAFETY: the dispatcher receives a live frame whose CV, TMP, literal, and
                // jump operands were range-checked when this op-array was compiled; reference
                // cells remain live for the duration of the comparison slow path.
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result = if let (Some(left), Some(right)) = (op1.as_long(), op2.as_long()) {
                    left < right
                } else if let Some((left, right)) = comparison_numeric_pair(op1, op2) {
                    left < right
                } else if let (Some(left), Some(right)) = (op1.as_str(), op2.as_str()) {
                    left < right
                } else {
                    let result = prepared_comparison_result(
                        eg,
                        frame,
                        op_array,
                        opline,
                        OpCode::IsSmaller_CvConst,
                        op1,
                        op2,
                    )?;
                    resume_pending_exception!();
                    let Ok(result) = result else {
                        throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                    };
                    result
                };

                match opline.opcode {
                    OpCode::IsSmaller_CvConst => {
                        let result_ptr = unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        };
                        unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
                    }
                    OpCode::JmpZ_Lt_CvConst if !result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    OpCode::JmpNZ_Lt_CvConst if result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    _ => {
                        opline_ptr = unsafe { opline_ptr.add(1) };
                    }
                }
            }

            OpCode::IsSmallerOrEqual_CvConst
            | OpCode::JmpZ_Le_CvConst
            | OpCode::JmpNZ_Le_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result = if let (Some(left), Some(right)) = (op1.as_long(), op2.as_long()) {
                    left <= right
                } else if let Some((left, right)) = comparison_numeric_pair(op1, op2) {
                    left <= right
                } else if let (Some(left), Some(right)) = (op1.as_str(), op2.as_str()) {
                    left <= right
                } else {
                    let result = prepared_comparison_result(
                        eg,
                        frame,
                        op_array,
                        opline,
                        OpCode::IsSmallerOrEqual_CvConst,
                        op1,
                        op2,
                    )?;
                    resume_pending_exception!();
                    let Ok(result) = result else {
                        throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                    };
                    result
                };

                match opline.opcode {
                    OpCode::IsSmallerOrEqual_CvConst => {
                        let result_ptr = unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        };
                        unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
                    }
                    OpCode::JmpZ_Le_CvConst if !result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    OpCode::JmpNZ_Le_CvConst if result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    _ => opline_ptr = unsafe { opline_ptr.add(1) },
                }
            }

            OpCode::IsEqual_CvConst | OpCode::JmpZ_Eq_CvConst | OpCode::JmpNZ_Eq_CvConst => {
                let op1_cv = unsafe { (*frame).cv(opline.op1 as u32) };
                let op1 = if op1_cv.is_reference() {
                    unsafe { &*op1_cv.as_ref_ptr() }
                } else {
                    op1_cv
                };
                let op2 = &op_array.literals()[opline.op2 as usize];
                let result = if let (Some(left), Some(right)) = (op1.as_long(), op2.as_long()) {
                    left == right
                } else if let Some((left, right)) = comparison_numeric_pair(op1, op2) {
                    left == right
                } else if let (Some(left), Some(right)) = (op1.as_str(), op2.as_str()) {
                    left == right
                } else {
                    let result = prepared_comparison_result(
                        eg,
                        frame,
                        op_array,
                        opline,
                        OpCode::IsEqual_CvConst,
                        op1,
                        op2,
                    )?;
                    resume_pending_exception!();
                    let Ok(result) = result else {
                        throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                    };
                    result
                };

                match opline.opcode {
                    OpCode::IsEqual_CvConst => {
                        let result_ptr = unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        };
                        unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
                    }
                    OpCode::JmpZ_Eq_CvConst if !result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    OpCode::JmpNZ_Eq_CvConst if result => {
                        unsafe {
                            (*frame).opline = op_array
                                .instructions()
                                .as_ptr()
                                .add(opline.result as usize)
                        };
                        continue;
                    }
                    _ => opline_ptr = unsafe { opline_ptr.add(1) },
                }
            }

            OpCode::Add => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let op1 = op1.dereferenced();
                let op2 = op2.dereferenced();
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    match l1.checked_add(l2) {
                        Some(sum) => unsafe { frame_tmp_set_long(frame, result_ptr, sum) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 + l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 + d2)) };
                } else if let (Some(left), Some(right)) = (op1.as_array(), op2.as_array()) {
                    write_array_union_result(frame, opline.result, left, right);
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} + {}",
                                op1.diagnostic_type_name(),
                                op2.diagnostic_type_name()
                            )
                        );
                    };
                    let result = prepared_add_result(&left, &right);
                    // SAFETY: `result_ptr` is this instruction's resolved result slot,
                    // and both operand borrows have ended before the owned result write.
                    unsafe { frame_tmp_set(frame, result_ptr, result) };
                }
            }

            OpCode::Sub => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    l1.checked_sub(l2).map_or_else(
                        || Err(Value::double(l1 as f64 - l2 as f64)),
                        Ok,
                    )
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    Err(Value::double(d1 - d2))
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} - {}",
                                op1.dereferenced().diagnostic_type_name(),
                                op2.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    split_arithmetic_result(prepared_sub_result(&left, &right))
                };
                // SAFETY: `result_ptr` is this instruction's resolved result slot,
                // and the operand borrows are no longer used after building `result`.
                unsafe {
                    match result {
                        Ok(value) => frame_tmp_set_long(frame, result_ptr, value),
                        Err(value) => frame_tmp_set(frame, result_ptr, value),
                    };
                }
            }

            OpCode::Mul => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    match l1.checked_mul(l2) {
                        Some(prod) => unsafe { frame_tmp_set_long(frame, result_ptr, prod) },
                        None => unsafe {
                            frame_tmp_set(frame, result_ptr, Value::double(l1 as f64 * l2 as f64))
                        },
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    unsafe { frame_tmp_set(frame, result_ptr, Value::double(d1 * d2)) };
                } else {
                    let canonical = opline._pad & ARITHMETIC_COMPOUND_ASSIGN == 0
                        && commutative_operator_uses_canonical_validation(op1, op2);
                    let pair = if canonical {
                        prepare_commutative_arithmetic_operator_pair(
                            eg, frame, op_array, opline, op1, op2,
                        )?
                    } else {
                        prepare_arithmetic_operator_pair(eg, frame, op_array, opline, op1, op2)?
                    };
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        let (error_left, error_right) = if canonical {
                            commutative_operator_error_operands(
                                op1,
                                op2,
                                opline.op1_type,
                                opline.op2_type,
                            )
                        } else {
                            (op1, op2)
                        };
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} * {}",
                                error_left.dereferenced().diagnostic_type_name(),
                                error_right.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    let result = prepared_mul_result(&left, &right);
                    // SAFETY: `result_ptr` is this instruction's resolved result slot,
                    // and both operand borrows have ended before the owned result write.
                    unsafe { frame_tmp_set(frame, result_ptr, result) };
                }
            }

            OpCode::Div => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    if l2 == 0 {
                        throw_operator!("DivisionByZeroError", "Division by zero");
                    }
                    if let Some(quotient) = l1.checked_div(l2)
                        && l1.checked_rem(l2) == Some(0)
                    {
                        Ok(quotient)
                    } else {
                        Err(Value::double(l1 as f64 / l2 as f64))
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    if d2 == 0.0 {
                        throw_operator!("DivisionByZeroError", "Division by zero");
                    }
                    Err(Value::double(d1 / d2))
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} / {}",
                                op1.dereferenced().diagnostic_type_name(),
                                op2.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    let Some(result) = prepared_div_result(&left, &right) else {
                        throw_operator!("DivisionByZeroError", "Division by zero");
                    };
                    split_arithmetic_result(result)
                };
                // SAFETY: `result_ptr` is this instruction's resolved result slot,
                // and the operand borrows are no longer used after building `result`.
                unsafe {
                    match result {
                        Ok(value) => frame_tmp_set_long(frame, result_ptr, value),
                        Err(value) => frame_tmp_set(frame, result_ptr, value),
                    };
                }
            }

            OpCode::Mod => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let (left, right) = if let (Some(left), Some(right)) =
                    (op1.as_long(), op2.as_long())
                {
                    (left, right)
                } else {
                    let Ok(left) = integer_operator_operand(op1) else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} % {}",
                                op1.dereferenced().diagnostic_type_name(),
                                op2.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    report_integer_operator_diagnostics(
                        eg, frame, op_array, opline, op1, left,
                    )?;
                    resume_pending_exception!();
                    let Ok(right) = integer_operator_operand(op2) else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} % {}",
                                op1.dereferenced().diagnostic_type_name(),
                                op2.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    report_integer_operator_diagnostics(
                        eg, frame, op_array, opline, op2, right,
                    )?;
                    resume_pending_exception!();
                    (left.value, right.value)
                };
                if right == 0 {
                    throw_operator!("DivisionByZeroError", "Modulo by zero");
                }
                let remainder = left.checked_rem(right).unwrap_or(0);
                // SAFETY: `result_ptr` is the current instruction's resolved
                // result slot, and both operand borrows are finished here.
                unsafe { frame_tmp_set_long(frame, result_ptr, remainder) };
            }

            OpCode::Concat_StringString => {
                let left = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                let right = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op2, opline.op2_type)
                };
                debug_assert_eq!(left.value_type(), ValueType::String);
                debug_assert_eq!(right.value_type(), ValueType::String);
                let lhs = unsafe { left.as_str().unwrap_unchecked() };
                let rhs = unsafe { right.as_str().unwrap_unchecked() };
                let mut concatenated = String::with_capacity(lhs.len() + rhs.len());
                concatenated.push_str(lhs);
                concatenated.push_str(rhs);
                let result_ptr = unsafe {
                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                };
                unsafe {
                    frame_tmp_set(frame, result_ptr, Value::string(concatenated))
                };
            }

            OpCode::Concat => {
                let compound = opline._pad & ARITHMETIC_COMPOUND_ASSIGN != 0;
                op_concat(eg, frame, op_array, opline, compound)?;
                resume_pending_exception!();
            }

            OpCode::Spaceship => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let ordering = |ordering| match ordering {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                let scalar_cmp = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    Some(ordering(l1.cmp(&l2)))
                } else if let Some((d1, d2)) = comparison_numeric_pair(op1, op2) {
                    Some(
                        d1.partial_cmp(&d2)
                            .map_or(PHP_COMPARISON_UNORDERED, ordering),
                    )
                } else {
                    op1.as_str()
                        .zip(op2.as_str())
                        .map(|(left, right)| ordering(left.cmp(right)))
                };
                let val = if let Some(cmp) = scalar_cmp {
                    cmp.signum() as i64
                } else {
                    let comparison = runtime_values_checked(
                        eg,
                        frame,
                        op_array,
                        opline,
                        op1,
                        op2,
                        RuntimeComparisonMode::Ordering,
                    )?;
                    resume_pending_exception!();
                    let Ok(cmp) = comparison else {
                        throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                    };
                    cmp.signum() as i64
                };
                unsafe { frame_tmp_set_long(frame, result_ptr, val) };
            }

            OpCode::Pow => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let result = if let (Some(l1), Some(l2)) =
                    (op1.to_arithmetic_long(), op2.to_arithmetic_long())
                {
                    if let Ok(exponent) = u32::try_from(l2)
                        && let Some(value) = l1.checked_pow(exponent)
                    {
                        Ok(value)
                    } else {
                        Err(Value::double((l1 as f64).powf(l2 as f64)))
                    }
                } else if let (Some(d1), Some(d2)) =
                    (op1.to_arithmetic_double(), op2.to_arithmetic_double())
                {
                    Err(Value::double(d1.powf(d2)))
                } else {
                    let pair = prepare_arithmetic_operator_pair(
                        eg, frame, op_array, opline, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Some((left, right)) = pair else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Unsupported operand types: {} ** {}",
                                op1.dereferenced().diagnostic_type_name(),
                                op2.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    split_arithmetic_result(prepared_pow_result(&left, &right))
                };
                // SAFETY: `result_ptr` is this instruction's resolved result slot,
                // and the operand borrows are no longer used after building `result`.
                unsafe {
                    match result {
                        Ok(value) => frame_tmp_set_long(frame, result_ptr, value),
                        Err(value) => frame_tmp_set(frame, result_ptr, value),
                    };
                }
            }

            OpCode::BitwiseAnd | OpCode::BitwiseOr | OpCode::BitwiseXor => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                if op1.value_type() == ValueType::Long && op2.value_type() == ValueType::Long {
                    // SAFETY: the exact Long guards make both raw payload reads
                    // valid, and `result_ptr` names this instruction's result slot.
                    let left = unsafe { op1.raw_long() };
                    let right = unsafe { op2.raw_long() };
                    let value = match opline.opcode {
                        OpCode::BitwiseAnd => left & right,
                        OpCode::BitwiseOr => left | right,
                        OpCode::BitwiseXor => left ^ right,
                        _ => unreachable!(),
                    };
                    unsafe { frame_tmp_set_long(frame, result_ptr, value) };
                } else if op1.dereferenced().as_str().is_some()
                    && op2.dereferenced().as_str().is_some()
                {
                    let value = bitwise_binary_value(op1, op2, opline.opcode);
                    unsafe { frame_tmp_set(frame, result_ptr, value) };
                } else {
                    let symbol = match opline.opcode {
                        OpCode::BitwiseAnd => "&",
                        OpCode::BitwiseOr => "|",
                        OpCode::BitwiseXor => "^",
                        _ => unreachable!(),
                    };
                    let canonical = opline._pad & ARITHMETIC_COMPOUND_ASSIGN == 0
                        && commutative_operator_uses_canonical_validation(op1, op2);
                    let (left, right) = if !canonical {
                        let Some(left) = prepare_integer_operator_operand(
                            eg, frame, op_array, opline, op1,
                        )? else {
                            throw_operator!(
                                "TypeError",
                                &format!(
                                    "Unsupported operand types: {} {symbol} {}",
                                    op1.dereferenced().diagnostic_type_name(),
                                    op2.dereferenced().diagnostic_type_name()
                                )
                            );
                        };
                        resume_pending_exception!();
                        let Some(right) = prepare_integer_operator_operand(
                            eg, frame, op_array, opline, op2,
                        )? else {
                            throw_operator!(
                                "TypeError",
                                &format!(
                                    "Unsupported operand types: {} {symbol} {}",
                                    op1.dereferenced().diagnostic_type_name(),
                                    op2.dereferenced().diagnostic_type_name()
                                )
                            );
                        };
                        resume_pending_exception!();
                        (left, right)
                    } else {
                        let (left, right) = match (
                            integer_operator_operand(op1),
                            integer_operator_operand(op2),
                        ) {
                            (Ok(left), Ok(right)) => (left, right),
                            _ => {
                                let (error_left, error_right) =
                                    commutative_operator_error_operands(
                                        op1,
                                        op2,
                                        opline.op1_type,
                                        opline.op2_type,
                                    );
                                throw_operator!(
                                    "TypeError",
                                    &format!(
                                        "Unsupported operand types: {} {symbol} {}",
                                        error_left.dereferenced().diagnostic_type_name(),
                                        error_right.dereferenced().diagnostic_type_name()
                                    )
                                );
                            }
                        };
                        report_integer_operator_diagnostics(
                            eg, frame, op_array, opline, op1, left,
                        )?;
                        resume_pending_exception!();
                        report_integer_operator_diagnostics(
                            eg, frame, op_array, opline, op2, right,
                        )?;
                        resume_pending_exception!();
                        (left.value, right.value)
                    };
                    let value = match opline.opcode {
                        OpCode::BitwiseAnd => left & right,
                        OpCode::BitwiseOr => left | right,
                        OpCode::BitwiseXor => left ^ right,
                        _ => unreachable!(),
                    };
                    unsafe { frame_tmp_set_long(frame, result_ptr, value) };
                }
            }

            OpCode::ShiftLeft => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let Some(l1) = prepare_integer_operator_operand(
                    eg, frame, op_array, opline, op1,
                )? else {
                    throw_operator!(
                        "TypeError",
                        &format!(
                            "Unsupported operand types: {} << {}",
                            op1.dereferenced().diagnostic_type_name(),
                            op2.dereferenced().diagnostic_type_name()
                        )
                    );
                };
                resume_pending_exception!();
                let Some(l2) = prepare_integer_operator_operand(
                    eg, frame, op_array, opline, op2,
                )? else {
                    throw_operator!(
                        "TypeError",
                        &format!(
                            "Unsupported operand types: {} << {}",
                            op1.dereferenced().diagnostic_type_name(),
                            op2.dereferenced().diagnostic_type_name()
                        )
                    );
                };
                resume_pending_exception!();
                if l2 < 0 {
                    throw_operator!("ArithmeticError", "Bit shift by negative number");
                }
                let result = if l2 >= i64::BITS as i64 {
                    0
                } else {
                    l1.wrapping_shl(l2 as u32)
                };
                unsafe { frame_tmp_set_long(frame, result_ptr, result) };
            }

            OpCode::ShiftRight => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let Some(l1) = prepare_integer_operator_operand(
                    eg, frame, op_array, opline, op1,
                )? else {
                    throw_operator!(
                        "TypeError",
                        &format!(
                            "Unsupported operand types: {} >> {}",
                            op1.dereferenced().diagnostic_type_name(),
                            op2.dereferenced().diagnostic_type_name()
                        )
                    );
                };
                resume_pending_exception!();
                let Some(l2) = prepare_integer_operator_operand(
                    eg, frame, op_array, opline, op2,
                )? else {
                    throw_operator!(
                        "TypeError",
                        &format!(
                            "Unsupported operand types: {} >> {}",
                            op1.dereferenced().diagnostic_type_name(),
                            op2.dereferenced().diagnostic_type_name()
                        )
                    );
                };
                resume_pending_exception!();
                if l2 < 0 {
                    throw_operator!("ArithmeticError", "Bit shift by negative number");
                }
                let result = if l2 >= i64::BITS as i64 {
                    if l1 < 0 { -1 } else { 0 }
                } else {
                    l1.wrapping_shr(l2 as u32)
                };
                unsafe { frame_tmp_set_long(frame, result_ptr, result) };
            }

            OpCode::BitwiseNot => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                if let Some(value) = val.dereferenced().as_long() {
                    // SAFETY: `result_ptr` is the current instruction's resolved
                    // result slot and the borrowed operand is only read by value.
                    unsafe { frame_tmp_set_long(frame, result_ptr, !value) };
                } else if val.dereferenced().as_str().is_some() {
                    let value = bitwise_not_value(val);
                    unsafe { frame_tmp_set(frame, result_ptr, value) };
                } else {
                    let Some(value) = prepare_integer_operator_operand(
                        eg, frame, op_array, opline, val,
                    )? else {
                        throw_operator!(
                            "TypeError",
                            &format!(
                                "Cannot perform bitwise not on {}",
                                val.dereferenced().diagnostic_type_name()
                            )
                        );
                    };
                    resume_pending_exception!();
                    unsafe { frame_tmp_set_long(frame, result_ptr, !value) };
                }
            }

            OpCode::IsEqual | OpCode::IsNotEqual | OpCode::IsSmaller | OpCode::IsSmallerOrEqual => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let result = if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                    match opline.opcode {
                        OpCode::IsEqual => l1 == l2,
                        OpCode::IsNotEqual => l1 != l2,
                        OpCode::IsSmaller => l1 < l2,
                        OpCode::IsSmallerOrEqual => l1 <= l2,
                        _ => unreachable!(),
                    }
                } else if let Some((d1, d2)) = comparison_numeric_pair(op1, op2) {
                    match opline.opcode {
                        OpCode::IsEqual => d1 == d2,
                        OpCode::IsNotEqual => d1 != d2,
                        OpCode::IsSmaller => d1 < d2,
                        OpCode::IsSmallerOrEqual => d1 <= d2,
                        _ => unreachable!(),
                    }
                } else if let (Some(s1), Some(s2)) = (op1.as_str(), op2.as_str()) {
                    match opline.opcode {
                        OpCode::IsEqual => s1 == s2,
                        OpCode::IsNotEqual => s1 != s2,
                        OpCode::IsSmaller => s1 < s2,
                        OpCode::IsSmallerOrEqual => s1 <= s2,
                        _ => unreachable!(),
                    }
                } else {
                    let result = prepared_comparison_result(
                        eg, frame, op_array, opline, opline.opcode, op1, op2,
                    )?;
                    resume_pending_exception!();
                    let Ok(result) = result else {
                        throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                    };
                    result
                };

                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                let op1 = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let op2 = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };

                let Ok(identical) = values_identical_checked(op1, op2) else {
                    throw_operator!("Error", "Nesting level too deep - recursive dependency?");
                };

                let result = match opline.opcode {
                    OpCode::IsIdentical => identical,
                    _ => !identical,
                };
                unsafe { frame_tmp_set_bool(frame, result_ptr, result) };
            }

            OpCode::Isset => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let is_set = val.value_type() != ValueType::Undef && val.value_type() != ValueType::Null;
                unsafe { frame_tmp_set_bool(frame, result_ptr, is_set) };
            }

            OpCode::Cast => {
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let source = val.dereferenced();
                let result_ptr = unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) };
                let casted = match opline.extended_value {
                    0 => {                                   // (int)
                        let converted = explicit_long_conversion(source);
                        if let Some(message) = explicit_numeric_cast_warning(
                            source,
                            ExplicitNumericCastTarget::Int,
                        ) {
                            report_php_warning(eg, frame, op_array, opline, &message, false)?;
                            resume_pending_exception!();
                        }
                        Value::long(converted)
                    }
                    1 => {                                   // (float)
                        let converted = explicit_float_conversion(source);
                        if let Some(message) = explicit_numeric_cast_warning(
                            source,
                            ExplicitNumericCastTarget::Float,
                        ) {
                            report_php_warning(eg, frame, op_array, opline, &message, false)?;
                            resume_pending_exception!();
                        }
                        Value::double(converted)
                    }
                    2 => {                                   // (string)
                        if source.as_double().is_some_and(f64::is_nan) {
                            let converted =
                                Value::string(source.echo_to_string_with_precision(eg.precision));
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "unexpected NAN value was coerced to string",
                                false,
                            )?;
                            resume_pending_exception!();
                            converted
                        } else if matches!(source.value_type(), ValueType::Object | ValueType::Closure) {
                            if source.value_type() == ValueType::Closure {
                                throw_operator!(
                                    "Error",
                                    "Object of class Closure could not be converted to string"
                                );
                            }
                            let class_name = source
                                .as_object()
                                .map(|object| object.class_name.to_string())
                                .unwrap_or_else(|| "object".to_string());
                            let conversion = call_magic_method(eg, source, "__tostring", &[])?;
                            resume_pending_exception!();
                            if let Some(result) = conversion {
                                let Some(rendered) = result.as_str() else {
                                    throw_operator!(
                                        "TypeError",
                                        &format!(
                                            "{class_name}::__toString(): Return value must be of type string"
                                        )
                                    );
                                };
                                Value::string(rendered)
                            } else {
                                throw_operator!(
                                    "Error",
                                    &format!(
                                        "Object of class {class_name} could not be converted to string"
                                    )
                                );
                            }
                        } else {
                            report_array_to_string_conversion!(source);
                            Value::string(source.echo_to_string_with_precision(eg.precision))
                        }
                    }
                    3 => {                                   // (bool)
                        if source.as_double().is_some_and(f64::is_nan) {
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "unexpected NAN value was coerced to bool",
                                false,
                            )?;
                            resume_pending_exception!();
                        }
                        Value::bool(val.dereferenced().is_truthy())
                    }
                    4 => {                                   // (array)
                        if source.as_double().is_some_and(f64::is_nan) {
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "unexpected NAN value was coerced to array",
                                false,
                            )?;
                            resume_pending_exception!();
                            let mut array = PhpArray::new();
                            array.push(val.dereferenced().clone());
                            Value::array(array)
                        } else {
                            match source.value_type() {
                                ValueType::Array => source.clone(),
                                ValueType::Object => cast_object_to_array(source, eg),
                                ValueType::Null | ValueType::Undef => Value::array(PhpArray::new()),
                                _ => {
                                    let mut array = PhpArray::new();
                                    array.push(source.clone());
                                    Value::array(array)
                                }
                            }
                        }
                    }
                    5 => {                                   // (object)
                        if source.as_double().is_some_and(f64::is_nan) {
                            let converted =
                                Value::object(PhpObject::std_class(HashMap::new()));
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "unexpected NAN value was coerced to object",
                                false,
                            )?;
                            resume_pending_exception!();
                            converted.as_object_mut().unwrap().set_property(
                                "scalar",
                                val.dereferenced().clone(),
                            );
                            converted
                        } else {
                            match source.value_type() {
                                ValueType::Object => source.clone(),
                                ValueType::Array => {
                                    let mut object = PhpObject::std_class(HashMap::new());
                                    for (key, value) in source.as_array().unwrap().iter() {
                                        let key = match key {
                                            ArrayKey::Int(key) => key.to_string(),
                                            ArrayKey::String(key) => key,
                                        };
                                        object.set_property(&key, value.clone());
                                    }
                                    Value::object(object)
                                }
                                ValueType::Null | ValueType::Undef => {
                                    Value::object(PhpObject::std_class(HashMap::new()))
                                }
                                _ => {
                                    let mut properties = HashMap::with_capacity(1);
                                    properties.insert("scalar".to_string(), source.clone());
                                    Value::object(PhpObject::std_class(properties))
                                }
                            }
                        }
                    }
                    _ => source.clone(),
                };
                unsafe { frame_tmp_set(frame, result_ptr, casted) };
            }

            OpCode::BoolNot => {
                // SAFETY: operands were allocated by this op array and the
                // compiler reserves the result as a live TMP/VAR frame slot.
                unsafe {
                    let val = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    );
                    let result_ptr =
                        (*frame).get_op_mut(opline.result as u32, opline.result_type);
                    frame_tmp_set_bool(frame, result_ptr, !val.is_truthy());
                }
            }

            OpCode::AssertCheck => {
                if !eg.assertion_state.active {
                    let target = opline.op1 as usize;
                    // SAFETY: the compiler emits a live assertion result slot
                    // and patches op1 to an instruction in this active array.
                    unsafe {
                        debug_assert!(target < op_array.instructions().len());
                        let result_ptr = (*frame).get_op_mut(
                            opline.result as u32,
                            opline.result_type,
                        );
                        frame_tmp_set_bool(frame, result_ptr, true);
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
                    continue;
                }
            }

            OpCode::Jmp => {
                #[cfg(feature = "vm-stats")]
                if opline.extended_value != 0 {
                    stats::inc_jit_rejected_backedge_hit(opline.extended_value);
                }
                // op1 = absolute instruction index to jump to
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }

                continue; // skip normal advance
            }

            OpCode::JmpFinally => {
                if opline._pad & crate::vm::instruction::JMP_FLAG_FINALLY_END != 0 {
                    if finally_jump_state(
                        frame,
                        op_array,
                        FINALLY_JUMP_RESUME,
                        0,
                        false,
                    )
                    .is_some()
                    {
                        continue;
                    }
                } else {
                    finally_jump_state(
                        frame,
                        op_array,
                        FINALLY_JUMP_START,
                        u32::from(opline.op1),
                        opline._pad & crate::vm::instruction::JMP_FLAG_TARGET_OUTSIDE_TRY != 0,
                    );
                    continue;
                }
            }

            #[cfg(feature = "quick-loops")]
            OpCode::QuickLongLoopJmp => {
                unsafe { execute_quick_loop_backedge(eg, frame, op_array, opline)? };
                continue;
            }

            #[cfg(not(feature = "quick-loops"))]
            OpCode::QuickLongLoopJmp => {
                let target = opline.op1 as usize;
                unsafe {
                    (*frame).opline = op_array.instructions().as_ptr().add(target);
                }
                continue;
            }

            OpCode::JmpZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if !val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpZ is also a block leader

            }

            OpCode::JmpNZ => {
                // op1 = value to test, op2 = absolute jump target
                let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if val.is_truthy() {
                    let target = opline.op2 as usize;
                    unsafe {
                        (*frame).opline = op_array.instructions().as_ptr().add(target);
                    }
    
                    continue;
                }
                // Fall-through after JmpNZ is also a block leader

            }

            OpCode::DirectInternalCall1 => {
                // The handler ID is emitted from the same metadata used to
                // register the direct ABI. No function lookup, cache probe or
                // FunctionType check remains in this hot path.
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                    opline.extended_value,
                ) else {
                    return Err(VmError::Fatal(
                        "Invalid direct internal handler ID".into(),
                    ));
                };
                if kind == crate::builtin_metadata::DirectInternalKind::Ord
                    && argument.dereferenced().value_type() == ValueType::Null
                {
                    report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        "ord(): Passing null to parameter #1 ($character) of type string is deprecated",
                    )?;
                }
                let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if kind.result_may_need_cleanup() && opline.result_type == OpType::Tmp {
                        unsafe { frame_tmp_set(frame, result_ptr, result) };
                    } else {
                        // Scalar direct kinds always overwrite their own unique
                        // TMP, so its previous value is Undef or scalar too.
                        unsafe { result_ptr.write(result) };
                    }
                }
            }

            OpCode::DirectInternalCall2 => {
                let first = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                let second = unsafe {
                    &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
                };
                let Some(kind) = crate::builtin_metadata::DirectInternalKind::from_id(
                    opline.extended_value,
                ) else {
                    return Err(VmError::Fatal(
                        "Invalid direct internal handler ID".into(),
                    ));
                };
                let result = crate::stdlib::invoke_direct_internal2(kind, first, second, eg)?;
                resume_pending_exception!();

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if kind.result_may_need_cleanup() && opline.result_type == OpType::Tmp {
                        unsafe { frame_tmp_set(frame, result_ptr, result) };
                    } else {
                        unsafe { result_ptr.write(result) };
                    }
                }
            }

            OpCode::Strlen => {
                let argument = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                if argument.dereferenced().value_type() == ValueType::Null {
                    report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        "strlen(): Passing null to parameter #1 ($string) of type string is deprecated",
                    )?;
                }
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                        unsafe { Value::write_long(result_ptr, length) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::long(length)) };
                    }
                }
            }

            OpCode::Strlen_Cv => {
                let argument = unsafe { (*frame).cv(opline.op1 as u32) };
                if argument.dereferenced().value_type() == ValueType::Null {
                    report_php_deprecation(
                        eg,
                        frame,
                        op_array,
                        opline,
                        "strlen(): Passing null to parameter #1 ($string) of type string is deprecated",
                    )?;
                }
                let length = crate::stdlib::direct_strlen_len(argument);

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    debug_assert!(matches!(opline.result_type, OpType::Tmp | OpType::Var));
                    unsafe { Value::write_long(result_ptr, length) };
                }
            }

            OpCode::Strlen_String => {
                let argument = unsafe {
                    &*proven_scalar_op_ptr(frame, op_array, opline.op1, opline.op1_type)
                };
                debug_assert_eq!(argument.value_type(), ValueType::String);
                let length = unsafe { argument.as_str().unwrap_unchecked().len() as i64 };

                if opline.result_type != OpType::Unused {
                    let result_ptr = unsafe {
                        (*frame).get_op_mut(opline.result as u32, opline.result_type)
                    };
                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                        unsafe { Value::write_long(result_ptr, length) };
                    } else {
                        unsafe { slot_set(result_ptr, Value::long(length)) };
                    }
                }
            }

            OpCode::CallUserFuncArray => {
                match op_call_user_func_array(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(thrown) => {
                        eg.exception = Some(thrown);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::InitUserCall => {
                // A one-argument call_user_func()/call_user_func_array() with
                // a simple argument compiles to an adjacent
                // InitUserCall + SendUser + DoFcall sequence. Once its runtime
                // callback names a pure direct-ABI internal function,
                // invoke that handler on the caller's borrowed value and skip
                // the callback frame and the next two VM dispatches entirely.
                let next = unsafe { &*opline_ptr.add(1) };
                let direct_shape = direct_user_calls_enabled()
                    && !op_array.strict_types
                    && opline.extended_value == 1
                    && matches!(next.opcode, OpCode::SendUser | OpCode::SendUserChecked)
                    && next.extended_value == 0
                    && unsafe { (*opline_ptr.add(2)).opcode == OpCode::DoFcall };
                let mut initialized = false;

                if direct_shape {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    let callback_raw = unsafe {
                        &*(*frame).get_op_ptr(
                            opline.op1 as u32,
                            opline.op1_type,
                            op_array,
                        )
                    };
                    let callback = if callback_raw.is_reference() {
                        unsafe { &*callback_raw.as_ref_ptr() }
                    } else {
                        callback_raw
                    };
                    let direct_kind = callback.as_str().and_then(|name| {
                        crate::builtin_metadata::direct_internal_spec(name)
                            .filter(|spec| {
                                spec.required_args <= 1
                                    && spec.max_args >= 1
                                    && spec.kind.lowering()
                                        != crate::builtin_metadata::DirectInternalLowering::Generic2
                            })
                            .map(|spec| spec.kind)
                    });

                    if let Some(kind) = direct_kind {
                        let argument = unsafe {
                            &*(*frame).get_op_ptr(
                                next.op1 as u32,
                                next.op1_type,
                                op_array,
                            )
                        };
                        let result = crate::stdlib::invoke_direct_internal1(kind, argument)?;
                        if next2.result_type != OpType::Unused {
                            let result_ptr = unsafe {
                                (*frame).get_op_mut(
                                    next2.result as u32,
                                    next2.result_type,
                                )
                            };
                            if matches!(next2.result_type, OpType::Tmp | OpType::Var) {
                                unsafe { frame_tmp_set(frame, result_ptr, result) };
                            } else {
                                unsafe { frame_slot_set(frame, result_ptr, result) };
                            }
                        }
                        // Loop-bottom advance adds one more instruction.
                        opline_ptr = unsafe { opline_ptr.add(2) };
                        initialized = true;
                    } else if let Some(resolved) =
                        resolve_user_call_at_opline(eg, frame, op_array, opline)
                    {
                        init_resolved_user_call(
                            eg,
                            frame,
                            opline.extended_value,
                            resolved,
                        );
                        initialized = true;
                    }
                }

                if !initialized {
                    match op_init_user_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ColdResult::Unhandled(thrown) => {
                            eg.exception = Some(thrown);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }

            OpCode::InitFcall => {
                // op1 = num_args
                // op2 = CONST index pointing to function name string
                // extended_value = CONST index of fallback name (for unqualified calls in namespace), 0 = no fallback

                if opline._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE != 0
                    && let Some((result, do_fcall_ptr)) = unsafe {
                        try_execute_callback_array_pipeline(eg, frame, op_array, opline_ptr)
                    }?
                {
                    unsafe { complete_direct_scalar_long_call(frame, do_fcall_ptr, result) };
                    continue 'vm;
                }
                if opline._pad & CALL_FLAG_STAGED_CALLBACK_ARRAY_PIPELINE != 0
                    && let Some((result, do_fcall_ptr)) = unsafe {
                        try_execute_staged_callback_array_pipeline(
                            eg,
                            frame,
                            op_array,
                            opline_ptr,
                        )
                    }?
                {
                    unsafe { complete_direct_scalar_long_call(frame, do_fcall_ptr, result) };
                    continue 'vm;
                }
                if opline._pad & CALL_FLAG_FILTER_MAP_CALLBACK_ARRAY_PIPELINE != 0
                    && let Some((result, do_fcall_ptr)) = unsafe {
                        try_execute_filter_map_callback_array_pipeline(
                            eg,
                            frame,
                            op_array,
                            opline_ptr,
                        )
                    }?
                {
                    unsafe { complete_direct_scalar_long_call(frame, do_fcall_ptr, result) };
                    continue 'vm;
                }
                if opline._pad & CALL_FLAG_CALLBACK_ARRAY_PIPELINE_JSON_SINK != 0
                    && let Some((result, do_fcall_ptr)) = unsafe {
                        try_execute_json_callback_array_pipeline(
                            eg,
                            frame,
                            op_array,
                            opline_ptr,
                        )
                }?
                {
                    // SAFETY: the callback pipeline returns this live frame's DoFcall site.
                    unsafe {
                        complete_direct_value_call(frame, do_fcall_ptr, Value::string(result))
                    };
                    continue 'vm;
                }

                // Inline cache: if we resolved this function before, reuse the pointer
                let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                let cached = op_array.cache[ip].func;
                let func_ptr = if !cached.is_null() {
                    cached
                } else {
                    let name_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                    let name = name_val.as_str().unwrap_or_else(|| {
                        panic!("INIT_FCALL: op2 must be a string");
                    });
                    let func_ptr_opt = eg.find_function(name).or_else(|| {
                        if opline.extended_value != 0 {
                            let fallback_val = unsafe { &*(*frame).get_op_ptr(opline.extended_value, OpType::Const, op_array) };
                            if let Some(fallback_name) = fallback_val.as_str() {
                                return eg.find_function(fallback_name);
                            }
                        }
                        None
                    });
                    match func_ptr_opt {
                        Some(ptr) => {
                            // Cache for next time (don't cache failures — function may be defined later via include)
                            unsafe { (*(op_array.cache.as_ptr().add(ip) as *mut crate::vm::instruction::InlineCache)).func = ptr; }
                            ptr
                        }
                        None => {
                            let err = make_error_value("Error", &format!("Call to undefined function {}()", name_val.as_str().unwrap_or("?")));
                            match throw_in_frame(eg, frame, err)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                    }
                };

                let num_args = opline.op1 as u32;
                let common = unsafe { &*func_ptr };
                let mut scalar_plan_eligible = false;
                if common.fn_type == FunctionType::User
                    && num_args == common.sig.public_arity()
                {
                    let user = unsafe { &*(func_ptr as *const UserFunction) };
                    scalar_plan_eligible = user.composed_scalar_long_plan.is_some()
                        || user.scalar_double_plan.is_some();
                    if let Some(plan) = user.scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_scalar_long_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = common.call_count.get();
                            if count < u32::MAX {
                                common.call_count.set(count + 1);
                            }
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_composed_scalar_long_call(
                                eg,
                                frame,
                                op_array,
                                opline_ptr,
                                func_ptr,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_scalar_double_call(
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                plan,
                            )
                        } {
                            stats::inc_do_fcall_fast();
                            stats::inc_return_fast();
                            let count = common.call_count.get();
                            if count < u32::MAX {
                                common.call_count.set(count + 1);
                            }
                            unsafe {
                                complete_direct_value_call(
                                    frame,
                                    do_fcall_ptr,
                                    Value::double(result),
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_double_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_composed_scalar_double_call(
                                eg,
                                frame,
                                op_array,
                                opline_ptr.add(1),
                                common,
                                user,
                                None,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_value_call(
                                    frame,
                                    do_fcall_ptr,
                                    Value::double(result),
                                );
                            }
                            continue 'vm;
                        }
                    }
                    if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                        scalar_plan_eligible = true;
                        if let Some((result, do_fcall_ptr)) = unsafe {
                            try_execute_direct_composed_scalar_body_call(
                                eg,
                                frame,
                                op_array,
                                opline_ptr,
                                func_ptr,
                                user,
                                plan,
                            )
                        } {
                            unsafe {
                                complete_direct_scalar_long_call(
                                    frame,
                                    do_fcall_ptr,
                                    result,
                                );
                            }
                            continue 'vm;
                        }
                    }
                }

                let pending_call = unsafe { (*frame).call };
                let deferred = should_defer_scalar_call(opline, scalar_plan_eligible);
                let call = if deferred {
                    eg.pending_call_stack.push_deferred_scalar_call(
                        func_ptr,
                        num_args,
                        num_args,
                        frame,
                        pending_call,
                    )
                } else {
                    eg.vm_stack.push_call_frame(
                        func_ptr,
                        num_args,
                        num_args,
                        frame,
                        pending_call,
                    )
                };
                unsafe {
                    (*frame).call = call;
                }

                // Peek ahead: if next is Sub_CvConst whose result feeds SendVal,
                // inline the subtraction + arg write, skip 2 instructions.
                let next = unsafe { &*opline_ptr.add(1) };
                if next.opcode == OpCode::Sub_CvConst {
                    let next2 = unsafe { &*opline_ptr.add(2) };
                    if next2.opcode == OpCode::SendVal
                        && next2.op1_type == OpType::Tmp
                        && next2.op1 == next.result
                    {
                        let op1_cv = unsafe { (*frame).cv(next.op1 as u32) };
                        let op1 = if op1_cv.is_reference() { unsafe { &*op1_cv.as_ref_ptr() } } else { op1_cv };
                        let op2 = &op_array.literals()[next.op2 as usize];
                        if let (Some(l1), Some(l2)) = (op1.as_long(), op2.as_long()) {
                            let dst = unsafe {
                                (call as *mut Value).add(CALL_FRAME_SLOTS + next2.op2 as usize)
                            };
                            match l1.checked_sub(l2) {
                                Some(diff) => unsafe { Value::write_long(dst, diff) },
                                None => unsafe { dst.write(Value::double(l1 as f64 - l2 as f64)) },
                            }
                            // Skip Sub_CvConst + SendVal: advance local +2, loop bottom adds +1 → net +3
                            opline_ptr = unsafe { opline_ptr.add(2) };
                        }
                    }
                } else if next.opcode == OpCode::SendVal
                    && unsafe { try_send_scalar_arg(frame, call, op_array, next) }
                {
                    // InitFcall + scalar SendVal: argument is already in the
                    // callee frame. Loop-bottom advance skips the SendVal.
                    opline_ptr = unsafe { opline_ptr.add(1) };
                }
            }

            OpCode::SendVal => {
                // Send value to pending call frame
                // op1 = value to send, op2 = argument number (0-based)
                let call = unsafe {
                    let call = (*frame).call;
                    if opline._pad & (SEND_FLAG_GLOBALS | SEND_FLAG_NONREFERENCEABLE) != 0 {
                        let common = &*(*call).func;
                        let parameter_index = opline.extended_value as usize;
                        if common.sig.is_param_by_ref(parameter_index as u32)
                            && !common
                                .sig
                                .is_param_prefer_ref(parameter_index as u32)
                        {
                            let parameter_name = common
                                .sig
                                .param_names
                                .get(parameter_index)
                                .map(String::as_str)
                                .unwrap_or("unknown");
                            let function_name = registered_function_name(eg, (*call).func);
                            let error = make_error_value(
                                "Error",
                                &format!(
                                    "{}(): Argument #{} (${}) could not be passed by reference",
                                    function_name,
                                    parameter_index + 1,
                                    parameter_name
                                ),
                            );
                            attach_call_argument_throwable_origin(
                                &error, eg, frame, op_array, opline,
                            );
                            cleanup_pending_calls(eg, frame);
                            match throw_in_frame(eg, frame, error)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    call
                };
                debug_assert!(!call.is_null());
                // SAFETY: `call` is the live pending activation and this
                // compiler-emitted send names a source in the live caller plus
                // one compiler-sized destination slot.
                let (dst, source, common) = unsafe {
                    (
                        (call as *mut Value).add(CALL_FRAME_SLOTS + opline.op2 as usize),
                        (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array),
                        &*(*call).func,
                    )
                };
                // SAFETY: source is resolved from the live caller op-array and
                // dst is the compiler-sized argument slot in the pending call.
                let borrowed = !unsafe { (*source).is_undef() }
                    && opline.op2 as u32 >= common.sig.this_offset
                    && unsafe {
                        try_init_borrowed_heap_arg(
                            call,
                            opline.op2 as u32 - common.sig.this_offset,
                            source,
                            dst,
                        )
                    };
                // For TMP/Var operands that are provably scalar (Long, Double, Bool, Null),
                // use raw 16-byte bitwise copy — no clone/drop overhead.
                // TMP values are consumed (not read again), so move semantics are valid.
                // IMPORTANT: owned types (String, Array, Object, Resource, Closure) and References
                // MUST go through clone to maintain refcount / avoid double-free.
                // SAFETY: source and dst are live slots established above;
                // Undef has no owned payload and dst has not been initialized.
                if unsafe { (*source).is_undef() } {
                    unsafe { dst.write(Value::null()) };
                } else if borrowed {
                    // The destination deliberately remains outside the owned
                    // heap bitmap; cleanup must not decrement the caller's Rc.
                } else if opline.op1_type == OpType::Tmp || opline.op1_type == OpType::Var {
                    let src = unsafe {
                        (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    };
                    let src_val = unsafe { &*src };
                    if !src_val.needs_cleanup() && !src_val.is_reference() {
                        // Scalar TMP/Var: safe bitwise move
                        unsafe { Value::raw_copy(src, dst) };
                    } else {
                        // Heap or reference TMP/Var: must clone + mark callee heap bits
                        let cloned = src_val.clone();
                        unsafe { dst.write(cloned) };
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                } else {
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                    let cloned = val.clone();
                    unsafe { dst.write(cloned) };
                    // Mark heap bit if needed
                    if unsafe { (*dst).needs_cleanup() } {
                        unsafe {
                            (*call).has_heap_slots = true;
                            let total = (*call).num_cvs + (*call).num_temps;
                            if total <= 64 {
                                (*call).heap_bitmap |= 1u64 << opline.op2;
                            }
                        }
                    }
                }
            }

            OpCode::SendRef => {
                // Send reference to caller's CV into callee frame
                // op1 = mutable CV/TMP/VAR slot in caller
                // op2 = argument number in callee (0-based)
                // SAFETY: the compiler emits a mutable frame operand and a
                // declared callee argument slot. References are forwarded
                // without reinterpreting external targets as frame offsets.
                unsafe {
                    if opline._pad & SEND_FLAG_GLOBALS != 0 {
                        let call = (*frame).call;
                        debug_assert!(!call.is_null());
                        let common = &*(*call).func;
                        let parameter_index = opline.extended_value as usize;
                        if common.sig.is_param_prefer_ref(parameter_index as u32) {
                            let source =
                                (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array);
                            let argument = (&*source).dereferenced().clone();
                            let arg_slot = (*call).cv_mut(opline.op2 as u32);
                            frame_slot_init(call, arg_slot as *mut Value, argument);
                            (*frame).opline = opline_ptr.add(1);
                            continue 'vm;
                        }
                        let parameter_name = common
                            .sig
                            .param_names
                            .get(parameter_index)
                            .map(String::as_str)
                            .unwrap_or("unknown");
                        let function_name = registered_function_name(eg, (*call).func);
                        let error = make_error_value(
                            "Error",
                            &format!(
                                "{}(): Argument #{} (${}) could not be passed by reference",
                                function_name,
                                parameter_index + 1,
                                parameter_name
                            ),
                        );
                        attach_call_argument_throwable_origin(
                            &error, eg, frame, op_array, opline,
                        );
                        cleanup_pending_calls(eg, frame);
                        match throw_in_frame(eg, frame, error)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(thrown) => {
                                eg.exception = Some(thrown);
                                return Ok(());
                            }
                        }
                    }
                    let caller_value = if opline.op1_type == OpType::Cv {
                        (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                    } else {
                        (*frame).get_op_mut(opline.op1 as u32, opline.op1_type)
                    };
                    let argument = materialize_reference_alias(frame, caller_value);
                    let call = (*frame).call;
                    debug_assert!(!call.is_null());
                    let arg_slot = (*call).cv_mut(opline.op2 as u32);
                    frame_slot_init(call, arg_slot as *mut Value, argument);
                }
            }

            OpCode::SendVarEx => {
                // Runtime-checked send: by-ref if callee expects it AND op1 is CV, else by-val
                // op2 = CV slot in callee, extended_value = parameter index for ref_args check
                if opline._pad & SEND_FLAG_INDIRECT_TEMPORARY != 0
                    && let Some(flow) = op_send_indirect_temporary_reference(
                        eg, frame, op_array, opline, opline_ptr,
                    )?
                {
                    match flow {
                        ColdResult::Continue => continue 'vm,
                        ColdResult::NewFrame(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue 'vm;
                        }
                        ColdResult::Unhandled(thrown) => {
                            eg.exception = Some(thrown);
                            return Ok(());
                        }
                        _ => unreachable!("indirect reference send returned invalid control flow"),
                    }
                }
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let param_idx = opline.extended_value;
                // SAFETY: call is the live pending frame. Closure's explicit
                // forwarding methods mark their variadic wrapper parameters
                // prefer-reference so writable arguments can retain aliases;
                // consult the wrapped closure signature before materializing
                // one for an ordinary by-value parameter.
                let is_ref = unsafe {
                    let func_common = &*(*call).func;
                    let forwarded_index = if func_common.fn_type == FunctionType::Internal
                        && func_common.sig.this_offset == 1
                        && func_common.sig.ref_args == func_common.sig.prefer_ref_args
                    {
                        if func_common.sig.prefer_ref_args == u64::MAX {
                            Some(param_idx)
                        } else if func_common.sig.prefer_ref_args == u64::MAX << 1
                            && param_idx > 0
                        {
                            Some(param_idx - 1)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(target_index) = forwarded_index {
                        let receiver = (*call).cv(0).dereferenced();
                        receiver.as_closure().is_some_and(|closure| {
                            let signature = &(*closure.func).sig;
                            let reference_index =
                                if target_index < signature.public_arity() {
                                    target_index
                                } else if signature.is_variadic {
                                    signature.public_arity()
                                } else {
                                    target_index
                                };
                            signature.is_param_by_ref(reference_index)
                        })
                    } else {
                        func_common.sig.is_param_by_ref(param_idx)
                    }
                };

                let yield_snapshot =
                    opline._pad & crate::vm::instruction::SEND_FLAG_YIELD_SNAPSHOT != 0;
                if is_ref && (opline.op1_type == OpType::Cv || yield_snapshot) {
                    // Same logic as SendRef
                    let argument = unsafe {
                        let base = (frame as *mut Value).add(CALL_FRAME_SLOTS);
                        let source_cv = if yield_snapshot {
                            debug_assert_eq!(opline.result_type, OpType::Unused);
                            opline.result
                        } else {
                            opline.op1
                        };
                        let raw_ptr = base.add(source_cv as usize);
                        materialize_reference_alias(frame, raw_ptr)
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    unsafe { frame_slot_init(call, arg_slot as *mut Value, argument) };
                } else {
                    // Same logic as SendVal
                    let source = unsafe {
                        (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                    };
                    let arg_slot = unsafe { (*call).cv_mut(opline.op2 as u32) };
                    if opline._pad & crate::vm::instruction::SEND_FLAG_FETCH_CV_R != 0
                        && unsafe { (*source).is_undef() }
                    {
                        let snapshot = snapshot_runtime_send_rvalue(
                            eg, frame, op_array, opline,
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            unsafe { cleanup_pending_calls(eg, frame) };
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                        unsafe { frame_slot_init(call, arg_slot as *mut Value, snapshot) };
                    } else if !unsafe {
                        try_init_borrowed_heap_arg(
                            call,
                            param_idx,
                            source,
                            arg_slot as *mut Value,
                        )
                    } {
                        // SAFETY: source belongs to the live caller frame and
                        // remains valid until this value is cloned below.
                        let cloned = if unsafe { (*source).is_undef() } {
                            Value::null()
                        } else {
                            unsafe { (&*source).clone() }
                        };
                        unsafe { frame_slot_init(call, arg_slot as *mut Value, cloned) };
                    }
                }
            }

            OpCode::SendUser => {
                // The compiler emits this only for a literal callback whose
                // immutable declaration has no by-reference parameters.
                let call = unsafe { (*frame).call };
                debug_assert!(!call.is_null());
                let func_common = unsafe { &*(*call).func };
                let destination_index = func_common
                    .sig
                    .param_cv_index(opline.extended_value);
                let value = unsafe {
                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                };
                // call_user_func forwards ordinary arguments by value. Follow
                // an existing reference for the read, but do not create a new
                // reference merely because the callback parameter is by-ref.
                let value = if value.is_reference() {
                    unsafe { &*value.as_ref_ptr() }
                } else {
                    value
                };
                let destination = unsafe { (*call).cv_mut(destination_index) };
                let value = if value.is_undef() {
                    Value::null()
                } else {
                    value.clone()
                };
                // SAFETY: destination_index was derived from the selected
                // callback signature and this pending slot is uninitialized.
                unsafe { frame_slot_init(call, destination as *mut Value, value) };
            }

            OpCode::SendUserChecked => {
                match op_send_user_checked(eg, frame, op_array, opline, opline_ptr)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(thrown) => {
                        eg.exception = Some(thrown);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::SendNamed => {
                match op_send_named(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::DoFcall => {
                // Execute the pending call
                let mut call;
                // Restore the caller's previous pending call, then detach the
                // activation before it becomes an executing frame. Leaving
                // that predecessor in the callee makes a later nested call
                // treat a caller-owned (and possibly already popped) frame as
                // its own pending call chain.
                // SAFETY: `frame` is the active VM frame and its pending-call
                // pointer names a live activation owned by one of the VM
                // stacks until this DoFcall path either enters or discards it.
                unsafe {
                    call = (*frame).call;
                    debug_assert!(!call.is_null());
                    let previous_pending_call = (*call).call;
                    (*frame).call = previous_pending_call;
                    (*call).call = std::ptr::null_mut();
                }

                // A non-contiguous pure-scalar call captured its arguments in a
                // compact activation. On success it never acquires body CVs or
                // TMPs; on any guard failure it becomes the ordinary ABI frame
                // and continues through the unchanged DoFcall implementation.
                // SAFETY: `call` is the live activation validated above;
                // deferred resolution either materializes that same call on
                // the main stack or consumes it and returns null.
                let resolved_deferred_call = unsafe {
                    (*call).is_deferred_scalar_call().then(|| {
                        resolve_deferred_scalar_call(eg, frame, call, opline, opline_ptr)
                    })
                };
                if let Some(resolved) = resolved_deferred_call {
                    call = resolved;
                    if call.is_null() {
                        continue 'vm;
                    }
                }
                let suppressed_call = opline._pad & CALL_FLAG_ERROR_SUPPRESS != 0;
                if suppressed_call {
                    eg.begin_error_suppression(call as usize);
                }
                // SAFETY: a non-null resolved pending activation always owns a
                // registered descriptor for the duration of DoFcall. A user
                // descriptor begins with FunctionCommon by the VM ABI.
                let (func_common_fast, user_callee_fast) = unsafe {
                    let common = &*(*call).func;
                    let user = (common.fn_type == FunctionType::User)
                        .then(|| &*((*call).func as *const UserFunction));
                    (common, user)
                };
                if func_common_fast.plan.has_deprecated_attribute() {
                    let reported = report_deprecated_user_call(
                        eg,
                        frame,
                        func_common_fast as *const FunctionCommon,
                        Some(call as usize),
                        None,
                    );
                    if let Err(error) = reported {
                        if suppressed_call {
                            eg.end_error_suppression(call as usize);
                        }
                        discard_pending_vm_call_frame(eg, call);
                        return Err(error);
                    }
                    if let Some(exception) = eg.exception.take() {
                        if suppressed_call {
                            eg.end_error_suppression(call as usize);
                        }
                        discard_pending_vm_call_frame(eg, call);
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                }
                if func_common_fast.plan.has_no_discard_attribute()
                    && opline.result_type == OpType::Unused
                    && opline._pad & CALL_FLAG_RETURN_EXPLICITLY_IGNORED == 0
                {
                    let reported = report_no_discard_user_call(
                        eg,
                        frame,
                        user_callee_fast.expect("NoDiscard call plan belongs to a user function"),
                        Some(call as usize),
                        None,
                    );
                    if let Err(error) = reported {
                        if suppressed_call {
                            eg.end_error_suppression(call as usize);
                        }
                        discard_pending_vm_call_frame(eg, call);
                        return Err(error);
                    }
                    if let Some(exception) = eg.exception.take() {
                        if suppressed_call {
                            eg.end_error_suppression(call as usize);
                        }
                        discard_pending_vm_call_frame(eg, call);
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                }
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                let generic_member_contract =
                    eg.take_pending_generic_member_call(call as usize);
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                if let Some(contract) = generic_member_contract.as_ref() {
                    validate_generic_member_arguments(eg, call, contract)?;
                }
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                let has_generic_member_contract = generic_member_contract.is_some();
                #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                let generic_member_contract: Option<
                    std::rc::Rc<crate::generics::GenericMethodContract>,
                > = None;
                #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                let has_generic_member_contract = false;

                // ── FastScalar path: tightest call protocol ──
                // Preconditions guaranteed at compile time: fixed arity, no by-ref,
                // no variadics, no generator, no globals, no type hints, no return type.
                // Runtime: only check fn_type + plan + no pending edge cases.
                // A compiler-proven pure binary recurrence can preserve the
                // PHP depth-first evaluation order with compact integer
                // activations. The already-created root frame supplies the
                // canonical argument ABI; all recursive descendants avoid it.
                if func_common_fast.fn_type == FunctionType::User
                    && !suppressed_call
                    && !has_generic_member_contract
                    && unsafe { (*call).num_args } == 1
                    && !unsafe { (*call).named_args_used }
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                    && eg.pending_closure_captures.is_empty()
                    && matches!(opline.result_type, OpType::Tmp | OpType::Var | OpType::Unused)
                {
                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    if let Some(plan) = user.binary_long_recursion_plan.as_ref() {
                        let method_dispatch_matches = if let Some(method_name) = &plan.method_name {
                            let receiver = unsafe { (*call).cv(0) };
                            if let Some(object) = receiver.as_object() {
                                let full_name = format!("{}::{}", object.class_name, method_name);
                                drop(object);
                                eg.find_function(&full_name)
                                    .is_some_and(|resolved| resolved == unsafe { (*call).func })
                            } else {
                                false
                            }
                        } else {
                            true
                        };
                        let argument_cv = func_common_fast.sig.param_cv_index(0);
                        let argument = unsafe { (*call).cv(argument_cv) };
                        if method_dispatch_matches
                            && argument.value_type() == ValueType::Long
                            && !argument.is_reference()
                        {
                            let evaluated = execute_binary_long_recursion(
                                eg,
                                plan,
                                unsafe { argument.raw_long() },
                            );
                            match evaluated {
                                Ok(Some(result)) => {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = func_common_fast.call_count.get();
                                    if count < u32::MAX {
                                        func_common_fast.call_count.set(count + 1);
                                    }
                                    if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                                        let result_ptr = unsafe {
                                            (frame as *mut Value)
                                                .add(CALL_FRAME_SLOTS + opline.result as usize)
                                        };
                                        unsafe { frame_tmp_set_long(frame, result_ptr, result) };
                                    }
                                    if unsafe { (*call).has_heap_slots } {
                                        unsafe { cleanup_frame_slots(call) };
                                    }
                                    pop_vm_call_frame(eg, call);
                                    unsafe { (*frame).opline = opline_ptr.add(1) };
                                    continue 'vm;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    if unsafe { (*call).has_heap_slots } {
                                        unsafe { cleanup_frame_slots(call) };
                                    }
                                    pop_vm_call_frame(eg, call);
                                    return Err(error);
                                }
                            }
                        }
                    }
                }

                // ── Fast path for fixed-signature internal functions ──
                // Internal handlers still receive their ordinary ExecuteData
                // frame, so this changes no stdlib ABI or argument ownership.
                // It only avoids the generic type/variadic/class validation
                // path when the constructor proved those features absent.
                if func_common_fast.fn_type == FunctionType::Internal
                    && !suppressed_call
                    && func_common_fast.plan.call == CallStrategy::Fast
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                    && eg.pending_closure_captures.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    // SAFETY: the FunctionType guard proves that the live
                    // descriptor has the repr(C) InternalFunction tail.
                    let internal = unsafe {
                        &*((*call).func as *const super::function::InternalFunction)
                    };
                    let raw_variadic_handler = (func_common_fast.sig.is_variadic
                        && num_args_fast
                            <= func_common_fast.sig.public_arity().saturating_add(1))
                    .then_some(internal.raw_variadic_handler)
                    .flatten();
                    let arity_ok = num_args_fast >= func_common_fast.sig.required_num_args
                        && if func_common_fast.sig.is_variadic {
                            raw_variadic_handler.is_some()
                        } else {
                            num_args_fast <= func_common_fast.sig.public_arity()
                        };
                    let required_args_present = !unsafe { (*call).named_args_used } || {
                        let mut all_present = true;
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                all_present = false;
                                break;
                            }
                        }
                        all_present
                    };

                    if arity_ok && required_args_present {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe {
                                (*frame).get_op_mut(opline.result as u32, opline.result_type)
                            },
                        };
                        unsafe { (*call).return_value = return_value_ptr };

                        if !return_value_ptr.is_null() {
                            unsafe {
                                frame_result_prepare_external_write(
                                    frame,
                                    return_value_ptr,
                                    opline.result_type,
                                )
                            };
                        }
                        let handler_result = if let Some(handler) = raw_variadic_handler {
                            handler(call, return_value_ptr, eg, num_args_fast)
                        } else {
                            (internal.handler)(call, return_value_ptr, eg)
                        };
                        if !return_value_ptr.is_null() {
                            unsafe {
                                frame_result_finish_external_write(
                                    frame,
                                    return_value_ptr,
                                    opline.result_type,
                                )
                            };
                        }
                        let internal_exception = eg.exception.take();
                        if let Some(exception) = internal_exception.as_ref() {
                            attach_internal_call_trace_if_missing(exception, call, frame, eg);
                        }
                        if internal_exception.is_none() && handler_result.is_ok() {
                            complete_object_construction(eg, call);
                        }
                        unsafe { cleanup_frame_slots(call) };
                        pop_vm_call_frame(eg, call);

                        if let Some(exc) = internal_exception {
                            match throw_in_frame(eg, frame, exc)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(thrown) => {
                                    eg.exception = Some(thrown);
                                    return Ok(());
                                }
                            }
                        }
                        if let Err(e) = handler_result {
                            return Err(e);
                        }

                        unsafe { (*frame).opline = opline_ptr.add(1) };
                        continue 'vm;
                    }
                }

                if func_common_fast.fn_type == FunctionType::User
                    && !suppressed_call
                    && !has_generic_member_contract
                    && (func_common_fast.plan.call == CallStrategy::FastScalar
                        || (func_common_fast.plan.call == CallStrategy::FastTypedScalar
                            && opline._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0))
                    // Both scalar ABIs are fixed-arity here; the typed variant
                    // enters only when the compiler proved every supplied
                    // argument. The required public count is therefore the
                    // exact arity, excluding the hidden method `$this`.
                    && unsafe { (*call).num_args } == func_common_fast.sig.required_num_args
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                    && eg.pending_closure_captures.is_empty()
                {
                    let has_hole = unsafe { (*call).named_args_used } && {
                        let mut hole = false;
                        for i in 0..func_common_fast.sig.public_arity() {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            if unsafe { (*(*call).cv(cv_idx)).is_undef() } {
                                hole = true;
                                break;
                            }
                        }
                        hole
                    };
                    if !has_hole {
                    stats::inc_do_fcall_fast();

                    // Function-level hotness tracking.
                    // Promotion uses can_promote_to_hot() — single source of truth.
                    let cc = func_common_fast.call_count.get();
                    if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                    if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                        if func_common_fast.can_promote_to_hot() {
                            func_common_fast.hot_status.set(HotStatus::Hot);
                        }
                    }

                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let return_value_ptr = match opline.result_type {
                        OpType::Tmp | OpType::Var => unsafe {
                            (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                        },
                        OpType::Unused => std::ptr::null_mut(),
                        _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                    };
                    unsafe { (*call).return_value = return_value_ptr };
                    unsafe {
                        (*call).opline = user.op_array.instructions.as_ptr();
                        (*frame).opline = opline_ptr.add(1);
                    }
                    eg.current_execute_data.set(call);

                    // Hot executor dispatch: if callee is hot, run in specialized executor.
                    // On Completed: callee returned, frame popped — restore caller state.
                    // On Bailout: callee still active — switch to it in baseline loop.
                    if func_common_fast.hot_status.get() == HotStatus::Hot {
                        match super::hot::execute_hot_frame(eg, call)? {
                            super::hot::HotResult::Completed => {
                                // Callee done. eg.current_execute_data is our caller (frame).
                                // (*frame).opline was already set to DoFcall+1 above.
                                // op_array unchanged (same caller function).
                                continue 'vm;
                            }
                            super::hot::HotResult::Bailout => {
                                match super::hot::resume_after_long_comparison(eg, call)? {
                                    super::hot::HotResult::Completed => continue,
                                    super::hot::HotResult::Bailout => {
                                        func_common_fast.hot_status.set(HotStatus::Cold);
                                        // Callee bailed. It's the active frame with opline at bailout point.
                                        frame = eg.current_execute_data.get();
                                        op_array = unsafe { (*frame).op_array() };
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        // Cold function — baseline interpreter
                        frame = call;
                        op_array = unsafe { (*frame).op_array() };
                        continue;
                    }
                    }
                }

                // ── Fast path for simple user function calls ──
                if func_common_fast.fn_type == FunctionType::User
                    && !suppressed_call
                    && !has_generic_member_contract
                    && matches!(
                        func_common_fast.plan.call,
                        CallStrategy::Fast | CallStrategy::FastTypedScalar
                    )
                    && eg.pending_invoke_this.is_none()
                    && eg.pending_named_variadic.is_empty()
                    && eg.pending_closure_captures.is_empty()
                {
                    let num_args_fast = unsafe { (*call).num_args };
                    let user = unsafe { &*((*call).func as *const UserFunction) };
                    let mut has_required_holes = false;
                    if func_common_fast.sig.required_num_args > 0 {
                        for i in 0..func_common_fast.sig.required_num_args {
                            let cv_idx = func_common_fast.sig.param_cv_index(i);
                            let val = unsafe { &*(*call).cv(cv_idx) };
                            if val.is_undef() {
                                has_required_holes = true;
                                break;
                            }
                        }
                    }
                    if !user.op_array.is_generator
                        && !has_required_holes
                        && num_args_fast >= func_common_fast.sig.required_num_args
                        && num_args_fast <= func_common_fast.sig.public_arity()
                    {
                        let caller_strict = op_array.strict_types;
                        let type_ok = opline._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
                            || unsafe {
                                compact_scalar_call_types_match(
                                    eg,
                                    call,
                                    func_common_fast,
                                    caller_strict,
                                )
                            };
                        if !type_ok {
                            // Fall through to full path for proper TypeError
                        } else {
                        stats::inc_do_fcall_fast();
                        let return_value_ptr = match opline.result_type {
                            OpType::Tmp | OpType::Var => unsafe {
                                (frame as *mut Value).add(CALL_FRAME_SLOTS + opline.result as usize)
                            },
                            OpType::Unused => std::ptr::null_mut(),
                            _ => unsafe { (*frame).get_op_mut(opline.result as u32, opline.result_type) },
                        };
                        unsafe { (*call).return_value = return_value_ptr };
                        if user.op_array.may_access_globals
                            && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                        {
                            let vars_to_sync = if !op_array.main_scope_vars.is_empty() {
                                &op_array.main_scope_vars
                            } else {
                                &op_array.global_vars
                            };
                            for (cv_idx, var_name) in vars_to_sync {
                                let cv_ptr = unsafe { (*frame).get_op_mut(*cv_idx, OpType::Cv) };
                                let val = unsafe { (*cv_ptr).clone() };
                                globals_set(&mut eg.globals, var_name, val);
                            }
                        }
                        unsafe {
                            (*call).opline = user.op_array.instructions.as_ptr();
                            (*frame).opline = opline_ptr.add(1);
                        }
                        // Function-level hotness tracking.
                        // Promotion uses can_promote_to_hot() — single source of truth.
                        let cc = func_common_fast.call_count.get();
                        if cc < u32::MAX { func_common_fast.call_count.set(cc + 1); }
                        if cc == FUNC_HOT_THRESHOLD && func_common_fast.hot_status.get() == HotStatus::Cold {
                            if func_common_fast.can_promote_to_hot() {
                                func_common_fast.hot_status.set(HotStatus::Hot);
                            }
                        }

                        eg.current_execute_data.set(call);

                        // Hot executor dispatch: Hot status implies eligible (promotion guard above).
                        if func_common_fast.hot_status.get() == HotStatus::Hot {
                            match super::hot::execute_hot_frame(eg, call)? {
                                super::hot::HotResult::Completed => {
                                    continue;
                                }
                                super::hot::HotResult::Bailout => {
                                    match super::hot::resume_after_long_comparison(eg, call)? {
                                        super::hot::HotResult::Completed => continue,
                                        super::hot::HotResult::Bailout => {
                                            func_common_fast.hot_status.set(HotStatus::Cold);
                                            frame = eg.current_execute_data.get();
                                            op_array = unsafe { (*frame).op_array() };
                                            continue;
                                        }
                                    }
                                }
                            }
                        } else {
                            frame = call;
                            op_array = unsafe { (*frame).op_array() };
                            continue 'vm;
                        }
                    } // else: type_ok
                    } // if arity/generator ok
                }

                // ── Full path (handles all edge cases) ──
                match execute_full_call(
                    eg,
                    frame,
                    op_array,
                    opline,
                    opline_ptr,
                    call,
                    generic_member_contract,
                )? {
                    ColdResult::Done => {
                        unsafe { (*frame).opline = opline_ptr.add(1) };
                        continue 'vm;
                    }
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    ColdResult::Continue => continue 'vm,
                    ColdResult::Return => return Ok(()),
                }
            }

            OpCode::PreInc => {
                // ++$var: increment the already-evaluated read snapshot and
                // publish it to the destination CV. A compiler-emitted
                // TMP/VAR operand without an op2 CV is the value-only form
                // used before property or dimension writeback.
                // Maybe-undefined CV reads use a TMP snapshot so a re-entrant
                // handler cannot replace the value consumed by this operation.
                // SAFETY: the compiler resolves the read, optional result and
                // optional writeback CV into initialized slots of this active
                // frame.
                unsafe {
                    let old_slot = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    );
                    let value_only = opline.op2_type == OpType::Unused
                        && matches!(opline.op1_type, OpType::Tmp | OpType::Var);
                    debug_assert!(
                        value_only
                            || opline.op2_type == OpType::Cv
                            || opline.op1_type == OpType::Cv
                    );
                    let old_slot = if value_only {
                        old_slot.dereferenced()
                    } else {
                        old_slot
                    };
                    if value_only
                        && let Some(number) = old_slot.as_long()
                        && let Some(incremented) = number.checked_add(1)
                    {
                        if opline.result_type != OpType::Unused {
                            let result_ptr =
                                (*frame).get_op_mut(opline.result as u32, opline.result_type);
                            if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                                frame_tmp_set_long(frame, result_ptr, incremented);
                            } else {
                                slot_set(result_ptr, Value::long(incremented));
                            }
                        }
                        (*frame).opline = opline_ptr.add(1);
                        continue 'vm;
                    }
                    let old = if old_slot.is_undef() {
                        Value::null()
                    } else {
                        old_slot.clone()
                    };
                    let writeback_cv = if value_only {
                        None
                    } else if opline.op2_type == OpType::Cv {
                        Some(opline.op2 as u32)
                    } else {
                        debug_assert_eq!(opline.op1_type, OpType::Cv);
                        Some(opline.op1 as u32)
                    };
                    reject_reference_incdec_overflow!(
                        writeback_cv,
                        &old,
                        PropertyIncDecOverflow::Increment
                    );
                    let Some((new_val, diagnostic)) = increment_php_value(&old) else {
                        throw_operator!(
                            "TypeError",
                            &format!("Cannot increment {}", old.diagnostic_type_name())
                        );
                    };
                    if let Some(diagnostic) = diagnostic {
                        report_incdec_diagnostic(eg, frame, op_array, opline, diagnostic)?;
                        if let Some(writeback_cv) = writeback_cv {
                            restore_incdec_snapshot_on_exception!(writeback_cv, old);
                        }
                        resume_pending_exception!();
                    }
                    let new_val = if let Some(writeback_cv) = writeback_cv {
                        prepare_reference_write!(writeback_cv, new_val)
                    } else {
                        new_val
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr =
                            (*frame).get_op_mut(opline.result as u32, opline.result_type);
                        if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                            frame_tmp_set(frame, result_ptr, new_val.clone());
                        } else {
                            slot_set(result_ptr, new_val.clone());
                        }
                    }
                    if let Some(writeback_cv) = writeback_cv {
                        let cv_ptr = (*frame).get_op_mut(writeback_cv, OpType::Cv);
                        slot_set(cv_ptr, new_val);
                    }
                }
            }

            OpCode::PreDec => {
                // SAFETY: the compiler resolves the read, optional result and
                // optional writeback CV into initialized slots of this active
                // frame. TMP/VAR without an op2 CV is the value-only form used
                // before property or dimension writeback.
                unsafe {
                    let old_slot = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    );
                    let value_only = opline.op2_type == OpType::Unused
                        && matches!(opline.op1_type, OpType::Tmp | OpType::Var);
                    debug_assert!(
                        value_only
                            || opline.op2_type == OpType::Cv
                            || opline.op1_type == OpType::Cv
                    );
                    let old_slot = if value_only {
                        old_slot.dereferenced()
                    } else {
                        old_slot
                    };
                    if value_only
                        && let Some(number) = old_slot.as_long()
                        && let Some(decremented) = number.checked_sub(1)
                    {
                        if opline.result_type != OpType::Unused {
                            let result_ptr =
                                (*frame).get_op_mut(opline.result as u32, opline.result_type);
                            if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                                frame_tmp_set_long(frame, result_ptr, decremented);
                            } else {
                                slot_set(result_ptr, Value::long(decremented));
                            }
                        }
                        (*frame).opline = opline_ptr.add(1);
                        continue 'vm;
                    }
                    let old = if old_slot.is_undef() {
                        Value::null()
                    } else {
                        old_slot.clone()
                    };
                    let writeback_cv = if value_only {
                        None
                    } else if opline.op2_type == OpType::Cv {
                        Some(opline.op2 as u32)
                    } else {
                        debug_assert_eq!(opline.op1_type, OpType::Cv);
                        Some(opline.op1 as u32)
                    };
                    reject_reference_incdec_overflow!(
                        writeback_cv,
                        &old,
                        PropertyIncDecOverflow::Decrement
                    );
                    let Some((new_val, diagnostic)) = decrement_php_value(&old) else {
                        throw_operator!(
                            "TypeError",
                            &format!("Cannot decrement {}", old.diagnostic_type_name())
                        );
                    };
                    if let Some(diagnostic) = diagnostic {
                        report_incdec_diagnostic(eg, frame, op_array, opline, diagnostic)?;
                        if let Some(writeback_cv) = writeback_cv {
                            restore_incdec_snapshot_on_exception!(writeback_cv, old);
                        }
                        resume_pending_exception!();
                    }
                    let new_val = if let Some(writeback_cv) = writeback_cv {
                        prepare_reference_write!(writeback_cv, new_val)
                    } else {
                        new_val
                    };
                    if opline.result_type != OpType::Unused {
                        let result_ptr =
                            (*frame).get_op_mut(opline.result as u32, opline.result_type);
                        if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                            frame_tmp_set(frame, result_ptr, new_val.clone());
                        } else {
                            slot_set(result_ptr, new_val.clone());
                        }
                    }
                    if let Some(writeback_cv) = writeback_cv {
                        let cv_ptr = (*frame).get_op_mut(writeback_cv, OpType::Cv);
                        slot_set(cv_ptr, new_val);
                    }
                }
            }

            OpCode::PostInc => {
                // $var++: increment CV in place, result = old value
                // SAFETY: the compiler resolves the read, optional result and
                // writeback CV into initialized slots of this active frame.
                unsafe {
                    let old = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    );
                    let old = if old.is_undef() {
                        Value::null()
                    } else {
                        old.clone()
                    };
                    let writeback_cv = if opline.op2_type == OpType::Cv {
                        opline.op2 as u32
                    } else {
                        debug_assert_eq!(opline.op1_type, OpType::Cv);
                        opline.op1 as u32
                    };
                    reject_reference_incdec_overflow!(
                        Some(writeback_cv),
                        &old,
                        PropertyIncDecOverflow::Increment
                    );
                    let Some((new_val, diagnostic)) = increment_php_value(&old) else {
                        throw_operator!(
                            "TypeError",
                            &format!("Cannot increment {}", old.diagnostic_type_name())
                        );
                    };
                    if let Some(diagnostic) = diagnostic {
                        report_incdec_diagnostic(eg, frame, op_array, opline, diagnostic)?;
                        restore_incdec_snapshot_on_exception!(writeback_cv, old);
                        resume_pending_exception!();
                    }
                    let new_val = prepare_reference_write!(writeback_cv, new_val);
                    if opline.result_type != OpType::Unused {
                        let result_ptr =
                            (*frame).get_op_mut(opline.result as u32, opline.result_type);
                        if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                            frame_tmp_set(frame, result_ptr, old.clone());
                        } else {
                            slot_set(result_ptr, old.clone());
                        }
                    }
                    let cv_ptr = (*frame).get_op_mut(writeback_cv, OpType::Cv);
                    slot_set(cv_ptr, new_val);
                }
            }

            OpCode::PostDec => {
                // SAFETY: the compiler resolves the read, optional result and
                // writeback CV into initialized slots of this active frame.
                unsafe {
                    let old = &*(*frame).get_op_ptr(
                        opline.op1 as u32,
                        opline.op1_type,
                        op_array,
                    );
                    let old = if old.is_undef() {
                        Value::null()
                    } else {
                        old.clone()
                    };
                    let writeback_cv = if opline.op2_type == OpType::Cv {
                        opline.op2 as u32
                    } else {
                        debug_assert_eq!(opline.op1_type, OpType::Cv);
                        opline.op1 as u32
                    };
                    reject_reference_incdec_overflow!(
                        Some(writeback_cv),
                        &old,
                        PropertyIncDecOverflow::Decrement
                    );
                    let Some((new_val, diagnostic)) = decrement_php_value(&old) else {
                        throw_operator!(
                            "TypeError",
                            &format!("Cannot decrement {}", old.diagnostic_type_name())
                        );
                    };
                    if let Some(diagnostic) = diagnostic {
                        report_incdec_diagnostic(eg, frame, op_array, opline, diagnostic)?;
                        restore_incdec_snapshot_on_exception!(writeback_cv, old);
                        resume_pending_exception!();
                    }
                    let new_val = prepare_reference_write!(writeback_cv, new_val);
                    if opline.result_type != OpType::Unused {
                        let result_ptr =
                            (*frame).get_op_mut(opline.result as u32, opline.result_type);
                        if matches!(opline.result_type, OpType::Tmp | OpType::Var) {
                            frame_tmp_set(frame, result_ptr, old.clone());
                        } else {
                            slot_set(result_ptr, old.clone());
                        }
                    }
                    let cv_ptr = (*frame).get_op_mut(writeback_cv, OpType::Cv);
                    slot_set(cv_ptr, new_val);
                }
            }

            OpCode::InitArray => {
                let capacity = opline.extended_value as usize;
                let array = match opline._pad {
                    0 => PhpArray::with_packed_capacity(capacity),
                    ARRAY_INIT_HASH_HINT => PhpArray::with_hash_capacity(capacity),
                    flags => {
                        if flags & ARRAY_INIT_DYNAMIC_CALL_CLASS != 0 {
                            // SAFETY: the compiler stores the already-evaluated
                            // class operand in this live frame and dispatch
                            // supplies an instruction from this op-array.
                            let (class_value, instruction_index) = unsafe {
                                (
                                    &*(*frame).get_op_ptr(
                                        opline.op1 as u32,
                                        opline.op1_type,
                                        op_array,
                                    ),
                                    (opline as *const Instruction)
                                        .offset_from(op_array.instructions.as_ptr())
                                        as usize,
                                )
                            };
                            let class_value = class_value.dereferenced();
                            if class_value.as_str().is_none() && class_value.as_object().is_none() {
                                match throw_invalid_dynamic_call_class(
                                    eg,
                                    frame,
                                    op_array,
                                    instruction_index,
                                )? {
                                    ThrowResult::Handled(new_frame, new_op_array) => {
                                        frame = new_frame;
                                        op_array = new_op_array;
                                        continue 'vm;
                                    }
                                    ThrowResult::Unhandled(exception) => {
                                        eg.exception = Some(exception);
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        if flags & ARRAY_INIT_HASH_HINT != 0 {
                            PhpArray::with_hash_capacity(capacity)
                        } else {
                            PhpArray::with_packed_capacity(capacity)
                        }
                    }
                };
                // SAFETY: InitArray's result is a compiler-owned TMP in this
                // live frame; frame_tmp_set records its heap ownership.
                unsafe {
                    let result_ptr = (*frame).get_op_mut(opline.result as u32, opline.result_type);
                    let mut value = Value::array(array);
                    if opline._pad & ARRAY_INIT_IMMUTABLE_LITERAL != 0 {
                        value.mark_immutable_array_literal();
                    }
                    frame_tmp_set(frame, result_ptr, value);
                }
            }

            OpCode::AddArrayElement => {
                // op1 = array TMP, op2 = value, result = key (or Unused for auto-key)
                if opline._pad & ARRAY_ELEMENT_REFERENCE != 0
                    && opline.op2_type != OpType::Cv
                {
                    return Err(VmError::Fatal(
                        "Reference array element source must be a variable".into(),
                    ));
                }
                // SAFETY: all operands are compiler-validated slots in the live
                // frame. The array TMP is exclusively mutated here; a reference
                // element aliases either a live CV or its owned reference cell.
                unsafe {
                    let mut cloned_val = if opline._pad & ARRAY_ELEMENT_REFERENCE != 0 {
                        let source = (*frame).cv_mut(opline.op2 as u32) as *mut Value;
                        if (*source).is_owned_reference() {
                            (*source).clone_owned_reference_alias()
                        } else if (*source).is_reference() {
                            Value::reference((*source).as_ref_ptr())
                        } else {
                            let current = reference_initial_value(std::mem::replace(
                                &mut *source,
                                Value::undef(),
                            ));
                            let binding = Value::owned_reference(current);
                            frame_slot_set(
                                frame,
                                source,
                                binding.clone_owned_reference_alias(),
                            );
                            binding
                        }
                    } else {
                        let val = &*(*frame).get_op_ptr(
                            opline.op2 as u32,
                            opline.op2_type,
                            op_array,
                        );
                        val.clone()
                    };
                    if opline._pad & ARRAY_ELEMENT_IMMUTABLE_CONTAINER != 0 {
                        cloned_val.demote_nested_immutable_array_owner();
                    }
                    let arr_ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                    let arr = &mut *arr_ptr;
                    let php_arr = arr.as_array_mut().ok_or_else(|| {
                        VmError::Fatal("AddArrayElement: operand is not an array".into())
                    })?;
                    if opline.result_type != OpType::Unused {
                        let key_val = &*(*frame).get_op_ptr(
                            opline.result as u32,
                            opline.result_type,
                            op_array,
                        );
                        match array_key_ref_or_throw!(
                            key_val,
                            &format!(
                                "Cannot access offset of type {} on array",
                                key_val.diagnostic_type_name()
                            ),
                            false
                        ) {
                            ArrayKeyRef::Int(key) => php_arr.set_int(key, cloned_val),
                            ArrayKeyRef::String(key) => {
                                if key_val.value_type() == ValueType::String {
                                    php_arr.set_str_value(key_val, cloned_val);
                                } else {
                                    php_arr.set_str(key, cloned_val);
                                }
                            }
                        }
                    } else {
                        php_arr.push(cloned_val);
                    }
                    if opline._pad & ARRAY_ELEMENT_FINAL_IMMUTABLE_LITERAL != 0 {
                        arr.mark_immutable_array_literal();
                    }
                }
            }

            OpCode::AddArrayUnpack => {
                match op_add_array_unpack(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AddCallArgument => {
                match op_add_call_argument(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AddCallUnpack => {
                match op_add_call_unpack(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchDimR => 'fetch_dim: {
                #[cfg(feature = "quick-loops")]
                if opline._pad & FETCH_DIM_ISSET == 0
                    && opline._pad & FETCH_DIM_MUTABLE == 0
                    && opline.extended_value != 0
                    && unsafe {
                        execute_quick_region_entry(eg, frame, op_array, opline)?
                    }
                {
                    continue 'vm;
                }

                // result = op1[op2]
                // SAFETY: each compiler-selected operand slot belongs to this
                // live frame. The cold diagnostic helper separately retains an
                // array before invoking synchronous user code.
                let idx_val = unsafe {
                    &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array)
                };
                let result_ptr = unsafe {
                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                };

                // SAFETY: FetchDimR operands are compiler-owned live-frame or
                // immutable literal slots. Keep only a raw read pointer across
                // the branch and reacquire it after diagnostic dispatch.
                let (mut arr_ptr, arr_is_false) = unsafe {
                    let ptr =
                        (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) as *mut Value;
                    let is_false = (&*ptr).dereferenced().value_type() == ValueType::False;
                    (ptr, is_false)
                };

                if opline._pad & FETCH_DIM_MUTABLE != 0 && arr_is_false {
                    // A false container is converted before PHP normalizes the
                    // current key. Publishing the empty array first lets a
                    // synchronous error handler observe or replace it.
                    let conversion = convert_false_array_location(
                        eg,
                        frame,
                        op_array,
                        opline,
                        FalseArrayLocation::Operand {
                            operand: opline.op1,
                            operand_type: opline.op1_type,
                        },
                        (opline.extended_value != 0)
                            .then_some(opline.extended_value as usize),
                    )?;
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    match conversion {
                        FalseArrayConversion::Jumped => continue 'vm,
                        FalseArrayConversion::Survived(None)
                        | FalseArrayConversion::Clobbered => {
                            // SAFETY: the callback is complete; resolve the
                            // compiler-owned operand again before reading it.
                            arr_ptr = unsafe {
                                (*frame).get_op_ptr(
                                    opline.op1 as u32,
                                    opline.op1_type,
                                    op_array,
                                ) as *mut Value
                            };
                        }
                        FalseArrayConversion::NotFalse
                        | FalseArrayConversion::Survived(Some(_)) => {
                            unreachable!("a prechecked false operand must convert")
                        }
                    }
                }

                // SAFETY: op1 still names a compiler-selected live-frame
                // operand after any synchronous conversion callback. Resolve
                // it again here instead of retaining the pre-callback borrow.
                let arr_val = unsafe { (&*arr_ptr).dereferenced() };

                if let Some(arr) = arr_val.as_array() {
                    if opline._pad & FETCH_DIM_ISSET != 0
                        && let Some(key) = idx_val.as_long()
                    {
                        write_fetch_dim_result(
                            frame,
                            result_ptr,
                            Value::bool(arr.get_int(key).is_some_and(|value| {
                                !matches!(value.value_type(), ValueType::Null | ValueType::Undef)
                            })),
                        );
                    } else {
                        match value_to_array_key_ref(idx_val) {
                            Ok(array_key) => {
                                let fetched = match &array_key {
                                    ArrayKeyRef::Int(key) => arr.get_int(*key),
                                    ArrayKeyRef::String(key) => {
                                        let cache_ip = unsafe {
                                            (opline as *const Instruction)
                                                .offset_from(op_array.instructions.as_ptr())
                                                as usize
                                        };
                                        unsafe {
                                            cached_string_array_value(op_array, cache_ip, arr, key)
                                        }
                                    }
                                };
                                let mutable_false = opline._pad & FETCH_DIM_MUTABLE != 0
                                    && fetched.is_some_and(|value| {
                                        value.dereferenced().value_type() == ValueType::False
                                    });
                                if mutable_false {
                                    let key = match array_key {
                                        ArrayKeyRef::Int(key) => ArrayKey::Int(key),
                                        ArrayKeyRef::String(key) => {
                                            ArrayKey::String(key.to_string())
                                        }
                                    };
                                    let conversion = convert_false_array_location(
                                        eg,
                                        frame,
                                        op_array,
                                        opline,
                                        FalseArrayLocation::Child {
                                            operand: opline.op1,
                                            operand_type: opline.op1_type,
                                            key,
                                        },
                                        (opline.extended_value != 0)
                                            .then_some(opline.extended_value as usize),
                                    )?;
                                    if let Some(exception) = eg.exception.take() {
                                        match throw_in_frame(eg, frame, exception)? {
                                            ThrowResult::Handled(new_frame, new_op_array) => {
                                                frame = new_frame;
                                                op_array = new_op_array;
                                                continue 'vm;
                                            }
                                            ThrowResult::Unhandled(exception) => {
                                                eg.exception = Some(exception);
                                                return Ok(());
                                            }
                                        }
                                    }
                                    match conversion {
                                        FalseArrayConversion::Survived(Some(converted)) => {
                                            write_fetch_dim_result(frame, result_ptr, converted);
                                            break 'fetch_dim;
                                        }
                                        FalseArrayConversion::Jumped => continue 'vm,
                                        FalseArrayConversion::Clobbered => {
                                            write_fetch_dim_result(
                                                frame,
                                                result_ptr,
                                                Value::null(),
                                            );
                                            break 'fetch_dim;
                                        }
                                        FalseArrayConversion::NotFalse
                                        | FalseArrayConversion::Survived(None) => {
                                            unreachable!(
                                                "a prechecked false child must convert or be clobbered"
                                            )
                                        }
                                    }
                                }
                                if fetched.is_none()
                                    && opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_SILENT) == 0
                                {
                                    let key = match array_key {
                                        ArrayKeyRef::Int(key) => key.to_string(),
                                        ArrayKeyRef::String(key) => format!("\"{key}\""),
                                    };
                                    report_php_warning(
                                        eg,
                                        frame,
                                        op_array,
                                        opline,
                                        &format!("Undefined array key {key}"),
                                        opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
                                    )?;
                                    if let Some(exception) = eg.exception.take() {
                                        match throw_in_frame(eg, frame, exception)? {
                                            ThrowResult::Handled(new_frame, new_op_array) => {
                                                frame = new_frame;
                                                op_array = new_op_array;
                                                continue 'vm;
                                            }
                                            ThrowResult::Unhandled(exception) => {
                                                eg.exception = Some(exception);
                                                return Ok(());
                                            }
                                        }
                                    }
                                }
                                let value = if opline._pad & FETCH_DIM_ISSET != 0 {
                                    Value::bool(fetched.is_some_and(|value| {
                                        !matches!(
                                            value.value_type(),
                                            ValueType::Null | ValueType::Undef
                                        )
                                    }))
                                } else {
                                    fetched.cloned().unwrap_or(Value::null())
                                };
                                write_fetch_dim_result(frame, result_ptr, value);
                            }
                            Err(error) => {
                                match fetch_dim_after_array_key_diagnostic(
                                    eg,
                                    frame,
                                    op_array,
                                    opline,
                                    result_ptr,
                                    idx_val.clone(),
                                    error,
                                )? {
                                    ColdResult::Done => {}
                                    ColdResult::NewFrame(new_frame, new_op_array) => {
                                        frame = new_frame;
                                        op_array = new_op_array;
                                        continue 'vm;
                                    }
                                    ColdResult::Unhandled(exception) => {
                                        eg.exception = Some(exception);
                                        return Ok(());
                                    }
                                    ColdResult::Continue | ColdResult::Return => {
                                        unreachable!(
                                            "array-key diagnostic cannot suspend execution"
                                        )
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(s) = arr_val.as_str() {
                    if opline._pad & FETCH_DIM_DESTRUCTURE != 0 {
                        write_fetch_dim_result(frame, result_ptr, Value::null());
                    } else {
                        // String offset access: $s[0] — PHP strings are byte-oriented
                        let bytes = s.as_bytes();
                        if let Some(idx) = idx_val.as_long() {
                        let pos = if idx >= 0 {
                            idx as usize
                        } else {
                            let len = bytes.len() as i64;
                            let p = len + idx;
                            if p >= 0 { p as usize } else { usize::MAX }
                        };
                        let val = if opline._pad & FETCH_DIM_ISSET != 0 {
                            Value::bool(pos < bytes.len())
                        } else if pos < bytes.len() {
                            // Single byte as a string
                            Value::string(String::from(bytes[pos] as char))
                        } else if opline._pad & FETCH_DIM_SILENT != 0 {
                            Value::string("")
                        } else {
                            report_php_warning(
                                eg,
                                frame,
                                op_array,
                                opline,
                                &format!("Uninitialized string offset {idx}"),
                                opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
                            )?;
                            if let Some(exception) = eg.exception.take() {
                                match throw_in_frame(eg, frame, exception)? {
                                    ThrowResult::Handled(new_frame, new_op_array) => {
                                        frame = new_frame;
                                        op_array = new_op_array;
                                        continue 'vm;
                                    }
                                    ThrowResult::Unhandled(exception) => {
                                        eg.exception = Some(exception);
                                        return Ok(());
                                    }
                                }
                            }
                            Value::string("")
                        };
                        write_fetch_dim_result(frame, result_ptr, val);
                        } else {
                            write_fetch_dim_result(
                                frame,
                                result_ptr,
                                if opline._pad & FETCH_DIM_ISSET != 0 {
                                    Value::bool(false)
                                } else {
                                    Value::null()
                                },
                            );
                        }
                    }
                } else if matches!(arr_val.value_type(), ValueType::Object | ValueType::Closure) {
                    let receiver = arr_val.clone();
                    let key = idx_val.clone();
                    let method = if opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_EMPTY) != 0 {
                        "offsetExists"
                    } else {
                        "offsetGet"
                    };
                    let suppressed = opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0;
                    if suppressed {
                        eg.begin_error_suppression(frame as usize);
                    }
                    let value = crate::stdlib::call_object_protocol_method(
                        eg,
                        &receiver,
                        "ArrayAccess",
                        method,
                        std::slice::from_ref(&key),
                    );
                    if suppressed {
                        eg.end_error_suppression(frame as usize);
                    }
                    let value = match value? {
                        Some(value) => value,
                        None => {
                            let instruction_index = (opline_ptr as usize
                                - op_array.instructions.as_ptr() as usize)
                                / std::mem::size_of::<Instruction>();
                            match throw_object_as_array(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &receiver,
                            )? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    };
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    let value = if opline._pad & FETCH_DIM_EMPTY != 0 && value.is_truthy() {
                        if suppressed {
                            eg.begin_error_suppression(frame as usize);
                        }
                        let fetched = crate::stdlib::call_object_protocol_method(
                            eg,
                            &receiver,
                            "ArrayAccess",
                            "offsetGet",
                            std::slice::from_ref(&key),
                        )?
                        .unwrap_or_else(Value::null);
                        if suppressed {
                            eg.end_error_suppression(frame as usize);
                        }
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                        fetched
                    } else if opline._pad & FETCH_DIM_EMPTY != 0 {
                        Value::null()
                    } else {
                        value
                    };
                    if opline._pad & FETCH_DIM_MUTABLE != 0 && !value.is_reference() {
                        let class_name = receiver
                            .as_object()
                            .map(|object| object.class_name.to_string())
                            .unwrap_or_else(|| "object".to_string());
                        report_php_notice(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Indirect modification of overloaded element of {class_name} has no effect"
                            ),
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    write_fetch_dim_result(frame, result_ptr, value.dereferenced().clone());
                } else {
                    if arr_val.value_type() == ValueType::Resource
                        && opline._pad & FETCH_DIM_DESTRUCTURE != 0
                    {
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            "Cannot use resource as array",
                            opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    } else if matches!(arr_val.value_type(), ValueType::Null | ValueType::Undef)
                        && opline._pad & FETCH_DIM_MUTABLE != 0
                        && opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_SILENT) == 0
                    {
                        let array_key = array_key_ref_or_throw!(
                            idx_val,
                            &format!(
                                "Cannot access offset of type {} on array",
                                idx_val.diagnostic_type_name()
                            ),
                            opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0
                        );
                        let key = match array_key {
                            ArrayKeyRef::Int(key) => key.to_string(),
                            ArrayKeyRef::String(key) => format!("\"{key}\""),
                        };
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!("Undefined array key {key}"),
                            opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    } else if arr_val.value_type() != ValueType::Undef
                        && opline._pad & FETCH_DIM_DESTRUCTURE == 0
                        && opline._pad & (FETCH_DIM_ISSET | FETCH_DIM_SILENT) == 0
                    {
                        report_php_warning(
                            eg,
                            frame,
                            op_array,
                            opline,
                            &format!(
                                "Trying to access array offset on {}",
                                arr_val.type_name()
                            ),
                            opline._pad & FETCH_DIM_ERROR_SUPPRESS != 0,
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    write_fetch_dim_result(
                        frame,
                        result_ptr,
                        if opline._pad & FETCH_DIM_ISSET != 0 {
                            Value::bool(false)
                        } else {
                            Value::null()
                        },
                    );
                }
            }

            OpCode::FetchGlobals
            | OpCode::FetchGlobal
            | OpCode::AssignGlobal
            | OpCode::UnsetGlobal
            | OpCode::BindGlobalRef
            | OpCode::AssignGlobalRef => {
                match op_global_dimension(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchDynamicVar
            | OpCode::AssignDynamicVar
            | OpCode::UnsetDynamicVar
            | OpCode::BindDynamicVarRef
            | OpCode::AssignDynamicVarRef
            | OpCode::BindDynamicGlobal => {
                match op_dynamic_variable(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AssignDim => 'assign_dim: {
                // op1[op2] = result (value source encoded in result/result_type)
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let mut cloned_val = if opline._pad & crate::vm::instruction::ASSIGN_DIM_REFERENCE != 0 {
                    if opline.result_type != OpType::Cv {
                        return Err(VmError::Fatal(
                            "Reference array assignment source must be a variable".into(),
                        ));
                    }
                    // SAFETY: the reference flag is emitted only for a source
                    // CV in this live frame. Materialization retains any heap
                    // payload and updates the frame cleanup bitmap atomically.
                    unsafe {
                        let source = (*frame).cv_mut(opline.result as u32) as *mut Value;
                        materialize_reference_alias(frame, source)
                    }
                } else {
                    let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                    val.clone()
                };
                // SAFETY: AssignDim op1 names a compiler-owned mutable slot in this
                // live frame. A PHP reference owns its target; if false-array
                // reporting runs, the pointer is reacquired before mutation.
                let (mut arr_ptr, arr_type) = unsafe {
                    let ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                    let ptr = if (&*ptr).is_reference() {
                        (&mut *ptr).as_ref_ptr()
                    } else {
                        ptr
                    };
                    (ptr, (&*ptr).value_type())
                };
                if opline._pad & ASSIGN_DIM_UNSET_REBUILD != 0
                    && !matches!(
                        arr_type,
                        ValueType::Array | ValueType::Object | ValueType::Closure
                    )
                {
                    if arr_type == ValueType::False {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            "Automatic conversion of false to array is deprecated",
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                        break 'assign_dim;
                    }
                    if matches!(arr_type, ValueType::Undef | ValueType::Null) {
                        break 'assign_dim;
                    }
                    let message = if arr_type == ValueType::String {
                        "Cannot use string offset as an array"
                    } else {
                        "Cannot unset offset in a non-array variable"
                    };
                    let instruction_index = (opline_ptr as usize
                        - op_array.instructions.as_ptr() as usize)
                        / std::mem::size_of::<Instruction>();
                    match throw_array_dimension_error(
                        eg,
                        frame,
                        op_array,
                        instruction_index,
                        message,
                    )? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue 'vm;
                        }
                        ThrowResult::Unhandled(exception) => {
                            eg.exception = Some(exception);
                            return Ok(());
                        }
                    }
                }
                if arr_type == ValueType::False {
                    let conversion = convert_false_array_location(
                        eg,
                        frame,
                        op_array,
                        opline,
                        FalseArrayLocation::Operand {
                            operand: opline.op1,
                            operand_type: opline.op1_type,
                        },
                        None,
                    )?;
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    match conversion {
                        FalseArrayConversion::Survived(None) => {}
                        FalseArrayConversion::Clobbered => break 'assign_dim,
                        FalseArrayConversion::NotFalse
                        | FalseArrayConversion::Survived(Some(_))
                        | FalseArrayConversion::Jumped => {
                            unreachable!("a prechecked false operand must convert")
                        }
                    }
                    // SAFETY: the diagnostic callback has returned and the
                    // identity check proved that the converted operand still
                    // owns the published array. Resolve its current slot anew.
                    arr_ptr = unsafe {
                        let ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                        if (&*ptr).is_reference() {
                            (&mut *ptr).as_ref_ptr()
                        } else {
                            ptr
                        }
                    };
                }
                // SAFETY: arr_ptr was resolved from the active frame after
                // any reentrant callback and remains owned by its operand.
                let arr = unsafe { &mut *arr_ptr };
                if matches!(arr.value_type(), ValueType::Object | ValueType::Closure) {
                    let receiver = arr.clone();
                    let args = [idx_val.clone(), cloned_val];
                    let handled = crate::stdlib::call_object_protocol_method(
                        eg,
                        &receiver,
                        "ArrayAccess",
                        "offsetSet",
                        &args,
                    )?;
                    if handled.is_none() {
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_object_as_array(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            &receiver,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    unsafe { (*frame).opline = opline_ptr.add(1) };
                    continue 'vm;
                }
                let key = array_key_owned_or_throw!(
                    idx_val,
                    &format!(
                        "Cannot access offset of type {} on array",
                        idx_val.diagnostic_type_name()
                    ),
                    false
                    ,
                    opline._pad & ASSIGN_DIM_KEY_ALREADY_NORMALIZED == 0
                );
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().set(key, cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    if let Some(element) = php_arr.get_key_mut(&key) {
                        if opline._pad & crate::vm::instruction::ASSIGN_DIM_REFERENCE != 0 {
                            // `=&` rebinds this array dimension itself. Writing
                            // through an existing reference would mutate its
                            // former target and leave the dimension attached
                            // to the wrong cell on the next foreach iteration.
                            *element = cloned_val;
                        } else {
                            cloned_val = prepare_constrained_write!(
                                element.reference_property_constraints(),
                                cloned_val
                            );
                            if opline._pad & crate::vm::instruction::ASSIGN_DIM_RESULT_VALUE != 0 {
                                debug_assert_eq!(opline.result_type, OpType::Tmp);
                                // SAFETY: the compiler emitted this TMP result for the
                                // current instruction in the live frame; both pointer
                                // lookup and replacement complete before dispatch advances.
                                let result = unsafe {
                                    (*frame).get_op_mut(opline.result as u32, opline.result_type)
                                };
                                unsafe { frame_slot_set(frame, result, cloned_val.clone()) };
                            }
                            assignment_slot_set(element, cloned_val);
                        }
                    } else {
                        php_arr.set(key, cloned_val);
                    }
                } else {
                    return Err(VmError::Fatal("Cannot use a scalar value as an array".into()));
                }
            }

            OpCode::ArrayPushOp => 'array_push: {
                // op1[] = op2
                let cloned_val = if opline._pad & crate::vm::instruction::ARRAY_ELEMENT_REFERENCE != 0 {
                    if opline.op2_type != OpType::Cv {
                        return Err(VmError::Fatal(
                            "Reference array append source must be a variable".into(),
                        ));
                    }
                    // SAFETY: the reference flag is emitted only for a source
                    // CV in this live frame. Materialization retains any heap
                    // payload and updates the frame cleanup bitmap atomically.
                    unsafe {
                        let source = (*frame).cv_mut(opline.op2 as u32) as *mut Value;
                        materialize_reference_alias(frame, source)
                    }
                } else {
                    let val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                    val.clone()
                };
                // SAFETY: ArrayPushOp op1 names a compiler-owned mutable slot in this
                // live frame. A PHP reference owns its target; if false-array
                // reporting runs, the pointer is reacquired before mutation.
                let (mut arr_ptr, arr_is_false) = unsafe {
                    let ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                    let ptr = if (&*ptr).is_reference() {
                        (&mut *ptr).as_ref_ptr()
                    } else {
                        ptr
                    };
                    (ptr, (&*ptr).value_type() == ValueType::False)
                };
                if arr_is_false {
                    let conversion = convert_false_array_location(
                        eg,
                        frame,
                        op_array,
                        opline,
                        FalseArrayLocation::Operand {
                            operand: opline.op1,
                            operand_type: opline.op1_type,
                        },
                        None,
                    )?;
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    match conversion {
                        FalseArrayConversion::Survived(None) => {}
                        FalseArrayConversion::Clobbered => break 'array_push,
                        FalseArrayConversion::NotFalse
                        | FalseArrayConversion::Survived(Some(_))
                        | FalseArrayConversion::Jumped => {
                            unreachable!("a prechecked false operand must convert")
                        }
                    }
                    // SAFETY: reacquire the compiler-owned slot after the
                    // callback; identity validation proved the conversion was
                    // not replaced by synchronous user code.
                    arr_ptr = unsafe {
                        let ptr = (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                        if (&*ptr).is_reference() {
                            (&mut *ptr).as_ref_ptr()
                        } else {
                            ptr
                        }
                    };
                }
                // SAFETY: arr_ptr was resolved from the active frame after
                // any reentrant callback and remains owned by its operand.
                let arr = unsafe { &mut *arr_ptr };
                if matches!(arr.value_type(), ValueType::Object | ValueType::Closure) {
                    let receiver = arr.clone();
                    let args = [Value::null(), cloned_val];
                    let handled = crate::stdlib::call_object_protocol_method(
                        eg,
                        &receiver,
                        "ArrayAccess",
                        "offsetSetAppend",
                        &args,
                    )?;
                    if handled.is_none() {
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_object_as_array(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            &receiver,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    unsafe { (*frame).opline = opline_ptr.add(1) };
                    continue 'vm;
                }
                // Auto-create array if variable is null/undef
                if arr.value_type() == ValueType::Null || arr.value_type() == ValueType::Undef {
                    unsafe { slot_set(arr_ptr, Value::array(PhpArray::new())) };
                    let arr = unsafe { &mut *arr_ptr };
                    arr.as_array_mut().unwrap().push(cloned_val);
                } else if let Some(php_arr) = arr.as_array_mut() {
                    if !php_arr.try_push(cloned_val) {
                        throw_operator!(
                            "Error",
                            "Cannot add element to the array as the next element is already occupied"
                        );
                    }
                } else {
                    return Err(VmError::Fatal("[] operator not supported for non-array".into()));
                }
            }

            OpCode::BindArrayAppendRef => 'bind_array_append: {
                let conversion = if operand_is_false(frame, opline.op1, opline.op1_type) {
                    convert_false_array_location(
                        eg,
                        frame,
                        op_array,
                        opline,
                        FalseArrayLocation::Operand {
                            operand: opline.op1,
                            operand_type: opline.op1_type,
                        },
                        None,
                    )?
                } else {
                    FalseArrayConversion::NotFalse
                };
                if !matches!(conversion, FalseArrayConversion::NotFalse) {
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    if matches!(conversion, FalseArrayConversion::Clobbered) {
                        break 'bind_array_append;
                    }
                }
                // SAFETY: both operands are compiler-allocated mutable slots
                // in the active frame. The owned reference cell is Rc-backed,
                // so array reallocations and frame teardown cannot invalidate
                // either alias.
                unsafe {
                    let mut array_ptr =
                        (*frame).get_op_mut(opline.op1 as u32, opline.op1_type);
                    if (&*array_ptr).is_reference() {
                        array_ptr = (&mut *array_ptr).as_ref_ptr();
                    }
                    let array_value = &mut *array_ptr;
                    debug_assert_eq!(opline.result_type, OpType::Cv);
                    // Reference assignment rebinds the CV itself. Following an
                    // existing reference here would replace the caller's value
                    // and leave this local bound to the old cell.
                    let target = (*frame).cv_mut(opline.result as u32) as *mut Value;
                    let mut binding = Value::owned_reference(Value::null());
                    if opline._pad & REFERENCE_RESULT_INTERNAL != 0 {
                        binding.mark_internal_reference_alias();
                    }
                    frame_slot_set(frame, target, binding);
                    if matches!(array_value.value_type(), ValueType::Object | ValueType::Closure) {
                        let receiver = array_value.clone();
                        let handled = crate::stdlib::call_object_protocol_method(
                            eg,
                            &receiver,
                            "ArrayAccess",
                            "offsetSetAppend",
                            &[Value::null(), Value::null()],
                        )?;
                        if handled.is_none() {
                            let instruction_index = (opline_ptr as usize
                                - op_array.instructions.as_ptr() as usize)
                                / std::mem::size_of::<Instruction>();
                            match throw_object_as_array(
                                eg,
                                frame,
                                op_array,
                                instruction_index,
                                &receiver,
                            )? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue 'vm;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    } else {
                        if matches!(array_value.value_type(), ValueType::Null | ValueType::Undef) {
                            slot_set(array_ptr, Value::array(PhpArray::new()));
                        }
                        let array = (&mut *array_ptr).as_array_mut().ok_or_else(|| {
                            VmError::Fatal("Cannot append a reference to a non-array".into())
                        })?;
                        array.push((*target).clone_owned_reference_alias());
                    }
                }
            }

            OpCode::UnsetDim => {
                // Remove key op2 from array op1
                let idx_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let arr_ptr = unsafe { (*frame).get_op_mut(opline.op1 as u32, opline.op1_type) };
                let arr = unsafe { &mut *arr_ptr };
                if matches!(arr.value_type(), ValueType::Object | ValueType::Closure) {
                    let receiver = arr.clone();
                    let key = idx_val.clone();
                    let handled = crate::stdlib::call_object_protocol_method(
                        eg,
                        &receiver,
                        "ArrayAccess",
                        "offsetUnset",
                        std::slice::from_ref(&key),
                    )?;
                    if handled.is_none() {
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_object_as_array(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            &receiver,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    unsafe { (*frame).opline = opline_ptr.add(1) };
                    continue 'vm;
                }
                let key = array_key_owned_or_throw!(
                    idx_val,
                    &format!(
                        "Cannot unset offset of type {} on array",
                        idx_val.diagnostic_type_name()
                    ),
                    false,
                    true
                );
                match arr.value_type() {
                    ValueType::Array => {
                        arr.as_array_mut().unwrap().remove(&key);
                    }
                    ValueType::Undef | ValueType::Null => {
                        // PHP silently ignores unset on undef/null
                    }
                    ValueType::False => {
                        report_php_deprecation(
                            eg,
                            frame,
                            op_array,
                            opline,
                            "Automatic conversion of false to array is deprecated",
                        )?;
                        if let Some(exception) = eg.exception.take() {
                            match throw_in_frame(eg, frame, exception)? {
                                ThrowResult::Handled(new_frame, new_op_array) => {
                                    frame = new_frame;
                                    op_array = new_op_array;
                                    continue;
                                }
                                ThrowResult::Unhandled(exception) => {
                                    eg.exception = Some(exception);
                                    return Ok(());
                                }
                            }
                        }
                    }
                    value_type => {
                        let message = if value_type == ValueType::String {
                            if opline._pad & UNSET_DIM_NESTED != 0 {
                                "Cannot use string offset as an array"
                            } else {
                                "Cannot unset string offsets"
                            }
                        } else {
                            "Cannot unset offset in a non-array variable"
                        };
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_array_dimension_error(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            message,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                }
            }

            OpCode::ForeachInit => {
                match op_foreach_init(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::ForeachNext => {
                match op_foreach_next::<true, false>(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::ForeachNextPlain => {
                match op_foreach_next::<false, false>(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::ForeachNextRef => {
                match op_foreach_next::<false, true>(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::ForeachWriteback => {
                op_foreach_writeback(frame, op_array, opline)?;
            }

            OpCode::Throw => {
                match op_throw(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::NewObj => {
                if opline._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE != 0
                    && unsafe {
                        try_execute_virtual_object_array_pipeline(
                            eg,
                            frame,
                            op_array,
                            opline_ptr,
                        )
                    }
                    .is_some()
                {
                    continue;
                }
                match op_new_obj(eg, frame, op_array, opline)? {
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::FetchObjR => {
                match try_cached_fetch_obj_r(frame, op_array, opline) {
                    CachedFetchObjResult::Miss => {
                        match op_fetch_obj_r_slow(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => {
                                frame = nf;
                                op_array = no;
                                continue;
                            }
                            ColdResult::Unhandled(exc) => {
                                eg.exception = Some(exc);
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                    CachedFetchObjResult::Complete => {}
                    CachedFetchObjResult::CompleteAndSkipNext => {
                        unsafe { (*frame).opline = opline_ptr.add(2) };
                        continue;
                    }
                }
            }

            OpCode::IssetObj => match op_isset_obj(eg, frame, op_array, opline)? {
                ColdResult::NewFrame(new_frame, new_op_array) => {
                    frame = new_frame;
                    op_array = new_op_array;
                    continue;
                }
                ColdResult::Unhandled(exception) => {
                    eg.exception = Some(exception);
                    return Ok(());
                }
                _ => {}
            },

            OpCode::UnsetObj => match op_unset_obj(eg, frame, op_array, opline)? {
                ColdResult::NewFrame(new_frame, new_op_array) => {
                    frame = new_frame;
                    op_array = new_op_array;
                    continue;
                }
                ColdResult::Unhandled(exception) => {
                    eg.exception = Some(exception);
                    return Ok(());
                }
                _ => {}
            },

            OpCode::BindObjPropRef => {
                match op_bind_obj_prop_ref(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::BindArrayDimRef => {
                match op_bind_array_dim_ref(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AssignObjProp => {
                // ── Cache-hit fast path for public, non-enum, non-readonly properties ──
                // SAFETY: the validated operand belongs to the live frame; the
                // borrowed value is consumed before the frame can advance.
                let obj_val = unsafe {
                    (&*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array))
                        .dereferenced()
                };
                if obj_val.value_type() == ValueType::Object {
                    // SAFETY: the object tag was checked above; `opline` and
                    // both operands belong to this live op-array/frame. A
                    // matching non-empty property cache proves its slot is
                    // valid before the raw undef read.
                    let (
                        obj_class_id,
                        ip,
                        property_flags,
                        dynamic_name_matches,
                        cached_slot_is_undef,
                    ) = unsafe {
                        let obj_class_id = obj_val.object_class_id_unchecked();
                        let ip = (opline as *const Instruction)
                            .offset_from(op_array.instructions.as_ptr())
                            as usize;
                        let ic = &op_array.cache[ip];
                        let property_flags = ic.property_flags();
                        let dynamic_name_matches = if opline.op2_type == OpType::Const {
                            true
                        } else {
                            let requested =
                            (&*(*frame).get_op_ptr(
                                opline.op2 as u32,
                                opline.op2_type,
                                op_array,
                            ))
                                .dereferenced()
                            .as_str();
                            requested.is_some_and(|requested| {
                                obj_val.as_object().is_some_and(|object| {
                                    object.property_name_at_slot(ic.property_slot())
                                        == Some(requested)
                                })
                            })
                        };
                        let cache_candidate = property_flags != 0
                            && ic.class_id == obj_class_id
                            && obj_class_id != 0
                            && dynamic_name_matches;
                        let cached_slot_is_undef = cache_candidate
                            && (&*obj_val
                                .object_property_slot_unchecked(ic.property_slot()))
                            .is_undef();
                        (
                            obj_class_id,
                            ip,
                            property_flags,
                            dynamic_name_matches,
                            cached_slot_is_undef,
                        )
                    };
                    let ic = &op_array.cache[ip];
                    let mut cache_matches = ic.class_id == obj_class_id
                        && obj_class_id != 0
                        && dynamic_name_matches;
                    // Lazy shells share the ordinary class/layout cache. Only
                    // still-undef slots need the cold sidecar guard; ordinary
                    // warmed writes retain the allocation-free cache hit.
                    if cached_slot_is_undef && eg.lazy_object_state(obj_val).is_some() {
                        cache_matches = false;
                    }
                    // flags == 3: read-safe + write-safe declared property slot.
                    if property_flags == 3 && cache_matches {
                        // SAFETY: the compiler-emitted source belongs to the
                        // live frame; a Reference target remains live through
                        // this non-reentrant cached assignment.
                        let val = unsafe {
                            let val = &*(*frame).get_op_ptr(
                                opline.result as u32,
                                opline.result_type,
                                op_array,
                            );
                            if val.is_reference() {
                                &*val.as_ref_ptr()
                            } else {
                                val
                            }
                        };
                        let mut cloned = if opline._pad & ASSIGN_PROP_MOVE_SOURCE != 0
                            && matches!(opline.result_type, OpType::Tmp | OpType::Var)
                        {
                            unsafe {
                                let source = (*frame)
                                    .get_op_mut(opline.result as u32, opline.result_type);
                                if (&*source).is_reference() {
                                    val.clone()
                                } else {
                                    frame_tmp_take!(frame, source)
                                }
                            }
                        } else {
                            val.clone()
                        };
                        unsafe {
                            let property = obj_val
                                .object_property_slot_unchecked(ic.property_slot())
                                as *mut Value;
                            cloned = prepare_constrained_write!(
                                (&*property).reference_property_constraints(),
                                cloned
                            );
                            let destructor = prepare_replaced_value_destructor(eg, &*property);
                            let destructor_ran = destructor.is_some();
                            assignment_slot_set(&mut *property, cloned);
                            run_prepared_value_destructor(eg, destructor)?;
                            if destructor_ran {
                                resume_pending_exception!();
                            }
                        };
                    } else if property_flags == 2
                        && cache_matches
                        && ic.typed_instance_property_tag()
                            == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT
                        && {
                            // SAFETY: as above, the source slot and any
                            // Reference target remain live for this opcode.
                            let source = unsafe {
                                let source = &*(*frame).get_op_ptr(
                                    opline.result as u32,
                                    opline.result_type,
                                    op_array,
                                );
                                if source.is_reference() {
                                    &*source.as_ref_ptr()
                                } else {
                                    source
                                }
                            };
                            if source.value_type() != ValueType::Long {
                                false
                            } else {
                                // SAFETY: class-id equality proves the cached
                                // declared slot belongs to this object.
                                unsafe {
                                    let property = obj_val
                                        .object_property_slot_unchecked(ic.property_slot())
                                        as *mut Value;
                                    let value = prepare_constrained_write!(
                                        (&*property).reference_property_constraints(),
                                        Value::long(source.raw_long())
                                    );
                                    assignment_slot_set(
                                        &mut *property,
                                        value,
                                    );
                                }
                                true
                            }
                        }
                    {
                    } else if cache_matches
                        && let Some(result) = try_assign_cached_typed_instance_property(
                            eg,
                            frame,
                            op_array,
                            opline,
                            obj_val,
                            obj_class_id,
                        )?
                    {
                        match result {
                            ColdResult::NewFrame(nf, no) => {
                                frame = nf;
                                op_array = no;
                                continue;
                            }
                            ColdResult::Unhandled(exc) => {
                                eg.exception = Some(exc);
                                return Ok(());
                            }
                            _ => {}
                        }
                    } else {
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        {
                            let generic_handled = if cache_matches {
                                if let Some(declaration) = ic.generic_property_declaration() {
                                    let name = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) }
                                        .as_str()
                                        .unwrap_or("");
                                    let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                                    eg.check_cached_generic_property_value(
                                        obj_val,
                                        name,
                                        val,
                                        declaration,
                                    )
                                    .map_err(VmError::Fatal)?;
                                    let mut cloned = val.clone();
                                    unsafe {
                                        let property = obj_val
                                            .object_property_slot_unchecked(ic.property_slot())
                                            as *mut Value;
                                        cloned = prepare_constrained_write!(
                                            (&*property).reference_property_constraints(),
                                            cloned
                                        );
                                        assignment_slot_set(&mut *property, cloned);
                                    };
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if !generic_handled {
                                match op_assign_obj_prop(eg, frame, op_array, opline)? {
                                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                                    _ => {}
                                }
                            }
                        }
                        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                        {
                            match op_assign_obj_prop(eg, frame, op_array, opline)? {
                                ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                                ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                                _ => {}
                            }
                        }
                    }
                } else {
                    match op_assign_obj_prop(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::AssignObjDim => {
                // $obj->prop[$key] = val
                let obj_ptr = unsafe { (*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                let key_val = unsafe { &*(*frame).get_op_ptr(opline.op2 as u32, opline.op2_type, op_array) };
                let val = unsafe { &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array) };
                let prop_name_val = &op_array.literals[opline.extended_value as usize];
                let prop_name = prop_name_val.as_str().unwrap_or("").to_string();
                let key = key_val.clone();
                let new_val = val.clone();

                let obj = unsafe { &*obj_ptr };
                let object_dimension = if let Some(php_obj) = obj.as_object() {
                    let caller_class = get_caller_class(frame, eg);
                    let receiver_in_scope = caller_class
                        .as_ref()
                        .is_some_and(|cc| eg.class_is_a(&php_obj.class_name, cc));
                    let effective_caller = if receiver_in_scope {
                        caller_class.as_deref()
                    } else {
                        None
                    };
                    let storage_key = crate::runtime::resolve_property_key(
                        eg,
                        &php_obj.class_name,
                        &prop_name,
                        effective_caller,
                    );
                    php_obj
                        .get_property(&storage_key)
                        .filter(|value| {
                            matches!(value.value_type(), ValueType::Object | ValueType::Closure)
                        })
                        .cloned()
                } else {
                    None
                };
                if let Some(receiver) = object_dimension {
                    let handled = crate::stdlib::call_object_protocol_method(
                        eg,
                        &receiver,
                        "ArrayAccess",
                        "offsetSet",
                        &[key, new_val],
                    )?;
                    if handled.is_none() {
                        let instruction_index = (opline_ptr as usize
                            - op_array.instructions.as_ptr() as usize)
                            / std::mem::size_of::<Instruction>();
                        match throw_object_as_array(
                            eg,
                            frame,
                            op_array,
                            instruction_index,
                            &receiver,
                        )? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    if let Some(exception) = eg.exception.take() {
                        match throw_in_frame(eg, frame, exception)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }
                    unsafe { (*frame).opline = opline_ptr.add(1) };
                    continue 'vm;
                }

                let arr_key = array_key_owned_or_throw!(
                    &key,
                    &format!(
                        "Cannot access offset of type {} on array",
                        key.diagnostic_type_name()
                    ),
                    false,
                    true
                );
                if let Some(mut php_obj) = obj.as_object_mut() {
                    let caller_class = get_caller_class(frame, eg);
                    let receiver_in_scope = caller_class.as_ref().map_or(false, |cc| {
                        eg.class_is_a(&php_obj.class_name, cc)
                    });
                    let effective_caller = if receiver_in_scope { caller_class.as_deref() } else { None };
                    let storage_key = crate::runtime::resolve_property_key(eg, &php_obj.class_name, &prop_name, effective_caller);
                    let object_class_name = php_obj.class_name.clone();
                    let property_definition = php_obj
                        .property_slot(&storage_key)
                        .and_then(|slot| eg.instance_property_definition(php_obj.class_id, slot));
                    let readonly_initialized = eg
                        .class_table
                        .get(php_obj.class_name.as_ref())
                        .is_some_and(|class| class.readonly_props.contains(&prop_name))
                        && php_obj
                            .get_property(&storage_key)
                            .is_some_and(|value| !value.is_undef());
                    if readonly_initialized {
                        drop(php_obj);
                        let error = make_error_value(
                            "Error",
                            &format!(
                                "Cannot indirectly modify readonly property {object_class_name}::${prop_name}"
                            ),
                        );
                        match throw_in_frame(eg, frame, error)? {
                            ThrowResult::Handled(new_frame, new_op_array) => {
                                frame = new_frame;
                                op_array = new_op_array;
                                continue 'vm;
                            }
                            ThrowResult::Unhandled(exception) => {
                                eg.exception = Some(exception);
                                return Ok(());
                            }
                        }
                    }

                    if let Some(arr_val) = php_obj.get_property_mut(&storage_key) {
                        // Property exists — mutate the array in place
                        if let Some(arr) = arr_val.as_array_mut() {
                            arr.set(arr_key, new_val);
                        } else if matches!(arr_val.value_type(), ValueType::Undef | ValueType::Null) {
                            // PHP materializes an array when a dimension is
                            // first assigned through an uninitialized typed
                            // `array` property (and through an untyped/null
                            // property). Validate the synthesized value
                            // against the declared contract before publishing.
                            let mut array = crate::value::PhpArray::new();
                            array.set(arr_key, new_val);
                            let mut initialized = Value::array(array);
                            if let Some(definition) = property_definition
                                && definition.is_typed()
                            {
                                initialized = prepare_property_assignment(
                                    initialized,
                                    definition,
                                    eg,
                                    op_array.strict_types,
                                    &object_class_name,
                                )
                                .map_err(VmError::Fatal)?;
                            }
                            *arr_val = initialized;
                        } else {
                            return Err(VmError::Fatal(format!(
                                "Cannot use object of type {} as array", object_class_name
                            )));
                        }
                    } else {
                        // Property doesn't exist — create new array
                        let mut new_arr = crate::value::PhpArray::new();
                        new_arr.set(arr_key, new_val);
                        php_obj.set_property(&storage_key, Value::array(new_arr));
                    }
                } else {
                    let error = make_error_value(
                        "Error",
                        &format!(
                            "Attempt to modify property \"{prop_name}\" on {}",
                            obj.dereferenced().type_name()
                        ),
                    );
                    match throw_in_frame(eg, frame, error)? {
                        ThrowResult::Handled(new_frame, new_op_array) => {
                            frame = new_frame;
                            op_array = new_op_array;
                            continue;
                        }
                        ThrowResult::Unhandled(exception) => {
                            eg.exception = Some(exception);
                            return Ok(());
                        }
                    }
                }
            }

            OpCode::InitMethodCall => {
                // ── Cache-hit fast path (inlined) ──
                // Most method calls hit the monomorphic inline cache.
                // Bypass the #[inline(never)] helper entirely on cache hit.
                let obj_val = unsafe { &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array) };
                if obj_val.value_type() == ValueType::Object {
                    let obj_class_id = unsafe { obj_val.object_class_id_unchecked() };
                    let ip = unsafe { (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize };
                    let ic = &op_array.cache[ip];
                    if !ic.func.is_null()
                        && ic.class_id == obj_class_id
                        && obj_class_id != 0
                        && method_return_dispatch_contract_matches(
                            opline,
                            unsafe { &*ic.func },
                        )
                    {
                        let func_ptr = ic.func;
                        let common = unsafe { &*func_ptr };
                        let needs_trait_class_scope = common.plan.needs_trait_class_scope();
                        let num_args = opline.extended_value;
                        let mut scalar_plan_eligible = false;
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        let linked_generic_long_proof =
                            ic.method_has_linked_generic_long_contract();
                        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                        let linked_generic_long_proof = false;

                        // A non-reifiable child's link-time Long contract is
                        // stable for the class ID already held by this IC. Try
                        // the exact scalar proof before materializing or
                        // cloning a runtime contract. A side exit continues to
                        // the canonical resolver and full boundary checks.
                        if !needs_trait_class_scope
                            && linked_generic_long_proof
                            && common.fn_type == FunctionType::User
                            && num_args == common.sig.public_arity()
                        {
                            let user = unsafe { &*(func_ptr as *const UserFunction) };
                            if let Some(plan) = user.scalar_long_plan.as_deref()
                                && let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                }
                            {
                                stats::inc_do_fcall_fast();
                                stats::inc_return_fast();
                                let count = common.call_count.get();
                                if count < u32::MAX {
                                    common.call_count.set(count + 1);
                                }
                                unsafe {
                                    complete_direct_scalar_long_call(frame, do_fcall_ptr, result);
                                }
                                continue 'vm;
                            }
                        }

                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        let generic_contract = if ic.method_has_generic_contract() {
                            let method_name = unsafe {
                                &*(*frame).get_op_ptr(
                                    opline.op2 as u32,
                                    opline.op2_type,
                                    op_array,
                                )
                            };
                            eg.generic_instance_method_contract(
                                obj_val,
                                method_name.as_str().unwrap_or(""),
                            )
                        } else {
                            None
                        };
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        let has_active_generic_contract = generic_contract.is_some();
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        let generic_long_fast_path = generic_contract
                            .as_deref()
                            .is_some_and(|contract| contract.admits_exact_long_call(num_args))
                            && !linked_generic_long_proof;
                        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                        let has_active_generic_contract = false;
                        #[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
                        let generic_long_fast_path = false;

                        // A substituted Long→Long contract is already guarded
                        // by the scalar plan's exact argument representation
                        // and checked Long result. Successful execution needs
                        // neither a frame nor pending/active sidecar state.
                        if !needs_trait_class_scope
                            && generic_long_fast_path
                            && common.fn_type == FunctionType::User
                            && num_args == common.sig.public_arity()
                        {
                            let user = unsafe { &*(func_ptr as *const UserFunction) };
                            if let Some(plan) = user.scalar_long_plan.as_deref() {
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                } {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = common.call_count.get();
                                    if count < u32::MAX {
                                        common.call_count.set(count + 1);
                                    }
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                        }

                        // Public monomorphic methods whose bodies do not use
                        // `$this` can consume adjacent scalar arguments through
                        // the same frame-free ABI as ordinary functions. The
                        // receiver/class cache above still provides normal PHP
                        // virtual-dispatch semantics.
                        if !needs_trait_class_scope
                            && !has_active_generic_contract
                            && common.fn_type == FunctionType::User
                            && num_args == common.sig.public_arity()
                        {
                            let user = unsafe { &*(func_ptr as *const UserFunction) };

                            // Method cache bits classify compiler-proven
                            // property bodies. Handle those before consulting
                            // the more general scalar planners so their common
                            // cache-hit path pays only the guards it needs.
                            if ic.method_has_long_property_plan() {
                                if let Some(plan) = user.long_property_plan.as_deref() {
                                    if opline._pad & CALL_FLAG_DEFERRED_SCALAR_CANDIDATE != 0
                                        && unsafe {
                                            try_execute_composed_long_property_call(
                                                frame,
                                                op_array,
                                                opline_ptr,
                                                obj_val,
                                                user,
                                                plan,
                                            )
                                        }
                                    {
                                        continue 'vm;
                                    }
                                    let sends = unsafe { opline_ptr.add(1) };
                                    let do_fcall_ptr = unsafe { sends.add(num_args as usize) };
                                    let do_fcall = unsafe { &*do_fcall_ptr };
                                    if plan.public_args as u32 == num_args
                                        && do_fcall.opcode == OpCode::DoFcall
                                        && do_fcall.result_type == OpType::Unused
                                        && unsafe {
                                            try_execute_long_property_method(
                                                frame,
                                                op_array,
                                                obj_val,
                                                sends,
                                                plan,
                                                user,
                                            )
                                        }
                                    {
                                        record_scalar_call(common);
                                        unsafe { (*frame).opline = do_fcall_ptr.add(1) };
                                        continue 'vm;
                                    }
                                }
                            }
                            if ic.method_has_property_getter_plan() {
                                if let Some(plan) = user.property_getter_plan.as_ref() {
                                    let do_fcall_ptr = unsafe { opline_ptr.add(1) };
                                    if unsafe {
                                        try_execute_direct_property_getter(
                                            frame,
                                            obj_val,
                                            do_fcall_ptr,
                                            user,
                                            plan,
                                        )
                                    } {
                                        continue 'vm;
                                    }
                                }
                            }

                            if let Some(plan) = user.object_array_plan.as_deref() {
                                if opline._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS != 0
                                    && unsafe {
                                        try_execute_direct_object_array_consumers(
                                            eg,
                                            frame,
                                            op_array,
                                            opline_ptr,
                                            obj_val,
                                            user,
                                            plan,
                                        )
                                    }
                                    .is_some()
                                {
                                    record_scalar_call(common);
                                    continue 'vm;
                                }
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_object_array_call(
                                        eg,
                                        frame,
                                        op_array,
                                        obj_val,
                                        opline_ptr.add(1),
                                        user,
                                        plan,
                                    )
                                } {
                                    record_scalar_call(common);
                                    unsafe {
                                        complete_direct_value_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }

                            if let Some(plan) = user.object_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_object_long_call(
                                        eg,
                                        frame,
                                        op_array,
                                        obj_val,
                                        opline_ptr.add(1),
                                        user,
                                        plan,
                                    )
                                } {
                                    record_scalar_call(common);
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }

                            scalar_plan_eligible =
                                scalar_plan_eligible
                                    || user.composed_scalar_long_plan.is_some()
                                    || user.long_property_plan.is_some()
                                    || user.scalar_double_plan.is_some();
                            if let Some(plan) = user.scalar_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_long_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                } {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = common.call_count.get();
                                    if count < u32::MAX {
                                        common.call_count.set(count + 1);
                                    }
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_composed_scalar_long_call(
                                        eg,
                                        frame,
                                        op_array,
                                        opline_ptr,
                                        func_ptr,
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.scalar_double_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_scalar_double_call(
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        plan,
                                    )
                                } {
                                    stats::inc_do_fcall_fast();
                                    stats::inc_return_fast();
                                    let count = common.call_count.get();
                                    if count < u32::MAX {
                                        common.call_count.set(count + 1);
                                    }
                                    unsafe {
                                        complete_direct_value_call(
                                            frame,
                                            do_fcall_ptr,
                                            Value::double(result),
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.composed_scalar_double_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_composed_scalar_double_call(
                                        eg,
                                        frame,
                                        op_array,
                                        opline_ptr.add(1),
                                        common,
                                        user,
                                        Some(obj_val),
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_value_call(
                                            frame,
                                            do_fcall_ptr,
                                            Value::double(result),
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                            if let Some(plan) = user.composed_scalar_long_plan.as_deref() {
                                scalar_plan_eligible = true;
                                if let Some((result, do_fcall_ptr)) = unsafe {
                                    try_execute_direct_composed_scalar_body_call(
                                        eg,
                                        frame,
                                        op_array,
                                        opline_ptr,
                                        func_ptr,
                                        user,
                                        plan,
                                    )
                                } {
                                    unsafe {
                                        complete_direct_scalar_long_call(
                                            frame,
                                            do_fcall_ptr,
                                            result,
                                        );
                                    }
                                    continue 'vm;
                                }
                            }
                        }

                        let pending_call = unsafe { (*frame).call };
                        let deferred = should_defer_scalar_call(
                            opline,
                            scalar_plan_eligible,
                        );
                        let call = if deferred {
                            eg.pending_call_stack.push_deferred_scalar_call(
                                func_ptr,
                                num_args + 1,
                                num_args,
                                frame,
                                pending_call,
                            )
                        } else {
                            eg.vm_stack.push_call_frame(
                                func_ptr,
                                num_args + 1,
                                num_args,
                                frame,
                                pending_call,
                            )
                        };
                        unsafe {
                            (*frame).call = call;
                            if common.plan.borrow_this() {
                                frame_set_borrowed_this(call, obj_val as *const Value);
                            } else {
                                frame_set_this(call, obj_val.clone());
                            }
                        }
                        initialize_trait_class_scope(
                            eg,
                            call,
                            func_ptr,
                            ic.method_trait_scope_class_id(),
                        );
                        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                        if let Some(contract) = generic_contract {
                            eg.push_pending_generic_member_call(call as usize, contract);
                        }

                        // Bind the contiguous scalar argument prefix while the
                        // new frame is hot in registers. Nested argument
                        // expressions naturally stop the fusion.
                        let bound = unsafe {
                            bind_contiguous_scalar_args(
                                frame,
                                call,
                                op_array,
                                opline_ptr.add(1),
                                num_args,
                                common.supports_scalar_long_plan(),
                            )
                        };

                        // When the whole scalar argument prefix was bound,
                        // fold the adjacent DoFcall into this cache-hit method
                        // setup. This removes one more baseline dispatch from
                        // the ordinary `$object->method(...)` protocol.
                        if !needs_trait_class_scope
                            && !has_active_generic_contract
                            && !common.plan.has_call_diagnostic_attribute()
                            && ic.method_fusion_eligible()
                            && bound == num_args as usize
                        {
                            let do_fcall_ptr = unsafe { opline_ptr.add(1 + bound) };
                            let do_fcall = unsafe { &*do_fcall_ptr };
                            let argument_contract_satisfied =
                                common.plan.call == CallStrategy::FastScalar
                                    || do_fcall._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
                                    || unsafe {
                                        compact_scalar_call_types_match(
                                            eg,
                                            call,
                                            common,
                                            op_array.strict_types,
                                        )
                                    };
                            if do_fcall.opcode == OpCode::DoFcall
                                && argument_contract_satisfied
                            {
                                match execute_fast_scalar_method_call(
                                    eg,
                                    frame,
                                    call,
                                    func_ptr,
                                    do_fcall,
                                    do_fcall_ptr,
                                )? {
                                    ColdResult::Continue => continue 'vm,
                                    ColdResult::NewFrame(nf, no) => {
                                        frame = nf;
                                        op_array = no;
                                        continue 'vm;
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                        if bound != 0 {
                            opline_ptr = unsafe { opline_ptr.add(bound) };
                        }
                    } else {
                        // Cache miss — full resolution in cold helper
                        match op_init_method_call(eg, frame, op_array, opline)? {
                            ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                            ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                            _ => {}
                        }
                    }
                } else {
                    // Non-object — cold path (error or __invoke)
                    match op_init_method_call(eg, frame, op_array, opline)? {
                        ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                        ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                        _ => {}
                    }
                }
            }

            OpCode::InitStaticCall => {
                match op_init_static_call(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue 'vm,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::InitLateStaticCall => {
                match op_init_late_static_call(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue 'vm,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::InitDynamicCall => {
                match op_init_dynamic_call(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::CheckGenericArgs => {
                op_check_generic_args(eg, frame, op_array, opline)?;
            }

            OpCode::CheckLateStaticGenericArgs => {
                op_check_late_static_generic_args(eg, frame, op_array, opline)?;
            }

            OpCode::CheckReifiedArgs => {
                op_check_reified_args(eg, frame)?;
            }

            OpCode::CheckReifiedReturn => {
                op_check_reified_return(eg, frame, op_array, opline)?;
            }

            OpCode::CheckGenericDefault => {
                op_check_generic_default(eg, frame, opline)?;
            }

            OpCode::FetchStaticProp => {
                match op_fetch_static_prop(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchLateStaticProp => {
                match op_fetch_late_static_prop(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchClassConst => {
                match op_fetch_class_const(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchLateClassConst => {
                match op_fetch_late_class_const(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchDynamicClassConst => {
                match op_fetch_dynamic_class_const(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::FetchLateDynamicClassConst => {
                match op_fetch_late_dynamic_class_const(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AssignStaticProp => {
                match op_assign_static_prop(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::AssignLateStaticProp => {
                match op_assign_late_static_prop(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::UnsetStaticProp => {
                match op_unset_static_prop(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => {
                        frame = nf;
                        op_array = no;
                        continue 'vm;
                    }
                    ColdResult::Unhandled(exc) => {
                        eg.exception = Some(exc);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::Instanceof => {
                op_instanceof(eg, frame, op_array, opline);
            }

            OpCode::FetchConst => {
                op_fetch_const(eg, frame, op_array, opline)?;
                resume_pending_exception!();
            }

            OpCode::BindDefaultParam => {
                if op_bind_default_param(frame, op_array, opline) {
                    continue;
                }
            }

            OpCode::BindGlobal => {
                op_bind_global(eg, frame, op_array, opline);
            }

            OpCode::CheckStatic => {
                if op_check_static(eg, frame, op_array, opline) {
                    continue;
                }
            }

            OpCode::BindStatic => {
                op_bind_static(eg, frame, op_array, opline)?;
                resume_pending_exception!();
            }

            OpCode::Return => {
                let func_common_ret = unsafe { &*(*frame).func };
                #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
                if let Some(contract) = eg.take_active_generic_member_call(frame as usize) {
                    validate_generic_member_return(eg, frame, op_array, opline, &contract)?;
                }

                // ── FastScalar return: tightest path ──
                // No return type check, no globals sync, no dirty_globals propagation.
                // Guaranteed by FastScalar invariant: no globals, no statics, no try/finally,
                // no return type, no generator, may_access_globals == false.
                if func_common_ret.plan.call == CallStrategy::FastScalar
                    && func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if frame_no_heap && opline.op1_type != OpType::Const {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                unsafe { frame_return_copy_scalar(frame, return_target, src) };
                            } else {
                                let (retval, _) =
                                    prepare_user_return_value(frame, op_array, opline, false);
                                // SAFETY: the non-null target is a writable
                                // caller slot and `retval` is newly owned.
                                unsafe { frame_return_set(frame, return_target, retval) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        run_frame_destructors(eg, frame)?;
                        complete_object_construction(eg, frame);
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        pop_vm_call_frame(eg, frame);
                        return Ok(());
                    }
                    run_frame_destructors(eg, frame)?;
                    complete_object_construction(eg, frame);
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // No dirty_globals check: FastScalar callee never touches globals,
                    // and may_access_globals == false means no deeper callee did either.
    
                    continue;
                }

                // ── Fast return path ──
                // Single precomputed flag check replaces 6 runtime conditions.
                // ReturnStrategy::Fast = no globals, no statics, no return type, no try/finally, not generator.
                if func_common_ret.plan.ret == ReturnStrategy::Fast
                    && eg.exception.is_none()
                {
                    // Inline exact scalar validation before transferring the
                    // value. Complex hints use ReturnStrategy::Full.
                    let ret_hint = &func_common_ret.sig.return_type_hint;
                    let has_return_type = !matches!(ret_hint, ParamTypeHint::None);
                    if has_return_type && opline.extended_value == 0 {
                        let err = return_type_error_value(
                            eg,
                            frame,
                            op_array,
                            opline,
                            ret_hint,
                            "none returned",
                        );
                        match throw_in_frame(eg, frame, err)? {
                            ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                            ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                        }
                    }
                    let return_type_proven = known_scalar_satisfies_type_hint(
                        opline.known_result_type(),
                        ret_hint,
                        op_array.strict_types,
                    );
                    let mut prepared_return = None;
                    if has_return_type
                        && !return_type_proven
                        && opline.op1_type != OpType::Unused
                    {
                        let retval = unsafe {
                            &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                        };
                        if check_fast_scalar_return_type_hint(retval, ret_hint) != Some(true) {
                            let source = retval.dereferenced().clone();
                            let callee_class = eg
                                .declaring_class_of(func_common_ret as *const FunctionCommon)
                                .map(str::to_owned);
                            let preparation = prepare_return_type_value(
                                &source,
                                ret_hint,
                                eg,
                                op_array.strict_types,
                                frame,
                                callee_class.as_deref(),
                            )?;
                            resume_pending_exception!();
                            match preparation {
                                ReturnTypePreparation::Exact => {}
                                ReturnTypePreparation::Coerced(value, diagnostic) => {
                                    if let Some(diagnostic) = diagnostic {
                                        report_return_coercion_diagnostic(
                                            eg,
                                            frame,
                                            op_array,
                                            opline,
                                            &source,
                                            diagnostic,
                                        )?;
                                        if let Some(exception) = eg.exception.take() {
                                            if matches!(
                                                diagnostic,
                                                ReturnCoercionDiagnostic::FloatToInt
                                                    | ReturnCoercionDiagnostic::FloatStringToInt
                                            ) {
                                                let outcome = format!(
                                                    "{} returned",
                                                    declared_type_error_value_name(&source)
                                                );
                                                let err = return_type_error_value(
                                                    eg,
                                                    frame,
                                                    op_array,
                                                    opline,
                                                    ret_hint,
                                                    &outcome,
                                                );
                                                append_replaced_exception(&err, &exception, eg);
                                                match throw_in_frame(eg, frame, err)? {
                                                    ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                                    ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                                }
                                            }
                                            eg.exception = Some(exception);
                                            resume_pending_exception!();
                                        }
                                    }
                                    prepared_return = Some(value);
                                }
                                ReturnTypePreparation::Invalid => {
                                    let outcome = format!(
                                        "{} returned",
                                        declared_type_error_value_name(&source)
                                    );
                                    let err = return_type_error_value(
                                        eg,
                                        frame,
                                        op_array,
                                        opline,
                                        ret_hint,
                                        &outcome,
                                    );
                                    match throw_in_frame(eg, frame, err)? {
                                        ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                        ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                    }
                                }
                            }
                        }
                    }
                    stats::inc_return_fast();
                    if opline.op1_type != OpType::Unused {
                        let return_target = unsafe { (*frame).return_value };
                        if !return_target.is_null() {
                            // Scalar-frame fast path: if frame has no heap slots and
                            // operand is a slot (not Const), ALL values are scalar.
                            // Skip clone/needs_cleanup entirely — raw 16-byte copy.
                            let frame_no_heap = !unsafe { (*frame).has_heap_slots };
                            if prepared_return.is_none()
                                && frame_no_heap
                                && opline.op1_type != OpType::Const
                            {
                                let retval_ptr = unsafe {
                                    (frame as *const Value).add(CALL_FRAME_SLOTS + opline.op1 as usize)
                                };
                                // CV ref check: even in scalar frame, CV could be a ref.
                                // But for Fast return path, function has no by-ref params
                                // and no globals, so refs are rare. Check anyway for safety.
                                let src = if opline.op1_type == OpType::Cv {
                                    let cv_val = unsafe { &*retval_ptr };
                                    if cv_val.is_reference() {
                                        unsafe { cv_val.as_ref_ptr() as *const Value }
                                    } else {
                                        retval_ptr
                                    }
                                } else {
                                    retval_ptr
                                };
                                // Caller's target: drop old only if caller has heap slots.
                                unsafe { frame_return_copy_scalar(frame, return_target, src) };
                            } else {
                                let retval = prepared_return.take().unwrap_or_else(|| {
                                    prepare_user_return_value(frame, op_array, opline, false).0
                                });
                                // SAFETY: the non-null target is a writable
                                // caller slot and `retval` is newly owned.
                                unsafe { frame_return_set(frame, return_target, retval) };
                            }
                        }
                    }
                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    // Recursive execute_ex boundary: callee done → return to caller's macro loop
                    if frame == initial_frame {
                        run_frame_destructors(eg, frame)?;
                        complete_object_construction(eg, frame);
                        eg.current_execute_data.set(prev);
                        unsafe { cleanup_frame_slots(frame) };
                        pop_vm_call_frame(eg, frame);
                        return Ok(());
                    }
                    run_frame_destructors(eg, frame)?;
                    complete_object_construction(eg, frame);
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    // Fast-return functions don't sync globals themselves, but a deeper
                    // callee (via full return) may have left dirty entries that need to
                    // propagate up to the main scope or a function with `global` bindings.
                    if !eg.dirty_globals.is_empty()
                        && (!op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty())
                    {
                        let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                            &op_array.main_scope_vars
                        } else {
                            &op_array.global_vars
                        };
                        for (cv_idx, var_name) in vars_to_check {
                            if eg.dirty_globals.contains(var_name) {
                                if let Some(val) = eg.globals.get(var_name) {
                                    let value = if val.is_owned_reference() {
                                        val.clone_owned_reference_alias()
                                    } else {
                                        val.clone()
                                    };
                                    // SAFETY: global metadata stores validated CV indices for this frame.
                                    let cv_ptr = unsafe { (*frame).cv_mut(*cv_idx) as *mut Value };
                                    unsafe { frame_slot_set(frame, cv_ptr, value) };
                                }
                            }
                        }
                        eg.dirty_globals.clear();
                    }
    
                    continue;
                }

                // ── Full return path ──
                stats::inc_return_full();
                // Note: don't clear dirty_globals here — deeper callees may have set entries
                // that need to propagate up to the main scope. Clearing happens in the
                // caller's "after return" handler when it actually consumes the dirty set.
                if !op_array.global_vars.is_empty() {
                    for (cv_idx, var_name) in &op_array.global_vars {
                        // SAFETY: global metadata stores validated CV indices
                        // for this live frame. The raw wrapper is inspected so
                        // an actually executed BindGlobal can be distinguished
                        // from an inactive conditional declaration.
                        let cv_ptr = unsafe {
                            if tracked_scope_global_cv(op_array, var_name) != Some(*cv_idx)
                                || !tracked_global_binding_is_active(
                                    eg,
                                    op_array,
                                    var_name,
                                    (*frame).cv(*cv_idx),
                                )
                            {
                                continue;
                            }
                            (*frame).cv_mut(*cv_idx) as *mut Value
                        };
                        // A later `static` declaration for the same CV rebinds
                        // the local slot away from the global reference. The
                        // global was already updated while that reference was
                        // active, so copying the rebound static cell back here
                        // would incorrectly overwrite it at function return.
                        if op_array
                            .static_vars
                            .iter()
                            .any(|(static_cv, _, _)| static_cv == cv_idx)
                        {
                            continue;
                        }
                        let val = unsafe {
                            if (*cv_ptr).is_owned_reference() {
                                (*cv_ptr).clone_owned_reference_alias()
                            } else {
                                (*cv_ptr).clone()
                            }
                        };
                        globals_set(&mut eg.globals, var_name, val);
                        eg.dirty_globals.insert(var_name.clone());
                    }
                }
                if !op_array.static_vars.is_empty() {
                    let func_name = op_array.name.clone();
                    for (cv_idx, var_name, _) in &op_array.static_vars {
                        // SAFETY: `cv_idx` comes from this frame's validated
                        // op array and the frame remains live until return.
                        // Inspect the raw CV wrapper. `get_op_mut` follows PHP
                        // references and would make every correctly bound
                        // static look like an ordinary value, replacing its
                        // shared cell at each return boundary.
                        let cv_ptr = unsafe { (*frame).cv_mut(*cv_idx) as *mut Value };
                        // SAFETY: `cv_mut` returned the initialized raw CV slot
                        // owned by the still-live frame.
                        let value = unsafe { &*cv_ptr };
                        // BindStatic installs the request-owned reference cell
                        // eagerly, so recursive calls observe mutations before
                        // the outer frame returns. Retain a defensive fallback
                        // for hand-built op arrays that predate that contract.
                        if !value.is_owned_reference() {
                            let binding = Value::owned_reference(value.dereferenced().clone());
                            eg.with_function_static_vars_mut(
                                frame as usize,
                                &func_name,
                                |statics| {
                                    statics.insert(var_name.clone(), binding);
                                },
                            );
                        }
                    }
                }

                // ── Return type validation ──
                let mut prepared_return = None;
                let func_common = unsafe { &*(*frame).func };
                let return_hint = &func_common.sig.return_type_hint;
                // A generator declaration describes the Generator object
                // produced at call time. Its internal `return` expression is
                // the independent value exposed by Generator::getReturn().
                if !op_array.is_generator
                    && !matches!(return_hint, crate::vm::function::ParamTypeHint::None)
                {
                    let has_explicit_value = opline.extended_value == 1;
                    match return_hint {
                        crate::vm::function::ParamTypeHint::Void => {
                            if has_explicit_value {
                                // Any explicit `return expr;` in a void function is an error,
                                // including `return null;` (PHP rejects it).
                                // Only bare `return;` (extended_value=0) is allowed.
                                let err = make_error_value("TypeError",
                                    "A void function must not return a value");
                                match throw_in_frame(eg, frame, err)? {
                                    ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                    ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                }
                            }
                            // bare "return;" is OK for void
                        }
                        crate::vm::function::ParamTypeHint::Never => {
                            let err = make_error_value("TypeError",
                                "A never-returning function must not return");
                            match throw_in_frame(eg, frame, err)? {
                                ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                            }
                        }
                        hint => {
                            if !has_explicit_value {
                                let err = return_type_error_value(
                                    eg,
                                    frame,
                                    op_array,
                                    opline,
                                    hint,
                                    "none returned",
                                );
                                match throw_in_frame(eg, frame, err)? {
                                    ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                    ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                }
                            } else if opline.op1_type != OpType::Unused {
                                let retval = unsafe {
                                    &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                                };
                                let source = retval.dereferenced().clone();
                                let ret_callee_class = eg
                                    .declaring_class_of(func_common as *const FunctionCommon)
                                    .map(str::to_owned);
                                let preparation = prepare_return_type_value(
                                    &source,
                                    hint,
                                    eg,
                                    op_array.strict_types,
                                    frame,
                                    ret_callee_class.as_deref(),
                                )?;
                                resume_pending_exception!();
                                match preparation {
                                    ReturnTypePreparation::Exact => {}
                                    ReturnTypePreparation::Coerced(value, diagnostic) => {
                                        if let Some(diagnostic) = diagnostic {
                                            report_return_coercion_diagnostic(
                                                eg,
                                                frame,
                                                op_array,
                                                opline,
                                                &source,
                                                diagnostic,
                                            )?;
                                            if let Some(exception) = eg.exception.take() {
                                                if matches!(
                                                    diagnostic,
                                                    ReturnCoercionDiagnostic::FloatToInt
                                                        | ReturnCoercionDiagnostic::FloatStringToInt
                                                ) {
                                                    let outcome = format!(
                                                        "{} returned",
                                                        declared_type_error_value_name(&source)
                                                    );
                                                    let err = return_type_error_value(
                                                        eg,
                                                        frame,
                                                        op_array,
                                                        opline,
                                                        hint,
                                                        &outcome,
                                                    );
                                                    append_replaced_exception(
                                                        &err,
                                                        &exception,
                                                        eg,
                                                    );
                                                    match throw_in_frame(eg, frame, err)? {
                                                        ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                                        ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                                    }
                                                }
                                                eg.exception = Some(exception);
                                                resume_pending_exception!();
                                            }
                                        }
                                        prepared_return = Some(value);
                                    }
                                    ReturnTypePreparation::Invalid => {
                                        let outcome = format!(
                                            "{} returned",
                                            declared_type_error_value_name(&source)
                                        );
                                        let err = return_type_error_value(
                                            eg,
                                            frame,
                                            op_array,
                                            opline,
                                            hint,
                                            &outcome,
                                        );
                                        match throw_in_frame(eg, frame, err)? {
                                            ThrowResult::Handled(nf, no) => { frame = nf; op_array = no; continue 'vm; }
                                            ThrowResult::Unhandled(t) => { eg.exception = Some(t); return Ok(()); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // A return replaces any earlier non-local jump before it
                // enters another intervening finally block.
                finally_jump_state(frame, op_array, FINALLY_JUMP_CLEAR, 0, false);

                // Check if we're inside a try region with a finally block
                let current_ip = unsafe {
                    (*frame).opline.offset_from(op_array.instructions.as_ptr()) as u32
                };
                let mut need_finally: Option<u32> = None;
                for entry in &op_array.try_entries {
                    if current_ip >= entry.try_start && current_ip < entry.finally_end
                        && entry.finally_start != 0xFFFFFFFF
                        // Don't re-enter finally if we're already inside it
                        && current_ip < entry.finally_start
                    {
                        need_finally = Some(entry.finally_start);
                        break;
                    }
                }

                if let Some(finally_ip) = need_finally {
                    // Write return value now (so it's available after finally)
                    if opline.op1_type != OpType::Unused {
                        // SAFETY: Return executes with a live frame and its caller-provided slot.
                        let return_target = unsafe { (*frame).return_value };
                        if return_target.is_null()
                            && prepared_return.is_some()
                            && func_common_ret.sig.returns_reference
                        {
                            let (_, warn_non_variable) = prepare_typed_user_return_value(
                                frame,
                                op_array,
                                opline,
                                true,
                                prepared_return.take(),
                            );
                            if warn_non_variable {
                                report_php_notice(
                                    eg,
                                    frame,
                                    op_array,
                                    opline,
                                    "Only variable references should be returned by reference",
                                )?;
                            }
                        }
                        if !return_target.is_null() {
                            let (retval, warn_non_variable) = prepare_typed_user_return_value(
                                frame,
                                op_array,
                                opline,
                                func_common_ret.sig.returns_reference,
                                prepared_return.take(),
                            );
                            if warn_non_variable {
                                report_php_notice(
                                    eg,
                                    frame,
                                    op_array,
                                    opline,
                                    "Only variable references should be returned by reference",
                                )?;
                            }
                            unsafe { frame_return_set(frame, return_target, retval) };
                        }
                    }
                    // Jump to finally; after finally ends, the pending return
                    // will be detected by the finally_end check
                    eg.exception = None; // no exception, just deferred return
                    let base_ptr = op_array.instructions.as_ptr();
                    unsafe { (*frame).opline = base_ptr.add(finally_ip as usize) };
                    // Mark that we need to return after finally completes (per-frame)
                    unsafe { (*frame).pending_return_after_finally = true; }
                    continue;
                }

                // If returning from inside a finally block while an exception
                // is pending, the return suppresses the exception (PHP semantics).
                if eg.exception.is_some() {
                    eg.exception = None;
                }
                eg.finally_exceptions.remove(&(frame as usize));

                if func_common_ret.plan.needs_late_static_scope() {
                    eg.discard_late_static_scope(frame as usize);
                }

                // Generator return — save return value and mark completed
                if op_array.is_generator {
                    if let Some(gen_ref) = eg.active_generator.take() {
                        let mut gen_data = gen_ref.borrow_mut();
                        if opline.op1_type != OpType::Unused {
                            let retval = unsafe {
                                &*(*frame).get_op_ptr(opline.op1 as u32, opline.op1_type, op_array)
                            };
                            gen_data.return_value = retval.clone();
                        }
                        gen_data.has_returned = true;
                        gen_data.state = crate::vm::generator::GeneratorState::Completed;
                        gen_data.value = Value::null();
                        gen_data.key = Value::null();
                        // The live frame owns the final CV/TMP values until
                        // normal frame cleanup below. Retaining the suspended
                        // snapshots after completion delays PHP lifetimes and
                        // can recursively drop an arbitrarily deep yield-from
                        // chain when the outer Generator object is released.
                        gen_data.cv_values.clear();
                        gen_data.tmp_values.clear();
                        gen_data.delegate = None;
                        drop(gen_data);
                        eg.active_generator = Some(gen_ref);
                    }

                    let prev = unsafe { (*frame).prev_execute_data };
                    if prev.is_null() {
                        return Ok(());
                    }
                    run_frame_destructors(eg, frame)?;
                    complete_object_construction(eg, frame);
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    pop_vm_call_frame(eg, frame);
                    frame = prev;
                    op_array = unsafe { (*frame).op_array() };
                    continue;
                }

                if opline.op1_type != OpType::Unused {
                    // SAFETY: Return executes with a live frame and its caller-provided slot.
                    let return_target = unsafe { (*frame).return_value };
                    if return_target.is_null()
                        && prepared_return.is_some()
                        && func_common_ret.sig.returns_reference
                    {
                        let (_, warn_non_variable) = prepare_typed_user_return_value(
                            frame,
                            op_array,
                            opline,
                            true,
                            prepared_return.take(),
                        );
                        if warn_non_variable {
                            report_php_notice(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "Only variable references should be returned by reference",
                            )?;
                        }
                    }
                    if !return_target.is_null() {
                        let (retval, warn_non_variable) = prepare_typed_user_return_value(
                            frame,
                            op_array,
                            opline,
                            func_common_ret.sig.returns_reference,
                            prepared_return.take(),
                        );
                        if warn_non_variable {
                            report_php_notice(
                                eg,
                                frame,
                                op_array,
                                opline,
                                "Only variable references should be returned by reference",
                            )?;
                        }
                        unsafe { frame_return_set(frame, return_target, retval) };
                    }
                }

                let prev = unsafe { (*frame).prev_execute_data };
                if prev.is_null() {
                    return Ok(());
                }
                // Recursive execute_ex boundary: callee done → return to caller's macro loop
                if frame == initial_frame {
                    run_frame_destructors(eg, frame)?;
                    complete_object_construction(eg, frame);
                    eg.current_execute_data.set(prev);
                    unsafe { cleanup_frame_slots(frame) };
                    pop_vm_call_frame(eg, frame);
                    return Ok(());
                }

                run_frame_destructors(eg, frame)?;
                complete_object_construction(eg, frame);
                eg.current_execute_data.set(prev);
                unsafe { cleanup_frame_slots(frame) };
                pop_vm_call_frame(eg, frame);
                frame = prev;
                op_array = unsafe { (*frame).op_array() };
                // After callee returns, selectively re-read globals that the callee modified.
                // Only update caller CVs for variables the callee wrote back via `global` keyword.
                // This avoids overwriting by-ref modifications to other variables.
                if !eg.dirty_globals.is_empty() {
                    let vars_to_check = if !op_array.main_scope_vars.is_empty() {
                        &op_array.main_scope_vars
                    } else {
                        &op_array.global_vars
                    };
                    for (cv_idx, var_name) in vars_to_check {
                        if eg.dirty_globals.contains(var_name) {
                            if let Some(val) = eg.globals.get(var_name) {
                                let value = if val.is_owned_reference() {
                                    val.clone_owned_reference_alias()
                                } else {
                                    val.clone()
                                };
                                // SAFETY: global metadata stores validated CV indices for this frame.
                                let cv_ptr = unsafe { (*frame).cv_mut(*cv_idx) as *mut Value };
                                unsafe { frame_slot_set(frame, cv_ptr, value) };
                            }
                        }
                    }
                    // Clear dirty set once consumed by a scope that tracks globals.
                    // Intermediate frames without main_scope_vars/global_vars leave
                    // dirty_globals intact so changes propagate up to main scope.
                    if !op_array.main_scope_vars.is_empty() || !op_array.global_vars.is_empty() {
                        eg.dirty_globals.clear();
                    }
                }

                continue;
            }

            OpCode::Yield => {
                match op_yield(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    _ => {}
                }
            }

            OpCode::YieldFrom => {
                match op_yield_from(eg, frame, op_array, opline)? {
                    ColdResult::Return => { return Ok(()); }
                    ColdResult::Continue => { continue; }
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    ColdResult::Done => {}
                }
            }

            OpCode::GeneratorReturn => {
                return Err(VmError::Fatal("GeneratorReturn outside generator context".into()));
            }

            OpCode::Include => {
                match op_include(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    ColdResult::Done => {}
                    ColdResult::Return => unreachable!("include cannot suspend the caller"),
                }
                // Refresh op_array — include may have changed frame context.
                op_array = unsafe { (*frame).op_array() };
            }

            OpCode::Eval => {
                match op_eval(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    ColdResult::Done => {}
                    ColdResult::Continue | ColdResult::Return => {
                        unreachable!("eval cannot suspend or pre-advance its caller")
                    }
                }
            }

            OpCode::CloneObj => {
                match op_clone_obj(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::ValidateCloneWith => {
                match op_validate_clone_with(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(nf, no) => { frame = nf; op_array = no; continue; }
                    ColdResult::Unhandled(exc) => { eg.exception = Some(exc); return Ok(()); }
                    _ => {}
                }
            }

            OpCode::EndCloneWith => op_end_clone_with(eg, frame),

            OpCode::ReportDeprecatedTraitUses => {
                op_report_deprecated_trait_uses(eg, frame, op_array, opline)?;
                resume_pending_exception!();
            }

            OpCode::DeclareClass => {
                match op_declare_class(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    ColdResult::Done => {}
                    ColdResult::Continue | ColdResult::Return => {
                        unreachable!("DeclareClass cannot suspend or pre-advance its caller")
                    }
                }
            }

            OpCode::CreateClosure => {
                op_create_closure(eg, frame, op_array, opline);
            }

            OpCode::CreateFirstClassCallable => {
                match op_create_first_class_callable(eg, frame, op_array, opline)? {
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    _ => {}
                }
            }

            OpCode::ClosureUseVar => {
                op_closure_use_var(frame, op_array, opline);
            }

            OpCode::NullSafeCheck => {
                match op_nullsafe_check(eg, frame, op_array, opline)? {
                    ColdResult::Continue => continue,
                    ColdResult::NewFrame(new_frame, new_op_array) => {
                        frame = new_frame;
                        op_array = new_op_array;
                        continue;
                    }
                    ColdResult::Unhandled(exception) => {
                        eg.exception = Some(exception);
                        return Ok(());
                    }
                    ColdResult::Done => {}
                    ColdResult::Return => unreachable!("nullsafe check cannot return a frame"),
                }
            }

            OpCode::ReleaseTemps => {
                release_statement_temps(
                    eg,
                    frame,
                    opline.op1 as usize,
                    opline.op2 as usize,
                )?;
                resume_pending_exception!();
            }

            // All opcodes handled — new opcodes must be added above
        }

        // Advance to next instruction.
        // Use local opline_ptr to avoid redundant memory load of (*frame).opline.
        unsafe { (*frame).opline = opline_ptr.add(1); }
    }
}

#[cold]
#[inline(never)]
fn bitwise_binary_value(left: &Value, right: &Value, opcode: OpCode) -> Value {
    if let (Some(left), Some(right)) =
        (left.dereferenced().as_str(), right.dereferenced().as_str())
    {
        let (operation, preserve_longer_tail): (fn(u8, u8) -> u8, bool) = match opcode {
            OpCode::BitwiseAnd => (|left, right| left & right, false),
            OpCode::BitwiseOr => (|left, right| left | right, true),
            OpCode::BitwiseXor => (|left, right| left ^ right, false),
            _ => unreachable!("non-bitwise opcode in bitwise fallback"),
        };
        return Value::string(crate::value::php_byte_string_binary(
            left,
            right,
            operation,
            preserve_longer_tail,
        ));
    }
    let left = left.to_long_val();
    let right = right.to_long_val();
    Value::long(match opcode {
        OpCode::BitwiseAnd => left & right,
        OpCode::BitwiseOr => left | right,
        OpCode::BitwiseXor => left ^ right,
        _ => unreachable!("non-bitwise opcode in bitwise fallback"),
    })
}

#[cold]
#[inline(never)]
fn bitwise_not_value(value: &Value) -> Value {
    if let Some(string) = value.dereferenced().as_str() {
        let bytes = crate::value::php_byte_string_bytes(string);
        return Value::string(crate::value::php_byte_string_from_bytes(
            bytes.into_iter().map(|byte| !byte),
        ));
    }
    Value::long(!value.to_long_val())
}
