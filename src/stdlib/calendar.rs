//! PHP 8.5 Calendar extension over one Julian-day conversion boundary.
//!
//! The arithmetic is independent of the parser, VM, and date/time subsystem.
//! Integer Julian days make the four admitted civil calendars share weekday,
//! month-name, information, and Unix projection code without request state.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::compiler::make_internal_function;
use crate::runtime::ExecutorGlobals;
use crate::value::{ArrayKey, PhpArray, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

const CAL_GREGORIAN: i64 = 0;
const CAL_JULIAN: i64 = 1;
const CAL_JEWISH: i64 = 2;
const CAL_FRENCH: i64 = 3;
const CAL_NUM_CALS: i64 = 4;

const CAL_DOW_DAYNO: i64 = 0;
const CAL_DOW_LONG: i64 = 1;
const CAL_DOW_SHORT: i64 = 2;

const CAL_MONTH_GREGORIAN_SHORT: i64 = 0;
const CAL_MONTH_GREGORIAN_LONG: i64 = 1;
const CAL_MONTH_JULIAN_SHORT: i64 = 2;
const CAL_MONTH_JULIAN_LONG: i64 = 3;
const CAL_MONTH_JEWISH: i64 = 4;
const CAL_MONTH_FRENCH: i64 = 5;

const CAL_EASTER_DEFAULT: i64 = 0;
const CAL_EASTER_ROMAN: i64 = 1;
const CAL_EASTER_ALWAYS_GREGORIAN: i64 = 2;
const CAL_EASTER_ALWAYS_JULIAN: i64 = 3;

const CAL_JEWISH_ADD_ALAFIM_GERESH: i64 = 2;
const CAL_JEWISH_ADD_ALAFIM: i64 = 4;
const CAL_JEWISH_ADD_GERESHAYIM: i64 = 8;

const UNIX_EPOCH_JD: i64 = 2_440_588;
const MAX_UNIX_JD: i64 = 106_751_993_607_888;
const FRENCH_EPOCH_JD: i64 = 2_375_840;
const FRENCH_LAST_JD: i64 = 2_380_952;
const HEBREW_EPOCH_JD: i64 = 347_998;
const HEBREW_LAST_JD: i64 = 324_542_846;
const HEBREW_LAST_YEAR: i64 = 887_605;
const MAX_EASTER_DAYS_YEAR: i64 = 7_378_697_629_483_820_644;

const MONTHS_LONG: [&str; 12] = [
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
const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_JEWISH: [&str; 13] = [
    "Tishri", "Heshvan", "Kislev", "Tevet", "Shevat", "Adar I", "Adar II", "Nisan", "Iyyar",
    "Sivan", "Tammuz", "Av", "Elul",
];
const MONTHS_JEWISH_COMMON: [&str; 13] = [
    "Tishri", "Heshvan", "Kislev", "Tevet", "Shevat", "", "Adar", "Nisan", "Iyyar", "Sivan",
    "Tammuz", "Av", "Elul",
];
const MONTHS_FRENCH: [&str; 13] = [
    "Vendemiaire",
    "Brumaire",
    "Frimaire",
    "Nivose",
    "Pluviose",
    "Ventose",
    "Germinal",
    "Floreal",
    "Prairial",
    "Messidor",
    "Thermidor",
    "Fructidor",
    "Extra",
];
const WEEKDAYS_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CivilDate {
    month: i64,
    day: i64,
    year: i64,
}

impl CivilDate {
    const INVALID: Self = Self {
        month: 0,
        day: 0,
        year: 0,
    };

    fn display(self) -> String {
        format!("{}/{}/{}", self.month, self.day, self.year)
    }

    fn is_valid(self) -> bool {
        self.month != 0
    }
}

fn write_result(rv: *mut Value, value: Value) {
    super::write_return_value(rv, value);
}

fn value_error(eg: &mut ExecutorGlobals, message: impl AsRef<str>) {
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        message.as_ref(),
    ));
}

fn required_int(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<i64>, VmError> {
    super::typed_internal_int_argument(ed, eg, function, index, parameter)
}

fn optional_nullable_int(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    index: u32,
    parameter: &str,
) -> Result<Option<Option<i64>>, VmError> {
    let argument = super::owned_argument(ed, index);
    if matches!(argument.value_type(), ValueType::Undef | ValueType::Null) {
        return Ok(Some(None));
    }
    Ok(required_int(ed, eg, function, index, parameter)?.map(Some))
}

fn direct_calendar_int(value: i64) -> i64 {
    i64::from(value as i32)
}

fn php_year_to_astronomical(year: i64) -> i64 {
    if year < 0 { year + 1 } else { year }
}

fn astronomical_year_to_php(year: i128) -> Option<i64> {
    let year = if year <= 0 { year - 1 } else { year };
    i64::try_from(year).ok()
}

/// Proleptic Gregorian date to integer Julian day. PHP has no year zero and
/// accepts day overflow only within the ordinary 1..=31 field boundary.
fn gregorian_to_jd(month: i64, day: i64, year: i64) -> i64 {
    if year == 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }
    let astronomical = i128::from(php_year_to_astronomical(year));
    let month = i128::from(month);
    let day = i128::from(day);
    let adjustment = (14 - month) / 12;
    let y = astronomical + 4800 - adjustment;
    let m = month + 12 * adjustment - 3;
    let jd = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32_045;
    i64::try_from(jd).ok().filter(|jd| *jd > 0).unwrap_or(0)
}

fn julian_to_jd(month: i64, day: i64, year: i64) -> i64 {
    if year == 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return 0;
    }
    let astronomical = i128::from(php_year_to_astronomical(year));
    let month = i128::from(month);
    let day = i128::from(day);
    let adjustment = (14 - month) / 12;
    let y = astronomical + 4800 - adjustment;
    let m = month + 12 * adjustment - 3;
    let jd = day + (153 * m + 2) / 5 + 365 * y + y / 4 - 32_083;
    i64::try_from(jd).ok().filter(|jd| *jd > 0).unwrap_or(0)
}

