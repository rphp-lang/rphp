//! Procedural Date extension surface.
//!
//! The first compatibility tranche intentionally owns the scalar UTC/fixed-
//! offset contract. IANA transition data, relative-time parsing and DateTime
//! object behavior remain separate checkpoints; the Date extension therefore
//! stays absent from `extension_loaded()` until those contracts are complete.

use super::*;

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[inline]
pub(super) fn current_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

pub(super) fn optional_timestamp(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
) -> Result<Option<i64>, VmError> {
    let Some(argument) = arg_opt!(ed, index) else {
        return Ok(Some(current_timestamp()));
    };
    let argument = argument.dereferenced();
    if argument.value_type() == ValueType::Null {
        return Ok(Some(current_timestamp()));
    }
    if argument.value_type() == ValueType::Long {
        return Ok(argument.as_long());
    }
    super::typed_internal_int_argument(ed, eg, function, index, "timestamp")
}

pub(super) fn is_supported_timezone(timezone: &str) -> bool {
    matches!(
        timezone,
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "Asia/Kolkata"
    )
}

pub(super) fn timezone_id(eg: &ExecutorGlobals) -> &str {
    eg.ini_overrides
        .as_deref()
        .and_then(|overrides| overrides.get("date.timezone"))
        .map(String::as_str)
        .unwrap_or("UTC")
}

pub(super) fn timezone_offset_seconds(eg: &ExecutorGlobals, _timestamp: i64) -> i64 {
    match timezone_id(eg) {
        "Asia/Kolkata" => 19_800,
        _ => 0,
    }
}

pub(super) fn timezone_spec(eg: &ExecutorGlobals, _timestamp: i64) -> (&str, &str, i64) {
    let identifier = timezone_id(eg);
    match identifier {
        "Asia/Kolkata" => (identifier, "IST", 19_800),
        "GMT" | "Etc/GMT" => (identifier, "GMT", 0),
        _ => (identifier, "UTC", 0),
    }
}

pub(super) fn format_timezone_offset(offset: i64, colon: bool) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.unsigned_abs();
    let hours = offset / 3_600;
    let minutes = offset % 3_600 / 60;
    if colon {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}{minutes:02}")
    }
}

pub(super) fn ordinal_suffix(day: i64) -> &'static str {
    if (11..=13).contains(&(day.rem_euclid(100))) {
        return "th";
    }
    match day.rem_euclid(10) {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

pub(super) fn internet_beats(timestamp: i64) -> i64 {
    timestamp
        .saturating_add(3_600)
        .rem_euclid(86_400)
        .saturating_mul(1_000)
        / 86_400
}

pub(super) fn iso_week_and_year(
    year: i64,
    _month: i64,
    _day: i64,
    weekday: i64,
    year_day: i64,
) -> (i64, i64) {
    let iso_weekday = if weekday == 0 { 7 } else { weekday };
    let mut week = (year_day + 11 - iso_weekday) / 7;
    let mut iso_year = year;
    if week < 1 {
        iso_year -= 1;
        week = iso_weeks_in_year(iso_year);
    } else if week > iso_weeks_in_year(year) {
        iso_year += 1;
        week = 1;
    }
    (iso_year, week)
}

fn iso_weeks_in_year(year: i64) -> i64 {
    let (_, _, _, _, _, _, january_first, _) =
        super::unix_to_parts(super::parts_to_unix(year, 1, 1, 0, 0, 0));
    if january_first == 4 || (january_first == 3 && super::is_leap_year(year)) {
        53
    } else {
        52
    }
}

fn timestamp_parts(
    eg: &ExecutorGlobals,
    timestamp: i64,
) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    super::unix_to_parts(timestamp.saturating_add(timezone_offset_seconds(eg, timestamp)))
}

