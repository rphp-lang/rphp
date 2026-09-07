// Kept in the execute module through include! so ordinary and static property
// opcodes can share one PHP-compatible type guard without widening Value,
// object layouts or instruction operands.

/// Commit a validated existing property without retaining an object borrow
/// while a displaced value invokes PHP. The caller publishes its assignment
/// result before running the returned destructor, just like an opcode write.
#[inline]
fn commit_existing_object_property(
    eg: &mut ExecutorGlobals,
    target: &Value,
    key: &str,
    assigned: Value,
    dynamic_only: bool,
) -> Option<PreparedValueDestructor> {
    let destructor = {
        let object = target.as_object().expect("property receiver");
        let property = if dynamic_only {
            object
                .get_dynamic_property_with_position(key)
                .map(|(value, _)| value)
        } else {
            object.get_property(key)
        };
        property.and_then(|value| prepare_replaced_value_destructor(eg, value))
    };
    {
        let mut object = target.as_object_mut().expect("property receiver");
        let property = if dynamic_only {
            object.get_dynamic_property_mut(key)
        } else {
            object.get_property_mut(key)
        }
        .expect("validated existing property");
        assignment_slot_set(property, assigned);
    }
    destructor
}

/// Native object protocols write in the receiver's own declaration scope.
/// This admits private storage, but does not bypass readonly, hooks, type
/// validation, shared-reference constraints, or displaced-value cleanup.
#[cold]
#[cfg(feature = "stream-registry")]
pub(crate) fn assign_internal_object_property(
    eg: &mut ExecutorGlobals,
    target: &Value,
    name: &str,
    mut assigned: Value,
    initialized_only: bool,
    source: (&str, usize),
) -> Result<bool, VmError> {
    let (class_name, key, definition, exists, initialized) = {
        let object = target.as_object().expect("protocol object");
        let key = crate::runtime::resolve_property_key(
            eg,
            &object.class_name,
            name,
            Some(&object.class_name),
        );
        let definition = object
            .property_slot(&key)
            .and_then(|slot| eg.instance_property_definition(object.class_id, slot))
            .cloned();
        let stored = object.get_property(&key);
        (
            object.class_name.clone(),
            key,
            definition,
            stored.is_some(),
            stored.is_some_and(|value| !value.is_undef()),
        )
    };
    let virtual_property = definition.as_ref().is_some_and(|property| {
        (property.has_get_hook || property.has_set_hook)
            && !property.get_hook_is_backed
            && !property.set_hook_is_backed
    });
    if initialized_only && !initialized && !virtual_property {
        return Ok(true);
    }
    if let Some(property) = definition.as_ref() {
        if property.is_readonly && initialized {
            eg.exception = Some(make_error_value(
                "Error",
                &format!(
                    "Cannot modify readonly property {}::${name}",
                    property_diagnostic_class_name(&property.declaring_class),
                ),
            ));
            return Ok(false);
        }
        if property.has_set_hook {
            let result = call_guarded_property_hook_method(
                eg,
                target,
                name,
                PROPERTY_GUARD_HOOK_SET,
                &property.declaring_class,
                &format!("${name}::set"),
                std::slice::from_ref(&assigned),
            )?;
            if eg.exception.is_some() || result.is_some() {
                return Ok(eg.exception.is_none());
            }
        }
        if property.has_get_hook && !property.has_set_hook && !property.get_hook_is_backed {
            eg.exception = Some(make_error_value(
                "Error",
                &format!("Property {class_name}::${name} is read-only"),
            ));
            return Ok(false);
        }
        #[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
        if let Some(declaration) = property.generic_declaration
            && let Err(message) =
                eg.check_cached_generic_property_value(target, name, &assigned, declaration)
        {
            eg.exception = Some(make_error_value("TypeError", &message));
            return Ok(false);
        }
        let source_value = assigned.clone();
        let prepared = prepare_property_assignment_with_stringable(
            assigned,
            property,
            eg,
            false,
            &class_name,
            target,
        )?;
        if eg.exception.is_some() {
            return Ok(false);
        }
        match prepared {
            Ok((value, diagnostic)) => {
                assigned = value;
                if let Some(diagnostic) = diagnostic {
                    let (level, label, message) = diagnostic.details(&source_value);
                    crate::stdlib::report_diagnostic_from(
                        eg,
                        eg.current_execute_data.get(),
                        source.0,
                        source.1,
                        level,
                        label,
                        &message,
                    )?;
                    if eg.exception.is_some() {
                        return Ok(false);
                    }
                }
            }
            Err(message) => {
                eg.exception = Some(make_error_value("TypeError", &message));
                return Ok(false);
            }
        }
    }
    let constraints = target
        .as_object()
        .and_then(|object| {
            object
                .get_property(&key)
                .map(Value::reference_property_constraints)
        })
        .unwrap_or_default();
    assigned =
        match prepare_reference_assignment_with_stringable(assigned, &constraints, eg, false)? {
            Ok(value) => value,
            Err(message) => {
                eg.exception = Some(make_error_value("TypeError", &message));
                return Ok(false);
            }
        };
    if eg.exception.is_some() {
        return Ok(false);
    }
    if exists {
        let destructor = commit_existing_object_property(eg, target, &key, assigned, false);
        run_prepared_value_destructor(eg, destructor)?;
    } else {
        let result = call_guarded_property_magic_method(
            eg,
            target,
            name,
            PROPERTY_GUARD_SET,
            "__set",
            &[Value::string(name), assigned.clone()],
        )?;
        if eg.exception.is_some() || result.is_some() {
            return Ok(eg.exception.is_none());
        }
        if eg
            .find_class(&class_name)
            .is_some_and(|class| class.is_readonly)
        {
            eg.exception = Some(make_error_value(
                "Error",
                &format!("Cannot create dynamic property {class_name}::${name}"),
            ));
            return Ok(false);
        }
        let allows_dynamic = target
            .as_object()
            .is_some_and(|object| object.is_dynamic_std_class())
            || eg
                .find_class(&class_name)
                .is_some_and(|class| class.allow_dynamic_properties);
        if !allows_dynamic {
            crate::stdlib::report_diagnostic_from(
                eg,
                eg.current_execute_data.get(),
                source.0,
                source.1,
                8192,
                "Deprecated",
                &format!("Creation of dynamic property {class_name}::${name} is deprecated"),
            )?;
            if eg.exception.is_some() {
                return Ok(false);
            }
        }
        target
            .as_object_mut()
            .expect("protocol object")
            .set_property(&key, assigned);
    }
    Ok(eg.exception.is_none())
}