fn gregorian_from_jd(jd: i64) -> CivilDate {
    if jd <= 0 || i128::from(jd) > (i128::from(i64::MAX) - 128_179) / 4 {
        return CivilDate::INVALID;
    }
    let a = i128::from(jd) + 32_044;
    let b = (4 * a + 3) / 146_097;
    let c = a - 146_097 * b / 4;
    let d = (4 * c + 3) / 1_461;
    let e = c - 1_461 * d / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4_800 + m / 10;
    let Some(year) = astronomical_year_to_php(year) else {
        return CivilDate::INVALID;
    };
    let Ok(year) = i32::try_from(year) else {
        return CivilDate::INVALID;
    };
    CivilDate {
        month: month as i64,
        day: day as i64,
        year: i64::from(year),
    }
}

fn julian_from_jd(jd: i64) -> CivilDate {
    if jd <= 0 || i128::from(jd) > (i128::from(i64::MAX) - 128_331) / 4 {
        return CivilDate::INVALID;
    }
    let c = i128::from(jd) + 32_082;
    let d = (4 * c + 3) / 1_461;
    let e = c - 1_461 * d / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = d - 4_800 + m / 10;
    let Some(year) = astronomical_year_to_php(year) else {
        return CivilDate::INVALID;
    };
    let Ok(year) = i32::try_from(year) else {
        return CivilDate::INVALID;
    };
    CivilDate {
        month: month as i64,
        day: day as i64,
        year: i64::from(year),
    }
}

fn french_leap_year(year: i64) -> bool {
    matches!(year, 3 | 7 | 11)
}

fn french_to_jd(month: i64, day: i64, year: i64) -> i64 {
    if !(1..=14).contains(&year) || !(1..=13).contains(&month) || !(1..=30).contains(&day) {
        return 0;
    }
    let completed_years = year - 1;
    let leap_days = (completed_years + 1) / 4;
    FRENCH_EPOCH_JD + completed_years * 365 + leap_days + (month - 1) * 30 + day - 1
}

fn french_from_jd(jd: i64) -> CivilDate {
    if !(FRENCH_EPOCH_JD..=FRENCH_LAST_JD).contains(&jd) {
        return CivilDate::INVALID;
    }
    let mut offset = jd - FRENCH_EPOCH_JD;
    for year in 1..=14 {
        let year_days = 365 + i64::from(french_leap_year(year));
        if offset < year_days {
            return CivilDate {
                month: offset / 30 + 1,
                day: offset % 30 + 1,
                year,
            };
        }
        offset -= year_days;
    }
    CivilDate::INVALID
}

fn hebrew_is_leap_year(year: i64) -> bool {
    (7 * year + 1).rem_euclid(19) < 7
}

fn hebrew_elapsed_days(year: i64) -> i64 {
    let months = (235 * year - 234) / 19;
    let parts = 12_084 + 13_753 * months;
    let mut day = 29 * months + parts / 25_920;
    if (3 * (day + 1)).rem_euclid(7) < 3 {
        day += 1;
    }
    day
}

fn hebrew_new_year_delay(year: i64) -> i64 {
    let previous = hebrew_elapsed_days(year - 1);
    let current = hebrew_elapsed_days(year);
    let next = hebrew_elapsed_days(year + 1);
    if next - current == 356 {
        2
    } else if current - previous == 382 {
        1
    } else {
        0
    }
}

fn hebrew_new_year(year: i64) -> i64 {
    HEBREW_EPOCH_JD + hebrew_elapsed_days(year) + hebrew_new_year_delay(year)
}

fn hebrew_year_days(year: i64) -> i64 {
    hebrew_new_year(year + 1) - hebrew_new_year(year)
}

fn hebrew_days_in_month(month: i64, year: i64) -> Option<i64> {
    if !(1..=HEBREW_LAST_YEAR).contains(&year) {
        return None;
    }
    let year_days = hebrew_year_days(year);
    match month {
        1 => Some(30),
        2 => Some(if year_days.rem_euclid(10) == 5 {
            30
        } else {
            29
        }),
        3 => Some(if year_days.rem_euclid(10) == 3 {
            29
        } else {
            30
        }),
        4 => Some(29),
        5 => Some(30),
        6 => Some(if hebrew_is_leap_year(year) { 30 } else { 0 }),
        7 => Some(29),
        8 => Some(30),
        9 => Some(29),
        10 => Some(30),
        11 => Some(29),
        12 => Some(30),
        13 => Some(29),
        _ => None,
    }
}

fn hebrew_to_jd(month: i64, day: i64, year: i64) -> i64 {
    if !(1..=HEBREW_LAST_YEAR).contains(&year)
        || !(1..=13).contains(&month)
        || !(1..=30).contains(&day)
    {
        return 0;
    }
    let mut jd = hebrew_new_year(year);
    for current in 1..month {
        let Some(days) = hebrew_days_in_month(current, year) else {
            return 0;
        };
        jd += days;
    }
    jd += day - 1;
    if jd > HEBREW_LAST_JD { 0 } else { jd }
}

fn hebrew_from_jd(jd: i64) -> CivilDate {
    if !(HEBREW_EPOCH_JD..=HEBREW_LAST_JD).contains(&jd) {
        return CivilDate::INVALID;
    }
    let mut low = 1;
    let mut high = HEBREW_LAST_YEAR + 1;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if hebrew_new_year(middle) <= jd {
            low = middle;
        } else {
            high = middle;
        }
    }
    let year = low;
    let mut offset = jd - hebrew_new_year(year);
    for month in 1..=13 {
        let days = hebrew_days_in_month(month, year).unwrap_or(0);
        if days == 0 {
            continue;
        }
        if offset < days {
            return CivilDate {
                month,
                day: offset + 1,
                year,
            };
        }
        offset -= days;
    }
    CivilDate::INVALID
}

