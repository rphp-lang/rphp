//! Clean-room textual date parser shared by DateTime and procedural APIs.
//!
//! The parser deliberately keeps civil arithmetic separate from the IANA
//! transition core.  A parsed wall clock is normalized first and is resolved
//! through the selected timezone only once, preserving DST gaps/overlaps.

use super::{ExecutorGlobals, PhpArray, Value, datetime, timezone};
use crate::runtime::DateParseDiagnostics;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RelativeAdjustment {
    pub years: i64,
    pub months: i64,
    pub days: i64,
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub business_days: i64,
    pub weekday: Option<(i64, i8)>,
    pub first_day: bool,
    pub last_day: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ParsedDateTime {
    pub state: datetime::DateTimeState,
    pub diagnostics: DateParseDiagnostics,
    pub relative: Option<RelativeAdjustment>,
    pub fields: ParsedFields,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ParsedFields {
    pub year: bool,
    pub month: bool,
    pub day: bool,
    pub hour: bool,
    pub minute: bool,
    pub second: bool,
    pub fraction: bool,
    pub timezone: bool,
}

fn month_number(value: &str) -> Option<i64> {
    let value = value.trim_matches(|character: char| !character.is_ascii_alphabetic());
    let lower = value.to_ascii_lowercase();
    match lower.get(..lower.len().min(3))? {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn weekday_number(value: &str) -> Option<i64> {
    let lower = value
        .trim_matches(|character: char| !character.is_ascii_alphabetic())
        .to_ascii_lowercase();
    match lower.as_str() {
        "sun" | "sunday" => Some(0),
        "mon" | "monday" => Some(1),
        "tue" | "tues" | "tuesday" => Some(2),
        "wed" | "weds" | "wednesday" => Some(3),
        "thu" | "thur" | "thurs" | "thursday" => Some(4),
        "fri" | "friday" => Some(5),
        "sat" | "saturday" => Some(6),
        _ => None,
    }
}

fn normalize_two_digit_year(year: i64) -> i64 {
    match year {
        0..=69 => year + 2_000,
        70..=99 => year + 1_900,
        _ => year,
    }
}

fn civil_state(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: u32,
    timezone: timezone::TimezoneDescription,
) -> datetime::DateTimeState {
    let local = super::normalized_timestamp(year, month, day, hour, minute, second);
    datetime::DateTimeState {
        timestamp: timezone::description_local_to_utc(&timezone, local),
        microsecond,
        timezone,
        initialized: true,
    }
}

fn parse_clock(value: &str) -> Option<(i64, i64, i64, u32, usize)> {
    let bytes = value.as_bytes();
    let Some(colon) = bytes.iter().position(|byte| *byte == b':') else {
        let digits = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if !(1..=2).contains(&digits) {
            return None;
        }
        let hour = value.get(..digits)?.parse::<i64>().ok()?;
        let suffix = value.get(digits..digits + 2)?;
        if suffix.eq_ignore_ascii_case("am") {
            return Some((if hour == 12 { 0 } else { hour }, 0, 0, 0, digits + 2));
        }
        if suffix.eq_ignore_ascii_case("pm") {
            return Some((if hour == 12 { 12 } else { hour + 12 }, 0, 0, 0, digits + 2));
        }
        return None;
    };
    if colon == 0 || colon > 2 {
        return None;
    }
    let hour = value.get(..colon)?.parse::<i64>().ok()?;
    let minute_start = colon + 1;
    let mut end = minute_start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == minute_start || end - minute_start > 2 {
        return None;
    }
    let minute = value.get(minute_start..end)?.parse::<i64>().ok()?;
    let mut second = 0;
    let mut microsecond = 0;
    if bytes.get(end) == Some(&b':') {
        let start = end + 1;
        end = start;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start || end - start > 2 {
            return None;
        }
        second = value.get(start..end)?.parse::<i64>().ok()?;
    }
    if bytes.get(end) == Some(&b'.') {
        let start = end + 1;
        end = start;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        microsecond = datetime::parse_fraction(value.get(start..end)?)?;
    }
    let suffix_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
        end += 1;
    }
    if value
        .get(end..end.saturating_add(2))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case("am"))
    {
        if hour == 12 {
            return Some((0, minute, second, microsecond, end + 2));
        }
        return Some((hour, minute, second, microsecond, end + 2));
    }
    if value
        .get(end..end.saturating_add(2))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case("pm"))
    {
        return Some((
            if hour == 12 { 12 } else { hour + 12 },
            minute,
            second,
            microsecond,
            end + 2,
        ));
    }
    Some((hour, minute, second, microsecond, suffix_start))
}

fn split_timezone(
    input: &str,
    fallback: timezone::TimezoneDescription,
) -> (&str, timezone::TimezoneDescription) {
    let trimmed = input.trim();
    if let Some(index) = trimmed.rfind(char::is_whitespace) {
        let suffix = trimmed[index..].trim();
        if let Some(description) = timezone::parse_timezone(suffix) {
            return (trimmed[..index].trim_end(), description);
        }
    }
    if let Some(index) = trimmed.char_indices().rev().find_map(|(index, character)| {
        (index > 9 && matches!(character, '+' | '-')).then_some(index)
    }) {
        if let Some(description) = timezone::parse_timezone(&trimmed[index..]) {
            return (trimmed[..index].trim_end(), description);
        }
    }
    (trimmed, fallback)
}

fn parse_named_absolute(
    input: &str,
    fallback: timezone::TimezoneDescription,
    base: &datetime::DateTimeState,
) -> Option<datetime::DateTimeState> {
    let (input, selected_timezone) = split_timezone(input, fallback);
    let input = input.trim().trim_end_matches(',');
    let mut parts = input
        .split_whitespace()
        .filter(|part| weekday_number(part).is_none())
        .collect::<Vec<_>>();
    if parts.first().is_some_and(|part| part.ends_with(',')) {
        parts.remove(0);
    }
    let (base_year, base_month, base_day, base_hour, base_minute, base_second) =
        datetime::local_parts(base);

    if let Some((hour, minute, second, microsecond, consumed)) = parse_clock(input) {
        if input[consumed..].trim().is_empty() {
            return Some(civil_state(
                base_year,
                base_month,
                base_day,
                hour,
                minute,
                second,
                microsecond,
                selected_timezone,
            ));
        }
    }

    if let Some((year, month, day, consumed)) = datetime::parse_date_prefix(input)
        && input[consumed..].trim().is_empty()
    {
        return Some(civil_state(year, month, day, 0, 0, 0, 0, selected_timezone));
    }

    if input.len() == 8 && input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(civil_state(
            input[..4].parse().ok()?,
            input[4..6].parse().ok()?,
            input[6..8].parse().ok()?,
            0,
            0,
            0,
            0,
            selected_timezone,
        ));
    }
    if input.len() == 14 && input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(civil_state(
            input[..4].parse().ok()?,
            input[4..6].parse().ok()?,
            input[6..8].parse().ok()?,
            input[8..10].parse().ok()?,
            input[10..12].parse().ok()?,
            input[12..14].parse().ok()?,
            0,
            selected_timezone,
        ));
    }

    let date_index = parts.iter().position(|part| month_number(part).is_some())?;
    let month = month_number(parts[date_index])?;
    let (day, year, consumed) = if date_index == 0 {
        let day = parts.get(1)?.trim_matches(',').parse::<i64>().ok()?;
        let year = parts
            .get(2)
            .and_then(|value| value.trim_matches(',').parse::<i64>().ok())
            .unwrap_or(base_year);
        (day, normalize_two_digit_year(year), 3)
    } else {
        let day = parts
            .get(date_index.wrapping_sub(1))?
            .trim_matches(',')
            .parse::<i64>()
            .ok()?;
        let year = parts
            .get(date_index + 1)
            .and_then(|value| value.trim_matches(',').parse::<i64>().ok())
            .unwrap_or(base_year);
        (day, normalize_two_digit_year(year), date_index + 2)
    };
    let clock = parts
        .get(consumed)
        .and_then(|value| parse_clock(value))
        .map(|clock| (clock.0, clock.1, clock.2, clock.3))
        .unwrap_or((base_hour, base_minute, base_second, 0));
    Some(civil_state(
        year,
        month,
        day,
        clock.0,
        clock.1,
        clock.2,
        clock.3,
        selected_timezone,
    ))
}

