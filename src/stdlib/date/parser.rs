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
    pub microseconds: i64,
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
    if let Some(month) = match lower.as_str() {
        "i" => Some(1),
        "ii" => Some(2),
        "iii" => Some(3),
        "iv" => Some(4),
        "v" => Some(5),
        "vi" => Some(6),
        "vii" => Some(7),
        "viii" => Some(8),
        "ix" => Some(9),
        "x" => Some(10),
        "xi" => Some(11),
        "xii" => Some(12),
        _ => None,
    } {
        return Some(month);
    }
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
    let normalized;
    let original_len = value.len();
    let mut dotted_suffix_extra = 0;
    let lower = value.to_ascii_lowercase();
    let value = if lower.ends_with("a.m.") {
        normalized = format!("{}am", &value[..value.len() - 4]);
        dotted_suffix_extra = 2;
        normalized.as_str()
    } else if lower.ends_with("p.m.") {
        normalized = format!("{}pm", &value[..value.len() - 4]);
        dotted_suffix_extra = 2;
        normalized.as_str()
    } else {
        value
    };
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
            return Some((
                if hour == 12 { 0 } else { hour },
                0,
                0,
                0,
                (digits + 2 + dotted_suffix_extra).min(original_len),
            ));
        }
        if suffix.eq_ignore_ascii_case("pm") {
            return Some((
                if hour == 12 { 12 } else { hour + 12 },
                0,
                0,
                0,
                (digits + 2 + dotted_suffix_extra).min(original_len),
            ));
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
            return Some((
                0,
                minute,
                second,
                microsecond,
                (end + 2 + dotted_suffix_extra).min(original_len),
            ));
        }
        return Some((
            hour,
            minute,
            second,
            microsecond,
            (end + 2 + dotted_suffix_extra).min(original_len),
        ));
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
            (end + 2 + dotted_suffix_extra).min(original_len),
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
        if let Some(description) = parse_datetime_timezone(suffix) {
            return (trimmed[..index].trim_end(), description);
        }
    }
    if let Some(index) = trimmed.char_indices().rev().find_map(|(index, character)| {
        (index > 9 && matches!(character, '+' | '-')).then_some(index)
    }) {
        if let Some(description) = parse_datetime_timezone(&trimmed[index..]) {
            return (trimmed[..index].trim_end(), description);
        }
    }
    (trimmed, fallback)
}

fn parse_datetime_timezone(value: &str) -> Option<timezone::TimezoneDescription> {
    timezone::parse_timezone(value)
        .or_else(|| value.strip_prefix("GMT").and_then(timezone::parse_timezone))
        .or_else(|| {
            let bytes = value.as_bytes();
            if let [sign @ (b'+' | b'-'), hour_1, hour_2] = bytes
                && [hour_1, hour_2].iter().all(|byte| byte.is_ascii_digit())
            {
                return timezone::parse_timezone(&format!(
                    "{}{}{}:00",
                    char::from(*sign),
                    char::from(*hour_1),
                    char::from(*hour_2)
                ));
            }
            if let [sign @ (b'+' | b'-'), hour, b':', minute] = bytes
                && [hour, minute].iter().all(|byte| byte.is_ascii_digit())
            {
                return timezone::parse_timezone(&format!(
                    "{}0{}:0{}",
                    char::from(*sign),
                    char::from(*hour),
                    char::from(*minute)
                ));
            }
            if let [sign @ (b'+' | b'-'), hour_1, hour_2, b':', minute] = bytes
                && [hour_1, hour_2, minute]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
            {
                return timezone::parse_timezone(&format!(
                    "{}{}{}:0{}",
                    char::from(*sign),
                    char::from(*hour_1),
                    char::from(*hour_2),
                    char::from(*minute)
                ));
            }
            None
        })
}

fn parse_clock_flexible(value: &str) -> Option<(i64, i64, i64, u32, usize)> {
    parse_clock(value).or_else(|| {
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 4 && digits == value.len() {
            return Some((
                value[..2].parse().ok()?,
                value[2..].parse().ok()?,
                0,
                0,
                digits,
            ));
        }
        if !(1..=2).contains(&digits) || digits != value.len() {
            return None;
        }
        Some((value.parse().ok()?, 0, 0, 0, digits))
    })
}

