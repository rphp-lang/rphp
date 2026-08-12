// Kept in the execute module through include! so ordinary and static property
// opcodes can share one PHP-compatible type guard without widening Value,
// object layouts or instruction operands.

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

/// Apply PHP's property-assignment scalar conversions. Long → Double is
/// accepted even in strict mode; the remaining conversions are weak-mode
/// only. Exact union members are tested before this function, preserving an
/// `int` for `int|float` rather than eagerly widening it.
fn coerce_property_value(value: &Value, hint: &ParamTypeHint, weak: bool) -> Option<Value> {
    match hint {
        ParamTypeHint::Float => match value.value_type() {
            ValueType::Long => Some(Value::double(value.as_long()? as f64)),
            ValueType::String if weak => value
                .as_str()?
                .trim()
                .parse::<f64>()
                .ok()
                .map(Value::double),
            ValueType::True | ValueType::False if weak => {
                Some(Value::double(f64::from(value.is_truthy())))
            }
            _ => None,
        },
        ParamTypeHint::Int if weak => match value.value_type() {
            ValueType::Double => Some(Value::long(value.as_double()? as i64)),
            ValueType::True | ValueType::False => Some(Value::long(i64::from(value.is_truthy()))),
            ValueType::String => {
                let numeric = value.as_str()?.trim();
                numeric
                    .parse::<i64>()
                    .ok()
                    .or_else(|| numeric.parse::<f64>().ok().map(|number| number as i64))
                    .map(Value::long)
            }
            _ => None,
        },
        ParamTypeHint::String if weak => match value.value_type() {
            ValueType::Long | ValueType::Double | ValueType::True | ValueType::False => {
                Some(Value::string(value.echo_to_string()))
            }
            _ => None,
        },
        ParamTypeHint::Bool if weak => match value.value_type() {
            ValueType::Long | ValueType::Double | ValueType::String => {
                Some(Value::bool(value.is_truthy()))
            }
            _ => None,
        },
        ParamTypeHint::Nullable(inner) if value.value_type() != ValueType::Null => {
            if matches!(inner.as_ref(), ParamTypeHint::None) {
                None
            } else {
                coerce_property_value(value, inner, weak)
            }
        }
        ParamTypeHint::Union(parts) => parts
            .iter()
            .find_map(|part| coerce_property_value(value, part, weak)),
        _ => None,
    }
}

#[inline]
fn prepare_property_assignment(
    value: Value,
    definition: &crate::compiler::compile::PropertyDefinition,
    eg: &ExecutorGlobals,
    strict: bool,
    called_class: &str,
) -> Result<Value, String> {
    if property_type_matches_exact(
        &value,
        &definition.type_hint,
        eg,
        &definition.type_scope,
        called_class,
    ) {
        return Ok(value);
    }
    if let Some(coerced) = coerce_property_value(&value, &definition.type_hint, !strict) {
        return Ok(coerced);
    }
    Err(format!(
        "Cannot assign {} to property {}::${} of type {}",
        value.type_name(),
        definition.type_scope,
        definition.name,
        definition.type_hint.display_name()
    ))
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
    let definition = unsafe { &*cache.typed_instance_property_definition() };
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
