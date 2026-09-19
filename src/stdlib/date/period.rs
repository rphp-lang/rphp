//! DatePeriod construction, getters and deterministic iteration.

use super::{datetime, interval, parser, *};
use crate::value::NativeObjectState;

#[derive(Clone, Debug)]
pub(super) struct DatePeriodState {
    start: datetime::DateTimeState,
    start_class: String,
    current: Option<datetime::DateTimeState>,
    end: Option<datetime::DateTimeState>,
    interval: interval::DateIntervalState,
    recurrences: Option<i64>,
    include_start: bool,
    include_end: bool,
    initialized: bool,
}

impl Default for DatePeriodState {
    fn default() -> Self {
        Self {
            start: datetime::DateTimeState::default(),
            start_class: "DateTimeImmutable".to_string(),
            current: None,
            end: None,
            interval: interval::DateIntervalState::default(),
            recurrences: None,
            include_start: true,
            include_end: false,
            initialized: false,
        }
    }
}

impl NativeObjectState for DatePeriodState {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn snapshot(value: &Value, eg: &mut ExecutorGlobals) -> Option<DatePeriodState> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DatePeriodState>().cloned();
    let class_name = object.class_name.to_string();
    drop(object);
    if state.as_ref().is_some_and(|state| state.initialized) {
        return state;
    }
    eg.exception = Some(crate::value::make_error_value(
        "DateObjectError",
        &format!(
            "Object of type {class_name} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    ));
    None
}

fn publish_properties(object: &mut PhpObject, state: &DatePeriodState, eg: &ExecutorGlobals) {
    let serialized = serialized(state, eg);
    let Some(values) = serialized.as_array() else {
        return;
    };
    for name in [
        "start",
        "current",
        "end",
        "interval",
        "recurrences",
        "include_start_date",
        "include_end_date",
    ] {
        if let Some(value) = values.get_str(name) {
            object.set_property(name, value.clone_for_php_storage());
        }
    }
}

fn install(receiver: &Value, state: DatePeriodState, eg: &ExecutorGlobals) -> bool {
    let Some(mut object) = receiver.as_object_mut() else {
        return false;
    };
    publish_properties(&mut object, &state, eg);
    *object.native_object_state_mut::<DatePeriodState>() = state;
    true
}

fn allocate_as(eg: &ExecutorGlobals, class_name: &str, state: DatePeriodState) -> Option<Value> {
    let class = eg.find_class(class_name)?;
    let mut object = PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    );
    *object.native_object_state_mut::<DatePeriodState>() = state.clone();
    let value = Value::object(object);
    install(&value, state, eg).then_some(value)
}

fn state_object(eg: &ExecutorGlobals, class_name: &str, state: datetime::DateTimeState) -> Value {
    datetime::allocate(eg, class_name, state).unwrap_or_else(Value::null)
}

fn interval_object(eg: &ExecutorGlobals, state: interval::DateIntervalState) -> Value {
    interval::allocate(eg, state).unwrap_or_else(Value::null)
}

fn comparison_key(state: &datetime::DateTimeState) -> (i64, u32) {
    (state.timestamp, state.microsecond)
}

fn advance(state: &mut datetime::DateTimeState, interval: &interval::DateIntervalState) -> bool {
    let before = comparison_key(state);
    interval::apply_to_datetime(state, interval, false);
    comparison_key(state) != before
}

fn generated_values(state: &DatePeriodState, eg: &ExecutorGlobals) -> PhpArray {
    let mut result = PhpArray::new();
    let mut current = state.start.clone();
    let mut emitted = 0_i64;
    let recurrence_limit = state.recurrences.map(|recurrences| {
        recurrences + i64::from(state.include_start) + i64::from(state.include_end)
    });
    if !state.include_start && !advance(&mut current, &state.interval) {
        return result;
    }
    loop {
        if let Some(limit) = recurrence_limit {
            if emitted >= limit {
                break;
            }
        }
        if let Some(end) = state.end.as_ref() {
            let order = comparison_key(&current).cmp(&comparison_key(end));
            if order.is_gt() || (order.is_eq() && !state.include_end) {
                break;
            }
        }
        result.push(state_object(eg, &state.start_class, current.clone()));
        emitted += 1;
        if !advance(&mut current, &state.interval) {
            break;
        }
        if emitted > 1_000_000 {
            break;
        }
    }
    result
}

fn serialized(state: &DatePeriodState, eg: &ExecutorGlobals) -> Value {
    let mut result = PhpArray::new();
    result.set_str(
        "start",
        state_object(eg, &state.start_class, state.start.clone()),
    );
    result.set_str(
        "current",
        state.current.clone().map_or_else(Value::null, |current| {
            state_object(eg, &state.start_class, current)
        }),
    );
    result.set_str(
        "end",
        state
            .end
            .clone()
            .map_or_else(Value::null, |end| state_object(eg, &state.start_class, end)),
    );
    result.set_str("interval", interval_object(eg, state.interval.clone()));
    result.set_str(
        "recurrences",
        Value::long(
            state.recurrences.unwrap_or(0)
                + i64::from(state.include_start)
                + i64::from(state.include_end),
        ),
    );
    result.set_str("include_start_date", Value::bool(state.include_start));
    result.set_str("include_end_date", Value::bool(state.include_end));
    Value::array(result)
}

const SERIALIZED_KEYS: [&str; 7] = [
    "start",
    "current",
    "end",
    "interval",
    "recurrences",
    "include_start_date",
    "include_end_date",
];

pub(crate) fn debug_projection(value: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DatePeriodState>()?;
    if !state.initialized {
        return None;
    }
    let mut result = super::custom_properties(value, &SERIALIZED_KEYS, eg);
    for name in SERIALIZED_KEYS {
        result.set_str(
            name,
            object
                .get_property(name)
                .cloned()
                .unwrap_or_else(Value::null),
        );
    }
    Some(Value::array(result))
}

fn options(value: Option<&Value>) -> (bool, bool) {
    let value = value.and_then(Value::as_long).unwrap_or(0);
    (value & 1 == 0, value & 2 != 0)
}

fn malformed_period(eg: &mut ExecutorGlobals, specification: &str) {
    eg.exception = Some(crate::value::make_error_value(
        "DateMalformedPeriodStringException",
        &format!("Unknown or bad format ({specification})"),
    ));
}

fn from_iso(
    specification: &str,
    option_value: Option<&Value>,
    eg: &mut ExecutorGlobals,
) -> Option<DatePeriodState> {
    let mut parts = specification.split('/').collect::<Vec<_>>();
    let recurrence = parts
        .first()
        .and_then(|part| part.strip_prefix('R'))
        .and_then(|count| {
            (!count.is_empty())
                .then(|| count.parse::<i64>().ok())
                .flatten()
        });
    if parts.first().is_some_and(|part| part.starts_with('R')) {
        parts.remove(0);
    }
    if parts.len() != 2 || recurrence.is_none() {
        return None;
    }
    let mut start = parser::parse_datetime(parts[0], None, None, eg).ok()?.state;
    // ISO repeating intervals normalize the `Z` designator to the numeric
    // zero-offset representation even though ordinary DateTime construction
    // retains `Z` as an abbreviation timezone.
    if start.timezone.kind == 2 && start.timezone.name == "Z" {
        start.timezone.kind = 1;
        start.timezone.name = "+00:00".to_string();
    }
    let interval = interval::parse_iso_duration(parts[1])?;
    let (include_start, include_end) = options(option_value);
    Some(DatePeriodState {
        start,
        start_class: "DateTimeImmutable".to_string(),
        current: None,
        end: None,
        interval,
        recurrences: recurrence,
        include_start,
        include_end,
        initialized: true,
    })
}

pub(crate) fn fn_date_period_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let first = arg!(ed, 1).dereferenced();
    if let Some(specification) = first.as_str() {
        super::super::report_internal_deprecation(
            eg,
            ed,
            "Calling DatePeriod::__construct(string $isostr, int $options = 0) is deprecated, use DatePeriod::createFromISO8601String() instead",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let Some(state) = from_iso(specification, arg_opt!(ed, 2), eg) else {
            malformed_period(eg, specification);
            return Ok(());
        };
        install(arg!(ed, 0), state, eg);
        return Ok(());
    }
    let start = datetime::state_snapshot(first, eg);
    let Some(start) = start else {
        return Ok(());
    };
    let start_class = first
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTimeImmutable".to_string());
    let Some(interval) = interval::snapshot(arg!(ed, 2), eg) else {
        return Ok(());
    };
    let third = arg!(ed, 3).dereferenced();
    let (recurrences, end) = if let Some(recurrences) = third.as_long() {
        if recurrences < 1 {
            eg.exception = Some(crate::value::make_error_value(
                "Exception",
                "Recurrence count must be greater than 0",
            ));
            return Ok(());
        }
        (Some(recurrences), None)
    } else {
        let Some(end) = datetime::state_snapshot(third, eg) else {
            return Ok(());
        };
        (None, Some(end))
    };
    let (include_start, include_end) = options(arg_opt!(ed, 4));
    install(
        arg!(ed, 0),
        DatePeriodState {
            start,
            start_class,
            current: None,
            end,
            interval,
            recurrences,
            include_start,
            include_end,
            initialized: true,
        },
        eg,
    );
    Ok(())
}

pub(crate) fn fn_date_period_create_from_iso(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let specification = arg_str!(ed, 1);
    if specification.starts_with('R') && specification.matches('/').count() == 1 {
        eg.exception = Some(crate::value::make_error_value(
            "DateMalformedPeriodStringException",
            &format!(
                "DatePeriod::createFromISO8601String(): ISO interval must contain an interval, \"{specification}\" given"
            ),
        ));
        return Ok(());
    }
    let Some(state) = from_iso(specification.as_ref(), arg_opt!(ed, 2), eg) else {
        malformed_period(eg, specification.as_ref());
        return Ok(());
    };
    let class_name = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .filter(|class_name| eg.class_is_a(class_name, "DatePeriod"))
        .unwrap_or("DatePeriod")
        .to_string();
    if let Some(result) = allocate_as(eg, &class_name, state) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_period_get_start(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, state_object(eg, &state.start_class, state.start));
}

pub(crate) fn fn_date_period_get_end(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(
        rv,
        state
            .end
            .map_or_else(Value::null, |end| state_object(eg, &state.start_class, end))
    );
}

pub(crate) fn fn_date_period_get_interval(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, interval_object(eg, state.interval));
}

pub(crate) fn fn_date_period_get_recurrences(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, state.recurrences.map_or_else(Value::null, Value::long));
}

