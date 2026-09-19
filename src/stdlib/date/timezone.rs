//! DateTimeZone and procedural timezone introspection backed exclusively by
//! RPHP's generated IANA database.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const AFRICA: i64 = 1;
const AMERICA: i64 = 2;
const ANTARCTICA: i64 = 4;
const ARCTIC: i64 = 8;
const ASIA: i64 = 16;
const ATLANTIC: i64 = 32;
const AUSTRALIA: i64 = 64;
const EUROPE: i64 = 128;
const INDIAN: i64 = 256;
const PACIFIC: i64 = 512;
const UTC: i64 = 1024;
const ALL: i64 = 0x07ff;
const ALL_WITH_BC: i64 = 0x0fff;
const PER_COUNTRY: i64 = 0x1000;

const ZONE_TAB: &str = include_str!("../../../data/tzdata/2026a/zone.tab");

#[derive(Clone, Debug)]
struct Location {
    country_code: String,
    latitude: f64,
    longitude: f64,
    comments: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AbbreviationRecord {
    dst: bool,
    offset: i32,
    timezone_id: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub(super) struct TimezoneDescription {
    pub(super) kind: i64,
    pub(super) name: String,
}

fn locations() -> &'static BTreeMap<&'static str, Location> {
    static LOCATIONS: OnceLock<BTreeMap<&'static str, Location>> = OnceLock::new();
    LOCATIONS.get_or_init(|| {
        let mut result = BTreeMap::new();
        for line in ZONE_TAB.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let Some(country_code) = fields.next() else {
                continue;
            };
            let Some(coordinates) = fields.next() else {
                continue;
            };
            let Some(identifier) = fields.next() else {
                continue;
            };
            let comments = fields.next().unwrap_or_default();
            let Some((latitude, longitude)) = parse_coordinates(coordinates) else {
                continue;
            };
            result.insert(
                identifier,
                Location {
                    country_code: country_code.to_string(),
                    latitude,
                    longitude,
                    comments: comments.to_string(),
                },
            );
        }
        result
    })
}

fn parse_coordinates(value: &str) -> Option<(f64, f64)> {
    let split = value
        .char_indices()
        .skip(1)
        .find(|(_, byte)| matches!(byte, '+' | '-'))?
        .0;
    Some((
        parse_coordinate(&value[..split], 2)?,
        parse_coordinate(&value[split..], 3)?,
    ))
}

fn parse_coordinate(value: &str, degree_digits: usize) -> Option<f64> {
    let sign = match value.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let digits = value.get(1..)?;
    if digits.len() != degree_digits + 2 && digits.len() != degree_digits + 4 {
        return None;
    }
    let degrees = digits.get(..degree_digits)?.parse::<f64>().ok()?;
    let minutes = digits
        .get(degree_digits..degree_digits + 2)?
        .parse::<f64>()
        .ok()?;
    let seconds = digits
        .get(degree_digits + 2..)
        .filter(|value| !value.is_empty())
        .map_or(Some(0.0), |value| value.parse::<f64>().ok())?;
    let coordinate = sign * (degrees + minutes / 60.0 + seconds / 3_600.0);
    Some((coordinate * 100_000.0).round() / 100_000.0)
}

fn fixed_offset(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let (sign, digits): (u8, Vec<u8>) = match bytes {
        [sign @ (b'+' | b'-'), h, b':', m1, m2]
            if [h, m1, m2].iter().all(|byte| byte.is_ascii_digit()) =>
        {
            (*sign, vec![b'0', *h, *m1, *m2])
        }
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(|byte| byte.is_ascii_digit()) =>
        {
            (*sign, vec![*h1, *h2, *m1, *m2])
        }
        [sign @ (b'+' | b'-'), h1, h2, m1, m2]
            if [h1, h2, m1, m2].iter().all(|byte| byte.is_ascii_digit()) =>
        {
            (*sign, vec![*h1, *h2, *m1, *m2])
        }
        _ => return None,
    };
    let hour = u16::from(digits[0] - b'0') * 10 + u16::from(digits[1] - b'0');
    let minute = u16::from(digits[2] - b'0') * 10 + u16::from(digits[3] - b'0');
    (hour <= 99 && minute <= 59).then(|| {
        format!(
            "{}{}{}:{}{}",
            char::from(sign),
            char::from(digits[0]),
            char::from(digits[1]),
            char::from(digits[2]),
            char::from(digits[3])
        )
    })
}

fn abbreviations() -> &'static BTreeMap<String, Vec<AbbreviationRecord>> {
    static ABBREVIATIONS: OnceLock<BTreeMap<String, Vec<AbbreviationRecord>>> = OnceLock::new();
    ABBREVIATIONS.get_or_init(|| {
        let excluded = ["admt", "east", "hkwt", "lmt", "pmmt", "set", "zmt"];
        let mut result: BTreeMap<String, BTreeSet<AbbreviationRecord>> = BTreeMap::new();
        for identifier in tzdb::identifiers() {
            let Some(states) = tzdb::states(identifier) else {
                continue;
            };
            for state in states {
                let key = state.abbreviation.to_ascii_lowercase();
                if key.is_empty()
                    || !key.bytes().all(|byte| byte.is_ascii_alphabetic())
                    || excluded.contains(&key.as_str())
                {
                    continue;
                }
                let records = result.entry(key).or_default();
                records.retain(|record| record.timezone_id != Some(identifier));
                records.insert(AbbreviationRecord {
                    dst: state.is_dst,
                    offset: state.offset,
                    timezone_id: Some(identifier),
                });
            }
        }

        // These legacy abbreviations are part of PHP's public table even
        // though modern IANA zones no longer emit them as transition states.
        for (key, offset, dst) in [
            ("cddt", -14_400, true),
            ("eddt", -10_800, true),
            ("mddt", -18_000, true),
            ("pddt", -21_600, true),
            ("uct", 0, false),
        ] {
            result
                .entry(key.to_string())
                .or_default()
                .insert(AbbreviationRecord {
                    dst,
                    offset,
                    timezone_id: None,
                });
        }
        for (letter, offset) in [
            ('a', 3_600),
            ('b', 7_200),
            ('c', 10_800),
            ('d', 14_400),
            ('e', 18_000),
            ('f', 21_600),
            ('g', 25_200),
            ('h', 28_800),
            ('i', 32_400),
            ('k', 36_000),
            ('l', 39_600),
            ('m', 43_200),
            ('n', -3_600),
            ('o', -7_200),
            ('p', -10_800),
            ('q', -14_400),
            ('r', -18_000),
            ('s', -21_600),
            ('t', -25_200),
            ('u', -28_800),
            ('v', -32_400),
            ('w', -36_000),
            ('x', -39_600),
            ('y', -43_200),
            ('z', 0),
        ] {
            result
                .entry(letter.to_string())
                .or_default()
                .insert(AbbreviationRecord {
                    dst: false,
                    offset,
                    timezone_id: None,
                });
        }
        result
            .into_iter()
            .map(|(key, values)| (key, values.into_iter().collect()))
            .collect()
    })
}

