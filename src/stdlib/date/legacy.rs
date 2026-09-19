//! Legacy Date formatting and solar calculations implemented without libc.

use super::*;

fn strftime_format(format: &str, timestamp: i64, utc: bool, eg: &ExecutorGlobals) -> String {
    let (timezone_id, abbreviation, offset, is_dst) = if utc {
        ("UTC", "GMT", 0, false)
    } else {
        let (identifier, abbreviation, offset) = super::timezone_spec(eg, timestamp);
        (
            identifier,
            abbreviation,
            offset,
            super::timezone_is_dst(eg, timestamp),
        )
    };
    let local = timestamp.saturating_add(offset);
    let (year, month, day, hour, minute, second, weekday, year_day) =
        super::super::unix_to_parts(local);
    let (iso_year, iso_week) = super::iso_week_and_year(year, month, day, weekday, year_day);
    let mut output = String::with_capacity(format.len() + 32);
    let mut percent = false;
    for character in format.chars() {
        if !percent {
            if character == '%' {
                percent = true;
            } else {
                output.push(character);
            }
            continue;
        }
        percent = false;
        let replacement = match character {
            '%' => "%".to_string(),
            'a' => WEEKDAYS[weekday as usize][..3].to_string(),
            'A' => WEEKDAYS[weekday as usize].to_string(),
            'b' | 'h' => MONTHS[month as usize][..3].to_string(),
            'B' => MONTHS[month as usize].to_string(),
            'c' => format!(
                "{} {} {:2} {hour:02}:{minute:02}:{second:02} {year}",
                &WEEKDAYS[weekday as usize][..3],
                &MONTHS[month as usize][..3],
                day,
            ),
            'C' => format!("{:02}", year.div_euclid(100)),
            'd' => format!("{day:02}"),
            'D' => format!("{month:02}/{day:02}/{:02}", year.rem_euclid(100)),
            'e' => format!("{day:2}"),
            'F' => format!("{year:04}-{month:02}-{day:02}"),
            'g' => format!("{:02}", iso_year.rem_euclid(100)),
            'G' => iso_year.to_string(),
            'H' => format!("{hour:02}"),
            'I' => format!(
                "{:02}",
                match hour % 12 {
                    0 => 12,
                    value => value,
                }
            ),
            'j' => format!("{:03}", year_day + 1),
            'm' => format!("{month:02}"),
            'M' => format!("{minute:02}"),
            'n' => "\n".to_string(),
            'p' => if hour < 12 { "AM" } else { "PM" }.to_string(),
            'P' => if hour < 12 { "am" } else { "pm" }.to_string(),
            'r' => format!(
                "{:02}:{minute:02}:{second:02} {}",
                match hour % 12 {
                    0 => 12,
                    value => value,
                },
                if hour < 12 { "AM" } else { "PM" },
            ),
            'R' => format!("{hour:02}:{minute:02}"),
            's' => timestamp
                .saturating_sub(if utc {
                    super::timezone_offset_seconds(eg, timestamp)
                } else {
                    0
                })
                .to_string(),
            'S' => format!("{second:02}"),
            't' => "\t".to_string(),
            'T' | 'X' => format!("{hour:02}:{minute:02}:{second:02}"),
            'u' => (if weekday == 0 { 7 } else { weekday }).to_string(),
            'U' => format!("{:02}", (year_day + 7 - weekday) / 7),
            'V' => format!("{iso_week:02}"),
            'w' => weekday.to_string(),
            'W' => {
                let monday_weekday = (weekday + 6) % 7;
                format!("{:02}", (year_day + 7 - monday_weekday) / 7)
            }
            'x' => format!("{month:02}/{day:02}/{:02}", year.rem_euclid(100)),
            'y' => format!("{:02}", year.rem_euclid(100)),
            'Y' => year.to_string(),
            'z' => super::format_timezone_offset(offset, false),
            'Z' => abbreviation.to_string(),
            other => {
                output.push('%');
                output.push(other);
                continue;
            }
        };
        output.push_str(&replacement);
    }
    if percent {
        output.push('%');
    }
    let _ = (timezone_id, is_dst);
    output
}