fn calendar_from_jd(jd: i64, calendar: i64) -> CivilDate {
    match calendar {
        CAL_GREGORIAN => gregorian_from_jd(jd),
        CAL_JULIAN => julian_from_jd(jd),
        CAL_JEWISH => hebrew_from_jd(jd),
        CAL_FRENCH => french_from_jd(jd),
        _ => CivilDate::INVALID,
    }
}

fn calendar_to_jd(calendar: i64, month: i64, day: i64, year: i64) -> i64 {
    let month = direct_calendar_int(month);
    let day = direct_calendar_int(day);
    let year = direct_calendar_int(year);
    match calendar {
        CAL_GREGORIAN => gregorian_to_jd(month, day, year),
        CAL_JULIAN => julian_to_jd(month, day, year),
        CAL_JEWISH => hebrew_to_jd(month, day, year),
        CAL_FRENCH => french_to_jd(month, day, year),
        _ => 0,
    }
}

fn weekday(jd: i64) -> usize {
    ((jd.rem_euclid(7) + 1) % 7) as usize
}

fn month_name(calendar: i64, date: CivilDate, short: bool) -> &'static str {
    let Some(index) = date
        .month
        .checked_sub(1)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return "";
    };
    match calendar {
        CAL_GREGORIAN | CAL_JULIAN => {
            if short {
                MONTHS_SHORT.get(index).copied().unwrap_or("")
            } else {
                MONTHS_LONG.get(index).copied().unwrap_or("")
            }
        }
        CAL_JEWISH => {
            let months = if hebrew_is_leap_year(date.year) {
                &MONTHS_JEWISH
            } else {
                &MONTHS_JEWISH_COMMON
            };
            months.get(index).copied().unwrap_or("")
        }
        CAL_FRENCH => MONTHS_FRENCH.get(index).copied().unwrap_or(""),
        _ => "",
    }
}

fn calendar_info(calendar: i64) -> PhpArray {
    let (months, abbreviations, maximum, name, symbol): (&[&str], &[&str], i64, &str, &str) =
        match calendar {
            CAL_GREGORIAN => (
                &MONTHS_LONG,
                &MONTHS_SHORT,
                31,
                "Gregorian",
                "CAL_GREGORIAN",
            ),
            CAL_JULIAN => (&MONTHS_LONG, &MONTHS_SHORT, 31, "Julian", "CAL_JULIAN"),
            CAL_JEWISH => (&MONTHS_JEWISH, &MONTHS_JEWISH, 30, "Jewish", "CAL_JEWISH"),
            CAL_FRENCH => (&MONTHS_FRENCH, &MONTHS_FRENCH, 30, "French", "CAL_FRENCH"),
            _ => unreachable!("validated calendar id"),
        };
    let mut month_values = PhpArray::with_hash_capacity(months.len());
    let mut abbreviation_values = PhpArray::with_hash_capacity(abbreviations.len());
    for (index, month) in months.iter().enumerate() {
        month_values.set(ArrayKey::Int(index as i64 + 1), Value::string(*month));
    }
    for (index, month) in abbreviations.iter().enumerate() {
        abbreviation_values.set(ArrayKey::Int(index as i64 + 1), Value::string(*month));
    }
    let mut info = PhpArray::with_hash_capacity(5);
    info.set_str("months", Value::array(month_values));
    info.set_str("abbrevmonths", Value::array(abbreviation_values));
    info.set_str("maxdaysinmonth", Value::long(maximum));
    info.set_str("calname", Value::string(name));
    info.set_str("calsymbol", Value::string(symbol));
    info
}

fn cal_from_jd_value(jd: i64, calendar: i64) -> PhpArray {
    let date = calendar_from_jd(jd, calendar);
    let dow = weekday(jd);
    let abbreviation = month_name(calendar, date, calendar <= CAL_JULIAN);
    let full = month_name(calendar, date, false);
    let mut result = PhpArray::with_hash_capacity(9);
    result.set_str("date", Value::string(date.display()));
    result.set_str("month", Value::long(date.month));
    result.set_str("day", Value::long(date.day));
    result.set_str("year", Value::long(date.year));
    if calendar == CAL_JEWISH && !date.is_valid() {
        result.set_str("dow", Value::null());
        result.set_str("abbrevdayname", Value::string(""));
        result.set_str("dayname", Value::string(""));
    } else {
        result.set_str("dow", Value::long(dow as i64));
        result.set_str("abbrevdayname", Value::string(WEEKDAYS_SHORT[dow]));
        result.set_str("dayname", Value::string(WEEKDAYS_LONG[dow]));
    }
    result.set_str("abbrevmonth", Value::string(abbreviation));
    result.set_str("monthname", Value::string(full));
    result
}

fn append_hebrew_number(mut value: i64, flags: i64, output: &mut Vec<u8>) {
    const LETTERS: [u8; 23] = [
        b'0', 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xeb, 0xec, 0xee, 0xf0,
        0xf1, 0xf2, 0xf4, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
    ];
    const VALUES: [(i64, u8); 22] = [
        (400, 0xfa),
        (300, 0xf9),
        (200, 0xf8),
        (100, 0xf7),
        (90, 0xf6),
        (80, 0xf4),
        (70, 0xf2),
        (60, 0xf1),
        (50, 0xf0),
        (40, 0xee),
        (30, 0xec),
        (20, 0xeb),
        (10, 0xe9),
        (9, 0xe8),
        (8, 0xe7),
        (7, 0xe6),
        (6, 0xe5),
        (5, 0xe4),
        (4, 0xe3),
        (3, 0xe2),
        (2, 0xe1),
        (1, 0xe0),
    ];
    if !(1..=9_999).contains(&value) {
        return;
    }
    let thousands = value / 1_000;
    if thousands > 0 {
        output.push(LETTERS[thousands as usize]);
        if flags & CAL_JEWISH_ADD_ALAFIM_GERESH != 0 {
            output.push(b'\'');
        }
        if flags & CAL_JEWISH_ADD_ALAFIM != 0 {
            output.extend_from_slice(b" \xe0\xec\xf4\xe9\xed ");
        }
        value %= 1_000;
    }
    let remainder_start = output.len();
    while value >= 400 {
        output.push(0xfa);
        value -= 400;
    }
    if value == 15 {
        output.extend_from_slice(&[0xe8, 0xe5]);
        value = 0;
    } else if value == 16 {
        output.extend_from_slice(&[0xe8, 0xe6]);
        value = 0;
    }
    for (amount, letter) in VALUES {
        if value >= amount {
            output.push(letter);
            value -= amount;
        }
    }
    if flags & CAL_JEWISH_ADD_GERESHAYIM == 0 || output.len() == remainder_start {
        return;
    }
    if output.len() == remainder_start + 1 {
        output.push(b'\'');
    } else {
        output.insert(output.len() - 1, b'"');
    }
}

