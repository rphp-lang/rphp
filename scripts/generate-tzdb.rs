//! Compile the public-domain IANA timezone source tables into RPHP's compact
//! transition database.
//!
//! This is an independent parser/compiler.  It intentionally consumes only
//! the data-only tzdb distribution and does not invoke or translate `zic`,
//! timelib, or any PHP implementation source.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_FILES: &[&str] = &[
    "africa",
    "antarctica",
    "asia",
    "australasia",
    "europe",
    "northamerica",
    "southamerica",
    "etcetera",
    "factory",
    "backward",
    // backzone restores pre-1970 histories for identifiers that are links in
    // the main data set.  A concrete Zone always takes precedence over a Link.
    "backzone",
];

const GENERATED_START_YEAR: i32 = 1800;
const GENERATED_END_YEAR: i32 = 2501;
const MAGIC: &[u8; 8] = b"RPHTZ02\0";

// PHP 8.5's 2026.1 timezone inventory intentionally omits this backzone-only
// identifier even though it is present in the raw 2026a archive.  Keeping the
// filter beside generation makes the public compatibility choice auditable.
const PHP_EXCLUDED_IDENTIFIERS: &[&str] = &["Asia/Hanoi"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClockBasis {
    Wall,
    Standard,
    Utc,
}

#[derive(Clone, Copy, Debug)]
struct ClockTime {
    seconds: i32,
    basis: ClockBasis,
}

#[derive(Clone, Copy, Debug)]
enum DaySpec {
    Exact(u8),
    Last(u8),
    OnOrAfter { weekday: u8, day: u8 },
    OnOrBefore { weekday: u8, day: u8 },
}

#[derive(Clone, Debug)]
struct Rule {
    from: i32,
    to: i32,
    month: u8,
    day: DaySpec,
    at: ClockTime,
    save: i32,
    letters: String,
}

#[derive(Clone, Debug)]
enum RuleMode {
    None,
    Fixed(i32),
    Named(String),
}

#[derive(Clone, Debug)]
struct Until {
    year: i32,
    month: u8,
    day: DaySpec,
    at: ClockTime,
}

#[derive(Clone, Debug)]
struct Era {
    base_offset: i32,
    rules: RuleMode,
    format: String,
    until: Option<Until>,
}

#[derive(Default)]
struct SourceDatabase {
    rules: HashMap<String, Vec<Rule>>,
    zones: BTreeMap<String, Vec<Era>>,
    links: BTreeMap<String, String>,
    packrat_zones: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NegativeDstSection {
    Ordinary,
    SkipVanguard,
    ReadRearguard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    offset: i32,
    abbreviation: String,
    is_dst: bool,
}

#[derive(Clone, Debug)]
struct Transition {
    timestamp: i64,
    state: State,
}

#[derive(Clone, Debug)]
struct CompiledZone {
    name: String,
    initial: State,
    transitions: Vec<Transition>,
}

#[derive(Clone, Debug)]
struct RuleEvent {
    nominal: i64,
    timestamp: i64,
    save: i32,
    letters: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("tzdb generation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let source = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: generate-tzdb <IANA source directory> <output file>".to_string())?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: generate-tzdb <IANA source directory> <output file>".to_string())?;
    if arguments.next().is_some() {
        return Err("usage: generate-tzdb <IANA source directory> <output file>".to_string());
    }

    let version = fs::read_to_string(source.join("version"))
        .map_err(|error| format!("read version: {error}"))?
        .trim()
        .to_string();
    if version != "2026a" {
        return Err(format!("expected IANA release 2026a, found {version:?}"));
    }

    let database = parse_sources(&source)?;
    let compiled = compile_database(&database)?;
    let bytes = encode_database(&version, &database, &compiled)?;
    let temporary = output.with_extension("bin.tmp");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(&temporary, bytes)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &output)
        .map_err(|error| format!("replace {}: {error}", output.display()))?;
    Ok(())
}

fn parse_sources(root: &Path) -> Result<SourceDatabase, String> {
    let mut database = SourceDatabase::default();
    let zone_table = fs::read_to_string(root.join("zone.tab"))
        .map_err(|error| format!("read zone.tab: {error}"))?;
    for line in zone_table.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if !line.starts_with('#') && fields.len() >= 3 {
            database.packrat_zones.insert(fields[2].to_string());
        }
    }
    for filename in SOURCE_FILES {
        parse_source_file(root, filename, &mut database)?;
    }
    for (name, eras) in &database.zones {
        if eras.is_empty() {
            return Err(format!("zone {name} has no eras"));
        }
        if eras[..eras.len() - 1]
            .iter()
            .any(|era| era.until.is_none())
        {
            return Err(format!("zone {name} has a non-final open era"));
        }
        if eras.last().is_some_and(|era| era.until.is_some()) {
            return Err(format!("zone {name} has no final open era"));
        }
    }
    Ok(database)
}