#[inline(always)]
fn publish_property_assignment_result(
    frame: *mut ExecuteData,
    opline: &Instruction,
    value: &Value,
) {
    if opline._pad & ASSIGN_PROP_RESULT_VALUE == 0 {
        return;
    }
    debug_assert!(matches!(opline.result_type, OpType::Tmp | OpType::Var));
    // SAFETY: the compiler sets the flag only when the source TMP is also the
    // expression result. The caller invokes this after all validation and a
    // successful property commit, while the compiler-owned result and frame
    // remain live.
    unsafe {
        let result = (*frame).get_op_mut(opline.result as u32, opline.result_type);
        frame_slot_set(frame, result, value.clone());
    }
}

#[inline(always)]
fn property_type_matches_exact(
    value: &Value,
    hint: &ParamTypeHint,
    eg: &ExecutorGlobals,
    declaring_class: &str,
    called_class: &str,
) -> bool {
    match hint {
        ParamTypeHint::None | ParamTypeHint::Mixed => true,
        ParamTypeHint::Int => value.value_type() == ValueType::Long,
        ParamTypeHint::Float => value.value_type() == ValueType::Double,
        ParamTypeHint::String => value.value_type() == ValueType::String,
        ParamTypeHint::Bool => matches!(value.value_type(), ValueType::True | ValueType::False),
        ParamTypeHint::Array => value.value_type() == ValueType::Array,
        ParamTypeHint::Callable | ParamTypeHint::ClassName(_) => check_type_hint_in_scopes(
            value,
            hint,
            eg,
            true,
            Some(declaring_class),
            Some(called_class),
        ),
        ParamTypeHint::Nullable(inner) => {
            if value.value_type() == ValueType::Null {
                true
            } else if matches!(inner.as_ref(), ParamTypeHint::None) {
                // The parser's standalone `null` type uses this otherwise
                // impossible shape; it must not become nullable mixed.
                false
            } else {
                property_type_matches_exact(
                    value,
                    inner,
                    eg,
                    declaring_class,
                    called_class,
                )
            }
        }
        ParamTypeHint::Union(parts) => parts.iter().any(|part| {
            property_type_matches_exact(value, part, eg, declaring_class, called_class)
        }),
        ParamTypeHint::Intersection(parts) => parts.iter().all(|part| {
            property_type_matches_exact(value, part, eg, declaring_class, called_class)
        }),
        ParamTypeHint::Void | ParamTypeHint::Never => false,
    }
}

