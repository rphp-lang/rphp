//! Unicode character properties for `\p{…}`, `\P{…}` and `\X`.
//!
//! General categories come from the generated `unicode_categories` runs.
//! Extended grapheme clusters follow the UAX #29 rules that matter for text
//! width and slicing: CR LF, Hangul syllable sequences, regional indicator
//! pairs, combining marks, and ZWJ emoji sequences.

use super::unicode_bidi_classes::{BIDI_CLASS_NAMES, BIDI_CLASS_RANGES};
use super::unicode_binary_properties::{
    BINARY_PROPERTY_ALIASES, BINARY_PROPERTY_NAMES, BINARY_PROPERTY_RANGES,
    EXTENDED_PICTOGRAPHIC_INDEX,
};
use super::unicode_case_folding::SIMPLE_CASE_FOLD;
use super::unicode_categories::{CATEGORY_NAMES, CATEGORY_RUNS};
use super::unicode_grapheme_breaks::{GRAPHEME_BREAK_NAMES, GRAPHEME_BREAK_RANGES};
use super::unicode_script_extensions::SCRIPT_EXTENSION_RUNS;
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

pub(super) fn category_is(c: char, name: &str) -> bool {
    CATEGORY_NAMES[usize::from(general_category(c))] == name
}

pub(super) fn category_has_initial(c: char, initial: char) -> bool {
    CATEGORY_NAMES[usize::from(general_category(c))].starts_with(initial)
}

/// PCRE2 uses Unicode simple case equivalence whenever UTF or UCP is active.
/// Without either option its default C-locale tables fold ASCII only.
pub(super) fn caseless_equal(left: char, right: char, unicode: bool) -> bool {
    if left == right {
        return true;
    }
    if !unicode {
        return left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(&right);
    }
    simple_case_fold(left) == simple_case_fold(right)
}

