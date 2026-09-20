//! Unicode character properties for `\p{…}`, `\P{…}` and `\X`.
//!
//! General categories come from the generated `unicode_categories` runs.
//! Extended grapheme clusters follow the UAX #29 rules that matter for text
//! width and slicing: CR LF, Hangul syllable sequences, regional indicator
//! pairs, combining marks, and ZWJ emoji sequences.

use super::unicode_categories::{CATEGORY_NAMES, CATEGORY_RUNS};
use super::unicode_scripts;

const CATEGORY_COUNT: usize = CATEGORY_NAMES.len();
const UNASSIGNED: u8 = (CATEGORY_COUNT - 1) as u8;

/// Index of the General_Category of `c` in `CATEGORY_NAMES`.
pub(super) fn general_category(c: char) -> u8 {
    let code_point = u32::from(c);
    let index = CATEGORY_RUNS.partition_point(|(_, last, _)| *last < code_point);
    match CATEGORY_RUNS.get(index) {
        Some((first, _, category)) if *first <= code_point => *category,
        _ => UNASSIGNED,
    }
}

fn category_index(name: &str) -> Option<u8> {
    CATEGORY_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as u8)
}

fn mask_of_initial(initial: char) -> u32 {
    CATEGORY_NAMES
        .iter()
        .enumerate()
        .filter(|(_, name)| name.starts_with(initial))
        .fold(0, |mask, (index, _)| mask | (1 << index))
}

/// The categories one `\p{…}` name selects, plus PCRE's special classes
/// and Unicode scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PropertyClass {
    mask: u32,
    /// `Xwd` also admits the underscore; `Xsp`/`Xps` add the ASCII controls.
    extra: Extra,
    /// A `SCRIPT_NAMES` index when the name selects a script.
    script: Option<u16>,
    negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Extra {
    None,
    Underscore,
    AsciiSpace,
    RegionalIndicator,
    ExtendedPictographic,
}

/// PCRE2 matches property names loosely: case, spaces, hyphens and
/// underscores are ignored, so `Old_Italic`, `olditalic` and `OLD-ITALIC`
/// name the same script.
fn loose_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn script_index(loose: &str) -> Option<u16> {
    if let Some(index) = unicode_scripts::SCRIPT_NAMES
        .iter()
        .position(|name| *name == loose)
    {
        return u16::try_from(index).ok();
    }
    unicode_scripts::SCRIPT_ALIASES
        .binary_search_by(|(alias, _)| (*alias).cmp(loose))
        .ok()
        .map(|position| unicode_scripts::SCRIPT_ALIASES[position].1)
}

/// Script of `c`, or `UNKNOWN_SCRIPT` for code points outside every run.
fn script_of(c: char) -> u16 {
    let code_point = c as u32;
    let runs = unicode_scripts::SCRIPT_RUNS;
    let position = runs.partition_point(|(first, _, _)| *first <= code_point);
    match position.checked_sub(1).map(|index| runs[index]) {
        Some((_, last, script)) if code_point <= last => script,
        _ => unicode_scripts::UNKNOWN_SCRIPT,
    }
}

