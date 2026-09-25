//! Repository-owned PCRE2 10.42-compatible backtracking regex engine.
//!
//! This is the native engine behind RPHP's PHP 8.5 `preg_*` surface. It owns
//! parsing, Unicode tables, matching, capture/backtracking state and execution
//! limits; it does not link to, invoke or use FFI to an external PCRE library.
//! The interpreter deliberately exposes the public no-JIT PCRE contract rather
//! than claiming PCRE2's machine-code JIT implementation.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

mod linear;
mod unicode;
mod unicode_bidi_classes;
mod unicode_binary_properties;
mod unicode_case_folding;
mod unicode_categories;
mod unicode_grapheme_breaks;
mod unicode_script_extensions;
mod unicode_scripts;

/// Keep the cache bounded so scripts generating regexes dynamically cannot
/// retain an unbounded amount of compiled AST data for the lifetime of the
/// executor. The cache is shared by every preg_* function in that executor.
pub const DEFAULT_REGEX_CACHE_CAPACITY: usize = 1024;

// ── AST ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Literal(char),
    AnyChar, // .
    /// `\C`: one 8-bit code unit in PHP's ordinary non-UTF byte view.
    ByteUnit,
    /// `\N` without a braced code point: any character except a newline,
    /// independently of dotall mode.
    NotNewline,
    Anchor(Anchor),
    CharClass {
        negated: bool,
        items: Vec<ClassItem>,
    },
    Shorthand(Shorthand),
    Group {
        index: Option<usize>,
        #[allow(dead_code)]
        name: Option<String>,
        inner: Box<Node>,
    },
    Alternation(Vec<Node>),
    Sequence(Vec<Node>),
    Quantifier {
        inner: Box<Node>,
        min: usize,
        max: Option<usize>,
        greedy: bool,
        possessive: bool,
    },
    Backreference(usize),
    NamedBackreference(String),
    Lookahead {
        positive: bool,
        atomic: bool,
        inner: Box<Node>,
    },
    Lookbehind {
        positive: bool,
        atomic: bool,
        inner: Box<Node>,
    },
    /// Atomic group `(?>...)`: the selected inner path cannot be revisited
    /// when a later node fails.
    Atomic(Box<Node>),
    /// PCRE2 script-run group. The inner expression matches normally, then
    /// the consumed Unicode span is checked as one UTS #39 script run.
    ScriptRun {
        inner: Box<Node>,
        atomic: bool,
    },
    /// PCRE `(*MARK:name)` / `(*:name)`: publish the last successful mark as
    /// the synthetic named capture `MARK` without consuming input.
    Mark(String),
    /// PCRE backtracking control verbs. These are zero-width on their
    /// preferred path and take effect only when matching later input fails.
    Control {
        verb: ControlVerb,
        name: Option<String>,
    },
    /// `\K`: preserve consumed input while resetting the reported start of
    /// the overall match to the current engine position.
    ResetStart,
    WordBoundary(bool), // true = \b, false = \B
    /// `\X`: one extended grapheme cluster.
    GraphemeCluster,
    /// `\R`: one newline sequence, with `\r\n` consumed atomically.
    Linebreak,
    /// A deterministic atom compiled under local `(?ims)` settings. Only
    /// leaf nodes are wrapped, so a reset inside a nested group cannot be
    /// accidentally shadowed by an enclosing option scope.
    LocalFlags {
        flags: LocalMatchFlags,
        inner: Box<Node>,
    },
    /// Parser-only representation for `\Q...\E`. It is lowered to literal
    /// atoms before the completed AST reaches the matcher.
    Quoted(Vec<char>),
    /// Recursive/subroutine call resolved against the immutable compiled
    /// pattern registry. Keeping the reference symbolic permits self and
    /// forward references without constructing a cyclic Rust AST.
    Subroutine(SubroutineTarget),
    /// PCRE conditional selected from capture participation or recursive
    /// subroutine state.
    Conditional {
        condition: CaptureCondition,
        yes: Box<Node>,
        no: Option<Box<Node>>,
    },
    /// Runtime-only zero-width continuation inserted after a capturing group.
    /// It publishes the group end before the following AST node is evaluated,
    /// so ordinary continuation matching can drive backtracking without first
    /// materializing every possible inner state.
    CaptureEnd {
        index: usize,
        start: usize,
    },
    /// Runtime-only zero-width continuation inserted after a subroutine.
    /// Captures made by the called group participate while it is matching,
    /// but PCRE restores the caller's capture registers before evaluating the
    /// nodes that follow the call.
    CaptureRestore {
        groups: Rc<Vec<Option<Match>>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SubroutineTarget {
    WholePattern,
    Group(usize),
    Name(String),
}

#[derive(Debug, Clone)]
enum CaptureCondition {
    Group(usize),
    Name(String),
    RecursionTarget(SubroutineTarget),
    AmbiguousRecursion {
        name: String,
        target: Option<SubroutineTarget>,
    },
    Assertion {
        positive: bool,
        lookbehind: bool,
        inner: Box<Node>,
    },
    Always(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlVerb {
    Fail,
    Accept,
    Commit,
    Prune,
    Skip,
    Then,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchControl {
    Accept,
    Commit,
    Prune,
    Skip(usize),
    Then,
}

#[inline]
fn next_attempt_after_control(control: Option<MatchControl>, start: usize) -> Option<usize> {
    match control {
        Some(MatchControl::Commit) | Some(MatchControl::Accept) => None,
        Some(MatchControl::Skip(position)) if position > start => Some(position),
        Some(MatchControl::Prune | MatchControl::Skip(_) | MatchControl::Then) | None => {
            start.checked_add(1)
        }
    }
}

#[derive(Debug, Clone)]
enum Anchor {
    Start,         // ^
    End,           // $
    AbsoluteStart, // \A
    AbsoluteEnd,   // \z
    FinalEnd,      // \Z
    SearchStart,   // \G
}

#[derive(Debug, Clone)]
enum ClassItem {
    Literal(char),
    Range(char, char),
    Shorthand(Shorthand),
    Posix { class: PosixClass, negated: bool },
}

#[derive(Debug, Clone, Copy)]
enum PosixClass {
    Alnum,
    Alpha,
    Ascii,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Word,
    Xdigit,
}

impl PosixClass {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "alnum" => Self::Alnum,
            "alpha" => Self::Alpha,
            "ascii" => Self::Ascii,
            "blank" => Self::Blank,
            "cntrl" => Self::Cntrl,
            "digit" => Self::Digit,
            "graph" => Self::Graph,
            "lower" => Self::Lower,
            "print" => Self::Print,
            "punct" => Self::Punct,
            "space" => Self::Space,
            "upper" => Self::Upper,
            "word" => Self::Word,
            "xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn matches(self, c: char, unicode: bool) -> bool {
        let is_letter = || unicode::category_has_initial(c, 'L');
        let is_number = || unicode::category_has_initial(c, 'N');
        let is_decimal = || unicode::category_is(c, "Nd");
        let is_space = || {
            if unicode {
                c.is_whitespace()
            } else {
                c.is_ascii_whitespace()
            }
        };
        let is_blank = || {
            if unicode {
                c == '\t' || unicode::category_is(c, "Zs")
            } else {
                matches!(c, '\t' | ' ')
            }
        };
        let is_control = || {
            if unicode {
                unicode::category_is(c, "Cc")
            } else {
                c.is_ascii_control()
            }
        };
        match self {
            Self::Alnum => {
                if unicode {
                    is_letter() || is_number()
                } else {
                    c.is_ascii_alphanumeric()
                }
            }
            Self::Alpha => {
                if unicode {
                    is_letter()
                } else {
                    c.is_ascii_alphabetic()
                }
            }
            Self::Ascii => c.is_ascii(),
            Self::Blank => is_blank(),
            Self::Cntrl => is_control(),
            Self::Digit => {
                if unicode {
                    is_decimal()
                } else {
                    c.is_ascii_digit()
                }
            }
            Self::Graph => !is_space() && !is_control(),
            Self::Lower => {
                if unicode {
                    unicode::category_is(c, "Ll")
                } else {
                    c.is_ascii_lowercase()
                }
            }
            Self::Print => (!is_space() && !is_control()) || is_blank(),
            Self::Punct => {
                if unicode {
                    unicode::category_has_initial(c, 'P')
                } else {
                    c.is_ascii_punctuation()
                }
            }
            Self::Space => is_space(),
            Self::Upper => {
                if unicode {
                    unicode::category_is(c, "Lu")
                } else {
                    c.is_ascii_uppercase()
                }
            }
            Self::Word => {
                if unicode {
                    is_letter() || is_number() || c == '_'
                } else {
                    c.is_ascii_alphanumeric() || c == '_'
                }
            }
            Self::Xdigit => c.is_ascii_hexdigit(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Shorthand {
    Digit,              // \d
    NonDigit,           // \D
    Word,               // \w
    NonWord,            // \W
    Space,              // \s
    NonSpace,           // \S
    HorizontalSpace,    // \h
    NonHorizontalSpace, // \H
    VerticalSpace,      // \v
    NonVerticalSpace,   // \V
    /// `\p{…}` / `\P{…}` Unicode property class.
    Property(unicode::PropertyClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalMatchFlags {
    case_insensitive: bool,
    multiline: bool,
    dotall: bool,
}

impl From<RegexFlags> for LocalMatchFlags {
    fn from(flags: RegexFlags) -> Self {
        Self {
            case_insensitive: flags.case_insensitive,
            multiline: flags.multiline,
            dotall: flags.dotall,
        }
    }
}

// ── Flags ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct RegexFlags {
    pub case_insensitive: bool,
    pub multiline: bool,
    pub dotall: bool,
    pub extended: bool,
    pub ungreedy: bool,
    line_options: LineOptions,
    /// PHP's `/u` enables both PCRE2_UTF and PCRE2_UCP, while leading
    /// `(*UTF)` and `(*UCP)` can select either capability independently.
    unicode_mode: UnicodeMode,
    /// PCRE `A` modifier: a match may only start at the search position, and
    /// each further match must begin where the previous one ended.
    pub anchored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum UnicodeMode {
    None,
    Ucp,
    Utf,
    UtfUcp,
}

impl UnicodeMode {
    fn utf(self) -> bool {
        matches!(self, Self::Utf | Self::UtfUcp)
    }

    fn ucp(self) -> bool {
        matches!(self, Self::Ucp | Self::UtfUcp)
    }

    fn with_utf(self) -> Self {
        if self.ucp() { Self::UtfUcp } else { Self::Utf }
    }

    fn with_ucp(self) -> Self {
        if self.utf() { Self::UtfUcp } else { Self::Ucp }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineOptions(u8);

impl LineOptions {
    const DOLLAR_END_ONLY: u8 = 1;
    const BSR_ANYCRLF: u8 = 1 << 1;
    const NEWLINE_SHIFT: u8 = 2;
    const NEWLINE_MASK: u8 = 0b111 << Self::NEWLINE_SHIFT;

    fn dollar_end_only(self) -> bool {
        self.0 & Self::DOLLAR_END_ONLY != 0
    }

    fn bsr_anycrlf(self) -> bool {
        self.0 & Self::BSR_ANYCRLF != 0
    }

    fn newline(self) -> NewlineConvention {
        match (self.0 & Self::NEWLINE_MASK) >> Self::NEWLINE_SHIFT {
            1 => NewlineConvention::Cr,
            2 => NewlineConvention::CrLf,
            3 => NewlineConvention::AnyCrLf,
            4 => NewlineConvention::Any,
            5 => NewlineConvention::Nul,
            _ => NewlineConvention::Lf,
        }
    }

    fn set_newline(&mut self, newline: NewlineConvention) {
        let value = match newline {
            NewlineConvention::Lf => 0,
            NewlineConvention::Cr => 1,
            NewlineConvention::CrLf => 2,
            NewlineConvention::AnyCrLf => 3,
            NewlineConvention::Any => 4,
            NewlineConvention::Nul => 5,
        };
        self.0 = (self.0 & !Self::NEWLINE_MASK) | (value << Self::NEWLINE_SHIFT);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewlineConvention {
    Lf,
    Cr,
    CrLf,
    AnyCrLf,
    Any,
    Nul,
}

/// Request-scoped execution limits applied by the public `preg_*` boundary.
/// The custom engine keeps these independent from parsing so one cached AST
/// can be reused after an `ini_set()` changes a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatchLimits {
    pub backtrack: usize,
    pub recursion: usize,
    /// Matcher heap-frame allowance. `usize::MAX` is the ordinary PHP
    /// context; a leading `(*LIMIT_HEAP=n)` clamps it for this pattern.
    pub heap_frames: usize,
    pub jit: bool,
}

impl Default for MatchLimits {
    fn default() -> Self {
        Self {
            backtrack: 1_000_000,
            recursion: 100_000,
            heap_frames: usize::MAX,
            jit: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchLimitError {
    Backtrack,
    Recursion,
    Heap,
    JitStack,
}

impl Default for RegexFlags {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            multiline: false,
            dotall: false,
            extended: false,
            ungreedy: false,
            line_options: LineOptions(0),
            unicode_mode: UnicodeMode::None,
            anchored: false,
        }
    }
}

#[inline]
fn end_anchor_matches(pos: usize, chars: &[char], flags: RegexFlags) -> bool {
    if pos == chars.len() {
        return true;
    }
    if flags.multiline {
        return newline_len_at(chars, pos, flags.line_options.newline()).is_some();
    }
    if flags.line_options.dollar_end_only() {
        return false;
    }
    newline_len_at(chars, pos, flags.line_options.newline())
        .is_some_and(|length| pos + length == chars.len())
}

#[inline]
fn final_end_anchor_matches(pos: usize, chars: &[char], flags: RegexFlags) -> bool {
    pos == chars.len()
        || newline_len_at(chars, pos, flags.line_options.newline())
            .is_some_and(|length| pos + length == chars.len())
}

fn newline_len_at(chars: &[char], pos: usize, convention: NewlineConvention) -> Option<usize> {
    let first = *chars.get(pos)?;
    match convention {
        NewlineConvention::Lf => (first == '\n').then_some(1),
        NewlineConvention::Cr => (first == '\r').then_some(1),
        NewlineConvention::CrLf => {
            (first == '\r' && chars.get(pos + 1) == Some(&'\n')).then_some(2)
        }
        NewlineConvention::AnyCrLf => {
            if first == '\r' && chars.get(pos + 1) == Some(&'\n') {
                Some(2)
            } else {
                matches!(first, '\r' | '\n').then_some(1)
            }
        }
        NewlineConvention::Any => {
            if first == '\r' && chars.get(pos + 1) == Some(&'\n') {
                Some(2)
            } else {
                matches!(
                    first,
                    '\n' | '\u{0b}' | '\u{0c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
                )
                .then_some(1)
            }
        }
        NewlineConvention::Nul => (first == '\0').then_some(1),
    }
}

fn newline_ends_at(chars: &[char], pos: usize, convention: NewlineConvention) -> bool {
    (pos > 0
        && newline_len_at(chars, pos - 1, convention).is_some_and(|length| pos - 1 + length == pos))
        || (pos > 1
            && newline_len_at(chars, pos - 2, convention)
                .is_some_and(|length| pos - 2 + length == pos))
}

// ── Public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Regex {
    ast: Node,
    flags: RegexFlags,
    num_groups: usize,
    symbols: PatternSymbols,
    /// Literal that every match must start with, when it can be proven from
    /// the AST. Used to skip impossible start positions before backtracking.
    start_literal: Option<char>,
    /// Whether boolean matching must retain capture contents for a later
    /// numeric or named backreference in the pattern.
    uses_backreferences: bool,
}

#[derive(Debug)]
struct SubroutineRegistry {
    root: Rc<Node>,
    by_index: HashMap<usize, Rc<Node>>,
    by_name: HashMap<String, Rc<Node>>,
    active_calls: RefCell<Vec<(SubroutineTarget, usize)>>,
}

type DuplicateNamedGroups = HashMap<String, Vec<usize>>;

#[derive(Debug)]
struct PatternFeatures {
    subroutines: Option<SubroutineRegistry>,
    duplicate_named_groups: Option<Rc<DuplicateNamedGroups>>,
    replacement_work_scale: usize,
    match_limit: Option<usize>,
    depth_limit: Option<usize>,
    heap_limit: Option<usize>,
}

#[derive(Debug)]
struct PatternSymbols {
    /// Named group name → group index.
    named_groups: HashMap<String, usize>,
    /// Allocated only for patterns that contain subroutine calls or duplicate
    /// named groups. The common pattern layout remains one optional pointer.
    features: Option<Rc<PatternFeatures>>,
}

impl PatternSymbols {
    #[inline]
    fn subroutines(&self) -> Option<&SubroutineRegistry> {
        self.features
            .as_deref()
            .and_then(|features| features.subroutines.as_ref())
    }

    #[inline]
    fn duplicate_named_groups(&self) -> Option<&Rc<DuplicateNamedGroups>> {
        self.features
            .as_deref()
            .and_then(|features| features.duplicate_named_groups.as_ref())
    }

    #[inline]
    fn replacement_work_scale(&self) -> usize {
        self.features
            .as_deref()
            .map_or(1, |features| features.replacement_work_scale)
    }
}

impl PatternFeatures {
    fn clamp_limits(&self, limits: &mut MatchLimits) {
        if let Some(limit) = self.match_limit {
            limits.backtrack = limits.backtrack.min(limit);
        }
        if let Some(limit) = self.depth_limit {
            limits.recursion = limits.recursion.min(limit);
        }
        if let Some(limit) = self.heap_limit {
            // PCRE2's limit is in KiB. Its interpreter heap frame is pattern
            // dependent, but the observable 10.42 boundary is eight ordinary
            // continuation frames per KiB for the capture/alternation shapes
            // that allocate them. Literal and specialized linear paths do
            // not consume this budget.
            limits.heap_frames = limits.heap_frames.min(limit.saturating_mul(8));
        }
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn make_subroutine_registry(ast: &Node, parser: &mut Parser) -> SubroutineRegistry {
    SubroutineRegistry {
        root: Rc::new(ast.clone()),
        by_index: std::mem::take(&mut parser.subpatterns_by_index),
        by_name: std::mem::take(&mut parser.subpatterns_by_name),
        active_calls: RefCell::new(Vec::new()),
    }
}

/// Per-executor cache of parsed and compiled PHP regular expressions.
///
/// Entries are evicted in insertion order. This keeps lookup O(1) on the hot
/// path without adding per-hit list maintenance; a repeated static pattern
/// remains cached until enough distinct patterns displace it.
pub struct RegexCache {
    /// Cache capacity in the low bits and the request-local `preg_last_error()`
    /// code in the high byte. Keeping both values in the existing word avoids
    /// growing `ExecutorGlobals` for scripts that never use PCRE diagnostics.
    capacity_and_last_error: usize,
    entries: HashMap<String, Rc<Regex>>,
    insertion_order: VecDeque<String>,
}

impl RegexCache {
    const ERROR_SHIFT: u32 = usize::BITS - 8;
    const CAPACITY_MASK: usize = (1usize << Self::ERROR_SHIFT) - 1;

    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity <= Self::CAPACITY_MASK,
            "regex cache capacity exceeds request-local packed representation"
        );
        Self {
            capacity_and_last_error: capacity,
            entries: HashMap::with_capacity(capacity),
            insertion_order: VecDeque::with_capacity(capacity),
        }
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity_and_last_error & Self::CAPACITY_MASK
    }

    /// PHP exposes the last PCRE operation status per request. The custom
    /// engine currently publishes the portable no-error/internal-error and
    /// malformed-UTF subsets; execution limits remain separate capabilities.
    #[inline]
    pub fn last_error(&self) -> u8 {
        (self.capacity_and_last_error >> Self::ERROR_SHIFT) as u8
    }

    #[inline]
    pub fn set_last_error(&mut self, error: u8) {
        self.capacity_and_last_error = self.capacity() | (usize::from(error) << Self::ERROR_SHIFT);
    }

    /// Return a shared compiled regex, compiling and caching it on a miss.
    /// Invalid patterns are not cached so callers preserve their existing
    /// false/null error behavior and no failed entry consumes cache capacity.
    pub fn get_or_compile(&mut self, php_pattern: &str) -> Result<Rc<Regex>, String> {
        if let Some(regex) = self.entries.get(php_pattern) {
            return Ok(Rc::clone(regex));
        }

        let (mut pattern, flags) = parse_php_regex(php_pattern)?;
        if !flags.unicode_mode.utf()
            && !leading_start_items_request_utf(&pattern)
            && !pattern.is_ascii()
        {
            // Without `u` PCRE compiles the pattern byte by byte; present its
            // UTF-8 bytes one char each so literals meet byte-view subjects.
            pattern = pattern.bytes().map(char::from).collect();
        }
        let regex = Rc::new(Regex::new(&pattern, flags)?);

        let capacity = self.capacity();
        if capacity == 0 {
            return Ok(regex);
        }

        if self.entries.len() == capacity {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.entries.remove(&oldest);
            }
        }

        let cache_key = php_pattern.to_string();
        self.entries.insert(cache_key.clone(), Rc::clone(&regex));
        self.insertion_order.push_back(cache_key);
        Ok(regex)
    }
}

fn leading_start_items_request_utf(pattern: &str) -> bool {
    let mut remainder = pattern;
    while let Some(item) = remainder.strip_prefix("(*") {
        let Some(end) = item.find(')') else {
            return false;
        };
        let name = &item[..end];
        if name == "UTF" {
            return true;
        }
        if !matches!(
            name,
            "UCP"
                | "NOTEMPTY"
                | "NOTEMPTY_ATSTART"
                | "NO_AUTO_POSSESS"
                | "NO_START_OPT"
                | "NO_DOTSTAR_ANCHOR"
                | "NO_JIT"
                | "CR"
                | "LF"
                | "CRLF"
                | "ANYCRLF"
                | "ANY"
                | "NUL"
                | "BSR_ANYCRLF"
                | "BSR_UNICODE"
        ) && !name.starts_with("LIMIT_")
        {
            return false;
        }
        remainder = &item[end + 1..];
    }
    false
}

impl Default for RegexCache {
    fn default() -> Self {
        Self::new(DEFAULT_REGEX_CACHE_CAPACITY)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

impl Match {
    pub fn as_str<'a>(&self, input: &'a str) -> &'a str {
        &input[self.start..self.end]
    }
}

#[inline]
fn publish_overall_match(groups: &mut [Option<Match>], start: usize, end: usize) {
    let reported_start = groups
        .first()
        .and_then(Option::as_ref)
        .map_or(start, |reset| reset.start);
    groups[0] = Some(Match {
        start: reported_start.min(end),
        end,
    });
}

#[derive(Debug, Clone)]
pub struct Captures {
    /// Group 0 = full match, group 1..N = capture groups.
    groups: Vec<Option<Match>>,
    /// Named group name → group index
    named_groups: HashMap<String, usize>,
    duplicate_named_groups: Option<Rc<DuplicateNamedGroups>>,
    mark: Option<String>,
}

impl Captures {
    pub fn get(&self, i: usize) -> Option<&Match> {
        self.groups.get(i).and_then(|m| m.as_ref())
    }

    pub fn get_named(&self, name: &str) -> Option<&Match> {
        self.named_group_slot(name).and_then(|i| self.get(i))
    }

    pub fn named_groups(&self) -> &HashMap<String, usize> {
        &self.named_groups
    }

    pub(crate) fn named_group_slot(&self, name: &str) -> Option<usize> {
        self.duplicate_named_groups
            .as_deref()
            .and_then(|groups| groups.get(name))
            .and_then(|indices| {
                indices
                    .iter()
                    .rev()
                    .find(|&&index| self.get(index).is_some())
                    .copied()
            })
            .or_else(|| self.named_groups.get(name).copied())
    }

    pub(crate) fn named_group_output_slot(&self, name: &str) -> Option<usize> {
        self.duplicate_named_groups
            .as_deref()
            .and_then(|groups| groups.get(name))
            .and_then(|indices| indices.first().copied())
            .or_else(|| self.named_groups.get(name).copied())
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn mark(&self) -> Option<&str> {
        self.mark.as_deref()
    }
}

/// Borrowed capture view used by consumers that can process each match before
/// the matcher advances. The backing capture slots are reused on the next
/// visit, so the view cannot escape the visitor call.
#[derive(Clone, Copy)]
pub(crate) struct CaptureView<'a> {
    groups: &'a [Option<Match>],
    symbols: &'a PatternSymbols,
    mark: Option<&'a str>,
}

impl CaptureView<'_> {
    pub(crate) fn get(&self, i: usize) -> Option<&Match> {
        self.groups.get(i).and_then(|capture| capture.as_ref())
    }

    pub(crate) fn named_groups(&self) -> &HashMap<String, usize> {
        &self.symbols.named_groups
    }

    pub(crate) fn named_group_slot(&self, name: &str) -> Option<usize> {
        self.symbols
            .duplicate_named_groups()
            .map(Rc::as_ref)
            .and_then(|groups| groups.get(name))
            .and_then(|indices| {
                indices
                    .iter()
                    .rev()
                    .find(|&&index| self.get(index).is_some())
                    .copied()
            })
            .or_else(|| self.symbols.named_groups.get(name).copied())
    }

    pub(crate) fn named_group_output_slot(&self, name: &str) -> Option<usize> {
        self.symbols
            .duplicate_named_groups()
            .map(Rc::as_ref)
            .and_then(|groups| groups.get(name))
            .and_then(|indices| indices.first().copied())
            .or_else(|| self.symbols.named_groups.get(name).copied())
    }

    pub(crate) fn len(&self) -> usize {
        self.groups.len()
    }

    pub(crate) fn mark(&self) -> Option<&str> {
        self.mark
    }
}

impl Regex {
    #[inline]
    fn linear_execution_uses_default_limits(&self, limits: MatchLimits) -> bool {
        self.effective_limits(limits) == MatchLimits::default()
    }

    #[inline]
    fn effective_limits(&self, mut limits: MatchLimits) -> MatchLimits {
        if let Some(features) = self.symbols.features.as_deref() {
            features.clamp_limits(&mut limits);
        }
        limits
    }

    #[inline]
    fn scaled_replacement_limits(&self, mut limits: MatchLimits) -> MatchLimits {
        limits = self.effective_limits(limits);
        let scale = self.symbols.replacement_work_scale();
        if scale > 1 && limits.backtrack > 0 {
            limits.backtrack = (limits.backtrack / scale).max(1);
        }
        limits
    }

    #[inline]
    pub(crate) fn is_unicode(&self) -> bool {
        self.flags.unicode_mode.utf()
    }

    /// Projection consumers need the declared slots even when there is no
    /// match. This exposes metadata only, not another matching path.
    pub(crate) fn capture_count(&self) -> usize {
        self.num_groups + 1
    }

    pub(crate) fn capture_names(&self) -> &HashMap<String, usize> {
        &self.symbols.named_groups
    }

    pub(crate) fn capture_name_output_slot(&self, name: &str) -> Option<usize> {
        self.symbols
            .duplicate_named_groups()
            .and_then(|groups| groups.get(name))
            .and_then(|indices| indices.first().copied())
            .or_else(|| self.symbols.named_groups.get(name).copied())
    }

    pub(crate) fn has_duplicate_named_groups(&self) -> bool {
        self.symbols.duplicate_named_groups().is_some()
    }

    /// Compile a regex pattern with given flags.
    pub fn new(pattern: &str, flags: RegexFlags) -> Result<Self, String> {
        let mut parser = Parser::new(pattern, flags);
        let ast = parser.parse()?;
        for lookbehind in &parser.lookbehinds {
            parser.validate_lookbehind(lookbehind)?;
        }
        if let Some((_, offset)) = parser
            .conditional_name_references
            .iter()
            .find(|(name, _)| !parser.named_groups.contains_key(name))
        {
            return Err(format!(
                "reference to non-existent subpattern at offset {offset}"
            ));
        }
        if let Some((_, offset)) = parser
            .numeric_backreferences
            .iter()
            .find(|(index, _)| *index == 0 || *index > parser.group_count)
        {
            return Err(format!(
                "reference to non-existent subpattern at offset {offset}"
            ));
        }
        if let Some((_, offset)) =
            parser
                .subroutine_references
                .iter()
                .find(|(target, _)| match target {
                    SubroutineTarget::WholePattern => false,
                    SubroutineTarget::Group(index) => {
                        *index == 0 || !parser.subpatterns_by_index.contains_key(index)
                    }
                    SubroutineTarget::Name(name) => !parser.subpatterns_by_name.contains_key(name),
                })
        {
            return Err(format!(
                "reference to non-existent subpattern at offset {offset}"
            ));
        }
        if estimated_compiled_units(&ast) > 65_535 {
            return Err(format!(
                "regular expression is too large at offset {}",
                pattern.len()
            ));
        }
        let start_literal = required_start_literal(&ast);
        let uses_backreferences = contains_backreference(&ast)
            || parser
                .subpatterns_by_index
                .values()
                .any(|node| contains_backreference(node));
        let subroutines = parser
            .saw_subroutine
            .then(|| make_subroutine_registry(&ast, &mut parser));
        let duplicate_named_groups = (!parser.duplicate_named_groups.is_empty())
            .then(|| Rc::new(std::mem::take(&mut parser.duplicate_named_groups)));
        let replacement_work_scale =
            if parser.saw_subroutine && quantified_subroutine_branching(&ast) {
                1024
            } else {
                1
            };
        let features = (subroutines.is_some()
            || duplicate_named_groups.is_some()
            || replacement_work_scale != 1
            || parser.match_limit.is_some()
            || parser.depth_limit.is_some()
            || parser.heap_limit.is_some())
        .then(|| {
            Rc::new(PatternFeatures {
                subroutines,
                duplicate_named_groups,
                replacement_work_scale,
                match_limit: parser.match_limit,
                depth_limit: parser.depth_limit,
                heap_limit: parser.heap_limit,
            })
        });
        Ok(Self {
            ast,
            flags: parser.flags,
            num_groups: parser.group_count,
            symbols: PatternSymbols {
                named_groups: parser.named_groups,
                features,
            },
            start_literal,
            uses_backreferences,
        })
    }

    /// Test whether the pattern matches without materializing capture output.
    /// Internal capture slots are retained only when the pattern needs their
    /// contents for backreferences.
    #[inline(always)]
    pub fn is_match(&self, subject: &str) -> bool {
        self.is_match_with_limits(subject, MatchLimits::default())
            .unwrap_or(false)
    }

    #[inline(always)]
    pub(crate) fn is_match_with_limits(
        &self,
        subject: &str,
        limits: MatchLimits,
    ) -> Result<bool, MatchLimitError> {
        if self.linear_execution_uses_default_limits(limits)
            && !self.uses_backreferences
            && linear::is_boolean_supported(&self.ast)
        {
            return Ok(linear::is_match(self, subject));
        }
        let chars: Vec<char> = subject.chars().collect();
        let byte_offsets = if self.uses_backreferences {
            ByteOffsets::for_subject(subject, &chars)
        } else {
            ByteOffsets::Identity
        };
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut groups = if self.uses_backreferences {
            vec![None; self.num_groups + 1]
        } else {
            Vec::new()
        };
        let mut budget = MatchBudget::new(self.effective_limits(limits));
        let mut start = 0;
        while start <= chars.len() {
            if let Some(literal) = self.start_literal
                && !self.flags.anchored
            {
                let Some(relative_start) = chars[start..]
                    .iter()
                    .position(|&candidate| chars_equal(candidate, literal, self.flags))
                else {
                    break;
                };
                start += relative_start;
            }
            groups.fill(None);
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                search_start: 0,
                groups: &mut groups,
                symbols: &self.symbols,
                mark: &mut mark,
                mark_positions: &mut mark_positions,
                control: &mut control,
                budget: &mut budget,
            };
            if match_seq_from(&self.ast, &[], start, &mut ctx).is_some() {
                return Ok(true);
            }
            if let Some(error) = budget.error() {
                return Err(error);
            }
            if self.flags.anchored {
                break;
            }
            let Some(next) = next_attempt_after_control(control, start) else {
                break;
            };
            start = next;
        }
        Ok(false)
    }

    /// Find first match in subject.  Returns captures (group 0 = whole match).
    pub fn captures(&self, subject: &str) -> Option<Captures> {
        self.captures_with_limits(subject, MatchLimits::default())
            .ok()
            .flatten()
    }

    pub(crate) fn captures_with_limits(
        &self,
        subject: &str,
        limits: MatchLimits,
    ) -> Result<Option<Captures>, MatchLimitError> {
        if self.linear_execution_uses_default_limits(limits)
            && self.num_groups == 0
            && let Some(first) = linear::try_first_match(self, subject)
        {
            return Ok(first.map(|full_match| Captures {
                groups: vec![Some(full_match)],
                named_groups: HashMap::new(),
                duplicate_named_groups: None,
                mark: None,
            }));
        }
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut groups = vec![None; self.num_groups + 1];
        let mut budget = MatchBudget::new(self.effective_limits(limits));
        // Try matching at every position; an anchored pattern only at the first.
        let mut next_allowed_start = 0usize;
        for start in 0..=chars.len() {
            if start < next_allowed_start {
                continue;
            }
            if self.flags.anchored && start > 0 {
                break;
            }
            if let Some(literal) = self.start_literal {
                if start == chars.len() || !chars_equal(chars[start], literal, self.flags) {
                    continue;
                }
            }
            groups.fill(None);
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    search_start: 0,
                    groups: &mut groups,
                    symbols: &self.symbols,
                    mark: &mut mark,
                    mark_positions: &mut mark_positions,
                    control: &mut control,
                    budget: &mut budget,
                };
                match_seq_from(&self.ast, &[], start, &mut ctx)
            };
            if let Some(end) = end {
                publish_overall_match(&mut groups, byte_offsets.get(start), byte_offsets.get(end));
                return Ok(Some(Captures {
                    groups,
                    named_groups: self.symbols.named_groups.clone(),
                    duplicate_named_groups: self.symbols.duplicate_named_groups().cloned(),
                    mark,
                }));
            }
            if let Some(error) = budget.error() {
                return Err(error);
            }
            let Some(next) = next_attempt_after_control(control, start) else {
                break;
            };
            next_allowed_start = next;
        }
        Ok(None)
    }

    /// Replace all occurrences.  Replacement can use `$1`, `$10`, `${2}`, `\\1` backrefs.
    pub fn replace_all(&self, subject: &str, replacement: &str) -> String {
        self.replace_limit(subject, replacement, usize::MAX).0
    }

    /// Replace at most `limit` occurrences and return the replacement count.
    pub fn replace_limit(&self, subject: &str, replacement: &str, limit: usize) -> (String, usize) {
        self.replace_limit_with_limits(subject, replacement, limit, MatchLimits::default())
            .unwrap_or_else(|_| (subject.to_string(), 0))
    }

    pub(crate) fn replace_limit_with_limits(
        &self,
        subject: &str,
        replacement: &str,
        limit: usize,
        limits: MatchLimits,
    ) -> Result<(String, usize), MatchLimitError> {
        if limit == 0 {
            return Ok((subject.to_string(), 0));
        }
        if self.linear_execution_uses_default_limits(limits)
            && self.num_groups == 0
            && node_definitely_consumes(&self.ast)
            && linear::is_supported(&self.ast)
        {
            let mut result = String::with_capacity(subject.len());
            let mut copied_until = 0usize;
            let mut count = 0usize;
            let visited: Result<usize, std::convert::Infallible> =
                linear::try_visit_captures(self, subject, |capture| {
                    if count == limit {
                        return Ok(false);
                    }
                    let matched = capture
                        .get(0)
                        .expect("linear replacement visitor always publishes group zero");
                    result.push_str(&subject[copied_until..matched.start]);
                    let groups = [Some(matched.clone())];
                    result.push_str(&expand_replacement(replacement, &groups, subject));
                    copied_until = matched.end;
                    count += 1;
                    Ok(true)
                });
            let _ = visited.unwrap();
            result.push_str(&subject[copied_until..]);
            return Ok((result, count));
        }
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut result = String::with_capacity(subject.len());
        let mut pos = 0;
        let mut search_start = 0;
        let mut count = 0;
        let mut retry_nonempty = false;
        let mut budget = MatchBudget::new(self.scaled_replacement_limits(limits));

        while pos <= chars.len() {
            if count == limit {
                result.push_str(&subject[byte_offsets.get(pos)..]);
                break;
            }
            let mut groups = vec![None; self.num_groups + 1];
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    search_start,
                    groups: &mut groups,
                    symbols: &self.symbols,
                    mark: &mut mark,
                    mark_positions: &mut mark_positions,
                    control: &mut control,
                    budget: &mut budget,
                };
                if retry_nonempty {
                    match_nonempty_at_start(&self.ast, pos, &mut ctx)
                } else {
                    match_seq_from(&self.ast, &[], pos, &mut ctx)
                }
            };
            if let Some(error) = budget.error() {
                return Err(error);
            }
            if let Some(end) = end {
                let attempt_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                publish_overall_match(&mut groups, attempt_start, match_end);
                let match_start = groups[0].as_ref().unwrap().start;

                // Append text before match
                result.push_str(&subject[byte_offsets.get(pos)..match_start]);
                // Append replacement with backreference expansion
                result.push_str(&expand_replacement(replacement, &groups, subject));
                count += 1;

                if end == pos {
                    retry_nonempty = true;
                    search_start = pos;
                } else {
                    retry_nonempty = false;
                    pos = end;
                    search_start = pos;
                }
            } else if retry_nonempty {
                retry_nonempty = false;
                if self.flags.anchored {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                }
                if pos < chars.len() {
                    result.push(chars[pos]);
                }
                pos += 1;
                search_start = pos;
            } else {
                if self.flags.anchored {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                }
                let Some(next) = next_attempt_after_control(control, pos) else {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                };
                let copied_end = next.min(chars.len());
                result.push_str(&subject[byte_offsets.get(pos)..byte_offsets.get(copied_end)]);
                pos = next;
            }
        }
        Ok((result, count))
    }

    /// Count non-overlapping matches without publishing capture data.
    #[inline(always)]
    pub(crate) fn count_matches_with_limits(
        &self,
        subject: &str,
        limits: MatchLimits,
    ) -> Result<usize, MatchLimitError> {
        if self.linear_execution_uses_default_limits(limits)
            && self.num_groups == 0
            && linear::is_supported(&self.ast)
            && let Some(count) = linear::try_count_matches(self, subject)
        {
            return Ok(count);
        }
        self.try_visit_captures_with_limits(subject, limits, |_| true)
    }

    #[cfg(test)]
    fn count_matches(&self, subject: &str) -> usize {
        self.count_matches_with_limits(subject, MatchLimits::default())
            .unwrap_or(0)
    }

    /// Visit non-overlapping matches in order while reusing one capture-slot
    /// buffer. Returning `false` stops before scanning the remaining subject.
    #[inline(never)]
    pub(crate) fn try_visit_captures<E, F>(&self, subject: &str, visitor: F) -> Result<usize, E>
    where
        F: for<'capture> FnMut(CaptureView<'capture>) -> Result<bool, E>,
    {
        // Prove the small iterative matcher once per consumer call. Its hot
        // loop lives in a separate codegen module so the canonical matcher and
        // unrelated preg_match layout remain stable.
        if linear::is_capture_visitor_supported(&self.ast) {
            linear::try_visit_captures(self, subject, visitor)
        } else {
            self.try_visit_backtracking_captures(subject, visitor)
        }
    }

    /// Limit-aware streaming visitor for `preg_match_all()` and `preg_split()`.
    /// Capture storage is reused between matches, so adding resource errors
    /// does not turn the ordinary projection path into an eager allocation.
    pub(crate) fn try_visit_captures_with_limits<F>(
        &self,
        subject: &str,
        limits: MatchLimits,
        mut visitor: F,
    ) -> Result<usize, MatchLimitError>
    where
        F: for<'capture> FnMut(CaptureView<'capture>) -> bool,
    {
        if self.linear_execution_uses_default_limits(limits)
            && linear::is_capture_visitor_supported(&self.ast)
        {
            let result: Result<usize, std::convert::Infallible> =
                linear::try_visit_captures(self, subject, |capture| Ok(visitor(capture)));
            return Ok(result.unwrap());
        }

        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut groups = vec![None; self.num_groups + 1];
        let mut pos = 0usize;
        let mut search_start = 0usize;
        let mut count = 0usize;
        let mut retry_nonempty = false;
        let start_literal = self.start_literal.filter(|_| !self.flags.anchored);

        while pos <= chars.len() {
            if !retry_nonempty && let Some(literal) = start_literal {
                let Some(relative_pos) = chars[pos..]
                    .iter()
                    .position(|&candidate| chars_equal(candidate, literal, self.flags))
                else {
                    break;
                };
                pos += relative_pos;
            }
            groups.fill(None);
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let mut budget = MatchBudget::new(self.effective_limits(limits));
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    search_start,
                    groups: &mut groups,
                    symbols: &self.symbols,
                    mark: &mut mark,
                    mark_positions: &mut mark_positions,
                    control: &mut control,
                    budget: &mut budget,
                };
                if retry_nonempty {
                    match_nonempty_at_start(&self.ast, pos, &mut ctx)
                } else {
                    match_seq_from(&self.ast, &[], pos, &mut ctx)
                }
            };
            if let Some(error) = budget.error() {
                return Err(error);
            }
            if let Some(end) = end {
                publish_overall_match(&mut groups, byte_offsets.get(pos), byte_offsets.get(end));
                count += 1;
                if !visitor(CaptureView {
                    groups: &groups,
                    symbols: &self.symbols,
                    mark: mark.as_deref(),
                }) {
                    break;
                }
                if end == pos {
                    retry_nonempty = true;
                    search_start = pos;
                } else {
                    retry_nonempty = false;
                    pos = end;
                    search_start = pos;
                }
            } else if retry_nonempty {
                retry_nonempty = false;
                if self.flags.anchored {
                    break;
                }
                pos += 1;
                search_start = pos;
            } else if self.flags.anchored {
                break;
            } else {
                let Some(next) = next_attempt_after_control(control, pos) else {
                    break;
                };
                pos = next;
            }
        }
        Ok(count)
    }

    #[inline(never)]
    fn try_visit_backtracking_captures<E, F>(
        &self,
        subject: &str,
        mut visitor: F,
    ) -> Result<usize, E>
    where
        F: for<'capture> FnMut(CaptureView<'capture>) -> Result<bool, E>,
    {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut groups = vec![None; self.num_groups + 1];
        let mut pos = 0;
        let mut search_start = 0;
        let mut retry_nonempty = false;
        let mut count = 0;
        let start_literal = self.start_literal.filter(|_| !self.flags.anchored);

        while pos <= chars.len() {
            if !retry_nonempty && let Some(literal) = start_literal {
                let Some(relative_pos) = chars[pos..]
                    .iter()
                    .position(|&candidate| chars_equal(candidate, literal, self.flags))
                else {
                    break;
                };
                pos += relative_pos;
            }
            groups.fill(None);
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let mut budget = MatchBudget::new(self.effective_limits(MatchLimits::default()));
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    search_start,
                    groups: &mut groups,
                    symbols: &self.symbols,
                    mark: &mut mark,
                    mark_positions: &mut mark_positions,
                    control: &mut control,
                    budget: &mut budget,
                };
                if retry_nonempty {
                    match_nonempty_at_start(&self.ast, pos, &mut ctx)
                } else {
                    match_seq_from(&self.ast, &[], pos, &mut ctx)
                }
            };
            if let Some(end) = end {
                let match_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                publish_overall_match(&mut groups, match_start, match_end);
                count += 1;
                let keep_scanning = visitor(CaptureView {
                    groups: &groups,
                    symbols: &self.symbols,
                    mark: mark.as_deref(),
                })?;
                if !keep_scanning {
                    break;
                }
                if end == pos {
                    retry_nonempty = true;
                    search_start = pos;
                } else {
                    retry_nonempty = false;
                    pos = end;
                    search_start = pos;
                }
            } else if retry_nonempty {
                retry_nonempty = false;
                if self.flags.anchored {
                    break;
                }
                pos += 1;
                search_start = pos;
            } else if self.flags.anchored {
                break;
            } else {
                let Some(next) = next_attempt_after_control(control, pos) else {
                    break;
                };
                pos = next;
            }
        }
        Ok(count)
    }

    /// Find all non-overlapping matches, returning a vector of Captures.
    pub fn captures_iter(&self, subject: &str) -> Vec<Captures> {
        let mut results = Vec::new();
        let completed: Result<usize, std::convert::Infallible> =
            self.try_visit_captures(subject, |captures| {
                results.push(Captures {
                    groups: captures.groups.to_vec(),
                    named_groups: captures.symbols.named_groups.clone(),
                    duplicate_named_groups: captures.symbols.duplicate_named_groups().cloned(),
                    mark: captures.mark.map(str::to_string),
                });
                Ok(true)
            });
        debug_assert!(completed.is_ok());
        results
    }

    /// Split subject by regex.  `limit` < 0 means no limit.
    pub fn split(&self, subject: &str, limit: i64) -> Vec<String> {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut parts = Vec::new();
        let mut last_end = 0usize; // char index of end of last match
        let mut splits = 0i64;
        let effective_limit = if limit <= 0 { i64::MAX } else { limit };

        let mut scan = 0;
        while scan <= chars.len() {
            if splits + 1 >= effective_limit {
                break;
            }
            // Try matching at every position starting from `scan`
            let mut found = false;
            let mut try_start = scan;
            while try_start <= chars.len() {
                if self.flags.anchored && try_start > scan {
                    break;
                }
                let mut groups = vec![None; self.num_groups + 1];
                let mut mark = None;
                let mut mark_positions = None;
                let mut control = None;
                let mut budget = MatchBudget::new(self.effective_limits(MatchLimits::default()));
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    search_start: scan,
                    groups: &mut groups,
                    symbols: &self.symbols,
                    mark: &mut mark,
                    mark_positions: &mut mark_positions,
                    control: &mut control,
                    budget: &mut budget,
                };
                if let Some(end) = match_seq_from(&self.ast, &[], try_start, &mut ctx) {
                    publish_overall_match(
                        &mut groups,
                        byte_offsets.get(try_start),
                        byte_offsets.get(end),
                    );
                    let full_match = groups[0].as_ref().unwrap();
                    let match_start_byte = full_match.start;
                    // Don't split on zero-length match at same position
                    if full_match.start == full_match.end
                        && full_match.start == byte_offsets.get(last_end)
                        && try_start < chars.len()
                    {
                        try_start += 1;
                        continue;
                    }
                    // Push text from last_end to match_start
                    let last_end_byte = byte_offsets.get(last_end);
                    parts.push(subject[last_end_byte..match_start_byte].to_string());
                    splits += 1;
                    last_end = end;
                    scan = if end == try_start { end + 1 } else { end };
                    found = true;
                    break;
                }
                let Some(next) = next_attempt_after_control(control, try_start) else {
                    break;
                };
                try_start = next;
            }
            if !found {
                break;
            }
        }
        // Push remainder
        let last_end_byte = byte_offsets.get(last_end);
        parts.push(subject[last_end_byte..].to_string());
        parts
    }

    /// Replace all occurrences, calling a closure for each match to produce the replacement.
    pub fn replace_all_with<F>(&self, subject: &str, mut replacer: F) -> String
    where
        F: FnMut(&Captures, &str) -> String,
    {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut result = String::new();
        let mut pos = 0;
        let mut search_start = 0;
        let mut retry_nonempty = false;

        while pos <= chars.len() {
            let mut groups = vec![None; self.num_groups + 1];
            let mut mark = None;
            let mut mark_positions = None;
            let mut control = None;
            let mut budget = MatchBudget::new(self.effective_limits(MatchLimits::default()));
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                search_start,
                groups: &mut groups,
                symbols: &self.symbols,
                mark: &mut mark,
                mark_positions: &mut mark_positions,
                control: &mut control,
                budget: &mut budget,
            };
            let end = if retry_nonempty {
                match_nonempty_at_start(&self.ast, pos, &mut ctx)
            } else {
                match_seq_from(&self.ast, &[], pos, &mut ctx)
            };
            if let Some(end) = end {
                let attempt_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                publish_overall_match(ctx.groups, attempt_start, match_end);
                let match_start = ctx.groups[0].as_ref().unwrap().start;

                result.push_str(&subject[byte_offsets.get(pos)..match_start]);
                let caps = Captures {
                    groups: groups.clone(),
                    named_groups: self.symbols.named_groups.clone(),
                    duplicate_named_groups: self.symbols.duplicate_named_groups().cloned(),
                    mark: mark.clone(),
                };
                result.push_str(&replacer(&caps, subject));

                if end == pos {
                    retry_nonempty = true;
                    search_start = pos;
                } else {
                    retry_nonempty = false;
                    pos = end;
                    search_start = pos;
                }
            } else if retry_nonempty {
                retry_nonempty = false;
                if self.flags.anchored {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                }
                if pos < chars.len() {
                    result.push(chars[pos]);
                }
                pos += 1;
                search_start = pos;
            } else {
                if self.flags.anchored {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                }
                let Some(next) = next_attempt_after_control(control, pos) else {
                    result.push_str(&subject[byte_offsets.get(pos)..]);
                    break;
                };
                let copied_end = next.min(chars.len());
                result.push_str(&subject[byte_offsets.get(pos)..byte_offsets.get(copied_end)]);
                pos = next;
            }
        }
        result.shrink_to_fit();
        result
    }
}