fn strftime_handler(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    utc: bool,
) -> Result<(), VmError> {
    let function = if utc { "gmstrftime" } else { "strftime" };
    super::super::report_internal_deprecation(
        eg,
        ed,
        &format!(
            "Function {function}() is deprecated since 8.1, use IntlDateFormatter::format() instead"
        ),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let format = arg_str!(ed, 0);
    let timestamp = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(Value::as_long)
        .unwrap_or_else(super::current_timestamp);
    ret!(
        rv,
        Value::string(strftime_format(format.as_ref(), timestamp, utc, eg))
    );
}

pub(crate) fn fn_strftime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    strftime_handler(ed, rv, eg, false)
}

pub(crate) fn fn_gmstrftime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    strftime_handler(ed, rv, eg, true)
}

fn normalize_degrees(value: f64) -> f64 {
    value.rem_euclid(360.0)
}

fn signed_degrees(value: f64) -> f64 {
    let value = normalize_degrees(value);
    if value >= 180.0 { value - 360.0 } else { value }
}

fn solar_position(day: f64) -> (f64, f64, f64) {
    let perihelion = 282.9404 + 4.70935e-5 * day;
    let eccentricity = 0.016_709 - 1.151e-9 * day;
    let mean_anomaly = normalize_degrees(356.0470 + 0.985_600_258_5 * day);
    let eccentric_anomaly = mean_anomaly
        + eccentricity.to_degrees()
            * mean_anomaly.to_radians().sin()
            * (1.0 + eccentricity * mean_anomaly.to_radians().cos());
    let x = eccentric_anomaly.to_radians().cos() - eccentricity;
    let y = (1.0 - eccentricity * eccentricity).sqrt() * eccentric_anomaly.to_radians().sin();
    let distance = x.hypot(y);
    let true_anomaly = y.atan2(x).to_degrees();
    let ecliptic_longitude = normalize_degrees(true_anomaly + perihelion);
    let obliquity = (23.4393 - 3.563e-7 * day).to_radians();
    let ecliptic = ecliptic_longitude.to_radians();
    let equatorial_x = distance * ecliptic.cos();
    let equatorial_y = distance * ecliptic.sin() * obliquity.cos();
    let equatorial_z = distance * ecliptic.sin() * obliquity.sin();
    let right_ascension = normalize_degrees(equatorial_y.atan2(equatorial_x).to_degrees());
    let declination = equatorial_z
        .atan2(equatorial_x.hypot(equatorial_y))
        .to_degrees();
    (right_ascension, declination, distance)
}

