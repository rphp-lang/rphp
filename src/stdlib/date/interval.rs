//! Native DateInterval state, ISO duration parsing and civil arithmetic.

use super::{datetime, parser, *};
use crate::value::NativeObjectState;

#[derive(Clone, Debug)]
pub(super) struct DateIntervalState {
    pub y: i64,
    pub m: i64,
    pub d: i64,
    pub h: i64,
    pub i: i64,
    pub s: i64,
    pub f: f64,
    pub invert: i64,
    pub days: Option<i64>,
    pub from_string: bool,
    pub date_string: Option<String>,
    pub initialized: bool,
}

impl Default for DateIntervalState {
    fn default() -> Self {
        Self {
            y: 0,
            m: 0,
            d: 0,
            h: 0,
            i: 0,
            s: 0,
            f: 0.0,
            invert: 0,
            days: None,
            from_string: false,
            date_string: None,
            initialized: false,
        }
    }
}

impl NativeObjectState for DateIntervalState {
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

pub(super) fn parse_iso_duration(input: &str) -> Option<DateIntervalState> {
    let input = input.strip_prefix('P')?;
    if input.is_empty() {
        return None;
    }
    let mut result = DateIntervalState {
        initialized: true,
        ..DateIntervalState::default()
    };
    let mut time = false;
    let mut number_start = 0;
    let mut found = false;
    let bytes = input.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'T' {
            time = true;
            position += 1;
            number_start = position;
            continue;
        }
        if bytes[position].is_ascii_digit() || bytes[position] == b'.' {
            position += 1;
            continue;
        }
        if number_start == position {
            return None;
        }
        let number = input.get(number_start..position)?;
        let designator = bytes[position] as char;
        match (time, designator) {
            (false, 'Y') => result.y = number.parse().ok()?,
            (false, 'M') => result.m = number.parse().ok()?,
            (false, 'W') => result.d = number.parse::<i64>().ok()?.saturating_mul(7),
            (false, 'D') => result.d = number.parse().ok()?,
            (true, 'H') => result.h = number.parse().ok()?,
            (true, 'M') => result.i = number.parse().ok()?,
            (true, 'S') => {
                let seconds = number.parse::<f64>().ok()?;
                result.s = seconds.floor() as i64;
                result.f = seconds.fract();
            }
            _ => return None,
        }
        found = true;
        position += 1;
        number_start = position;
    }
    (found && number_start == position).then_some(result)
}

fn from_relative(input: &str) -> Option<DateIntervalState> {
    let relative = parser::parse_relative(input)?;
    Some(DateIntervalState {
        y: relative.years,
        m: relative.months,
        d: relative.days,
        h: relative.hours,
        i: relative.minutes,
        s: relative.seconds,
        from_string: true,
        date_string: Some(input.to_string()),
        initialized: true,
        ..DateIntervalState::default()
    })
}

fn set_property(object: &mut PhpObject, name: &str, value: Value) {
    let _ = object.set_property(name, value);
}

fn publish_properties(object: &mut PhpObject, state: &DateIntervalState) {
    if state.from_string {
        set_property(object, "from_string", Value::bool(true));
        set_property(
            object,
            "date_string",
            Value::string(state.date_string.as_deref().unwrap_or_default()),
        );
        return;
    }
    for (name, value) in [
        ("y", state.y),
        ("m", state.m),
        ("d", state.d),
        ("h", state.h),
        ("i", state.i),
        ("s", state.s),
    ] {
        set_property(object, name, Value::long(value));
    }
    set_property(object, "f", Value::double(state.f));
    set_property(object, "invert", Value::long(state.invert));
    set_property(
        object,
        "days",
        state.days.map_or_else(|| Value::bool(false), Value::long),
    );
    set_property(object, "from_string", Value::bool(false));
}

pub(super) fn allocate(eg: &ExecutorGlobals, state: DateIntervalState) -> Option<Value> {
    let class = eg.find_class("DateInterval")?;
    let mut object = PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    );
    publish_properties(&mut object, &state);
    *object.native_object_state_mut::<DateIntervalState>() = state;
    Some(Value::object(object))
}

fn value_long(object: &PhpObject, name: &str, fallback: i64) -> i64 {
    object
        .get_property(name)
        .and_then(Value::as_long)
        .unwrap_or(fallback)
}

