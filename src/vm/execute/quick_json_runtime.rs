// Loop-invariant JSON projection prelude for the shared typed executor/JIT.

#[cfg(feature = "quick-loops")]
fn quick_invariant_project_value<'a>(
    root: &'a Value,
    path: &[QuickInvariantPathElement],
    op_array: &crate::compiler::OpArray,
) -> Option<&'a Value> {
    let mut value = root;
    for element in path {
        let array = value.as_array()?;
        value = match *element {
            QuickInvariantPathElement::StringLiteral(literal) => {
                let key = op_array.literals.get(literal as usize)?.as_str()?;
                if let Some(key) = canonical_decimal_array_key(key) {
                    array.get_int(key)?
                } else {
                    array.get_str(key)?
                }
            }
            QuickInvariantPathElement::Integer(index) => array.get_int(index)?,
        };
    }
    Some(value)
}

/// Decode one invariant associative JSON input, validate every fixed typed or
/// derived projection, then publish the final PHP value and scalar inputs
/// atomically. A failed source/path/type guard leaves the frame untouched.
#[cfg(feature = "quick-loops")]
unsafe fn prepare_quick_typed_invariant_source(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    source: Option<&crate::vm::quick::QuickTypedInvariantSource>,
    slot_base: *mut Value,
) -> bool {
    let Some(source) = source else {
        return true;
    };
    let input_source = match source.producer {
        QuickTypedInvariantProducer::JsonDecodeAssociative { input } => input,
    };
    let input = match input_source {
        QuickInvariantInput::StringSlot(slot) => {
            let value = &*slot_base.add(slot as usize);
            if value.is_reference() {
                return false;
            }
            let Some(input) = value.as_str() else {
                return false;
            };
            input
        }
        QuickInvariantInput::StringLiteral(literal) => {
            let Some(input) = op_array
                .literals
                .get(literal as usize)
                .and_then(Value::as_str)
            else {
                return false;
            };
            input
        }
    };

    let decoded = crate::stdlib::json_decode_string(input, true);
    let mut projected = Vec::with_capacity(source.projections.len());
    for output in &source.projections {
        let Some(value) = quick_invariant_project_value(&decoded, &output.path, op_array) else {
            return false;
        };
        let value = match output.kind {
            QuickInvariantValueKind::Long => {
                let Some(value) = value.as_long() else {
                    return false;
                };
                Value::long(value)
            }
            QuickInvariantValueKind::Double => {
                if value.value_type() != ValueType::Double {
                    return false;
                }
                Value::double(value.raw_double())
            }
            QuickInvariantValueKind::String => {
                if value.value_type() != ValueType::String {
                    return false;
                }
                value.clone()
            }
            QuickInvariantValueKind::StringLength => {
                let Some(value) = value.as_str() else {
                    return false;
                };
                Value::long(value.len() as i64)
            }
        };
        projected.push(value);
    }

    frame_slot_set(
        frame,
        slot_base.add(source.destination as usize),
        decoded,
    );
    for (output, value) in source.projections.iter().zip(projected) {
        frame_slot_set(frame, slot_base.add(output.result as usize), value);
    }
    true
}