fn parse_source_file(
    root: &Path,
    filename: &str,
    database: &mut SourceDatabase,
) -> Result<(), String> {
    let path = root.join(filename);
    let input = fs::read_to_string(&path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut current_zone: Option<String> = None;
    let mut negative_dst_section = NegativeDstSection::Ordinary;

    for (line_index, raw_line) in input.lines().enumerate() {
        let line_number = line_index + 1;
        if filename == "backzone" && raw_line.starts_with("#PACKRATLIST zone.tab Link ") {
            let fields: Vec<_> = raw_line.split_whitespace().collect();
            if fields.len() != 5 {
                return Err(format!(
                    "{}:{line_number}: malformed PACKRATLIST Link record",
                    path.display()
                ));
            }
            database
                .links
                .insert(fields[4].to_string(), fields[3].to_string());
            continue;
        }
        if raw_line.contains("Vanguard section") && raw_line.contains("negative DST") {
            negative_dst_section = NegativeDstSection::SkipVanguard;
            continue;
        }
        if raw_line.contains("Rearguard section") && raw_line.contains("negative DST") {
            negative_dst_section = NegativeDstSection::ReadRearguard;
            continue;
        }
        if raw_line.contains("End of rearguard section")
            && negative_dst_section != NegativeDstSection::Ordinary
        {
            negative_dst_section = NegativeDstSection::Ordinary;
            continue;
        }
        let record_line = match negative_dst_section {
            NegativeDstSection::Ordinary => raw_line,
            NegativeDstSection::SkipVanguard => continue,
            NegativeDstSection::ReadRearguard => {
                let Some(record) = rearguard_record(raw_line) else {
                    continue;
                };
                record
            }
        };
        let content = record_line.split('#').next().unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = content.split_whitespace().collect();
        let location = || format!("{}:{line_number}", path.display());
        match fields.first().copied() {
            Some("Rule") => {
                current_zone = None;
                if fields.len() != 10 {
                    return Err(format!("{}: malformed Rule record", location()));
                }
                let from = parse_year(fields[2], None, &location())?;
                let to = parse_year(fields[3], Some(from), &location())?;
                if fields[4] != "-" {
                    return Err(format!(
                        "{}: unsupported nontrivial Rule TYPE {:?}",
                        location(),
                        fields[4]
                    ));
                }
                let rule = Rule {
                    from,
                    to,
                    month: parse_month(fields[5], &location())?,
                    day: parse_day_spec(fields[6], &location())?,
                    at: parse_clock_time(fields[7], &location())?,
                    save: parse_offset(fields[8], &location())?,
                    letters: if fields[9] == "-" {
                        String::new()
                    } else {
                        fields[9].to_string()
                    },
                };
                database
                    .rules
                    .entry(fields[1].to_string())
                    .or_default()
                    .push(rule);
            }
            Some("Zone") => {
                if fields.len() < 5 {
                    return Err(format!("{}: malformed Zone record", location()));
                }
                let name = fields[1].to_string();
                let era = parse_era(&fields[2..], &location())?;
                // backzone supplies concrete pre-1970 histories for names that
                // appear as links in the main set.  It must not replace a real
                // Zone from the primary regional files.
                if filename == "backzone" && database.zones.contains_key(&name) {
                    return Err(format!(
                        "{}: backzone unexpectedly duplicates concrete zone {name}",
                        location()
                    ));
                }
                if database.zones.contains_key(&name) {
                    return Err(format!("{}: duplicate Zone {name}", location()));
                }
                database.zones.insert(name.clone(), vec![era]);
                current_zone = Some(name);
            }
            Some("Link") => {
                current_zone = None;
                if fields.len() != 3 {
                    return Err(format!("{}: malformed Link record", location()));
                }
                // backzone deliberately retargets a few compatibility links
                // to the restored concrete pre-1970 zone.  Source-file order
                // is therefore significant and the later data wins.
                database
                    .links
                    .insert(fields[2].to_string(), fields[1].to_string());
            }
            Some(_) if record_line.starts_with(char::is_whitespace) => {
                let name = current_zone.as_ref().ok_or_else(|| {
                    format!("{}: continuation without preceding Zone", location())
                })?;
                let era = parse_era(&fields, &location())?;
                database
                    .zones
                    .get_mut(name)
                    .expect("current zone exists")
                    .push(era);
            }
            Some(other) => {
                return Err(format!("{}: unsupported record {other:?}", location()));
            }
            None => {}
        }
    }
    Ok(())
}

fn rearguard_record(line: &str) -> Option<&str> {
    let uncommented = line.strip_prefix('#')?;
    let trimmed = uncommented.trim_start();
    if trimmed.starts_with("Rule\t")
        || trimmed.starts_with("Zone\t")
        || trimmed.starts_with("Link\t")
    {
        return Some(trimmed);
    }
    if uncommented.starts_with(char::is_whitespace)
        && trimmed
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
    {
        return Some(uncommented);
    }
    None
}

fn parse_era(fields: &[&str], location: &str) -> Result<Era, String> {
    if fields.len() < 3 {
        return Err(format!("{location}: malformed Zone era"));
    }
    let rules = match fields[1] {
        "-" => RuleMode::None,
        value if looks_like_offset(value) => RuleMode::Fixed(parse_offset(value, location)?),
        value => RuleMode::Named(value.to_string()),
    };
    let until = if fields.len() == 3 {
        None
    } else {
        if fields.len() > 7 {
            return Err(format!("{location}: too many Zone UNTIL fields"));
        }
        let year = fields[3]
            .parse::<i32>()
            .map_err(|_| format!("{location}: invalid UNTIL year {:?}", fields[3]))?;
        let month = fields
            .get(4)
            .map(|value| parse_month(value, location))
            .transpose()?
            .unwrap_or(1);
        let day = fields
            .get(5)
            .map(|value| parse_day_spec(value, location))
            .transpose()?
            .unwrap_or(DaySpec::Exact(1));
        let at = fields
            .get(6)
            .map(|value| parse_clock_time(value, location))
            .transpose()?
            .unwrap_or(ClockTime {
                seconds: 0,
                basis: ClockBasis::Wall,
            });
        Some(Until {
            year,
            month,
            day,
            at,
        })
    };
    Ok(Era {
        base_offset: parse_offset(fields[0], location)?,
        rules,
        format: fields[2].to_string(),
        until,
    })
}

fn parse_year(value: &str, only_year: Option<i32>, location: &str) -> Result<i32, String> {
    match value {
        "only" => only_year.ok_or_else(|| format!("{location}: unexpected 'only' year")),
        "max" | "maximum" => Ok(i32::MAX),
        "min" | "minimum" => Ok(i32::MIN),
        _ => value
            .parse::<i32>()
            .map_err(|_| format!("{location}: invalid year {value:?}")),
    }
}

fn parse_month(value: &str, location: &str) -> Result<u8, String> {
    let prefix: String = value.chars().take(3).collect::<String>().to_ascii_lowercase();
    match prefix.as_str() {
        "jan" => Ok(1),
        "feb" => Ok(2),
        "mar" => Ok(3),
        "apr" => Ok(4),
        "may" => Ok(5),
        "jun" => Ok(6),
        "jul" => Ok(7),
        "aug" => Ok(8),
        "sep" => Ok(9),
        "oct" => Ok(10),
        "nov" => Ok(11),
        "dec" => Ok(12),
        _ => Err(format!("{location}: invalid month {value:?}")),
    }
}

fn parse_weekday(value: &str, location: &str) -> Result<u8, String> {
    let prefix: String = value.chars().take(3).collect::<String>().to_ascii_lowercase();
    match prefix.as_str() {
        "sun" => Ok(0),
        "mon" => Ok(1),
        "tue" => Ok(2),
        "wed" => Ok(3),
        "thu" => Ok(4),
        "fri" => Ok(5),
        "sat" => Ok(6),
        _ => Err(format!("{location}: invalid weekday {value:?}")),
    }
}

fn parse_day_spec(value: &str, location: &str) -> Result<DaySpec, String> {
    if let Ok(day) = value.parse::<u8>() {
        return Ok(DaySpec::Exact(day));
    }
    if let Some(weekday) = value.strip_prefix("last") {
        return Ok(DaySpec::Last(parse_weekday(weekday, location)?));
    }
    if let Some((weekday, day)) = value.split_once(">=") {
        return Ok(DaySpec::OnOrAfter {
            weekday: parse_weekday(weekday, location)?,
            day: day
                .parse::<u8>()
                .map_err(|_| format!("{location}: invalid day spec {value:?}"))?,
        });
    }
    if let Some((weekday, day)) = value.split_once("<=") {
        return Ok(DaySpec::OnOrBefore {
            weekday: parse_weekday(weekday, location)?,
            day: day
                .parse::<u8>()
                .map_err(|_| format!("{location}: invalid day spec {value:?}"))?,
        });
    }
    Err(format!("{location}: invalid day spec {value:?}"))
}

fn parse_clock_time(value: &str, location: &str) -> Result<ClockTime, String> {
    let (number, basis) = match value.as_bytes().last().copied() {
        Some(b'w') => (&value[..value.len() - 1], ClockBasis::Wall),
        Some(b's') => (&value[..value.len() - 1], ClockBasis::Standard),
        Some(b'u' | b'g' | b'z') => (&value[..value.len() - 1], ClockBasis::Utc),
        _ => (value, ClockBasis::Wall),
    };
    Ok(ClockTime {
        seconds: parse_offset(number, location)?,
        basis,
    })
}

fn looks_like_offset(value: &str) -> bool {
    value
        .as_bytes()
        .iter()
        .any(|byte| byte.is_ascii_digit())
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b':'))
}