pub(super) fn parse_timezone(value: &str) -> Option<TimezoneDescription> {
    if let Some(name) = fixed_offset(value) {
        return Some(TimezoneDescription { kind: 1, name });
    }
    if matches!(value, "UTC" | "Etc/UTC") {
        return Some(TimezoneDescription {
            kind: 3,
            name: value.to_string(),
        });
    }
    if !value.contains('/') && abbreviations().contains_key(&value.to_ascii_lowercase()) {
        return Some(TimezoneDescription {
            kind: 2,
            name: value.to_string(),
        });
    }
    tzdb::contains(value).then(|| TimezoneDescription {
        kind: 3,
        name: value.to_string(),
    })
}

pub(super) fn timezone_object(eg: &ExecutorGlobals, description: TimezoneDescription) -> Value {
    let class = eg
        .find_class("DateTimeZone")
        .expect("DateTimeZone is registered before request execution");
    let mut object = PhpObject::with_layout(
        class.class_id,
        class.property_layout.clone(),
        class.property_defaults.to_vec(),
    );
    object.set_property("timezone_type", Value::long(description.kind));
    object.set_property("timezone", Value::string(description.name));
    Value::object(object)
}

pub(super) fn object_description(value: &Value) -> Option<TimezoneDescription> {
    let object = value.as_object()?;
    let kind = object.get_property("timezone_type")?.as_long()?;
    let name = object.get_property("timezone")?.as_str()?.to_string();
    Some(TimezoneDescription { kind, name })
}