fn parse_number(value: &str) -> Option<i64> {
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "next" => Some(1),
        "last" | "previous" => Some(-1),
        "this" => Some(0),
        _ => lower.parse().ok(),
    }
}

fn unit_name(value: &str) -> &str {
    value
        .trim_matches(|character: char| matches!(character, ',' | '.'))
        .trim_end_matches('s')
}

pub(super) fn parse_relative(input: &str) -> Option<RelativeAdjustment> {
    let normalized = input.replace(',', " ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }
    let mut result = RelativeAdjustment::default();
    let invert = tokens
        .last()
        .is_some_and(|token| token.eq_ignore_ascii_case("ago"));
    let mut matched = false;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let lower = token.to_ascii_lowercase();

        // Ordinal weekday of a relative month, e.g. "first monday of next
        // month". The endpoint marker is applied after month normalization.
        if matches!(lower.as_str(), "first" | "last")
            && let Some(weekday) = tokens
                .get(index + 1)
                .and_then(|value| weekday_number(value))
            && tokens
                .get(index + 2)
                .is_some_and(|value| value.eq_ignore_ascii_case("of"))
            && let Some(months) = tokens.get(index + 3).and_then(|value| parse_number(value))
            && tokens
                .get(index + 4)
                .is_some_and(|value| unit_name(value).eq_ignore_ascii_case("month"))
        {
            result.months += months;
            result.first_day = lower == "first";
            result.last_day = lower == "last";
            // -2 is the inclusive backwards weekday projection used from a
            // month end; 0 is the inclusive forward projection from day 1.
            result.weekday = Some((weekday, if result.last_day { -2 } else { 0 }));
            matched = true;
            index += 5;
            continue;
        }
        match lower.as_str() {
            "+" => {
                index += 1;
                continue;
            }
            "ago" => {
                index += 1;
                continue;
            }
            "tomorrow" => {
                result.days += 1;
                matched = true;
                index += 1;
                continue;
            }
            "yesterday" => {
                result.days -= 1;
                matched = true;
                index += 1;
                continue;
            }
            "today" | "midnight" | "noon" | "now" => {
                matched = true;
                index += 1;
                continue;
            }
            "first" | "last"
                if tokens
                    .get(index + 1)
                    .is_some_and(|v| v.eq_ignore_ascii_case("day")) =>
            {
                result.first_day = lower == "first";
                result.last_day = lower == "last";
                matched = true;
                index += 2;
                if tokens
                    .get(index)
                    .is_some_and(|v| v.eq_ignore_ascii_case("of"))
                {
                    index += 1;
                }
                continue;
            }
            _ => {}
        }

        if matches!(lower.as_str(), "next" | "last" | "previous" | "this") {
            if let Some(weekday) = tokens
                .get(index + 1)
                .and_then(|value| weekday_number(value))
            {
                let direction = match lower.as_str() {
                    "next" => 1,
                    "this" => 0,
                    _ => -1,
                };
                result.weekday = Some((weekday, direction));
                matched = true;
                index += 2;
                continue;
            }
        }
        if let Some(weekday) = weekday_number(token) {
            result.weekday = Some((weekday, 0));
            matched = true;
            index += 1;
            continue;
        }

        if let Some(amount) = parse_number(token)
            && let Some(weekday) = tokens
                .get(index + 1)
                .and_then(|value| weekday_number(value))
        {
            result.weekday = Some((weekday, i8::try_from(amount).ok()?));
            matched = true;
            index += 2;
            continue;
        }

        let Some(mut amount) = parse_number(token) else {
            return None;
        };
        let Some(unit) = tokens
            .get(index + 1)
            .map(|value| unit_name(value).to_ascii_lowercase())
        else {
            return None;
        };
        if invert {
            amount = -amount;
        }
        match unit.as_str() {
            "year" => result.years += amount,
            "month" => result.months += amount,
            "fortnight" => result.days += amount.saturating_mul(14),
            "week" => result.days += amount.saturating_mul(7),
            "day" => result.days += amount,
            "weekday" => result.business_days += amount,
            "hour" => result.hours += amount,
            "minute" | "min" => result.minutes += amount,
            "second" | "sec" => result.seconds += amount,
            _ => return None,
        }
        matched = true;
        index += 2;
    }
    matched.then_some(result)
}