fn parse_offset(value: &str, location: &str) -> Result<i32, String> {
    let (sign, digits) = match value.as_bytes().first().copied() {
        Some(b'-') => (-1_i64, &value[1..]),
        Some(b'+') => (1_i64, &value[1..]),
        _ => (1_i64, value),
    };
    let mut parts = digits.split(':');
    let hours = parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or_else(|| format!("{location}: invalid offset {value:?}"))?;
    let minutes = parts
        .next()
        .map(|part| part.parse::<i64>())
        .transpose()
        .map_err(|_| format!("{location}: invalid offset {value:?}"))?
        .unwrap_or(0);
    let seconds = parts
        .next()
        .map(|part| part.parse::<i64>())
        .transpose()
        .map_err(|_| format!("{location}: invalid offset {value:?}"))?
        .unwrap_or(0);
    if parts.next().is_some() || minutes >= 60 || seconds >= 60 {
        return Err(format!("{location}: invalid offset {value:?}"));
    }
    let total = sign * (hours * 3_600 + minutes * 60 + seconds);
    i32::try_from(total).map_err(|_| format!("{location}: offset out of range {value:?}"))
}

fn compile_database(database: &SourceDatabase) -> Result<Vec<CompiledZone>, String> {
    let mut output = Vec::with_capacity(database.zones.len());
    for (name, eras) in &database.zones {
        if PHP_EXCLUDED_IDENTIFIERS.contains(&name.as_str()) {
            continue;
        }
        if database.links.contains_key(name)
            && !database.packrat_zones.contains(name)
            && name.contains('/')
        {
            continue;
        }
        output.push(compile_zone(name, eras, &database.rules)?);
    }
    Ok(output)
}

