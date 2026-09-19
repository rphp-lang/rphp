//! Native DateTime/DateTimeImmutable state and the absolute civil-time API.

use super::*;
use crate::value::NativeObjectState;

#[derive(Clone, Debug)]
pub(super) struct DateTimeState {
    pub(super) timestamp: i64,
    pub(super) microsecond: u32,
    pub(super) timezone: timezone::TimezoneDescription,
    pub(super) initialized: bool,
}

impl Default for DateTimeState {
    fn default() -> Self {
        Self {
            timestamp: 0,
            microsecond: 0,
            timezone: timezone::TimezoneDescription {
                kind: 3,
                name: "UTC".to_string(),
            },
            initialized: false,
        }
    }
}

impl NativeObjectState for DateTimeState {
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

fn now() -> (i64, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
            duration.subsec_micros(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
            if duration.subsec_micros() == 0 {
                (-seconds, 0)
            } else {
                (
                    seconds.saturating_neg().saturating_sub(1),
                    1_000_000 - duration.subsec_micros(),
                )
            }
        }
    }
}

fn parse_fraction(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = value.parse::<u32>().ok()?;
    Some(parsed * 10_u32.pow(6_u32.saturating_sub(value.len() as u32)))
}

fn parse_timestamp(value: &str) -> Option<(i64, u32)> {
    let number = value.parse::<f64>().ok()?;
    if !number.is_finite() || number < i64::MIN as f64 || number > i64::MAX as f64 {
        return None;
    }
    let seconds = number.floor();
    let microsecond = ((number - seconds) * 1_000_000.0).round();
    let mut seconds = seconds as i64;
    let mut microsecond = microsecond as u32;
    if microsecond == 1_000_000 {
        seconds = seconds.saturating_add(1);
        microsecond = 0;
    }
    Some((seconds, microsecond))
}

fn parse_date_prefix(value: &str) -> Option<(i64, i64, i64, usize)> {
    let bytes = value.as_bytes();
    let year_end = if bytes
        .first()
        .is_some_and(|byte| matches!(byte, b'+' | b'-'))
    {
        value[1..].find('-')? + 1
    } else {
        value.find('-')?
    };
    let month_end = value[year_end + 1..].find('-')? + year_end + 1;
    let end = month_end + 3;
    let year = value.get(..year_end)?.parse::<i64>().ok()?;
    let month = value.get(year_end + 1..month_end)?.parse::<i64>().ok()?;
    let day = value.get(month_end + 1..end)?.parse::<i64>().ok()?;
    Some((year, month, day, end))
}

fn parse_clock(value: &str) -> Option<(i64, i64, i64, u32, usize)> {
    let bytes = value.as_bytes();
    if bytes.len() < 5 || bytes.get(2) != Some(&b':') {
        return None;
    }
    let hour = value.get(..2)?.parse::<i64>().ok()?;
    let minute = value.get(3..5)?.parse::<i64>().ok()?;
    let mut second = 0;
    let mut microsecond = 0;
    let mut consumed = 5;
    if bytes.get(consumed) == Some(&b':') {
        second = value.get(consumed + 1..consumed + 3)?.parse::<i64>().ok()?;
        consumed += 3;
    }
    if bytes.get(consumed) == Some(&b'.') {
        let start = consumed + 1;
        consumed = start;
        while bytes.get(consumed).is_some_and(u8::is_ascii_digit) {
            consumed += 1;
        }
        microsecond = parse_fraction(value.get(start..consumed)?)?;
    }
    Some((hour, minute, second, microsecond, consumed))
}

fn parse_absolute(
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &ExecutorGlobals,
) -> Option<DateTimeState> {
    let input = input.trim();
    let fallback = supplied_timezone.unwrap_or_else(|| timezone::default_description(eg));
    if input.is_empty() || input.eq_ignore_ascii_case("now") {
        let (timestamp, microsecond) = now();
        return Some(DateTimeState {
            timestamp,
            microsecond,
            timezone: fallback,
            initialized: true,
        });
    }
    if let Some(timestamp) = input.strip_prefix('@').and_then(parse_timestamp) {
        return Some(DateTimeState {
            timestamp: timestamp.0,
            microsecond: timestamp.1,
            timezone: timezone::TimezoneDescription {
                kind: 1,
                name: "+00:00".to_string(),
            },
            initialized: true,
        });
    }
    if let Some(description) = timezone::parse_timezone(input) {
        let (timestamp, microsecond) = now();
        return Some(DateTimeState {
            timestamp,
            microsecond,
            timezone: description,
            initialized: true,
        });
    }

    let (year, month, day, date_end) = parse_date_prefix(input)?;
    let mut remainder = input.get(date_end..)?.trim_start();
    if let Some(value) = remainder.strip_prefix('T') {
        remainder = value;
    }
    let (hour, minute, second, microsecond, consumed) = if remainder.is_empty() {
        (0, 0, 0, 0, 0)
    } else {
        parse_clock(remainder)?
    };
    remainder = remainder.get(consumed..)?.trim();
    let description = if remainder.is_empty() {
        fallback
    } else if remainder == "Z" {
        timezone::TimezoneDescription {
            kind: 2,
            name: "Z".to_string(),
        }
    } else {
        timezone::parse_timezone(remainder)?
    };
    let local = super::normalized_timestamp(year, month, day, hour, minute, second);
    Some(DateTimeState {
        timestamp: timezone::description_local_to_utc(&description, local),
        microsecond,
        timezone: description,
        initialized: true,
    })
}