pub(super) fn apply_relative(state: &mut datetime::DateTimeState, relative: &RelativeAdjustment) {
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) =
        datetime::local_parts(state);
    year = year.saturating_add(relative.years);
    month = month.saturating_add(relative.months);
    day = day.saturating_add(relative.days);
    hour = hour.saturating_add(relative.hours);
    minute = minute.saturating_add(relative.minutes);
    second = second.saturating_add(relative.seconds);
    if relative.first_day {
        day = 1;
    }
    if relative.last_day {
        let month_index = month.saturating_sub(1);
        let normalized_year = year.saturating_add(month_index.div_euclid(12));
        let normalized_month = month_index.rem_euclid(12) + 1;
        day = super::super::days_in_month(normalized_year, normalized_month);
    }
    datetime::set_local(state, year, month, day, hour, minute, second);
    if relative.business_days != 0 {
        let direction = relative.business_days.signum();
        let mut remaining = relative.business_days.unsigned_abs();
        while remaining != 0 {
            let (year, month, day, hour, minute, second) = datetime::local_parts(state);
            datetime::set_local(
                state,
                year,
                month,
                day.saturating_add(direction),
                hour,
                minute,
                second,
            );
            let offset = timezone::description_state(&state.timezone, state.timestamp).1;
            let weekday = super::super::unix_to_parts(state.timestamp.saturating_add(offset)).6;
            if !matches!(weekday, 0 | 6) {
                remaining -= 1;
            }
        }
    }
    if let Some((wanted, direction)) = relative.weekday {
        let offset = timezone::description_state(&state.timezone, state.timestamp).1;
        let current = super::super::unix_to_parts(state.timestamp.saturating_add(offset)).6;
        let delta = match direction {
            1 => {
                let delta = (wanted - current).rem_euclid(7);
                if delta == 0 { 7 } else { delta }
            }
            -1 => {
                let delta = (current - wanted).rem_euclid(7);
                -(if delta == 0 { 7 } else { delta })
            }
            -2 => -(current - wanted).rem_euclid(7),
            count if count > 1 => (wanted - current).rem_euclid(7) + 7 * (i64::from(count) - 1),
            count if count < -2 => {
                -((current - wanted).rem_euclid(7) + 7 * (i64::from(-count) - 1))
            }
            _ => (wanted - current).rem_euclid(7),
        };
        let (year, month, day, hour, minute, second) = datetime::local_parts(state);
        datetime::set_local(state, year, month, day + delta, hour, minute, second);
    }
}