fn solar_events_utc_hours(
    year: i64,
    month: i64,
    day_of_month: i64,
    latitude: f64,
    longitude: f64,
    zenith: f64,
) -> (SolarHorizon, SolarHorizon, f64) {
    let (transit, declination, distance) =
        solar_transit_utc_hours(year, month, day_of_month, longitude);
    let mut altitude = 90.0 - zenith;
    if zenith < 91.0 {
        altitude -= 0.2666 / distance;
    }
    let latitude = latitude.to_radians();
    let declination = declination.to_radians();
    let cosine_hour = (altitude.to_radians().sin() - latitude.sin() * declination.sin())
        / (latitude.cos() * declination.cos());
    let hours = match cosine_hour {
        value if value > 1.0 => {
            return (SolarHorizon::Never, SolarHorizon::Never, transit);
        }
        value if value < -1.0 => {
            return (SolarHorizon::Always, SolarHorizon::Always, transit);
        }
        value => value.acos().to_degrees() / 15.0,
    };
    (
        SolarHorizon::Event((transit - hours).rem_euclid(24.0)),
        SolarHorizon::Event((transit + hours).rem_euclid(24.0)),
        transit,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SolarHorizon {
    Event(f64),
    Always,
    Never,
}

fn solar_transit_utc_hours(
    year: i64,
    month: i64,
    day_of_month: i64,
    longitude: f64,
) -> (f64, f64, f64) {
    let day = super::super::parts_to_unix(year, month, day_of_month, 0, 0, 0).div_euclid(86_400)
        as f64
        - 10_956.0;
    let day = day + 0.5 - longitude / 360.0;
    let sidereal_at_midnight =
        normalize_degrees(180.0 + 356.0470 + 282.9404 + (0.985_600_258_5 + 4.70935e-5) * day);
    let (right_ascension, declination, distance) = solar_position(day);
    let mut transit =
        12.0 - signed_degrees(sidereal_at_midnight + 180.0 + longitude - right_ascension) / 15.0;
    transit = transit.rem_euclid(24.0);
    (transit, declination, distance)
}

fn solar_event(
    timestamp: i64,
    latitude: f64,
    longitude: f64,
    zenith: f64,
    sunrise: bool,
    eg: &ExecutorGlobals,
) -> SolarHorizon {
    let offset = super::timezone_offset_seconds(eg, timestamp);
    let (year, month, day, _, _, _, _, _) =
        super::super::unix_to_parts(timestamp.saturating_add(offset));
    let events = solar_events_utc_hours(year, month, day, latitude, longitude, zenith);
    let horizon = if sunrise { events.0 } else { events.1 };
    let SolarHorizon::Event(utc_hours) = horizon else {
        return horizon;
    };
    let midnight = super::super::parts_to_unix(year, month, day, 0, 0, 0);
    let event = midnight.saturating_add((utc_hours * 3_600.0) as i64);
    SolarHorizon::Event(event as f64)
}

fn solar_transit(
    timestamp: i64,
    _latitude: f64,
    longitude: f64,
    eg: &ExecutorGlobals,
) -> Option<i64> {
    let offset = super::timezone_offset_seconds(eg, timestamp);
    let (year, month, day, _, _, _, _, _) =
        super::super::unix_to_parts(timestamp.saturating_add(offset));
    let transit = solar_transit_utc_hours(year, month, day, longitude).0;
    let midnight = super::super::parts_to_unix(year, month, day, 0, 0, 0);
    Some(midnight.saturating_add((transit * 3_600.0) as i64))
}

fn float_argument(value: Option<&Value>, fallback: f64) -> f64 {
    value
        .filter(|value| !matches!(value.value_type(), ValueType::Null | ValueType::Undef))
        .map(Value::to_float_val)
        .unwrap_or(fallback)
}

fn sun_value(
    timestamp: i64,
    return_format: i64,
    latitude: f64,
    longitude: f64,
    zenith: f64,
    utc_offset: f64,
    sunrise: bool,
    eg: &ExecutorGlobals,
) -> Value {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !zenith.is_finite()
        || !utc_offset.is_finite()
        || !(utc_offset * 3_600.0).is_finite()
    {
        return Value::bool(false);
    }
    let offset = super::timezone_offset_seconds(eg, timestamp);
    let (year, month, day, _, _, _, _, _) =
        super::super::unix_to_parts(timestamp.saturating_add(offset));
    let events = solar_events_utc_hours(year, month, day, latitude, longitude, zenith);
    let horizon = if sunrise { events.0 } else { events.1 };
    let SolarHorizon::Event(utc_hours) = horizon else {
        return Value::bool(false);
    };
    let midnight = super::super::parts_to_unix(year, month, day, 0, 0, 0);
    let event = midnight.saturating_add((utc_hours * 3_600.0) as i64);
    match return_format {
        0 => Value::long(event),
        1 => {
            let local = (utc_hours + utc_offset).rem_euclid(24.0);
            let total_minutes = (local * 60.0) as i64;
            Value::string(format!(
                "{:02}:{:02}",
                total_minutes.div_euclid(60).rem_euclid(24),
                total_minutes.rem_euclid(60)
            ))
        }
        2 => Value::double((utc_hours + utc_offset).rem_euclid(24.0)),
        _ => Value::bool(false),
    }
}

fn sun_function(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    sunrise: bool,
) -> Result<(), VmError> {
    let function = if sunrise {
        "date_sunrise"
    } else {
        "date_sunset"
    };
    super::super::report_internal_deprecation(
        eg,
        ed,
        &format!("Function {function}() is deprecated since 8.1, use date_sun_info() instead"),
    )?;
    if eg.exception.is_some() {
        return Ok(());
    }
    let timestamp = arg_long!(ed, 0);
    let return_format = arg_opt!(ed, 1).and_then(Value::as_long).unwrap_or(1);
    if !(0..=2).contains(&return_format) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            &format!(
                "{function}(): Argument #2 ($returnFormat) must be one of SUNFUNCS_RET_TIMESTAMP, SUNFUNCS_RET_STRING, or SUNFUNCS_RET_DOUBLE"
            ),
        ));
        return Ok(());
    }
    let latitude = float_argument(arg_opt!(ed, 2), 31.7667);
    let longitude = float_argument(arg_opt!(ed, 3), 35.2333);
    let zenith = float_argument(arg_opt!(ed, 4), 90.833333);
    let default_offset = super::timezone_offset_seconds(eg, timestamp) as f64 / 3_600.0;
    let utc_offset = float_argument(arg_opt!(ed, 5), default_offset);
    ret!(
        rv,
        sun_value(
            timestamp,
            return_format,
            latitude,
            longitude,
            zenith,
            utc_offset,
            sunrise,
            eg,
        )
    );
}