impl PropertyClass {
    /// Parse the text between `\p{` and `}` (or a single-letter form).
    pub(super) fn parse(name: &str, negated: bool) -> Option<Self> {
        let (name, inverted) = match name.strip_prefix('^') {
            Some(rest) => (rest, true),
            None => (name, false),
        };
        let negated = negated != inverted;
        let loose = loose_name(name);
        let mut script = None;
        let (mask, extra) = match loose.as_str() {
            "any" => (u32::MAX, Extra::None),
            "l&" => (
                (1 << category_index("Lu")?)
                    | (1 << category_index("Ll")?)
                    | (1 << category_index("Lt")?),
                Extra::None,
            ),
            "xan" => (mask_of_initial('L') | mask_of_initial('N'), Extra::None),
            "xwd" => (
                mask_of_initial('L') | mask_of_initial('N'),
                Extra::Underscore,
            ),
            "xsp" | "xps" => (mask_of_initial('Z'), Extra::AsciiSpace),
            "regionalindicator" | "ri" => (0, Extra::RegionalIndicator),
            "extendedpictographic" => (0, Extra::ExtendedPictographic),
            _ if loose.len() == 1 => {
                let mask = mask_of_initial(loose.chars().next()?.to_ascii_uppercase());
                if mask == 0 {
                    return None;
                }
                (mask, Extra::None)
            }
            _ => match loose_category_index(&loose) {
                Some(index) => (1 << index, Extra::None),
                None => {
                    script = Some(script_index(&loose)?);
                    (0, Extra::None)
                }
            },
        };
        Some(Self {
            mask,
            extra,
            script,
            negated,
        })
    }

    pub(super) fn matches(self, c: char) -> bool {
        let category = general_category(c);
        let extra = match self.extra {
            Extra::None => false,
            Extra::Underscore => c == '_',
            Extra::AsciiSpace => matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'),
            Extra::RegionalIndicator => is_regional_indicator(c),
            Extra::ExtendedPictographic => is_extended_pictographic(c),
        };
        let script = self.script.is_some_and(|script| script_of(c) == script);
        (self.mask & (1 << category) != 0 || extra || script) != self.negated
    }
}