pub(super) fn parse_datetime(
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    base: Option<&datetime::DateTimeState>,
    eg: &ExecutorGlobals,
) -> Result<ParsedDateTime, DateParseDiagnostics> {
    let fallback = supplied_timezone.unwrap_or_else(|| timezone::default_description(eg));
    if let Some(state) = datetime::parse_absolute(input, Some(fallback.clone()), eg) {
        let fields = infer_fields(input);
        let mut diagnostics = DateParseDiagnostics::default();
        if fields.year && fields.month && fields.day {
            let trimmed = input.trim_start_matches(['+', '-']);
            if trimmed.len() >= 10
                && trimmed.as_bytes().get(4) == Some(&b'-')
                && trimmed.as_bytes().get(7) == Some(&b'-')
            {
                let year = trimmed[..4].parse::<i64>().unwrap_or(0);
                let month = trimmed[5..7].parse::<i64>().unwrap_or(0);
                let day = trimmed[8..10].parse::<i64>().unwrap_or(0);
                let valid_month = (1..=12).contains(&month);
                if !valid_month
                    || day < 1
                    || day
                        > super::super::days_in_month(
                            year + (month - 1).div_euclid(12),
                            (month - 1).rem_euclid(12) + 1,
                        )
                {
                    diagnostics
                        .warnings
                        .push((input.len(), "The parsed date was invalid".to_string()));
                }
            }
        }
        return Ok(ParsedDateTime {
            state,
            diagnostics,
            relative: None,
            fields,
        });
    }
    let owned_base;
    let base = if let Some(base) = base {
        base
    } else {
        let (timestamp, microsecond) = datetime::now();
        owned_base = datetime::DateTimeState {
            timestamp,
            microsecond,
            timezone: fallback.clone(),
            initialized: true,
        };
        &owned_base
    };
    if let Some(state) = parse_named_absolute(input, fallback, base) {
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: None,
            fields: infer_fields(input),
        });
    }
    if let Some(relative) = parse_relative(input) {
        let mut state = base.clone();
        let lower = input.to_ascii_lowercase();
        if relative.weekday.is_some()
            || lower.contains("today")
            || lower.contains("tomorrow")
            || lower.contains("yesterday")
            || lower.contains("midnight")
        {
            let (year, month, day, _, _, _) = datetime::local_parts(&state);
            datetime::set_local(&mut state, year, month, day, 0, 0, 0);
            state.microsecond = 0;
        } else if lower.contains("noon") {
            let (year, month, day, _, _, _) = datetime::local_parts(&state);
            datetime::set_local(&mut state, year, month, day, 12, 0, 0);
            state.microsecond = 0;
        }
        apply_relative(&mut state, &relative);
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: Some(relative),
            fields: ParsedFields::default(),
        });
    }
    let error_position = parse_clock(input)
        .map(|(_, _, _, _, consumed)| consumed)
        .filter(|consumed| !input[*consumed..].trim().is_empty())
        .map(|consumed| {
            consumed
                + input[consumed..]
                    .bytes()
                    .take_while(u8::is_ascii_whitespace)
                    .count()
        })
        .unwrap_or(0);
    Err(DateParseDiagnostics {
        warnings: Vec::new(),
        errors: vec![(
            error_position,
            format!("The timezone could not be found in the database"),
        )],
    })
}