fn compile_zone(
    name: &str,
    eras: &[Era],
    all_rules: &HashMap<String, Vec<Rule>>,
) -> Result<CompiledZone, String> {
    let first = eras.first().expect("validated nonempty zone");
    let (initial_save, initial_letters) = initial_rule_state(&first.rules);
    let mut initial = state_for(first, initial_save, initial_letters, 0);
    let mut transitions = Vec::new();
    let mut current = initial.clone();
    let mut start = i64::MIN;

    for era in eras {
        let mut events = match &era.rules {
            RuleMode::Named(name) => {
                let rules = all_rules
                    .get(name)
                    .ok_or_else(|| format!("zone {name}: missing rule set {name}"))?;
                compile_rule_events(rules, era.base_offset)?
            }
            RuleMode::None | RuleMode::Fixed(_) => Vec::new(),
        };
        let end = era
            .until
            .as_ref()
            .map(|until| era_end_timestamp(until, era.base_offset, &events))
            .transpose()?
            .unwrap_or(i64::MAX);
        if end <= start {
            return Err(format!("zone {name}: non-increasing era boundary"));
        }

        // A rule may begin at the exact wall-clock instant that starts a new
        // zone era.  Its `w` basis is interpreted using the state inherited
        // from the preceding era (Europe/Berlin's 1945 SovietZone boundary is
        // the canonical example), not a synthetic zero-save state.  Align the
        // event with the already-resolved era boundary so the intermediate
        // state is never published.
        if start != i64::MIN {
            let inherited_wall = start.saturating_add(i64::from(current.offset));
            if let Some(event) = events
                .iter_mut()
                .find(|event| event.nominal == inherited_wall)
            {
                event.timestamp = start;
                events.sort_by_key(|event| event.timestamp);
            }
        }

        let (enter_save, _) = state_before(start, &era.rules, &events, "");
        // The parser selects IANA's annotated rearguard records for negative
        // DST regions.  Their minimum save is therefore ordinary standard
        // time and every larger save is daylight time, matching PHP's `I`.
        let standard_save = events
            .iter()
            .filter(|event| event.timestamp >= start && event.timestamp < end)
            .map(|event| event.save)
            .chain([0, enter_save])
            .min()
            .unwrap_or(0);
        let default_letters = default_standard_letters(&era.rules, all_rules, standard_save)?;
        let (enter_save, enter_letters) =
            state_before(start, &era.rules, &events, default_letters);
        let enter = state_for(era, enter_save, enter_letters, standard_save);
        if start == i64::MIN {
            initial = enter.clone();
            current = enter;
        } else {
            push_transition(&mut transitions, &mut current, start, enter);
        }

        for event in events
            .iter()
            .filter(|event| event.timestamp >= start && event.timestamp < end)
        {
            let state = state_for(era, event.save, &event.letters, standard_save);
            push_transition(&mut transitions, &mut current, event.timestamp, state);
        }
        start = end;
    }

    Ok(CompiledZone {
        name: name.to_string(),
        initial,
        transitions,
    })
}

