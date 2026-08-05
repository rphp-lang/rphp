// Loop-invariant JSON projection prelude for the shared typed executor/JIT.

#[cfg(feature = "quick-loops")]
fn quick_json_project_long(
    root: &Value,
    path: &[QuickJsonPathElement],
    op_array: &crate::compiler::OpArray,
) -> Option<i64> {
    let mut value = root;
    for element in path {
        let array = value.as_array()?;
        value = match *element {
            QuickJsonPathElement::StringLiteral(literal) => {
                let key = op_array.literals.get(literal as usize)?.as_str()?;
                if let Some(key) = canonical_decimal_array_key(key) {
                    array.get_int(key)?
                } else {
                    array.get_str(key)?
                }
            }
            QuickJsonPathElement::Integer(index) => array.get_int(index)?,
        };
    }
    value.as_long()
}

/// Decode one invariant associative JSON input, validate every fixed Long
/// projection, then publish the final PHP value and scalar inputs atomically.
/// A failed guard leaves the canonical frame untouched.
#[cfg(feature = "quick-loops")]
unsafe fn prepare_quick_json_decode_projection(
    frame: *mut ExecuteData,
    op_array: &crate::compiler::OpArray,
    plan: &QuickLongOpsLoop,
    slot_base: *mut Value,
    slots: &mut [i64; 64],
) -> bool {
    let Some(projection) = plan.json_decode_projection.as_ref() else {
        return true;
    };
    let input = match projection.input {
        QuickJsonInput::StringSlot(slot) => {
            let value = &*slot_base.add(slot as usize);
            if value.is_reference() {
                return false;
            }
            let Some(input) = value.as_str() else {
                return false;
            };
            input
        }
        QuickJsonInput::StringLiteral(literal) => {
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
    let mut projected = [0i64; 64];
    for output in &projection.projections {
        let Some(value) = quick_json_project_long(&decoded, &output.path, op_array) else {
            return false;
        };
        projected[output.result as usize] = value;
    }

    frame_slot_set(
        frame,
        slot_base.add(projection.destination as usize),
        decoded,
    );
    for output in &projection.projections {
        let value = projected[output.result as usize];
        slots[output.result as usize] = value;
        frame_tmp_set_long(frame, slot_base.add(output.result as usize), value);
    }
    true
}
