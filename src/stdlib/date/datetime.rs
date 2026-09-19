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

pub(super) fn now() -> (i64, u32) {
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

pub(super) fn parse_fraction(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = value.parse::<u32>().ok()?;
    Some(parsed * 10_u32.pow(6_u32.saturating_sub(value.len() as u32)))
}

pub(super) fn parse_timestamp(value: &str) -> Option<(i64, u32)> {
    let number = value.parse::<f64>().ok()?;
    if !number.is_finite() || number < i64::MIN as f64 || number >= i64::MAX as f64 {
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

pub(super) fn parse_date_prefix(value: &str) -> Option<(i64, i64, i64, usize)> {
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

pub(super) fn parse_absolute(
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

pub(super) fn parse_timezone_argument(
    value: Option<&Value>,
) -> Option<timezone::TimezoneDescription> {
    let value = value?.dereferenced();
    if value.value_type() == ValueType::Null || value.value_type() == ValueType::Undef {
        return None;
    }
    timezone::object_description(value)
}

pub(super) fn state_snapshot(value: &Value, eg: &mut ExecutorGlobals) -> Option<DateTimeState> {
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
    let base = if eg.class_is_a(&class, "DateTimeImmutable") {
        "DateTimeImmutable"
    } else {
        "DateTime"
    };
    let inheritance = (!class.eq_ignore_ascii_case(base))
        .then(|| format!(" (inheriting {base})"))
        .unwrap_or_default();
    eg.exception = Some(crate::value::make_error_value(
        "DateObjectError",
        &format!(
            "Object of type {class}{inheritance} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    ));
    None
}

pub(super) fn install_state(value: &Value, state: DateTimeState) -> bool {
    let Some(mut object) = value.as_object_mut() else {
        return false;
    };
    *object.native_object_state_mut::<DateTimeState>() = state;
    true
}

pub(super) fn allocate(
    eg: &ExecutorGlobals,
    class_name: &str,
    state: DateTimeState,
) -> Option<Value> {
    let class = eg.find_class(class_name)?;
    let mut object = PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    );
    *object.native_object_state_mut::<DateTimeState>() = state;
    Some(Value::object(object))
}

fn called_date_class(ed: *mut ExecuteData, eg: &ExecutorGlobals, base_class: &str) -> String {
    crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .filter(|class_name| eg.class_is_a(class_name, base_class))
        .unwrap_or(base_class)
        .to_string()
}

pub(super) fn state_format(state: &DateTimeState, format: &str) -> String {
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

pub(super) fn serialized_state(state: &DateTimeState) -> Value {
    let mut result = PhpArray::new();
    result.set_str("date", Value::string(state_format(state, "Y-m-d H:i:s.u")));
    result.set_str("timezone_type", Value::long(state.timezone.kind));
    result.set_str("timezone", Value::string(&state.timezone.name));
    Value::array(result)
}

const SERIALIZED_KEYS: [&str; 3] = ["date", "timezone_type", "timezone"];

pub(crate) fn debug_projection(value: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DateTimeState>()?;
    state.initialized.then(|| {
        let mut result = super::custom_properties(value, &SERIALIZED_KEYS, eg);
        let serialized = serialized_state(state);
        let native = serialized
            .as_array()
            .expect("DateTime serialized state is an array");
        for (key, value) in native.iter() {
            result.set(key, value.clone_for_php_storage());
        }
        Value::array(result)
    })
}

pub(crate) fn comparison(left: &Value, right: &Value) -> Option<i32> {
    let left = left
        .as_object()?
        .native_object_state::<DateTimeState>()?
        .clone();
    let right = right
        .as_object()?
        .native_object_state::<DateTimeState>()?
        .clone();
    if !left.initialized || !right.initialized {
        return None;
    }
    Some(
        match (left.timestamp, left.microsecond).cmp(&(right.timestamp, right.microsecond)) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        },
    )
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
    match super::parser::parse_datetime(input, supplied_timezone, None, eg) {
        Ok(parsed) => {
            eg.set_date_parse_diagnostics(parsed.diagnostics);
            install_state(receiver, parsed.state)
        }
        Err(diagnostics) => {
            eg.set_date_parse_diagnostics(diagnostics);
            malformed(eg, input);
            false
        }
    }
}

fn create(
    class_name: &str,
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    match super::parser::parse_datetime(input, supplied_timezone, None, eg) {
        Ok(parsed) => {
            eg.set_date_parse_diagnostics(parsed.diagnostics);
            allocate(eg, class_name, parsed.state)
        }
        Err(diagnostics) => {
            eg.set_date_parse_diagnostics(diagnostics);
            None
        }
    }
}

pub(super) fn mutate(
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

pub(super) fn local_parts(state: &DateTimeState) -> (i64, i64, i64, i64, i64, i64) {
    let offset = timezone::description_state(&state.timezone, state.timestamp).1;
    let (year, month, day, hour, minute, second, _, _) =
        super::unix_to_parts(state.timestamp.saturating_add(offset));
    (year, month, day, hour, minute, second)
}

pub(super) fn set_local(
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
    let mut result = serialized_state(&state)
        .as_array()
        .expect("DateTime serialized state is an array")
        .clone();
    super::append_custom_properties(&mut result, arg!(ed, 0), &SERIALIZED_KEYS, eg);
    ret!(rv, Value::array(result));
}

fn invalid_serialization(eg: &mut ExecutorGlobals, class_name: &str) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        &format!("Invalid serialization data for {class_name} object"),
    ));
}

fn serialized_fields_state(
    date: Option<&Value>,
    timezone_type: Option<&Value>,
    timezone_name: Option<&Value>,
    eg: &mut ExecutorGlobals,
) -> Option<DateTimeState> {
    let date = date?.dereferenced().as_str()?;
    let timezone_type = timezone_type?.dereferenced().as_long()?;
    let timezone_name = timezone_name?.dereferenced().as_str()?;
    if !(1..=3).contains(&timezone_type) {
        return None;
    }
    let mut timezone = timezone::parse_timezone(timezone_name)?;
    timezone.kind = timezone_type;
    parse_absolute(date, Some(timezone), eg)
}

fn unserialize_into(receiver: &Value, data: &PhpArray, eg: &mut ExecutorGlobals) -> bool {
    let class_name = receiver
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTime".to_string());
    let Some(state) = serialized_fields_state(
        data.get_str("date"),
        data.get_str("timezone_type"),
        data.get_str("timezone"),
        eg,
    ) else {
        invalid_serialization(eg, &class_name);
        return false;
    };
    if !install_state(receiver, state) {
        return false;
    }
    super::restore_custom_properties(receiver, data, &SERIALIZED_KEYS, eg)
}

pub(crate) fn fn_date_time_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    unserialize_into(arg!(ed, 0), &data, eg);
    Ok(())
}

pub(crate) fn fn_date_time_wakeup(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let Some(object) = receiver.as_object() else {
        return Ok(());
    };
    let class_name = object.class_name.to_string();
    let date = object.get_property("date").cloned();
    let timezone_type = object.get_property("timezone_type").cloned();
    let timezone_name = object.get_property("timezone").cloned();
    drop(object);
    let Some(state) = serialized_fields_state(
        date.as_ref(),
        timezone_type.as_ref(),
        timezone_name.as_ref(),
        eg,
    ) else {
        invalid_serialization(eg, &class_name);
        return Ok(());
    };
    install_state(&receiver, state);
    Ok(())
}

fn set_state(
    class_name: &str,
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let Some(state) = serialized_fields_state(
        data.get_str("date"),
        data.get_str("timezone_type"),
        data.get_str("timezone"),
        eg,
    ) else {
        invalid_serialization(eg, class_name);
        return Ok(());
    };
    let class_name = called_date_class(ed, eg, class_name);
    if let Some(value) = allocate(eg, &class_name, state) {
        super::restore_custom_properties(&value, &data, &SERIALIZED_KEYS, eg);
        ret!(rv, value);
    }
    Ok(())
}

pub(crate) fn fn_date_time_set_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_state("DateTime", ed, rv, eg)
}

pub(crate) fn fn_date_time_immutable_set_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_state("DateTimeImmutable", ed, rv, eg)
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
        let supplied = if number.is_nan() {
            "NAN".to_string()
        } else if number == f64::INFINITY {
            "INF".to_string()
        } else if number == f64::NEG_INFINITY {
            "-INF".to_string()
        } else {
            number.to_string()
        };
        eg.exception = Some(crate::value::make_error_value(
            "DateRangeError",
            &format!(
                "{class_name}::createFromTimestamp(): Argument #1 ($timestamp) must be a finite number between {} and {}.999999, {supplied} given",
                i64::MIN,
                i64::MAX,
            ),
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
    let class_name = called_date_class(ed, eg, class_name);
    if let Some(result) = allocate(eg, &class_name, state) {
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
    let timezone_value = arg!(ed, 0).dereferenced();
    let timezone_class = timezone_value
        .as_object()
        .map(|object| object.class_name.to_string());
    if !timezone_class
        .as_deref()
        .is_some_and(|class| eg.class_is_a(class, "DateTimeZone"))
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "timezone_offset_get(): Argument #1 ($object) must be of type DateTimeZone, {} given",
                timezone_value.diagnostic_type_name()
            ),
        ));
        return Ok(());
    }
    let datetime_value = arg!(ed, 1).dereferenced();
    let datetime_class = datetime_value
        .as_object()
        .map(|object| object.class_name.to_string());
    if !datetime_class
        .as_deref()
        .is_some_and(|class| eg.class_is_a(class, "DateTimeInterface"))
    {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "timezone_offset_get(): Argument #2 ($datetime) must be of type DateTimeInterface, {} given",
                datetime_value.diagnostic_type_name()
            ),
        ));
        return Ok(());
    }
    let Some(timezone) = timezone::checked_object_description(timezone_value, eg) else {
        return Ok(());
    };
    let Some(datetime) = state_snapshot(datetime_value, eg) else {
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

fn create_from_format(
    class_name: &str,
    format: &str,
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    match super::parser::parse_from_format(format, input, supplied_timezone, eg) {
        Ok(parsed) => {
            eg.set_date_parse_diagnostics(parsed.diagnostics);
            allocate(eg, class_name, parsed.state)
        }
        Err(diagnostics) => {
            eg.set_date_parse_diagnostics(diagnostics);
            None
        }
    }
}

fn create_from_format_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    class_name: &str,
) -> Result<(), VmError> {
    let format = arg_str!(ed, 1);
    let input = arg_str!(ed, 2);
    let timezone = parse_timezone_argument(arg_opt!(ed, 3));
    let class_name = called_date_class(ed, eg, class_name);
    let result = create_from_format(&class_name, format.as_ref(), input.as_ref(), timezone, eg)
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

pub(crate) fn fn_date_time_create_from_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_from_format_handler(ed, rv, eg, "DateTime")
}