fn initial_rule_state(mode: &RuleMode) -> (i32, &str) {
    match mode {
        RuleMode::Fixed(save) => (*save, ""),
        RuleMode::None | RuleMode::Named(_) => (0, ""),
    }
}

fn default_standard_letters<'a>(
    mode: &RuleMode,
    all_rules: &'a HashMap<String, Vec<Rule>>,
    standard_save: i32,
) -> Result<&'a str, String> {
    let RuleMode::Named(name) = mode else {
        return Ok("");
    };
    let rules = all_rules
        .get(name)
        .ok_or_else(|| format!("missing rule set {name}"))?;
    Ok(rules
        .iter()
        .filter(|rule| rule.save == standard_save)
        .min_by_key(|rule| (rule.from, rule.month))
        .map(|rule| rule.letters.as_str())
        .unwrap_or(""))
}

fn state_before<'a>(
    timestamp: i64,
    mode: &RuleMode,
    events: &'a [RuleEvent],
    default_letters: &'a str,
) -> (i32, &'a str) {
    match mode {
        RuleMode::None => (0, ""),
        RuleMode::Fixed(save) => (*save, ""),
        RuleMode::Named(_) => events
            .iter()
            .rev()
            .find(|event| event.timestamp < timestamp)
            .map(|event| (event.save, event.letters.as_str()))
            .unwrap_or((0, default_letters)),
    }
}

