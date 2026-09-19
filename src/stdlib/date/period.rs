//! DatePeriod construction, getters and deterministic iteration.

use super::{datetime, interval, parser, *};
use crate::value::NativeObjectState;

#[derive(Clone, Debug)]
pub(super) struct DatePeriodState {
    start: datetime::DateTimeState,
    start_is_null: bool,
    start_class: String,
    current: Option<datetime::DateTimeState>,
    end: Option<datetime::DateTimeState>,
    interval: interval::DateIntervalState,
    recurrences: Option<i64>,
    effective_recurrences: i64,
    include_start: bool,
    include_end: bool,
    initialized: bool,
}

impl Default for DatePeriodState {
    fn default() -> Self {
        Self {
            start: datetime::DateTimeState::default(),
            start_is_null: true,
            start_class: "DateTimeImmutable".to_string(),
            current: None,
            end: None,
            interval: interval::DateIntervalState::default(),
            recurrences: None,
            effective_recurrences: 0,
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

#[derive(Clone)]
struct DatePeriodIteratorState {
    owner: Value,
    terminal: datetime::DateTimeState,
}

impl Default for DatePeriodIteratorState {
    fn default() -> Self {
        Self {
            owner: Value::null(),
            terminal: datetime::DateTimeState::default(),
        }
    }
}

impl NativeObjectState for DatePeriodIteratorState {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        visit(&self.owner);
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        pending.push(std::mem::replace(&mut self.owner, Value::null()));
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
    let inheritance = (!class_name.eq_ignore_ascii_case("DatePeriod"))
        .then_some(" (inheriting DatePeriod)")
        .unwrap_or_default();
    eg.exception = Some(crate::value::make_error_value(
        "DateObjectError",
        &format!(
            "Object of type {class_name}{inheritance} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    ));
    None
}

fn initialized_state(value: &Value) -> Option<DatePeriodState> {
    value
        .as_object()?
        .native_object_state::<DatePeriodState>()
        .filter(|state| state.initialized)
        .cloned()
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

fn generated_values(
    state: &DatePeriodState,
    eg: &ExecutorGlobals,
) -> (PhpArray, datetime::DateTimeState) {
    let mut result = PhpArray::new();
    let mut current = state.start.clone();
    let mut emitted = 0_i64;
    let recurrence_limit = state.recurrences.map(|recurrences| {
        recurrences + i64::from(state.include_start) + i64::from(state.include_end)
    });
    if !state.include_start && !advance(&mut current, &state.interval) {
        return (result, current);
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
    (result, current)
}

fn iterator_from_period(
    eg: &ExecutorGlobals,
    array: PhpArray,
    owner: Value,
    terminal: datetime::DateTimeState,
) -> Option<Value> {
    let iterator = super::super::builtin_classes::array_object::iterator_from_array(eg, array)?;
    if let Some(mut object) = iterator.as_object_mut() {
        *object.native_object_state_mut::<DatePeriodIteratorState>() =
            DatePeriodIteratorState { owner, terminal };
    }
    Some(iterator)
}

pub(crate) fn iterator_projection(
    receiver: &Value,
    projected: Option<&Value>,
    exhausted: bool,
    eg: &ExecutorGlobals,
) {
    let marker = receiver.as_object().and_then(|object| {
        object
            .native_object_state::<DatePeriodIteratorState>()
            .cloned()
    });
    let Some(marker) = marker else {
        return;
    };
    let current = projected
        .and_then(datetime::initialized_state)
        .or_else(|| exhausted.then_some(marker.terminal));
    let Some(current) = current else {
        return;
    };
    let Some(mut owner) = marker.owner.as_object_mut() else {
        return;
    };
    let Some(state) = owner.native_object_state::<DatePeriodState>().cloned() else {
        return;
    };
    let visible = state_object(eg, &state.start_class, current.clone());
    owner.set_property("current", visible);
    owner.native_object_state_mut::<DatePeriodState>().current = Some(current);
}

pub(crate) fn clone_iterator_value(receiver: &Value, value: Value, eg: &ExecutorGlobals) -> Value {
    let is_period_iterator = receiver.as_object().is_some_and(|object| {
        object
            .native_object_state::<DatePeriodIteratorState>()
            .is_some()
    });
    if !is_period_iterator {
        return value;
    }
    let class_name = value
        .as_object()
        .map(|object| object.class_name.to_string());
    let state = datetime::initialized_state(&value);
    match (class_name, state) {
        (Some(class_name), Some(state)) => {
            datetime::allocate(eg, &class_name, state).unwrap_or(value)
        }
        _ => value,
    }
}

pub(crate) fn iterator_disallows_references(receiver: &Value) -> bool {
    receiver.as_object().is_some_and(|object| {
        object
            .native_object_state::<DatePeriodIteratorState>()
            .is_some()
    })
}

fn serialized(state: &DatePeriodState, eg: &ExecutorGlobals) -> Value {
    let mut result = PhpArray::new();
    result.set_str(
        "start",
        if state.start_is_null {
            Value::null()
        } else {
            state_object(eg, &state.start_class, state.start.clone())
        },
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
    result.set_str("recurrences", Value::long(state.effective_recurrences));
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

const PERIOD_CONSTRUCTOR_SIGNATURE: &str = "DatePeriod::__construct() accepts (DateTimeInterface, DateInterval, int [, int]), or (DateTimeInterface, DateInterval, DateTime [, int]), or (string [, int]) as arguments";
const PERIOD_RECURRENCE_RANGE: i64 = 2_147_483_640;

fn invalid_constructor_arguments(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        PERIOD_CONSTRUCTOR_SIGNATURE,
    ));
}

fn invalid_recurrence(eg: &mut ExecutorGlobals, function: &str) {
    eg.exception = Some(crate::value::make_error_value(
        "DateMalformedPeriodStringException",
        &format!(
            "{function}(): Recurrence count must be greater or equal to 1 and lower than {PERIOD_RECURRENCE_RANGE}"
        ),
    ));
}

fn malformed_iso_period(eg: &mut ExecutorGlobals, function: &str, specification: &str) {
    let slash_count = specification.matches('/').count();
    let message = if specification.starts_with('R') && slash_count == 0 {
        format!("{function}(): ISO interval must contain a start date, \"{specification}\" given")
    } else if specification.starts_with('R') && slash_count == 1 {
        format!("{function}(): ISO interval must contain an interval, \"{specification}\" given")
    } else if !specification.starts_with('R') && slash_count == 1 {
        format!(
            "{function}(): Recurrence count must be greater or equal to 1 and lower than {PERIOD_RECURRENCE_RANGE}"
        )
    } else {
        format!("Unknown or bad format ({specification})")
    };
    eg.exception = Some(crate::value::make_error_value(
        "DateMalformedPeriodStringException",
        &message,
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
        start_is_null: false,
        start_class: "DateTimeImmutable".to_string(),
        current: None,
        end: None,
        interval,
        recurrences: recurrence,
        effective_recurrences: recurrence? + i64::from(include_start) + i64::from(include_end),
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
    let Some(first) = arg_opt!(ed, 1).map(Value::dereferenced) else {
        invalid_constructor_arguments(eg);
        return Ok(());
    };
    if matches!(first.value_type(), ValueType::String | ValueType::Null) {
        if arg_opt!(ed, 3).is_some()
            || arg_opt!(ed, 4).is_some()
            || arg_opt!(ed, 2).is_some_and(|value| {
                !matches!(
                    value.dereferenced().value_type(),
                    ValueType::Long | ValueType::Double | ValueType::True | ValueType::False
                )
            })
        {
            invalid_constructor_arguments(eg);
            return Ok(());
        }
        let specification = if first.value_type() == ValueType::Null {
            super::super::report_internal_deprecation(
                eg,
                ed,
                "DatePeriod::__construct(): Passing null to parameter #1 ($start) of type string is deprecated",
            )?;
            if eg.exception.is_some() {
                return Ok(());
            }
            ""
        } else {
            first.as_str().unwrap_or_default()
        };
        super::super::report_internal_deprecation(
            eg,
            ed,
            "Calling DatePeriod::__construct(string $isostr, int $options = 0) is deprecated, use DatePeriod::createFromISO8601String() instead",
        )?;
        if eg.exception.is_some() {
            return Ok(());
        }
        let Some(state) = from_iso(specification, arg_opt!(ed, 2), eg) else {
            if specification.is_empty() {
                malformed_period(eg, specification);
            } else {
                malformed_iso_period(eg, "DatePeriod::__construct", specification);
            }
            return Ok(());
        };
        install(arg!(ed, 0), state, eg);
        return Ok(());
    }
    let first_class = first
        .as_object()
        .map(|object| object.class_name.to_string());
    if !first_class
        .as_deref()
        .is_some_and(|class_name| eg.class_is_a(class_name, "DateTimeInterface"))
        || arg_opt!(ed, 2).is_none()
        || arg_opt!(ed, 3).is_none()
    {
        invalid_constructor_arguments(eg);
        return Ok(());
    }
    let Some(start) = datetime::initialized_state(first) else {
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            "Object of type DateTimeInterface has not been correctly initialized by calling parent::__construct() in its constructor",
        ));
        return Ok(());
    };
    let start_class = first_class.unwrap_or_else(|| "DateTimeImmutable".to_string());
    let interval_value = arg!(ed, 2).dereferenced();
    let interval_class = interval_value
        .as_object()
        .map(|object| object.class_name.to_string());
    if !interval_class
        .as_deref()
        .is_some_and(|class_name| eg.class_is_a(class_name, "DateInterval"))
    {
        invalid_constructor_arguments(eg);
        return Ok(());
    }
    let Some(interval) = interval::snapshot(interval_value, eg) else {
        return Ok(());
    };
    let interval = interval::for_period(interval);
    let third = arg!(ed, 3).dereferenced();
    let (recurrences, end) = if let Some(recurrences) = third.as_long() {
        if !(1..PERIOD_RECURRENCE_RANGE).contains(&recurrences) {
            invalid_recurrence(eg, "DatePeriod::__construct");
            return Ok(());
        }
        (Some(recurrences), None)
    } else {
        let end_class = third
            .as_object()
            .map(|object| object.class_name.to_string());
        if !end_class
            .as_deref()
            .is_some_and(|class_name| eg.class_is_a(class_name, "DateTimeInterface"))
        {
            invalid_constructor_arguments(eg);
            return Ok(());
        }
        let Some(end) = datetime::initialized_state(third) else {
            eg.exception = Some(crate::value::make_error_value(
                "DateObjectError",
                "Object of type DateTimeInterface has not been correctly initialized by calling parent::__construct() in its constructor",
            ));
            return Ok(());
        };
        (None, Some(end))
    };
    let (include_start, include_end) = options(arg_opt!(ed, 4));
    let effective_recurrences =
        recurrences.unwrap_or(0) + i64::from(include_start) + i64::from(include_end);
    if recurrences.is_some() && effective_recurrences >= PERIOD_RECURRENCE_RANGE {
        eg.exception = Some(crate::value::make_error_value(
            "DateMalformedStringException",
            &format!(
                "DatePeriod::__construct(): Recurrence count must be greater or equal to 1 and lower than {PERIOD_RECURRENCE_RANGE} (including options)"
            ),
        ));
        return Ok(());
    }
    install(
        arg!(ed, 0),
        DatePeriodState {
            start,
            start_is_null: false,
            start_class,
            current: None,
            end,
            interval,
            recurrences,
            effective_recurrences,
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
    let class_name = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .filter(|class_name| eg.class_is_a(class_name, "DatePeriod"))
        .unwrap_or("DatePeriod")
        .to_string();
    if eg
        .find_class(&class_name)
        .is_some_and(|class| class.is_abstract)
    {
        eg.exception = Some(crate::value::make_error_value(
            "Error",
            &format!("Cannot instantiate abstract class {class_name}"),
        ));
        return Ok(());
    }
    let specification = arg_str!(ed, 1);
    let Some(state) = from_iso(specification.as_ref(), arg_opt!(ed, 2), eg) else {
        malformed_iso_period(
            eg,
            "DatePeriod::createFromISO8601String",
            specification.as_ref(),
        );
        return Ok(());
    };
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
    let Some(state) = initialized_state(arg!(ed, 0)) else {
        ret!(rv, Value::null());
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
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = initialized_state(arg!(ed, 0)) else {
        ret!(rv, Value::null());
    };
    ret!(rv, state.recurrences.map_or_else(Value::null, Value::long));
}

pub(crate) fn fn_date_period_get_iterator(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        if eg.exception.is_some() {
            eg.exception = Some(crate::value::make_error_value(
                "DateObjectError",
                "Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor",
            ));
        }
        return Ok(());
    };
    let (values, terminal) = generated_values(&state, eg);
    if let Some(iterator) = iterator_from_period(eg, values, arg!(ed, 0).clone(), terminal) {
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

fn serialized_core(
    data: &PhpArray,
    include_start: bool,
    include_end: bool,
) -> Option<DatePeriodState> {
    let start_value = data.get_str("start")?.dereferenced();
    let start_is_null = start_value.value_type() == ValueType::Null;
    let (start, start_class) = if start_is_null {
        (
            datetime::DateTimeState::default(),
            "DateTimeImmutable".to_string(),
        )
    } else {
        if start_value.value_type() != ValueType::Object {
            return None;
        }
        (
            datetime::initialized_state(start_value)?,
            start_value
                .as_object()
                .map(|object| object.class_name.to_string())
                .unwrap_or_else(|| "DateTimeImmutable".to_string()),
        )
    };
    let interval_value = data.get_str("interval")?.dereferenced();
    if interval_value.value_type() != ValueType::Object {
        return None;
    }
    let interval = interval::initialized_state(interval_value)?;
    let optional_datetime = |name: &str| -> Option<Option<datetime::DateTimeState>> {
        let value = data.get_str(name)?.dereferenced();
        match value.value_type() {
            ValueType::Null => Some(None),
            ValueType::Object => datetime::initialized_state(value).map(Some),
            _ => None,
        }
    };
    let current = optional_datetime("current")?;
    let end = optional_datetime("end")?;
    if start_is_null && (current.is_some() || end.is_some()) {
        return None;
    }
    let effective_recurrences = data.get_str("recurrences")?;
    let effective_recurrence_value = effective_recurrences.dereferenced();
    let effective_recurrence = match effective_recurrence_value.value_type() {
        ValueType::Null => None,
        ValueType::Long => effective_recurrence_value.as_long(),
        _ => return None,
    };
    if effective_recurrence.is_some_and(|value| value < 0) {
        return None;
    }
    let recurrences = if end.is_some() || effective_recurrence_value.value_type() == ValueType::Null
    {
        None
    } else {
        let effective = effective_recurrence?;
        let recurrences = effective
            .saturating_sub(i64::from(include_start))
            .saturating_sub(i64::from(include_end));
        (recurrences >= 0).then_some(recurrences)?;
        Some(recurrences)
    };
    Some(DatePeriodState {
        start,
        start_is_null,
        start_class,
        current,
        end,
        interval,
        recurrences,
        effective_recurrences: effective_recurrence.unwrap_or(0),
        include_start,
        include_end,
        initialized: !start_is_null,
    })
}

fn serialized_flag(data: &PhpArray, name: &str) -> Option<bool> {
    let value = data.get_str(name)?.dereferenced();
    matches!(value.value_type(), ValueType::True | ValueType::False).then(|| value.is_truthy())
}

fn state_from_array(data: &PhpArray, _eg: &mut ExecutorGlobals) -> Option<DatePeriodState> {
    let include_start = serialized_flag(data, "include_start_date")?;
    let include_end = serialized_flag(data, "include_end_date")?;
    serialized_core(data, include_start, include_end)
}

pub(crate) fn fn_date_period_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let previous = initialized_state(arg!(ed, 0)).unwrap_or_default();
    let Some(mut state) = serialized_core(&data, previous.include_start, previous.include_end)
    else {
        if eg.exception.is_none() {
            invalid_serialization(eg);
        }
        return Ok(());
    };
    let include_start = serialized_flag(&data, "include_start_date");
    let include_end = serialized_flag(&data, "include_end_date");
    if include_start.is_none() || include_end.is_none() {
        install(arg!(ed, 0), state, eg);
        invalid_serialization(eg);
        return Ok(());
    }
    state.include_start = include_start.unwrap_or_default();
    state.include_end = include_end.unwrap_or_default();
    if state.end.is_none() {
        state.recurrences = Some(
            state
                .effective_recurrences
                .saturating_sub(i64::from(state.include_start))
                .saturating_sub(i64::from(state.include_end)),
        );
    }
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