fn hebrew_date_bytes(date: CivilDate, flags: i64) -> Vec<u8> {
    const HEBREW_MONTHS_LEAP: [&[u8]; 13] = [
        b"\xfa\xf9\xf8\xe9",
        b"\xe7\xf9\xe5\xef",
        b"\xeb\xf1\xec\xe5",
        b"\xe8\xe1\xfa",
        b"\xf9\xe1\xe8",
        b"\xe0\xe3\xf8 \xe0\x27",
        b"\xe0\xe3\xf8 \xe1\x27",
        b"\xf0\xe9\xf1\xef",
        b"\xe0\xe9\xe9\xf8",
        b"\xf1\xe9\xe5\xef",
        b"\xfa\xee\xe5\xe6",
        b"\xe0\xe1",
        b"\xe0\xec\xe5\xec",
    ];
    const HEBREW_MONTHS_COMMON: [&[u8]; 13] = [
        b"\xfa\xf9\xf8\xe9",
        b"\xe7\xf9\xe5\xef",
        b"\xeb\xf1\xec\xe5",
        b"\xe8\xe1\xfa",
        b"\xf9\xe1\xe8",
        b"",
        b"\xe0\xe3\xf8",
        b"\xf0\xe9\xf1\xef",
        b"\xe0\xe9\xe9\xf8",
        b"\xf1\xe9\xe5\xef",
        b"\xfa\xee\xe5\xe6",
        b"\xe0\xe1",
        b"\xe0\xec\xe5\xec",
    ];
    if !date.is_valid() {
        return b"0/0/0".to_vec();
    }
    let mut result = Vec::with_capacity(48);
    append_hebrew_number(date.day, flags, &mut result);
    result.push(b' ');
    let months = if hebrew_is_leap_year(date.year) {
        &HEBREW_MONTHS_LEAP
    } else {
        &HEBREW_MONTHS_COMMON
    };
    result.extend_from_slice(months[(date.month - 1) as usize]);
    result.push(b' ');
    append_hebrew_number(date.year, flags, &mut result);
    result
}

fn easter_days_for(year: i64, mode: i64) -> i64 {
    let gregorian = match mode {
        CAL_EASTER_ALWAYS_GREGORIAN => true,
        CAL_EASTER_ALWAYS_JULIAN => false,
        CAL_EASTER_ROMAN => year > 1582,
        _ => year > 1752,
    };
    let year = i128::from(year);
    let golden = year.rem_euclid(19);
    let (month, day) = if gregorian {
        let century = year / 100;
        let years_in_cycle = year.rem_euclid(100);
        let leap_centuries = century / 4;
        let century_remainder = century.rem_euclid(4);
        let solar_correction = (century + 8) / 25;
        let lunar_correction = (century - solar_correction + 1) / 3;
        let epact = (19 * golden + century - leap_centuries - lunar_correction + 15).rem_euclid(30);
        let leap_quarters = years_in_cycle / 4;
        let year_remainder = years_in_cycle.rem_euclid(4);
        let weekday =
            (32 + 2 * century_remainder + 2 * leap_quarters - epact - year_remainder).rem_euclid(7);
        let adjust = (golden + 11 * epact + 22 * weekday) / 451;
        let packed = epact + weekday - 7 * adjust + 114;
        (packed / 31, packed.rem_euclid(31) + 1)
    } else {
        let epact = (19 * golden + 15).rem_euclid(30);
        let weekday = (2 * year.rem_euclid(4) + 4 * year.rem_euclid(7) - epact + 34).rem_euclid(7);
        let packed = epact + weekday + 114;
        (packed / 31, packed.rem_euclid(31) + 1)
    };
    if month == 3 {
        (day - 21) as i64
    } else {
        (day + 10) as i64
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn unix_to_jd(timestamp: i64) -> Option<i64> {
    if timestamp < 0 {
        return None;
    }
    let days = timestamp / 86_400;
    let jd = UNIX_EPOCH_JD.checked_add(days)?;
    let date = gregorian_from_jd(jd);
    (date.is_valid() && date.year <= i64::from(i32::MAX)).then_some(jd)
}

fn current_year() -> i64 {
    unix_to_jd(current_timestamp())
        .map(gregorian_from_jd)
        .filter(|date| date.is_valid())
        .map(|date| date.year)
        .unwrap_or(1970)
}

fn parts_to_unix(year: i64, month: i64, day: i64) -> Option<i64> {
    let jd = gregorian_to_jd(month, day, year);
    i128::from(jd - UNIX_EPOCH_JD)
        .checked_mul(86_400)
        .and_then(|value| i64::try_from(value).ok())
}

fn validate_calendar(
    eg: &mut ExecutorGlobals,
    function: &str,
    position: u32,
    calendar: i64,
) -> bool {
    if (0..CAL_NUM_CALS).contains(&calendar) {
        return true;
    }
    value_error(
        eg,
        format!("{function}(): Argument #{position} ($calendar) must be a valid calendar ID"),
    );
    false
}

fn fn_cal_days_in_month(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(calendar) = required_int(ed, eg, "cal_days_in_month", 0, "calendar")? else {
        return Ok(());
    };
    let Some(month) = required_int(ed, eg, "cal_days_in_month", 1, "month")? else {
        return Ok(());
    };
    let Some(year) = required_int(ed, eg, "cal_days_in_month", 2, "year")? else {
        return Ok(());
    };
    if !validate_calendar(eg, "cal_days_in_month", 1, calendar) {
        return Ok(());
    }
    if !(1..=2_147_483_646).contains(&month) {
        value_error(
            eg,
            "cal_days_in_month(): Argument #2 ($month) must be between 1 and 2147483646",
        );
        return Ok(());
    }
    if year > 2_147_483_646 {
        value_error(
            eg,
            "cal_days_in_month(): Argument #3 ($year) must be less than 2147483646",
        );
        return Ok(());
    }
    let start = calendar_to_jd(calendar, month, 1, year);
    if start == 0 {
        value_error(eg, "Invalid date");
        return Ok(());
    }
    let mut next = calendar_to_jd(calendar, month + 1, 1, year);
    if next == 0 {
        let next_year = if year == -1 { 1 } else { year + 1 };
        next = calendar_to_jd(calendar, 1, 1, next_year);
        if calendar == CAL_FRENCH && next == 0 {
            next = FRENCH_LAST_JD + 1;
        }
    }
    write_result(rv, Value::long(next - start));
    Ok(())
}

fn fn_cal_from_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "cal_from_jd", 0, "julian_day")? else {
        return Ok(());
    };
    let Some(calendar) = required_int(ed, eg, "cal_from_jd", 1, "calendar")? else {
        return Ok(());
    };
    if !validate_calendar(eg, "cal_from_jd", 2, calendar) {
        return Ok(());
    }
    write_result(rv, Value::array(cal_from_jd_value(jd, calendar)));
    Ok(())
}