fn state_for(era: &Era, save: i32, letters: &str, standard_save: i32) -> State {
    let offset = era.base_offset.saturating_add(save);
    State {
        offset,
        abbreviation: format_abbreviation(&era.format, letters, save, offset),
        is_dst: save > standard_save,
    }
}

fn format_abbreviation(format: &str, letters: &str, save: i32, offset: i32) -> String {
    if let Some((standard, daylight)) = format.split_once('/') {
        return if save == 0 { standard } else { daylight }.to_string();
    }
    if format.contains("%s") {
        return format.replace("%s", letters);
    }
    if format == "%z" {
        return numeric_abbreviation(offset);
    }
    format.to_string()
}

fn numeric_abbreviation(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = i64::from(offset).unsigned_abs();
    let hours = absolute / 3_600;
    let minutes = absolute % 3_600 / 60;
    let seconds = absolute % 60;
    if seconds != 0 {
        format!("{sign}{hours:02}{minutes:02}{seconds:02}")
    } else if minutes != 0 {
        format!("{sign}{hours:02}{minutes:02}")
    } else {
        format!("{sign}{hours:02}")
    }
}

fn compile_rule_events(rules: &[Rule], base_offset: i32) -> Result<Vec<RuleEvent>, String> {
    #[derive(Clone)]
    struct Pending<'a> {
        nominal: i64,
        at: ClockTime,
        save: i32,
        letters: &'a str,
    }

    let mut pending = Vec::new();
    for rule in rules {
        let first = rule.from.max(GENERATED_START_YEAR - 1);
        let last = rule.to.min(GENERATED_END_YEAR + 1);
        if last < first {
            continue;
        }
        for year in first..=last {
            let day = resolve_day(year, rule.month, rule.day)?;
            let nominal = civil_timestamp(year, rule.month, day, 0, 0, 0)
                .checked_add(i64::from(rule.at.seconds))
                .ok_or_else(|| "rule timestamp overflow".to_string())?;
            pending.push(Pending {
                nominal,
                at: rule.at,
                save: rule.save,
                letters: &rule.letters,
            });
        }
    }
    pending.sort_by_key(|event| event.nominal);

    let mut current_save = 0_i32;
    let mut output = Vec::with_capacity(pending.len());
    for event in pending {
        let adjustment = match event.at.basis {
            ClockBasis::Utc => 0,
            ClockBasis::Standard => base_offset,
            ClockBasis::Wall => base_offset.saturating_add(current_save),
        };
        let timestamp = event.nominal.saturating_sub(i64::from(adjustment));
        output.push(RuleEvent {
            nominal: event.nominal,
            timestamp,
            save: event.save,
            letters: event.letters.to_string(),
        });
        current_save = event.save;
    }
    output.sort_by_key(|event| event.timestamp);
    Ok(output)
}