fn value_double(object: &PhpObject, name: &str, fallback: f64) -> f64 {
    object
        .get_property(name)
        .and_then(|value| {
            value
                .as_double()
                .or_else(|| value.as_long().map(|v| v as f64))
        })
        .unwrap_or(fallback)
}

pub(super) fn snapshot(value: &Value, eg: &mut ExecutorGlobals) -> Option<DateIntervalState> {
    let object = value.as_object()?;
    let Some(native) = object.native_object_state::<DateIntervalState>() else {
        let class = object.class_name.to_string();
        drop(object);
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            &format!(
                "Object of type {class} has not been correctly initialized by calling parent::__construct() in its constructor"
            ),
        ));
        return None;
    };
    if !native.initialized {
        let class = object.class_name.to_string();
        drop(object);
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            &format!(
                "Object of type {class} has not been correctly initialized by calling parent::__construct() in its constructor"
            ),
        ));
        return None;
    }
    let mut result = native.clone();
    if !result.from_string {
        result.y = value_long(&object, "y", result.y);
        result.m = value_long(&object, "m", result.m);
        result.d = value_long(&object, "d", result.d);
        result.h = value_long(&object, "h", result.h);
        result.i = value_long(&object, "i", result.i);
        result.s = value_long(&object, "s", result.s);
        result.f = value_double(&object, "f", result.f);
        result.invert = value_long(&object, "invert", result.invert);
        result.days = object.get_property("days").and_then(Value::as_long);
    }
    Some(result)
}

pub(crate) fn debug_projection(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DateIntervalState>()?;
    state.initialized.then(|| {
        let mut array = PhpArray::new();
        if state.from_string {
            array.set_str("from_string", Value::bool(true));
            array.set_str(
                "date_string",
                Value::string(state.date_string.as_deref().unwrap_or_default()),
            );
        } else {
            for (name, value) in [
                ("y", value_long(&object, "y", state.y)),
                ("m", value_long(&object, "m", state.m)),
                ("d", value_long(&object, "d", state.d)),
                ("h", value_long(&object, "h", state.h)),
                ("i", value_long(&object, "i", state.i)),
                ("s", value_long(&object, "s", state.s)),
            ] {
                array.set_str(name, Value::long(value));
            }
            array.set_str("f", Value::double(value_double(&object, "f", state.f)));
            array.set_str(
                "invert",
                Value::long(value_long(&object, "invert", state.invert)),
            );
            array.set_str(
                "days",
                object
                    .get_property("days")
                    .cloned()
                    .unwrap_or_else(|| state.days.map_or_else(|| Value::bool(false), Value::long)),
            );
            array.set_str("from_string", Value::bool(false));
        }
        Value::array(array)
    })
}

fn format(state: &DateIntervalState, format: &str) -> String {
    let mut output = String::with_capacity(format.len() + 16);
    let mut escaped = false;
    for character in format.chars() {
        if !escaped {
            if character == '%' {
                escaped = true;
            } else {
                output.push(character);
            }
            continue;
        }
        escaped = false;
        match character {
            '%' => output.push('%'),
            'Y' => output.push_str(&format!("{:02}", state.y)),
            'y' => output.push_str(&state.y.to_string()),
            'M' => output.push_str(&format!("{:02}", state.m)),
            'm' => output.push_str(&state.m.to_string()),
            'D' => output.push_str(&format!("{:02}", state.d)),
            'd' => output.push_str(&state.d.to_string()),
            'H' => output.push_str(&format!("{:02}", state.h)),
            'h' => output.push_str(&state.h.to_string()),
            'I' => output.push_str(&format!("{:02}", state.i)),
            'i' => output.push_str(&state.i.to_string()),
            'S' => output.push_str(&format!("{:02}", state.s)),
            's' => output.push_str(&state.s.to_string()),
            'F' => output.push_str(&format!("{:06}", (state.f * 1_000_000.0).round() as i64)),
            'f' => output.push_str(&((state.f * 1_000_000.0).round() as i64).to_string()),
            'R' => output.push(if state.invert != 0 { '-' } else { '+' }),
            'r' => {
                if state.invert != 0 {
                    output.push('-');
                }
            }
            'a' => output.push_str(
                &state
                    .days
                    .map_or_else(|| "(unknown)".to_string(), |days| days.to_string()),
            ),
            other => {
                output.push('%');
                output.push(other);
            }
        }
    }
    if escaped {
        output.push('%');
    }
    output
}