fn fn_cal_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let argument = super::owned_argument(ed, 0);
    let calendar = if argument.value_type() == ValueType::Undef {
        -1
    } else {
        let Some(value) = required_int(ed, eg, "cal_info", 0, "calendar")? else {
            return Ok(());
        };
        value
    };
    if calendar == -1 {
        let mut result = PhpArray::with_hash_capacity(CAL_NUM_CALS as usize);
        for id in 0..CAL_NUM_CALS {
            result.set(ArrayKey::Int(id), Value::array(calendar_info(id)));
        }
        write_result(rv, Value::array(result));
        return Ok(());
    }
    if !validate_calendar(eg, "cal_info", 1, calendar) {
        return Ok(());
    }
    write_result(rv, Value::array(calendar_info(calendar)));
    Ok(())
}

fn fn_cal_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let names = ["calendar", "month", "day", "year"];
    let mut values = [0i64; 4];
    for (index, name) in names.into_iter().enumerate() {
        let Some(value) = required_int(ed, eg, "cal_to_jd", index as u32, name)? else {
            return Ok(());
        };
        values[index] = value;
    }
    let [calendar, month, day, year] = values;
    if !validate_calendar(eg, "cal_to_jd", 1, calendar) {
        return Ok(());
    }
    if !(1..=2_147_483_646).contains(&month) {
        value_error(
            eg,
            "cal_to_jd(): Argument #2 ($month) must be between 1 and 2147483646",
        );
        return Ok(());
    }
    if !(-2_147_483_648..=2_147_483_647).contains(&day) {
        value_error(
            eg,
            "cal_to_jd(): Argument #3 ($day) must be between -2147483648 and 2147483647",
        );
        return Ok(());
    }
    if year > 2_147_483_646 {
        value_error(
            eg,
            "cal_to_jd(): Argument #4 ($year) must be less than 2147483646",
        );
        return Ok(());
    }
    write_result(rv, Value::long(calendar_to_jd(calendar, month, day, year)));
    Ok(())
}

fn fn_gregorian_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(month) = required_int(ed, eg, "gregoriantojd", 0, "month")? else {
        return Ok(());
    };
    let Some(day) = required_int(ed, eg, "gregoriantojd", 1, "day")? else {
        return Ok(());
    };
    let Some(year) = required_int(ed, eg, "gregoriantojd", 2, "year")? else {
        return Ok(());
    };
    write_result(
        rv,
        Value::long(gregorian_to_jd(
            direct_calendar_int(month),
            direct_calendar_int(day),
            direct_calendar_int(year),
        )),
    );
    Ok(())
}

fn fn_julian_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(month) = required_int(ed, eg, "juliantojd", 0, "month")? else {
        return Ok(());
    };
    let Some(day) = required_int(ed, eg, "juliantojd", 1, "day")? else {
        return Ok(());
    };
    let Some(year) = required_int(ed, eg, "juliantojd", 2, "year")? else {
        return Ok(());
    };
    write_result(
        rv,
        Value::long(julian_to_jd(
            direct_calendar_int(month),
            direct_calendar_int(day),
            direct_calendar_int(year),
        )),
    );
    Ok(())
}

fn fn_french_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(month) = required_int(ed, eg, "frenchtojd", 0, "month")? else {
        return Ok(());
    };
    let Some(day) = required_int(ed, eg, "frenchtojd", 1, "day")? else {
        return Ok(());
    };
    let Some(year) = required_int(ed, eg, "frenchtojd", 2, "year")? else {
        return Ok(());
    };
    write_result(
        rv,
        Value::long(french_to_jd(
            direct_calendar_int(month),
            direct_calendar_int(day),
            direct_calendar_int(year),
        )),
    );
    Ok(())
}

fn fn_jewish_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(month) = required_int(ed, eg, "jewishtojd", 0, "month")? else {
        return Ok(());
    };
    let Some(day) = required_int(ed, eg, "jewishtojd", 1, "day")? else {
        return Ok(());
    };
    let Some(year) = required_int(ed, eg, "jewishtojd", 2, "year")? else {
        return Ok(());
    };
    if i32::try_from(month).is_err() {
        value_error(
            eg,
            "jewishtojd(): Argument #1 ($month) must be between -2147483648 and 2147483647",
        );
        return Ok(());
    }
    if i32::try_from(day).is_err() {
        value_error(
            eg,
            "jewishtojd(): Argument #2 ($day) must be between -2147483648 and 2147483647",
        );
        return Ok(());
    }
    if i32::try_from(year).is_err() {
        value_error(
            eg,
            "jewishtojd(): Argument #3 ($year) must be between -2147483648 and 2147483647",
        );
        return Ok(());
    }
    write_result(rv, Value::long(hebrew_to_jd(month, day, year)));
    Ok(())
}