fn expanded_named_tokens(input: &str) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(input.len() + 8);
    for (index, character) in chars.iter().copied().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|index| chars.get(index))
            .copied();
        let next = chars.get(index + 1).copied();
        let is_compact_timezone_offset = matches!(character, '+' | '-')
            && previous.is_some_and(char::is_whitespace)
            && chars
                .get(index + 1..index + 5)
                .is_some_and(|digits| digits.iter().all(char::is_ascii_digit))
            && chars
                .get(index + 5)
                .is_none_or(|character| character.is_whitespace());
        let boundary = previous.is_some_and(|previous| {
            (previous.is_ascii_alphabetic() && character.is_ascii_digit())
                || (previous.is_ascii_digit()
                    && character.is_ascii_alphabetic()
                    && !matches!(
                        chars[index..]
                            .iter()
                            .take(2)
                            .collect::<String>()
                            .to_ascii_lowercase()
                            .as_str(),
                        "am" | "pm"
                    ))
                || (!is_compact_timezone_offset
                    && (character == '+' || (character == '-' && !previous.is_ascii_digit()))
                    && !matches!(previous, 'e' | 'E')
                    && next.is_some_and(|next| next.is_ascii_digit()))
        });
        if boundary && !output.chars().last().is_some_and(char::is_whitespace) {
            output.push(' ');
        }
        output.push(character);
        if !is_compact_timezone_offset
            && (character == '+'
                || (character == '-'
                    && !previous.is_some_and(|previous| previous.is_ascii_digit())))
            && next.is_some_and(|next| next.is_ascii_digit())
            && !output.ends_with("  ")
        {
            output.push(' ');
        }
    }
    output
}

