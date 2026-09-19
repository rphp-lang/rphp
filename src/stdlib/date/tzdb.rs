//! RPHP's independently compiled IANA timezone transition database.
//!
//! The binary is generated from the public-domain IANA 2026a source tables by
//! `scripts/generate-tzdb.rs`.  No native timezone implementation or FFI is
//! used at runtime.

use std::sync::OnceLock;

const DATA: &[u8] = include_bytes!("../../../data/tzdata/2026a/rphp-tzdb.bin");
const MAGIC: &[u8; 8] = b"RPHTZ02\0";
const GENERATED_END_YEAR: i64 = 2500;
const FUTURE_CYCLE_START_YEAR: i64 = 2100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ZoneState {
    pub(super) offset: i32,
    pub(super) abbreviation: &'static str,
    pub(super) is_dst: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ZoneTransition {
    pub(super) timestamp: i64,
    pub(super) state: ZoneState,
}

#[derive(Clone, Copy, Debug)]
struct EncodedState {
    offset: i32,
    abbreviation: u16,
    is_dst: bool,
}

#[derive(Clone, Copy, Debug)]
struct Transition {
    timestamp: i64,
    state: EncodedState,
}

#[derive(Debug)]
struct Zone {
    initial: EncodedState,
    transitions: Vec<Transition>,
}

#[derive(Clone, Copy, Debug)]
struct Identifier {
    name: &'static str,
    zone: u16,
}

#[derive(Debug)]
struct Database {
    #[allow(dead_code)]
    version: &'static str,
    abbreviations: Vec<&'static str>,
    zones: Vec<Zone>,
    identifiers: Vec<Identifier>,
}

static DATABASE: OnceLock<Database> = OnceLock::new();

#[inline]
pub(super) fn contains(identifier: &str) -> bool {
    fixed_state(identifier).is_some() || lookup_zone(identifier).is_some()
}

pub(super) fn state_at(identifier: &str, timestamp: i64) -> Option<ZoneState> {
    if let Some(state) = fixed_state(identifier) {
        return Some(state);
    }
    let database = database();
    let zone = lookup_zone_in(database, identifier)?;
    let timestamp = canonical_transition_timestamp(timestamp);
    let transition = zone
        .transitions
        .partition_point(|transition| transition.timestamp <= timestamp);
    let state = transition
        .checked_sub(1)
        .map(|index| zone.transitions[index].state)
        .unwrap_or(zone.initial);
    Some(decode_state(database, state))
}

#[inline]
pub(super) fn fixed_state(identifier: &str) -> Option<ZoneState> {
    let abbreviation = match identifier {
        "UTC" | "Etc/UTC" => "UTC",
        "GMT" | "Etc/GMT" => "GMT",
        _ => return None,
    };
    Some(ZoneState {
        offset: 0,
        abbreviation,
        is_dst: false,
    })
}

pub(super) fn version() -> &'static str {
    database().version
}

#[cfg(test)]
pub(super) fn identifier_count() -> usize {
    database().identifiers.len()
}

pub(super) fn identifiers() -> impl Iterator<Item = &'static str> {
    database()
        .identifiers
        .iter()
        .map(|identifier| identifier.name)
}

pub(super) fn states(identifier: &str) -> Option<Vec<ZoneState>> {
    if let Some(state) = fixed_state(identifier) {
        return Some(vec![state]);
    }
    let database = database();
    let zone = lookup_zone_in(database, identifier)?;
    let mut states = Vec::with_capacity(zone.transitions.len().saturating_add(1));
    states.push(decode_state(database, zone.initial));
    for transition in &zone.transitions {
        let state = decode_state(database, transition.state);
        if !states.contains(&state) {
            states.push(state);
        }
    }
    Some(states)
}