pub(super) fn checked_object_description(
    value: &Value,
    eg: &mut ExecutorGlobals,
) -> Option<TimezoneDescription> {
    if let Some(description) = object_description(value) {
        return Some(description);
    }
    let class = value
        .as_object()
        .map(|object| object.class_name.to_string())
        .unwrap_or_else(|| "DateTimeZone".to_string());
    let inheritance = (!class.eq_ignore_ascii_case("DateTimeZone"))
        .then_some(" (inheriting DateTimeZone)")
        .unwrap_or_default();
    eg.exception = Some(crate::value::make_error_value(
        "DateObjectError",
        &format!(
            "Object of type {class}{inheritance} has not been correctly initialized by calling parent::__construct() in its constructor"
        ),
    ));
    None
}

pub(crate) fn debug_projection(value: &Value, eg: &ExecutorGlobals) -> Option<Value> {
    let object = value.as_object()?;
    let description = object_description(value)?;
    let mut result = PhpArray::new();
    let mut hierarchy = Vec::new();
    let mut class = eg.class_by_id(object.class_id);
    while let Some(definition) = class {
        hierarchy.push(definition.name.clone());
        class = definition
            .parent
            .as_deref()
            .and_then(|parent| eg.find_class(parent));
    }
    hierarchy.reverse();
    let mut declared = std::collections::HashSet::new();
    for class_name in hierarchy {
        let Some(definition) = eg.find_class(&class_name) else {
            continue;
        };
        for property in &definition.properties {
            if !property.declaring_class.eq_ignore_ascii_case(&class_name)
                || matches!(property.name.as_str(), "timezone_type" | "timezone")
                || !declared.insert(property.name.clone())
            {
                continue;
            }
            if let Some(value) = object
                .get_property(&property.name)
                .filter(|value| !value.is_undef())
            {
                result.set_str(&property.name, value.clone_for_php_storage());
            }
        }
    }
    object.for_each_dynamic_property(|name, property| {
        if name != "timezone_type"
            && name != "timezone"
            && !declared.contains(name)
            && !property.is_undef()
        {
            result.set_str(name, property.clone_for_php_storage());
        }
    });
    result.set_str("timezone_type", Value::long(description.kind));
    result.set_str("timezone", Value::string(description.name));
    Some(Value::array(result))
}

pub(crate) fn comparison(left: &Value, right: &Value, eg: &mut ExecutorGlobals) -> Option<i32> {
    let left_class = left.as_object()?.class_name.to_string();
    let right_class = right.as_object()?.class_name.to_string();
    if !eg.class_is_a(&left_class, "DateTimeZone") || !eg.class_is_a(&right_class, "DateTimeZone") {
        return None;
    }
    let (Some(left), Some(right)) = (object_description(left), object_description(right)) else {
        eg.exception = Some(crate::value::make_error_value(
            "DateObjectError",
            "Trying to compare uninitialized DateTimeZone objects",
        ));
        return Some(1);
    };
    if left.kind != right.kind {
        eg.exception = Some(crate::value::make_error_value(
            "DateException",
            "Cannot compare two different kinds of DateTimeZone objects",
        ));
        return Some(1);
    }
    Some(i32::from(left.name != right.name))
}

fn fixed_offset_seconds(value: &str) -> Option<i64> {
    let normalized = fixed_offset(value)?;
    let sign = if normalized.as_bytes()[0] == b'-' {
        -1
    } else {
        1
    };
    let hour = normalized.get(1..3)?.parse::<i64>().ok()?;
    let minute = normalized.get(4..6)?.parse::<i64>().ok()?;
    Some(sign * (hour * 3_600 + minute * 60))
}

pub(super) fn default_description(eg: &ExecutorGlobals) -> TimezoneDescription {
    parse_timezone(super::timezone_id(eg)).unwrap_or(TimezoneDescription {
        kind: 3,
        name: "UTC".to_string(),
    })
}