/// PCRE2's compiled program uses a bounded 16-bit code-unit representation.
/// Count an intentionally conservative equivalent before admitting an AST;
/// bounded repetitions are expanded by PCRE2 and therefore multiply their
/// inner program size even though this interpreter keeps one node.
#[cold]
fn estimated_compiled_units(node: &Node) -> usize {
    let add = |left: usize, right: usize| left.saturating_add(right);
    match node {
        Node::Alternation(branches) => branches.iter().fold(4, |total, branch| {
            add(total, add(3, estimated_compiled_units(branch)))
        }),
        Node::Sequence(nodes) => nodes
            .iter()
            .fold(0, |total, node| add(total, estimated_compiled_units(node))),
        Node::Group { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => add(6, estimated_compiled_units(inner)),
        Node::Quantifier {
            inner,
            max: Some(max),
            ..
        } => {
            let inner_units = estimated_compiled_units(inner);
            let specialized = matches!(
                inner.as_ref(),
                Node::Literal(_)
                    | Node::AnyChar
                    | Node::ByteUnit
                    | Node::NotNewline
                    | Node::CharClass { .. }
                    | Node::Shorthand(_)
                    | Node::Backreference(_)
                    | Node::NamedBackreference(_)
                    | Node::GraphemeCluster
                    | Node::Linebreak
            );
            add(
                6,
                if specialized {
                    inner_units
                } else {
                    inner_units.saturating_mul(*max)
                },
            )
        }
        Node::Quantifier {
            inner, max: None, ..
        } => add(6, estimated_compiled_units(inner)),
        Node::Conditional { yes, no, .. } => add(
            8,
            add(
                estimated_compiled_units(yes),
                no.as_deref().map_or(0, estimated_compiled_units),
            ),
        ),
        Node::CharClass { items, .. } => add(4, items.len().saturating_mul(3)),
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::Anchor(_)
        | Node::WordBoundary(_)
        | Node::Shorthand(_)
        | Node::Backreference(_)
        | Node::NamedBackreference(_)
        | Node::GraphemeCluster
        | Node::Linebreak
        | Node::Mark(_)
        | Node::Control { .. }
        | Node::ResetStart
        | Node::Subroutine(_)
        | Node::CaptureEnd { .. }
        | Node::CaptureRestore { .. } => 4,
        Node::Quoted(_) => unreachable!("quoted literals are lowered by the parser"),
    }
}

fn quantified_subroutine_branching(node: &Node) -> bool {
    match node {
        Node::Quantifier { inner, .. } => {
            (contains_subroutine_call(inner) && match_states_can_branch(inner))
                || quantified_subroutine_branching(inner)
        }
        Node::Alternation(nodes) | Node::Sequence(nodes) => {
            nodes.iter().any(quantified_subroutine_branching)
        }
        Node::Group { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => quantified_subroutine_branching(inner),
        Node::Conditional { yes, no, .. } => {
            quantified_subroutine_branching(yes)
                || no.as_deref().is_some_and(quantified_subroutine_branching)
        }
        _ => false,
    }
}

fn contains_subroutine_call(node: &Node) -> bool {
    match node {
        Node::Subroutine(_) => true,
        Node::Alternation(nodes) | Node::Sequence(nodes) => {
            nodes.iter().any(contains_subroutine_call)
        }
        Node::Group { inner, .. }
        | Node::Quantifier { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => contains_subroutine_call(inner),
        Node::Conditional { yes, no, .. } => {
            contains_subroutine_call(yes) || no.as_deref().is_some_and(contains_subroutine_call)
        }
        _ => false,
    }
}

fn contains_control_verb(node: &Node) -> bool {
    match node {
        Node::Control { .. } => true,
        Node::Alternation(nodes) | Node::Sequence(nodes) => nodes.iter().any(contains_control_verb),
        Node::Group { inner, .. }
        | Node::Quantifier { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => contains_control_verb(inner),
        Node::Conditional { yes, no, .. } => {
            contains_control_verb(yes) || no.as_deref().is_some_and(contains_control_verb)
        }
        _ => false,
    }
}

fn node_definitely_consumes(node: &Node) -> bool {
    match node {
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::CharClass { .. }
        | Node::Shorthand(_)
        | Node::GraphemeCluster
        | Node::Linebreak => true,
        Node::Sequence(nodes) => nodes.iter().any(node_definitely_consumes),
        Node::Alternation(nodes) => !nodes.is_empty() && nodes.iter().all(node_definitely_consumes),
        Node::Group { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => node_definitely_consumes(inner),
        Node::Quantifier { inner, min, .. } => *min > 0 && node_definitely_consumes(inner),
        _ => false,
    }
}

/// Find a literal that must occur at the first consumed position. Returning
/// None is always safe; Some is returned only when the AST proves the prefix.
fn required_start_literal(node: &Node) -> Option<char> {
    match node {
        Node::Literal(ch) => Some(*ch),
        Node::Sequence(nodes) => {
            for node in nodes {
                match node {
                    // These nodes consume no input, so the required literal can
                    // still come from the next node in the sequence.
                    Node::Anchor(_)
                    | Node::WordBoundary(_)
                    | Node::Lookahead { .. }
                    | Node::Lookbehind { .. }
                    | Node::Mark(_)
                    | Node::ResetStart
                    | Node::CaptureEnd { .. } => continue,
                    _ => return required_start_literal(node),
                }
            }
            None
        }
        Node::Group { inner, .. } | Node::ScriptRun { inner, .. } => required_start_literal(inner),
        // A locally caseless literal cannot safely drive the root flag's
        // case-sensitive prefix skip. Keep the optimization conservative.
        Node::LocalFlags { .. } => None,
        Node::Alternation(branches) => {
            let first = required_start_literal(branches.first()?)?;
            branches
                .iter()
                .skip(1)
                .all(|branch| required_start_literal(branch) == Some(first))
                .then_some(first)
        }
        Node::Quantifier { inner, min, .. } if *min > 0 => required_start_literal(inner),
        _ => None,
    }
}

fn contains_backreference(node: &Node) -> bool {
    match node {
        Node::Backreference(_) | Node::NamedBackreference(_) => true,
        Node::Group { inner, .. }
        | Node::Quantifier { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => contains_backreference(inner),
        Node::Alternation(nodes) | Node::Sequence(nodes) => {
            nodes.iter().any(contains_backreference)
        }
        Node::Conditional {
            condition: _,
            yes: _,
            no: _,
        } => {
            // Capture and recursion conditions consult participation state.
            true
        }
        Node::CaptureEnd { .. } => false,
        _ => false,
    }
}

// ── Helper: expand replacement backreferences ───────────────────────────────

fn expand_replacement(repl: &str, groups: &[Option<Match>], input: &str) -> String {
    let mut out = String::new();
    let bytes = repl.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                // ${N} or ${name}
                if let Some(end) = bytes[i + 2..].iter().position(|&b| b == b'}') {
                    let num_str = &repl[i + 2..i + 2 + end];
                    if let Ok(n) = num_str.parse::<usize>() {
                        if let Some(Some(m)) = groups.get(n) {
                            out.push_str(&input[m.start..m.end]);
                        }
                        i += 3 + end;
                        continue;
                    }
                }
            } else if bytes[i + 1].is_ascii_digit() {
                // PHP replacement backreferences consume at most two digits;
                // `$103` is group 10 followed by the literal `3`.
                let mut j = i + 1;
                while j < bytes.len() && j < i + 3 && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let num_str = &repl[i + 1..j];
                if let Ok(n) = num_str.parse::<usize>() {
                    if let Some(Some(m)) = groups.get(n) {
                        out.push_str(&input[m.start..m.end]);
                    }
                    i = j;
                    continue;
                }
            }
        } else if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            // The backslash spelling follows the same two-digit boundary.
            let mut j = i + 1;
            while j < bytes.len() && j < i + 3 && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let num_str = &repl[i + 1..j];
            if let Ok(n) = num_str.parse::<usize>() {
                if let Some(Some(m)) = groups.get(n) {
                    out.push_str(&input[m.start..m.end]);
                }
                i = j;
                continue;
            }
        }
        // Copy one whole character: a non-ASCII literal must keep its
        // encoding instead of being re-read as individual Latin-1 bytes.
        let width = repl[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&repl[i..i + width]);
        i += width;
    }
    out
}

// ── Byte offset from char index ─────────────────────────────────────────────

/// Build the character array used by the matcher and a parallel O(1) mapping
/// from every character boundary to its byte offset in the original UTF-8
/// subject. Recomputing an offset from the start for every capture makes a
/// repeated-match scan quadratic.
fn subject_chars(input: &str) -> (Vec<char>, ByteOffsets) {
    let chars: Vec<char> = input.chars().collect();
    let byte_offsets = ByteOffsets::for_subject(input, &chars);
    (chars, byte_offsets)
}

enum ByteOffsets {
    Identity,
    Utf8(Vec<usize>),
}

impl ByteOffsets {
    fn for_subject(input: &str, chars: &[char]) -> Self {
        if input.is_ascii() {
            return Self::Identity;
        }

        let mut byte_offsets = Vec::with_capacity(chars.len() + 1);
        let mut byte_offset = 0;
        byte_offsets.push(byte_offset);
        for ch in chars {
            byte_offset += ch.len_utf8();
            byte_offsets.push(byte_offset);
        }
        Self::Utf8(byte_offsets)
    }

    #[inline]
    fn get(&self, char_index: usize) -> usize {
        match self {
            Self::Identity => char_index,
            Self::Utf8(byte_offsets) => byte_offsets[char_index],
        }
    }
}

// ── Match context ───────────────────────────────────────────────────────────

struct MatchMetadata<'a> {
    input: &'a str,
    byte_offsets: &'a ByteOffsets,
}

struct MatchBudget {
    backtracks_remaining: usize,
    recursion_limit: usize,
    recursion_depth: usize,
    heap_frames_remaining: usize,
    /// The native PCRE2 JIT has a separate, deliberately bounded stack. The
    /// custom engine models that resource independently from match_limit so a
    /// huge quantified path cannot allocate one continuation per character.
    jit_steps_remaining: usize,
    jit: bool,
    error: Option<MatchLimitError>,
}

impl MatchBudget {
    const JIT_STEP_LIMIT: usize = 32_768;

    fn new(limits: MatchLimits) -> Self {
        Self {
            backtracks_remaining: limits.backtrack,
            recursion_limit: limits.recursion,
            recursion_depth: 0,
            heap_frames_remaining: limits.heap_frames,
            jit_steps_remaining: if limits.jit {
                Self::JIT_STEP_LIMIT
            } else {
                usize::MAX
            },
            jit: limits.jit,
            error: None,
        }
    }

    #[inline]
    fn error(&self) -> Option<MatchLimitError> {
        self.error
    }

    #[inline]
    fn has_error(&self) -> bool {
        self.error().is_some()
    }

    #[inline]
    fn set_error(&mut self, error: MatchLimitError) {
        self.error = Some(error);
    }

    #[inline]
    fn enter_recursion(&mut self) -> bool {
        if self.has_error() {
            return false;
        }
        if self.recursion_depth >= self.recursion_limit {
            self.set_error(MatchLimitError::Recursion);
            return false;
        }
        self.recursion_depth += 1;
        true
    }

    #[inline]
    fn leave_recursion(&mut self) {
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }

    #[inline]
    fn retain_quantifier_continuation(&mut self) -> bool {
        if self.has_error() {
            return false;
        }
        if self.heap_frames_remaining == 0 {
            self.set_error(MatchLimitError::Heap);
            return false;
        }
        self.heap_frames_remaining -= 1;
        if self.jit {
            if self.jit_steps_remaining == 0 {
                self.set_error(MatchLimitError::JitStack);
                return false;
            }
            self.jit_steps_remaining -= 1;
        }
        true
    }

    #[inline]
    fn consume_backtrack(&mut self) -> bool {
        if self.has_error() {
            return false;
        }
        if self.backtracks_remaining == 0 {
            self.set_error(MatchLimitError::Backtrack);
            return false;
        }
        self.backtracks_remaining -= 1;
        true
    }

    #[inline]
    fn consume_alternative(&mut self) -> bool {
        self.consume_backtrack()
    }

    #[inline]
    fn reject_impossible_search_if_cost_exceeds(&mut self, required: usize) -> bool {
        if required <= self.backtracks_remaining {
            return false;
        }
        self.set_error(MatchLimitError::Backtrack);
        true
    }
}

struct MatchCtx<'a> {
    chars: &'a [char],
    metadata: &'a MatchMetadata<'a>,
    flags: RegexFlags,
    search_start: usize,
    groups: &'a mut Vec<Option<Match>>,
    symbols: &'a PatternSymbols,
    mark: &'a mut Option<String>,
    mark_positions: &'a mut Option<Vec<(String, usize)>>,
    control: &'a mut Option<MatchControl>,
    budget: &'a mut MatchBudget,
}