pub(super) fn simple_case_fold(character: char) -> char {
    let code_point = u32::from(character);
    SIMPLE_CASE_FOLD
        .binary_search_by_key(&code_point, |(source, _)| *source)
        .ok()
        .and_then(|index| char::from_u32(SIMPLE_CASE_FOLD[index].1))
        .unwrap_or(character)
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
    script_extensions: bool,
    binary: Option<u8>,
    bidi_class: Option<u8>,
    negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Extra {
    None,
    Underscore,
    AsciiSpace,
    RegionalIndicator,
    ExtendedPictographic,
    UniversalCharacterName,
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

fn binary_property_index(loose: &str) -> Option<u8> {
    if let Some(index) = BINARY_PROPERTY_NAMES.iter().position(|name| *name == loose) {
        return u8::try_from(index).ok();
    }
    BINARY_PROPERTY_ALIASES
        .binary_search_by(|(alias, _)| (*alias).cmp(loose))
        .ok()
        .map(|position| BINARY_PROPERTY_ALIASES[position].1)
}

fn binary_property_matches(index: u8, c: char) -> bool {
    let code_point = u32::from(c);
    let ranges = BINARY_PROPERTY_RANGES[usize::from(index)];
    let position = ranges.partition_point(|(first, _)| *first <= code_point);
    position
        .checked_sub(1)
        .is_some_and(|position| code_point <= ranges[position].1)
}

fn bidi_class_index(loose: &str) -> Option<u8> {
    BIDI_CLASS_NAMES
        .binary_search(&loose)
        .ok()
        .and_then(|index| u8::try_from(index).ok())
}

fn bidi_class_matches(index: u8, c: char) -> bool {
    let code_point = u32::from(c);
    let ranges = BIDI_CLASS_RANGES[usize::from(index)];
    let position = ranges.partition_point(|(first, _)| *first <= code_point);
    position
        .checked_sub(1)
        .is_some_and(|position| code_point <= ranges[position].1)
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

pub(super) fn script_name(c: char) -> &'static str {
    unicode_scripts::SCRIPT_NAMES[usize::from(script_of(c))]
}

impl PropertyClass {
    /// Parse the text between `\p{` and `}` (or a single-letter form).
    pub(super) fn parse(name: &str, negated: bool) -> Option<Self> {
        let (name, inverted) = match name.strip_prefix('^') {
            Some(rest) => (rest, true),
            None => (name, false),
        };
        let negated = negated != inverted;
        let (property_type, property_name) = name
            .split_once([':', '='])
            .map_or((None, name), |(kind, value)| {
                (Some(loose_name(kind)), value)
            });
        let loose = loose_name(property_name);
        let mut script = None;
        let mut script_extensions = false;
        let mut binary = None;
        let mut bidi_class = None;
        let (mask, extra) = if let Some(property_type) = property_type {
            match property_type.as_str() {
                "sc" | "script" => {
                    script = Some(script_index(&loose)?);
                    (0, Extra::None)
                }
                "scx" | "scriptextensions" => {
                    script = Some(script_index(&loose)?);
                    script_extensions = true;
                    (0, Extra::None)
                }
                "bc" | "bidiclass" => {
                    bidi_class = Some(bidi_class_index(&loose)?);
                    (0, Extra::None)
                }
                _ => return None,
            }
        } else {
            match loose.as_str() {
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
                "xuc" => (0, Extra::UniversalCharacterName),
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
                        if let Some(index) = binary_property_index(&loose) {
                            binary = Some(index);
                        } else {
                            script = Some(script_index(&loose)?);
                            script_extensions = true;
                        }
                        (0, Extra::None)
                    }
                },
            }
        };
        Some(Self {
            mask,
            extra,
            script,
            script_extensions,
            binary,
            bidi_class,
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
            Extra::UniversalCharacterName => matches!(c, '$' | '@' | '`') || c >= '\u{a0}',
        };
        let script = self.script.is_some_and(|script| {
            if self.script_extensions {
                script_has_extension(c, script)
            } else {
                script_of(c) == script
            }
        });
        let binary = self
            .binary
            .is_some_and(|index| binary_property_matches(index, c));
        let bidi_class = self
            .bidi_class
            .is_some_and(|index| bidi_class_matches(index, c));
        (self.mask & (1 << category) != 0 || extra || script || binary || bidi_class)
            != self.negated
    }
}

pub(super) fn has_explicit_script_extensions(c: char) -> bool {
    let code_point = u32::from(c);
    let position = SCRIPT_EXTENSION_RUNS.partition_point(|(first, _, _)| *first <= code_point);
    position
        .checked_sub(1)
        .is_some_and(|position| code_point <= SCRIPT_EXTENSION_RUNS[position].1)
}

pub(super) fn script_has_extension(c: char, script: u16) -> bool {
    let code_point = u32::from(c);
    let position = SCRIPT_EXTENSION_RUNS.partition_point(|(first, _, _)| *first <= code_point);
    match position
        .checked_sub(1)
        .map(|index| SCRIPT_EXTENSION_RUNS[index])
    {
        Some((_, last, scripts)) if code_point <= last => scripts.contains(&script),
        _ => script_of(c) == script,
    }
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// `category_index` for a loose-form name such as `lu`.
fn loose_category_index(loose: &str) -> Option<u8> {
    CATEGORY_NAMES
        .iter()
        .position(|name: &&str| name.eq_ignore_ascii_case(loose))
        .and_then(|index| u8::try_from(index).ok())
}

fn is_extended_pictographic(c: char) -> bool {
    binary_property_matches(EXTENDED_PICTOGRAPHIC_INDEX, c)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GraphemeBreak {
    Cr,
    Control,
    Extend,
    L,
    Lf,
    Lv,
    Lvt,
    Prepend,
    RegionalIndicator,
    SpacingMark,
    T,
    V,
    Zwj,
    Other,
}

fn grapheme_break(character: char) -> GraphemeBreak {
    let code_point = u32::from(character);
    let position = GRAPHEME_BREAK_RANGES.partition_point(|(first, _, _)| *first <= code_point);
    let Some((_, last, class)) = position
        .checked_sub(1)
        .map(|index| GRAPHEME_BREAK_RANGES[index])
    else {
        return GraphemeBreak::Other;
    };
    if code_point > last {
        return GraphemeBreak::Other;
    }
    match GRAPHEME_BREAK_NAMES[usize::from(class)] {
        "CR" => GraphemeBreak::Cr,
        "Control" => GraphemeBreak::Control,
        "Extend" => GraphemeBreak::Extend,
        "L" => GraphemeBreak::L,
        "LF" => GraphemeBreak::Lf,
        "LV" => GraphemeBreak::Lv,
        "LVT" => GraphemeBreak::Lvt,
        "Prepend" => GraphemeBreak::Prepend,
        "Regional_Indicator" => GraphemeBreak::RegionalIndicator,
        "SpacingMark" => GraphemeBreak::SpacingMark,
        "T" => GraphemeBreak::T,
        "V" => GraphemeBreak::V,
        "ZWJ" => GraphemeBreak::Zwj,
        _ => GraphemeBreak::Other,
    }
}

fn joins_grapheme(chars: &[char], start: usize, boundary: usize) -> bool {
    let previous = grapheme_break(chars[boundary - 1]);
    let next = grapheme_break(chars[boundary]);
    if previous == GraphemeBreak::Cr && next == GraphemeBreak::Lf {
        return true;
    }
    if matches!(
        previous,
        GraphemeBreak::Cr | GraphemeBreak::Lf | GraphemeBreak::Control
    ) || matches!(
        next,
        GraphemeBreak::Cr | GraphemeBreak::Lf | GraphemeBreak::Control
    ) {
        return false;
    }
    if matches!(previous, GraphemeBreak::L)
        && matches!(
            next,
            GraphemeBreak::L | GraphemeBreak::V | GraphemeBreak::Lv | GraphemeBreak::Lvt
        )
    {
        return true;
    }
    if matches!(previous, GraphemeBreak::Lv | GraphemeBreak::V)
        && matches!(next, GraphemeBreak::V | GraphemeBreak::T)
    {
        return true;
    }
    if matches!(previous, GraphemeBreak::Lvt | GraphemeBreak::T) && next == GraphemeBreak::T {
        return true;
    }
    if matches!(
        next,
        GraphemeBreak::Extend | GraphemeBreak::Zwj | GraphemeBreak::SpacingMark
    ) || previous == GraphemeBreak::Prepend
    {
        return true;
    }
    if previous == GraphemeBreak::Zwj && is_extended_pictographic(chars[boundary]) {
        let mut cursor = boundary - 1;
        while cursor > start && grapheme_break(chars[cursor - 1]) == GraphemeBreak::Extend {
            cursor -= 1;
        }
        if cursor > start && is_extended_pictographic(chars[cursor - 1]) {
            return true;
        }
    }
    if previous == GraphemeBreak::RegionalIndicator && next == GraphemeBreak::RegionalIndicator {
        let preceding = chars[start..boundary]
            .iter()
            .rev()
            .take_while(|character| grapheme_break(**character) == GraphemeBreak::RegionalIndicator)
            .count();
        return preceding % 2 == 1;
    }
    false
}

/// Length in characters of the extended grapheme cluster starting at `pos`.
pub(super) fn grapheme_cluster_len(chars: &[char], pos: usize) -> Option<usize> {
    chars.get(pos)?;
    let mut index = pos + 1;
    while index < chars.len() && joins_grapheme(chars, pos, index) {
        index += 1;
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