pub(super) fn description_state(
    description: &TimezoneDescription,
    timestamp: i64,
) -> (String, i64, bool) {
    match description.kind {
        1 => {
            let offset = fixed_offset_seconds(&description.name).unwrap_or_default();
            (
                format!("GMT{}", super::format_timezone_offset(offset, false)),
                offset,
                false,
            )
        }
        2 => {
            let record = abbreviations()
                .get(&description.name.to_ascii_lowercase())
                .and_then(|records| records.first());
            (
                description.name.clone(),
                record.map_or(0, |record| i64::from(record.offset)),
                record.is_some_and(|record| record.dst),
            )
        }
        _ if matches!(description.name.as_str(), "UTC" | "Etc/UTC") => {
            ("UTC".to_string(), 0, false)
        }
        _ if matches!(description.name.as_str(), "GMT" | "Etc/GMT") => {
            ("GMT".to_string(), 0, false)
        }
        _ => tzdb::state_at(&description.name, timestamp).map_or_else(
            || ("UTC".to_string(), 0, false),
            |state| {
                (
                    state.abbreviation.to_string(),
                    i64::from(state.offset),
                    state.is_dst,
                )
            },
        ),
    }
}

pub(super) fn description_local_to_utc(description: &TimezoneDescription, local: i64) -> i64 {
    if description.kind != 3 {
        return local.saturating_sub(description_state(description, local).1);
    }
    if matches!(
        description.name.as_str(),
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT"
    ) {
        return local;
    }
    let mut offsets = [0_i64; 3];
    let mut count = 0;
    for probe in [
        local.saturating_sub(86_400),
        local,
        local.saturating_add(86_400),
    ] {
        let Some(state) = tzdb::state_at(&description.name, probe) else {
            return local;
        };
        let offset = i64::from(state.offset);
        if !offsets[..count].contains(&offset) {
            offsets[count] = offset;
            count += 1;
        }
    }

    let mut valid = [0_i64; 3];
    let mut valid_count = 0;
    let mut gap_candidate = i64::MIN;
    for offset in offsets[..count].iter().copied() {
        let candidate = local.saturating_sub(offset);
        gap_candidate = gap_candidate.max(candidate);
        if tzdb::state_at(&description.name, candidate)
            .is_some_and(|state| i64::from(state.offset) == offset)
        {
            valid[valid_count] = candidate;
            valid_count += 1;
        }
    }
    if valid_count == 0 {
        gap_candidate
    } else {
        *valid[..valid_count]
            .iter()
            .min()
            .expect("a valid wall-clock representation exists")
    }
}

fn location_array(location: &Location) -> Value {
    let mut result = PhpArray::new();
    result.set_str("country_code", Value::string(&location.country_code));
    result.set_str("latitude", Value::double(location.latitude));
    result.set_str("longitude", Value::double(location.longitude));
    result.set_str("comments", Value::string(&location.comments));
    Value::array(result)
}

fn timezone_location(description: &TimezoneDescription) -> Option<Value> {
    if description.kind != 3 {
        return None;
    }
    if let Some(location) = locations().get(description.name.as_str()) {
        return Some(location_array(location));
    }
    Some(location_array(&Location {
        country_code: "??".to_string(),
        latitude: 0.0,
        longitude: 0.0,
        comments: "?".to_string(),
    }))
}

fn transition_time(timestamp: i64) -> String {
    let timestamp = i128::from(timestamp);
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let hour = seconds / 3_600;
    let minute = seconds % 3_600 / 60;
    let second = seconds % 60;

    // Civil date from days since the Unix epoch.  This is the wide-integer
    // form of Howard Hinnant's civil_from_days algorithm.  PHP's timelib
    // exposes the full signed timestamp range here, including the default
    // PHP_INT_MIN boundary, so the ordinary i64 date projection is too narrow.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    if month <= 2 {
        year += 1;
    }

    let year = if year < 0 {
        format!("-{:04}", -year)
    } else if year > 9_999 {
        format!("+{year}")
    } else {
        format!("{year:04}")
    };
    format!("{year}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}+00:00")
}

fn transition_array(description: &TimezoneDescription, begin: i64, end: i64) -> Option<Value> {
    if description.kind != 3 {
        return None;
    }
    let transitions = tzdb::transitions(&description.name, begin, end)?;
    let mut result = PhpArray::new();
    for transition in transitions {
        let time = transition_time(transition.timestamp);
        let mut entry = PhpArray::new();
        entry.set_str("ts", Value::long(transition.timestamp));
        entry.set_str("time", Value::string(time));
        entry.set_str("offset", Value::long(i64::from(transition.state.offset)));
        entry.set_str("isdst", Value::bool(transition.state.is_dst));
        entry.set_str("abbr", Value::string(transition.state.abbreviation));
        result.push(Value::array(entry));
    }
    Some(Value::array(result))
}