fn parse_timezone_argument(value: Option<&Value>) -> Option<timezone::TimezoneDescription> {
    let value = value?.dereferenced();
    if value.value_type() == ValueType::Null || value.value_type() == ValueType::Undef {
        return None;
    }
    timezone::object_description(value)
}

fn state_snapshot(value: &Value, eg: &mut ExecutorGlobals) -> Option<DateTimeState> {
    let state = value
        .as_object()
        .and_then(|object| object.native_object_state::<DateTimeState>().cloned());
    if state.as_ref().is_some_and(|state| state.initialized) {
        return state;
    }
    let class = value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTime".to_string());
    eg.exception = Some(crate::value::make_error_value(
        "DateObjectError",
        &format!(
            "Object of type {class} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    ));
    None
}

fn install_state(value: &Value, state: DateTimeState) -> bool {
    let Some(mut object) = value.as_object_mut() else {
        return false;
    };
    *object.native_object_state_mut::<DateTimeState>() = state;
    true
}

fn allocate(eg: &ExecutorGlobals, class_name: &str, state: DateTimeState) -> Option<Value> {
    let class = eg.find_class(class_name)?;
    let mut object = PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    );
    *object.native_object_state_mut::<DateTimeState>() = state;
    Some(Value::object(object))
}

fn state_format(state: &DateTimeState, format: &str) -> String {
    let (abbreviation, offset, is_dst) =
        timezone::description_state(&state.timezone, state.timestamp);
    crate::stdlib::format_php_date_with_microseconds(
        format,
        state.timestamp,
        state.microsecond,
        &state.timezone.name,
        &abbreviation,
        offset,
        is_dst,
    )
}

fn serialized_state(state: &DateTimeState) -> Value {
    let mut result = PhpArray::new();
    result.set_str("date", Value::string(state_format(state, "Y-m-d H:i:s.u")));
    result.set_str("timezone_type", Value::long(state.timezone.kind));
    result.set_str("timezone", Value::string(&state.timezone.name));
    Value::array(result)
}

pub(crate) fn debug_projection(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DateTimeState>()?;
    state.initialized.then(|| serialized_state(state))
}

fn malformed(eg: &mut ExecutorGlobals, input: &str) {
    eg.exception = Some(crate::value::make_error_value(
        "DateMalformedStringException",
        &format!("Failed to parse time string ({input}) at position 0: Could not parse '{input}'"),
    ));
}

fn construct(
    receiver: &Value,
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &mut ExecutorGlobals,
) -> bool {
    let Some(state) = parse_absolute(input, supplied_timezone, eg) else {
        malformed(eg, input);
        return false;
    };
    install_state(receiver, state)
}

fn create(
    class_name: &str,
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let state = parse_absolute(input, supplied_timezone, eg)?;
    allocate(eg, class_name, state)
}

fn mutate(
    receiver: &Value,
    eg: &mut ExecutorGlobals,
    action: impl FnOnce(&mut DateTimeState),
) -> Option<Value> {
    let class_name = receiver.as_object()?.class_name.to_string();
    let immutable = eg.class_is_a(&class_name, "DateTimeImmutable");
    let result = if immutable {
        Value::object(receiver.as_object()?.clone_for_php())
    } else {
        receiver.clone()
    };
    let mut object = result.as_object_mut()?;
    let state = object.native_object_state_mut::<DateTimeState>();
    if !state.initialized {
        drop(object);
        state_snapshot(receiver, eg)?;
        return None;
    }
    action(state);
    drop(object);
    Some(result)
}

fn local_parts(state: &DateTimeState) -> (i64, i64, i64, i64, i64, i64) {
    let offset = timezone::description_state(&state.timezone, state.timestamp).1;
    let (year, month, day, hour, minute, second, _, _) =
        super::unix_to_parts(state.timestamp.saturating_add(offset));
    (year, month, day, hour, minute, second)
}

fn set_local(
    state: &mut DateTimeState,
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) {
    let local = super::normalized_timestamp(year, month, day, hour, minute, second);
    state.timestamp = timezone::description_local_to_utc(&state.timezone, local);
}

fn iso_date(year: i64, week: i64, weekday: i64) -> (i64, i64, i64) {
    let january_fourth = super::parts_to_unix(year, 1, 4, 0, 0, 0);
    let (_, _, _, _, _, _, weekday_of_january_fourth, _) = super::unix_to_parts(january_fourth);
    let iso_weekday = if weekday_of_january_fourth == 0 {
        7
    } else {
        weekday_of_january_fourth
    };
    let target = january_fourth
        .saturating_sub((iso_weekday - 1).saturating_mul(86_400))
        .saturating_add((week - 1).saturating_mul(7 * 86_400))
        .saturating_add((weekday - 1).saturating_mul(86_400));
    let (year, month, day, _, _, _, _, _) = super::unix_to_parts(target);
    (year, month, day)
}

pub(crate) fn fn_date_time_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Undef)
        .and_then(Value::as_str)
        .unwrap_or("now");
    let timezone = parse_timezone_argument(arg_opt!(ed, 2));
    construct(arg!(ed, 0), input, timezone, eg);
    Ok(())
}