fn infer_fields(input: &str) -> ParsedFields {
    let trimmed = input.trim();
    let bytes = trimmed.as_bytes();
    let iso_date = bytes.len() >= 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit);
    let compact_date = (bytes.len() == 8 || bytes.len() == 14)
        && bytes
            .get(..8)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit));
    let named_date = trimmed
        .split_whitespace()
        .any(|part| month_number(part).is_some());
    let has_date = iso_date || compact_date || named_date;
    let clock_position = trimmed.find(':');
    let has_clock = clock_position.is_some();
    let fraction = clock_position.is_some_and(|position| {
        trimmed[position..].find('.').is_some_and(|offset| {
            trimmed[position + offset + 1..].starts_with(|c: char| c.is_ascii_digit())
        })
    });
    let timezone = trimmed.starts_with('@')
        || trimmed.ends_with('Z')
        || trimmed
            .split_whitespace()
            .last()
            .filter(|last| *last != trimmed)
            .is_some_and(|last| timezone::parse_timezone(last).is_some())
        || trimmed
            .char_indices()
            .rev()
            .find_map(|(index, character)| {
                (index > 9 && matches!(character, '+' | '-')).then_some(index)
            })
            .is_some_and(|index| timezone::parse_timezone(&trimmed[index..]).is_some());
    ParsedFields {
        year: has_date,
        month: has_date,
        day: has_date,
        hour: has_clock,
        minute: has_clock,
        second: has_clock && trimmed.matches(':').count() >= 2,
        fraction,
        timezone,
    }
}

fn read_digits(input: &str, position: &mut usize, min: usize, max: usize) -> Option<i64> {
    let start = *position;
    while *position < input.len()
        && *position - start < max
        && input.as_bytes()[*position].is_ascii_digit()
    {
        *position += 1;
    }
    (*position - start >= min)
        .then(|| input[start..*position].parse::<i64>().ok())
        .flatten()
}