fn area_flag(identifier: &str) -> i64 {
    match identifier.split_once('/').map(|(area, _)| area) {
        Some("Africa") => AFRICA,
        Some("America") => AMERICA,
        Some("Antarctica") => ANTARCTICA,
        Some("Arctic") => ARCTIC,
        Some("Asia") => ASIA,
        Some("Atlantic") => ATLANTIC,
        Some("Australia") => AUSTRALIA,
        Some("Europe") => EUROPE,
        Some("Indian") => INDIAN,
        Some("Pacific") => PACIFIC,
        _ if identifier == "UTC" => UTC,
        _ => 0,
    }
}

fn identifier_array(group: i64, country: Option<&str>) -> Value {
    let mut names = BTreeSet::new();
    if group == ALL_WITH_BC {
        names.extend(tzdb::identifiers());
        names.insert("UTC");
    } else if group == PER_COUNTRY {
        if let Some(country) = country {
            for (&identifier, location) in locations() {
                if location.country_code.eq_ignore_ascii_case(country) {
                    names.insert(identifier);
                }
            }
        }
    } else {
        for &identifier in locations().keys() {
            if area_flag(identifier) & group != 0 {
                names.insert(identifier);
            }
        }
        if group & UTC != 0 {
            names.insert("UTC");
        }
    }
    let mut result = PhpArray::new();
    for name in names {
        result.push(Value::string(name));
    }
    Value::array(result)
}

fn abbreviations_array() -> Value {
    let mut result = PhpArray::new();
    for (abbreviation, records) in abbreviations() {
        let mut values = PhpArray::new();
        for record in records {
            let mut row = PhpArray::new();
            row.set_str("dst", Value::bool(record.dst));
            row.set_str("offset", Value::long(i64::from(record.offset)));
            row.set_str(
                "timezone_id",
                record.timezone_id.map_or_else(Value::null, Value::string),
            );
            values.push(Value::array(row));
        }
        result.set_str(abbreviation, Value::array(values));
    }
    Value::array(result)
}

fn name_from_abbreviation(abbreviation: &str, offset: i64, is_dst: i64) -> Option<&'static str> {
    let preferred = match abbreviation.to_ascii_uppercase().as_str() {
        "UTC" | "GMT" | "UCT" | "Z" => Some("UTC"),
        "CET" | "CEST" => Some("Europe/Berlin"),
        "EDT" | "EST" => Some("America/New_York"),
        "ADT" | "AST" => Some("America/Halifax"),
        "PDT" | "PST" => Some("America/Los_Angeles"),
        "CDT" | "CST" => Some("America/Chicago"),
        "MDT" | "MST" => Some("America/Denver"),
        _ => None,
    };
    if preferred.is_some() {
        return preferred;
    }
    if !abbreviation.is_empty() {
        if let Some(identifier) = abbreviations()
            .get(&abbreviation.to_ascii_lowercase())
            .and_then(|records| records.iter().find_map(|record| record.timezone_id))
        {
            return Some(identifier);
        }
    }
    if let Some(preferred) = match (offset, is_dst) {
        (-14_400, 1) => Some("America/New_York"),
        (-14_400, 0) => Some("America/Halifax"),
        (3_600, 1) => Some("Europe/London"),
        (-7_200, 1) => Some("America/Sao_Paulo"),
        _ => None,
    } {
        return Some(preferred);
    }
    None
}

pub(crate) fn fn_timezone_version_get(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let version = tzdb::version();
    let number = version
        .strip_prefix("2026")
        .and_then(|suffix| suffix.as_bytes().first().copied())
        .map_or(0, |letter| i64::from(letter.saturating_sub(b'a')) + 1);
    ret!(rv, Value::string(format!("2026.{number}")));
}

pub(crate) fn fn_timezone_identifiers_list(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let group = arg_opt!(ed, 0).and_then(Value::as_long).unwrap_or(ALL);
    let country = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(Value::as_str);
    if group == PER_COUNTRY && country.is_none_or(|country| country.len() != 2) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "timezone_identifiers_list(): Argument #2 ($countryCode) must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
        ));
        return Ok(());
    }
    ret!(rv, identifier_array(group, country));
}