pub(crate) fn fn_date_period_get_iterator(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let values = generated_values(&state, eg);
    if let Some(iterator) =
        super::super::builtin_classes::array_object::iterator_from_array(eg, values)
    {
        ret!(rv, iterator);
    }
    Ok(())
}

pub(crate) fn fn_date_period_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let mut result = serialized(&state, eg)
        .as_array()
        .expect("DatePeriod serialized state is an array")
        .clone();
    super::append_custom_properties(&mut result, arg!(ed, 0), &SERIALIZED_KEYS, eg);
    ret!(rv, Value::array(result));
}

fn invalid_serialization(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Invalid serialization data for DatePeriod object",
    ));
}

fn state_from_array(data: &PhpArray, eg: &mut ExecutorGlobals) -> Option<DatePeriodState> {
    let start_value = data.get_str("start")?.dereferenced();
    if start_value.value_type() != ValueType::Object {
        return None;
    }
    let start = datetime::state_snapshot(start_value, eg)?;
    let start_class = start_value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTimeImmutable".to_string());
    let interval_value = data.get_str("interval")?.dereferenced();
    if interval_value.value_type() != ValueType::Object {
        return None;
    }
    let interval = interval::snapshot(interval_value, eg)?;
    let current = data.get_str("current").and_then(|value| {
        let value = value.dereferenced();
        match value.value_type() {
            ValueType::Null => None,
            ValueType::Object => datetime::state_snapshot(value, eg),
            _ => None,
        }
    });
    if data.get_str("current").is_some_and(|value| {
        value.dereferenced().value_type() != ValueType::Null && current.is_none()
    }) {
        return None;
    }
    let end = data.get_str("end").and_then(|value| {
        let value = value.dereferenced();
        match value.value_type() {
            ValueType::Null => None,
            ValueType::Object => datetime::state_snapshot(value, eg),
            _ => None,
        }
    });
    if data
        .get_str("end")
        .is_some_and(|value| value.dereferenced().value_type() != ValueType::Null && end.is_none())
    {
        return None;
    }
    let include_start = data.get_str("include_start_date").and_then(|value| {
        matches!(
            value.dereferenced().value_type(),
            ValueType::True | ValueType::False
        )
        .then(|| value.dereferenced().is_truthy())
    })?;
    let include_end = data.get_str("include_end_date").and_then(|value| {
        matches!(
            value.dereferenced().value_type(),
            ValueType::True | ValueType::False
        )
        .then(|| value.dereferenced().is_truthy())
    })?;
    let effective_recurrences = data.get_str("recurrences")?;
    let recurrences =
        if end.is_some() || effective_recurrences.dereferenced().value_type() == ValueType::Null {
            None
        } else {
            let effective = effective_recurrences.dereferenced().as_long()?;
            Some(
                effective
                    .saturating_sub(i64::from(include_start))
                    .saturating_sub(i64::from(include_end)),
            )
        };
    Some(DatePeriodState {
        start,
        start_class,
        current,
        end,
        interval,
        recurrences,
        include_start,
        include_end,
        initialized: true,
    })
}