#[cold]
#[inline(never)]
fn property_array_auto_init_error(
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &ExecutorGlobals,
    called_class: &str,
) -> Option<String> {
    let array = Value::array(PhpArray::new());
    (!property_type_matches_exact(
        &array,
        &definition.type_hint,
        eg,
        &definition.type_scope,
        called_class,
    ))
    .then(|| {
        format!(
            "Cannot auto-initialize an array inside property {}::${} of type {}",
            property_diagnostic_class_name(&definition.type_scope),
            definition.name,
            definition.type_hint.property_declaration_display_name(),
        )
    })
}

#[cold]
#[inline(never)]
fn reference_array_auto_init_error(
    constraints: &[crate::value::ReferencePropertyConstraint],
    eg: &ExecutorGlobals,
) -> Option<String> {
    let array = Value::array(PhpArray::new());
    let constraint = constraints.iter().find(|constraint| {
        !property_type_matches_exact(
            &array,
            &constraint.type_hint,
            eg,
            &constraint.type_scope,
            &constraint.called_class,
        )
    })?;
    Some(format!(
        "Cannot auto-initialize an array inside a reference held by property {}::${} of type {}",
        property_diagnostic_class_name(&constraint.declaring_class),
        constraint.property,
        constraint.type_hint.property_declaration_display_name(),
    ))
}

#[inline(always)]
fn operand_reference_property_constraints(
    frame: *mut ExecuteData,
    operand: u16,
    operand_type: OpType,
) -> Vec<crate::value::ReferencePropertyConstraint> {
    match operand_type {
        OpType::Cv | OpType::Tmp | OpType::Var => inspect_operand(
            frame,
            operand,
            operand_type,
            false,
            Value::reference_property_constraints,
        ),
        OpType::Const | OpType::Unused => return Vec::new(),
    }
}

/// Apply the common scalar conversion table while the older property helpers
/// retain their value-only ABI. Observable write opcodes use
/// `prepare_property_assignment_with_diagnostic()` below so the diagnostic is
/// published before storage is committed.
fn coerce_property_value(value: &Value, hint: &ParamTypeHint, weak: bool) -> Option<Value> {
    coerce_scalar_value(value, hint, weak).map(|(value, _)| value)
}

/// PHP names the concrete runtime class in typed-property diagnostics instead
/// of collapsing every declared object to the generic `object` value kind.
/// Read the immutable class name without borrowing property storage: the value
/// being assigned may be the same object whose property is currently guarded.
#[inline(always)]
fn property_assignment_type_name(value: &Value) -> &str {
    match value.value_type() {
        ValueType::True => "true",
        ValueType::False => "false",
        ValueType::Object => {
            // SAFETY: the type tag was checked above and class names are immutable
            // for the lifetime of the object allocation.
            let class = unsafe { value.object_class_name_unchecked() };
            class
                .strip_prefix("class@anonymous#")
                .map_or(class, |_| "class@anonymous")
        }
        _ => value.type_name(),
    }
}