pub(super) fn parse_from_format(
    format: &str,
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    eg: &ExecutorGlobals,
) -> Result<ParsedDateTime, DateParseDiagnostics> {
    let fallback = supplied_timezone.unwrap_or_else(|| timezone::default_description(eg));
    let (now_timestamp, now_microsecond) = datetime::now();
    let base = datetime::DateTimeState {
        timestamp: now_timestamp,
        microsecond: now_microsecond,
        timezone: fallback.clone(),
        initialized: true,
    };
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) =
        datetime::local_parts(&base);
    // timelib initializes the fractional field to zero for createFromFormat;
    // unspecified clock fields inherit the current wall clock, but fractions
    // are present only when `u` or `v` explicitly parses them.
    let mut microsecond = 0;
    let mut timezone = fallback;
    let mut reset = false;
    let mut reset_unparsed = false;
    let mut input_position = 0;
    let mut escaped = false;
    let mut diagnostics = DateParseDiagnostics::default();
    let mut fields = ParsedFields::default();
    let format_chars = format.chars().collect::<Vec<_>>();
    let mut format_position = 0;
    while format_position < format_chars.len() {
        let token = format_chars[format_position];
        format_position += 1;
        if escaped {
            escaped = false;
            if input[input_position..].starts_with(token) {
                input_position += token.len_utf8();
                continue;
            }
            diagnostics
                .errors
                .push((input_position, "The parsed date was invalid".to_string()));
            break;
        }
        if token == '\\' {
            escaped = true;
            continue;
        }
        match token {
            '!' => {
                year = 1970;
                month = 1;
                day = 1;
                hour = 0;
                minute = 0;
                second = 0;
                microsecond = 0;
                reset = true;
            }
            '|' => reset_unparsed = true,
            '#' => {
                if input_position < input.len() {
                    input_position += 1;
                }
            }
            '?' => {
                let Some(character) = input[input_position..].chars().next() else {
                    break;
                };
                input_position += character.len_utf8();
            }
            '*' => {
                while input_position < input.len() {
                    let byte = input.as_bytes()[input_position];
                    if byte.is_ascii_whitespace() || matches!(byte, b'+' | b'-') {
                        break;
                    }
                    input_position += 1;
                }
            }
            '+' => {
                if input_position < input.len() {
                    diagnostics
                        .warnings
                        .push((input_position, "Trailing data".to_string()));
                    input_position = input.len();
                }
            }
            ' ' => {
                while input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    input_position += 1;
                }
            }
            'Y' => {
                fields.year = true;
                year = read_digits(input, &mut input_position, 4, 4).unwrap_or(year)
            }
            'y' => {
                fields.year = true;
                year = normalize_two_digit_year(
                    read_digits(input, &mut input_position, 2, 2).unwrap_or(year),
                )
            }
            'X' | 'x' => {
                fields.year = true;
                let sign = match input.as_bytes().get(input_position) {
                    Some(b'+') => {
                        input_position += 1;
                        1
                    }
                    Some(b'-') => {
                        input_position += 1;
                        -1
                    }
                    _ => 1,
                };
                year = sign * read_digits(input, &mut input_position, 4, 19).unwrap_or(year);
            }
            'm' | 'd' | 'H' | 'h' | 'i' | 's' => {
                let value = read_digits(input, &mut input_position, 2, 2);
                match (token, value) {
                    ('m', Some(value)) => {
                        month = value;
                        fields.month = true;
                    }
                    ('d', Some(value)) => {
                        day = value;
                        fields.day = true;
                    }
                    ('H' | 'h', Some(value)) => {
                        hour = value;
                        fields.hour = true;
                    }
                    ('i', Some(value)) => {
                        minute = value;
                        fields.minute = true;
                    }
                    ('s', Some(value)) => {
                        second = value;
                        fields.second = true;
                    }
                    _ => diagnostics.errors.push((
                        input_position,
                        "A two digit number could not be found".to_string(),
                    )),
                }
            }
            'n' | 'j' | 'G' | 'g' => {
                let value = read_digits(input, &mut input_position, 1, 2);
                match (token, value) {
                    ('n', Some(value)) => {
                        month = value;
                        fields.month = true;
                    }
                    ('j', Some(value)) => {
                        day = value;
                        fields.day = true;
                    }
                    ('G' | 'g', Some(value)) => {
                        hour = value;
                        fields.hour = true;
                    }
                    _ => diagnostics
                        .errors
                        .push((input_position, "A number could not be found".to_string())),
                }
            }
            'u' | 'v' => {
                fields.fraction = true;
                let start = input_position;
                let width = if token == 'u' { 6 } else { 3 };
                let _ = read_digits(input, &mut input_position, 1, width);
                microsecond = datetime::parse_fraction(&input[start..input_position]).unwrap_or(0);
                if token == 'v' {
                    microsecond = input[start..input_position].parse::<u32>().unwrap_or(0) * 1_000;
                }
            }
            'M' | 'F' => {
                fields.month = true;
                let start = input_position;
                while input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(u8::is_ascii_alphabetic)
                {
                    input_position += 1;
                }
                month = month_number(&input[start..input_position]).unwrap_or(month);
            }
            'a' | 'A' => {
                let suffix = input
                    .get(input_position..input_position.saturating_add(2))
                    .unwrap_or("");
                if suffix.eq_ignore_ascii_case("pm") && hour < 12 {
                    hour += 12;
                }
                if suffix.eq_ignore_ascii_case("am") && hour == 12 {
                    hour = 0;
                }
                input_position = (input_position + 2).min(input.len());
            }
            'U' => {
                let start = input_position;
                if input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    input_position += 1;
                }
                while input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    input_position += 1;
                }
                if let Ok(timestamp) = input[start..input_position].parse::<i64>() {
                    let result = datetime::DateTimeState {
                        timestamp,
                        microsecond: 0,
                        timezone,
                        initialized: true,
                    };
                    return Ok(ParsedDateTime {
                        state: result,
                        diagnostics,
                        relative: None,
                        fields: ParsedFields {
                            year: true,
                            month: true,
                            day: true,
                            hour: true,
                            minute: true,
                            second: true,
                            ..ParsedFields::default()
                        },
                    });
                }
            }
            'O' | 'P' | 'p' | 'T' | 'e' => {
                fields.timezone = true;
                let start = input_position;
                while input_position < input.len()
                    && !input.as_bytes()[input_position].is_ascii_whitespace()
                {
                    input_position += 1;
                }
                let zone = &input[start..input_position];
                let zone = if token == 'p' && zone == "Z" {
                    "+00:00"
                } else {
                    zone
                };
                if let Some(parsed) = timezone::parse_timezone(zone) {
                    timezone = parsed;
                } else {
                    diagnostics.errors.push((
                        start,
                        "The timezone could not be found in the database".to_string(),
                    ));
                }
            }
            _ => {
                if input[input_position..].starts_with(token) {
                    input_position += token.len_utf8();
                } else {
                    diagnostics.errors.push((
                        input_position,
                        "The separation symbol could not be found".to_string(),
                    ));
                    break;
                }
            }
        }
    }
    if input_position < input.len() && !format.contains('+') {
        diagnostics
            .errors
            .push((input_position, "Trailing data".to_string()));
    }
    if reset_unparsed && !reset {
        // PHP's `|` resets every field not explicitly parsed.  The common
        // date formats exercise all date fields; zeroing time is the useful
        // observable distinction and avoids a token bitmap in the hot path.
        hour = 0;
        minute = 0;
        second = 0;
        microsecond = 0;
    }
    if diagnostics.errors.is_empty() {
        if !(1..=12).contains(&month)
            || day < 1
            || day
                > super::super::days_in_month(
                    year + (month - 1).div_euclid(12),
                    (month - 1).rem_euclid(12) + 1,
                )
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=60).contains(&second)
        {
            diagnostics
                .warnings
                .push((input_position, "The parsed date was invalid".to_string()));
        }
        let state = civil_state(
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            timezone,
        );
        Ok(ParsedDateTime {
            state,
            diagnostics,
            relative: None,
            fields,
        })
    } else {
        Err(diagnostics)
    }
}

