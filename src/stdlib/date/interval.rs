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
    relative: Option<parser::RelativeAdjustment>,
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
            relative: None,
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

fn parse_absolute_pair(input: &str, eg: &ExecutorGlobals) -> Option<DateIntervalState> {
    let (start, end) = input.split_once('/').or_else(|| input.split_once(' '))?;
    if start.is_empty() || end.is_empty() {
        return None;
    }
    let start = parser::parse_datetime(start, None, None, eg).ok()?.state;
    let end = parser::parse_datetime(end, None, None, eg).ok()?.state;
    Some(difference(&start, &end, false))
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
        relative: Some(relative),
        ..DateIntervalState::default()
    })
}

fn contains_non_relative_elements(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let has_relative_unit = [
        " year", " month", " week", " day", " hour", " minute", " second", " weekday",
    ]
    .iter()
    .any(|unit| lower.contains(unit));
    has_relative_unit
        && (lower.contains(':')
            || lower.contains(" noon")
            || lower.ends_with(" utc")
            || [
                " january",
                " february",
                " march",
                " april",
                " may",
                " june",
                " july",
                " august",
                " september",
                " october",
                " november",
                " december",
            ]
            .iter()
            .any(|month| lower.contains(month)))
}

fn relative_parse_message(input: &str, unserializing: bool) -> String {
    let (position, character, reason) = if input.is_empty() {
        (0, ' ', "Empty string")
    } else if input.len() > 10
        && input.as_bytes().get(4) == Some(&b'-')
        && input.as_bytes().get(7) == Some(&b'-')
        && input.as_bytes().get(10) == Some(&b'-')
    {
        (10, '-', "Unexpected character")
    } else {
        (
            0,
            input.chars().next().unwrap_or(' '),
            "The timezone could not be found in the database",
        )
    };
    format!(
        "Unknown or bad format ({input}) at position {position} ({character}){}: {reason}",
        if unserializing {
            " while unserializing"
        } else {
            ""
        }
    )
}