#[inline(always)]
fn property_diagnostic_class_name(class: &str) -> &str {
    class
        .strip_prefix("class@anonymous#")
        .map_or(class, |_| "class@anonymous")
}

#[derive(Clone, Copy)]
enum PropertyIncDecOverflow {
    Increment,
    Decrement,
}

impl PropertyIncDecOverflow {
    #[inline(always)]
    fn from_assignment_flags(flags: u16) -> Option<Self> {
        if flags & PROPERTY_INCDEC_INCREMENT != 0 {
            Some(Self::Increment)
        } else if flags & PROPERTY_INCDEC_DECREMENT != 0 {
            Some(Self::Decrement)
        } else {
            None
        }
    }

    #[inline(always)]
    fn from_dim_assignment_flags(flags: u16) -> Option<Self> {
        if flags & ASSIGN_DIM_INCDEC_INCREMENT != 0 {
            Some(Self::Increment)
        } else if flags & ASSIGN_DIM_INCDEC_DECREMENT != 0 {
            Some(Self::Decrement)
        } else {
            None
        }
    }

    #[inline(always)]
    fn overflow_value(self, current: &Value) -> Option<Value> {
        let current = current.as_long()?;
        match self {
            Self::Increment if current == i64::MAX => {
                Some(Value::double(current as f64 + 1.0))
            }
            Self::Decrement if current == i64::MIN => {
                Some(Value::double(current as f64 - 1.0))
            }
            _ => None,
        }
    }

    fn action(self) -> &'static str {
        match self {
            Self::Increment => "increment",
            Self::Decrement => "decrement",
        }
    }

    fn boundary(self) -> &'static str {
        match self {
            Self::Increment => "maximal",
            Self::Decrement => "minimal",
        }
    }
}

#[cold]
#[inline(never)]
fn reference_incdec_overflow_message(
    reference: &Value,
    current: &Value,
    eg: &ExecutorGlobals,
    overflow: PropertyIncDecOverflow,
) -> Option<String> {
    let overflow_value = overflow.overflow_value(current)?;
    let constraint = reference
        .reference_property_constraints()
        .into_iter()
        .find(|constraint| {
            !property_type_matches_exact(
                &overflow_value,
                &constraint.type_hint,
                eg,
                &constraint.type_scope,
                &constraint.called_class,
            )
        })?;
    Some(format!(
        "Cannot {} a reference held by property {}::${} of type {} past its {} value",
        overflow.action(),
        property_diagnostic_class_name(&constraint.declaring_class),
        constraint.property,
        constraint.type_hint.property_declaration_display_name(),
        overflow.boundary(),
    ))
}

#[cold]
#[inline(never)]
fn property_incdec_overflow_message(
    stored: &Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &ExecutorGlobals,
    called_class: &str,
    overflow: PropertyIncDecOverflow,
) -> Option<String> {
    let current = stored.dereferenced();
    let overflow_value = overflow.overflow_value(current)?;
    if stored.is_owned_reference()
        && let Some(message) =
            reference_incdec_overflow_message(stored, current, eg, overflow)
    {
        return Some(message);
    }
    if property_type_matches_exact(
        &overflow_value,
        &definition.type_hint,
        eg,
        &definition.type_scope,
        called_class,
    ) {
        return None;
    }
    Some(format!(
        "Cannot {} property {}::${} of type {} past its {} value",
        overflow.action(),
        property_diagnostic_class_name(&definition.type_scope),
        definition.name,
        definition.type_hint.property_declaration_display_name(),
        overflow.boundary(),
    ))
}

#[inline]
pub(crate) fn prepare_property_assignment(
    value: Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &ExecutorGlobals,
    strict: bool,
    called_class: &str,
) -> Result<Value, String> {
    prepare_property_assignment_with_diagnostic(value, definition, eg, strict, called_class)
        .map(|(value, _)| value)
}