/// `category_index` for a loose-form name such as `lu`.
fn loose_category_index(loose: &str) -> Option<u8> {
    CATEGORY_NAMES
        .iter()
        .position(|name: &&str| name.eq_ignore_ascii_case(loose))
        .and_then(|index| u8::try_from(index).ok())
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

fn is_extend(c: char) -> bool {
    matches!(
        CATEGORY_NAMES[usize::from(general_category(c))],
        "Mn" | "Me" | "Mc"
    ) || c == '\u{200C}'
        || c == '\u{200D}'
}

fn is_control(c: char) -> bool {
    matches!(
        CATEGORY_NAMES[usize::from(general_category(c))],
        "Cc" | "Zl" | "Zp"
    ) || (CATEGORY_NAMES[usize::from(general_category(c))] == "Cf"
        && !matches!(c, '\u{200C}' | '\u{200D}'))
}

fn is_extended_pictographic(c: char) -> bool {
    matches!(
        u32::from(c),
        0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x2199 | 0x21A9..=0x21AA
            | 0x231A..=0x231B | 0x2328 | 0x23CF | 0x23E9..=0x23F3 | 0x23F8..=0x23FA
            | 0x24C2 | 0x25AA..=0x25AB | 0x25B6 | 0x25C0 | 0x25FB..=0x25FE
            | 0x2600..=0x27BF | 0x2934..=0x2935 | 0x2B05..=0x2B07 | 0x2B1B..=0x2B1C
            | 0x2B50 | 0x2B55 | 0x3030 | 0x303D | 0x3297 | 0x3299
            | 0x1F000..=0x1FAFF | 0x1FC00..=0x1FFFD
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hangul {
    L,
    V,
    T,
    Lv,
    Lvt,
    None,
}

fn hangul_kind(c: char) -> Hangul {
    match u32::from(c) {
        0x1100..=0x115F | 0xA960..=0xA97C => Hangul::L,
        0x1160..=0x11A7 | 0xD7B0..=0xD7C6 => Hangul::V,
        0x11A8..=0x11FF | 0xD7CB..=0xD7FB => Hangul::T,
        0xAC00..=0xD7A3 => {
            if (u32::from(c) - 0xAC00) % 28 == 0 {
                Hangul::Lv
            } else {
                Hangul::Lvt
            }
        }
        _ => Hangul::None,
    }
}

/// Length in characters of the extended grapheme cluster starting at `pos`.
pub(super) fn grapheme_cluster_len(chars: &[char], pos: usize) -> Option<usize> {
    let first = *chars.get(pos)?;
    if first == '\r' {
        return Some(if chars.get(pos + 1) == Some(&'\n') {
            2
        } else {
            1
        });
    }
    if is_control(first) {
        return Some(1);
    }
    let mut index = pos + 1;
    if is_regional_indicator(first) && chars.get(index).is_some_and(|c| is_regional_indicator(*c)) {
        index += 1;
    } else {
        let mut previous = hangul_kind(first);
        while let Some(&next) = chars.get(index) {
            let kind = hangul_kind(next);
            let joins = match (previous, kind) {
                (Hangul::L, Hangul::L | Hangul::V | Hangul::Lv | Hangul::Lvt) => true,
                (Hangul::Lv | Hangul::V, Hangul::V | Hangul::T) => true,
                (Hangul::Lvt | Hangul::T, Hangul::T) => true,
                _ => false,
            };
            if !joins {
                break;
            }
            previous = kind;
            index += 1;
        }
    }
    while let Some(&next) = chars.get(index) {
        if is_extend(next) {
            index += 1;
            if next == '\u{200D}'
                && chars
                    .get(index)
                    .is_some_and(|c| is_extended_pictographic(*c))
            {
                index += 1;
            }
            continue;
        }
        break;
    }
    Some(index - pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_the_generated_runs() {
        let name = |c: char| CATEGORY_NAMES[usize::from(general_category(c))];
        assert_eq!(name('a'), "Ll");
        assert_eq!(name('A'), "Lu");
        assert_eq!(name('5'), "Nd");
        assert_eq!(name(' '), "Zs");
        assert_eq!(name('\u{301}'), "Mn");
        assert_eq!(name('\u{200D}'), "Cf");
        assert_eq!(name('\u{1F1E8}'), "So");
        assert_eq!(name('\u{E000}'), "Co");
        assert_eq!(name('\u{10FFFF}'), "Cn");
        assert_eq!(name('\u{378}'), "Cn");
    }

    #[test]
    fn property_classes_select_categories_and_specials() {
        let class = |name: &str, negated: bool| PropertyClass::parse(name, negated).unwrap();
        assert!(class("L", false).matches('é'));
        assert!(!class("L", false).matches('1'));
        assert!(class("L", true).matches('1'));
        assert!(class("^L", false).matches('1'));
        assert!(!class("^L", true).matches('1'));
        assert!(class("Mn", false).matches('\u{301}'));
        assert!(class("L&", false).matches('a') && !class("L&", false).matches('ª'));
        assert!(class("Xwd", false).matches('_') && !class("Xan", false).matches('_'));
        assert!(class("Xsp", false).matches('\t') && class("Xsp", false).matches('\u{3000}'));
        assert!(class("Any", false).matches('\u{378}'));
        assert!(class("Regional_Indicator", false).matches('\u{1F1E8}'));
        assert!(!class("RI", false).matches('A'));
        assert!(PropertyClass::parse("Nope", false).is_none());
        assert!(PropertyClass::parse("Q", false).is_none());
    }

    #[test]
    fn grapheme_clusters_join_marks_hangul_flags_and_emoji_sequences() {
        let len = |text: &str| {
            let chars: Vec<char> = text.chars().collect();
            grapheme_cluster_len(&chars, 0).unwrap()
        };
        assert_eq!(len("e\u{301}x"), 2);
        assert_eq!(len("\r\nx"), 2);
        assert_eq!(len("\n\u{301}"), 1);
        assert_eq!(len("\u{1F1E8}\u{1F1FF}\u{1F1E8}"), 2);
        assert_eq!(len("\u{1100}\u{1161}\u{11A8}x"), 3);
        assert_eq!(len("\u{AC00}\u{11A8}"), 2);
        assert_eq!(len("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}!"), 5);
        assert_eq!(len("a\u{FE0F}b"), 2);
        assert_eq!(len("x"), 1);
        assert!(grapheme_cluster_len(&[], 0).is_none());
    }
}