pub(super) fn apply_to_datetime(
    state: &mut datetime::DateTimeState,
    interval: &DateIntervalState,
    subtract: bool,
) {
    let mut sign = if interval.invert != 0 { -1 } else { 1 };
    if subtract {
        sign = -sign;
    }
    let relative = parser::RelativeAdjustment {
        years: sign * interval.y,
        months: sign * interval.m,
        days: sign * interval.d,
        hours: sign * interval.h,
        minutes: sign * interval.i,
        seconds: sign * interval.s,
        ..parser::RelativeAdjustment::default()
    };
    parser::apply_relative(state, &relative);
    let micros = (interval.f * 1_000_000.0).round() as i64 * sign;
    let total = i64::from(state.microsecond) + micros;
    state.timestamp = state.timestamp.saturating_add(total.div_euclid(1_000_000));
    state.microsecond = total.rem_euclid(1_000_000) as u32;
}

pub(super) fn difference(
    base: &datetime::DateTimeState,
    target: &datetime::DateTimeState,
    absolute: bool,
) -> DateIntervalState {
    let inverted = target.timestamp < base.timestamp
        || (target.timestamp == base.timestamp && target.microsecond < base.microsecond);
    let (earlier, later) = if inverted {
        (target, base)
    } else {
        (base, target)
    };
    let (ey, em, ed, eh, ei, es) = datetime::local_parts(earlier);
    let (ly, lm, ld, lh, li, ls) = datetime::local_parts(later);
    let mut years = ly - ey;
    let mut months = lm - em;
    let mut days = ld - ed;
    let mut hours = lh - eh;
    let mut minutes = li - ei;
    let mut seconds = ls - es;
    let mut micros = i64::from(later.microsecond) - i64::from(earlier.microsecond);
    if micros < 0 {
        micros += 1_000_000;
        seconds -= 1;
    }
    if seconds < 0 {
        seconds += 60;
        minutes -= 1;
    }
    if minutes < 0 {
        minutes += 60;
        hours -= 1;
    }
    if hours < 0 {
        hours += 24;
        days -= 1;
    }
    if days < 0 {
        months -= 1;
        let previous_month = lm - 1;
        let previous_year = ly + (previous_month - 1).div_euclid(12);
        let previous_month = (previous_month - 1).rem_euclid(12) + 1;
        days += super::super::days_in_month(previous_year, previous_month);
    }
    if months < 0 {
        months += 12;
        years -= 1;
    }
    let base_micros = i128::from(base.timestamp) * 1_000_000 + i128::from(base.microsecond);
    let target_micros = i128::from(target.timestamp) * 1_000_000 + i128::from(target.microsecond);
    let total_days = ((target_micros - base_micros).unsigned_abs() / 86_400_000_000) as i64;
    DateIntervalState {
        y: years,
        m: months,
        d: days,
        h: hours,
        i: minutes,
        s: seconds,
        f: micros as f64 / 1_000_000.0,
        invert: i64::from(inverted && !absolute),
        days: Some(total_days),
        initialized: true,
        ..DateIntervalState::default()
    }
}

pub(crate) fn fn_date_interval_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 1);
    let Some(state) = parse_iso_duration(input.as_ref()) else {
        eg.exception = Some(crate::value::make_error_value(
            "DateMalformedIntervalStringException",
            &format!("Unknown or bad format ({input})"),
        ));
        return Ok(());
    };
    let Some(mut object) = arg!(ed, 0).as_object_mut() else {
        return Ok(());
    };
    publish_properties(&mut object, &state);
    *object.native_object_state_mut::<DateIntervalState>() = state;
    Ok(())
}

pub(crate) fn fn_date_interval_create_from_date_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 1);
    let result = from_relative(input.as_ref())
        .and_then(|state| allocate(eg, state))
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

pub(crate) fn fn_date_interval_create_from_date_string_global(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 0);
    let result = from_relative(input.as_ref())
        .and_then(|state| allocate(eg, state))
        .unwrap_or_else(|| Value::bool(false));
    ret!(rv, result)
}

