#[inline]
fn object_property_throw<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    class: &str,
    message: String,
) -> ColdResult<'a> {
    let error = make_error_value(class, &message);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }
}

/// Complete typed-property path for conversions, complex declarations and
/// generic runtime contracts after the exact-int write above declines.
#[inline]
fn try_assign_cached_typed_instance_property<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    opline: &Instruction,
    object: &Value,
    object_class_id: u32,
) -> Result<Option<ColdResult<'a>>, VmError> {
    let ip = unsafe {
        (opline as *const Instruction).offset_from(op_array.instructions.as_ptr()) as usize
    };
    let cache = &op_array.cache[ip];
    if cache.property_flags() != 2
        || cache.class_id != object_class_id
        || object_class_id == 0
    {
        return Ok(None);
    }

    let source = unsafe {
        &*(*frame).get_op_ptr(opline.result as u32, opline.result_type, op_array)
    };
    let source = if source.is_reference() {
        unsafe { &*source.as_ref_ptr() }
    } else {
        source
    };
    let definition = unsafe { &*cache.typed_instance_property_definition() };
    let tag = cache.typed_instance_property_tag();

    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if let Some(declaration) = definition.generic_declaration
        && let Err(message) = eg.check_cached_generic_property_value(
            object,
            &definition.name,
            source,
            declaration,
        )
    {
        return Ok(Some(object_property_throw(
            eg,
            frame,
            "TypeError",
            message,
        )));
    }
    #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
    if definition.generic_declaration.is_some() {
        unsafe {
            object.object_set_property_slot_unchecked(cache.property_slot(), source.clone());
        }
        return Ok(Some(ColdResult::Done));
    }

    // Exact non-generic scalar contracts need neither metadata traversal nor
    // allocation. Float retains PHP's strict-mode int widening rule.
    if definition.generic_declaration.is_none() {
        let source_type = source.value_type();
        let fast_value = match tag {
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT
                if source_type == ValueType::Long => Some(source.clone()),
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                if source_type == ValueType::Double => Some(source.clone()),
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT
                if source_type == ValueType::Long => {
                    Some(Value::double(source.as_long().unwrap() as f64))
                }
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_STRING
                if source_type == ValueType::String => Some(source.clone()),
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_BOOL
                if matches!(source_type, ValueType::True | ValueType::False) => {
                    Some(source.clone())
                }
            crate::vm::instruction::InlineCache::TYPED_PROPERTY_ARRAY
                if source_type == ValueType::Array => Some(source.clone()),
            _ => None,
        };
        if let Some(value) = fast_value {
            unsafe {
                object.object_set_property_slot_unchecked(cache.property_slot(), value);
            }
            return Ok(Some(ColdResult::Done));
        }
    }

    let called_class = eg
        .class_by_id(object_class_id)
        .map_or("?", |class| class.name.as_str());
    let value = match prepare_property_assignment(
        source.clone(),
        definition,
        eg,
        op_array.strict_types,
        called_class,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(Some(object_property_throw(
                eg,
                frame,
                "TypeError",
                message,
            )));
        }
    };
    unsafe {
        object.object_set_property_slot_unchecked(cache.property_slot(), value);
    }
    Ok(Some(ColdResult::Done))
}