fn era_end_timestamp(
    until: &Until,
    base_offset: i32,
    events: &[RuleEvent],
) -> Result<i64, String> {
    let day = resolve_day(until.year, until.month, until.day)?;
    let nominal = civil_timestamp(until.year, until.month, day, 0, 0, 0)
        .checked_add(i64::from(until.at.seconds))
        .ok_or_else(|| "era boundary overflow".to_string())?;
    let save = events
        .iter()
        .filter(|event| event.nominal < nominal)
        .max_by_key(|event| event.nominal)
        .map(|event| event.save)
        .unwrap_or(0);
    let adjustment = match until.at.basis {
        ClockBasis::Utc => 0,
        ClockBasis::Standard => base_offset,
        ClockBasis::Wall => base_offset.saturating_add(save),
    };
    Ok(nominal.saturating_sub(i64::from(adjustment)))
}

fn push_transition(
    transitions: &mut Vec<Transition>,
    current: &mut State,
    timestamp: i64,
    state: State,
) {
    if *current == state {
        return;
    }
    if let Some(last) = transitions.last_mut().filter(|last| last.timestamp == timestamp) {
        last.state = state.clone();
    } else {
        transitions.push(Transition {
            timestamp,
            state: state.clone(),
        });
    }
    *current = state;
}