fn fn_jd_to_gregorian(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdtogregorian", 0, "julian_day")? else {
        return Ok(());
    };
    write_result(rv, Value::string(gregorian_from_jd(jd).display()));
    Ok(())
}

fn fn_jd_to_julian(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdtojulian", 0, "julian_day")? else {
        return Ok(());
    };
    write_result(rv, Value::string(julian_from_jd(jd).display()));
    Ok(())
}

fn fn_jd_to_french(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdtofrench", 0, "julian_day")? else {
        return Ok(());
    };
    write_result(rv, Value::string(french_from_jd(jd).display()));
    Ok(())
}

fn fn_jd_to_jewish(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdtojewish", 0, "julian_day")? else {
        return Ok(());
    };
    let hebrew = if super::owned_argument(ed, 1).value_type() == ValueType::Undef {
        false
    } else {
        let Some(value) = super::typed_internal_bool_argument(ed, eg, "jdtojewish", 1, "hebrew")?
        else {
            return Ok(());
        };
        value
    };
    let flags = if super::owned_argument(ed, 2).value_type() == ValueType::Undef {
        0
    } else {
        let Some(value) = required_int(ed, eg, "jdtojewish", 2, "flags")? else {
            return Ok(());
        };
        value
    };
    let date = hebrew_from_jd(jd);
    if hebrew && !(1..=9_999).contains(&date.year) {
        value_error(eg, "Year out of range (0-9999)");
        return Ok(());
    }
    if hebrew {
        write_result(rv, Value::binary_string(&hebrew_date_bytes(date, flags)));
    } else {
        write_result(rv, Value::string(date.display()));
    }
    Ok(())
}

fn fn_jd_day_of_week(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jddayofweek", 0, "julian_day")? else {
        return Ok(());
    };
    let mode = if super::owned_argument(ed, 1).value_type() == ValueType::Undef {
        CAL_DOW_DAYNO
    } else {
        let Some(value) = required_int(ed, eg, "jddayofweek", 1, "mode")? else {
            return Ok(());
        };
        value
    };
    let day = weekday(jd);
    let result = match mode {
        CAL_DOW_LONG => Value::string(WEEKDAYS_LONG[day]),
        CAL_DOW_SHORT => Value::string(WEEKDAYS_SHORT[day]),
        _ => Value::long(day as i64),
    };
    write_result(rv, result);
    Ok(())
}

fn fn_jd_month_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdmonthname", 0, "julian_day")? else {
        return Ok(());
    };
    let Some(mode) = required_int(ed, eg, "jdmonthname", 1, "mode")? else {
        return Ok(());
    };
    let (calendar, short) = match mode {
        CAL_MONTH_GREGORIAN_SHORT => (CAL_GREGORIAN, true),
        CAL_MONTH_GREGORIAN_LONG => (CAL_GREGORIAN, false),
        CAL_MONTH_JULIAN_SHORT => (CAL_JULIAN, true),
        CAL_MONTH_JULIAN_LONG => (CAL_JULIAN, false),
        CAL_MONTH_JEWISH => (CAL_JEWISH, false),
        CAL_MONTH_FRENCH => (CAL_FRENCH, false),
        _ => (CAL_GREGORIAN, true),
    };
    let date = calendar_from_jd(jd, calendar);
    write_result(rv, Value::string(month_name(calendar, date, short)));
    Ok(())
}

fn fn_easter_days(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(year) = optional_nullable_int(ed, eg, "easter_days", 0, "year")? else {
        return Ok(());
    };
    let year = year.unwrap_or_else(current_year);
    let mode = if super::owned_argument(ed, 1).value_type() == ValueType::Undef {
        CAL_EASTER_DEFAULT
    } else {
        let Some(value) = required_int(ed, eg, "easter_days", 1, "mode")? else {
            return Ok(());
        };
        value
    };
    if !(1..=MAX_EASTER_DAYS_YEAR).contains(&year) {
        value_error(
            eg,
            format!(
                "easter_days(): Argument #1 ($year) must be between 1 and {MAX_EASTER_DAYS_YEAR}"
            ),
        );
        return Ok(());
    }
    write_result(rv, Value::long(easter_days_for(year, mode)));
    Ok(())
}

fn fn_easter_date(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(year) = optional_nullable_int(ed, eg, "easter_date", 0, "year")? else {
        return Ok(());
    };
    let year = year.unwrap_or_else(current_year);
    let mode = if super::owned_argument(ed, 1).value_type() == ValueType::Undef {
        CAL_EASTER_DEFAULT
    } else {
        let Some(value) = required_int(ed, eg, "easter_date", 1, "mode")? else {
            return Ok(());
        };
        value
    };
    if !(1..=MAX_EASTER_DAYS_YEAR).contains(&year) {
        value_error(
            eg,
            format!(
                "easter_date(): Argument #1 ($year) must be between 1 and {MAX_EASTER_DAYS_YEAR}"
            ),
        );
        return Ok(());
    }
    if year < 1970 {
        value_error(
            eg,
            "easter_date(): Argument #1 ($year) must be a year after 1970 (inclusive)",
        );
        return Ok(());
    }
    if year > 2_000_000_000 {
        value_error(
            eg,
            "easter_date(): Argument #1 ($year) must be a year before 2.000.000.000 (inclusive)",
        );
        return Ok(());
    }
    let offset = easter_days_for(year, mode);
    let (month, day) = if offset <= 10 {
        (3, 21 + offset)
    } else {
        (4, offset - 10)
    };
    let Some(timestamp) = parts_to_unix(year, month, day) else {
        return Ok(());
    };
    write_result(rv, Value::long(timestamp));
    Ok(())
}