pub(crate) fn fn_timezone_abbreviations_list(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(rv, abbreviations_array());
}

pub(crate) fn fn_timezone_name_from_abbr(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(abbreviation) =
        super::typed_internal_string_argument(ed, eg, "timezone_name_from_abbr", 0, "abbr")?
    else {
        return Ok(());
    };
    let offset = arg_opt!(ed, 1).and_then(Value::as_long).unwrap_or(-1);
    let is_dst = arg_opt!(ed, 2).and_then(Value::as_long).unwrap_or(-1);
    match name_from_abbreviation(&abbreviation, offset, is_dst) {
        Some(identifier) => ret!(rv, Value::string(identifier)),
        None => ret!(rv, Value::bool(false)),
    }
}

pub(crate) fn fn_timezone_open(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timezone) =
        super::typed_internal_string_argument(ed, eg, "timezone_open", 0, "timezone")?
    else {
        return Ok(());
    };
    let Some(description) = parse_timezone(&timezone) else {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("timezone_open(): Unknown or bad timezone ({timezone})"),
        )?;
        ret!(rv, Value::bool(false));
    };
    ret!(rv, timezone_object(eg, description));
}

pub(crate) fn fn_timezone_name_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(name) =
        checked_object_description(arg!(ed, 0), eg).map(|description| description.name)
    else {
        return Ok(());
    };
    ret!(rv, Value::string(name));
}

pub(crate) fn fn_timezone_location_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(description) = checked_object_description(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let result = timezone_location(&description);
    ret!(rv, result.unwrap_or_else(|| Value::bool(false)));
}

pub(crate) fn fn_timezone_transitions_get(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let begin = arg_opt!(ed, 1).map(Value::to_long_val).unwrap_or(i64::MIN);
    let end = arg_opt!(ed, 2)
        .map(Value::to_long_val)
        .unwrap_or(i64::from(i32::MAX));
    let Some(description) = checked_object_description(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let result = transition_array(&description, begin, end);
    ret!(rv, result.unwrap_or_else(|| Value::bool(false)));
}

pub(crate) fn fn_date_time_zone_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let timezone = arg_str!(ed, 1);
    let Some(description) = parse_timezone(&timezone) else {
        eg.exception = Some(crate::value::make_error_value(
            "Exception",
            &format!("DateTimeZone::__construct(): Unknown or bad timezone ({timezone})"),
        ));
        return Ok(());
    };
    if let Some(mut object) = arg!(ed, 0).as_object_mut() {
        object.set_property("timezone_type", Value::long(description.kind));
        object.set_property("timezone", Value::string(description.name));
    }
    Ok(())
}

pub(crate) fn fn_date_time_zone_get_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_timezone_name_get(ed, rv, eg)
}

pub(crate) fn fn_date_time_zone_get_location(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_timezone_location_get(ed, rv, eg)
}

pub(crate) fn fn_date_time_zone_get_transitions(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_timezone_transitions_get(ed, rv, eg)
}

pub(crate) fn fn_date_time_zone_list_abbreviations(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_timezone_abbreviations_list(ed, rv, eg)
}

pub(crate) fn fn_date_time_zone_list_identifiers(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // Static internal methods reserve CV 0; explicit parameters start at 1.
    let group = arg_opt!(ed, 1).and_then(Value::as_long).unwrap_or(ALL);
    let country = arg_opt!(ed, 2)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(Value::as_str);
    if group == PER_COUNTRY && country.is_none_or(|country| country.len() != 2) {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "DateTimeZone::listIdentifiers(): Argument #2 ($countryCode) must be a two-letter ISO 3166-1 compatible country code when argument #1 ($timezoneGroup) is DateTimeZone::PER_COUNTRY",
        ));
        return Ok(());
    }
    ret!(rv, identifier_array(group, country));
}

pub(crate) fn fn_date_time_zone_serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(description) = checked_object_description(arg!(ed, 0), eg) else {
        return Ok(());
    };
    let mut result = PhpArray::new();
    result.set_str("timezone_type", Value::long(description.kind));
    result.set_str("timezone", Value::string(description.name));
    if let Some(object) = arg!(ed, 0).as_object() {
        object.for_each_property(|name, property| {
            if name != "timezone_type" && name != "timezone" && !property.is_undef() {
                result.set_str(name, property.clone_for_php_storage());
            }
        });
    }
    ret!(rv, Value::array(result));
}

