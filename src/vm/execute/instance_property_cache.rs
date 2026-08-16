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

#[inline]
fn object_property_throw_at<'a>(
    eg: &mut ExecutorGlobals,
    frame: *mut ExecuteData,
    op_array: &'a crate::compiler::OpArray,
    instruction_index: usize,
    class: &str,
    message: String,
) -> ColdResult<'a> {
    let error = make_error_value(class, &message);
    attach_throwable_origin(&error, eg, frame, op_array, instruction_index);
    match throw_in_frame(eg, frame, error) {
        ThrowResult::Handled(new_frame, new_op_array) => {
            ColdResult::NewFrame(new_frame, new_op_array)
        }
        ThrowResult::Unhandled(thrown) => ColdResult::Unhandled(thrown),
    }
}

#[inline]
fn prepare_cached_instance_reference_write(
    object: &Value,
    slot: usize,
    value: Value,
    eg: &ExecutorGlobals,
    strict: bool,
) -> Result<Value, String> {
    // SAFETY: callers prove the cached class ID before supplying its declared
    // slot, which remains stable for the lifetime of this object allocation.
    let constraints = unsafe {
        (&*object.object_property_slot_unchecked(slot)).reference_property_constraints()
    };
    prepare_reference_assignment(value, &constraints, eg, strict)
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
    // SAFETY: dispatch passes `opline` from this op-array, so offset_from has
    // common provenance and selects the corresponding stable cache entry.
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

    // SAFETY: the live dispatch frame owns the compiler-emitted source slot.
    // A Reference points at a live VM slot, and flags == 2 proves that the
    // tagged cache word contains a stable PropertyDefinition pointer.
    let (source, definition) = unsafe {
        let source = &*(*frame).get_op_ptr(
            opline.result as u32,
            opline.result_type,
            op_array,
        );
        let source = if source.is_reference() {
            &*source.as_ref_ptr()
        } else {
            source
        };
        (
            source,
            cache
                .typed_instance_property_definition()
                .expect("typed instance cache must retain its definition"),
        )
    };
    let tag = cache.typed_instance_property_tag();
    let set_value = |value| {
        // SAFETY: the class-id guard above proves that the cached slot belongs
        // to this object; the object is not borrowed elsewhere in this path.
        // SAFETY: the cache guard also keeps this declared slot addressable for
        // the duration of the assignment-through-reference operation.
        unsafe {
            let property = object.object_property_slot_unchecked(cache.property_slot())
                as *mut Value;
            assignment_slot_set(&mut *property, value);
        };
    };

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
        let value = match prepare_cached_instance_reference_write(
            object,
            cache.property_slot(),
            source.clone(),
            eg,
            op_array.strict_types,
        ) {
            Ok(value) => value,
            Err(message) => {
                return Ok(Some(object_property_throw(eg, frame, "TypeError", message)));
            }
        };
        set_value(value);
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
            let value = match prepare_cached_instance_reference_write(
                object,
                cache.property_slot(),
                value,
                eg,
                op_array.strict_types,
            ) {
                Ok(value) => value,
                Err(message) => {
                    return Ok(Some(object_property_throw(eg, frame, "TypeError", message)));
                }
            };
            set_value(value);
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
    let value = match prepare_cached_instance_reference_write(
        object,
        cache.property_slot(),
        value,
        eg,
        op_array.strict_types,
    ) {
        Ok(value) => value,
        Err(message) => {
            return Ok(Some(object_property_throw(eg, frame, "TypeError", message)));
        }
    };
    set_value(value);
    Ok(Some(ColdResult::Done))
}
