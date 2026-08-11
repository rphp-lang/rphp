// Included at the original runtime item position to preserve hot-function ordering.

unsafe fn prepare_native_mixed_properties(
    kernel: &NativeQuickLongMixedKernel,
    resolved_object_ops: &[QuickResolvedObjectOp],
    slots: &mut [i64; 64],
) -> Option<[*mut Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES]> {
    let mut values = [std::ptr::null_mut(); NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    for index in 0..kernel.property_binding_count as usize {
        let op_index = kernel.property_binding_op_indices[index] as usize;
        let property_index = kernel.property_binding_property_indices[index] as usize;
        let (receiver, property_slot) = match *resolved_object_ops.get(op_index)? {
            QuickResolvedObjectOp::PropertyMethod {
                receiver,
                property_slots,
                property_count,
                ..
            } => {
                if property_index >= property_count as usize {
                    return None;
                }
                (receiver, property_slots[property_index])
            }
            QuickResolvedObjectOp::PropertyGetter {
                receiver,
                property_slot,
                ..
            } if property_index == 0 => (receiver, property_slot),
            QuickResolvedObjectOp::ComposedProperty {
                outer_receiver,
                outer_user,
                outer_plan,
                inner_receiver,
                inner_property_slot,
                ..
            } => {
                let outer_plan = &*outer_plan;
                let outer_user = &*outer_user;
                if property_index < outer_plan.properties.len() {
                    let property = outer_plan.properties.get(property_index)?;
                    let cache = outer_user.op_array.cache.get(property.cache_ip as usize)?;
                    (outer_receiver, cache.property_slot())
                } else if property_index == NATIVE_COMPOSED_PROPERTY_INNER_INDEX {
                    (inner_receiver, inner_property_slot)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        if receiver.is_null() {
            return None;
        }
        let value = (*receiver).object_property_slot_unchecked(property_slot) as *mut Value;
        if value.is_null()
            || (*value).value_type() != ValueType::Long
            || (*value).is_reference()
        {
            return None;
        }
        let shadow_slot = kernel.property_binding_slots[index] as usize;
        slots[shadow_slot] = (*value).raw_long();
        values[index] = value;
    }
    Some(values)
}

unsafe fn commit_native_mixed_properties(
    kernel: &NativeQuickLongMixedKernel,
    properties: &[*mut Value; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    slots: &[i64; 64],
) {
    for index in 0..kernel.property_binding_count as usize {
        Value::write_long(
            properties[index],
            slots[kernel.property_binding_slots[index] as usize],
        );
    }
}