fn resolve_subroutine(target: &SubroutineTarget, ctx: &MatchCtx<'_>) -> Option<Rc<Node>> {
    let registry = ctx.symbols.subroutines()?;
    match target {
        SubroutineTarget::WholePattern => Some(Rc::clone(&registry.root)),
        SubroutineTarget::Group(index) => registry.by_index.get(index).cloned(),
        SubroutineTarget::Name(name) => registry.by_name.get(name).cloned(),
    }
}

fn capture_condition_matches(
    condition: &CaptureCondition,
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> bool {
    match condition {
        CaptureCondition::Group(index) => ctx.groups.get(*index).is_some_and(Option::is_some),
        CaptureCondition::Name(name) => named_group_is_set(ctx.symbols, ctx.groups, name),
        CaptureCondition::RecursionTarget(target) => ctx
            .symbols
            .subroutines()
            .and_then(|registry| registry.active_calls.borrow().last().cloned())
            .is_some_and(|(active, _)| active == *target),
        CaptureCondition::AmbiguousRecursion { name, target } => {
            if ctx.symbols.named_groups.contains_key(name) {
                named_group_is_set(ctx.symbols, ctx.groups, name)
            } else if let Some(target) = target {
                ctx.symbols
                    .subroutines()
                    .and_then(|registry| registry.active_calls.borrow().last().cloned())
                    .is_some_and(|(active, _)| active == *target)
            } else {
                in_subroutine(ctx)
            }
        }
        CaptureCondition::Assertion {
            positive,
            lookbehind,
            inner,
        } => {
            let saved_groups = ctx.groups.clone();
            let saved_mark = ctx.mark.clone();
            let saved_mark_positions = ctx.mark_positions.clone();
            let matched = if *lookbehind {
                let mut found = false;
                for start in (0..=pos).rev() {
                    *ctx.groups = saved_groups.clone();
                    *ctx.mark = saved_mark.clone();
                    *ctx.mark_positions = saved_mark_positions.clone();
                    let result = match_seq_from(inner, &[], start, ctx);
                    let accepted = matches!(ctx.control.take(), Some(MatchControl::Accept));
                    if accepted || result == Some(pos) {
                        found = true;
                        break;
                    }
                    ctx.control.take();
                    if ctx.budget.has_error() {
                        break;
                    }
                }
                found
            } else {
                let matched = match_seq_from(inner, &[], pos, ctx).is_some();
                ctx.control.take();
                matched
            };
            if matched != *positive {
                *ctx.groups = saved_groups;
                *ctx.mark = saved_mark;
                *ctx.mark_positions = saved_mark_positions;
                false
            } else {
                if !matched {
                    *ctx.groups = saved_groups;
                    *ctx.mark = saved_mark;
                    *ctx.mark_positions = saved_mark_positions;
                }
                true
            }
        }
        CaptureCondition::Always(value) => *value,
    }
}

fn named_group_is_set(symbols: &PatternSymbols, groups: &[Option<Match>], name: &str) -> bool {
    if let Some(indices) = symbols
        .duplicate_named_groups()
        .and_then(|duplicates| duplicates.get(name))
    {
        return indices
            .iter()
            .any(|&index| groups.get(index).is_some_and(Option::is_some));
    }
    symbols
        .named_groups
        .get(name)
        .and_then(|&index| groups.get(index))
        .is_some_and(Option::is_some)
}

fn first_set_named_group(
    symbols: &PatternSymbols,
    groups: &[Option<Match>],
    name: &str,
) -> Option<usize> {
    if let Some(indices) = symbols
        .duplicate_named_groups()
        .and_then(|duplicates| duplicates.get(name))
    {
        return indices
            .iter()
            .find(|&&index| groups.get(index).is_some_and(Option::is_some))
            .copied();
    }
    symbols.named_groups.get(name).copied()
}

#[inline]
fn in_subroutine(ctx: &MatchCtx<'_>) -> bool {
    ctx.symbols
        .subroutines()
        .is_some_and(|registry| !registry.active_calls.borrow().is_empty())
}

#[inline]
fn subroutine_call_is_active(ctx: &MatchCtx<'_>, target: &SubroutineTarget, pos: usize) -> bool {
    ctx.symbols.subroutines().is_some_and(|registry| {
        registry
            .active_calls
            .borrow()
            .iter()
            .any(|(active, active_pos)| active == target && *active_pos == pos)
    })
}

#[inline]
fn push_subroutine_call(ctx: &mut MatchCtx<'_>, target: SubroutineTarget, pos: usize) {
    ctx.symbols
        .subroutines()
        .expect("subroutine registry initializes cold match state")
        .active_calls
        .borrow_mut()
        .push((target, pos));
}

#[inline]
fn pop_subroutine_call(ctx: &mut MatchCtx<'_>) {
    if let Some(registry) = ctx.symbols.subroutines() {
        registry.active_calls.borrow_mut().pop();
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_subroutine(
    target: &SubroutineTarget,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    if subroutine_call_is_active(ctx, target, pos) || !ctx.budget.enter_recursion() {
        return None;
    }
    let target_kind = target.clone();
    let target = resolve_subroutine(target, ctx);
    push_subroutine_call(ctx, target_kind, pos);
    let initial = BacktrackState::take(pos, ctx);
    let caller_groups = Rc::new(initial.groups.clone());
    let mut continuation = Vec::with_capacity(rest.len() + 1);
    continuation.push(Node::CaptureRestore {
        groups: Rc::clone(&caller_groups),
    });
    continuation.extend_from_slice(rest);
    let result = target.and_then(|target| match_seq_from(&target, &continuation, pos, ctx));
    if result.is_some() && matches!(ctx.control, Some(MatchControl::Accept)) {
        ctx.groups.clone_from(caller_groups.as_ref());
    }
    if result.is_none() {
        initial.install(ctx);
    }
    pop_subroutine_call(ctx);
    ctx.budget.leave_recursion();
    result
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_conditional(
    condition: &CaptureCondition,
    yes: &Node,
    no: Option<&Node>,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let selected = if capture_condition_matches(condition, pos, ctx) {
        Some(yes)
    } else {
        no
    };
    match selected {
        Some(selected) => match_seq_from(selected, rest, pos, ctx),
        None => match_rest(rest, pos, ctx),
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_non_atomic_lookahead(
    inner: &Node,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let (initial, states) = collect_match_states(inner, pos, ctx);
    let accepted = matches!(ctx.control.take(), Some(MatchControl::Accept));
    for state in states {
        state.install(ctx);
        if let Some(end) = match_rest(rest, pos, ctx) {
            return Some(end);
        }
        if ctx.control.is_some() || accepted {
            return None;
        }
    }
    initial.install(ctx);
    None
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_non_atomic_lookbehind(
    inner: &Node,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let original = BacktrackState::take(pos, ctx);
    for start in (0..=pos).rev() {
        let mut initial = original.clone();
        initial.end = start;
        let states = collect_match_states_from(inner, initial, ctx);
        let accepted = matches!(ctx.control.take(), Some(MatchControl::Accept));
        for state in states
            .into_iter()
            .filter(|state| accepted || state.end == pos)
        {
            state.install(ctx);
            if let Some(end) = match_rest(rest, pos, ctx) {
                return Some(end);
            }
            if ctx.control.is_some() || accepted {
                return None;
            }
        }
    }
    original.install(ctx);
    None
}

fn decimal_digit_block(c: char) -> Option<u32> {
    if !unicode::category_is(c, "Nd") {
        return None;
    }
    let mut first = u32::from(c);
    for _ in 0..9 {
        let Some(previous) = first.checked_sub(1).and_then(char::from_u32) else {
            break;
        };
        if !unicode::category_is(previous, "Nd") {
            break;
        }
        first -= 1;
    }
    Some(first)
}

fn script_run_is_valid(chars: &[char]) -> bool {
    if chars.len() < 2 {
        return true;
    }
    let mut script_sets = Vec::new();
    let mut digit_block = None;
    for &character in chars {
        let script = unicode::script_name(character);
        if script == "unknown" {
            return false;
        }
        if unicode::has_explicit_script_extensions(character) {
            let extensions = unicode_scripts::SCRIPT_NAMES
                .iter()
                .enumerate()
                .filter_map(|(index, name)| {
                    unicode::script_has_extension(character, index as u16).then_some(*name)
                })
                .collect::<HashSet<_>>();
            if !extensions.is_empty() {
                script_sets.push(extensions);
            }
        } else if !matches!(script, "common" | "inherited") {
            script_sets.push(HashSet::from([script]));
        }
        if let Some(block) = decimal_digit_block(character) {
            if digit_block.is_some_and(|previous| previous != block) {
                return false;
            }
            digit_block = Some(block);
        }
    }
    if script_sets.len() <= 1 {
        return true;
    }
    let east_asian = ["han", "hiragana", "katakana", "hangul", "bopomofo"];
    if script_sets
        .iter()
        .all(|scripts| scripts.iter().all(|script| east_asian.contains(script)))
    {
        let combined = script_sets
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        return combined.contains("han")
            || combined
                .iter()
                .all(|script| matches!(*script, "hiragana" | "katakana"));
    }
    let mut intersection = script_sets.remove(0);
    for scripts in script_sets {
        intersection.retain(|script| scripts.contains(script));
        if intersection.is_empty() {
            return false;
        }
    }
    true
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_script_run(
    inner: &Node,
    atomic: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let (initial, mut states) = collect_match_states(inner, pos, ctx);
    let accepted = matches!(ctx.control.take(), Some(MatchControl::Accept));
    if atomic || accepted {
        states.truncate(1);
    }
    for state in states {
        let end = state.end;
        if !accepted && !script_run_is_valid(&ctx.chars[pos..end]) {
            continue;
        }
        state.install(ctx);
        if let Some(end) = match_rest(rest, end, ctx) {
            return Some(end);
        }
        if ctx.control.is_some() || atomic || accepted {
            return None;
        }
    }
    initial.install(ctx);
    None
}

// ── Core matching (backtracking with continuation) ──────────────────────────
//
// The key insight: to support proper backtracking, every node match must know
// about the "rest" of the sequence that follows it.  When a quantifier tries
// N repetitions and the rest fails, it can backtrack to N-1, etc.
//
// `match_seq_from(node, rest, pos, ctx)` tries to match `node` followed by
// all nodes in `rest` starting at `pos`.  Returns Some(final_pos) on success.

/// Inclusive range of subject characters a node can consume, or `None` when
/// it is unbounded (backreferences, `\R`, `\X`, open-ended quantifiers).
fn node_length_range(node: &Node) -> Option<(usize, usize)> {
    match node {
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::CharClass { .. }
        | Node::Shorthand(_) => Some((1, 1)),
        Node::Anchor(_)
        | Node::WordBoundary(_)
        | Node::Lookahead { .. }
        | Node::Lookbehind { .. }
        | Node::Mark(_)
        | Node::Control { .. }
        | Node::ResetStart
        | Node::CaptureEnd { .. }
        | Node::CaptureRestore { .. } => Some((0, 0)),
        Node::Group { inner, .. }
        | Node::Atomic(inner)
        | Node::ScriptRun { inner, .. }
        | Node::LocalFlags { inner, .. } => node_length_range(inner),
        Node::Sequence(nodes) => nodes.iter().try_fold((0usize, 0usize), |(min, max), node| {
            let (lo, hi) = node_length_range(node)?;
            Some((min + lo, max.checked_add(hi)?))
        }),
        Node::Alternation(branches) => {
            let mut range: Option<(usize, usize)> = None;
            for branch in branches {
                let (lo, hi) = node_length_range(branch)?;
                range = Some(range.map_or((lo, hi), |(min, max)| (min.min(lo), max.max(hi))));
            }
            range
        }
        Node::Quantifier {
            inner,
            min,
            max: Some(max),
            ..
        } => {
            let (lo, hi) = node_length_range(inner)?;
            Some((lo.checked_mul(*min)?, hi.checked_mul(*max)?))
        }
        Node::Quantifier { max: None, .. }
        | Node::Backreference(_)
        | Node::NamedBackreference(_)
        | Node::GraphemeCluster
        | Node::Linebreak
        | Node::Subroutine(_) => None,
        Node::Conditional { yes, no, .. } => {
            let (yes_min, yes_max) = node_length_range(yes)?;
            let (no_min, no_max) = match no.as_deref() {
                Some(no) => node_length_range(no)?,
                None => (0, 0),
            };
            Some((yes_min.min(no_min), yes_max.max(no_max)))
        }
        Node::Quoted(chars) => Some((chars.len(), chars.len())),
    }
}

fn match_seq_from(node: &Node, rest: &[Node], pos: usize, ctx: &mut MatchCtx) -> Option<usize> {
    if ctx.budget.has_error() {
        return None;
    }
    match node {
        Node::Sequence(nodes) => {
            // Flatten: match first element with rest = remaining + outer rest
            if nodes.is_empty() {
                return match_rest(rest, pos, ctx);
            }
            if nodes.len() == 1 {
                return match_seq_from(&nodes[0], rest, pos, ctx);
            }
            if rest.is_empty() {
                return match_seq_from(&nodes[0], &nodes[1..], pos, ctx);
            }
            // Build combined rest: nodes[1..] ++ rest
            let mut combined: Vec<Node> = nodes[1..].to_vec();
            combined.extend_from_slice(rest);
            match_seq_from(&nodes[0], &combined, pos, ctx)
        }
        Node::Literal(ch) => {
            if pos >= ctx.chars.len() {
                return None;
            }
            let matches = chars_equal(ctx.chars[pos], *ch, ctx.flags);
            if matches {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::AnyChar => {
            if pos >= ctx.chars.len() {
                return None;
            }
            if ctx.flags.dotall
                || newline_len_at(ctx.chars, pos, ctx.flags.line_options.newline()).is_none()
            {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::ByteUnit => {
            if pos < ctx.chars.len() {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::NotNewline => {
            if pos < ctx.chars.len()
                && newline_len_at(ctx.chars, pos, ctx.flags.line_options.newline()).is_none()
            {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::Start) => {
            let ok = if ctx.flags.multiline {
                pos == 0 || newline_ends_at(ctx.chars, pos, ctx.flags.line_options.newline())
            } else {
                pos == 0
            };
            if ok { match_rest(rest, pos, ctx) } else { None }
        }
        Node::Anchor(Anchor::AbsoluteStart) => {
            if pos == 0 {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::AbsoluteEnd) => {
            if pos == ctx.chars.len() {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::FinalEnd) => {
            if final_end_anchor_matches(pos, ctx.chars, ctx.flags) {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::SearchStart) => {
            if pos == ctx.search_start {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::End) => {
            let ok = end_anchor_matches(pos, ctx.chars, ctx.flags);
            if ok { match_rest(rest, pos, ctx) } else { None }
        }
        Node::WordBoundary(positive) => {
            let at_boundary = is_word_boundary(ctx.chars, pos, ctx.flags.unicode_mode.ucp());
            if at_boundary == *positive {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::CharClass { negated, items } => {
            if pos >= ctx.chars.len() {
                return None;
            }
            let c = ctx.chars[pos];
            let in_class = items
                .iter()
                .any(|item| match_class_item(item, c, ctx.flags));
            if in_class != *negated {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::Shorthand(sh) => {
            if pos >= ctx.chars.len() {
                return None;
            }
            if match_shorthand(*sh, ctx.chars[pos], ctx.flags.unicode_mode.ucp()) {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::GraphemeCluster => {
            let length = unicode::grapheme_cluster_len(ctx.chars, pos)?;
            match_rest(rest, pos + length, ctx)
        }
        Node::Linebreak => {
            let first = *ctx.chars.get(pos)?;
            let length = if first == '\r' && ctx.chars.get(pos + 1) == Some(&'\n') {
                2
            } else if matches!(first, '\n' | '\r')
                || (!ctx.flags.line_options.bsr_anycrlf()
                    && matches!(
                        first,
                        '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
                    ))
            {
                1
            } else {
                return None;
            };
            match_rest(rest, pos + length, ctx)
        }
        Node::LocalFlags { flags, inner } => {
            let saved = ctx.flags;
            ctx.flags.case_insensitive = flags.case_insensitive;
            ctx.flags.multiline = flags.multiline;
            ctx.flags.dotall = flags.dotall;
            let end = match_seq_from(inner, &[], pos, ctx);
            ctx.flags = saved;
            end.and_then(|end| match_rest(rest, end, ctx))
        }
        Node::Alternation(branches) => {
            for (index, branch) in branches.iter().enumerate() {
                if index != 0 && !ctx.budget.consume_alternative() {
                    return None;
                }
                let saved_groups = ctx.groups.clone();
                let saved_mark = ctx.mark.clone();
                let saved_mark_positions = ctx.mark_positions.clone();
                if let Some(end) = match_seq_from(branch, rest, pos, ctx) {
                    return Some(end);
                }
                if matches!(*ctx.control, Some(MatchControl::Then)) {
                    *ctx.control = None;
                } else if ctx.control.is_some() {
                    return None;
                }
                *ctx.groups = saved_groups;
                *ctx.mark = saved_mark;
                *ctx.mark_positions = saved_mark_positions;
            }
            None
        }
        Node::Group {
            index,
            name: _,
            inner,
        } => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            let tracked_index = index.filter(|idx| *idx < ctx.groups.len());
            let start_offset = tracked_index.map_or(0, |_| ctx.metadata.byte_offsets.get(pos));
            let result =
                match_seq_from_with_group(inner, rest, pos, ctx, tracked_index, start_offset);
            if matches!(*ctx.control, Some(MatchControl::Accept))
                && let (Some(index), Some(end)) = (tracked_index, result)
            {
                ctx.groups[index] = Some(Match {
                    start: start_offset,
                    end: ctx.metadata.byte_offsets.get(end),
                });
            }
            ctx.budget.leave_recursion();
            result
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            possessive,
        } => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            let result = match_quantifier(inner, *min, *max, *greedy, *possessive, rest, pos, ctx);
            ctx.budget.leave_recursion();
            result
        }
        Node::Backreference(n) => match_backref_by_index(*n, rest, pos, ctx),
        Node::NamedBackreference(name) => {
            if let Some(idx) = first_set_named_group(ctx.symbols, ctx.groups, name.as_str()) {
                match_backref_by_index(idx, rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Lookahead {
            positive,
            atomic,
            inner,
        } => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            if *positive && !*atomic {
                let result = match_non_atomic_lookahead(inner, rest, pos, ctx);
                ctx.budget.leave_recursion();
                return result;
            }
            let saved = ctx.groups.clone();
            let saved_mark = ctx.mark.clone();
            let saved_mark_positions = ctx.mark_positions.clone();
            // Use empty rest — lookahead doesn't consume input, just checks
            let result = match_seq_from(inner, &[], pos, ctx);
            // Every control verb is contained by an assertion. ACCEPT makes
            // a positive assertion succeed immediately; the remaining verbs
            // make it fail when they are triggered by backtracking.
            ctx.control.take();
            let result = if *positive {
                if result.is_some() {
                    // Keep captures from inside lookahead (PHP behavior)
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    *ctx.mark_positions = saved_mark_positions;
                    None
                }
            } else {
                if result.is_none() {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    *ctx.mark_positions = saved_mark_positions;
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    *ctx.mark_positions = saved_mark_positions;
                    None
                }
            };
            ctx.budget.leave_recursion();
            result
        }
        Node::Lookbehind {
            positive,
            atomic,
            inner,
        } => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            if *positive && !*atomic {
                let result = match_non_atomic_lookbehind(inner, rest, pos, ctx);
                ctx.budget.leave_recursion();
                return result;
            }
            // Try matching inner ending at `pos`. PCRE lookbehinds have a
            // bounded length, so only starts within that window can succeed;
            // scanning back to the subject start made every lookbehind O(n).
            let (shortest, longest) = node_length_range(inner).unwrap_or((0, pos));
            let earliest = pos.saturating_sub(longest);
            let latest = pos.saturating_sub(shortest);
            let found = earliest <= latest
                && (earliest..=latest).rev().any(|start| {
                    let saved = ctx.groups.clone();
                    let saved_mark = ctx.mark.clone();
                    let saved_mark_positions = ctx.mark_positions.clone();
                    let result = match_seq_from(inner, &[], start, ctx);
                    let accepted = matches!(ctx.control.take(), Some(MatchControl::Accept));
                    if accepted || result == Some(pos) {
                        true
                    } else {
                        *ctx.groups = saved;
                        *ctx.mark = saved_mark;
                        *ctx.mark_positions = saved_mark_positions;
                        false
                    }
                });
            let result = if found == *positive {
                match_rest(rest, pos, ctx)
            } else {
                None
            };
            ctx.budget.leave_recursion();
            result
        }
        Node::Atomic(inner) => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            let result =
                match_seq_from(inner, &[], pos, ctx).and_then(|end| match_rest(rest, end, ctx));
            ctx.budget.leave_recursion();
            result
        }
        Node::ScriptRun { inner, atomic } => {
            if !ctx.budget.enter_recursion() {
                return None;
            }
            let result = match_script_run(inner, *atomic, rest, pos, ctx);
            ctx.budget.leave_recursion();
            result
        }
        Node::Mark(name) => {
            let saved = ctx.mark.clone();
            let saved_positions = ctx.mark_positions.clone();
            *ctx.mark = Some(name.clone());
            ctx.mark_positions
                .get_or_insert_with(Vec::new)
                .push((name.clone(), pos));
            let result = match_rest(rest, pos, ctx);
            if result.is_none() {
                *ctx.mark = saved;
                *ctx.mark_positions = saved_positions;
            }
            result
        }
        Node::Control { verb, name } => {
            if let Some(name) = name {
                *ctx.mark = Some(name.clone());
            }
            match verb {
                ControlVerb::Fail => None,
                ControlVerb::Accept => {
                    *ctx.control = Some(MatchControl::Accept);
                    Some(pos)
                }
                ControlVerb::Commit
                | ControlVerb::Prune
                | ControlVerb::Skip
                | ControlVerb::Then => {
                    let result = match_rest(rest, pos, ctx);
                    if result.is_none() && ctx.control.is_none() {
                        *ctx.control = Some(match verb {
                            ControlVerb::Commit => MatchControl::Commit,
                            ControlVerb::Prune => MatchControl::Prune,
                            ControlVerb::Skip => {
                                if let Some(name) = name {
                                    let Some(position) =
                                        ctx.mark_positions.as_deref().and_then(|marks| {
                                            marks.iter().rev().find_map(|(mark, position)| {
                                                (mark == name).then_some(*position)
                                            })
                                        })
                                    else {
                                        return None;
                                    };
                                    MatchControl::Skip(position)
                                } else {
                                    MatchControl::Skip(pos)
                                }
                            }
                            ControlVerb::Then => MatchControl::Then,
                            ControlVerb::Fail | ControlVerb::Accept => unreachable!(),
                        });
                    }
                    result
                }
            }
        }
        Node::ResetStart => {
            let Some(slot) = ctx.groups.get_mut(0) else {
                return match_rest(rest, pos, ctx);
            };
            let saved = slot.clone();
            let offset = ctx.metadata.byte_offsets.get(pos);
            *slot = Some(Match {
                start: offset,
                end: offset,
            });
            let result = match_rest(rest, pos, ctx);
            if result.is_none() {
                ctx.groups[0] = saved;
            }
            result
        }
        Node::Subroutine(target) => match_subroutine(target, rest, pos, ctx),
        Node::Conditional { condition, yes, no } => {
            match_conditional(condition, yes, no.as_deref(), rest, pos, ctx)
        }
        Node::CaptureEnd { index, start } => match_capture_end(*index, *start, rest, pos, ctx),
        Node::CaptureRestore { groups } => {
            let callee_groups = std::mem::replace(ctx.groups, groups.as_ref().clone());
            let result = match_rest(rest, pos, ctx);
            if result.is_none() {
                *ctx.groups = callee_groups;
            }
            result
        }
        Node::Quoted(_) => unreachable!("quoted literals are lowered by the parser"),
    }
}

#[inline(never)]
fn match_capture_end(
    index: usize,
    start: usize,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let previous = ctx.groups.get(index).cloned().unwrap_or(None);
    if let Some(slot) = ctx.groups.get_mut(index) {
        *slot = Some(Match {
            start,
            end: ctx.metadata.byte_offsets.get(pos),
        });
    }
    let result = match_rest(rest, pos, ctx);
    if result.is_none()
        && let Some(slot) = ctx.groups.get_mut(index)
    {
        *slot = previous;
    }
    result
}

/// Helper: match a group node, setting the group capture after the inner match succeeds
/// and before matching the rest.
fn match_seq_from_with_group(
    inner: &Node,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx,
    group_idx: Option<usize>,
    start_offset: usize,
) -> Option<usize> {
    // A terminal group has no continuation that could reject its preferred
    // inner match. Let the ordinary matcher consume that path directly; this
    // keeps the common `(prefix)(digits+)` capture shape allocation-free.
    if rest.is_empty() {
        let end = match_seq_from(inner, &[], pos, ctx)?;
        if let Some(idx) = group_idx {
            ctx.groups[idx] = Some(Match {
                start: start_offset,
                end: ctx.metadata.byte_offsets.get(end),
            });
        }
        return Some(end);
    }

    let Some(index) = group_idx else {
        return match_seq_from(inner, rest, pos, ctx);
    };
    if !capture_needs_streaming_continuation(inner) {
        let (initial, states) = collect_match_states(inner, pos, ctx);
        for state in states {
            let end = state.end;
            state.install(ctx);
            ctx.groups[index] = Some(Match {
                start: start_offset,
                end: ctx.metadata.byte_offsets.get(end),
            });
            if let Some(final_pos) = match_rest(rest, end, ctx) {
                return Some(final_pos);
            }
        }
        initial.install(ctx);
        return None;
    }
    let mut continuation = Vec::with_capacity(rest.len() + 1);
    continuation.push(Node::CaptureEnd {
        index,
        start: start_offset,
    });
    continuation.extend_from_slice(rest);
    match_seq_from(inner, &continuation, pos, ctx)
}

/// One possible matcher continuation. Capture registers and the last MARK are
/// part of the state: two paths ending at the same subject position are not
/// interchangeable when PHP later publishes their captures.
#[derive(Clone, PartialEq, Eq, Hash)]
struct BacktrackState {
    end: usize,
    groups: Vec<Option<Match>>,
    mark: Option<String>,
    mark_positions: Option<Vec<(String, usize)>>,
}

impl BacktrackState {
    fn take(end: usize, ctx: &mut MatchCtx<'_>) -> Self {
        Self {
            end,
            groups: std::mem::take(ctx.groups),
            mark: ctx.mark.take(),
            mark_positions: ctx.mark_positions.take(),
        }
    }

    fn install(self, ctx: &mut MatchCtx<'_>) {
        *ctx.groups = self.groups;
        *ctx.mark = self.mark;
        *ctx.mark_positions = self.mark_positions;
    }
}

fn collect_match_states(
    node: &Node,
    pos: usize,
    ctx: &mut MatchCtx,
) -> (BacktrackState, Vec<BacktrackState>) {
    let initial = BacktrackState::take(pos, ctx);
    let states = collect_match_states_from(node, initial.clone(), ctx);
    (initial, states)
}

/// PHP's global preg loops retry an empty match once at the same offset with
/// PCRE2_NOTEMPTY_ATSTART|PCRE2_ANCHORED. This lets a later non-empty
/// alternative match before the loop advances one character.
fn match_nonempty_at_start(node: &Node, pos: usize, ctx: &mut MatchCtx<'_>) -> Option<usize> {
    let (initial, states) = collect_match_states(node, pos, ctx);
    for state in states {
        if state.end == pos {
            continue;
        }
        let end = state.end;
        state.install(ctx);
        return Some(end);
    }
    initial.install(ctx);
    None
}

fn capture_needs_streaming_continuation(node: &Node) -> bool {
    match node {
        Node::Sequence(nodes) | Node::Alternation(nodes) => {
            nodes.iter().any(capture_needs_streaming_continuation)
        }
        Node::Quantifier { inner, .. } => {
            match_states_can_branch(inner) || capture_needs_streaming_continuation(inner)
        }
        Node::Group { inner, .. }
        | Node::Lookahead { inner, .. }
        | Node::Lookbehind { inner, .. }
        | Node::Atomic(inner) => capture_needs_streaming_continuation(inner),
        Node::ScriptRun { .. } | Node::Subroutine(_) | Node::Conditional { .. } => true,
        _ => false,
    }
}

fn collect_match_states_from(
    node: &Node,
    state: BacktrackState,
    ctx: &mut MatchCtx,
) -> Vec<BacktrackState> {
    if ctx.budget.has_error() {
        return Vec::new();
    }
    match node {
        Node::Sequence(nodes) => {
            let mut states = vec![state];
            for node in nodes {
                let mut next = Vec::new();
                for state in states {
                    next.extend(collect_match_states_from(node, state, ctx));
                }
                states = next;
                if states.is_empty() || matches!(*ctx.control, Some(MatchControl::Accept)) {
                    break;
                }
            }
            states
        }
        Node::Alternation(branches) => {
            let mut states = Vec::new();
            let last = branches.len().saturating_sub(1);
            let mut initial = Some(state);
            for (index, branch) in branches.iter().enumerate() {
                if index != 0 && !ctx.budget.consume_alternative() {
                    break;
                }
                let branch_state = if index == last {
                    initial.take().unwrap()
                } else {
                    initial.as_ref().unwrap().clone()
                };
                states.extend(collect_match_states_from(branch, branch_state, ctx));
            }
            states
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            possessive,
        } => {
            if !ctx.budget.enter_recursion() {
                return Vec::new();
            }
            let states = if match_states_can_branch(inner) {
                collect_branching_quantifier_states(
                    inner,
                    *min,
                    *max,
                    *greedy,
                    *possessive,
                    state,
                    ctx,
                )
            } else {
                collect_single_path_quantifier_states(
                    inner,
                    *min,
                    *max,
                    *greedy,
                    *possessive,
                    state,
                    ctx,
                )
            };
            ctx.budget.leave_recursion();
            states
        }
        Node::Group {
            index,
            name: _,
            inner,
        } => {
            if !ctx.budget.enter_recursion() {
                return Vec::new();
            }
            let pos = state.end;
            let tracked_index = index.filter(|idx| *idx < state.groups.len());
            let start_offset = tracked_index.map_or(0, |_| ctx.metadata.byte_offsets.get(pos));
            let mut states = collect_match_states_from(inner, state, ctx);
            for state in &mut states {
                if let Some(idx) = tracked_index {
                    let end_offset = ctx.metadata.byte_offsets.get(state.end);
                    state.groups[idx] = Some(Match {
                        start: start_offset,
                        end: end_offset,
                    });
                }
            }
            ctx.budget.leave_recursion();
            states
        }
        Node::Subroutine(target) => collect_subroutine_states(target, state, ctx),
        Node::Conditional { condition, yes, no } => {
            collect_conditional_states(condition, yes, no.as_deref(), state, ctx)
        }
        // For simple nodes, delegate to match_seq_from with empty rest
        _ => {
            let pos = state.end;
            state.install(ctx);
            if let Some(end) = match_seq_from(node, &[], pos, ctx) {
                vec![BacktrackState::take(end, ctx)]
            } else {
                Vec::new()
            }
        }
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn collect_subroutine_states(
    target: &SubroutineTarget,
    state: BacktrackState,
    ctx: &mut MatchCtx<'_>,
) -> Vec<BacktrackState> {
    let start = state.end;
    if subroutine_call_is_active(ctx, target, start) || !ctx.budget.enter_recursion() {
        return Vec::new();
    }
    let target_kind = target.clone();
    let target = resolve_subroutine(target, ctx);
    let caller_groups = state.groups.clone();
    push_subroutine_call(ctx, target_kind, start);
    let mut states = target.map_or_else(Vec::new, |target| {
        collect_match_states_from(&target, state, ctx)
    });
    for state in &mut states {
        state.groups.clone_from(&caller_groups);
    }
    pop_subroutine_call(ctx);
    ctx.budget.leave_recursion();
    states
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn collect_conditional_states(
    condition: &CaptureCondition,
    yes: &Node,
    no: Option<&Node>,
    state: BacktrackState,
    ctx: &mut MatchCtx<'_>,
) -> Vec<BacktrackState> {
    let pos = state.end;
    state.install(ctx);
    let selected = if capture_condition_matches(condition, pos, ctx) {
        Some(yes)
    } else {
        no
    };
    let state = BacktrackState::take(pos, ctx);
    match selected {
        Some(selected) => collect_match_states_from(selected, state, ctx),
        None => vec![state],
    }
}

/// Whether collecting this node can publish more than one continuation from
/// one input state. Keeping the common single-atom quantifier on a dedicated
/// path avoids allocating a convergence set for ordinary capture patterns.
fn match_states_can_branch(node: &Node) -> bool {
    match node {
        Node::Sequence(nodes) => nodes.iter().any(match_states_can_branch),
        Node::Alternation(branches) => {
            branches.len() > 1 || branches.iter().any(match_states_can_branch)
        }
        Node::Quantifier { .. } => true,
        Node::Group { inner, .. } => match_states_can_branch(inner),
        Node::ScriptRun { .. } | Node::Subroutine(_) | Node::Conditional { .. } => true,
        _ => false,
    }
}

/// Return whether consecutive selections of this alternation branch can be
/// folded into one selection while computing the reachable states of an
/// enclosing unbounded quantifier.  `A* A*` is the same language as `A*`, and
/// `A+ A+` adds no endpoint that `A+` did not already publish.  Restrict the
/// proof to side-effect-free single-character atoms so captures, MARKs and
/// assertions retain their exact PCRE ordering.
fn absorbable_repeated_branch(node: &Node) -> bool {
    let Node::Quantifier {
        inner,
        min,
        max: None,
        possessive: false,
        ..
    } = node
    else {
        return false;
    };
    if *min > 1 {
        return false;
    }
    matches!(
        inner.as_ref(),
        Node::Literal(_)
            | Node::AnyChar
            | Node::CharClass { .. }
            | Node::Shorthand(_)
            | Node::GraphemeCluster
            | Node::Linebreak
    )
}

/// Return the atom from a greedy, non-possessive `atom+` branch. Repeating
/// this branch inside an outer `*` admits every partition of one homogeneous
/// run, which is the classic PCRE exponential backtracking shape.
fn partitionable_plus_atom(node: &Node) -> Option<&Node> {
    match node {
        Node::Quantifier {
            inner,
            min: 1,
            max: None,
            greedy: true,
            possessive: false,
        } if matches!(
            inner.as_ref(),
            Node::Literal(_) | Node::AnyChar | Node::CharClass { .. } | Node::Shorthand(_)
        ) =>
        {
            Some(inner)
        }
        Node::Group {
            index: None, inner, ..
        } => partitionable_plus_atom(inner),
        _ => None,
    }
}

fn first_required_consuming_node(node: &Node) -> Option<&Node> {
    match node {
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::CharClass { .. }
        | Node::Shorthand(_)
        | Node::GraphemeCluster
        | Node::Linebreak => Some(node),
        Node::Sequence(nodes) => {
            for node in nodes {
                if let Some(node) = first_required_consuming_node(node) {
                    return Some(node);
                }
                if !matches!(
                    node,
                    Node::Anchor(_)
                        | Node::WordBoundary(_)
                        | Node::Lookahead { .. }
                        | Node::Lookbehind { .. }
                        | Node::Mark(_)
                        | Node::ResetStart
                        | Node::CaptureEnd { .. }
                        | Node::CaptureRestore { .. }
                ) {
                    return None;
                }
            }
            None
        }
        Node::Group { inner, .. } | Node::Atomic(inner) | Node::ScriptRun { inner, .. } => {
            first_required_consuming_node(inner)
        }
        Node::Anchor(_)
        | Node::WordBoundary(_)
        | Node::Lookahead { .. }
        | Node::Lookbehind { .. }
        | Node::Mark(_)
        | Node::ResetStart
        | Node::CaptureEnd { .. }
        | Node::CaptureRestore { .. } => None,
        // Optional repetitions, local option changes, conditionals and calls
        // need the full matcher to identify their first required character.
        _ => None,
    }
}

fn first_required_consuming_rest(rest: &[Node]) -> Option<&Node> {
    for node in rest {
        if let Some(node) = first_required_consuming_node(node) {
            return Some(node);
        }
        if !matches!(
            node,
            Node::Anchor(_)
                | Node::WordBoundary(_)
                | Node::Lookahead { .. }
                | Node::Lookbehind { .. }
                | Node::Mark(_)
                | Node::ResetStart
                | Node::CaptureEnd { .. }
                | Node::CaptureRestore { .. }
        ) {
            return None;
        }
    }
    None
}

fn single_atom_matches(atom: &Node, candidate: char, flags: RegexFlags) -> bool {
    match atom {
        Node::Literal(literal) => chars_equal(*literal, candidate, flags),
        Node::AnyChar => {
            flags.dotall || newline_len_at(&[candidate], 0, flags.line_options.newline()).is_none()
        }
        Node::ByteUnit => candidate as u32 <= u8::MAX as u32,
        Node::NotNewline => newline_len_at(&[candidate], 0, flags.line_options.newline()).is_none(),
        Node::CharClass { negated, items } => {
            items
                .iter()
                .any(|item| match_class_item(item, candidate, flags))
                != *negated
        }
        Node::Shorthand(shorthand) => {
            match_shorthand(*shorthand, candidate, flags.unicode_mode.ucp())
        }
        Node::GraphemeCluster => true,
        Node::Linebreak => matches!(
            candidate,
            '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ),
        _ => false,
    }
}

/// Prove overlap by enumerating a finite positive side. Unknown intersections
/// stay conservative: returning true may skip this cold optimization, while a
/// false result is used to model PCRE's auto-possessification boundary.
fn single_atoms_may_overlap(left: &Node, right: &Node, flags: RegexFlags) -> bool {
    if let Node::Literal(candidate) = left {
        return single_atom_matches(right, *candidate, flags);
    }
    if let Node::Literal(candidate) = right {
        return single_atom_matches(left, *candidate, flags);
    }

    let finite_candidates = |node: &Node| -> Option<Vec<char>> {
        let Node::CharClass {
            negated: false,
            items,
        } = node
        else {
            return None;
        };
        let mut candidates = Vec::new();
        for item in items {
            match item {
                ClassItem::Literal(candidate) => candidates.push(*candidate),
                ClassItem::Range(start, end) => {
                    let width = (*end as u32).saturating_sub(*start as u32);
                    if width > 255 {
                        return None;
                    }
                    candidates.extend((*start as u32..=*end as u32).filter_map(char::from_u32));
                }
                _ => return None,
            }
        }
        Some(candidates)
    };

    if let Some(candidates) = finite_candidates(left) {
        return candidates
            .into_iter()
            .any(|candidate| single_atom_matches(right, candidate, flags));
    }
    if let Some(candidates) = finite_candidates(right) {
        return candidates
            .into_iter()
            .any(|candidate| single_atom_matches(left, candidate, flags));
    }
    true
}

/// If an outer `*` repeats an overlapping `atom+` and its continuation cannot
/// start anywhere in the remaining subject, PCRE explores every partition of
/// the homogeneous run before reporting no match. Its exact match-call count
/// for a run of length `n` is `2^(n+1)-1`. The native engine deduplicates those
/// equivalent states, so account for the eliminated paths without allocating
/// them; this preserves `pcre.backtrack_limit` while retaining linear memory.
fn impossible_partition_search_cost(
    inner: &Node,
    rest: &[Node],
    pos: usize,
    ctx: &MatchCtx<'_>,
) -> Option<usize> {
    let continuation = first_required_consuming_rest(rest)?;
    if (pos..ctx.chars.len()).any(|candidate| branch_may_start_at(continuation, candidate, ctx)) {
        return None;
    }

    let inner = match inner {
        Node::Group {
            index: None, inner, ..
        } => inner.as_ref(),
        _ => inner,
    };
    let branches: &[Node] = match inner {
        Node::Alternation(branches) => branches,
        _ => std::slice::from_ref(inner),
    };
    let mut recognized_viable_branch = false;
    let mut overlapping_run_length = 0usize;
    for branch in branches {
        let Some(atom) = partitionable_plus_atom(branch) else {
            // A branch that cannot start here is irrelevant to this failed
            // run. A viable shape we do not recognize must retain the full
            // canonical matcher instead of guessing its partition cost.
            if branch_may_start_at(branch, pos, ctx) {
                return None;
            }
            continue;
        };
        let run_length = ctx.chars[pos..]
            .iter()
            .take_while(|candidate| single_atom_matches(atom, **candidate, ctx.flags))
            .count();
        if run_length == 0 {
            continue;
        }
        recognized_viable_branch = true;
        if single_atoms_may_overlap(atom, continuation, ctx.flags) {
            overlapping_run_length = overlapping_run_length.max(run_length);
        }
    }
    if !recognized_viable_branch {
        return None;
    }
    // PCRE auto-possessifies a repeated atom whose language cannot overlap the
    // continuation. Since that continuation was already proven impossible,
    // the result is a no-match without consuming the public backtrack budget.
    if overlapping_run_length == 0 {
        return Some(0);
    }
    let exponent = u32::try_from(overlapping_run_length.saturating_add(1)).unwrap_or(u32::MAX);
    Some(
        1usize
            .checked_shl(exponent)
            .map_or(usize::MAX, |paths| paths - 1),
    )
}

/// Cheap, conservative first-token proof used before expanding an
/// alternation branch into backtracking states. Returning `true` is allowed
/// for an unknown/zero-width shape; returning `false` means the branch cannot
/// possibly start at this subject position.
fn branch_may_start_at(node: &Node, pos: usize, ctx: &MatchCtx<'_>) -> bool {
    match node {
        Node::Literal(literal) => ctx
            .chars
            .get(pos)
            .is_some_and(|candidate| chars_equal(*candidate, *literal, ctx.flags)),
        Node::AnyChar => ctx.chars.get(pos).is_some_and(|_| {
            ctx.flags.dotall
                || newline_len_at(ctx.chars, pos, ctx.flags.line_options.newline()).is_none()
        }),
        Node::CharClass { negated, items } => ctx.chars.get(pos).is_some_and(|candidate| {
            items
                .iter()
                .any(|item| match_class_item(item, *candidate, ctx.flags))
                != *negated
        }),
        Node::Shorthand(shorthand) => ctx.chars.get(pos).is_some_and(|candidate| {
            match_shorthand(*shorthand, *candidate, ctx.flags.unicode_mode.ucp())
        }),
        Node::GraphemeCluster | Node::Linebreak => pos < ctx.chars.len(),
        Node::Sequence(nodes) => nodes
            .first()
            .is_none_or(|first| branch_may_start_at(first, pos, ctx)),
        Node::Group { inner, .. } | Node::Atomic(inner) => branch_may_start_at(inner, pos, ctx),
        Node::Alternation(branches) => branches
            .iter()
            .any(|branch| branch_may_start_at(branch, pos, ctx)),
        Node::Quantifier { inner, min, .. } => *min == 0 || branch_may_start_at(inner, pos, ctx),
        // Anchors, lookarounds, conditionals, backreferences and subroutines
        // need runtime state. Keep them conservative.
        _ => true,
    }
}

fn collect_single_path_quantifier_states(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    possessive: bool,
    state: BacktrackState,
    ctx: &mut MatchCtx,
) -> Vec<BacktrackState> {
    let limit = max.unwrap_or(usize::MAX);
    let mut repetitions = Vec::new();
    let mut current_reps = 0usize;
    let mut current = state;
    loop {
        if current_reps >= min {
            repetitions.push(current.clone());
        }
        if current_reps >= limit || !ctx.budget.retain_quantifier_continuation() {
            break;
        }
        let current_end = current.end;
        let mut next = collect_match_states_from(inner, current, ctx);
        let Some(next) = next.pop() else {
            break;
        };
        current_reps += 1;
        current = next;
        if current.end == current_end {
            if current_reps >= min && (greedy || current_reps == min) {
                repetitions.push(current.clone());
            }
            break;
        }
    }
    if greedy {
        repetitions.reverse();
    }
    if possessive {
        repetitions.truncate(1);
    }
    repetitions
}

/// Nested alternatives and quantifiers can reach the same continuation
/// through exponentially many partitions. Once repetition count, subject
/// position, captures and MARK agree, every future decision is identical.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn collect_branching_quantifier_states(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    possessive: bool,
    state: BacktrackState,
    ctx: &mut MatchCtx,
) -> Vec<BacktrackState> {
    let mut repetitions = Vec::new();
    let limit = max.unwrap_or(usize::MAX);
    // For an unbounded 0/1-minimum repetition, remember the last absorbable
    // alternation branch.  Re-selecting the same `A*`/`A+` branch cannot reach
    // a new endpoint and is the source of quadratic (and, when nested,
    // exponential) partition enumeration in patterns such as
    // `(?:[^]]*|\[(?:...)*\])*`.  A different branch clears the marker, so
    // delimiter-crossing nested alternatives remain fully backtrackable.
    let collapse_repeated_branches = max.is_none() && min <= 1;
    let mut pending = vec![(0usize, state, None::<usize>)];
    let mut seen = HashSet::new();
    while let Some((current_reps, state, blocked_branch)) = pending.pop() {
        if current_reps >= min {
            repetitions.push((current_reps, state.clone()));
        }
        if current_reps >= limit {
            continue;
        }
        if !ctx.budget.retain_quantifier_continuation() {
            break;
        }
        let current_end = state.end;
        let mut next_states = Vec::new();
        match inner {
            Node::Alternation(branches) if collapse_repeated_branches => {
                for (branch_index, branch) in branches.iter().enumerate() {
                    let absorbable = absorbable_repeated_branch(branch);
                    if absorbable && blocked_branch == Some(branch_index) {
                        continue;
                    }
                    if !branch_may_start_at(branch, state.end, ctx) {
                        continue;
                    }
                    if branch_index != 0 && !ctx.budget.consume_alternative() {
                        break;
                    }
                    let branch_state = state.clone();
                    next_states.extend(
                        collect_match_states_from(branch, branch_state, ctx)
                            .into_iter()
                            .map(|next| (next, absorbable.then_some(branch_index))),
                    );
                }
            }
            _ => next_states.extend(
                collect_match_states_from(inner, state, ctx)
                    .into_iter()
                    .map(|next| (next, None)),
            ),
        }
        for (next, next_blocked_branch) in next_states.into_iter().rev() {
            if next.end == current_end {
                let next_reps = current_reps + 1;
                if next_reps >= min && (greedy || next_reps == min) {
                    repetitions.push((next_reps, next));
                }
                continue;
            }
            let next_reps = current_reps + 1;
            let equivalent_reps = if max.is_none() && next_reps >= min {
                min
            } else {
                next_reps
            };
            let continuation = (next_reps, next, next_blocked_branch);
            if seen.insert((equivalent_reps, continuation.1.clone(), continuation.2)) {
                pending.push(continuation);
            }
        }
    }
    if greedy {
        repetitions.sort_by(|a, b| b.1.end.cmp(&a.1.end).then_with(|| a.0.cmp(&b.0)));
    } else {
        repetitions.sort_by(|a, b| a.1.end.cmp(&b.1.end).then_with(|| b.0.cmp(&a.0)));
    }
    if possessive {
        repetitions.truncate(1);
    }
    repetitions.into_iter().map(|(_, state)| state).collect()
}

/// Match remaining nodes in the rest slice.
fn match_rest(rest: &[Node], pos: usize, ctx: &mut MatchCtx) -> Option<usize> {
    if matches!(*ctx.control, Some(MatchControl::Accept)) {
        return Some(pos);
    }
    if rest.is_empty() {
        return Some(pos);
    }
    match_seq_from(&rest[0], &rest[1..], pos, ctx)
}

fn match_backref_by_index(
    n: usize,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx,
) -> Option<usize> {
    if let Some(Some(m)) = ctx.groups.get(n).cloned() {
        let captured = &ctx.metadata.input[m.start..m.end];
        let cap_chars: Vec<char> = captured.chars().collect();
        if pos + cap_chars.len() > ctx.chars.len() {
            return None;
        }
        for (i, &cc) in cap_chars.iter().enumerate() {
            let matches = chars_equal(ctx.chars[pos + i], cc, ctx.flags);
            if !matches {
                return None;
            }
        }
        match_rest(rest, pos + cap_chars.len(), ctx)
    } else {
        None
    }
}

/// Recognize the common PCRE construction used to match delimiters nested to
/// a fixed depth:
///
/// `(?: [^{}]* | \{ (?: [^{}]* | \{ ... \} )* \} )*`
///
/// The expression is regular, capture-free and deterministic with respect to
/// delimiter depth. A generic backtracker otherwise rediscovers every
/// partition of each `[^{}]*` suffix. Return the delimiter pair and admitted
/// nesting depth when the AST proves this exact shape.
fn balanced_delimiter_spec(node: &Node) -> Option<(char, char, usize, bool)> {
    let Node::Alternation(branches) = node else {
        return None;
    };
    let [plain, nested] = branches.as_slice() else {
        return None;
    };
    let Node::Sequence(nodes) = nested else {
        return None;
    };
    let [
        Node::Literal(nested_open),
        content,
        Node::Literal(nested_close),
    ] = nodes.as_slice()
    else {
        return None;
    };
    let (open, close) = (*nested_open, *nested_close);
    let plain_allows_open = delimiter_excluding_star(plain, open, close)?;

    let nested_depth = if delimiter_excluding_star(content, open, close) == Some(plain_allows_open)
    {
        0
    } else {
        let Node::Quantifier {
            inner,
            min: 0,
            max: None,
            greedy: true,
            possessive: false,
        } = content
        else {
            return None;
        };
        let (inner_open, inner_close, depth, inner_allows_open) = balanced_delimiter_spec(inner)?;
        if inner_open != open || inner_close != close || inner_allows_open != plain_allows_open {
            return None;
        }
        depth
    };
    Some((open, close, nested_depth + 1, plain_allows_open))
}

fn delimiter_excluding_star(node: &Node, open: char, close: char) -> Option<bool> {
    let Node::Quantifier {
        inner,
        min: 0,
        max: None,
        greedy: true,
        possessive: false,
    } = node
    else {
        return None;
    };
    let Node::CharClass {
        negated: true,
        items,
    } = inner.as_ref()
    else {
        return None;
    };
    let excludes_close = items
        .iter()
        .any(|item| matches!(item, ClassItem::Literal(candidate) if *candidate == close));
    if !excludes_close
        || items.iter().any(|item| {
            !matches!(item, ClassItem::Literal(candidate) if *candidate == open || *candidate == close)
        })
    {
        return None;
    }
    let excludes_open = items
        .iter()
        .any(|item| matches!(item, ClassItem::Literal(candidate) if *candidate == open));
    Some(!excludes_open)
}

fn node_is_captureless_pure(node: &Node) -> bool {
    match node {
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::Anchor(_)
        | Node::CharClass { .. }
        | Node::Shorthand(_)
        | Node::WordBoundary(_)
        | Node::GraphemeCluster
        | Node::Linebreak => true,
        Node::Sequence(nodes) | Node::Alternation(nodes) => {
            nodes.iter().all(node_is_captureless_pure)
        }
        Node::Quantifier { inner, .. } | Node::Atomic(inner) | Node::LocalFlags { inner, .. } => {
            node_is_captureless_pure(inner)
        }
        Node::Group {
            index: None, inner, ..
        } => node_is_captureless_pure(inner),
        Node::Group { index: Some(_), .. }
        | Node::Backreference(_)
        | Node::NamedBackreference(_)
        | Node::Lookahead { .. }
        | Node::Lookbehind { .. }
        | Node::ScriptRun { .. }
        | Node::Mark(_)
        | Node::Control { .. }
        | Node::ResetStart
        | Node::Subroutine(_)
        | Node::Conditional { .. }
        | Node::CaptureEnd { .. }
        | Node::CaptureRestore { .. } => false,
        Node::Quoted(_) => unreachable!("quoted literals are lowered by the parser"),
    }
}

/// Return the literal characters that every proven one-character path through
/// this node excludes. An empty set proves that the union of the paths covers
/// every character. This recognizes nested forms such as
/// `(?:[^]]*|nested|(?:[^{}]*|object)*)`, whose outer repetition is therefore
/// language-equivalent to a dot-all star even though no single branch is.
fn repeatable_single_char_exclusions(node: &Node) -> Option<Vec<char>> {
    match node {
        Node::CharClass {
            negated: true,
            items,
        } => {
            let mut excluded = Vec::with_capacity(items.len());
            for item in items {
                let ClassItem::Literal(character) = item else {
                    return None;
                };
                if !excluded.contains(character) {
                    excluded.push(*character);
                }
            }
            Some(excluded)
        }
        Node::Alternation(branches) => {
            let mut covered = branches
                .iter()
                .filter_map(repeatable_single_char_exclusions);
            let mut excluded = covered.next()?;
            for branch_excluded in covered {
                excluded.retain(|character| branch_excluded.contains(character));
            }
            Some(excluded)
        }
        Node::Quantifier {
            inner,
            min,
            max: None,
            ..
        } if *min <= 1 => repeatable_single_char_exclusions(inner),
        Node::Group {
            index: None, inner, ..
        }
        | Node::Atomic(inner) => repeatable_single_char_exclusions(inner),
        _ => None,
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_universal_quantifier(
    greedy: bool,
    possessive: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let initial_groups = ctx.groups.clone();
    let initial_mark = ctx.mark.clone();
    let initial_mark_positions = ctx.mark_positions.clone();
    let try_endpoint = |endpoint, ctx: &mut MatchCtx<'_>| {
        if !ctx.budget.consume_backtrack() {
            return None;
        }
        *ctx.groups = initial_groups.clone();
        *ctx.mark = initial_mark.clone();
        *ctx.mark_positions = initial_mark_positions.clone();
        match_rest(rest, endpoint, ctx)
    };

    if greedy {
        for endpoint in (pos..=ctx.chars.len()).rev() {
            if let Some(end) = try_endpoint(endpoint, ctx) {
                return Some(end);
            }
            if ctx.control.is_some() {
                return None;
            }
            if possessive {
                break;
            }
        }
    } else {
        for endpoint in pos..=ctx.chars.len() {
            if let Some(end) = try_endpoint(endpoint, ctx) {
                return Some(end);
            }
            if ctx.control.is_some() {
                return None;
            }
            if possessive {
                break;
            }
        }
    }
    *ctx.groups = initial_groups;
    *ctx.mark = initial_mark;
    *ctx.mark_positions = initial_mark_positions;
    None
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_balanced_delimiter_quantifier(
    open: char,
    close: char,
    max_depth: usize,
    plain_allows_open: bool,
    greedy: bool,
    possessive: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    let mut endpoints = Vec::new();
    endpoints.push(pos);
    let mut current = pos;
    let mut reachable = vec![false; max_depth + 1];
    reachable[0] = true;
    while let Some(&candidate) = ctx.chars.get(current) {
        let mut next = vec![false; max_depth + 1];
        if candidate == close {
            for depth in 1..=max_depth {
                if reachable[depth] {
                    next[depth - 1] = true;
                }
            }
        } else {
            for depth in 0..=max_depth {
                if !reachable[depth] {
                    continue;
                }
                if candidate != open || plain_allows_open {
                    next[depth] = true;
                }
                if candidate == open && depth < max_depth {
                    next[depth + 1] = true;
                }
            }
        }
        if !next.iter().any(|reachable| *reachable) {
            break;
        }
        current += 1;
        if next[0] {
            endpoints.push(current);
        }
        reachable = next;
    }

    if greedy {
        endpoints.reverse();
    }
    if possessive {
        endpoints.truncate(1);
    }
    let initial_groups = ctx.groups.clone();
    let initial_mark = ctx.mark.clone();
    let initial_mark_positions = ctx.mark_positions.clone();
    for endpoint in endpoints {
        if !ctx.budget.consume_backtrack() {
            break;
        }
        *ctx.groups = initial_groups.clone();
        *ctx.mark = initial_mark.clone();
        *ctx.mark_positions = initial_mark_positions.clone();
        if let Some(end) = match_rest(rest, endpoint, ctx) {
            return Some(end);
        }
        if ctx.control.is_some() {
            return None;
        }
    }
    *ctx.groups = initial_groups;
    *ctx.mark = initial_mark;
    *ctx.mark_positions = initial_mark_positions;
    None
}

#[inline]
fn chars_equal(left: char, right: char, flags: RegexFlags) -> bool {
    if flags.case_insensitive {
        unicode::caseless_equal(
            left,
            right,
            flags.unicode_mode.utf() || flags.unicode_mode.ucp(),
        )
    } else {
        left == right
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_control_quantifier(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx<'_>,
) -> Option<usize> {
    if !ctx.budget.enter_recursion() {
        return None;
    }
    let result = (|| {
        if max == Some(0) {
            return (min == 0).then(|| match_rest(rest, pos, ctx)).flatten();
        }

        let try_repetition = |ctx: &mut MatchCtx<'_>| {
            let next_min = min.saturating_sub(1);
            let next_max = max.map(|limit| limit.saturating_sub(1));
            let mut continuation = Vec::with_capacity(rest.len() + 1);
            if next_min != 0 || next_max != Some(0) {
                continuation.push(Node::Quantifier {
                    inner: Box::new(inner.clone()),
                    min: next_min,
                    max: next_max,
                    greedy,
                    possessive: false,
                });
            }
            continuation.extend_from_slice(rest);
            match_seq_from(inner, &continuation, pos, ctx)
        };

        if min != 0 {
            return try_repetition(ctx);
        }
        if greedy {
            if let Some(end) = try_repetition(ctx) {
                return Some(end);
            }
            if ctx.control.is_some() {
                return None;
            }
            match_rest(rest, pos, ctx)
        } else {
            if let Some(end) = match_rest(rest, pos, ctx) {
                return Some(end);
            }
            if ctx.control.is_some() {
                return None;
            }
            try_repetition(ctx)
        }
    })();
    ctx.budget.leave_recursion();
    result
}

fn match_quantifier(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    possessive: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx,
) -> Option<usize> {
    let limit = max.unwrap_or(usize::MAX);

    if min == 0
        && max.is_none()
        && greedy
        && !possessive
        && let Some(cost) = impossible_partition_search_cost(inner, rest, pos, ctx)
    {
        ctx.budget.reject_impossible_search_if_cost_exceeds(cost);
        return None;
    }

    if !possessive && contains_control_verb(inner) && node_definitely_consumes(inner) {
        return match_control_quantifier(inner, min, max, greedy, rest, pos, ctx);
    }

    if min == 0
        && max.is_none()
        && !rest.is_empty()
        && node_is_captureless_pure(inner)
        && repeatable_single_char_exclusions(inner).is_some_and(|excluded| excluded.is_empty())
    {
        return match_universal_quantifier(greedy, possessive, rest, pos, ctx);
    }

    if min == 0
        && max.is_none()
        && !rest.is_empty()
        && let Some((open, close, depth, plain_allows_open)) = balanced_delimiter_spec(inner)
    {
        return match_balanced_delimiter_quantifier(
            open,
            close,
            depth,
            plain_allows_open,
            greedy,
            possessive,
            rest,
            pos,
            ctx,
        );
    }

    // With no continuation there is nothing to backtrack into. Consume
    // directly to the greedy maximum (or lazy minimum) instead of allocating,
    // cloning and sorting every intermediate state.
    if rest.is_empty() {
        let tracks_captures = ctx.groups.len() > 1;
        let initial_groups = tracks_captures.then(|| ctx.groups.clone());
        let initial_mark = ctx.mark.clone();
        let initial_mark_positions = ctx.mark_positions.clone();
        let mut current_pos = pos;
        let mut repetitions = 0usize;

        if !greedy && min == 0 {
            return Some(pos);
        }

        while repetitions < limit {
            let saved_groups = tracks_captures.then(|| ctx.groups.clone());
            let saved_mark = ctx.mark.clone();
            let saved_mark_positions = ctx.mark_positions.clone();
            match match_seq_from(inner, &[], current_pos, ctx) {
                Some(next_pos) => {
                    if matches!(*ctx.control, Some(MatchControl::Accept)) {
                        return Some(next_pos);
                    }
                    repetitions += 1;
                    if next_pos == current_pos {
                        if !greedy
                            && repetitions > min
                            && let Some(saved_groups) = saved_groups
                        {
                            *ctx.groups = saved_groups;
                        }
                        *ctx.mark = saved_mark;
                        *ctx.mark_positions = saved_mark_positions;
                        break;
                    }
                    current_pos = next_pos;
                    if !greedy && repetitions >= min {
                        return Some(current_pos);
                    }
                }
                None => {
                    if ctx.control.is_some() {
                        return None;
                    }
                    if let Some(saved_groups) = saved_groups {
                        *ctx.groups = saved_groups;
                    }
                    *ctx.mark = saved_mark;
                    *ctx.mark_positions = saved_mark_positions;
                    break;
                }
            }
        }

        if repetitions >= min {
            if ctx.budget.has_error() {
                return None;
            }
            return Some(current_pos);
        }
        if let Some(initial_groups) = initial_groups {
            *ctx.groups = initial_groups;
        }
        *ctx.mark = initial_mark;
        *ctx.mark_positions = initial_mark_positions;
        return None;
    }

    // Compound atoms usually have one preferred result at each token. Follow
    // that PCRE-order lane first so deterministic strings such as JSON do not
    // materialize every partition of `([^"\\]*|\\.)*`. If the continuation
    // rejects it, the exhaustive collector below still provides full
    // backtracking semantics.
    let inner_can_branch = match inner {
        Node::Sequence(_)
        | Node::Alternation(_)
        | Node::Quantifier { .. }
        | Node::Group { .. }
        | Node::Subroutine(_)
        | Node::Conditional { .. } => match_states_can_branch(inner),
        _ => false,
    };
    if inner_can_branch {
        if let Some(end) =
            match_quantifier_preferred_path(inner, min, max, greedy, possessive, rest, pos, ctx)
        {
            return Some(end);
        }
        // Possessive repetition commits both the preferred inner path and the
        // repetition count. Falling through to the exhaustive collector
        // would incorrectly revisit a later alternation branch.
        if possessive {
            return None;
        }
    }
    if ctx.budget.has_error() {
        return None;
    }

    // A quantified compound atom can have several valid outcomes for the
    // same repetition count. The continuation must be allowed to reject the
    // preferred inner path and resume inside that atom (for example an
    // optional header whose later conditional inspects one of its captures).
    // Keep this stateful collector behind the existing branching proof so
    // ordinary literal/class quantifiers retain the allocation-free loop.
    if inner_can_branch {
        let initial = BacktrackState::take(pos, ctx);
        let states = collect_branching_quantifier_states(
            inner,
            min,
            max,
            greedy,
            possessive,
            initial.clone(),
            ctx,
        );
        for state in states {
            let end = state.end;
            state.install(ctx);
            if let Some(final_pos) = match_rest(rest, end, ctx) {
                return Some(final_pos);
            }
            if ctx.control.is_some() {
                return None;
            }
        }
        initial.install(ctx);
        return None;
    }

    // Collect all possible (reps, end_position, saved_groups) tuples when a
    // continuation may require the quantifier to give characters back.
    let tracks_captures = ctx.groups.len() > 1;
    let mut states: Vec<(
        usize,
        usize,
        Option<Vec<Option<Match>>>,
        Option<String>,
        Option<Vec<(String, usize)>>,
    )> = Vec::new();

    if !ctx.budget.consume_backtrack() {
        return None;
    }

    let mut current_pos = pos;
    let mut current_reps = 0usize;
    loop {
        if current_reps >= min {
            let groups = tracks_captures.then(|| ctx.groups.clone());
            states.push((
                current_reps,
                current_pos,
                groups,
                ctx.mark.clone(),
                ctx.mark_positions.clone(),
            ));
        }
        if current_reps >= limit {
            break;
        }
        if !ctx.budget.retain_quantifier_continuation() {
            break;
        }
        let saved = tracks_captures.then(|| ctx.groups.clone());
        let saved_mark = ctx.mark.clone();
        let saved_mark_positions = ctx.mark_positions.clone();
        // Try one more repetition
        match match_seq_from(inner, &[], current_pos, ctx) {
            Some(next_pos) => {
                if matches!(*ctx.control, Some(MatchControl::Accept)) {
                    return Some(next_pos);
                }
                current_reps += 1;
                if next_pos == current_pos {
                    if current_reps >= min && (greedy || current_reps == min) {
                        states.push((
                            current_reps,
                            current_pos,
                            tracks_captures.then(|| ctx.groups.clone()),
                            ctx.mark.clone(),
                            ctx.mark_positions.clone(),
                        ));
                    }
                    break;
                }
                current_pos = next_pos;
            }
            None => {
                if ctx.control.is_some() {
                    return None;
                }
                if let Some(saved) = saved {
                    *ctx.groups = saved;
                }
                *ctx.mark = saved_mark;
                *ctx.mark_positions = saved_mark_positions;
                break;
            }
        }
    }

    if ctx.budget.has_error() {
        return None;
    }

    // collect_states records states in increasing repetition order, so the
    // greedy path can iterate backwards without sorting.
    if possessive {
        if let Some((_, end_pos, saved_groups, mark, mark_positions)) = states.pop() {
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            *ctx.mark = mark;
            *ctx.mark_positions = mark_positions;
            return match_rest(rest, end_pos, ctx);
        }
    } else if greedy {
        for (_, end_pos, saved_groups, mark, mark_positions) in states.into_iter().rev() {
            if !ctx.budget.consume_backtrack() {
                return None;
            }
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            *ctx.mark = mark;
            *ctx.mark_positions = mark_positions;
            if let Some(final_pos) = match_rest(rest, end_pos, ctx) {
                return Some(final_pos);
            }
            if ctx.control.is_some() {
                return None;
            }
        }
    } else {
        for (_, end_pos, saved_groups, mark, mark_positions) in states {
            if !ctx.budget.consume_backtrack() {
                return None;
            }
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            *ctx.mark = mark;
            *ctx.mark_positions = mark_positions;
            if let Some(final_pos) = match_rest(rest, end_pos, ctx) {
                return Some(final_pos);
            }
            if ctx.control.is_some() {
                return None;
            }
        }
    }
    None
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
fn match_quantifier_preferred_path(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    possessive: bool,
    rest: &[Node],
    pos: usize,
    ctx: &mut MatchCtx,
) -> Option<usize> {
    let initial_groups = ctx.groups.clone();
    let initial_mark = ctx.mark.clone();
    let initial_mark_positions = ctx.mark_positions.clone();
    let tracks_captures = ctx.groups.len() > 1;
    let limit = max.unwrap_or(usize::MAX);
    let mut states = Vec::new();
    let mut current_pos = pos;
    let mut current_reps = 0usize;

    loop {
        if current_reps >= min {
            if !greedy {
                let saved_groups = tracks_captures.then(|| ctx.groups.clone());
                let saved_mark = ctx.mark.clone();
                let saved_mark_positions = ctx.mark_positions.clone();
                if !ctx.budget.consume_backtrack() {
                    *ctx.groups = initial_groups;
                    *ctx.mark = initial_mark;
                    *ctx.mark_positions = initial_mark_positions;
                    return None;
                }
                if let Some(end) = match_rest(rest, current_pos, ctx) {
                    return Some(end);
                }
                if ctx.control.is_some() {
                    return None;
                }
                if let Some(saved_groups) = saved_groups {
                    *ctx.groups = saved_groups;
                }
                *ctx.mark = saved_mark;
                *ctx.mark_positions = saved_mark_positions;
            }
            states.push((
                current_pos,
                tracks_captures.then(|| ctx.groups.clone()),
                ctx.mark.clone(),
                ctx.mark_positions.clone(),
            ));
        }
        if current_reps >= limit || !ctx.budget.retain_quantifier_continuation() {
            break;
        }
        let saved_groups = tracks_captures.then(|| ctx.groups.clone());
        let saved_mark = ctx.mark.clone();
        let saved_mark_positions = ctx.mark_positions.clone();
        match match_seq_from(inner, &[], current_pos, ctx) {
            Some(next_pos) => {
                if matches!(*ctx.control, Some(MatchControl::Accept)) {
                    return Some(next_pos);
                }
                current_reps += 1;
                if next_pos == current_pos {
                    if current_reps >= min && (greedy || current_reps == min) {
                        states.push((
                            current_pos,
                            tracks_captures.then(|| ctx.groups.clone()),
                            ctx.mark.clone(),
                            ctx.mark_positions.clone(),
                        ));
                    }
                    break;
                }
                current_pos = next_pos;
            }
            None => {
                if ctx.control.is_some() {
                    return None;
                }
                if let Some(saved_groups) = saved_groups {
                    *ctx.groups = saved_groups;
                }
                *ctx.mark = saved_mark;
                *ctx.mark_positions = saved_mark_positions;
                break;
            }
        }
    }

    if !greedy {
        *ctx.groups = initial_groups;
        *ctx.mark = initial_mark;
        *ctx.mark_positions = initial_mark_positions;
        return None;
    }
    states.reverse();
    if possessive {
        states.truncate(1);
    }
    for (end, groups, mark, mark_positions) in states {
        if !ctx.budget.consume_backtrack() {
            *ctx.groups = initial_groups;
            *ctx.mark = initial_mark;
            *ctx.mark_positions = initial_mark_positions;
            return None;
        }
        if let Some(groups) = groups {
            *ctx.groups = groups;
        }
        *ctx.mark = mark;
        *ctx.mark_positions = mark_positions;
        if let Some(final_pos) = match_rest(rest, end, ctx) {
            return Some(final_pos);
        }
        if ctx.control.is_some() {
            return None;
        }
    }
    *ctx.groups = initial_groups;
    *ctx.mark = initial_mark;
    *ctx.mark_positions = initial_mark_positions;
    None
}

fn is_word_boundary(chars: &[char], pos: usize, unicode: bool) -> bool {
    let before = if pos > 0 {
        is_word_char(chars[pos - 1], unicode)
    } else {
        false
    };
    let after = if pos < chars.len() {
        is_word_char(chars[pos], unicode)
    } else {
        false
    };
    before != after
}

fn is_word_char(c: char, unicode: bool) -> bool {
    (if unicode {
        c.is_alphanumeric()
    } else {
        c.is_ascii_alphanumeric()
    }) || c == '_'
}

fn match_shorthand(sh: Shorthand, c: char, unicode: bool) -> bool {
    let horizontal_space = || {
        matches!(
            c,
            '\u{0009}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'
                ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    };
    let vertical_space = || {
        matches!(
            c,
            '\u{000a}'
                | '\u{000b}'
                | '\u{000c}'
                | '\u{000d}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        )
    };
    match sh {
        Shorthand::Digit => c.is_ascii_digit(),
        Shorthand::NonDigit => !c.is_ascii_digit(),
        Shorthand::Word => is_word_char(c, unicode),
        Shorthand::NonWord => !is_word_char(c, unicode),
        Shorthand::Space => {
            if unicode {
                c.is_whitespace()
            } else {
                c.is_ascii_whitespace()
            }
        }
        Shorthand::NonSpace => {
            if unicode {
                !c.is_whitespace()
            } else {
                !c.is_ascii_whitespace()
            }
        }
        Shorthand::HorizontalSpace => horizontal_space(),
        Shorthand::NonHorizontalSpace => !horizontal_space(),
        Shorthand::VerticalSpace => vertical_space(),
        Shorthand::NonVerticalSpace => !vertical_space(),
        Shorthand::Property(class) => class.matches(c),
    }
}

fn match_class_item(item: &ClassItem, c: char, flags: RegexFlags) -> bool {
    match item {
        ClassItem::Literal(l) => chars_equal(c, *l, flags),
        ClassItem::Range(lo, hi) => {
            if flags.case_insensitive {
                if c >= *lo && c <= *hi {
                    true
                } else if flags.unicode_mode.utf() || flags.unicode_mode.ucp() {
                    let folded = unicode::simple_case_fold(c);
                    let lower = unicode::simple_case_fold(*lo);
                    let upper = unicode::simple_case_fold(*hi);
                    folded >= lower && folded <= upper
                } else if c.is_ascii() && lo.is_ascii() && hi.is_ascii() {
                    let folded = c.to_ascii_lowercase();
                    let lower = lo.to_ascii_lowercase();
                    let upper = hi.to_ascii_lowercase();
                    folded >= lower && folded <= upper
                } else {
                    false
                }
            } else {
                c >= *lo && c <= *hi
            }
        }
        ClassItem::Shorthand(sh) => match_shorthand(*sh, c, flags.unicode_mode.ucp()),
        ClassItem::Posix { class, negated } => {
            class.matches(c, flags.unicode_mode.ucp()) != *negated
        }
    }
}

// ── Parser ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct ParserOptionState {
    flags: RegexFlags,
    duplicate_names: bool,
    no_auto_capture: bool,
    extended_more: bool,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    group_count: usize,
    named_groups: HashMap<String, usize>,
    group_names_by_index: HashMap<usize, String>,
    duplicate_named_groups: DuplicateNamedGroups,
    duplicate_names: bool,
    no_auto_capture: bool,
    extended_more: bool,
    lookaround_depth: usize,
    subpatterns_by_index: HashMap<usize, Rc<Node>>,
    subpatterns_by_name: HashMap<String, Rc<Node>>,
    saw_subroutine: bool,
    flags: RegexFlags,
    base_match_flags: LocalMatchFlags,
    match_limit: Option<usize>,
    depth_limit: Option<usize>,
    heap_limit: Option<usize>,
    start_items_allowed: bool,
    /// Named capture conditions may legally refer forward. Validate them only
    /// after the complete pattern has populated the name table.
    conditional_name_references: Vec<(String, usize)>,
    numeric_backreferences: Vec<(usize, usize)>,
    subroutine_references: Vec<(SubroutineTarget, usize)>,
    lookbehinds: Vec<Node>,
}

impl Parser {
    fn new(pattern: &str, flags: RegexFlags) -> Self {
        Self {
            chars: pattern.chars().collect(),
            pos: 0,
            group_count: 0,
            named_groups: HashMap::new(),
            group_names_by_index: HashMap::new(),
            duplicate_named_groups: HashMap::new(),
            duplicate_names: false,
            no_auto_capture: false,
            extended_more: false,
            lookaround_depth: 0,
            subpatterns_by_index: HashMap::new(),
            subpatterns_by_name: HashMap::new(),
            saw_subroutine: false,
            flags,
            base_match_flags: flags.into(),
            match_limit: None,
            depth_limit: None,
            heap_limit: None,
            start_items_allowed: true,
            conditional_name_references: Vec::new(),
            numeric_backreferences: Vec::new(),
            subroutine_references: Vec::new(),
            lookbehinds: Vec::new(),
        }
    }

    fn option_state(&self) -> ParserOptionState {
        ParserOptionState {
            flags: self.flags,
            duplicate_names: self.duplicate_names,
            no_auto_capture: self.no_auto_capture,
            extended_more: self.extended_more,
        }
    }

    fn restore_option_state(&mut self, state: ParserOptionState) {
        self.flags = state.flags;
        self.duplicate_names = state.duplicate_names;
        self.no_auto_capture = state.no_auto_capture;
        self.extended_more = state.extended_more;
    }

    fn parse_scoped_alternation(&mut self) -> Result<Node, String> {
        let outer = self.option_state();
        let result = self.parse_alternation();
        self.restore_option_state(outer);
        result
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn capture_group(&mut self, index: usize, name: Option<String>, inner: Node) -> Node {
        let group = Node::Group {
            index: Some(index),
            name: name.clone(),
            inner: Box::new(inner),
        };
        let shared = Rc::new(group.clone());
        self.subpatterns_by_index.insert(index, Rc::clone(&shared));
        if let Some(name) = name {
            self.subpatterns_by_name.entry(name).or_insert(shared);
        }
        group
    }

    fn noncapturing_group(inner: Node) -> Node {
        Node::Group {
            index: None,
            name: None,
            inner: Box::new(inner),
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn register_named_group(&mut self, name: &str, index: usize) -> Result<(), String> {
        if let Some(previous) = self.group_names_by_index.get(&index)
            && previous != name
        {
            return Err(format!(
                "different names for subpatterns of the same number are not allowed at offset {}",
                self.pos
            ));
        }
        self.group_names_by_index
            .entry(index)
            .or_insert_with(|| name.to_string());
        if let Some(&previous) = self.named_groups.get(name) {
            // A branch-reset group may deliberately reuse one physical slot.
            if previous == index {
                return Ok(());
            }
            if !self.duplicate_names {
                return Err(
                    "two named subpatterns have the same name (PCRE2_DUPNAMES not set)".into(),
                );
            }
            self.duplicate_named_groups
                .entry(name.to_string())
                .or_insert_with(|| vec![previous])
                .push(index);
        }
        self.named_groups.insert(name.to_string(), index);
        Ok(())
    }

    fn group_name_character_is_valid(&self, character: char, first: bool) -> bool {
        if character == '_' || character.is_ascii_alphabetic() {
            return true;
        }
        if !first && character.is_ascii_digit() {
            return true;
        }
        self.flags.unicode_mode.utf()
            && (unicode::category_has_initial(character, 'L')
                || (!first && unicode::category_is(character, "Nd")))
    }

    fn parse_group_name(&mut self, closing: char) -> Result<String, String> {
        let start = self.pos;
        let Some(first) = self.peek() else {
            return Err(format!("subpattern name expected at offset {start}"));
        };
        if first.is_ascii_digit()
            || (self.flags.unicode_mode.utf() && unicode::category_is(first, "Nd"))
        {
            return Err(format!(
                "subpattern name must start with a non-digit at offset {start}"
            ));
        }
        if !self.group_name_character_is_valid(first, true) {
            return Err(format!("subpattern name expected at offset {start}"));
        }
        let mut name = String::new();
        while let Some(character) = self.peek() {
            if character == closing {
                self.advance();
                if name.len() > 32 {
                    return Err(format!(
                        "subpattern name is too long (maximum 32 code units) at offset {}",
                        self.pos.saturating_sub(1)
                    ));
                }
                return Ok(name);
            }
            if !self.group_name_character_is_valid(character, name.is_empty()) {
                return Err(format!(
                    "syntax error in subpattern name (missing terminator?) at offset {}",
                    self.pos
                ));
            }
            name.push(character);
            self.advance();
        }
        Err(format!(
            "syntax error in subpattern name (missing terminator?) at offset {}",
            self.pos
        ))
    }

    fn parse_lookaround_inner(&mut self) -> Result<Node, String> {
        self.lookaround_depth += 1;
        let inner = self.parse_scoped_alternation();
        self.lookaround_depth -= 1;
        inner
    }

    fn validate_lookbehind(&self, inner: &Node) -> Result<(), String> {
        let mut visiting = HashSet::new();
        let fixed = match inner {
            // PCRE 10.42 permits top-level alternatives to have different
            // fixed lengths, but a nested alternation is itself variable.
            Node::Alternation(branches) => branches
                .iter()
                .all(|branch| self.fixed_length(branch, &mut visiting).is_some()),
            _ => self.fixed_length(inner, &mut visiting).is_some(),
        };
        if fixed {
            Ok(())
        } else {
            Err("lookbehind assertion is not fixed length at offset 0".into())
        }
    }

    fn fixed_length(&self, node: &Node, visiting: &mut HashSet<usize>) -> Option<usize> {
        match node {
            Node::Literal(_)
            | Node::AnyChar
            | Node::ByteUnit
            | Node::NotNewline
            | Node::CharClass { .. }
            | Node::Shorthand(_) => Some(1),
            Node::GraphemeCluster | Node::Linebreak => None,
            Node::Anchor(_)
            | Node::WordBoundary(_)
            | Node::Lookahead { .. }
            | Node::Lookbehind { .. }
            | Node::Mark(_)
            | Node::Control { .. }
            | Node::ResetStart
            | Node::CaptureEnd { .. }
            | Node::CaptureRestore { .. } => Some(0),
            Node::Sequence(nodes) => nodes.iter().try_fold(0usize, |total, node| {
                total.checked_add(self.fixed_length(node, visiting)?)
            }),
            Node::Alternation(branches) => {
                let mut length = None;
                for branch in branches {
                    let branch_length = self.fixed_length(branch, visiting)?;
                    if length.is_some_and(|length| length != branch_length) {
                        return None;
                    }
                    length = Some(branch_length);
                }
                Some(length.unwrap_or(0))
            }
            Node::Quantifier {
                inner, min, max, ..
            } if *max == Some(*min) => self.fixed_length(inner, visiting)?.checked_mul(*min),
            Node::Quantifier { .. } => None,
            Node::Group { inner, .. }
            | Node::Atomic(inner)
            | Node::ScriptRun { inner, .. }
            | Node::LocalFlags { inner, .. } => self.fixed_length(inner, visiting),
            Node::Backreference(index) => {
                if !visiting.insert(*index) {
                    return None;
                }
                let length = self
                    .subpatterns_by_index
                    .get(index)
                    .and_then(|group| self.fixed_length(group, visiting));
                visiting.remove(index);
                length
            }
            Node::NamedBackreference(name) => self
                .named_groups
                .get(name)
                .and_then(|index| self.fixed_length(&Node::Backreference(*index), visiting)),
            Node::Conditional { yes, no, .. } => {
                let yes = self.fixed_length(yes, visiting)?;
                let no = no
                    .as_deref()
                    .map_or(Some(0), |no| self.fixed_length(no, visiting))?;
                (yes == no).then_some(yes)
            }
            Node::Subroutine(SubroutineTarget::WholePattern) => None,
            Node::Subroutine(SubroutineTarget::Group(index)) => {
                if !visiting.insert(*index) {
                    return None;
                }
                let length = self
                    .subpatterns_by_index
                    .get(index)
                    .and_then(|group| self.fixed_length(group, visiting));
                visiting.remove(index);
                length
            }
            Node::Subroutine(SubroutineTarget::Name(name)) => {
                self.named_groups.get(name).and_then(|index| {
                    self.fixed_length(&Node::Subroutine(SubroutineTarget::Group(*index)), visiting)
                })
            }
            Node::Quoted(chars) => Some(chars.len()),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn parse(&mut self) -> Result<Node, String> {
        let node = self.parse_alternation()?;
        if self.pos < self.chars.len() {
            Err(format!(
                "Unexpected character '{}' at position {}",
                self.chars[self.pos], self.pos
            ))
        } else {
            Ok(node)
        }
    }

    fn parse_alternation(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.parse_sequence()?];
        while self.peek() == Some('|') {
            self.advance();
            branches.push(self.parse_sequence()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(Node::Alternation(branches))
        }
    }

    fn parse_sequence(&mut self) -> Result<Node, String> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            // In extended mode, skip whitespace and #-comments
            if self.flags.extended {
                if c.is_ascii_whitespace() {
                    self.advance();
                    continue;
                }
                if c == '#' {
                    while self.peek().is_some() {
                        if let Some(length) =
                            newline_len_at(&self.chars, self.pos, self.flags.line_options.newline())
                        {
                            self.pos += length;
                            break;
                        }
                        self.advance();
                    }
                    continue;
                }
            }
            let atom_flags = LocalMatchFlags::from(self.flags);
            let atom = self.parse_atom()?;
            let quantified = if let Node::Quoted(chars) = atom {
                self.lower_quoted_atom(chars, atom_flags)?
            } else {
                let atom = self.annotate_local_flags(atom, atom_flags);
                self.parse_quantifier(atom)?
            };
            nodes.push(quantified);
        }
        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(Node::Sequence(nodes))
        }
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
        if self.peek() != Some('(') {
            self.start_items_allowed = false;
        }
        match self.peek() {
            Some('.') => {
                self.advance();
                Ok(Node::AnyChar)
            }
            Some('^') => {
                self.advance();
                Ok(Node::Anchor(Anchor::Start))
            }
            Some('$') => {
                self.advance();
                Ok(Node::Anchor(Anchor::End))
            }
            Some('[') => self.parse_char_class(),
            Some('(') => self.parse_group(),
            Some('\\') => self.parse_escape(),
            Some('{') => {
                // A brace can begin a quantifier only after a repeatable
                // atom. At the start of an expression PCRE treats it as a
                // literal when no valid counted repetition can exist.
                self.advance();
                Ok(Node::Literal('{'))
            }
            Some(c) if c != '*' && c != '+' && c != '?' => {
                self.advance();
                Ok(Node::Literal(c))
            }
            Some(c) => Err(format!(
                "Unexpected quantifier '{}' without preceding element at position {}",
                c, self.pos
            )),
            None => Err("Unexpected end of pattern".into()),
        }
    }

    fn parse_escape(&mut self) -> Result<Node, String> {
        self.advance(); // consume '\'
        match self.advance() {
            Some('d') => Ok(Node::Shorthand(Shorthand::Digit)),
            Some('D') => Ok(Node::Shorthand(Shorthand::NonDigit)),
            Some('w') => Ok(Node::Shorthand(Shorthand::Word)),
            Some('W') => Ok(Node::Shorthand(Shorthand::NonWord)),
            Some('s') => Ok(Node::Shorthand(Shorthand::Space)),
            Some('S') => Ok(Node::Shorthand(Shorthand::NonSpace)),
            Some('h') => Ok(Node::Shorthand(Shorthand::HorizontalSpace)),
            Some('H') => Ok(Node::Shorthand(Shorthand::NonHorizontalSpace)),
            Some('v') => Ok(Node::Shorthand(Shorthand::VerticalSpace)),
            Some('V') => Ok(Node::Shorthand(Shorthand::NonVerticalSpace)),
            Some('b') => Ok(Node::WordBoundary(true)),
            Some('B') => Ok(Node::WordBoundary(false)),
            Some('A') => Ok(Node::Anchor(Anchor::AbsoluteStart)),
            Some('Z') => Ok(Node::Anchor(Anchor::FinalEnd)),
            Some('z') => Ok(Node::Anchor(Anchor::AbsoluteEnd)),
            Some('G') => Ok(Node::Anchor(Anchor::SearchStart)),
            Some('K') => {
                if self.lookaround_depth != 0 {
                    let offset = self.chars.iter().map(|ch| ch.len_utf8()).sum::<usize>();
                    return Err(format!(
                        "\\K is not allowed in lookarounds (but see PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK) at offset {offset}"
                    ));
                }
                Ok(Node::ResetStart)
            }
            Some('n') => Ok(Node::Literal('\n')),
            Some('r') => Ok(Node::Literal('\r')),
            Some('t') => Ok(Node::Literal('\t')),
            Some('a') => Ok(Node::Literal('\u{0007}')),
            Some('e') => Ok(Node::Literal('\u{001b}')),
            Some('f') => Ok(Node::Literal('\u{000c}')),
            Some('c') => Ok(Node::Literal(self.parse_control_escape()?)),
            Some('C') => {
                if self.flags.unicode_mode.utf() {
                    return Err(format!(
                        "using \\C is incompatible with the 'u' modifier at offset {}",
                        self.pos
                    ));
                }
                Ok(Node::ByteUnit)
            }
            Some('N') => self.parse_capital_n_escape(),
            Some('Q') => Ok(Node::Quoted(self.parse_quoted_literal())),
            // Outside a quoted span, PCRE treats the quote terminator as an
            // ignored zero-width escape.
            Some('E') => Ok(Node::Sequence(Vec::new())),
            Some('0') => Ok(Node::Literal(self.parse_octal_escape('0')?)),
            Some('o') => Ok(Node::Literal(self.parse_braced_octal_escape()?)),
            Some('k') => {
                // \k<name>, \k'name', or \k{name} — named backreference.
                let delim = self.peek();
                if matches!(delim, Some('<' | '\'' | '{')) {
                    self.advance();
                    let close = match delim {
                        Some('<') => '>',
                        Some('\'') => '\'',
                        Some('{') => '}',
                        _ => unreachable!(),
                    };
                    let name = self.parse_group_name(close)?;
                    Ok(Node::NamedBackreference(name))
                } else {
                    Err(format!(
                        "\u{005c}k is not followed by a braced, angle-bracketed, or quoted name at offset {}",
                        self.pos
                    ))
                }
            }
            Some('g') => self.parse_g_escape(),
            Some(c) if c.is_ascii_digit() && c != '0' => {
                // PCRE's legacy spelling is context-sensitive: values below
                // 10, values beginning with 8/9, and existing group numbers
                // are backreferences; otherwise up to three octal digits are
                // one character and any remaining digits are literals.
                let mut num_str = String::new();
                num_str.push(c);
                while let Some(next) = self.peek() {
                    if next.is_ascii_digit() {
                        num_str.push(next);
                        self.advance();
                    } else {
                        break;
                    }
                }
                let n: usize = num_str.parse().unwrap();
                if n < 10 || matches!(c, '8' | '9') || n <= self.group_count {
                    Ok(Node::Backreference(n))
                } else {
                    let octal_len = num_str
                        .bytes()
                        .take(3)
                        .take_while(|digit| matches!(*digit, b'0'..=b'7'))
                        .count();
                    let value = u32::from_str_radix(&num_str[..octal_len], 8)
                        .map_err(|_| "invalid octal escape")?;
                    let octal = char::from_u32(value).ok_or("invalid octal escape")?;
                    let mut chars = Vec::with_capacity(1 + num_str.len() - octal_len);
                    chars.push(octal);
                    chars.extend(num_str[octal_len..].chars());
                    Ok(if chars.len() == 1 {
                        Node::Literal(octal)
                    } else {
                        Node::Quoted(chars)
                    })
                }
            }
            Some('x') => Ok(Node::Literal(self.parse_hex_escape()?)),
            Some('p') => Ok(Node::Shorthand(Shorthand::Property(
                self.parse_property_escape(false)?,
            ))),
            Some('P') => Ok(Node::Shorthand(Shorthand::Property(
                self.parse_property_escape(true)?,
            ))),
            Some('X') => Ok(Node::GraphemeCluster),
            Some('R') => Ok(Node::Linebreak),
            // PCRE2_EXTRA_BAD_ESCAPE_IS_LITERAL is not enabled by PHP. Keep
            // invalid letter escapes as compile errors even when the legacy
            // `/X` modifier is present (PHP accepts that modifier as a no-op).
            Some(c) if c.is_ascii_alphabetic() => Err(format!(
                "unrecognized character follows \\ at offset {}",
                self.pos.saturating_sub(1)
            )),
            Some(c) => Ok(Node::Literal(c)), // \/, \\, \., etc.
            None => Err("Unexpected end after \\".into()),
        }
    }

    fn annotate_local_flags(&self, node: Node, flags: LocalMatchFlags) -> Node {
        let is_runtime_atom = matches!(
            node,
            Node::Literal(_)
                | Node::AnyChar
                | Node::ByteUnit
                | Node::NotNewline
                | Node::Anchor(_)
                | Node::CharClass { .. }
                | Node::Shorthand(_)
                | Node::Backreference(_)
                | Node::NamedBackreference(_)
                | Node::WordBoundary(_)
                | Node::GraphemeCluster
                | Node::Linebreak
        );
        if is_runtime_atom && flags != self.base_match_flags {
            Node::LocalFlags {
                flags,
                inner: Box::new(node),
            }
        } else {
            node
        }
    }

    fn lower_quoted_atom(
        &mut self,
        chars: Vec<char>,
        flags: LocalMatchFlags,
    ) -> Result<Node, String> {
        let Some((&last, prefix)) = chars.split_last() else {
            return Ok(Node::Sequence(Vec::new()));
        };
        let mut nodes = Vec::with_capacity(chars.len());
        nodes.extend(
            prefix
                .iter()
                .copied()
                .map(Node::Literal)
                .map(|node| self.annotate_local_flags(node, flags)),
        );
        let last = self.annotate_local_flags(Node::Literal(last), flags);
        nodes.push(self.parse_quantifier(last)?);
        Ok(if nodes.len() == 1 {
            nodes.pop().unwrap()
        } else {
            Node::Sequence(nodes)
        })
    }

    fn parse_control_escape(&mut self) -> Result<char, String> {
        let value = self
            .advance()
            .ok_or("missing character after \\c at offset")?;
        if !((' '..='~').contains(&value)) {
            return Err("character following \\c must be printable ASCII at offset".into());
        }
        let value = value.to_ascii_uppercase() as u8 ^ 0x40;
        Ok(char::from(value))
    }

    fn parse_capital_n_escape(&mut self) -> Result<Node, String> {
        if self.peek() != Some('{') {
            return Ok(Node::NotNewline);
        }
        self.advance();
        if self.advance() != Some('U') || self.advance() != Some('+') {
            return Err("unsupported escape sequence after \\N at offset".into());
        }
        let mut value = 0u32;
        let mut digits = 0usize;
        loop {
            match self.advance() {
                Some('}') if digits != 0 => break,
                Some(c) => {
                    let digit = c
                        .to_digit(16)
                        .ok_or("invalid hexadecimal number in \\N{U+...}")?;
                    value = value
                        .checked_mul(16)
                        .and_then(|value| value.checked_add(digit))
                        .filter(|value| *value <= 0x10ffff)
                        .ok_or("character code point value in \\N{U+...} is too large")?;
                    digits += 1;
                }
                _ => return Err("missing closing brace for \\N{U+...}".into()),
            }
        }
        char::from_u32(value)
            .map(Node::Literal)
            .ok_or_else(|| "invalid Unicode code point in \\N{U+...}".into())
    }

    fn parse_quoted_literal(&mut self) -> Vec<char> {
        let mut literal = Vec::new();
        while let Some(c) = self.advance() {
            if c == '\\' && self.peek() == Some('E') {
                self.advance();
                break;
            }
            literal.push(c);
        }
        literal
    }

    fn parse_g_escape(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('{') => {
                self.advance();
                let content_start = self.pos;
                let content = self.parse_delimited_text('}')?;
                let numeric = content.starts_with(|c| matches!(c, '+' | '-'))
                    || content.chars().all(|c| c.is_ascii_digit());
                if !numeric
                    && (content.is_empty()
                        || !content.chars().enumerate().all(|(index, character)| {
                            self.group_name_character_is_valid(character, index == 0)
                        }))
                {
                    return Err(format!(
                        "subpattern name expected at offset {}",
                        content_start
                    ));
                }
                self.backreference_from_text(&content)
            }
            Some('<') | Some('\'') => {
                let opening = self.advance().unwrap();
                let closing = if opening == '<' { '>' } else { '\'' };
                let reference_offset = self.pos;
                let content = self.parse_delimited_text(closing)?;
                self.saw_subroutine = true;
                let target = self.subroutine_target_from_text(&content)?;
                self.subroutine_references
                    .push((target.clone(), reference_offset));
                Ok(Node::Subroutine(target))
            }
            Some('+' | '-' | '0'..='9') => {
                let content = self.parse_signed_decimal_text();
                self.backreference_from_text(&content)
            }
            _ => Err(format!("malformed \\g escape at offset {}", self.pos)),
        }
    }

    fn parse_callout(&mut self) -> Result<Node, String> {
        self.advance(); // consume C
        if self.peek() == Some(')') {
            self.advance();
            return Ok(Node::Sequence(Vec::new()));
        }
        if self.peek().is_some_and(|c| c.is_ascii_digit()) {
            let mut value = 0u16;
            while let Some(digit) = self.peek().and_then(|c| c.to_digit(10)) {
                value = value
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(digit as u16))
                    .filter(|value| *value <= 255)
                    .ok_or_else(|| {
                        format!(
                            "number after (?C is greater than 255 at offset {}",
                            self.pos + 1
                        )
                    })?;
                self.advance();
            }
            if self.advance() != Some(')') {
                return Err("malformed numeric callout".into());
            }
            return Ok(Node::Sequence(Vec::new()));
        }
        let opening = self.advance().ok_or("missing callout string delimiter")?;
        let closing = if opening == '{' { '}' } else { opening };
        if !matches!(opening, '`' | '\'' | '"' | '^' | '%' | '#' | '$' | '{') {
            return Err("invalid callout string delimiter".into());
        }
        loop {
            let c = self.advance().ok_or("unterminated callout string")?;
            if c != closing {
                continue;
            }
            if self.peek() == Some(closing) {
                self.advance();
                continue;
            }
            break;
        }
        if self.advance() != Some(')') {
            return Err("malformed callout string".into());
        }
        // PHP installs no application callout callback. A valid callout is
        // consequently an observable zero-width no-op.
        Ok(Node::Sequence(Vec::new()))
    }

    fn parse_delimited_text(&mut self, closing: char) -> Result<String, String> {
        let mut text = String::new();
        while let Some(c) = self.advance() {
            if c == closing {
                return Ok(text);
            }
            text.push(c);
        }
        Err("unterminated reference name".into())
    }

    fn parse_signed_decimal_text(&mut self) -> String {
        let mut text = String::new();
        if self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
            text.push(self.advance().unwrap());
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            text.push(self.advance().unwrap());
        }
        text
    }

    fn backreference_from_text(&mut self, text: &str) -> Result<Node, String> {
        if text.starts_with(|c| matches!(c, '+' | '-')) || text.chars().all(|c| c.is_ascii_digit())
        {
            let value = text
                .parse::<isize>()
                .map_err(|_| "invalid numeric backreference")?;
            let first = text.as_bytes().first().copied();
            let index = if matches!(first, Some(b'+' | b'-')) {
                let relative_base = if first == Some(b'-') {
                    self.group_count as isize + 1
                } else {
                    self.group_count as isize
                };
                relative_base
                    .checked_add(value)
                    .filter(|index| *index > 0)
                    .ok_or("reference to non-existent subpattern")? as usize
            } else {
                usize::try_from(value).map_err(|_| "invalid numeric backreference")?
            };
            let offset = if index == 0 {
                self.pos
            } else {
                self.pos.saturating_sub(1)
            };
            self.numeric_backreferences.push((index, offset));
            Ok(Node::Backreference(index))
        } else if !text.is_empty() {
            Ok(Node::NamedBackreference(text.to_string()))
        } else {
            Err("empty backreference".into())
        }
    }

    fn subroutine_target_from_text(&self, text: &str) -> Result<SubroutineTarget, String> {
        if text.starts_with(|c| matches!(c, '+' | '-')) || text.chars().all(|c| c.is_ascii_digit())
        {
            let value = text
                .parse::<isize>()
                .map_err(|_| "invalid numeric subroutine reference")?;
            let first = text.as_bytes().first().copied();
            let index = if matches!(first, Some(b'+' | b'-')) {
                let relative_base = if first == Some(b'-') {
                    self.group_count as isize + 1
                } else {
                    self.group_count as isize
                };
                relative_base
                    .checked_add(value)
                    .filter(|index| *index > 0)
                    .ok_or("reference to non-existent subpattern")? as usize
            } else {
                usize::try_from(value).map_err(|_| "invalid numeric subroutine reference")?
            };
            Ok(if index == 0 {
                SubroutineTarget::WholePattern
            } else {
                SubroutineTarget::Group(index)
            })
        } else if !text.is_empty() {
            Ok(SubroutineTarget::Name(text.to_string()))
        } else {
            Err("empty subroutine reference".into())
        }
    }

    /// `\xhh` or `\x{h…}` after the `x` was consumed.
    fn parse_hex_escape(&mut self) -> Result<char, String> {
        let mut value: u32 = 0;
        if self.peek() == Some('{') {
            self.advance();
            let mut digits = 0;
            loop {
                match self.advance() {
                    Some('}') => break,
                    Some(c) => {
                        let digit = c.to_digit(16).ok_or_else(|| {
                            format!(
                                "non-hex character in \\x{{}} (closing brace missing?) at offset {}",
                                self.pos.saturating_sub(1)
                            )
                        })?;
                        value = value
                            .checked_mul(16)
                            .and_then(|value| value.checked_add(digit))
                            .filter(|value| *value <= 0x10FFFF)
                            .ok_or("character code point value in \\x{} or \\o{} is too large")?;
                        digits += 1;
                    }
                    None => return Err("Malformed \\x{} escape sequence".into()),
                }
            }
            if digits == 0 {
                return Err("Malformed \\x{} escape sequence".into());
            }
        } else {
            for _ in 0..2 {
                let Some(digit) = self.peek().and_then(|c| c.to_digit(16)) else {
                    break;
                };
                value = value * 16 + digit;
                self.advance();
            }
        }
        if self.flags.unicode_mode.utf() && (0xd800..=0xdfff).contains(&value) {
            return Err(format!(
                "disallowed Unicode code point (>= 0xd800 && <= 0xdfff) at offset {}",
                self.pos.saturating_sub(1)
            ));
        }
        char::from_u32(value)
            .ok_or_else(|| "character code point value in \\x{} or \\o{} is too large".into())
    }

    /// A traditional PCRE octal escape (`\0`, `\000`, …), after its first
    /// digit was consumed. PCRE limits this spelling to three octal digits.
    fn parse_octal_escape(&mut self, first: char) -> Result<char, String> {
        let mut value = first.to_digit(8).unwrap();
        for _ in 1..3 {
            let Some(digit) = self.peek().and_then(|c| c.to_digit(8)) else {
                break;
            };
            value = value * 8 + digit;
            self.advance();
        }
        char::from_u32(value).ok_or_else(|| "octal value is greater than \\377".into())
    }

    /// PCRE2's unambiguous braced octal spelling (`\o{...}`).
    fn parse_braced_octal_escape(&mut self) -> Result<char, String> {
        if self.advance() != Some('{') {
            return Err("digits missing in \\o{} at offset".into());
        }
        let mut value = 0u32;
        let mut digits = 0usize;
        loop {
            match self.advance() {
                Some('}') if digits != 0 => break,
                Some(c) => {
                    let digit = c.to_digit(8).ok_or_else(|| {
                        format!(
                            "non-octal character in \\o{{}} (closing brace missing?) at offset {}",
                            self.pos.saturating_sub(1)
                        )
                    })?;
                    value = value
                        .checked_mul(8)
                        .and_then(|value| value.checked_add(digit))
                        .filter(|value| *value <= 0x10FFFF)
                        .ok_or("character code point value in \\x{} or \\o{} is too large")?;
                    digits += 1;
                }
                None => return Err("missing closing brace for \\o{} at offset".into()),
            }
        }
        char::from_u32(value)
            .ok_or_else(|| "character code point value in \\x{} or \\o{} is too large".into())
    }

    /// `\p…` / `\P…` after the letter was consumed.
    fn parse_property_escape(&mut self, negated: bool) -> Result<unicode::PropertyClass, String> {
        let name = if self.peek() == Some('{') {
            self.advance();
            let mut name = String::new();
            loop {
                match self.advance() {
                    Some('}') => break,
                    Some(c) => name.push(c),
                    None => return Err("malformed \\P or \\p sequence".into()),
                }
            }
            name
        } else {
            self.advance()
                .map(String::from)
                .ok_or("malformed \\P or \\p sequence")?
        };
        unicode::PropertyClass::parse(&name, negated)
            .ok_or_else(|| format!("unknown property after \\P or \\p at offset {}", self.pos))
    }

    /// One escaped character-class atom after the backslash was consumed.
    fn parse_class_escape(&mut self) -> Result<ClassItem, String> {
        match self.advance() {
            Some('d') => Ok(ClassItem::Shorthand(Shorthand::Digit)),
            Some('D') => Ok(ClassItem::Shorthand(Shorthand::NonDigit)),
            Some('w') => Ok(ClassItem::Shorthand(Shorthand::Word)),
            Some('W') => Ok(ClassItem::Shorthand(Shorthand::NonWord)),
            Some('s') => Ok(ClassItem::Shorthand(Shorthand::Space)),
            Some('S') => Ok(ClassItem::Shorthand(Shorthand::NonSpace)),
            Some('h') => Ok(ClassItem::Shorthand(Shorthand::HorizontalSpace)),
            Some('H') => Ok(ClassItem::Shorthand(Shorthand::NonHorizontalSpace)),
            Some('v') => Ok(ClassItem::Shorthand(Shorthand::VerticalSpace)),
            Some('V') => Ok(ClassItem::Shorthand(Shorthand::NonVerticalSpace)),
            Some('b') => Ok(ClassItem::Literal('\u{0008}')),
            Some('n') => Ok(ClassItem::Literal('\n')),
            Some('r') => Ok(ClassItem::Literal('\r')),
            Some('t') => Ok(ClassItem::Literal('\t')),
            Some('a') => Ok(ClassItem::Literal('\u{0007}')),
            Some('e') => Ok(ClassItem::Literal('\u{001b}')),
            Some('f') => Ok(ClassItem::Literal('\u{000c}')),
            Some('c') => Ok(ClassItem::Literal(self.parse_control_escape()?)),
            Some(c @ '0'..='7') => Ok(ClassItem::Literal(self.parse_octal_escape(c)?)),
            Some('o') => Ok(ClassItem::Literal(self.parse_braced_octal_escape()?)),
            Some('x') => Ok(ClassItem::Literal(self.parse_hex_escape()?)),
            Some('p') => Ok(ClassItem::Shorthand(Shorthand::Property(
                self.parse_property_escape(false)?,
            ))),
            Some('P') => Ok(ClassItem::Shorthand(Shorthand::Property(
                self.parse_property_escape(true)?,
            ))),
            // PCRE does not enable BAD_ESCAPE_IS_LITERAL. Alphabetic escapes
            // that have no character-class meaning are compilation errors.
            Some(ec) if ec.is_ascii_alphabetic() => Err(format!(
                "escape sequence is invalid in character class at offset {}",
                self.pos.saturating_sub(1)
            )),
            Some(ec) => Ok(ClassItem::Literal(ec)),
            None => Err("Unexpected end in character class escape".into()),
        }
    }

    fn parse_char_class(&mut self) -> Result<Node, String> {
        self.advance(); // consume '['
        let negated = if self.peek() == Some('^') {
            self.advance();
            true
        } else {
            false
        };

        let mut items = Vec::new();
        // First char can be ']' as literal
        if self.peek() == Some(']') {
            self.advance();
            items.push(ClassItem::Literal(']'));
        }

        while let Some(c) = self.peek() {
            if c == ']' {
                self.advance();
                return Ok(Node::CharClass { negated, items });
            }
            self.advance();
            if self.extended_more && matches!(c, ' ' | '\t') {
                continue;
            }
            if c == '\\' && self.peek() == Some('Q') {
                self.advance();
                while let Some(quoted) = self.advance() {
                    if quoted == '\\' && self.peek() == Some('E') {
                        self.advance();
                        break;
                    }
                    items.push(ClassItem::Literal(quoted));
                }
                continue;
            }
            let item = if c == '[' && self.peek() == Some(':') {
                self.advance(); // consume ':'
                let negated = if self.peek() == Some('^') {
                    self.advance();
                    true
                } else {
                    false
                };
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    if c == ':' && self.chars.get(self.pos + 1) == Some(&']') {
                        self.advance();
                        self.advance();
                        break;
                    }
                    if c == ']' {
                        return Err("invalid POSIX character class".into());
                    }
                    name.push(c);
                    self.advance();
                }
                let class = PosixClass::parse(&name)
                    .ok_or_else(|| format!("unknown POSIX class name '{name}'"))?;
                ClassItem::Posix { class, negated }
            } else if c == '\\' {
                self.parse_class_escape()?
            } else {
                ClassItem::Literal(c)
            };
            // Ranges accept literal or escaped endpoints: a-z, \x{41}-\x{43}.
            let range_follows = self.peek() == Some('-')
                && self
                    .chars
                    .get(self.pos + 1)
                    .is_some_and(|next| *next != ']');
            if let ClassItem::Literal(lo) = item
                && range_follows
            {
                self.advance(); // consume '-'
                let hi = match self.advance() {
                    Some('\\') => match self.parse_class_escape()? {
                        ClassItem::Literal(hi) => hi,
                        _ => return Err("invalid range in character class".into()),
                    },
                    Some(hi) => hi,
                    None => return Err("Unexpected end in character class range".into()),
                };
                if lo > hi {
                    return Err(format!(
                        "range out of order in character class at offset {}",
                        self.pos.saturating_sub(1)
                    ));
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(item);
            }
        }
        Err("Unterminated character class".into())
    }

    fn parse_star_group(&mut self) -> Result<Node, String> {
        let mut verb = String::new();
        while let Some(c) = self.peek() {
            if matches!(c, ':' | '=' | ')') {
                break;
            }
            verb.push(c);
            self.advance();
        }
        let delimiter = self.peek().ok_or("Unterminated PCRE special group")?;
        let is_limit = delimiter == '='
            && matches!(
                verb.as_str(),
                "LIMIT_MATCH" | "LIMIT_DEPTH" | "LIMIT_RECURSION" | "LIMIT_HEAP"
            );
        let is_start_item = is_limit
            || (delimiter == ')'
                && matches!(
                    verb.as_str(),
                    "UTF"
                        | "UCP"
                        | "NOTEMPTY"
                        | "NOTEMPTY_ATSTART"
                        | "NO_AUTO_POSSESS"
                        | "NO_START_OPT"
                        | "NO_DOTSTAR_ANCHOR"
                        | "NO_JIT"
                        | "CR"
                        | "LF"
                        | "CRLF"
                        | "ANYCRLF"
                        | "ANY"
                        | "NUL"
                        | "BSR_ANYCRLF"
                        | "BSR_UNICODE"
                ));
        if is_start_item && !self.start_items_allowed {
            return Err(format!(
                "(*VERB) not recognized or malformed at offset {}",
                self.pos
            ));
        }
        if !is_start_item {
            self.start_items_allowed = false;
        }

        if (verb.is_empty() || verb == "MARK") && delimiter == ':' {
            self.advance();
            let name = self.parse_delimited_text(')')?;
            if name.is_empty() {
                return Err("name is zero length in (*MARK)".into());
            }
            return Ok(Node::Mark(name));
        }
        if verb == "MARK" {
            return Err(format!(
                "(*MARK) must have an argument at offset {}",
                self.pos
            ));
        }

        let control = match verb.as_str() {
            "FAIL" | "F" => Some(ControlVerb::Fail),
            "ACCEPT" => Some(ControlVerb::Accept),
            "COMMIT" => Some(ControlVerb::Commit),
            "PRUNE" => Some(ControlVerb::Prune),
            "SKIP" => Some(ControlVerb::Skip),
            "THEN" => Some(ControlVerb::Then),
            _ => None,
        };
        if let Some(control) = control {
            let name = match delimiter {
                ')' => {
                    self.advance();
                    None
                }
                ':' => {
                    self.advance();
                    let name = self.parse_delimited_text(')')?;
                    if name.is_empty() {
                        return Err(format!("name is zero length in (*{verb})"));
                    }
                    Some(name)
                }
                _ => return Err(format!("malformed (*{verb}) control verb")),
            };
            return Ok(Node::Control {
                verb: control,
                name,
            });
        }

        if delimiter == '='
            && matches!(
                verb.as_str(),
                "LIMIT_MATCH" | "LIMIT_DEPTH" | "LIMIT_RECURSION" | "LIMIT_HEAP"
            )
        {
            self.advance();
            let digits = self.parse_delimited_text(')')?;
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("malformed (*{verb}=...) setting"));
            }
            let limit = digits
                .parse::<usize>()
                .map_err(|_| format!("(*{verb}) value is too large"))?;
            match verb.as_str() {
                "LIMIT_MATCH" => {
                    self.match_limit = Some(self.match_limit.map_or(limit, |old| old.min(limit)))
                }
                "LIMIT_DEPTH" | "LIMIT_RECURSION" => {
                    self.depth_limit = Some(self.depth_limit.map_or(limit, |old| old.min(limit)))
                }
                "LIMIT_HEAP" => {
                    self.heap_limit = Some(self.heap_limit.map_or(limit, |old| old.min(limit)))
                }
                _ => unreachable!(),
            }
            return Ok(Node::Sequence(Vec::new()));
        }

        if delimiter == ')' {
            self.advance();
            match verb.as_str() {
                "UTF" => self.flags.unicode_mode = self.flags.unicode_mode.with_utf(),
                "UCP" => self.flags.unicode_mode = self.flags.unicode_mode.with_ucp(),
                // PHP 8.5 accepts these leading PCRE2 directives, but its
                // preg_* wrapper does not propagate them as match options.
                // Consequently both are observable no-ops at this boundary.
                "NOTEMPTY" | "NOTEMPTY_ATSTART" => {}
                "NO_AUTO_POSSESS" | "NO_START_OPT" | "NO_DOTSTAR_ANCHOR" | "NO_JIT" => {}
                "CR" => self.flags.line_options.set_newline(NewlineConvention::Cr),
                "LF" => self.flags.line_options.set_newline(NewlineConvention::Lf),
                "CRLF" => self.flags.line_options.set_newline(NewlineConvention::CrLf),
                "ANYCRLF" => self
                    .flags
                    .line_options
                    .set_newline(NewlineConvention::AnyCrLf),
                "ANY" => self.flags.line_options.set_newline(NewlineConvention::Any),
                "NUL" => self.flags.line_options.set_newline(NewlineConvention::Nul),
                "BSR_ANYCRLF" => self.flags.line_options.0 |= LineOptions::BSR_ANYCRLF,
                "BSR_UNICODE" => self.flags.line_options.0 &= !LineOptions::BSR_ANYCRLF,
                "atomic"
                | "pla"
                | "positive_lookahead"
                | "napla"
                | "non_atomic_positive_lookahead"
                | "nla"
                | "negative_lookahead"
                | "plb"
                | "positive_lookbehind"
                | "naplb"
                | "non_atomic_positive_lookbehind"
                | "nlb"
                | "negative_lookbehind"
                | "script_run"
                | "sr"
                | "atomic_script_run"
                | "asr" => {
                    return Err(format!(
                        "(*alpha_assertion) not recognized at offset {}",
                        self.pos.saturating_sub(1)
                    ));
                }
                _ => {
                    return Err(format!(
                        "(*VERB) not recognized or malformed at offset {}",
                        self.pos.saturating_sub(1)
                    ));
                }
            }
            return Ok(Node::Sequence(Vec::new()));
        }

        if delimiter == ':' {
            let verb_offset = self.pos;
            if !matches!(
                verb.as_str(),
                "atomic"
                    | "pla"
                    | "positive_lookahead"
                    | "napla"
                    | "non_atomic_positive_lookahead"
                    | "nla"
                    | "negative_lookahead"
                    | "plb"
                    | "positive_lookbehind"
                    | "naplb"
                    | "non_atomic_positive_lookbehind"
                    | "nlb"
                    | "negative_lookbehind"
                    | "script_run"
                    | "sr"
                    | "atomic_script_run"
                    | "asr"
            ) {
                return Err(format!(
                    "(*VERB) not recognized or malformed at offset {verb_offset}"
                ));
            }
            self.advance();
            let inner = self.parse_scoped_alternation()?;
            if self.advance() != Some(')') {
                return Err(format!("Unterminated (*{verb}:...) group"));
            }
            return Ok(match verb.as_str() {
                "atomic" => Node::Atomic(Box::new(inner)),
                "pla" | "positive_lookahead" => Node::Lookahead {
                    positive: true,
                    atomic: true,
                    inner: Box::new(inner),
                },
                "napla" | "non_atomic_positive_lookahead" => Node::Lookahead {
                    positive: true,
                    atomic: false,
                    inner: Box::new(inner),
                },
                "nla" | "negative_lookahead" => Node::Lookahead {
                    positive: false,
                    atomic: true,
                    inner: Box::new(inner),
                },
                "plb" | "positive_lookbehind" => Node::Lookbehind {
                    positive: true,
                    atomic: true,
                    inner: {
                        self.lookbehinds.push(inner.clone());
                        Box::new(inner)
                    },
                },
                "naplb" | "non_atomic_positive_lookbehind" => Node::Lookbehind {
                    positive: true,
                    atomic: false,
                    inner: {
                        self.lookbehinds.push(inner.clone());
                        Box::new(inner)
                    },
                },
                "nlb" | "negative_lookbehind" => Node::Lookbehind {
                    positive: false,
                    atomic: true,
                    inner: {
                        self.lookbehinds.push(inner.clone());
                        Box::new(inner)
                    },
                },
                "script_run" | "sr" => Node::ScriptRun {
                    inner: Box::new(inner),
                    atomic: false,
                },
                "atomic_script_run" | "asr" => Node::ScriptRun {
                    inner: Box::new(inner),
                    atomic: true,
                },
                _ => unreachable!("verb spelling was validated above"),
            });
        }

        Err(format!(
            "(*VERB) not recognized or malformed at offset {}",
            self.pos
        ))
    }

    fn parse_group(&mut self) -> Result<Node, String> {
        self.advance(); // consume '('

        // PCRE MARK control verb. Symfony's compiled route matcher uses the
        // short `(*:id)` spelling to identify the successful route branch.
        if self.peek() == Some('*') {
            self.advance();
            return self.parse_star_group();
        }

        // Check for special group types
        if self.peek() == Some('?') {
            self.advance(); // consume '?'
            if self.chars[self.pos..].starts_with(&['(', 'D', 'E', 'F', 'I', 'N', 'E', ')']) {
                self.start_items_allowed = false;
                self.pos += 8;
                loop {
                    self.skip_extended_spacing();
                    if self.peek() == Some(')') {
                        self.advance();
                        break;
                    }
                    if self.peek().is_none() {
                        return Err("Unterminated PCRE DEFINE block".into());
                    }
                    let definition = self.parse_group()?;
                    let Node::Group { name: Some(_), .. } = &definition else {
                        return Err("PCRE DEFINE entries must be named groups".into());
                    };
                }
                return Ok(Node::Sequence(Vec::new()));
            }

            let option_start = self.pos;
            if let Some(options) = self.try_parse_option_group(option_start)? {
                if !matches!(&options, Node::Sequence(nodes) if nodes.is_empty()) {
                    self.start_items_allowed = false;
                }
                return Ok(options);
            }
            self.pos = option_start;
            self.start_items_allowed = false;

            match self.peek() {
                Some('#') => {
                    self.advance();
                    while let Some(c) = self.advance() {
                        if c == ')' {
                            return Ok(Node::Sequence(Vec::new()));
                        }
                    }
                    Err("Unterminated PCRE comment".into())
                }
                Some('C') => self.parse_callout(),
                Some('(') => self.parse_conditional(),
                Some('&') => {
                    // Keep calls symbolic so DEFINE blocks can reference a
                    // group declared later in the block.
                    self.advance();
                    self.saw_subroutine = true;
                    let reference_offset = self.pos;
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        if c == ')' {
                            self.advance();
                            let target = SubroutineTarget::Name(name);
                            self.subroutine_references
                                .push((target.clone(), reference_offset));
                            return Ok(Node::Subroutine(target));
                        }
                        name.push(c);
                        self.advance();
                    }
                    Err("Unterminated named PCRE subroutine".into())
                }
                Some('R') => {
                    self.advance();
                    self.saw_subroutine = true;
                    if self.advance() != Some(')') {
                        return Err("Unterminated recursive PCRE subroutine".into());
                    }
                    Ok(Node::Subroutine(SubroutineTarget::WholePattern))
                }
                Some('+' | '-') => {
                    self.saw_subroutine = true;
                    let target = self.parse_signed_decimal_text();
                    if self.advance() != Some(')') {
                        return Err("Unterminated relative PCRE subroutine".into());
                    }
                    let target = self.subroutine_target_from_text(&target)?;
                    self.subroutine_references
                        .push((target.clone(), self.pos.saturating_sub(1)));
                    Ok(Node::Subroutine(target))
                }
                Some(c) if c.is_ascii_digit() => {
                    self.saw_subroutine = true;
                    let mut number = 0usize;
                    while let Some(c) = self.peek() {
                        let Some(digit) = c.to_digit(10) else {
                            break;
                        };
                        number = number
                            .checked_mul(10)
                            .and_then(|number| number.checked_add(digit as usize))
                            .ok_or("PCRE subroutine number is too large")?;
                        self.advance();
                    }
                    if self.advance() != Some(')') {
                        return Err("Unterminated numeric PCRE subroutine".into());
                    }
                    let target = if number == 0 {
                        SubroutineTarget::WholePattern
                    } else {
                        SubroutineTarget::Group(number)
                    };
                    if !matches!(target, SubroutineTarget::WholePattern) {
                        self.subroutine_references
                            .push((target.clone(), self.pos.saturating_sub(1)));
                    }
                    Ok(Node::Subroutine(target))
                }
                Some('|') => {
                    // Branch-reset group (?|...): capture numbering restarts
                    // at the same base for every alternative and continues
                    // after the largest branch number.
                    self.advance();
                    let outer_options = self.option_state();
                    let base_group_count = self.group_count;
                    let mut max_group_count = base_group_count;
                    let mut branches = Vec::new();
                    let result: Result<(), String> = (|| {
                        loop {
                            self.group_count = base_group_count;
                            branches.push(self.parse_sequence()?);
                            max_group_count = max_group_count.max(self.group_count);
                            match self.advance() {
                                Some('|') => continue,
                                Some(')') => break,
                                _ => return Err("Unterminated branch-reset group".into()),
                            }
                        }
                        Ok(())
                    })();
                    self.restore_option_state(outer_options);
                    result?;
                    self.group_count = max_group_count;
                    let inner = if branches.len() == 1 {
                        branches.pop().unwrap()
                    } else {
                        Node::Alternation(branches)
                    };
                    Ok(Self::noncapturing_group(inner))
                }
                Some(':') => {
                    // Non-capturing group (?:...)
                    self.advance();
                    let inner = self.parse_scoped_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated non-capturing group".into());
                    }
                    Ok(Self::noncapturing_group(inner))
                }
                Some('>') => {
                    self.advance();
                    let inner = self.parse_scoped_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated atomic group".into());
                    }
                    Ok(Node::Atomic(Box::new(inner)))
                }
                Some('=') => {
                    // Positive lookahead (?=...)
                    self.advance();
                    let inner = self.parse_lookaround_inner()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated lookahead".into());
                    }
                    Ok(Node::Lookahead {
                        positive: true,
                        atomic: true,
                        inner: Box::new(inner),
                    })
                }
                Some('!') => {
                    // Negative lookahead (?!...)
                    self.advance();
                    let inner = self.parse_lookaround_inner()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated lookahead".into());
                    }
                    Ok(Node::Lookahead {
                        positive: false,
                        atomic: true,
                        inner: Box::new(inner),
                    })
                }
                Some('*') => {
                    // PCRE2 non-atomic positive lookahead shorthand. The
                    // ordinary assertion path is equivalent until a later
                    // failure requests re-entry; the dedicated backtracking
                    // continuation is handled by the same captured states.
                    self.advance();
                    let inner = self.parse_lookaround_inner()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated non-atomic lookahead".into());
                    }
                    Ok(Node::Lookahead {
                        positive: true,
                        atomic: false,
                        inner: Box::new(inner),
                    })
                }
                Some('<') => {
                    self.advance(); // consume '<'
                    match self.peek() {
                        Some('=') => {
                            // Positive lookbehind (?<=...)
                            self.advance();
                            let inner = self.parse_lookaround_inner()?;
                            self.lookbehinds.push(inner.clone());
                            if self.advance() != Some(')') {
                                return Err("Unterminated lookbehind".into());
                            }
                            Ok(Node::Lookbehind {
                                positive: true,
                                atomic: true,
                                inner: Box::new(inner),
                            })
                        }
                        Some('!') => {
                            // Negative lookbehind (?<!...)
                            self.advance();
                            let inner = self.parse_lookaround_inner()?;
                            self.lookbehinds.push(inner.clone());
                            if self.advance() != Some(')') {
                                return Err("Unterminated lookbehind".into());
                            }
                            Ok(Node::Lookbehind {
                                positive: false,
                                atomic: true,
                                inner: Box::new(inner),
                            })
                        }
                        Some('*') => {
                            self.advance();
                            let inner = self.parse_lookaround_inner()?;
                            self.lookbehinds.push(inner.clone());
                            if self.advance() != Some(')') {
                                return Err("Unterminated non-atomic lookbehind".into());
                            }
                            Ok(Node::Lookbehind {
                                positive: true,
                                atomic: false,
                                inner: Box::new(inner),
                            })
                        }
                        _ => {
                            // Named group (?<name>...)
                            let name = self.parse_group_name('>')?;
                            self.group_count += 1;
                            let idx = self.group_count;
                            self.register_named_group(&name, idx)?;
                            let inner = self.parse_scoped_alternation()?;
                            if self.advance() != Some(')') {
                                return Err("Unterminated named group".into());
                            }
                            Ok(self.capture_group(idx, Some(name), inner))
                        }
                    }
                }
                Some('\'') => {
                    // Perl-compatible apostrophe spelling: (?'name'...).
                    self.advance();
                    let name = self.parse_group_name('\'')?;
                    self.group_count += 1;
                    let idx = self.group_count;
                    self.register_named_group(&name, idx)?;
                    let inner = self.parse_scoped_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated named group".into());
                    }
                    Ok(self.capture_group(idx, Some(name), inner))
                }
                Some('P') => {
                    self.advance(); // consume 'P'
                    if self.peek() == Some('=') {
                        // (?P=name) — named backreference
                        self.advance(); // consume '='
                        let name = self.parse_group_name(')')?;
                        return Ok(Node::NamedBackreference(name));
                    }
                    if self.peek() == Some('>') {
                        self.advance();
                        self.saw_subroutine = true;
                        let reference_offset = self.pos;
                        let name = self.parse_delimited_text(')')?;
                        let target = SubroutineTarget::Name(name);
                        self.subroutine_references
                            .push((target.clone(), reference_offset));
                        return Ok(Node::Subroutine(target));
                    }
                    // (?P<name>...)
                    if self.advance() != Some('<') {
                        return Err("Expected '<' or '=' after (?P".into());
                    }
                    let name = self.parse_group_name('>')?;
                    self.group_count += 1;
                    let idx = self.group_count;
                    self.register_named_group(&name, idx)?;
                    let inner = self.parse_scoped_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated named group".into());
                    }
                    Ok(self.capture_group(idx, Some(name), inner))
                }
                _ => Err(format!(
                    "Unknown group modifier '?{}'",
                    self.peek().unwrap_or(' ')
                )),
            }
        } else {
            // Capturing group
            self.start_items_allowed = false;
            if self.no_auto_capture {
                let inner = self.parse_scoped_alternation()?;
                if self.advance() != Some(')') {
                    return Err("Unterminated non-capturing group".into());
                }
                return Ok(Self::noncapturing_group(inner));
            }
            self.group_count += 1;
            let idx = self.group_count;
            let inner = self.parse_scoped_alternation()?;
            if self.advance() != Some(')') {
                return Err("Unterminated capturing group".into());
            }
            Ok(self.capture_group(idx, None, inner))
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
    fn parse_conditional(&mut self) -> Result<Node, String> {
        self.advance(); // consume the condition's '('
        let condition = match self.peek() {
            Some('R') => {
                self.advance();
                self.saw_subroutine = true;
                if self.peek() == Some('&') {
                    self.advance();
                    let mut name = String::new();
                    while self.peek().is_some_and(|c| c != ')') {
                        name.push(self.advance().unwrap());
                    }
                    CaptureCondition::RecursionTarget(SubroutineTarget::Name(name))
                } else if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    let digits = self.parse_signed_decimal_text();
                    CaptureCondition::AmbiguousRecursion {
                        name: format!("R{digits}"),
                        target: Some(self.subroutine_target_from_text(&digits)?),
                    }
                } else {
                    CaptureCondition::AmbiguousRecursion {
                        name: "R".to_string(),
                        target: None,
                    }
                }
            }
            Some('+' | '-' | '0'..='9') => {
                let reference = self.parse_signed_decimal_text();
                let Node::Backreference(index) = self.backreference_from_text(&reference)? else {
                    unreachable!()
                };
                CaptureCondition::Group(index)
            }
            Some('<') | Some('\'') => {
                let delimiter = self.advance().unwrap();
                let closing = if delimiter == '<' { '>' } else { '\'' };
                let name_offset = self.pos;
                let name = self.parse_group_name(closing)?;
                self.conditional_name_references
                    .push((name.clone(), name_offset));
                CaptureCondition::Name(name)
            }
            Some('?') => {
                self.advance();
                let (positive, lookbehind) = match self.advance() {
                    Some('=') => (true, false),
                    Some('!') => (false, false),
                    Some('<') => match self.advance() {
                        Some('=') => (true, true),
                        Some('!') => (false, true),
                        _ => return Err("assertion expected after (?< in condition".into()),
                    },
                    _ => {
                        return Err(format!(
                            "assertion expected after (?( or (?(?C) at offset {}",
                            self.pos + 1
                        ));
                    }
                };
                let inner = self.parse_lookaround_inner()?;
                if lookbehind {
                    self.lookbehinds.push(inner.clone());
                }
                CaptureCondition::Assertion {
                    positive,
                    lookbehind,
                    inner: Box::new(inner),
                }
            }
            Some(_) => {
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    if c == ')' {
                        break;
                    }
                    name.push(c);
                    self.advance();
                }
                if name.starts_with("VERSION") {
                    CaptureCondition::Always(Self::version_condition_matches(&name)?)
                } else {
                    let offset = self.pos.saturating_sub(name.chars().count());
                    self.conditional_name_references
                        .push((name.clone(), offset));
                    CaptureCondition::Name(name)
                }
            }
            None => return Err("Unterminated PCRE condition".into()),
        };
        if self.advance() != Some(')') {
            return Err("Unterminated PCRE condition".into());
        }
        let outer = self.option_state();
        let result: Result<(Node, Option<Box<Node>>), String> = (|| {
            let yes = self.parse_sequence()?;
            let no = if self.peek() == Some('|') {
                self.advance();
                Some(Box::new(self.parse_sequence()?))
            } else {
                None
            };
            if self.advance() != Some(')') {
                return Err("Unterminated PCRE conditional group".into());
            }
            Ok((yes, no))
        })();
        self.restore_option_state(outer);
        let (yes, no) = result?;
        Ok(Node::Conditional {
            condition,
            yes: Box::new(yes),
            no,
        })
    }

    fn version_condition_matches(condition: &str) -> Result<bool, String> {
        let rest = condition
            .strip_prefix("VERSION")
            .ok_or("invalid VERSION condition")?;
        let (at_least, version) = if let Some(version) = rest.strip_prefix(">=") {
            (true, version)
        } else if let Some(version) = rest.strip_prefix('=') {
            (false, version)
        } else {
            return Err("malformed VERSION condition".into());
        };
        let mut components = version.split('.');
        let major = components
            .next()
            .ok_or("missing VERSION number")?
            .parse::<u16>()
            .map_err(|_| "invalid VERSION number")?;
        let minor = components
            .next()
            .unwrap_or("0")
            .parse::<u16>()
            .map_err(|_| "invalid VERSION number")?;
        if components.next().is_some() {
            return Err("invalid VERSION number".into());
        }
        let current = (10u16, 42u16);
        Ok(if at_least {
            current >= (major, minor)
        } else {
            current == (major, minor)
        })
    }

    fn skip_extended_spacing(&mut self) {
        if !self.flags.extended {
            return;
        }
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.advance();
            }
            if self.peek() != Some('#') {
                break;
            }
            while let Some(c) = self.advance() {
                if c == '\n' {
                    break;
                }
            }
        }
    }

    /// Parse `(?im-sx)`, `(?im-sx:...)`, `(?^)`, and the PCRE-specific J/U
    /// switches. Runtime flags are attached only to deterministic atoms while
    /// parse-time flags affect the remainder of the current lexical group.
    fn try_parse_option_group(&mut self, start: usize) -> Result<Option<Node>, String> {
        if self.peek() == Some('-')
            && self
                .chars
                .get(self.pos + 1)
                .is_some_and(|c| c.is_ascii_digit())
        {
            return Ok(None);
        }
        let outer = self.option_state();
        let mut state = outer;
        let mut disabling = false;
        let mut saw_hyphen = false;
        let mut saw_option = false;
        let mut reset_standard = false;
        let mut set_x_count = 0usize;

        if self.peek() == Some('^') {
            self.advance();
            saw_option = true;
            reset_standard = true;
            state.flags.case_insensitive = false;
            state.flags.multiline = false;
            state.flags.dotall = false;
            state.flags.extended = false;
            state.no_auto_capture = false;
            state.extended_more = false;
        }

        while let Some(option) = self.peek() {
            if option == '-' {
                if reset_standard || saw_hyphen {
                    self.restore_option_state(outer);
                    self.pos = start;
                    return Err("invalid hyphen in option setting".into());
                }
                saw_hyphen = true;
                disabling = true;
                saw_option = true;
                self.advance();
                continue;
            }
            if !matches!(option, 'i' | 'm' | 'n' | 's' | 'x' | 'J' | 'U') {
                break;
            }
            saw_option = true;
            self.advance();
            let enabled = !disabling;
            match option {
                'i' => state.flags.case_insensitive = enabled,
                'm' => state.flags.multiline = enabled,
                'n' => state.no_auto_capture = enabled,
                's' => state.flags.dotall = enabled,
                'x' => {
                    if enabled {
                        set_x_count += 1;
                        state.flags.extended = true;
                        state.extended_more = set_x_count >= 2;
                    } else {
                        state.flags.extended = false;
                        state.extended_more = false;
                    }
                }
                'J' => state.duplicate_names = enabled,
                'U' => state.flags.ungreedy = enabled,
                _ => unreachable!(),
            }
        }

        // `(?)` is a valid empty setting. Anything else that did not look
        // like an option belongs to another `(?...)` construct.
        if !saw_option && self.peek() != Some(')') {
            self.restore_option_state(outer);
            self.pos = start;
            return Ok(None);
        }

        match self.peek() {
            Some(')') => {
                self.advance();
                self.restore_option_state(state);
                Ok(Some(Node::Sequence(Vec::new())))
            }
            Some(':') => {
                self.advance();
                self.restore_option_state(state);
                let inner = self.parse_alternation();
                self.restore_option_state(outer);
                let inner = inner?;
                if self.advance() != Some(')') {
                    return Err("Unterminated scoped PCRE option group".into());
                }
                Ok(Some(Self::noncapturing_group(inner)))
            }
            _ => {
                self.restore_option_state(outer);
                self.pos = start;
                if saw_option {
                    Err("invalid option setting".into())
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        // Anchors and assertions can't be quantified
        match &atom {
            Node::Anchor(_)
            | Node::WordBoundary(_)
            | Node::Mark(_)
            | Node::Control { .. }
            | Node::ResetStart => {
                return Ok(atom);
            }
            _ => {}
        }

        let default_greedy = !self.flags.ungreedy;

        match self.peek() {
            Some('*') => {
                self.advance();
                let (greedy, possessive) = self.quantifier_mode(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 0,
                    max: None,
                    greedy,
                    possessive,
                })
            }
            Some('+') => {
                self.advance();
                let (greedy, possessive) = self.quantifier_mode(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 1,
                    max: None,
                    greedy,
                    possessive,
                })
            }
            Some('?') => {
                self.advance();
                let (greedy, possessive) = self.quantifier_mode(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 0,
                    max: Some(1),
                    greedy,
                    possessive,
                })
            }
            Some('{') => {
                if !self.counted_quantifier_has_complete_syntax() {
                    // PCRE recognizes a counted quantifier only when its
                    // entire closing syntax is present. Thus `a{12`,
                    // `a{12x}` and `a{12,` all leave the brace literal, while
                    // a complete but invalid `a{12,3}` is a compile error.
                    return Ok(atom);
                }
                self.advance();
                let (min, max) = self.parse_counted_quantifier()?;
                let (greedy, possessive) = self.quantifier_mode(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min,
                    max,
                    greedy,
                    possessive,
                })
            }
            _ => Ok(atom),
        }
    }

    fn counted_quantifier_has_complete_syntax(&self) -> bool {
        debug_assert_eq!(self.chars.get(self.pos), Some(&'{'));
        let mut cursor = self.pos + 1;
        let digits_start = cursor;
        while self
            .chars
            .get(cursor)
            .is_some_and(|character| character.is_ascii_digit())
        {
            cursor += 1;
        }
        if cursor == digits_start {
            return false;
        }
        match self.chars.get(cursor) {
            Some('}') => true,
            Some(',') => {
                cursor += 1;
                while self
                    .chars
                    .get(cursor)
                    .is_some_and(|character| character.is_ascii_digit())
                {
                    cursor += 1;
                }
                self.chars.get(cursor) == Some(&'}')
            }
            _ => false,
        }
    }

    fn quantifier_mode(&mut self, default_greedy: bool) -> (bool, bool) {
        if self.peek() == Some('?') {
            self.advance();
            (!default_greedy, false)
        } else if self.peek() == Some('+') {
            self.advance();
            (true, true)
        } else {
            (default_greedy, false)
        }
    }

    fn parse_counted_quantifier(&mut self) -> Result<(usize, Option<usize>), String> {
        let mut num_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
                num_str.push(c);
            } else {
                break;
            }
        }
        let min: usize = num_str.parse().map_err(|_| "Invalid quantifier")?;

        match self.peek() {
            Some('}') => {
                self.advance();
                if min > 65_535 {
                    return Err(format!(
                        "number too big in {{}} quantifier at offset {}",
                        self.pos.saturating_sub(1)
                    ));
                }
                Ok((min, Some(min))) // {n}
            }
            Some(',') => {
                self.advance();
                let mut max_str = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.advance();
                        max_str.push(c);
                    } else {
                        break;
                    }
                }
                if self.advance() != Some('}') {
                    return Err("Expected '}' in quantifier".into());
                }
                if max_str.is_empty() {
                    if min > 65_535 {
                        return Err(format!(
                            "number too big in {{}} quantifier at offset {}",
                            self.pos.saturating_sub(1)
                        ));
                    }
                    Ok((min, None)) // {n,}
                } else {
                    let max: usize = max_str.parse().map_err(|_| "Invalid quantifier max")?;
                    let offset = self.pos.saturating_sub(1);
                    if min > 65_535 || max > 65_535 {
                        return Err(format!(
                            "number too big in {{}} quantifier at offset {offset}"
                        ));
                    }
                    if min > max {
                        return Err(format!(
                            "numbers out of order in {{}} quantifier at offset {offset}"
                        ));
                    }
                    Ok((min, Some(max))) // {n,m}
                }
            }
            _ => Err("Expected ',' or '}' in quantifier".into()),
        }
    }
}

// ── PHP delimiter parser (public utility) ───────────────────────────────────

/// Parse a PHP-style regex like `/pattern/flags` into (pattern, flags).
/// Supports paired delimiters: `{...}`, `(...)`, `[...]`, `<...>`.
pub fn parse_php_regex(input: &str) -> Result<(String, RegexFlags), String> {
    let bytes = input.as_bytes();
    // PHP skips ASCII whitespace before looking for the delimiter.  Modifier
    // parsing is deliberately narrower: only spaces and line endings are
    // ignored there, while tabs and the other control-space bytes are unknown
    // modifiers.  `str::trim()` therefore cannot model either boundary.
    let mut start = 0;
    while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
        start += 1;
    }
    if start == bytes.len() {
        return Err("Empty regular expression".into());
    }
    let open = bytes[start];
    if open.is_ascii_alphanumeric() || open == b'\\' || open == 0 {
        return Err("Delimiter must not be alphanumeric, backslash, or NUL byte".into());
    }
    let close = match open {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        _ => open, // symmetric delimiter like / ~ # !
    };

    let is_paired = matches!(open, b'{' | b'(' | b'[' | b'<');
    let mut depth = 1usize;
    let mut escaped = false;
    let mut close_pos = None;
    for (position, &byte) in bytes.iter().enumerate().skip(start + 1) {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if is_paired && byte == open {
            depth += 1;
            continue;
        }
        if byte == close {
            depth -= 1;
            if depth == 0 {
                close_pos = Some(position);
                break;
            }
        }
    }
    let close_pos = close_pos.ok_or_else(|| {
        if is_paired {
            format!("No ending matching delimiter '{}' found", close as char)
        } else {
            format!("No ending delimiter '{}' found", close as char)
        }
    })?;
    let pattern = &input[start + 1..close_pos];
    let flags_str = &input[close_pos + 1..];

    let mut flags = RegexFlags::default();
    let mut duplicate_names = false;
    let mut no_auto_capture = false;
    for ch in flags_str.chars() {
        match ch {
            'i' => flags.case_insensitive = true,
            'm' => flags.multiline = true,
            's' => flags.dotall = true,
            'x' => flags.extended = true,
            'U' => flags.ungreedy = true,
            'u' => flags.unicode_mode = UnicodeMode::UtfUcp,
            // PCRE's study modifier is an optimization hint and cannot alter
            // observable match or replacement results.
            'S' | 'X' => {}
            'D' => flags.line_options.0 |= LineOptions::DOLLAR_END_ONLY,
            'A' => flags.anchored = true,
            // Duplicate-name handling is a compile-time parser option. Prefix
            // its inline equivalent so RegexFlags and MatchCtx retain their
            // common-path layout.
            'J' => duplicate_names = true,
            'n' => no_auto_capture = true,
            ' ' | '\n' | '\r' => {}
            '\0' => return Err("NUL byte is not a valid modifier".into()),
            _ => return Err(format!("Unknown modifier '{}'", ch)),
        }
    }
    let pattern = match (duplicate_names, no_auto_capture) {
        (true, true) => format!("(?Jn){pattern}"),
        (true, false) => format!("(?J){pattern}"),
        (false, true) => format!("(?n){pattern}"),
        (false, false) => pattern.to_string(),
    };
    Ok((pattern, flags))
}

#[cfg(test)]
#[path = "regex/tests.rs"]
mod tests;