fn resolve_day(year: i32, month: u8, spec: DaySpec) -> Result<i32, String> {
    let month_days = i32::from(days_in_month(year, month));
    let day = match spec {
        DaySpec::Exact(day) => i32::from(day),
        DaySpec::Last(weekday) => {
            let last_weekday = weekday_for_date(year, month, month_days);
            month_days - i32::from((last_weekday + 7 - weekday) % 7)
        }
        DaySpec::OnOrAfter { weekday, day } => {
            let day = i32::from(day);
            let actual = weekday_for_date(year, month, day);
            day + i32::from((weekday + 7 - actual) % 7)
        }
        DaySpec::OnOrBefore { weekday, day } => {
            let day = i32::from(day);
            let actual = weekday_for_date(year, month, day);
            day - i32::from((actual + 7 - weekday) % 7)
        }
    };
    // IANA deliberately permits expressions such as Sun>=25 in February;
    // the resolved day may spill by at most six days into an adjacent month.
    if day < -5 || day > month_days + 6 {
        return Err(format!(
            "invalid resolved day {day} for {year}-{month:02} from {spec:?}"
        ));
    }
    Ok(day)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn weekday_for_date(year: i32, month: u8, day: i32) -> u8 {
    (days_from_civil(i64::from(year), i64::from(month), i64::from(day)) + 4)
        .rem_euclid(7) as u8
}

fn civil_timestamp(
    year: i32,
    month: u8,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
) -> i64 {
    days_from_civil(i64::from(year), i64::from(month), i64::from(day)) * 86_400
        + i64::from(hour) * 3_600
        + i64::from(minute) * 60
        + i64::from(second)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100
        + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn encode_database(
    version: &str,
    source: &SourceDatabase,
    zones: &[CompiledZone],
) -> Result<Vec<u8>, String> {
    let zone_indices: HashMap<&str, usize> = zones
        .iter()
        .enumerate()
        .map(|(index, zone)| (zone.name.as_str(), index))
        .collect();

    let mut abbreviations = BTreeSet::new();
    for zone in zones {
        abbreviations.insert(zone.initial.abbreviation.clone());
        for transition in &zone.transitions {
            abbreviations.insert(transition.state.abbreviation.clone());
        }
    }
    let abbreviations: Vec<String> = abbreviations.into_iter().collect();
    let abbreviation_indices: HashMap<&str, usize> = abbreviations
        .iter()
        .enumerate()
        .map(|(index, value)| (value.as_str(), index))
        .collect();

    let mut identifiers = BTreeMap::new();
    for (index, zone) in zones.iter().enumerate() {
        identifiers.insert(zone.name.clone(), index);
    }
    for name in source.links.keys() {
        if identifiers.contains_key(name) {
            continue;
        }
        let target = resolve_link(name, &zone_indices, &source.links)?;
        let index = *zone_indices
            .get(target.as_str())
            .ok_or_else(|| format!("link {name} resolves to missing zone {target}"))?;
        identifiers.insert(name.clone(), index);
    }

    let mut output = Vec::new();
    output.extend_from_slice(MAGIC);
    put_string_u8(&mut output, version)?;
    put_u16(&mut output, abbreviations.len())?;
    for abbreviation in &abbreviations {
        put_string_u8(&mut output, abbreviation)?;
    }
    put_u16(&mut output, zones.len())?;
    for zone in zones {
        let mut states = vec![zone.initial.clone()];
        for transition in &zone.transitions {
            if !states.contains(&transition.state) {
                states.push(transition.state.clone());
            }
        }
        put_u8(&mut output, states.len())?;
        for state in &states {
            put_state(&mut output, state, &abbreviation_indices)?;
        }
        put_u32(&mut output, zone.transitions.len())?;
        let mut previous = None;
        for transition in &zone.transitions {
            if let Some(previous) = previous {
                let delta = transition
                    .timestamp
                    .checked_sub(previous)
                    .filter(|delta| *delta > 0)
                    .ok_or_else(|| format!("zone {} has unordered transitions", zone.name))?;
                put_var_u64(&mut output, delta as u64);
            } else {
                output.extend_from_slice(&transition.timestamp.to_le_bytes());
            }
            let state = states
                .iter()
                .position(|state| state == &transition.state)
                .expect("all transition states were collected");
            put_u8(&mut output, state)?;
            previous = Some(transition.timestamp);
        }
    }
    put_u16(&mut output, identifiers.len())?;
    for (name, zone) in identifiers {
        put_string_u16(&mut output, &name)?;
        put_u16(&mut output, zone)?;
    }
    Ok(output)
}

fn put_u8(output: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u8::try_from(value).map_err(|_| format!("value {value} exceeds u8"))?;
    output.push(value);
    Ok(())
}

fn put_var_u64(output: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn resolve_link(
    name: &str,
    zones: &HashMap<&str, usize>,
    links: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut current = name;
    let mut visited = HashSet::new();
    loop {
        if zones.contains_key(current) {
            return Ok(current.to_string());
        }
        if !visited.insert(current.to_string()) {
            return Err(format!("link cycle at {current}"));
        }
        current = links
            .get(current)
            .map(String::as_str)
            .ok_or_else(|| format!("link {name} has missing target {current}"))?;
    }
}

fn put_state(
    output: &mut Vec<u8>,
    state: &State,
    abbreviations: &HashMap<&str, usize>,
) -> Result<(), String> {
    output.extend_from_slice(&state.offset.to_le_bytes());
    let abbreviation = *abbreviations
        .get(state.abbreviation.as_str())
        .expect("all abbreviations were collected");
    put_u16(output, abbreviation)?;
    output.push(u8::from(state.is_dst));
    Ok(())
}

fn put_u16(output: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u16::try_from(value).map_err(|_| format!("value {value} exceeds u16"))?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_u32(output: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| format!("value {value} exceeds u32"))?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_string_u8(output: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let length = u8::try_from(value.len())
        .map_err(|_| format!("string length {} exceeds u8", value.len()))?;
    output.push(length);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_string_u16(output: &mut Vec<u8>, value: &str) -> Result<(), String> {
    put_u16(output, value.len())?;
    output.extend_from_slice(value.as_bytes());
    Ok(())
}