#[inline]
fn prepare_property_assignment_with_diagnostic(
    value: Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &ExecutorGlobals,
    strict: bool,
    called_class: &str,
) -> Result<(Value, Option<ScalarCoercionDiagnostic>), String> {
    if property_type_matches_exact(
        &value,
        &definition.type_hint,
        eg,
        &definition.type_scope,
        called_class,
    ) {
        return Ok((value, None));
    }
    if let Some((coerced, diagnostic)) =
        coerce_scalar_value(&value, &definition.type_hint, !strict)
    {
        return Ok((coerced, diagnostic));
    }
    Err(format!(
        "Cannot assign {} to property {}::${} of type {}",
        property_assignment_type_name(&value),
        property_diagnostic_class_name(&definition.type_scope),
        definition.name,
        definition.type_hint.property_declaration_display_name()
    ))
}

#[cold]
#[inline(never)]
fn prepare_reference_assignment_scalar(
    value: Value,
    constraints: &[crate::value::ReferencePropertyConstraint],
    eg: &ExecutorGlobals,
    strict: bool,
) -> Result<Value, String> {
    if constraints.is_empty() {
        return Ok(value);
    }

    let mut candidates = Vec::with_capacity(constraints.len() + 1);
    candidates.push(value.clone());
    for constraint in constraints {
        if !property_type_matches_exact(
            &value,
            &constraint.type_hint,
            eg,
            &constraint.type_scope,
            &constraint.called_class,
        ) {
            if let Some(coerced) = coerce_property_value(&value, &constraint.type_hint, !strict) {
                candidates.push(coerced);
            } else {
                return Err(format!(
                    "Cannot assign {} to reference held by property {}::${} of type {}",
                    property_assignment_type_name(&value),
                    property_diagnostic_class_name(&constraint.declaring_class),
                    constraint.property,
                    constraint.type_hint.property_declaration_display_name()
                ));
            }
        }
    }

    if let Some(candidate) = candidates.into_iter().find(|candidate| {
        constraints.iter().all(|constraint| {
            property_type_matches_exact(
                candidate,
                &constraint.type_hint,
                eg,
                &constraint.type_scope,
                &constraint.called_class,
            )
        })
    }) {
        return Ok(candidate);
    }

    let owners = constraints
        .iter()
        .map(|constraint| {
            format!(
                "property {}::${} of type {}",
                property_diagnostic_class_name(&constraint.declaring_class),
                constraint.property,
                constraint.type_hint.property_declaration_display_name()
            )
        })
        .collect::<Vec<_>>()
        .join(" and ");
    Err(format!(
        "Cannot assign {} to reference held by {owners}, as this would result in an inconsistent type conversion",
        property_assignment_type_name(&value),
    ))
}

#[inline]
fn property_type_accepts_string(hint: &ParamTypeHint) -> bool {
    match hint {
        ParamTypeHint::String => true,
        ParamTypeHint::Nullable(inner) => property_type_accepts_string(inner),
        ParamTypeHint::Union(parts) => parts.iter().any(property_type_accepts_string),
        _ => false,
    }
}

/// Canonical CV reference-assignment boundary. The dispatch caller already
/// routes every ordinary type failure through `throw_operator_error`; retain
/// that hot ABI while carrying a rare engine control result only in the cold
/// error value.
enum ReferenceAssignmentError {
    Type(String),
    Vm(VmError),
}

#[cold]
#[inline(never)]
fn wrap_reference_string_conversion_exception(eg: &mut ExecutorGlobals, message: &str) {
    let Some(previous) = eg.exception.take() else {
        return;
    };
    let error = make_error_value("TypeError", message);
    if let Some(mut object) = error.as_object_mut() {
        let key = eg
            .find_property_visibility("TypeError", "previous")
            .map_or_else(
                || "previous".to_string(),
                |(_, declaring)| crate::runtime::mangle_private_prop(&declaring, "previous"),
            );
        object.set_property(&key, previous);
    }
    eg.exception = Some(error);
}