pub(crate) fn fn_date_interval_format(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let format_string = arg_str!(ed, 1);
    ret!(rv, Value::string(format(&state, format_string.as_ref())));
}

pub(crate) fn fn_date_interval_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(_state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    if let Some(projection) = debug_projection(arg!(ed, 0)) {
        ret!(rv, projection);
    }
    Ok(())
}

fn state_from_array(data: &PhpArray) -> DateIntervalState {
    let from_string = data
        .get_str("from_string")
        .and_then(|value| {
            matches!(
                value.dereferenced().value_type(),
                ValueType::True | ValueType::False
            )
            .then(|| value.dereferenced().is_truthy())
        })
        .unwrap_or(false);
    if from_string {
        let date_string = data
            .get_str("date_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return from_relative(date_string).unwrap_or(DateIntervalState {
            from_string: true,
            date_string: Some(date_string.to_string()),
            initialized: true,
            ..DateIntervalState::default()
        });
    }
    let integer = |name: &str, fallback: i64| {
        data.get_str(name)
            .and_then(Value::as_long)
            .unwrap_or(fallback)
    };
    DateIntervalState {
        y: integer("y", -1),
        m: integer("m", -1),
        d: integer("d", -1),
        h: integer("h", -1),
        i: integer("i", -1),
        s: integer("s", -1),
        f: data
            .get_str("f")
            .and_then(|value| {
                value
                    .as_double()
                    .or_else(|| value.as_long().map(|v| v as f64))
            })
            .unwrap_or(0.0),
        invert: integer("invert", 0),
        days: match data.get_str("days") {
            Some(value) => value.as_long(),
            None => Some(-1),
        },
        from_string: false,
        date_string: None,
        initialized: true,
    }
}

fn install(receiver: &Value, state: DateIntervalState) -> bool {
    let Some(mut object) = receiver.as_object_mut() else {
        return false;
    };
    publish_properties(&mut object, &state);
    *object.native_object_state_mut::<DateIntervalState>() = state;
    true
}

pub(crate) fn fn_date_interval_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    install(arg!(ed, 0), state_from_array(&data));
    Ok(())
}

pub(crate) fn fn_date_interval_wakeup(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let Some(object) = receiver.as_object() else {
        return Ok(());
    };
    let from_string = object
        .get_property("from_string")
        .and_then(|value| {
            matches!(
                value.dereferenced().value_type(),
                ValueType::True | ValueType::False
            )
            .then(|| value.dereferenced().is_truthy())
        })
        .unwrap_or(false);
    let state = if from_string {
        let source = object
            .get_property("date_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        from_relative(source).unwrap_or(DateIntervalState {
            from_string: true,
            date_string: Some(source.to_string()),
            initialized: true,
            ..DateIntervalState::default()
        })
    } else {
        DateIntervalState {
            y: value_long(&object, "y", -1),
            m: value_long(&object, "m", -1),
            d: value_long(&object, "d", -1),
            h: value_long(&object, "h", -1),
            i: value_long(&object, "i", -1),
            s: value_long(&object, "s", -1),
            f: value_double(&object, "f", 0.0),
            invert: value_long(&object, "invert", 0),
            days: match object.get_property("days") {
                Some(value) => value.as_long(),
                None => Some(-1),
            },
            initialized: true,
            ..DateIntervalState::default()
        }
    };
    drop(object);
    install(&receiver, state);
    Ok(())
}

pub(crate) fn fn_date_interval_set_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    if let Some(value) = allocate(eg, state_from_array(&data)) {
        ret!(rv, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_duration_parser_distinguishes_date_and_time_minutes() {
        let interval = parse_iso_duration("P2Y4DT6H8M0.25S").unwrap();
        assert_eq!((interval.y, interval.m, interval.d), (2, 0, 4));
        assert_eq!((interval.h, interval.i, interval.s), (6, 8, 0));
        assert_eq!(interval.f, 0.25);
    }

    #[test]
    fn interval_formatter_preserves_sign_and_unknown_days() {
        let interval = DateIntervalState {
            invert: 1,
            y: 2,
            initialized: true,
            ..DateIntervalState::default()
        };
        assert_eq!(format(&interval, "%R%Y %r%y %a"), "-02 -2 (unknown)");
    }
}