fn invalid_serialization(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Invalid serialization data for DateTimeZone object",
    ));
}

fn serialized_description(
    timezone_type: Option<&Value>,
    timezone_name: Option<&Value>,
) -> Option<TimezoneDescription> {
    let timezone_type = timezone_type?.dereferenced().as_long()?;
    let timezone_name = timezone_name?.dereferenced().as_str()?;
    if !(1..=3).contains(&timezone_type) {
        return None;
    }
    let mut description = parse_timezone(timezone_name)?;
    description.kind = timezone_type;
    Some(description)
}

fn install_description(receiver: &Value, description: TimezoneDescription) -> bool {
    let Some(mut object) = receiver.as_object_mut() else {
        return false;
    };
    object.set_property("timezone_type", Value::long(description.kind));
    object.set_property("timezone", Value::string(description.name));
    true
}

pub(crate) fn fn_date_time_zone_unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let Some(description) =
        serialized_description(data.get_str("timezone_type"), data.get_str("timezone"))
    else {
        invalid_serialization(eg);
        return Ok(());
    };
    install_description(arg!(ed, 0), description);
    if let Some(mut object) = arg!(ed, 0).as_object_mut() {
        for (key, value) in data.iter() {
            let ArrayKey::String(name) = key else {
                continue;
            };
            if name != "timezone_type" && name != "timezone" {
                object.set_property(&name, value.clone_for_php_storage());
            }
        }
    }
    Ok(())
}

pub(crate) fn fn_date_time_zone_wakeup(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = arg!(ed, 0).clone();
    let Some(object) = receiver.as_object() else {
        return Ok(());
    };
    let timezone_type = object.get_property("timezone_type").cloned();
    let timezone_name = object.get_property("timezone").cloned();
    drop(object);
    let Some(description) = serialized_description(timezone_type.as_ref(), timezone_name.as_ref())
    else {
        invalid_serialization(eg);
        return Ok(());
    };
    install_description(&receiver, description);
    Ok(())
}

pub(crate) fn fn_date_time_zone_set_state(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 1).dereferenced().as_array() else {
        return Ok(());
    };
    let Some(description) =
        serialized_description(data.get_str("timezone_type"), data.get_str("timezone"))
    else {
        invalid_serialization(eg);
        return Ok(());
    };
    ret!(rv, timezone_object(eg, description));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_tab_and_offsets_are_decoded_without_host_timezone_data() {
        let prague = locations().get("Europe/Prague").unwrap();
        assert_eq!(prague.country_code, "CZ");
        assert!((prague.latitude - 50.08333).abs() < 0.00001);
        assert_eq!(fixed_offset("-0430").as_deref(), Some("-04:30"));
        assert!(fixed_offset("+99:60").is_none());
    }

    #[test]
    fn php_abbreviation_key_inventory_is_derived_from_iana_and_legacy_rows() {
        assert_eq!(abbreviations().len(), 144);
        assert_eq!(
            abbreviations()["acst"]
                .iter()
                .filter_map(|record| record.timezone_id)
                .collect::<Vec<_>>(),
            vec![
                "Australia/Adelaide",
                "Australia/Broken_Hill",
                "Australia/Darwin",
                "Australia/North",
                "Australia/South",
                "Australia/Yancowinna",
            ]
        );
    }

    #[test]
    fn transition_time_matches_timelib_across_the_signed_timestamp_range() {
        assert_eq!(
            transition_time(i64::MIN),
            "-292277022657-01-27T08:29:52+00:00"
        );
        assert_eq!(
            transition_time(-62_198_755_200),
            "-0001-01-01T00:00:00+00:00"
        );
        assert_eq!(
            transition_time(-62_167_219_200),
            "0000-01-01T00:00:00+00:00"
        );
        assert_eq!(transition_time(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(
            transition_time(253_402_300_800),
            "+10000-01-01T00:00:00+00:00"
        );
        assert_eq!(
            transition_time(i64::MAX),
            "+292277026596-12-04T15:30:07+00:00"
        );
    }
}