#[cold]
#[inline(never)]
fn prepare_reference_assignment(
    value: Value,
    constraints: &[crate::value::ReferencePropertyConstraint],
    eg: &mut ExecutorGlobals,
    strict: bool,
) -> Result<Value, ReferenceAssignmentError> {
    let prepared = prepare_reference_assignment_scalar(value.clone(), constraints, eg, strict);
    if prepared.is_ok()
        || strict
        || value.dereferenced().value_type() != ValueType::Object
        || constraints.is_empty()
        || !constraints.iter().all(|constraint| {
            !property_type_matches_exact(
                &value,
                &constraint.type_hint,
                eg,
                &constraint.type_scope,
                &constraint.called_class,
            ) && property_type_accepts_string(&constraint.type_hint)
        })
    {
        return prepared.map_err(ReferenceAssignmentError::Type);
    }
    let rendered = match call_object_string_conversion(eg, &value) {
        Ok(rendered) => rendered,
        Err(error) => return Err(ReferenceAssignmentError::Vm(error)),
    };
    if eg.exception.is_some() {
        if let Err(message) = &prepared {
            wrap_reference_string_conversion_exception(eg, message);
        }
        return prepared.map_err(ReferenceAssignmentError::Type);
    }
    let Some(rendered) = rendered else {
        return prepared.map_err(ReferenceAssignmentError::Type);
    };
    prepare_reference_assignment_scalar(
        rendered,
        constraints,
        eg,
        strict,
    )
    .map_err(ReferenceAssignmentError::Type)
}

/// Extend the non-reentrant scalar property guard with PHP's weak Stringable
/// conversion. The user hook runs before the caller commits storage, so a
/// reentrant write remains visible during conversion but the outer assignment
/// wins only after the converted value passes the complete property contract.
#[cold]
#[inline(never)]
fn prepare_property_assignment_with_stringable(
    value: Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &mut ExecutorGlobals,
    strict: bool,
    called_class: &str,
    receiver: *const Value,
) -> Result<Result<(Value, Option<ScalarCoercionDiagnostic>), String>, VmError> {
    let prepared = prepare_property_assignment_with_diagnostic(
        value.clone(),
        definition,
        eg,
        strict,
        called_class,
    );
    if prepared.is_ok()
        || strict
        || value.dereferenced().value_type() != ValueType::Object
        || !property_type_accepts_string(&definition.type_hint)
    {
        return Ok(prepared);
    }
    let Some(rendered) = call_object_string_conversion(eg, &value)? else {
        return Ok(prepared);
    };
    let prepared = prepare_property_assignment_with_diagnostic(
        rendered,
        definition,
        eg,
        strict,
        called_class,
    );
    // SAFETY: the active opcode still owns the receiver slot. Re-read it only
    // after user code returns because that code may have replaced its value.
    if unsafe { (&*receiver).as_object().is_none() } {
        eg.exception = Some(make_error_value(
            "Error",
            &format!(
                "Object was released while assigning to property {}::${}",
                property_diagnostic_class_name(called_class),
                definition.name,
            ),
        ));
    }
    Ok(prepared)
}

/// Apply the same deferred Stringable conversion to a cell owned by typed
/// properties. Every owner must independently choose string conversion; mixed
/// exact/coerced outcomes retain the canonical inconsistent-conversion error.
#[cold]
#[inline(never)]
fn prepare_reference_assignment_with_stringable(
    value: Value,
    constraints: &[crate::value::ReferencePropertyConstraint],
    eg: &mut ExecutorGlobals,
    strict: bool,
) -> Result<Result<Value, String>, VmError> {
    let prepared = prepare_reference_assignment_scalar(value.clone(), constraints, eg, strict);
    if prepared.is_ok()
        || strict
        || value.dereferenced().value_type() != ValueType::Object
        || constraints.is_empty()
        || !constraints.iter().all(|constraint| {
            !property_type_matches_exact(
                &value,
                &constraint.type_hint,
                eg,
                &constraint.type_scope,
                &constraint.called_class,
            ) && property_type_accepts_string(&constraint.type_hint)
        })
    {
        return Ok(prepared);
    }
    let rendered = call_object_string_conversion(eg, &value)?;
    if eg.exception.is_some() {
        if let Err(message) = &prepared {
            wrap_reference_string_conversion_exception(eg, message);
        }
        return Ok(prepared);
    }
    let Some(rendered) = rendered else {
        return Ok(prepared);
    };
    Ok(prepare_reference_assignment_scalar(
        rendered,
        constraints,
        eg,
        strict,
    ))
}