pub(crate) fn fn_date_period_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let Some(state) = state_from_array(&data, eg) else {
        if eg.exception.is_none() {
            invalid_serialization(eg);
        }
        return Ok(());
    };
    if install(arg!(ed, 0), state, eg) {
        super::restore_custom_properties(arg!(ed, 0), &data, &SERIALIZED_KEYS, eg);
    }
    Ok(())
}

pub(crate) fn fn_date_period_wakeup(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let Some(object) = receiver.as_object() else {
        return Ok(());
    };
    let mut data = PhpArray::new();
    for name in [
        "start",
        "current",
        "end",
        "interval",
        "recurrences",
        "include_start_date",
        "include_end_date",
    ] {
        if let Some(value) = object.get_property(name) {
            data.set_str(name, value.clone_for_php_storage());
        }
    }
    drop(object);
    let Some(state) = state_from_array(&data, eg) else {
        if eg.exception.is_none() {
            invalid_serialization(eg);
        }
        return Ok(());
    };
    install(&receiver, state, eg);
    Ok(())
}

pub(crate) fn fn_date_period_set_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let Some(state) = state_from_array(&data, eg) else {
        if eg.exception.is_none() {
            invalid_serialization(eg);
        }
        return Ok(());
    };
    let class_name = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .filter(|class_name| eg.class_is_a(class_name, "DatePeriod"))
        .unwrap_or("DatePeriod")
        .to_string();
    if let Some(value) = allocate_as(eg, &class_name, state) {
        ret!(rv, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurrence_flags_control_visible_count() {
        let count =
            |include_start, include_end| 2 + i64::from(include_start) + i64::from(include_end);
        assert_eq!(count(true, false), 3);
        assert_eq!(count(false, false), 2);
        assert_eq!(count(true, true), 4);
    }
}