fn parse_relative_interval(input: &str) -> Result<DateIntervalState, String> {
    if contains_non_relative_elements(input) {
        return Err(format!("String '{input}' contains non-relative elements"));
    }
    from_relative(input).ok_or_else(|| relative_parse_message(input, false))
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
        let inheritance = (!class.eq_ignore_ascii_case("DateInterval"))
            .then_some(" (inheriting DateInterval)")
            .unwrap_or_default();
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            &format!(
                "Object of type {class}{inheritance} has not been correctly initialized by calling parent::__construct() in its constructor"
            ),
        ));
        return None;
    };
    if !native.initialized {
        let class = object.class_name.to_string();
        drop(object);
        let inheritance = (!class.eq_ignore_ascii_case("DateInterval"))
            .then_some(" (inheriting DateInterval)")
            .unwrap_or_default();
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            &format!(
                "Object of type {class}{inheritance} has not been correctly initialized by calling parent::__construct() in its constructor"
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

pub(super) fn initialized_state(value: &Value) -> Option<DateIntervalState> {
    let object = value.as_object()?;
    let native = object.native_object_state::<DateIntervalState>()?;
    if !native.initialized {
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

/// DatePeriod snapshots relative intervals into its ordinary interval state,
/// while retaining the parsed adjustment needed for weekday/first/last rules.
pub(super) fn for_period(mut state: DateIntervalState) -> DateIntervalState {
    if state.from_string {
        state.from_string = false;
        state.date_string = None;
    }
    state
}

pub(crate) fn virtual_property(value: &Value, name: &str) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DateIntervalState>()?;
    state.initialized.then_some(())?;
    let integer = match name {
        "y" => state.y,
        "m" => state.m,
        "d" => state.d,
        "h" => state.h,
        "i" => state.i,
        "s" => state.s,
        "invert" => state.invert,
        _ => {
            return match name {
                "f" => Some(Value::double(state.f)),
                "days" => Some(state.days.map_or_else(|| Value::bool(false), Value::long)),
                "from_string" => Some(Value::bool(state.from_string)),
                "date_string" if state.from_string => Some(Value::string(
                    state.date_string.as_deref().unwrap_or_default(),
                )),
                _ => None,
            };
        }
    };
    Some(Value::long(integer))
}

/// DateInterval's public component fields are native virtual properties. PHP
/// stores their normalized numeric value in timelib state while the value of
/// the assignment expression itself remains unchanged.
pub(crate) fn write_virtual_property(value: &Value, name: &str, supplied: &Value) -> bool {
    let supplied = supplied.dereferenced();
    let Some(mut object) = value.as_object_mut() else {
        return false;
    };
    let state = object.native_object_state_mut::<DateIntervalState>();
    if !state.initialized {
        return false;
    }
    let stored = match name {
        "y" => {
            state.y = supplied.to_long_val();
            Value::long(state.y)
        }
        "m" => {
            state.m = supplied.to_long_val();
            Value::long(state.m)
        }
        "d" => {
            state.d = supplied.to_long_val();
            Value::long(state.d)
        }
        "h" => {
            state.h = supplied.to_long_val();
            Value::long(state.h)
        }
        "i" => {
            state.i = supplied.to_long_val();
            Value::long(state.i)
        }
        "s" => {
            state.s = supplied.to_long_val();
            Value::long(state.s)
        }
        "f" => {
            state.f = supplied.to_float_val();
            Value::double(state.f)
        }
        "invert" => {
            state.invert = supplied.to_long_val();
            Value::long(state.invert)
        }
        _ => return false,
    };
    object.set_property(name, stored);
    true
}

const SERIALIZED_KEYS: [&str; 12] = [
    "y",
    "m",
    "d",
    "h",
    "i",
    "s",
    "f",
    "invert",
    "days",
    "from_string",
    "date_string",
    "relative",
];

fn native_projection(value: &Value, state: &DateIntervalState) -> Option<PhpArray> {
    let object = value.as_object()?;
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
    Some(array)
}

pub(crate) fn debug_projection(value: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<DateIntervalState>()?;
    if !state.initialized {
        return None;
    }
    let mut result = super::custom_properties(value, &SERIALIZED_KEYS, eg);
    let native = native_projection(value, state)?;
    for (key, value) in native.iter() {
        result.set(key, value.clone_for_php_storage());
    }
    Some(Value::array(result))
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
    let elapsed_seconds = interval.relative.is_none().then(|| {
        interval
            .h
            .saturating_mul(3_600)
            .saturating_add(interval.i.saturating_mul(60))
            .saturating_add(interval.s)
    });
    let relative = interval.relative.clone().map_or_else(
        || parser::RelativeAdjustment {
            years: interval.y,
            months: interval.m,
            days: interval.d,
            ..parser::RelativeAdjustment::default()
        },
        |relative| relative,
    );
    let relative = parser::RelativeAdjustment {
        years: sign * relative.years,
        months: sign * relative.months,
        days: sign * relative.days,
        hours: sign * relative.hours,
        minutes: sign * relative.minutes,
        seconds: sign * relative.seconds,
        microseconds: sign * relative.microseconds,
        business_days: sign * relative.business_days,
        business_days_present: relative.business_days_present,
        weekday: relative
            .weekday
            .map(|(weekday, direction)| (weekday, direction.saturating_mul(sign as i8))),
        weekday_resets_time: relative.weekday_resets_time,
        weekday_before_days: relative.weekday_before_days,
        first_day: relative.first_day,
        last_day: relative.last_day,
    };
    if interval.relative.is_some()
        || relative.years != 0
        || relative.months != 0
        || relative.days != 0
    {
        parser::apply_relative(state, &relative);
    }
    if let Some(elapsed_seconds) = elapsed_seconds {
        state.timestamp = state
            .timestamp
            .saturating_add(elapsed_seconds.saturating_mul(sign));
    }
    let micros = (interval.f * 1_000_000.0).round() as i64 * sign;
    let total = i64::from(state.microsecond) + micros;
    state.timestamp = state.timestamp.saturating_add(total.div_euclid(1_000_000));
    state.microsecond = total.rem_euclid(1_000_000) as u32;
}

pub(super) fn subtraction_is_unsupported(interval: &DateIntervalState) -> bool {
    interval.relative.as_ref().is_some_and(|relative| {
        relative.business_days != 0
            || relative.weekday.is_some()
            || relative.first_day
            || relative.last_day
    })
}

pub(super) fn difference(
    base: &datetime::DateTimeState,
    target: &datetime::DateTimeState,
    absolute: bool,
) -> DateIntervalState {
    let same_region = base.timezone.kind == 3
        && target.timezone.kind == 3
        && base.timezone.name == target.timezone.name;
    let base_local = datetime::local_parts(base);
    let target_local = datetime::local_parts(target);
    let base_civil = (
        base_local.0,
        base_local.1,
        base_local.2,
        base_local.3,
        base_local.4,
        base_local.5,
        base.microsecond,
    );
    let target_civil = (
        target_local.0,
        target_local.1,
        target_local.2,
        target_local.3,
        target_local.4,
        target_local.5,
        target.microsecond,
    );
    let inverted = if same_region {
        base_civil > target_civil
    } else {
        (base.timestamp, base.microsecond) > (target.timestamp, target.microsecond)
    };
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
    let earlier_zone = timezone::description_state(&earlier.timezone, earlier.timestamp);
    let later_zone = timezone::description_state(&later.timezone, later.timestamp);
    let offset_delta = later_zone.1 - earlier_zone.1;
    let mut interval_inverted = inverted;

    if !same_region {
        seconds = seconds
            .saturating_sub(later_zone.1)
            .saturating_add(earlier_zone.1);
    } else if later.timestamp < earlier.timestamp {
        // During a backward overlap civil ordering and absolute ordering can
        // disagree.  PHP flips the minute/second distance and the interval
        // direction while retaining the surrounding civil date components.
        let flipped = (minutes.saturating_mul(60) + seconds - offset_delta).unsigned_abs() as i64;
        hours = flipped / 3_600;
        minutes = (flipped % 3_600) / 60;
        seconds = flipped % 60;
        interval_inverted = !interval_inverted;
    }

    normalize_difference(
        if interval_inverted { earlier } else { later },
        interval_inverted,
        &mut years,
        &mut months,
        &mut days,
        &mut hours,
        &mut minutes,
        &mut seconds,
        &mut micros,
    );

    if same_region {
        if earlier_zone.2 && !later_zone.2 {
            if later
                .timestamp
                .saturating_sub(earlier.timestamp)
                .saturating_add(offset_delta)
                < 86_400
            {
                hours -= offset_delta / 3_600;
                minutes -= (offset_delta % 3_600) / 60;
            }
        } else if !earlier_zone.2 && later_zone.2 {
            if let Some((transition_state, transition_time)) =
                super::tzdb::state_and_transition_at(&later.timezone.name, later.timestamp)
                && !(earlier.timestamp.saturating_add(86_400) > transition_time
                    && earlier.timestamp.saturating_add(86_400)
                        <= transition_time.saturating_add(offset_delta))
                && later.timestamp >= transition_time
                && later
                    .timestamp
                    .saturating_sub(earlier.timestamp)
                    .saturating_add(offset_delta)
                    .rem_euclid(86_400)
                    > later.timestamp.saturating_sub(transition_time)
            {
                let transition_offset = i64::from(transition_state.offset);
                hours -= (transition_offset - earlier_zone.1) / 3_600;
                minutes -= ((transition_offset - earlier_zone.1) % 3_600) / 60;
            }
        } else if later.timestamp.saturating_sub(earlier.timestamp) >= 86_400
            && let Some((transition_state, transition_time)) = super::tzdb::state_and_transition_at(
                &later.timezone.name,
                later.timestamp - later_zone.1,
            )
        {
            let correction = earlier_zone.1 - i64::from(transition_state.offset);
            if later.timestamp >= transition_time.saturating_sub(correction)
                && later.timestamp < transition_time
            {
                days -= 1;
                hours = 24;
            }
        }
    }

    let total_days = difference_days(base, target);
    DateIntervalState {
        y: years,
        m: months,
        d: days,
        h: hours,
        i: minutes,
        s: seconds,
        f: micros as f64 / 1_000_000.0,
        invert: i64::from(interval_inverted && !absolute),
        days: Some(total_days),
        initialized: true,
        ..DateIntervalState::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn normalize_difference(
    calendar_base: &datetime::DateTimeState,
    inverted: bool,
    years: &mut i64,
    months: &mut i64,
    days: &mut i64,
    hours: &mut i64,
    minutes: &mut i64,
    seconds: &mut i64,
    micros: &mut i64,
) {
    normalize_unit(micros, seconds, 1_000_000);
    normalize_unit(seconds, minutes, 60);
    normalize_unit(minutes, hours, 60);
    normalize_unit(hours, days, 24);
    normalize_unit(months, years, 12);

    let (mut year, mut month, ..) = datetime::local_parts(calendar_base);
    while *days < 0 {
        if inverted {
            *days += super::super::days_in_month(year, month);
            *months -= 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        } else {
            month -= 1;
            if month < 1 {
                month = 12;
                year -= 1;
            }
            *days += super::super::days_in_month(year, month);
            *months -= 1;
        }
    }
    normalize_unit(months, years, 12);
}

fn normalize_unit(value: &mut i64, carry: &mut i64, radix: i64) {
    if !(0..radix).contains(value) {
        *carry = carry.saturating_add(value.div_euclid(radix));
        *value = value.rem_euclid(radix);
    }
}

fn difference_days(one: &datetime::DateTimeState, two: &datetime::DateTimeState) -> i64 {
    let one_zone = timezone::description_state(&one.timezone, one.timestamp);
    let two_zone = timezone::description_state(&two.timezone, two.timestamp);
    let same_timezone = one.timezone.kind == two.timezone.kind
        && if one.timezone.kind == 3 {
            one.timezone.name == two.timezone.name
        } else {
            one_zone.1 == two_zone.1
        };
    if same_timezone {
        let one_local = datetime::local_parts(one);
        let two_local = datetime::local_parts(two);
        let one_day = super::super::parts_to_unix(one_local.0, one_local.1, one_local.2, 0, 0, 0)
            .div_euclid(86_400);
        let two_day = super::super::parts_to_unix(two_local.0, two_local.1, two_local.2, 0, 0, 0)
            .div_euclid(86_400);
        let mut days = two_day.saturating_sub(one_day).unsigned_abs() as i64;
        let (earliest, latest) =
            if (one.timestamp, one.microsecond) <= (two.timestamp, two.microsecond) {
                (one, two)
            } else {
                (two, one)
            };
        let earliest_microsecond = earliest.microsecond;
        let latest_microsecond = latest.microsecond;
        let earliest = datetime::local_parts(earliest);
        let latest = datetime::local_parts(latest);
        if (latest.3, latest.4, latest.5, latest_microsecond)
            < (earliest.3, earliest.4, earliest.5, earliest_microsecond)
            && days > 0
        {
            days -= 1;
        }
        days
    } else {
        // timelib's public `days` field intentionally ignores fractions and
        // truncates the absolute epoch-second distance towards zero.
        one.timestamp.abs_diff(two.timestamp).saturating_div(86_400) as i64
    }
}

pub(crate) fn fn_date_interval_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = arg_str!(ed, 1);
    let state = parse_iso_duration(input.as_ref()).or_else(|| parse_absolute_pair(&input, eg));
    let Some(state) = state else {
        let looks_like_complete_date = input.len() >= 20
            && input.as_bytes().get(4) == Some(&b'-')
            && input.as_bytes().get(7) == Some(&b'-')
            && input.as_bytes().get(10) == Some(&b'T');
        let message = if input.ends_with('/') || (!input.contains('/') && looks_like_complete_date)
        {
            format!("Failed to parse interval ({input})")
        } else {
            format!("Unknown or bad format ({input})")
        };
        eg.exception = Some(crate::value::make_error_value(
            "DateMalformedIntervalStringException",
            &message,
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
    let Some(input) = super::super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "DateInterval::createFromDateString",
        1,
        "datetime",
        "string",
    )?
    else {
        return Ok(());
    };
    match parse_relative_interval(input.as_str().unwrap_or_default()) {
        Ok(state) => {
            if let Some(result) = allocate(eg, state) {
                ret!(rv, result);
            }
        }
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value(
                "DateMalformedIntervalStringException",
                &message,
            ));
        }
    }
    Ok(())
}

pub(crate) fn fn_date_interval_create_from_date_string_global(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(input) = super::super::typed_internal_string_value_argument_expected(
        ed,
        eg,
        "date_interval_create_from_date_string",
        0,
        "datetime",
        "string",
    )?
    else {
        return Ok(());
    };
    match parse_relative_interval(input.as_str().unwrap_or_default()) {
        Ok(state) => {
            if let Some(result) = allocate(eg, state) {
                ret!(rv, result);
            }
        }
        Err(message) => {
            super::super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                &format!("date_interval_create_from_date_string(): {message}"),
            )?;
            if eg.exception.is_none() {
                ret!(rv, Value::bool(false));
            }
        }
    }
    Ok(())
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
    let Some(state) = snapshot(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let Some(mut result) = native_projection(arg!(ed, 0), &state) else {
        return Ok(());
    };
    super::append_custom_properties(&mut result, arg!(ed, 0), &SERIALIZED_KEYS, eg);
    ret!(rv, Value::array(result));
}

fn serialized_integer(data: &PhpArray, name: &str, fallback: i64) -> i64 {
    let Some(value) = data.get_str(name) else {
        return fallback;
    };
    let value = value.dereferenced();
    match value.value_type() {
        ValueType::Long => value.as_long().unwrap_or(fallback),
        ValueType::True => 1,
        ValueType::False | ValueType::Null => 0,
        ValueType::Double => value.as_double().unwrap_or_default() as i64,
        ValueType::String => value
            .as_str()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(0),
        _ => fallback,
    }
}

fn serialized_fraction(data: &PhpArray) -> (f64, Option<f64>) {
    let Some(value) = data.get_str("f") else {
        return (0.0, None);
    };
    let value = value.dereferenced();
    let raw = value
        .as_double()
        .or_else(|| value.as_long().map(|value| value as f64))
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
        .unwrap_or(0.0);
    let scaled = raw * 1_000_000.0;
    let overflow = (!scaled.is_finite()
        || !(-(i64::MAX as f64)..i64::MAX as f64).contains(&scaled))
    .then_some(scaled);
    // timelib stores fractional seconds as signed microseconds. The legacy
    // unserialization path performs the historical wrapping cast before
    // projecting it back to a PHP float.
    let microseconds = (scaled.trunc() as i128) as i64;
    (microseconds as f64 / 1_000_000.0, overflow)
}

fn state_from_array(data: &PhpArray) -> Result<(DateIntervalState, Option<f64>), String> {
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
    if from_string || data.get_str("date_string").is_some() {
        let date_string = data
            .get_str("date_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if date_string.len() >= 10
            && date_string.as_bytes().get(4) == Some(&b'-')
            && date_string.as_bytes().get(7) == Some(&b'-')
            && date_string
                .as_bytes()
                .get(10)
                .is_none_or(|byte| *byte == b' ')
        {
            return Ok((
                DateIntervalState {
                    from_string: true,
                    date_string: Some(date_string.to_string()),
                    initialized: true,
                    ..DateIntervalState::default()
                },
                None,
            ));
        }
        return from_relative(date_string)
            .map(|state| (state, None))
            .ok_or_else(|| relative_parse_message(date_string, true));
    }
    let (fraction, overflow) = serialized_fraction(data);
    let days = match data.get_str("days") {
        Some(value) if value.dereferenced().value_type() == ValueType::False => None,
        Some(_) => Some(serialized_integer(data, "days", -1)),
        None => Some(-1),
    };
    Ok((
        DateIntervalState {
            y: serialized_integer(data, "y", -1),
            m: serialized_integer(data, "m", -1),
            d: serialized_integer(data, "d", -1),
            h: serialized_integer(data, "h", -1),
            i: serialized_integer(data, "i", -1),
            s: serialized_integer(data, "s", -1),
            f: fraction,
            invert: serialized_integer(data, "invert", 0),
            days,
            from_string: false,
            date_string: None,
            initialized: true,
            relative: None,
        },
        overflow,
    ))
}

fn report_fraction_overflow(
    overflow: Option<f64>,
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(number) = overflow else {
        return Ok(());
    };
    let rendered = Value::double(number).echo_to_string_with_precision(-1);
    super::super::report_internal_diagnostic(
        eg,
        ed,
        2,
        "Warning",
        &format!("The float {rendered} is not representable as an int, cast occurred"),
    )
    .map(|_| ())
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
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    match state_from_array(&data) {
        Ok((state, overflow)) => {
            report_fraction_overflow(overflow, ed, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if install(arg!(ed, 0), state) {
                super::restore_custom_properties(arg!(ed, 0), &data, &SERIALIZED_KEYS, ed, eg);
            }
        }
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("Error", &message));
        }
    }
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
            relative: None,
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
    match state_from_array(&data) {
        Ok((state, overflow)) => {
            report_fraction_overflow(overflow, ed, eg)?;
            if eg.exception.is_some() {
                return Ok(());
            }
            if let Some(value) = allocate(eg, state) {
                ret!(rv, value);
            }
        }
        Err(message) => {
            eg.exception = Some(crate::value::make_error_value("Error", &message));
        }
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