#[cold]
#[inline(never)]
pub(crate) fn prepare_typed_property_reference_attachment(
    value: Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    constraints: &[crate::value::ReferencePropertyConstraint],
    eg: &ExecutorGlobals,
    strict: bool,
    called_class: &str,
) -> Result<Value, String> {
    let original_type = property_assignment_type_name(&value).to_string();
    let prepared = prepare_property_assignment(value, definition, eg, strict, called_class)?;
    if let Some(existing) = constraints.iter().find(|constraint| {
        !property_type_matches_exact(
            &prepared,
            &constraint.type_hint,
            eg,
            &constraint.type_scope,
            &constraint.called_class,
        )
    }) {
        return Err(format!(
            "Reference with value of type {} held by property {}::${} of type {} is not compatible with property {}::${} of type {}",
            original_type,
            property_diagnostic_class_name(&existing.declaring_class),
            existing.property,
            existing.type_hint.property_declaration_display_name(),
            property_diagnostic_class_name(&definition.declaring_class),
            definition.name,
            definition.type_hint.property_declaration_display_name()
        ));
    }
    Ok(prepared)
}

/// Whether a warmed instance-property write cache can accept this exact value
/// without scalar coercion. Generic definitions retain their own runtime
/// boundary because a substituted contract may be stricter than its erased
/// PropertyDefinition.
#[inline(always)]
fn instance_property_cache_accepts_exact_non_generic_write(
    cache: &crate::vm::instruction::InlineCache,
    value: &Value,
    eg: &ExecutorGlobals,
    called_class: &str,
) -> bool {
    if cache.property_flags() == 3 {
        return true;
    }
    if cache.property_flags() != 2 {
        return false;
    }
    match cache.typed_instance_property_tag() {
        crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT => {
            return value.dereferenced().value_type() == ValueType::Long;
        }
        crate::vm::instruction::InlineCache::TYPED_PROPERTY_FLOAT => {
            return value.dereferenced().value_type() == ValueType::Double;
        }
        crate::vm::instruction::InlineCache::TYPED_PROPERTY_STRING => {
            return value.dereferenced().value_type() == ValueType::String;
        }
        crate::vm::instruction::InlineCache::TYPED_PROPERTY_BOOL => {
            return matches!(
                value.dereferenced().value_type(),
                ValueType::True | ValueType::False
            );
        }
        crate::vm::instruction::InlineCache::TYPED_PROPERTY_ARRAY => {
            return value.dereferenced().value_type() == ValueType::Array;
        }
        _ => {}
    }
    let definition = cache
        .typed_instance_property_definition()
        .expect("typed instance cache must retain its definition");
    definition.generic_declaration.is_none()
        && property_type_matches_exact(
            value,
            &definition.type_hint,
            eg,
            &definition.type_scope,
            called_class,
        )
}

/// Long-only plans may write any i64 once the cache proves a non-generic exact
/// `int` property. This restores the established property-plan/JIT admission
/// without allowing `float`, unions or reified `T` to bypass their guards.
#[inline(always)]
fn instance_property_cache_accepts_long_write(
    cache: &crate::vm::instruction::InlineCache,
) -> bool {
    if cache.property_flags() == 3 {
        return true;
    }
    cache.property_flags() == 2
        && cache.typed_instance_property_tag()
            == crate::vm::instruction::InlineCache::TYPED_PROPERTY_INT
}