fn fn_jd_to_unix(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(jd) = required_int(ed, eg, "jdtounix", 0, "julian_day")? else {
        return Ok(());
    };
    if !(UNIX_EPOCH_JD..=MAX_UNIX_JD).contains(&jd) {
        value_error(
            eg,
            format!("jday must be between {UNIX_EPOCH_JD} and {MAX_UNIX_JD}"),
        );
        return Ok(());
    }
    write_result(rv, Value::long((jd - UNIX_EPOCH_JD) * 86_400));
    Ok(())
}

fn fn_unix_to_jd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(timestamp) = optional_nullable_int(ed, eg, "unixtojd", 0, "timestamp")? else {
        return Ok(());
    };
    let timestamp = timestamp.unwrap_or_else(current_timestamp);
    if timestamp < 0 {
        value_error(
            eg,
            "unixtojd(): Argument #1 ($timestamp) must be greater than or equal to 0",
        );
        return Ok(());
    }
    match unix_to_jd(timestamp) {
        Some(jd) => write_result(rv, Value::long(jd)),
        None => write_result(rv, Value::bool(false)),
    }
    Ok(())
}

struct Declaration {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    parameter_types: fn() -> Vec<ParamTypeHint>,
    return_type: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
    diagnostics: Option<&'static [Option<&'static str>]>,
}

fn no_defaults<const N: usize>() -> Vec<Option<Value>> {
    vec![None; N]
}

fn ints<const N: usize>() -> Vec<ParamTypeHint> {
    vec![ParamTypeHint::Int; N]
}

fn int_type() -> ParamTypeHint {
    ParamTypeHint::Int
}

fn string_type() -> ParamTypeHint {
    ParamTypeHint::String
}

fn array_type() -> ParamTypeHint {
    ParamTypeHint::Array
}

fn string_or_int_type() -> ParamTypeHint {
    ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Int])
}

fn int_or_false_type() -> ParamTypeHint {
    ParamTypeHint::Union(vec![
        ParamTypeHint::Int,
        ParamTypeHint::ClassName("false".to_string()),
    ])
}

fn nullable_int_and_int() -> Vec<ParamTypeHint> {
    vec![
        ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
        ParamTypeHint::Int,
    ]
}

fn jd_to_jewish_types() -> Vec<ParamTypeHint> {
    vec![ParamTypeHint::Int, ParamTypeHint::Bool, ParamTypeHint::Int]
}