fn parse_numeric_absolute(
    input: &str,
    fallback: timezone::TimezoneDescription,
) -> Option<datetime::DateTimeState> {
    let (input, selected_timezone) = split_timezone(input, fallback);
    if input.len() >= 7
        && input
            .as_bytes()
            .get(4)
            .is_some_and(|byte| matches!(byte, b'W' | b'w'))
        && input[..4].bytes().all(|byte| byte.is_ascii_digit())
    {
        let year = input[..4].parse::<i64>().ok()?;
        let week = input[5..7].parse::<i64>().ok()?;
        let weekday = input
            .get(7..)
            .map(|suffix| suffix.trim_start_matches(|character| matches!(character, '-' | ' ')))
            .filter(|suffix| !suffix.is_empty())
            .and_then(|suffix| suffix[..1].parse::<i64>().ok())
            .unwrap_or(1);
        let january_fourth = super::normalized_timestamp(year, 1, 4, 0, 0, 0);
        let january_fourth_weekday = super::super::unix_to_parts(january_fourth).6;
        let monday =
            january_fourth.saturating_sub((january_fourth_weekday + 6).rem_euclid(7) * 86_400);
        let timestamp = monday
            .saturating_add(((week - 1).saturating_mul(7) + weekday - 1).saturating_mul(86_400));
        let (year, month, day, ..) = super::super::unix_to_parts(timestamp);
        return Some(civil_state(year, month, day, 0, 0, 0, 0, selected_timezone));
    }
    if input.len() >= 9
        && input
            .get(8..9)
            .is_some_and(|value| value.eq_ignore_ascii_case("t"))
        && input[..8].bytes().all(|byte| byte.is_ascii_digit())
    {
        let clock_text = input[9..].trim_end_matches(['Z', 'z']);
        if clock_text.len() == 6 && clock_text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some(civil_state(
                input[..4].parse().ok()?,
                input[4..6].parse().ok()?,
                input[6..8].parse().ok()?,
                clock_text[..2].parse().ok()?,
                clock_text[2..4].parse().ok()?,
                clock_text[4..6].parse().ok()?,
                0,
                if input.ends_with('Z') || input.ends_with('z') {
                    timezone::TimezoneDescription {
                        kind: 2,
                        name: "Z".to_string(),
                    }
                } else {
                    selected_timezone
                },
            ));
        }
    }
    let split = input
        .char_indices()
        .find(|(_, character)| character.is_ascii_whitespace() || *character == 'T')
        .map(|(index, _)| index)
        .unwrap_or(input.len());
    let date = &input[..split];
    let remainder = input[split..]
        .trim_start_matches(|character: char| character.is_ascii_whitespace() || character == 'T');

    let (year, month, day) = if date.len() == 7 && date.bytes().all(|byte| byte.is_ascii_digit()) {
        let year = date[..4].parse::<i64>().ok()?;
        let ordinal = date[4..].parse::<i64>().ok()?;
        let timestamp = super::normalized_timestamp(year, 1, ordinal, 0, 0, 0);
        let (year, month, day, ..) = super::super::unix_to_parts(timestamp);
        (year, month, day)
    } else {
        let separator = ['-', '.', '/']
            .into_iter()
            .find(|separator| date.contains(*separator))?;
        let parts = date.split(separator).collect::<Vec<_>>();
        match parts.as_slice() {
            [year, ordinal] if year.len() >= 4 && ordinal.len() == 3 && separator != '/' => {
                let year = year.parse::<i64>().ok()?;
                let ordinal = ordinal.parse::<i64>().ok()?;
                let timestamp = super::normalized_timestamp(year, 1, ordinal, 0, 0, 0);
                let (year, month, day, ..) = super::super::unix_to_parts(timestamp);
                (year, month, day)
            }
            [year, month] if year.len() >= 4 && separator == '-' => {
                (year.parse().ok()?, month.parse().ok()?, 1)
            }
            [year, month, day] if year.len() >= 4 => {
                (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
            }
            [month, day, year] if separator == '/' && year.len() >= 2 => (
                if year.len() == 2 {
                    normalize_two_digit_year(year.parse().ok()?)
                } else {
                    year.parse().ok()?
                },
                month.parse().ok()?,
                day.parse().ok()?,
            ),
            [day, month, year] if year.len() >= 4 => {
                (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
            }
            _ => return None,
        }
    };
    let (hour, minute, second, microsecond) = if remainder.is_empty() {
        (0, 0, 0, 0)
    } else {
        let parsed = parse_clock_flexible(remainder)?;
        if !remainder[parsed.4..].trim().is_empty() {
            return None;
        }
        (parsed.0, parsed.1, parsed.2, parsed.3)
    };
    Some(civil_state(
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
        selected_timezone,
    ))
}

fn parse_named_absolute(
    input: &str,
    fallback: timezone::TimezoneDescription,
    base: &datetime::DateTimeState,
) -> Option<datetime::DateTimeState> {
    let (input, selected_timezone) = split_timezone(input, fallback);
    // timelib accepts a redundant zone abbreviation after an explicit offset
    // (for example `+0000 GMT`). Peel both suffixes before parsing the civil
    // portion, with the explicit offset taking precedence.
    let (input, selected_timezone) = split_timezone(input, selected_timezone);
    let input = input.trim().trim_end_matches(',');
    let expanded = expanded_named_tokens(&input.replace('-', " "));
    let parts = expanded.split_whitespace().collect::<Vec<_>>();
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

    let month_index = parts.iter().position(|part| month_number(part).is_some())?;
    let month = month_number(parts[month_index])?;
    let mut selected_timezone = selected_timezone;
    let mut clock = None;
    let mut numbers = Vec::new();
    let mut first_day = false;
    let mut ordinal_weekday = None;
    let mut unrecognized = false;
    for (index, part) in parts.iter().enumerate() {
        if index == month_index {
            continue;
        }
        let cleaned = part.trim_matches(|character: char| matches!(character, ',' | '.'));
        if cleaned.is_empty() || weekday_number(cleaned).is_some() {
            continue;
        }
        if let Some(parsed) = parse_clock(cleaned)
            && parsed.4 == cleaned.len()
        {
            clock = Some((parsed.0, parsed.1, parsed.2, parsed.3));
            continue;
        }
        if let Some(zone) = parse_datetime_timezone(cleaned) {
            selected_timezone = zone;
            continue;
        }
        if let Ok(number) = cleaned.parse::<i64>() {
            numbers.push((index, cleaned.len(), number));
            continue;
        }
        match cleaned.to_ascii_lowercase().as_str() {
            "of" | "day" | "st" | "nd" | "rd" | "th" => {}
            "am" | "pm" => {
                let Some((hour, minute, second, microsecond)) = clock else {
                    unrecognized = true;
                    continue;
                };
                clock = Some((
                    match cleaned.to_ascii_lowercase().as_str() {
                        "am" if hour == 12 => 0,
                        "pm" if hour < 12 => hour + 12,
                        _ => hour,
                    },
                    minute,
                    second,
                    microsecond,
                ));
            }
            "first" => {
                first_day = true;
                if let Some(weekday) = parts.get(index + 1).and_then(|value| weekday_number(value))
                {
                    ordinal_weekday = Some((weekday, 0));
                }
            }
            "second" | "third" | "fourth" | "fifth" => {
                if let Some(weekday) = parts.get(index + 1).and_then(|value| weekday_number(value))
                {
                    ordinal_weekday = Some((weekday, parse_number(cleaned)? as i8));
                    first_day = true;
                }
            }
            "last" => {
                if let Some(weekday) = parts.get(index + 1).and_then(|value| weekday_number(value))
                {
                    ordinal_weekday = Some((weekday, -2));
                } else {
                    unrecognized = true;
                }
            }
            "midnight" => clock = Some((0, 0, 0, 0)),
            "noon" => clock = Some((12, 0, 0, 0)),
            _ => unrecognized = true,
        }
    }
    if unrecognized {
        return None;
    }

    let before = numbers.iter().find(|(index, _, _)| *index < month_index);
    let after = numbers
        .iter()
        .filter(|(index, _, _)| *index > month_index)
        .collect::<Vec<_>>();
    let before_is_year = before.is_some_and(|(_, width, _)| *width >= 3);
    let mut day = before
        .filter(|_| !before_is_year)
        .map(|(_, _, value)| *value)
        .unwrap_or(base_day);
    let mut year = base_year;
    if let Some((_, _, value)) = before.filter(|_| before_is_year) {
        year = *value;
        day = 1;
    }
    if let Some((_, width, value)) = after.first().copied() {
        if before.is_some() && !before_is_year {
            year = if *width <= 2 {
                normalize_two_digit_year(*value)
            } else {
                *value
            };
        } else if after.len() > 1 || *width <= 2 {
            day = *value;
            if let Some((_, width, value)) = after.get(1).copied() {
                year = if *width <= 2 {
                    normalize_two_digit_year(*value)
                } else {
                    *value
                };
            }
        } else {
            year = *value;
            day = 1;
        }
    }
    if first_day || ordinal_weekday.is_some() {
        day = 1;
    }
    let clock = clock.unwrap_or((base_hour, base_minute, base_second, base.microsecond));
    let mut state = civil_state(
        year,
        month,
        day,
        clock.0,
        clock.1,
        clock.2,
        clock.3,
        selected_timezone,
    );
    if let Some((weekday, direction)) = ordinal_weekday {
        apply_relative(
            &mut state,
            &RelativeAdjustment {
                weekday: Some((weekday, direction)),
                ..RelativeAdjustment::default()
            },
        );
        if clock == (base_hour, base_minute, base_second, base.microsecond) {
            let (year, month, day, _, _, _) = datetime::local_parts(&state);
            datetime::set_local(&mut state, year, month, day, 0, 0, 0);
            state.microsecond = 0;
        }
    }
    Some(state)
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
        "first" => Some(1),
        "second" => Some(2),
        "third" => Some(3),
        "fourth" => Some(4),
        "fifth" => Some(5),
        "next" => Some(1),
        "last" | "previous" => Some(-1),
        "this" => Some(0),
        _ => lower.parse().ok(),
    }
}