pub(crate) fn fn_date_time_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let format = arg_str!(ed, 1);
    ret!(rv, Value::string(state_format(&state, format.as_ref())));
}

pub(crate) fn fn_date_time_get_timestamp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, Value::long(state.timestamp));
}

pub(crate) fn fn_date_time_get_microsecond(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, Value::long(i64::from(state.microsecond)));
}

pub(crate) fn fn_date_time_get_offset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(
        rv,
        Value::long(timezone::description_state(&state.timezone, state.timestamp).1)
    );
}

pub(crate) fn fn_date_time_get_timezone(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, timezone::timezone_object(eg, state.timezone));
}

pub(crate) fn fn_date_time_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, serialized_state(&state));
}

pub(crate) fn fn_date_time_set_timestamp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let timestamp = arg_long!(ed, 1);
    let Some(result) = mutate(arg!(ed, 0), eg, |state| {
        state.timestamp = timestamp;
        state.microsecond = 0;
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set_timezone(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timezone) = timezone::object_description(arg!(ed, 1)) else {
        return Ok(());
    };
    let Some(result) = mutate(arg!(ed, 0), eg, |state| state.timezone = timezone) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set_date(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (year, month, day) = (arg_long!(ed, 1), arg_long!(ed, 2), arg_long!(ed, 3));
    let Some(result) = mutate(arg!(ed, 0), eg, |state| {
        let (_, _, _, hour, minute, second) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set_time(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let hour = arg_long!(ed, 1);
    let minute = arg_long!(ed, 2);
    let second = arg_opt!(ed, 3).and_then(Value::as_long).unwrap_or(0);
    let microsecond = arg_opt!(ed, 4).and_then(Value::as_long).unwrap_or(0);
    if !(0..=999_999).contains(&microsecond) {
        eg.exception = Some(crate::value::make_error_value(
            "DateRangeError",
            "DateTime::setTime(): Argument #4 ($microsecond) must be between 0 and 999999",
        ));
        return Ok(());
    }
    let Some(result) = mutate(arg!(ed, 0), eg, |state| {
        let (year, month, day, _, _, _) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
        state.microsecond = microsecond as u32;
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set_microsecond(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let microsecond = arg_long!(ed, 1);
    if !(0..=999_999).contains(&microsecond) {
        eg.exception = Some(crate::value::make_error_value(
            "DateRangeError",
            "DateTime::setMicrosecond(): Argument #1 ($microsecond) must be between 0 and 999999",
        ));
        return Ok(());
    }
    let Some(result) = mutate(arg!(ed, 0), eg, |state| {
        state.microsecond = microsecond as u32
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set_iso_date(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (year, week) = (arg_long!(ed, 1), arg_long!(ed, 2));
    let weekday = arg_opt!(ed, 3).and_then(Value::as_long).unwrap_or(1);
    let Some(result) = mutate(arg!(ed, 0), eg, |state| {
        let (year, month, day) = iso_date(year, week, weekday);
        let (_, _, _, hour, minute, second) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

fn create_from_timestamp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    class_name: &str,
) -> Result<(), VmError> {
    let value = arg!(ed, 1).dereferenced();
    let number = value
        .as_long()
        .map(|value| value as f64)
        .or_else(|| value.as_double())
        .unwrap_or_default();
    let Some((timestamp, microsecond)) = parse_timestamp(&number.to_string()) else {
        eg.exception = Some(crate::value::make_error_value(
            "DateRangeError",
            "DateTime::createFromTimestamp(): Argument #1 ($timestamp) is out of range",
        ));
        return Ok(());
    };
    let state = DateTimeState {
        timestamp,
        microsecond,
        timezone: timezone::TimezoneDescription {
            kind: 1,
            name: "+00:00".to_string(),
        },
        initialized: true,
    };
    if let Some(result) = allocate(eg, class_name, state) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_time_create_from_timestamp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_from_timestamp(ed, rv, eg, "DateTime")
}

pub(crate) fn fn_date_time_immutable_create_from_timestamp(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_from_timestamp(ed, rv, eg, "DateTimeImmutable")
}

pub(crate) fn fn_date_create(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_opt!(ed, 0).and_then(Value::as_str).unwrap_or("now");
    let timezone = parse_timezone_argument(arg_opt!(ed, 1));
    match create("DateTime", input, timezone, eg) {
        Some(result) => ret!(rv, result),
        None => ret!(rv, Value::bool(false)),
    }
}

pub(crate) fn fn_date_create_immutable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_opt!(ed, 0).and_then(Value::as_str).unwrap_or("now");
    let timezone = parse_timezone_argument(arg_opt!(ed, 1));
    match create("DateTimeImmutable", input, timezone, eg) {
        Some(result) => ret!(rv, result),
        None => ret!(rv, Value::bool(false)),
    }
}

pub(crate) fn fn_date_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let format = arg_str!(ed, 1);
    ret!(rv, Value::string(state_format(&state, format.as_ref())));
}

pub(crate) fn fn_date_timestamp_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, Value::long(state.timestamp));
}

pub(crate) fn fn_date_offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(
        rv,
        Value::long(timezone::description_state(&state.timezone, state.timestamp).1)
    );
}

pub(crate) fn fn_date_timezone_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = state_snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    ret!(rv, timezone::timezone_object(eg, state.timezone));
}

pub(crate) fn fn_date_timestamp_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let timestamp = arg_long!(ed, 1);
    let receiver = arg!(ed, 0).clone();
    let Some(result) = mutate(&receiver, eg, |state| {
        state.timestamp = timestamp;
        state.microsecond = 0;
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_timezone_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timezone) = timezone::object_description(arg!(ed, 1)) else {
        return Ok(());
    };
    let receiver = arg!(ed, 0).clone();
    let Some(result) = mutate(&receiver, eg, |state| state.timezone = timezone) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_date_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let (year, month, day) = (arg_long!(ed, 1), arg_long!(ed, 2), arg_long!(ed, 3));
    let receiver = arg!(ed, 0).clone();
    let Some(result) = mutate(&receiver, eg, |state| {
        let (_, _, _, hour, minute, second) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_time_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let (hour, minute) = (arg_long!(ed, 1), arg_long!(ed, 2));
    let second = arg_opt!(ed, 3).and_then(Value::as_long).unwrap_or(0);
    let microsecond = arg_opt!(ed, 4).and_then(Value::as_long).unwrap_or(0);
    let Some(result) = mutate(&receiver, eg, |state| {
        let (year, month, day, _, _, _) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
        state.microsecond = microsecond.clamp(0, 999_999) as u32;
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_date_iso_date_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let (year, week) = (arg_long!(ed, 1), arg_long!(ed, 2));
    let weekday = arg_opt!(ed, 3).and_then(Value::as_long).unwrap_or(1);
    let Some(result) = mutate(&receiver, eg, |state| {
        let (year, month, day) = iso_date(year, week, weekday);
        let (_, _, _, hour, minute, second) = local_parts(state);
        set_local(state, year, month, day, hour, minute, second);
    }) else {
        return Ok(());
    };
    ret!(rv, result);
}

pub(crate) fn fn_timezone_offset_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timezone) = timezone::object_description(arg!(ed, 0)) else {
        return Ok(());
    };
    let Some(datetime) = state_snapshot(arg!(ed, 1), eg) else {
        return Ok(());
    };
    ret!(
        rv,
        Value::long(timezone::description_state(&timezone, datetime.timestamp).1)
    );
}

pub(crate) fn fn_date_time_zone_get_offset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_timezone_offset_get(ed, rv, eg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_and_absolute_parsers_preserve_microseconds() {
        assert_eq!(parse_timestamp("0.125"), Some((0, 125_000)));
        assert_eq!(parse_timestamp("-0.125"), Some((-1, 875_000)));
        assert_eq!(parse_fraction("12"), Some(120_000));
    }

    #[test]
    fn iso_week_projection_matches_php_boundaries() {
        assert_eq!(iso_date(2009, 1, 1), (2008, 12, 29));
        assert_eq!(iso_date(2020, 53, 7), (2021, 1, 3));
    }
}