fn nullable_component(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Option<i64>>, VmError> {
    let Some(argument) = arg_opt!(ed, index) else {
        return Ok(Some(None));
    };
    if argument.dereferenced().value_type() == ValueType::Null {
        return Ok(Some(None));
    }
    Ok(super::typed_internal_int_argument(ed, eg, function, index, parameter)?.map(Some))
}

fn normalized_timestamp(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    let month_index = month.saturating_sub(1);
    let normalized_year = year.saturating_add(month_index.div_euclid(12));
    let normalized_month = month_index.rem_euclid(12) + 1;
    super::parts_to_unix(normalized_year, normalized_month, 1, hour, minute, second)
        .saturating_add(day.saturating_sub(1).saturating_mul(86_400))
}

#[inline]
fn normalized_mktime_year(year: i64) -> i64 {
    match year {
        0..=69 => year + 2_000,
        70..=100 => year + 1_900,
        _ => year,
    }
}

pub(super) fn fn_mktime_impl(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let exact = std::array::from_fn::<_, 6, _>(|index| arg!(ed, index as u32).dereferenced());
    if exact
        .iter()
        .all(|value| value.value_type() == ValueType::Long)
    {
        let local = normalized_timestamp(
            normalized_mktime_year(exact[5].as_long().unwrap_or_default()),
            exact[3].as_long().unwrap_or_default(),
            exact[4].as_long().unwrap_or_default(),
            exact[0].as_long().unwrap_or_default(),
            exact[1].as_long().unwrap_or_default(),
            exact[2].as_long().unwrap_or_default(),
        );
        let offset = timezone_offset_seconds(eg, local);
        ret!(rv, Value::long(local.saturating_sub(offset)));
    }
    let Some(hour) = super::typed_internal_int_argument(ed, eg, "mktime", 0, "hour")? else {
        return Ok(());
    };
    let current = timestamp_parts(eg, current_timestamp());
    let Some(minute) = nullable_component(ed, eg, "mktime", 1, "minute")? else {
        return Ok(());
    };
    let Some(second) = nullable_component(ed, eg, "mktime", 2, "second")? else {
        return Ok(());
    };
    let Some(month) = nullable_component(ed, eg, "mktime", 3, "month")? else {
        return Ok(());
    };
    let Some(day) = nullable_component(ed, eg, "mktime", 4, "day")? else {
        return Ok(());
    };
    let Some(year) = nullable_component(ed, eg, "mktime", 5, "year")? else {
        return Ok(());
    };
    let local = normalized_timestamp(
        normalized_mktime_year(year.unwrap_or(current.0)),
        month.unwrap_or(current.1),
        day.unwrap_or(current.2),
        hour,
        minute.unwrap_or(current.4),
        second.unwrap_or(current.5),
    );
    let offset = timezone_offset_seconds(eg, local);
    ret!(rv, Value::long(local.saturating_sub(offset)));
}

pub(super) fn fn_gmmktime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(hour) = super::typed_internal_int_argument(ed, eg, "gmmktime", 0, "hour")? else {
        return Ok(());
    };
    let current = super::unix_to_parts(current_timestamp());
    let Some(minute) = nullable_component(ed, eg, "gmmktime", 1, "minute")? else {
        return Ok(());
    };
    let Some(second) = nullable_component(ed, eg, "gmmktime", 2, "second")? else {
        return Ok(());
    };
    let Some(month) = nullable_component(ed, eg, "gmmktime", 3, "month")? else {
        return Ok(());
    };
    let Some(day) = nullable_component(ed, eg, "gmmktime", 4, "day")? else {
        return Ok(());
    };
    let Some(year) = nullable_component(ed, eg, "gmmktime", 5, "year")? else {
        return Ok(());
    };
    ret!(
        rv,
        Value::long(normalized_timestamp(
            normalized_mktime_year(year.unwrap_or(current.0)),
            month.unwrap_or(current.1),
            day.unwrap_or(current.2),
            hour,
            minute.unwrap_or(current.4),
            second.unwrap_or(current.5),
        ))
    );
}

pub(super) fn fn_checkdate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(month) = super::typed_internal_int_argument(ed, eg, "checkdate", 0, "month")? else {
        return Ok(());
    };
    let Some(day) = super::typed_internal_int_argument(ed, eg, "checkdate", 1, "day")? else {
        return Ok(());
    };
    let Some(year) = super::typed_internal_int_argument(ed, eg, "checkdate", 2, "year")? else {
        return Ok(());
    };
    let valid = (1..=32_767).contains(&year)
        && (1..=12).contains(&month)
        && (1..=super::days_in_month(year, month)).contains(&day);
    ret!(rv, Value::bool(valid));
}

pub(super) fn fn_localtime(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timestamp) = optional_timestamp(ed, eg, "localtime", 0)? else {
        return Ok(());
    };
    let associative = if arg_opt!(ed, 1).is_some() {
        let Some(associative) =
            super::typed_internal_bool_argument(ed, eg, "localtime", 1, "associative")?
        else {
            return Ok(());
        };
        associative
    } else {
        false
    };
    let (year, month, day, hour, minute, second, weekday, year_day) =
        timestamp_parts(eg, timestamp);
    let values = [
        second,
        minute,
        hour,
        day,
        month - 1,
        year - 1900,
        weekday,
        year_day,
        0,
    ];
    let mut result = PhpArray::with_packed_capacity(values.len());
    if associative {
        for (name, value) in [
            "tm_sec", "tm_min", "tm_hour", "tm_mday", "tm_mon", "tm_year", "tm_wday", "tm_yday",
            "tm_isdst",
        ]
        .into_iter()
        .zip(values)
        {
            result.set_str(name, Value::long(value));
        }
    } else {
        for value in values {
            result.push(Value::long(value));
        }
    }
    ret!(rv, Value::array(result));
}