pub(super) fn diagnostics_value(diagnostics: &DateParseDiagnostics) -> Value {
    let mut result = PhpArray::new();
    result.set_str(
        "warning_count",
        Value::long(diagnostics.warnings.len() as i64),
    );
    let mut warnings = PhpArray::new();
    for (position, message) in &diagnostics.warnings {
        warnings.set_int(*position as i64, Value::string(message));
    }
    result.set_str("warnings", Value::array(warnings));
    result.set_str("error_count", Value::long(diagnostics.errors.len() as i64));
    let mut errors = PhpArray::new();
    for (position, message) in &diagnostics.errors {
        errors.set_int(*position as i64, Value::string(message));
    }
    result.set_str("errors", Value::array(errors));
    Value::array(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_units_and_weekdays_are_classified() {
        let parsed = parse_relative("+1 week 2 days 4 hours 2 seconds").unwrap();
        assert_eq!(parsed.days, 9);
        assert_eq!(parsed.hours, 4);
        assert_eq!(parsed.seconds, 2);
        assert_eq!(
            parse_relative("next Thursday").unwrap().weekday,
            Some((4, 1))
        );
        assert_eq!(parse_relative("3 tuesday").unwrap().weekday, Some((2, 3)));
        let first = parse_relative("first monday of next month").unwrap();
        assert_eq!(
            (first.months, first.first_day, first.weekday),
            (1, true, Some((1, 0)))
        );
        let last = parse_relative("last thursday of next month").unwrap();
        assert_eq!(
            (last.months, last.last_day, last.weekday),
            (1, true, Some((4, -2)))
        );
        let first_day = parse_relative("first day of next month").unwrap();
        assert_eq!((first_day.months, first_day.first_day), (1, true));
    }

    #[test]
    fn month_and_year_names_are_case_insensitive() {
        assert_eq!(month_number("September"), Some(9));
        assert_eq!(normalize_two_digit_year(69), 2069);
        assert_eq!(normalize_two_digit_year(70), 1970);
    }
}