/// Return PHP's transition projection for `[begin, end)`: the state active at
/// the left edge followed by every actual transition strictly inside it.
pub(super) fn transitions(identifier: &str, begin: i64, end: i64) -> Option<Vec<ZoneTransition>> {
    if begin >= end {
        return Some(Vec::new());
    }
    if let Some(state) = fixed_state(identifier) {
        return Some(vec![ZoneTransition {
            timestamp: begin,
            state,
        }]);
    }
    let database = database();
    let zone = lookup_zone_in(database, identifier)?;
    let active = zone
        .transitions
        .partition_point(|transition| transition.timestamp <= begin)
        .checked_sub(1)
        .map(|index| zone.transitions[index].state)
        .unwrap_or(zone.initial);
    let mut result = Vec::new();
    result.push(ZoneTransition {
        timestamp: begin,
        state: decode_state(database, active),
    });
    let first = zone
        .transitions
        .partition_point(|transition| transition.timestamp <= begin);
    for transition in &zone.transitions[first..] {
        if transition.timestamp >= end {
            break;
        }
        result.push(ZoneTransition {
            timestamp: transition.timestamp,
            state: decode_state(database, transition.state),
        });
    }
    Some(result)
}

fn lookup_zone(identifier: &str) -> Option<&'static Zone> {
    lookup_zone_in(database(), identifier)
}

fn lookup_zone_in<'a>(database: &'a Database, identifier: &str) -> Option<&'a Zone> {
    let index = database
        .identifiers
        .binary_search_by(|candidate| candidate.name.cmp(identifier))
        .ok()?;
    database
        .zones
        .get(usize::from(database.identifiers[index].zone))
}

fn decode_state(database: &Database, state: EncodedState) -> ZoneState {
    ZoneState {
        offset: state.offset,
        abbreviation: database.abbreviations[usize::from(state.abbreviation)],
        is_dst: state.is_dst,
    }
}

/// The final IANA rules are Gregorian-periodic.  The generator carries them
/// through 2500; timestamps after that are projected onto an equivalent year
/// in the complete 2100..=2499 cycle.  This retains weekday, leap-year and
/// UTC-boundary placement without growing the embedded transition table for an
/// unbounded PHP integer domain.
fn canonical_transition_timestamp(timestamp: i64) -> i64 {
    let (year, month, day, hour, minute, second, _, _) = super::super::unix_to_parts(timestamp);
    if year < GENERATED_END_YEAR {
        return timestamp;
    }
    let mapped_year = FUTURE_CYCLE_START_YEAR + (year - FUTURE_CYCLE_START_YEAR).rem_euclid(400);
    super::super::parts_to_unix(mapped_year, month, day, hour, minute, second)
}

fn database() -> &'static Database {
    DATABASE.get_or_init(|| decode_database(DATA).expect("embedded IANA timezone data is valid"))
}

fn decode_database(data: &'static [u8]) -> Result<Database, &'static str> {
    let mut cursor = Cursor::new(data);
    if cursor.take(MAGIC.len())? != MAGIC {
        return Err("invalid timezone database magic");
    }
    let version = cursor.string_u8()?;
    if version != "2026a" {
        return Err("unexpected timezone database version");
    }

    let abbreviation_count = usize::from(cursor.u16()?);
    let mut abbreviations = Vec::with_capacity(abbreviation_count);
    for _ in 0..abbreviation_count {
        abbreviations.push(cursor.string_u8()?);
    }

    let zone_count = usize::from(cursor.u16()?);
    let mut zones = Vec::with_capacity(zone_count);
    for _ in 0..zone_count {
        let state_count = usize::from(cursor.u8()?);
        if state_count == 0 {
            return Err("timezone zone has no states");
        }
        let mut states = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            states.push(cursor.state(abbreviation_count)?);
        }
        let initial = states[0];
        let transition_count =
            usize::try_from(cursor.u32()?).map_err(|_| "transition count does not fit usize")?;
        let mut transitions = Vec::with_capacity(transition_count);
        let mut previous = None;
        for index in 0..transition_count {
            let timestamp = if index == 0 {
                cursor.i64()?
            } else {
                let delta = i64::try_from(cursor.var_u64()?)
                    .map_err(|_| "timezone transition delta does not fit i64")?;
                previous
                    .and_then(|previous: i64| previous.checked_add(delta))
                    .ok_or("timezone transition timestamp overflow")?
            };
            if previous.is_some_and(|previous| previous >= timestamp) {
                return Err("timezone transitions are not strictly ordered");
            }
            previous = Some(timestamp);
            let state = states
                .get(usize::from(cursor.u8()?))
                .copied()
                .ok_or("timezone transition state is outside zone table")?;
            transitions.push(Transition { timestamp, state });
        }
        zones.push(Zone {
            initial,
            transitions,
        });
    }

    let identifier_count = usize::from(cursor.u16()?);
    let mut identifiers = Vec::with_capacity(identifier_count);
    let mut previous = None;
    for _ in 0..identifier_count {
        let name = cursor.string_u16()?;
        if previous.is_some_and(|previous: &str| previous >= name) {
            return Err("timezone identifiers are not strictly ordered");
        }
        previous = Some(name);
        let zone = cursor.u16()?;
        if usize::from(zone) >= zone_count {
            return Err("timezone identifier points outside zone table");
        }
        identifiers.push(Identifier { name, zone });
    }
    if !cursor.is_finished() {
        return Err("trailing timezone database bytes");
    }
    Ok(Database {
        version,
        abbreviations,
        zones,
        identifiers,
    })
}