const CAL_INFO_DIAGNOSTICS: &[Option<&str>] = &[Some("-1")];
const EASTER_DIAGNOSTICS: &[Option<&str>] = &[Some("null"), Some("CAL_EASTER_DEFAULT")];
const DAY_OF_WEEK_DIAGNOSTICS: &[Option<&str>] = &[None, Some("CAL_DOW_DAYNO")];
const JD_TO_JEWISH_DIAGNOSTICS: &[Option<&str>] = &[None, Some("false"), Some("0")];
const UNIX_TO_JD_DIAGNOSTICS: &[Option<&str>] = &[Some("null")];

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = [
        Declaration {
            name: "cal_days_in_month",
            handler: fn_cal_days_in_month,
            required: 3,
            parameters: &["calendar", "month", "year"],
            parameter_types: ints::<3>,
            return_type: int_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "cal_from_jd",
            handler: fn_cal_from_jd,
            required: 2,
            parameters: &["julian_day", "calendar"],
            parameter_types: ints::<2>,
            return_type: array_type,
            defaults: no_defaults::<2>,
            diagnostics: None,
        },
        Declaration {
            name: "cal_info",
            handler: fn_cal_info,
            required: 0,
            parameters: &["calendar"],
            parameter_types: ints::<1>,
            return_type: array_type,
            defaults: || vec![Some(Value::long(-1))],
            diagnostics: Some(CAL_INFO_DIAGNOSTICS),
        },
        Declaration {
            name: "cal_to_jd",
            handler: fn_cal_to_jd,
            required: 4,
            parameters: &["calendar", "month", "day", "year"],
            parameter_types: ints::<4>,
            return_type: int_type,
            defaults: no_defaults::<4>,
            diagnostics: None,
        },
        Declaration {
            name: "easter_date",
            handler: fn_easter_date,
            required: 0,
            parameters: &["year", "mode"],
            parameter_types: nullable_int_and_int,
            return_type: int_type,
            defaults: || vec![Some(Value::null()), Some(Value::long(CAL_EASTER_DEFAULT))],
            diagnostics: Some(EASTER_DIAGNOSTICS),
        },
        Declaration {
            name: "easter_days",
            handler: fn_easter_days,
            required: 0,
            parameters: &["year", "mode"],
            parameter_types: nullable_int_and_int,
            return_type: int_type,
            defaults: || vec![Some(Value::null()), Some(Value::long(CAL_EASTER_DEFAULT))],
            diagnostics: Some(EASTER_DIAGNOSTICS),
        },
        Declaration {
            name: "frenchtojd",
            handler: fn_french_to_jd,
            required: 3,
            parameters: &["month", "day", "year"],
            parameter_types: ints::<3>,
            return_type: int_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "gregoriantojd",
            handler: fn_gregorian_to_jd,
            required: 3,
            parameters: &["month", "day", "year"],
            parameter_types: ints::<3>,
            return_type: int_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "jddayofweek",
            handler: fn_jd_day_of_week,
            required: 1,
            parameters: &["julian_day", "mode"],
            parameter_types: ints::<2>,
            return_type: string_or_int_type,
            defaults: || vec![None, Some(Value::long(CAL_DOW_DAYNO))],
            diagnostics: Some(DAY_OF_WEEK_DIAGNOSTICS),
        },
        Declaration {
            name: "jdmonthname",
            handler: fn_jd_month_name,
            required: 2,
            parameters: &["julian_day", "mode"],
            parameter_types: ints::<2>,
            return_type: string_type,
            defaults: no_defaults::<2>,
            diagnostics: None,
        },
        Declaration {
            name: "jdtofrench",
            handler: fn_jd_to_french,
            required: 1,
            parameters: &["julian_day"],
            parameter_types: ints::<1>,
            return_type: string_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "jdtogregorian",
            handler: fn_jd_to_gregorian,
            required: 1,
            parameters: &["julian_day"],
            parameter_types: ints::<1>,
            return_type: string_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "jdtojewish",
            handler: fn_jd_to_jewish,
            required: 1,
            parameters: &["julian_day", "hebrew", "flags"],
            parameter_types: jd_to_jewish_types,
            return_type: string_type,
            defaults: || vec![None, Some(Value::bool(false)), Some(Value::long(0))],
            diagnostics: Some(JD_TO_JEWISH_DIAGNOSTICS),
        },
        Declaration {
            name: "jdtojulian",
            handler: fn_jd_to_julian,
            required: 1,
            parameters: &["julian_day"],
            parameter_types: ints::<1>,
            return_type: string_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "jdtounix",
            handler: fn_jd_to_unix,
            required: 1,
            parameters: &["julian_day"],
            parameter_types: ints::<1>,
            return_type: int_type,
            defaults: no_defaults::<1>,
            diagnostics: None,
        },
        Declaration {
            name: "jewishtojd",
            handler: fn_jewish_to_jd,
            required: 3,
            parameters: &["month", "day", "year"],
            parameter_types: ints::<3>,
            return_type: int_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "juliantojd",
            handler: fn_julian_to_jd,
            required: 3,
            parameters: &["month", "day", "year"],
            parameter_types: ints::<3>,
            return_type: int_type,
            defaults: no_defaults::<3>,
            diagnostics: None,
        },
        Declaration {
            name: "unixtojd",
            handler: fn_unix_to_jd,
            required: 0,
            parameters: &["timestamp"],
            parameter_types: || vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int))],
            return_type: int_or_false_type,
            defaults: || vec![Some(Value::null())],
            diagnostics: Some(UNIX_TO_JD_DIAGNOSTICS),
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let mut function = Box::new(
            make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.common.sig.param_type_hints = (declaration.parameter_types)();
        function.common.sig.return_type_hint = (declaration.return_type)();
        function.handler_validates_types = true;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("calendar function registration is unique");
        let defaults = (declaration.defaults)();
        if let Some(diagnostics) = declaration.diagnostics {
            eg.register_internal_function_reflection_metadata_with_diagnostics(
                pointer,
                defaults,
                diagnostics,
                "calendar",
            );
        } else {
            eg.register_internal_function_reflection_metadata(pointer, defaults, "calendar");
        }
        functions.push(function);
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_calendars_round_trip_oracle_boundaries() {
        assert_eq!(gregorian_to_jd(1, 1, 1970), UNIX_EPOCH_JD);
        assert_eq!(
            gregorian_from_jd(2_451_604),
            CivilDate {
                month: 2,
                day: 29,
                year: 2000
            }
        );
        assert_eq!(
            gregorian_from_jd(gregorian_to_jd(2, 29, 1900)),
            CivilDate {
                month: 3,
                day: 1,
                year: 1900
            }
        );
        assert_eq!(julian_to_jd(10, 4, 1582), 2_299_160);
        assert_eq!(
            julian_from_jd(2_299_160),
            CivilDate {
                month: 10,
                day: 4,
                year: 1582
            }
        );
        assert_eq!(french_to_jd(1, 1, 1), FRENCH_EPOCH_JD);
        assert_eq!(
            french_from_jd(FRENCH_LAST_JD),
            CivilDate {
                month: 13,
                day: 5,
                year: 14
            }
        );
    }

    #[test]
    fn hebrew_calendar_handles_variable_and_leap_years() {
        assert_eq!(hebrew_to_jd(1, 1, 1), HEBREW_EPOCH_JD);
        assert_eq!(hebrew_to_jd(1, 1, 5_785), 2_460_587);
        assert_eq!(
            hebrew_from_jd(2_460_587),
            CivilDate {
                month: 1,
                day: 1,
                year: 5_785
            }
        );
        assert_eq!(hebrew_days_in_month(2, 5_771), Some(30));
        assert_eq!(hebrew_days_in_month(3, 5_772), Some(30));
        assert_eq!(hebrew_days_in_month(6, 5_772), Some(0));
        assert_eq!(hebrew_days_in_month(6, 5_784), Some(30));
    }

    #[test]
    fn easter_modes_match_cutovers_and_modern_dates() {
        assert_eq!(easter_days_for(1_582, CAL_EASTER_DEFAULT), 25);
        assert_eq!(easter_days_for(1_583, CAL_EASTER_ROMAN), 20);
        assert_eq!(easter_days_for(1_752, CAL_EASTER_DEFAULT), 8);
        assert_eq!(easter_days_for(1_753, CAL_EASTER_DEFAULT), 32);
        assert_eq!(easter_days_for(2_025, CAL_EASTER_ALWAYS_GREGORIAN), 30);
        assert_eq!(easter_days_for(2_025, CAL_EASTER_ALWAYS_JULIAN), 17);
    }

    #[test]
    fn hebrew_text_projection_is_byte_exact_iso_8859_8() {
        let date = CivilDate {
            month: 1,
            day: 1,
            year: 5_785,
        };
        assert_eq!(
            hebrew_date_bytes(date, 0),
            b"\xe0 \xfa\xf9\xf8\xe9 \xe4\xfa\xf9\xf4\xe4"
        );
        assert_eq!(
            hebrew_date_bytes(
                date,
                CAL_JEWISH_ADD_ALAFIM_GERESH | CAL_JEWISH_ADD_GERESHAYIM
            ),
            b"\xe0\x27 \xfa\xf9\xf8\xe9 \xe4\x27\xfa\xf9\xf4\x22\xe4"
        );
    }
}