pub(super) fn fn_getdate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timestamp) = optional_timestamp(ed, eg, "getdate", 0)? else {
        return Ok(());
    };
    let (year, month, day, hour, minute, second, weekday, year_day) =
        timestamp_parts(eg, timestamp);
    let mut result = PhpArray::new();
    result.set_str("seconds", Value::long(second));
    result.set_str("minutes", Value::long(minute));
    result.set_str("hours", Value::long(hour));
    result.set_str("mday", Value::long(day));
    result.set_str("wday", Value::long(weekday));
    result.set_str("mon", Value::long(month));
    result.set_str("year", Value::long(year));
    result.set_str("yday", Value::long(year_day));
    result.set_str("weekday", Value::string(WEEKDAYS[weekday as usize]));
    result.set_str("month", Value::string(MONTHS[month as usize]));
    result.set_int(0, Value::long(timestamp));
    ret!(rv, Value::array(result));
}

pub(super) fn fn_idate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(format) = super::typed_internal_string_argument(ed, eg, "idate", 0, "format")? else {
        return Ok(());
    };
    let Some(timestamp) = optional_timestamp(ed, eg, "idate", 1)? else {
        return Ok(());
    };
    let mut characters = format.chars();
    let Some(format) = characters.next() else {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "idate(): idate format is one char",
        )?;
        ret!(rv, Value::bool(false));
    };
    if characters.next().is_some() {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "idate(): idate format is one char",
        )?;
        ret!(rv, Value::bool(false));
    }
    let (year, month, day, hour, minute, second, weekday, year_day) =
        timestamp_parts(eg, timestamp);
    let (iso_year, iso_week) = iso_week_and_year(year, month, day, weekday, year_day);
    let value = match format {
        'B' => internet_beats(timestamp),
        'd' => day,
        'h' => {
            if hour % 12 == 0 {
                12
            } else {
                hour % 12
            }
        }
        'H' => hour,
        'i' => minute,
        'I' => 0,
        'L' => i64::from(super::is_leap_year(year)),
        'm' => month,
        'N' => {
            if weekday == 0 {
                7
            } else {
                weekday
            }
        }
        'o' => iso_year,
        's' => second,
        't' => super::days_in_month(year, month),
        'U' => timestamp,
        'w' => weekday,
        'W' => iso_week,
        'y' => year.rem_euclid(100),
        'Y' => year,
        'z' => year_day,
        'Z' => timezone_offset_seconds(eg, timestamp),
        _ => {
            super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "idate(): Unrecognized date format token",
            )?;
            ret!(rv, Value::bool(false));
        }
    };
    ret!(rv, Value::long(value));
}

pub(super) fn fn_date_default_timezone_get(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, Value::string(timezone_id(eg)));
}

pub(super) fn fn_date_default_timezone_set(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timezone) = super::typed_internal_string_argument(
        ed,
        eg,
        "date_default_timezone_set",
        0,
        "timezoneId",
    )?
    else {
        return Ok(());
    };
    if !is_supported_timezone(&timezone) {
        super::report_internal_diagnostic(
            eg,
            ed,
            8,
            "Notice",
            &format!("date_default_timezone_set(): Timezone ID '{timezone}' is invalid"),
        )?;
        ret!(rv, Value::bool(false));
    }
    eg.ini_overrides
        .get_or_insert_with(|| Box::new(std::collections::HashMap::new()))
        .insert("date.timezone".to_string(), timezone);
    ret!(rv, Value::bool(true));
}

#[cfg(test)]
mod tests {
    use super::{
        format_timezone_offset, internet_beats, iso_week_and_year, normalized_mktime_year,
        ordinal_suffix,
    };

    #[test]
    fn scalar_date_helpers_cover_php_boundaries() {
        assert_eq!(ordinal_suffix(1), "st");
        assert_eq!(ordinal_suffix(12), "th");
        assert_eq!(ordinal_suffix(23), "rd");
        assert_eq!(internet_beats(0), 41);
        assert_eq!(format_timezone_offset(19_800, false), "+0530");
        assert_eq!(format_timezone_offset(-18_000, true), "-05:00");
        assert_eq!(iso_week_and_year(1970, 1, 1, 4, 0), (1970, 1));
        assert_eq!(iso_week_and_year(2021, 1, 1, 5, 0), (2020, 53));
        assert_eq!(normalized_mktime_year(0), 2000);
        assert_eq!(normalized_mktime_year(69), 2069);
        assert_eq!(normalized_mktime_year(70), 1970);
        assert_eq!(normalized_mktime_year(100), 2000);
        assert_eq!(normalized_mktime_year(101), 101);
    }
}