fn unit_name(value: &str) -> &str {
    let value = value.trim_matches(|character: char| matches!(character, ',' | '.'));
    if value.len() > 1 && !matches!(value, "ms" | "µs") {
        value.strip_suffix('s').unwrap_or(value)
    } else {
        value
    }
}

fn parse_relative_number(value: &str) -> Option<i64> {
    parse_number(value).or_else(|| {
        value
            .split_once('.')
            .and_then(|(_, fractional)| fractional.parse::<i64>().ok())
    })
}

pub(super) fn parse_relative(input: &str) -> Option<RelativeAdjustment> {
    let sign_normalized = input
        .replace("--", "+")
        .replace("++", "+")
        .replace("+-", "-")
        .replace("-+", "-");
    let normalized = expanded_named_tokens(&sign_normalized.replace(',', " "));
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
        if matches!(
            lower.as_str(),
            "first" | "second" | "third" | "fourth" | "fifth" | "last"
        ) && let Some(weekday) = tokens
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
            result.first_day = lower != "last";
            result.last_day = lower == "last";
            // -2 is the inclusive backwards weekday projection used from a
            // month end; 0 is the inclusive forward projection from day 1.
            result.weekday = Some((
                weekday,
                if result.last_day {
                    -2
                } else if lower == "first" {
                    0
                } else {
                    parse_number(&lower).unwrap_or(1) as i8
                },
            ));
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
        if let Some(weekday) = weekday_number(token)
            && tokens
                .get(index + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("next"))
            && tokens
                .get(index + 2)
                .is_some_and(|value| unit_name(value).eq_ignore_ascii_case("week"))
        {
            result.weekday = Some((weekday, 1));
            matched = true;
            index += 3;
            continue;
        }
        if matches!(lower.as_str(), "next" | "this")
            && tokens
                .get(index + 1)
                .is_some_and(|value| unit_name(value).eq_ignore_ascii_case("week"))
            && let Some(weekday) = tokens
                .get(index + 2)
                .and_then(|value| weekday_number(value))
        {
            result.weekday = Some((weekday, if lower == "next" { 1 } else { 0 }));
            matched = true;
            index += 3;
            continue;
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

        if token == "+" || token == "-" {
            let sign = if token == "-" { -1 } else { 1 };
            let Some(number) = tokens
                .get(index + 1)
                .and_then(|value| parse_relative_number(value))
            else {
                return None;
            };
            let Some(unit) = tokens
                .get(index + 2)
                .map(|value| unit_name(value).to_ascii_lowercase())
            else {
                return None;
            };
            let amount = sign * number * if invert { -1 } else { 1 };
            match unit.as_str() {
                "year" => result.years += amount,
                "month" => result.months += amount,
                "week" => result.days += amount.saturating_mul(7),
                "day" => result.days += amount,
                "hour" | "hr" | "h" => result.hours += amount,
                "minute" | "min" => result.minutes += amount,
                "second" | "sec" => result.seconds += amount,
                // timelib accepts this ambiguous one-letter token but does
                // not classify it as the seconds unit.
                "s" => {}
                "millisecond" | "msec" | "ms" => {
                    result.microseconds += amount.saturating_mul(1_000)
                }
                "microsecond" | "usec" | "µs" | "µsec" => result.microseconds += amount,
                _ => return None,
            }
            matched = true;
            index += 3;
            continue;
        }

        let Some(mut amount) = parse_relative_number(token) else {
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
            "hour" | "hr" | "h" => result.hours += amount,
            "minute" | "min" => result.minutes += amount,
            "second" | "sec" => result.seconds += amount,
            "s" => {}
            "millisecond" | "msec" | "ms" => result.microseconds += amount.saturating_mul(1_000),
            "microsecond" | "usec" | "µs" | "µsec" => result.microseconds += amount,
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
    if relative.microseconds != 0 {
        let total = i64::from(state.microsecond).saturating_add(relative.microseconds);
        state.timestamp = state.timestamp.saturating_add(total.div_euclid(1_000_000));
        state.microsecond = total.rem_euclid(1_000_000) as u32;
    }
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

type ParsedClock = (i64, i64, i64, u32);

fn extract_clock_expression(input: &str) -> (String, Option<ParsedClock>) {
    let tokens = input.split_whitespace().collect::<Vec<_>>();
    if tokens.len() == 3
        && tokens[0].eq_ignore_ascii_case("back")
        && tokens[1].eq_ignore_ascii_case("of")
        && let Some(parsed) = parse_clock(tokens[2])
        && parsed.4 == tokens[2].len()
    {
        return (String::new(), Some((parsed.0, 15, parsed.2, parsed.3)));
    }
    let mut remainder = Vec::with_capacity(tokens.len());
    let mut clock = None;
    for (index, token) in tokens.iter().copied().enumerate() {
        let cleaned = token.trim_matches(',');
        let followed_by_relative_unit = tokens.get(index + 1).is_some_and(|value| {
            matches!(
                unit_name(value).to_ascii_lowercase().as_str(),
                "year"
                    | "month"
                    | "fortnight"
                    | "week"
                    | "day"
                    | "weekday"
                    | "hour"
                    | "hr"
                    | "h"
                    | "minute"
                    | "min"
                    | "second"
                    | "sec"
                    | "s"
                    | "millisecond"
                    | "msec"
                    | "ms"
                    | "microsecond"
                    | "usec"
                    | "µs"
                    | "µsec"
            )
        });
        if cleaned.eq_ignore_ascii_case("midnight") {
            clock = Some((0, 0, 0, 0));
        } else if cleaned.eq_ignore_ascii_case("noon") {
            clock = Some((12, 0, 0, 0));
        } else if !followed_by_relative_unit
            && let Some(parsed) = parse_clock(cleaned)
            && parsed.4 == cleaned.len()
        {
            clock = Some((parsed.0, parsed.1, parsed.2, parsed.3));
        } else {
            remainder.push(token);
        }
    }
    (remainder.join(" "), clock)
}

fn apply_relative_expression(
    mut state: datetime::DateTimeState,
    relative: &RelativeAdjustment,
    source: &str,
    clock: Option<ParsedClock>,
) -> datetime::DateTimeState {
    let lower = source.to_ascii_lowercase();
    if relative.weekday.is_some()
        || lower.contains("today")
        || lower.contains("tomorrow")
        || lower.contains("yesterday")
        || lower.contains("midnight")
        || lower.contains("noon")
    {
        let (year, month, day, _, _, _) = datetime::local_parts(&state);
        datetime::set_local(&mut state, year, month, day, 0, 0, 0);
        state.microsecond = 0;
    }
    apply_relative(&mut state, relative);
    if let Some((hour, minute, second, microsecond)) = clock {
        let (year, month, day, _, _, _) = datetime::local_parts(&state);
        datetime::set_local(&mut state, year, month, day, hour, minute, second);
        state.microsecond = microsecond;
    }
    state
}

pub(super) fn parse_datetime(
    input: &str,
    supplied_timezone: Option<timezone::TimezoneDescription>,
    base: Option<&datetime::DateTimeState>,
    eg: &ExecutorGlobals,
) -> Result<ParsedDateTime, DateParseDiagnostics> {
    let comment_stripped;
    let input = if input.trim_end().ends_with(')')
        && let Some(open) = input.rfind(" (")
        && timezone::parse_timezone(input[open + 2..input.trim_end().len() - 1].trim()).is_some()
    {
        comment_stripped = input[..open].trim_end().to_string();
        comment_stripped.as_str()
    } else {
        input
    };
    let has_explicit_base = base.is_some();
    let fallback = supplied_timezone.unwrap_or_else(|| timezone::default_description(eg));
    // The low-level absolute parser intentionally accepts a valid prefix.
    // Do not let that prefix consume an otherwise valid relative suffix (or
    // relative-only keyword such as `noon`) before the timelib-style grammar
    // below gets a chance to apply it.
    let relative_probe = expanded_named_tokens(input);
    let has_relative_expression = matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "noon" | "midnight"
    ) || parse_relative(&relative_probe).is_some()
        || relative_probe
            .char_indices()
            .filter_map(|(index, character)| character.is_ascii_whitespace().then_some(index))
            .any(|index| {
                let suffix = relative_probe[index..].trim();
                parse_relative(suffix).is_some() || {
                    let (relative_source, _) = extract_clock_expression(suffix);
                    parse_relative(&relative_source).is_some()
                }
            });
    if !has_relative_expression
        && let Some(state) = datetime::parse_absolute(input, Some(fallback.clone()), eg)
    {
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
    let mut default_named_base;
    let named_base = if has_explicit_base {
        base
    } else {
        default_named_base = base.clone();
        let (year, month, day, _, _, _) = datetime::local_parts(&default_named_base);
        datetime::set_local(&mut default_named_base, year, month, day, 0, 0, 0);
        default_named_base.microsecond = 0;
        &default_named_base
    };
    if !has_relative_expression && let Some(state) = parse_numeric_absolute(input, fallback.clone())
    {
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: None,
            fields: infer_fields(input),
        });
    }
    if !has_relative_expression
        && let Some(state) = parse_named_absolute(input, fallback, named_base)
    {
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: None,
            fields: infer_fields(input),
        });
    }
    let sign_normalized_expression = input
        .replace("--", "+")
        .replace("++", "+")
        .replace("+-", "-")
        .replace("-+", "-");
    let expanded_expression = expanded_named_tokens(&sign_normalized_expression);
    let expression = expanded_expression.as_str();
    let split_positions = expression
        .char_indices()
        .filter_map(|(index, character)| character.is_ascii_whitespace().then_some(index))
        .collect::<Vec<_>>();
    for split in split_positions.into_iter().rev() {
        let prefix = expression[..split].trim();
        let suffix = expression[split..].trim();
        // A sign separated from its relative amount belongs to the suffix.
        // Letting the named-date parser consume a dangling sign turns a
        // negative adjustment into the corresponding positive one.
        if prefix.ends_with(['+', '-']) {
            continue;
        }
        let (relative_source, clock) = extract_clock_expression(suffix);
        let prefix_state = datetime::parse_absolute(prefix, Some(base.timezone.clone()), eg)
            .or_else(|| parse_numeric_absolute(prefix, base.timezone.clone()))
            .or_else(|| parse_named_absolute(prefix, base.timezone.clone(), named_base));
        let Some(prefix_state) = prefix_state else {
            continue;
        };
        if relative_source.is_empty()
            && let Some((hour, minute, second, microsecond)) = clock
        {
            let (year, month, day, _, _, _) = datetime::local_parts(&prefix_state);
            return Ok(ParsedDateTime {
                state: civil_state(
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                    microsecond,
                    prefix_state.timezone,
                ),
                diagnostics: DateParseDiagnostics::default(),
                relative: None,
                fields: infer_fields(input),
            });
        }
        let Some(relative) = parse_relative(&relative_source) else {
            continue;
        };
        let state = apply_relative_expression(prefix_state, &relative, suffix, clock);
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: Some(relative),
            fields: infer_fields(prefix),
        });
    }
    let (relative_source, clock) = extract_clock_expression(expression);
    if !relative_source.is_empty()
        && let Some((hour, minute, second, microsecond)) = clock
        && let Some(mut state) =
            datetime::parse_absolute(&relative_source, Some(base.timezone.clone()), eg)
                .or_else(|| parse_numeric_absolute(&relative_source, base.timezone.clone()))
                .or_else(|| {
                    parse_named_absolute(&relative_source, base.timezone.clone(), named_base)
                })
    {
        let (year, month, day, _, _, _) = datetime::local_parts(&state);
        datetime::set_local(&mut state, year, month, day, hour, minute, second);
        state.microsecond = microsecond;
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: None,
            fields: infer_fields(input),
        });
    }
    if let Some(relative) = parse_relative(&relative_source) {
        let state = apply_relative_expression(base.clone(), &relative, input, clock);
        return Ok(ParsedDateTime {
            state,
            diagnostics: DateParseDiagnostics::default(),
            relative: Some(relative),
            fields: ParsedFields::default(),
        });
    }
    if relative_source.is_empty()
        && let Some((hour, minute, second, microsecond)) = clock
    {
        let (year, month, day, _, _, _) = datetime::local_parts(base);
        return Ok(ParsedDateTime {
            state: civil_state(
                year,
                month,
                day,
                hour,
                minute,
                second,
                microsecond,
                base.timezone.clone(),
            ),
            diagnostics: DateParseDiagnostics::default(),
            relative: None,
            fields: ParsedFields {
                hour: true,
                minute: true,
                second: true,
                fraction: microsecond != 0,
                ..ParsedFields::default()
            },
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
    let unexpected = input
        .chars()
        .next()
        .is_some_and(|character| !character.is_ascii() || character.is_ascii_control());
    Err(DateParseDiagnostics {
        warnings: Vec::new(),
        errors: vec![(
            error_position,
            if unexpected {
                "Unexpected character".to_string()
            } else {
                "The timezone could not be found in the database".to_string()
            },
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
    let has_clock = (bytes.len() == 14 && bytes.iter().all(u8::is_ascii_digit))
        || clock_position.is_some()
        || trimmed
            .split_whitespace()
            .any(|part| parse_clock(part).is_some_and(|parsed| parsed.4 == part.len()))
        || trimmed
            .split_whitespace()
            .any(|part| matches!(part.to_ascii_lowercase().as_str(), "noon" | "midnight"));
    let fraction = clock_position.is_some_and(|position| {
        trimmed[position..].find('.').is_some_and(|offset| {
            trimmed[position + offset + 1..].starts_with(|c: char| c.is_ascii_digit())
        })
    });
    let timezone = trimmed.starts_with('@')
        || trimmed.ends_with('Z')
        || timezone::parse_timezone(trimmed).is_some()
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
    let mut allow_trailing = false;
    let mut unix_timestamp = None;
    let mut parsed_year = false;
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
                fields = ParsedFields {
                    year: true,
                    month: true,
                    day: true,
                    hour: true,
                    minute: true,
                    second: true,
                    fraction: true,
                    timezone: false,
                };
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
                    if byte.is_ascii_digit() {
                        break;
                    }
                    input_position += 1;
                }
            }
            '+' => {
                allow_trailing = true;
            }
            ' ' => {
                let start = input_position;
                while input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    input_position += 1;
                }
                if input_position == start {
                    diagnostics.errors.push((
                        input_position,
                        "Not enough data available to satisfy format".to_string(),
                    ));
                }
            }
            'Y' => {
                fields.year = true;
                parsed_year = true;
                year = read_digits(input, &mut input_position, 4, 4).unwrap_or(year)
            }
            'y' => {
                fields.year = true;
                parsed_year = true;
                year = normalize_two_digit_year(
                    read_digits(input, &mut input_position, 2, 2).unwrap_or(year),
                )
            }
            'X' | 'x' => {
                fields.year = true;
                parsed_year = true;
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
                let value = read_digits(input, &mut input_position, 1, 2);
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
            'D' | 'l' => {
                let start = input_position;
                while input
                    .as_bytes()
                    .get(input_position)
                    .is_some_and(u8::is_ascii_alphabetic)
                {
                    input_position += 1;
                }
                if input_position == start {
                    diagnostics
                        .errors
                        .push((start, "A textual day could not be found".to_string()));
                }
            }
            'S' => {
                let suffix = input
                    .get(input_position..input_position.saturating_add(2))
                    .unwrap_or("");
                if matches!(
                    suffix.to_ascii_lowercase().as_str(),
                    "st" | "nd" | "rd" | "th"
                ) {
                    input_position += 2;
                } else {
                    diagnostics.errors.push((
                        input_position,
                        "The separation symbol could not be found".to_string(),
                    ));
                }
            }
            'z' => {
                let start = input_position;
                let ordinal = read_digits(input, &mut input_position, 1, 3);
                if !parsed_year {
                    diagnostics.errors.push((
                        start,
                        "A 'day of year' can only come after a year has been found".to_string(),
                    ));
                    continue;
                }
                let Some(ordinal) = ordinal else {
                    diagnostics
                        .errors
                        .push((start, "A number could not be found".to_string()));
                    continue;
                };
                let timestamp = super::normalized_timestamp(year, 1, ordinal + 1, 0, 0, 0);
                let (parsed_year, parsed_month, parsed_day, ..) =
                    super::super::unix_to_parts(timestamp);
                year = parsed_year;
                month = parsed_month;
                day = parsed_day;
                fields.month = true;
                fields.day = true;
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
                    unix_timestamp = Some(timestamp);
                    timezone = timezone::TimezoneDescription {
                        kind: 1,
                        name: "+00:00".to_string(),
                    };
                    parsed_year = true;
                    fields.year = true;
                    fields.month = true;
                    fields.day = true;
                    fields.hour = true;
                    fields.minute = true;
                    fields.second = true;
                }
            }
            'O' | 'P' | 'p' | 'T' | 'e' => {
                fields.timezone = true;
                let start = input_position;
                match token {
                    'P' | 'p' if input.as_bytes().get(input_position) == Some(&b'Z') => {
                        input_position += 1;
                    }
                    'O' | 'P' | 'p' => {
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
                            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b':')
                        {
                            input_position += 1;
                        }
                    }
                    _ => {
                        while input_position < input.len()
                            && !input.as_bytes()[input_position].is_ascii_whitespace()
                            && !(token == 'e' && input.as_bytes()[input_position] == b']')
                        {
                            input_position += 1;
                        }
                    }
                }
                let zone = &input[start..input_position];
                let zone = if token == 'p' && zone == "Z" {
                    "+00:00"
                } else {
                    zone
                };
                if let Some(parsed) = parse_datetime_timezone(zone) {
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
    if input_position < input.len() {
        if allow_trailing {
            diagnostics
                .warnings
                .push((input_position, "Trailing data".to_string()));
        } else {
            diagnostics
                .errors
                .push((input_position, "Trailing data".to_string()));
        }
    }
    if reset_unparsed && !reset {
        // PHP's `|` resets every field not explicitly parsed.  The common
        // date formats exercise all date fields; zeroing time is the useful
        // observable distinction and avoids a token bitmap in the hot path.
        hour = 0;
        minute = 0;
        second = 0;
        microsecond = 0;
        fields = ParsedFields {
            year: true,
            month: true,
            day: true,
            hour: true,
            minute: true,
            second: true,
            fraction: true,
            timezone: fields.timezone,
        };
    }
    if diagnostics.errors.is_empty()
        && (!(1..=12).contains(&month)
            || day < 1
            || day
                > super::super::days_in_month(
                    year + (month - 1).div_euclid(12),
                    (month - 1).rem_euclid(12) + 1,
                )
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=60).contains(&second))
    {
        diagnostics
            .warnings
            .push((input_position, "The parsed date was invalid".to_string()));
    }
    let state = if let Some(timestamp) = unix_timestamp {
        datetime::DateTimeState {
            timestamp,
            microsecond,
            timezone,
            initialized: true,
        }
    } else {
        civil_state(
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            timezone,
        )
    };
    Ok(ParsedDateTime {
        state,
        diagnostics,
        relative: None,
        fields,
    })
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

    #[test]
    fn explicit_bases_survive_clock_and_relative_suffix_parsing() {
        let eg = ExecutorGlobals::new();
        let base = datetime::DateTimeState {
            timestamp: super::super::normalized_timestamp(2016, 10, 3, 12, 47, 18),
            microsecond: 81_921,
            timezone: timezone::TimezoneDescription {
                kind: 2,
                name: "Z".to_string(),
            },
            initialized: true,
        };
        let noon = parse_datetime("noon", None, Some(&base), &eg).unwrap();
        assert_eq!(datetime::local_parts(&noon.state), (2016, 10, 3, 12, 0, 0));
        assert_eq!(noon.state.microsecond, 0);

        let adjusted =
            parse_datetime("2006-05-08 13:06:44 -0400 +30 days", None, Some(&base), &eg).unwrap();
        assert_eq!(
            datetime::local_parts(&adjusted.state),
            (2006, 6, 7, 13, 6, 44)
        );
        assert_eq!(adjusted.state.timestamp, 1_149_700_004);
    }
}