pub(crate) fn fn_date_sunrise(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    sun_function(ed, rv, eg, true)
}

pub(crate) fn fn_date_sunset(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    sun_function(ed, rv, eg, false)
}

pub(crate) fn fn_date_sun_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let timestamp = arg_long!(ed, 0);
    let latitude = float_argument(arg_opt!(ed, 1), 0.0);
    let longitude = float_argument(arg_opt!(ed, 2), 0.0);
    for (position, name, value) in [(2, "latitude", latitude), (3, "longitude", longitude)] {
        if !value.is_finite() {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                &format!("date_sun_info(): Argument #{position} (${name}) must be finite"),
            ));
            return Ok(());
        }
    }
    let mut result = PhpArray::new();
    let mut events = Vec::with_capacity(8);
    for (name, zenith, sunrise) in [
        ("sunrise", 90.583333, true),
        ("sunset", 90.583333, false),
        ("civil_twilight_begin", 96.0, true),
        ("civil_twilight_end", 96.0, false),
        ("nautical_twilight_begin", 102.0, true),
        ("nautical_twilight_end", 102.0, false),
        ("astronomical_twilight_begin", 108.0, true),
        ("astronomical_twilight_end", 108.0, false),
    ] {
        let value = solar_event(timestamp, latitude, longitude, zenith, sunrise, eg);
        events.push((name, value));
    }
    result.set_str("sunrise", solar_info_value(events[0].1));
    result.set_str("sunset", solar_info_value(events[1].1));
    result.set_str(
        "transit",
        solar_transit(timestamp, latitude, longitude, eg)
            .map_or_else(|| Value::bool(false), Value::long),
    );
    for (name, value) in events.into_iter().skip(2) {
        result.set_str(name, solar_info_value(value));
    }
    ret!(rv, Value::array(result));
}

fn solar_info_value(value: SolarHorizon) -> Value {
    match value {
        SolarHorizon::Event(timestamp) => Value::long(timestamp as i64),
        SolarHorizon::Always => Value::bool(true),
        SolarHorizon::Never => Value::bool(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solar_equator_events_are_finite_and_ordered() {
        let (SolarHorizon::Event(rise), SolarHorizon::Event(set), _) =
            solar_events_utc_hours(2024, 3, 20, 0.0, 0.0, 90.833333)
        else {
            panic!("equatorial events must exist");
        };
        assert!(rise < set);
    }
}