struct Cursor {
    bytes: &'static [u8],
    offset: usize,
}

impl Cursor {
    fn new(bytes: &'static [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'static [u8], &'static str> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("timezone data offset overflow")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or("truncated timezone database")?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, &'static str> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("exact slice length"),
        ))
    }

    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("exact slice length"),
        ))
    }

    fn i32(&mut self) -> Result<i32, &'static str> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("exact slice length"),
        ))
    }

    fn i64(&mut self) -> Result<i64, &'static str> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().expect("exact slice length"),
        ))
    }

    fn var_u64(&mut self) -> Result<u64, &'static str> {
        let mut value = 0_u64;
        for shift in (0..=63).step_by(7) {
            let byte = self.u8()?;
            if shift == 63 && byte > 1 {
                return Err("timezone variable integer overflow");
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("timezone variable integer is too long")
    }

    fn string_u8(&mut self) -> Result<&'static str, &'static str> {
        let length = usize::from(self.u8()?);
        std::str::from_utf8(self.take(length)?).map_err(|_| "invalid timezone database UTF-8")
    }

    fn string_u16(&mut self) -> Result<&'static str, &'static str> {
        let length = usize::from(self.u16()?);
        std::str::from_utf8(self.take(length)?).map_err(|_| "invalid timezone database UTF-8")
    }

    fn state(&mut self, abbreviation_count: usize) -> Result<EncodedState, &'static str> {
        let offset = self.i32()?;
        let abbreviation = self.u16()?;
        if usize::from(abbreviation) >= abbreviation_count {
            return Err("timezone state points outside abbreviation table");
        }
        let is_dst = match self.u8()? {
            0 => false,
            1 => true,
            _ => return Err("invalid timezone daylight flag"),
        };
        Ok(EncodedState {
            offset,
            abbreviation,
            is_dst,
        })
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{contains, identifier_count, state_at, version};

    #[test]
    fn embedded_database_has_expected_release_and_inventory() {
        assert_eq!(version(), "2026a");
        assert_eq!(identifier_count(), 598);
        assert!(contains("UTC"));
        assert!(contains("America/New_York"));
        assert!(contains("Pacific/Chatham"));
        assert!(!contains("Europe/Not_A_Real_Place"));
    }

    #[test]
    fn representative_transition_states_are_available() {
        let winter = state_at("America/New_York", 1_704_067_200).unwrap();
        assert_eq!(
            (winter.offset, winter.abbreviation, winter.is_dst),
            (-18_000, "EST", false)
        );
        let summer = state_at("America/New_York", 1_719_792_000).unwrap();
        assert_eq!(
            (summer.offset, summer.abbreviation, summer.is_dst),
            (-14_400, "EDT", true)
        );
        let kathmandu = state_at("Asia/Kathmandu", 1_704_067_200).unwrap();
        assert_eq!(
            (kathmandu.offset, kathmandu.abbreviation),
            (20_700, "+0545")
        );
    }
}