pub(crate) fn fn_date_time_immutable_create_from_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_from_format_handler(ed, rv, eg, "DateTimeImmutable")
}

pub(crate) fn fn_date_create_from_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let format = arg_str!(ed, 0);
    let input = arg_str!(ed, 1);
    let timezone = parse_timezone_argument(arg_opt!(ed, 2));
    let result = create_from_format("DateTime", format.as_ref(), input.as_ref(), timezone, eg)
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

pub(crate) fn fn_date_create_immutable_from_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let format = arg_str!(ed, 0);
    let input = arg_str!(ed, 1);
    let timezone = parse_timezone_argument(arg_opt!(ed, 2));
    let result = create_from_format(
        "DateTimeImmutable",
        format.as_ref(),
        input.as_ref(),
        timezone,
        eg,
    )
    .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

pub(crate) fn fn_date_get_last_errors(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = eg
        .date_parse_diagnostics()
        .map(super::parser::diagnostics_value)
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

fn modify_value(
    receiver: &Value,
    input: &str,
    throw_on_error: bool,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let base = state_snapshot(receiver, eg)?;
    match super::parser::parse_datetime(input, None, Some(&base), eg) {
        Ok(parsed) => {
            eg.set_date_parse_diagnostics(parsed.diagnostics);
            mutate(receiver, eg, |state| *state = parsed.state)
        }
        Err(diagnostics) => {
            eg.set_date_parse_diagnostics(diagnostics);
            if throw_on_error {
                malformed(eg, input);
            }
            None
        }
    }
}

pub(crate) fn fn_date_time_modify(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 1);
    if let Some(result) = modify_value(arg!(ed, 0), input.as_ref(), true, eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_modify(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let input = arg_str!(ed, 1);
    let result =
        modify_value(&receiver, input.as_ref(), false, eg).unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

fn add_or_subtract(
    receiver: &Value,
    interval_value: &Value,
    subtract: bool,
    function: &str,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let interval = super::interval::snapshot(interval_value, eg)?;
    if subtract && super::interval::subtraction_is_unsupported(&interval) {
        eg.exception = Some(crate::value::make_error_value(
            "DateInvalidOperationException",
            &format!(
                "{function}(): Only non-special relative time specifications are supported for subtraction"
            ),
        ));
        return None;
    }
    mutate(receiver, eg, |state| {
        super::interval::apply_to_datetime(state, &interval, subtract)
    })
}

pub(crate) fn fn_date_time_add(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(result) = add_or_subtract(arg!(ed, 0), arg!(ed, 1), false, "DateTime::add", eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_time_sub(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class = arg!(ed, 0)
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTime".to_string());
    if let Some(result) =
        add_or_subtract(arg!(ed, 0), arg!(ed, 1), true, &format!("{class}::sub"), eg)
    {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_add(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    if let Some(result) = add_or_subtract(&receiver, arg!(ed, 1), false, "date_add", eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_sub(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    if let Some(result) = add_or_subtract(&receiver, arg!(ed, 1), true, "date_sub", eg) {
        ret!(rv, result);
    }
    Ok(())
}

fn diff_values(
    base: &Value,
    target: &Value,
    absolute: bool,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let base = state_snapshot(base, eg)?;
    let target = state_snapshot(target, eg)?;
    super::interval::allocate(eg, super::interval::difference(&base, &target, absolute))
}

pub(crate) fn fn_date_time_diff(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let absolute = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    if let Some(result) = diff_values(arg!(ed, 0), arg!(ed, 1), absolute, eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_diff(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let absolute = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    if let Some(result) = diff_values(arg!(ed, 0), arg!(ed, 1), absolute, eg) {
        ret!(rv, result);
    }
    Ok(())
}

fn parse_projection(
    parsed: Result<super::parser::ParsedDateTime, crate::runtime::DateParseDiagnostics>,
) -> Value {
    let (state, diagnostics, relative, fields) = match parsed {
        Ok(parsed) => (
            Some(parsed.state),
            parsed.diagnostics,
            parsed.relative,
            parsed.fields,
        ),
        Err(diagnostics) => (
            None,
            diagnostics,
            None,
            super::parser::ParsedFields::default(),
        ),
    };
    let mut result = PhpArray::new();
    if relative.is_some() {
        for name in [
            "year", "month", "day", "hour", "minute", "second", "fraction",
        ] {
            result.set_str(name, Value::bool(false));
        }
    } else if let Some(state) = state.as_ref() {
        let (year, month, day, hour, minute, second) = local_parts(state);
        for (name, value, present) in [
            ("year", year, fields.year),
            ("month", month, fields.month),
            ("day", day, fields.day),
            ("hour", hour, fields.hour),
            ("minute", minute, fields.minute),
            ("second", second, fields.second),
        ] {
            result.set_str(
                name,
                if present {
                    Value::long(value)
                } else {
                    Value::bool(false)
                },
            );
        }
        result.set_str(
            "fraction",
            if fields.fraction {
                Value::double(f64::from(state.microsecond) / 1_000_000.0)
            } else {
                Value::bool(false)
            },
        );
    } else {
        for name in [
            "year", "month", "day", "hour", "minute", "second", "fraction",
        ] {
            result.set_str(name, Value::bool(false));
        }
    }
    let diagnostics_value = super::parser::diagnostics_value(&diagnostics);
    if let Some(array) = diagnostics_value.as_array() {
        for (key, value) in array.iter() {
            match key {
                ArrayKey::Int(index) => result.set_int(index, value.clone()),
                ArrayKey::String(name) => result.set_str(&name, value.clone()),
            }
        }
    }
    if let Some(relative) = relative {
        result.set_str("is_localtime", Value::bool(false));
        let mut relative_value = PhpArray::new();
        for (name, value) in [
            ("year", relative.years),
            ("month", relative.months),
            ("day", relative.days),
            ("hour", relative.hours),
            ("minute", relative.minutes),
            ("second", relative.seconds),
        ] {
            relative_value.set_str(name, Value::long(value));
        }
        result.set_str("relative", Value::array(relative_value));
    } else if let Some(state) = state {
        result.set_str("is_localtime", Value::bool(fields.timezone));
        if fields.timezone {
            result.set_str("zone_type", Value::long(state.timezone.kind));
        }
        match (fields.timezone, state.timezone.kind) {
            (true, 1) => {
                result.set_str(
                    "zone",
                    Value::long(timezone::description_state(&state.timezone, state.timestamp).1),
                );
                result.set_str("is_dst", Value::bool(false));
            }
            (true, 2) => {
                let (_, offset, is_dst) =
                    timezone::description_state(&state.timezone, state.timestamp);
                result.set_str("zone", Value::long(offset));
                result.set_str("is_dst", Value::bool(is_dst));
                result.set_str("tz_abbr", Value::string(&state.timezone.name));
            }
            (true, 3) => result.set_str("tz_id", Value::string(&state.timezone.name)),
            _ => {}
        }
    } else {
        result.set_str("is_localtime", Value::bool(true));
        result.set_str("zone_type", Value::long(0));
    }
    Value::array(result)
}

pub(crate) fn fn_date_parse(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 0);
    let result = parse_projection(super::parser::parse_datetime(
        input.as_ref(),
        None,
        None,
        eg,
    ));
    ret!(rv, result)
}

pub(crate) fn fn_date_parse_from_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let format = arg_str!(ed, 0);
    let input = arg_str!(ed, 1);
    let result = parse_projection(super::parser::parse_from_format(
        format.as_ref(),
        input.as_ref(),
        None,
        eg,
    ));
    ret!(rv, result)
}

pub(crate) fn fn_strtotime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 0);
    let base_timestamp = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(Value::as_long)
        .unwrap_or_else(super::current_timestamp);
    let base = DateTimeState {
        timestamp: base_timestamp,
        microsecond: 0,
        timezone: timezone::default_description(eg),
        initialized: true,
    };
    let result = match super::parser::parse_datetime(input.as_ref(), None, Some(&base), eg) {
        Ok(parsed) => {
            eg.set_date_parse_diagnostics(parsed.diagnostics);
            Value::long(parsed.state.timestamp)
        }
        Err(diagnostics) => {
            eg.set_date_parse_diagnostics(diagnostics);
            Value::bool(false)
        }
    };
    ret!(rv, result)
}

fn create_from_interface(
    source: &Value,
    class_name: &str,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Option<Value> {
    let state = state_snapshot(source, eg)?;
    let class_name = called_date_class(ed, eg, class_name);
    allocate(eg, &class_name, state)
}

fn require_date_source(
    source: &Value,
    required_class: &str,
    method: &str,
    eg: &mut ExecutorGlobals,
) -> bool {
    let supplied_class = source
        .dereferenced()
        .as_object()
        .map(|object| object.class_name.to_string());
    if supplied_class
        .as_deref()
        .is_some_and(|class_name| eg.class_is_a(class_name, required_class))
    {
        return true;
    }
    let supplied = supplied_class.unwrap_or_else(|| source.diagnostic_type_name().to_string());
    eg.exception = Some(crate::value::make_error_value(
        "TypeError",
        &format!(
            "{method}(): Argument #1 ($object) must be of type {required_class}, {supplied} given"
        ),
    ));
    false
}

pub(crate) fn fn_date_time_create_from_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(result) = create_from_interface(arg!(ed, 1), "DateTime", ed, eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_time_immutable_create_from_interface(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(result) = create_from_interface(arg!(ed, 1), "DateTimeImmutable", ed, eg) {
        ret!(rv, result);
    }
    Ok(())
}

pub(crate) fn fn_date_time_create_from_immutable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !require_date_source(
        arg!(ed, 1),
        "DateTimeImmutable",
        "DateTime::createFromImmutable",
        eg,
    ) {
        return Ok(());
    }
    fn_date_time_create_from_interface(ed, rv, eg)
}

pub(crate) fn fn_date_time_immutable_create_from_mutable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if !require_date_source(
        arg!(ed, 1),
        "DateTime",
        "DateTimeImmutable::createFromMutable",
        eg,
    ) {
        return Ok(());
    }
    fn_date_time_immutable_create_from_interface(ed, rv, eg)
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
