//! Custom PCRE-compatible backtracking regex engine.
//!
//! Supports the subset of PCRE commonly used in PHP:
//! - Literals, `.` (any char), `^`, `$`
//! - Character classes `[abc]`, `[a-z]`, `[^abc]`, `\d`, `\w`, `\s`, `\D`, `\W`, `\S`, `\b`, `\B`
//! - Quantifiers `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}` (greedy by default, lazy with `?`)
//! - Grouping `(...)`, non-capturing `(?:...)`, named `(?P<name>...)`, `(?<name>...)`
//! - Alternation `|`
//! - Backreferences `\1`..`\99`
//! - Lookahead `(?=...)`, `(?!...)`
//! - Lookbehind `(?<=...)`, `(?<!...)`
//! - Escape sequences `\n`, `\r`, `\t`, `\\`, `\/`
//!
//! Flags: `i` (case-insensitive), `m` (multiline), `s` (dotall), `x` (extended/comments), `U` (ungreedy), `u` (UTF-8), `S` (study hint)

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

mod linear;
mod unicode;
mod unicode_categories;
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
        inner: Box<Node>,
    },
    Lookbehind {
        positive: bool,
        inner: Box<Node>,
    },
    /// Atomic group `(?>...)`: the selected inner path cannot be revisited
    /// when a later node fails.
    Atomic(Box<Node>),
    /// PCRE `(*MARK:name)` / `(*:name)`: publish the last successful mark as
    /// the synthetic named capture `MARK` without consuming input.
    Mark(String),
    WordBoundary(bool), // true = \b, false = \B
    /// `\X`: one extended grapheme cluster.
    GraphemeCluster,
    /// `\R`: one newline sequence, with `\r\n` consumed atomically.
    Linebreak,
}

#[derive(Debug, Clone)]
enum Anchor {
    Start,         // ^
    End,           // $
    AbsoluteStart, // \A
}

#[derive(Debug, Clone)]
enum ClassItem {
    Literal(char),
    Range(char, char),
    Shorthand(Shorthand),
}

#[derive(Debug, Clone, Copy)]
enum Shorthand {
    Digit,    // \d
    NonDigit, // \D
    Word,     // \w
    NonWord,  // \W
    Space,    // \s
    NonSpace, // \S
    /// `\p{…}` / `\P{…}` Unicode property class.
    Property(unicode::PropertyClass),
}

// ── Flags ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct RegexFlags {
    pub case_insensitive: bool,
    pub multiline: bool,
    pub dotall: bool,
    pub extended: bool,
    pub ungreedy: bool,
    pub dollar_end_only: bool,
    /// PCRE UTF mode. Besides requiring valid UTF-8 at the public preg_*
    /// boundary, PHP 8.5 enables Unicode character properties for the word
    /// and whitespace shorthand classes admitted by this checkpoint.
    pub unicode: bool,
    /// PCRE `A` modifier: a match may only start at the search position, and
    /// each further match must begin where the previous one ended.
    pub anchored: bool,
}

impl Default for RegexFlags {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            multiline: false,
            dotall: false,
            extended: false,
            ungreedy: false,
            dollar_end_only: false,
            unicode: false,
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
        return chars.get(pos) == Some(&'\n');
    }
    if flags.dollar_end_only {
        return false;
    }
    (pos + 1 == chars.len() && chars[pos] == '\n')
        || (pos + 2 == chars.len() && chars[pos] == '\r' && chars[pos + 1] == '\n')
}

// ── Public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Regex {
    ast: Node,
    flags: RegexFlags,
    num_groups: usize,
    /// Named group name → group index
    named_groups: HashMap<String, usize>,
    /// Literal that every match must start with, when it can be proven from
    /// the AST. Used to skip impossible start positions before backtracking.
    start_literal: Option<char>,
    /// Whether boolean matching must retain capture contents for a later
    /// numeric or named backreference in the pattern.
    uses_backreferences: bool,
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
        if !flags.unicode && !pattern.is_ascii() {
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

impl Default for RegexCache {
    fn default() -> Self {
        Self::new(DEFAULT_REGEX_CACHE_CAPACITY)
    }
}

#[derive(Debug, Clone)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

impl Match {
    pub fn as_str<'a>(&self, input: &'a str) -> &'a str {
        &input[self.start..self.end]
    }
}

#[derive(Debug, Clone)]
pub struct Captures {
    /// Group 0 = full match, group 1..N = capture groups.
    groups: Vec<Option<Match>>,
    /// Named group name → group index
    named_groups: HashMap<String, usize>,
    mark: Option<String>,
}

impl Captures {
    pub fn get(&self, i: usize) -> Option<&Match> {
        self.groups.get(i).and_then(|m| m.as_ref())
    }

    pub fn get_named(&self, name: &str) -> Option<&Match> {
        self.named_groups.get(name).and_then(|&i| self.get(i))
    }

    pub fn named_groups(&self) -> &HashMap<String, usize> {
        &self.named_groups
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
    named_groups: &'a HashMap<String, usize>,
    mark: Option<&'a str>,
}

impl CaptureView<'_> {
    pub(crate) fn get(&self, i: usize) -> Option<&Match> {
        self.groups.get(i).and_then(|capture| capture.as_ref())
    }

    pub(crate) fn named_groups(&self) -> &HashMap<String, usize> {
        self.named_groups
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
    pub(crate) fn is_unicode(&self) -> bool {
        self.flags.unicode
    }

    /// Projection consumers need the declared slots even when there is no
    /// match. This exposes metadata only, not another matching path.
    pub(crate) fn capture_count(&self) -> usize {
        self.num_groups + 1
    }

    pub(crate) fn capture_names(&self) -> &HashMap<String, usize> {
        &self.named_groups
    }

    /// Compile a regex pattern with given flags.
    pub fn new(pattern: &str, flags: RegexFlags) -> Result<Self, String> {
        let mut parser = Parser::new(pattern, flags);
        let ast = parser.parse()?;
        let start_literal = required_start_literal(&ast);
        let uses_backreferences = contains_backreference(&ast);
        Ok(Self {
            ast,
            flags,
            num_groups: parser.group_count,
            named_groups: parser.named_groups,
            start_literal,
            uses_backreferences,
        })
    }

    /// Test whether the pattern matches without materializing capture output.
    /// Internal capture slots are retained only when the pattern needs their
    /// contents for backreferences.
    #[inline(always)]
    pub fn is_match(&self, subject: &str) -> bool {
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
        let mut start = 0;
        while start <= chars.len() {
            if let Some(literal) = self.start_literal
                && !self.flags.anchored
            {
                let Some(relative_start) = chars[start..].iter().position(|&candidate| {
                    chars_equal(candidate, literal, self.flags.case_insensitive)
                }) else {
                    break;
                };
                start += relative_start;
            }
            groups.fill(None);
            let mut mark = None;
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
                mark: &mut mark,
            };
            if match_seq_from(&self.ast, &[], start, &mut ctx).is_some() {
                return true;
            }
            if self.flags.anchored {
                break;
            }
            start += 1;
        }
        false
    }

    /// Find first match in subject.  Returns captures (group 0 = whole match).
    pub fn captures(&self, subject: &str) -> Option<Captures> {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut groups = vec![None; self.num_groups + 1];
        // Try matching at every position; an anchored pattern only at the first.
        for start in 0..=chars.len() {
            if self.flags.anchored && start > 0 {
                break;
            }
            if let Some(literal) = self.start_literal {
                if start == chars.len()
                    || !chars_equal(chars[start], literal, self.flags.case_insensitive)
                {
                    continue;
                }
            }
            groups.fill(None);
            let mut mark = None;
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
                    mark: &mut mark,
                };
                match_seq_from(&self.ast, &[], start, &mut ctx)
            };
            if let Some(end) = end {
                groups[0] = Some(Match {
                    start: byte_offsets.get(start),
                    end: byte_offsets.get(end),
                });
                return Some(Captures {
                    groups,
                    named_groups: self.named_groups.clone(),
                    mark,
                });
            }
        }
        None
    }

    /// Replace all occurrences.  Replacement can use `$1`, `$10`, `${2}`, `\\1` backrefs.
    pub fn replace_all(&self, subject: &str, replacement: &str) -> String {
        self.replace_limit(subject, replacement, usize::MAX).0
    }

    /// Replace at most `limit` occurrences and return the replacement count.
    pub fn replace_limit(&self, subject: &str, replacement: &str, limit: usize) -> (String, usize) {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut result = String::new();
        let mut pos = 0;
        let mut count = 0;

        while pos <= chars.len() {
            if count == limit {
                result.push_str(&subject[byte_offsets.get(pos)..]);
                break;
            }
            let mut groups = vec![None; self.num_groups + 1];
            let mut mark = None;
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
                mark: &mut mark,
            };
            if let Some(end) = match_seq_from(&self.ast, &[], pos, &mut ctx) {
                let match_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                ctx.groups[0] = Some(Match {
                    start: match_start,
                    end: match_end,
                });

                // Append text before match
                result.push_str(&subject[byte_offsets.get(pos)..match_start]);
                // Append replacement with backreference expansion
                result.push_str(&expand_replacement(replacement, &groups, subject));
                count += 1;

                if end == pos {
                    // Zero-length match — advance by one to avoid infinite loop
                    if pos < chars.len() {
                        result.push(chars[pos]);
                    }
                    pos += 1;
                } else {
                    pos = end;
                }
            } else if self.flags.anchored {
                result.push_str(&subject[byte_offsets.get(pos)..]);
                break;
            } else {
                if pos < chars.len() {
                    result.push(chars[pos]);
                }
                pos += 1;
            }
        }
        (result, count)
    }

    /// Count non-overlapping matches without publishing capture data.
    #[inline(always)]
    pub(crate) fn count_matches(&self, subject: &str) -> usize {
        if self.num_groups == 0
            && linear::is_supported(&self.ast)
            && let Some(count) = linear::try_count_matches(self, subject)
        {
            return count;
        }

        let count: Result<usize, std::convert::Infallible> =
            self.try_visit_captures(subject, |_| Ok(true));
        count.unwrap()
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
        if self.num_groups == 0 && linear::is_supported(&self.ast) {
            linear::try_visit_captures(self, subject, visitor)
        } else {
            self.try_visit_backtracking_captures(subject, visitor)
        }
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
        let mut count = 0;
        let start_literal = self.start_literal.filter(|_| !self.flags.anchored);

        while pos <= chars.len() {
            if let Some(literal) = start_literal {
                let Some(relative_pos) = chars[pos..].iter().position(|&candidate| {
                    chars_equal(candidate, literal, self.flags.case_insensitive)
                }) else {
                    break;
                };
                pos += relative_pos;
            }
            groups.fill(None);
            let mut mark = None;
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
                    mark: &mut mark,
                };
                match_seq_from(&self.ast, &[], pos, &mut ctx)
            };
            if let Some(end) = end {
                let match_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                groups[0] = Some(Match {
                    start: match_start,
                    end: match_end,
                });
                count += 1;
                let keep_scanning = visitor(CaptureView {
                    groups: &groups,
                    named_groups: &self.named_groups,
                    mark: mark.as_deref(),
                })?;
                if !keep_scanning {
                    break;
                }
                if end == pos {
                    pos += 1; // avoid infinite loop on zero-length match
                } else {
                    pos = end;
                }
            } else if self.flags.anchored {
                break;
            } else {
                pos += 1;
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
                    named_groups: captures.named_groups.clone(),
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
            for try_start in scan..=chars.len() {
                if self.flags.anchored && try_start > scan {
                    break;
                }
                let mut groups = vec![None; self.num_groups + 1];
                let mut mark = None;
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
                    mark: &mut mark,
                };
                if let Some(end) = match_seq_from(&self.ast, &[], try_start, &mut ctx) {
                    let match_start_byte = byte_offsets.get(try_start);
                    // Don't split on zero-length match at same position
                    if end == try_start && try_start == last_end && try_start < chars.len() {
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

        while pos <= chars.len() {
            let mut groups = vec![None; self.num_groups + 1];
            let mut mark = None;
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
                mark: &mut mark,
            };
            if let Some(end) = match_seq_from(&self.ast, &[], pos, &mut ctx) {
                let match_start = byte_offsets.get(pos);
                let match_end = byte_offsets.get(end);
                ctx.groups[0] = Some(Match {
                    start: match_start,
                    end: match_end,
                });

                result.push_str(&subject[byte_offsets.get(pos)..match_start]);
                let caps = Captures {
                    groups: groups.clone(),
                    named_groups: self.named_groups.clone(),
                    mark: mark.clone(),
                };
                result.push_str(&replacer(&caps, subject));

                if end == pos {
                    if pos < chars.len() {
                        result.push(chars[pos]);
                    }
                    pos += 1;
                } else {
                    pos = end;
                }
            } else if self.flags.anchored {
                result.push_str(&subject[byte_offsets.get(pos)..]);
                break;
            } else {
                if pos < chars.len() {
                    result.push(chars[pos]);
                }
                pos += 1;
            }
        }
        result
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
                    | Node::Mark(_) => continue,
                    _ => return required_start_literal(node),
                }
            }
            None
        }
        Node::Group { inner, .. } => required_start_literal(inner),
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
        | Node::Atomic(inner) => contains_backreference(inner),
        Node::Alternation(nodes) | Node::Sequence(nodes) => {
            nodes.iter().any(contains_backreference)
        }
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
                // $N — parse multi-digit group number
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
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
            // \N — parse multi-digit group number
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
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

struct MatchCtx<'a> {
    chars: &'a [char],
    metadata: &'a MatchMetadata<'a>,
    flags: RegexFlags,
    groups: &'a mut Vec<Option<Match>>,
    named_groups: &'a HashMap<String, usize>,
    mark: &'a mut Option<String>,
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
        Node::Literal(_) | Node::AnyChar | Node::CharClass { .. } | Node::Shorthand(_) => {
            Some((1, 1))
        }
        Node::Anchor(_)
        | Node::WordBoundary(_)
        | Node::Lookahead { .. }
        | Node::Lookbehind { .. }
        | Node::Mark(_) => Some((0, 0)),
        Node::Group { inner, .. } | Node::Atomic(inner) => node_length_range(inner),
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
        | Node::Linebreak => None,
    }
}

fn match_seq_from(node: &Node, rest: &[Node], pos: usize, ctx: &mut MatchCtx) -> Option<usize> {
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
            let matches = if ctx.flags.case_insensitive {
                ctx.chars[pos].to_lowercase().eq(ch.to_lowercase())
            } else {
                ctx.chars[pos] == *ch
            };
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
            if ctx.flags.dotall || ctx.chars[pos] != '\n' {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::Anchor(Anchor::Start) => {
            let ok = if ctx.flags.multiline {
                pos == 0 || (pos > 0 && ctx.chars[pos - 1] == '\n')
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
        Node::Anchor(Anchor::End) => {
            let ok = end_anchor_matches(pos, ctx.chars, ctx.flags);
            if ok { match_rest(rest, pos, ctx) } else { None }
        }
        Node::WordBoundary(positive) => {
            let at_boundary = is_word_boundary(ctx.chars, pos, ctx.flags.unicode);
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
            if match_shorthand(*sh, ctx.chars[pos], ctx.flags.unicode) {
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
            } else if matches!(
                first,
                '\n' | '\u{0b}' | '\u{0c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
            ) {
                1
            } else {
                return None;
            };
            match_rest(rest, pos + length, ctx)
        }
        Node::Alternation(branches) => {
            for branch in branches {
                let saved_groups = ctx.groups.clone();
                let saved_mark = ctx.mark.clone();
                if let Some(end) = match_seq_from(branch, rest, pos, ctx) {
                    return Some(end);
                }
                *ctx.groups = saved_groups;
                *ctx.mark = saved_mark;
            }
            None
        }
        Node::Group {
            index,
            name: _,
            inner,
        } => {
            let tracked_index = index.filter(|idx| *idx < ctx.groups.len());
            let start_offset = tracked_index.map_or(0, |_| ctx.metadata.byte_offsets.get(pos));
            match_seq_from_with_group(inner, rest, pos, ctx, tracked_index, start_offset)
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            possessive,
        } => match_quantifier(inner, *min, *max, *greedy, *possessive, rest, pos, ctx),
        Node::Backreference(n) => match_backref_by_index(*n, rest, pos, ctx),
        Node::NamedBackreference(name) => {
            if let Some(&idx) = ctx.named_groups.get(name.as_str()) {
                match_backref_by_index(idx, rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Lookahead { positive, inner } => {
            let saved = ctx.groups.clone();
            let saved_mark = ctx.mark.clone();
            // Use empty rest — lookahead doesn't consume input, just checks
            let result = match_seq_from(inner, &[], pos, ctx);
            if *positive {
                if result.is_some() {
                    // Keep captures from inside lookahead (PHP behavior)
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    None
                }
            } else {
                if result.is_none() {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    *ctx.mark = saved_mark;
                    None
                }
            }
        }
        Node::Lookbehind { positive, inner } => {
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
                    let result = match_seq_from(inner, &[], start, ctx);
                    if result == Some(pos) {
                        true
                    } else {
                        *ctx.groups = saved;
                        *ctx.mark = saved_mark;
                        false
                    }
                });
            if found == *positive {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
        }
        Node::Atomic(inner) => {
            let end = match_seq_from(inner, &[], pos, ctx)?;
            match_rest(rest, end, ctx)
        }
        Node::Mark(name) => {
            let saved = ctx.mark.clone();
            *ctx.mark = Some(name.clone());
            let result = match_rest(rest, pos, ctx);
            if result.is_none() {
                *ctx.mark = saved;
            }
            result
        }
    }
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

    let (initial, states) = collect_match_states(inner, pos, ctx);
    for state in states {
        let end = state.end;
        state.install(ctx);
        if let Some(idx) = group_idx {
            let end_offset = ctx.metadata.byte_offsets.get(end);
            ctx.groups[idx] = Some(Match {
                start: start_offset,
                end: end_offset,
            });
        }
        if let Some(final_pos) = match_rest(rest, end, ctx) {
            return Some(final_pos);
        }
    }
    initial.install(ctx);
    None
}

/// One possible matcher continuation. Capture registers and the last MARK are
/// part of the state: two paths ending at the same subject position are not
/// interchangeable when PHP later publishes their captures.
#[derive(Clone)]
struct BacktrackState {
    end: usize,
    groups: Vec<Option<Match>>,
    mark: Option<String>,
}

impl BacktrackState {
    fn take(end: usize, ctx: &mut MatchCtx<'_>) -> Self {
        Self {
            end,
            groups: std::mem::take(ctx.groups),
            mark: ctx.mark.take(),
        }
    }

    fn install(self, ctx: &mut MatchCtx<'_>) {
        *ctx.groups = self.groups;
        *ctx.mark = self.mark;
    }
}

/// Collect every possible continuation for a node and return the caller's
/// initial state separately. Group and quantified alternatives consume these
/// immutable snapshots in PCRE backtracking order.
fn collect_match_states(
    node: &Node,
    pos: usize,
    ctx: &mut MatchCtx,
) -> (BacktrackState, Vec<BacktrackState>) {
    let initial = BacktrackState::take(pos, ctx);
    let states = collect_match_states_from(node, initial.clone(), ctx);
    (initial, states)
}

fn collect_match_states_from(
    node: &Node,
    state: BacktrackState,
    ctx: &mut MatchCtx,
) -> Vec<BacktrackState> {
    match node {
        Node::Sequence(nodes) => {
            let mut states = vec![state];
            for node in nodes {
                let mut next = Vec::new();
                for state in states {
                    next.extend(collect_match_states_from(node, state, ctx));
                }
                states = next;
                if states.is_empty() {
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
            let mut repetitions = Vec::new();
            fn collect_reps(
                inner: &Node,
                min: usize,
                limit: usize,
                current_reps: usize,
                state: BacktrackState,
                ctx: &mut MatchCtx,
                repetitions: &mut Vec<(usize, BacktrackState)>,
            ) {
                if current_reps >= min {
                    repetitions.push((current_reps, state.clone()));
                }
                if current_reps >= limit {
                    return;
                }
                let current_end = state.end;
                for next in collect_match_states_from(inner, state, ctx) {
                    if next.end == current_end {
                        continue;
                    }
                    collect_reps(inner, min, limit, current_reps + 1, next, ctx, repetitions);
                }
            }
            collect_reps(
                inner,
                *min,
                max.unwrap_or(usize::MAX),
                0,
                state,
                ctx,
                &mut repetitions,
            );
            if *greedy {
                repetitions.sort_by(|a, b| b.0.cmp(&a.0));
            } else {
                repetitions.sort_by(|a, b| a.0.cmp(&b.0));
            }
            if *possessive {
                repetitions.truncate(1);
            }
            repetitions.into_iter().map(|(_, state)| state).collect()
        }
        Node::Group {
            index,
            name: _,
            inner,
        } => {
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
            states
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

/// Match remaining nodes in the rest slice.
fn match_rest(rest: &[Node], pos: usize, ctx: &mut MatchCtx) -> Option<usize> {
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
            let matches = if ctx.flags.case_insensitive {
                ctx.chars[pos + i].to_lowercase().eq(cc.to_lowercase())
            } else {
                ctx.chars[pos + i] == cc
            };
            if !matches {
                return None;
            }
        }
        match_rest(rest, pos + cap_chars.len(), ctx)
    } else {
        None
    }
}

#[inline]
fn chars_equal(left: char, right: char, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.to_lowercase().eq(right.to_lowercase())
    } else {
        left == right
    }
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

    // With no continuation there is nothing to backtrack into. Consume
    // directly to the greedy maximum (or lazy minimum) instead of allocating,
    // cloning and sorting every intermediate state.
    if rest.is_empty() {
        let tracks_captures = ctx.groups.len() > 1;
        let initial_groups = tracks_captures.then(|| ctx.groups.clone());
        let mut current_pos = pos;
        let mut repetitions = 0usize;

        if !greedy && min == 0 {
            return Some(pos);
        }

        while repetitions < limit {
            let saved_groups = tracks_captures.then(|| ctx.groups.clone());
            match match_seq_from(inner, &[], current_pos, ctx) {
                Some(next_pos) if next_pos != current_pos => {
                    current_pos = next_pos;
                    repetitions += 1;
                    if !greedy && repetitions >= min {
                        return Some(current_pos);
                    }
                }
                _ => {
                    if let Some(saved_groups) = saved_groups {
                        *ctx.groups = saved_groups;
                    }
                    break;
                }
            }
        }

        if repetitions >= min {
            return Some(current_pos);
        }
        if let Some(initial_groups) = initial_groups {
            *ctx.groups = initial_groups;
        }
        return None;
    }

    // Collect all possible (reps, end_position, saved_groups) tuples when a
    // continuation may require the quantifier to give characters back.
    let tracks_captures = ctx.groups.len() > 1;
    let mut states: Vec<(usize, usize, Option<Vec<Option<Match>>>)> = Vec::new();

    fn collect_states(
        inner: &Node,
        min: usize,
        limit: usize,
        pos: usize,
        current_reps: usize,
        tracks_captures: bool,
        ctx: &mut MatchCtx,
        states: &mut Vec<(usize, usize, Option<Vec<Option<Match>>>)>,
    ) {
        if current_reps >= min {
            let groups = tracks_captures.then(|| ctx.groups.clone());
            states.push((current_reps, pos, groups));
        }
        if current_reps >= limit {
            return;
        }
        let saved = tracks_captures.then(|| ctx.groups.clone());
        // Try one more repetition
        if let Some(np) = match_seq_from(inner, &[], pos, ctx) {
            if np == pos {
                // Zero-width match — don't recurse to avoid infinite loop
                if let Some(saved) = saved {
                    *ctx.groups = saved;
                }
                return;
            }
            collect_states(
                inner,
                min,
                limit,
                np,
                current_reps + 1,
                tracks_captures,
                ctx,
                states,
            );
        }
        if let Some(saved) = saved {
            *ctx.groups = saved;
        }
    }

    collect_states(inner, min, limit, pos, 0, tracks_captures, ctx, &mut states);

    // collect_states records states in increasing repetition order, so the
    // greedy path can iterate backwards without sorting.
    if possessive {
        if let Some((_, end_pos, saved_groups)) = states.pop() {
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            return match_rest(rest, end_pos, ctx);
        }
    } else if greedy {
        for (_, end_pos, saved_groups) in states.into_iter().rev() {
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            if let Some(final_pos) = match_rest(rest, end_pos, ctx) {
                return Some(final_pos);
            }
        }
    } else {
        for (_, end_pos, saved_groups) in states {
            if let Some(saved_groups) = saved_groups {
                *ctx.groups = saved_groups;
            }
            if let Some(final_pos) = match_rest(rest, end_pos, ctx) {
                return Some(final_pos);
            }
        }
    }
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
        Shorthand::Property(class) => class.matches(c),
    }
}

fn match_class_item(item: &ClassItem, c: char, flags: RegexFlags) -> bool {
    match item {
        ClassItem::Literal(l) => {
            if flags.case_insensitive {
                c.to_lowercase().eq(l.to_lowercase())
            } else {
                c == *l
            }
        }
        ClassItem::Range(lo, hi) => {
            if flags.case_insensitive {
                let cl = c.to_ascii_lowercase();
                let ll = lo.to_ascii_lowercase();
                let hl = hi.to_ascii_lowercase();
                cl >= ll && cl <= hl
            } else {
                c >= *lo && c <= *hi
            }
        }
        ClassItem::Shorthand(sh) => match_shorthand(*sh, c, flags.unicode),
    }
}

// ── Parser ──────────────────────────────────────────────────────────────────

struct Parser {
    chars: Vec<char>,
    pos: usize,
    group_count: usize,
    named_groups: HashMap<String, usize>,
    defined_subpatterns: HashMap<String, Node>,
    /// Bodies of the capture groups parsed so far, by index, for inlining
    /// later subroutine calls to them.
    completed_groups: HashMap<usize, Node>,
    flags: RegexFlags,
}

/// Drop capture wrappers from a subroutine body: PCRE restores every capture
/// set inside a call once it returns, so an inlined copy must not publish
/// them.
fn strip_captures(node: Node) -> Node {
    match node {
        Node::Group {
            index: Some(_),
            inner,
            ..
        } => strip_captures(*inner),
        Node::Group { index, name, inner } => Node::Group {
            index,
            name,
            inner: Box::new(strip_captures(*inner)),
        },
        Node::Alternation(branches) => {
            Node::Alternation(branches.into_iter().map(strip_captures).collect())
        }
        Node::Sequence(items) => Node::Sequence(items.into_iter().map(strip_captures).collect()),
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            possessive,
        } => Node::Quantifier {
            inner: Box::new(strip_captures(*inner)),
            min,
            max,
            greedy,
            possessive,
        },
        Node::Lookahead { positive, inner } => Node::Lookahead {
            positive,
            inner: Box::new(strip_captures(*inner)),
        },
        Node::Lookbehind { positive, inner } => Node::Lookbehind {
            positive,
            inner: Box::new(strip_captures(*inner)),
        },
        Node::Atomic(inner) => Node::Atomic(Box::new(strip_captures(*inner))),
        other => other,
    }
}

impl Parser {
    fn new(pattern: &str, flags: RegexFlags) -> Self {
        Self {
            chars: pattern.chars().collect(),
            pos: 0,
            group_count: 0,
            named_groups: HashMap::new(),
            defined_subpatterns: HashMap::new(),
            completed_groups: HashMap::new(),
            flags,
        }
    }

    /// Inline a subroutine call to capture group `index`. Only a group that
    /// is already complete can be copied; recursion into an open group and
    /// forward calls stay engine non-claims.
    fn subroutine_call(&self, index: usize) -> Result<Node, String> {
        match self.completed_groups.get(&index) {
            Some(inner) => Ok(strip_captures(inner.clone())),
            None if index == 0 || index <= self.group_count => {
                Err("Unsupported PCRE recursive subroutine call".into())
            }
            None => Err("Unsupported PCRE forward subroutine call".into()),
        }
    }

    fn named_subroutine_call(&self, name: &str) -> Result<Node, String> {
        if let Some(definition) = self.defined_subpatterns.get(name) {
            return Ok(definition.clone());
        }
        match self.named_groups.get(name) {
            Some(&index) => self.subroutine_call(index),
            None => Err(format!("Unknown PCRE subpattern '{name}'")),
        }
    }

    /// Read a group name or number up to `close`, then resolve it as a
    /// subroutine call.
    fn parse_subroutine_reference(&mut self, close: char) -> Result<Node, String> {
        let mut reference = String::new();
        while let Some(c) = self.peek() {
            self.advance();
            if c == close {
                return self.resolve_subroutine_reference(&reference);
            }
            reference.push(c);
        }
        Err("Unterminated PCRE subroutine call".into())
    }

    fn resolve_subroutine_reference(&self, reference: &str) -> Result<Node, String> {
        if reference == "R" {
            return self.subroutine_call(0);
        }
        if let Some(relative) = reference.strip_prefix('-') {
            let back: usize = relative
                .parse()
                .map_err(|_| "Malformed relative PCRE subroutine call".to_string())?;
            return match (back > 0).then(|| self.group_count.checked_sub(back - 1)) {
                Some(Some(index)) if index > 0 => self.subroutine_call(index),
                _ => Err("reference to non-existent subpattern".into()),
            };
        }
        if let Some(forward) = reference.strip_prefix('+') {
            let ahead: usize = forward
                .parse()
                .map_err(|_| "Malformed relative PCRE subroutine call".to_string())?;
            return self.subroutine_call(self.group_count + ahead);
        }
        match reference.parse::<usize>() {
            Ok(index) => self.subroutine_call(index),
            Err(_) => self.named_subroutine_call(reference),
        }
    }

    /// Resolve the `\g{...}` / `\gn` backreference spellings.
    fn backreference_by_reference(&self, reference: &str) -> Result<Node, String> {
        if let Some(relative) = reference.strip_prefix('-') {
            let back: usize = relative
                .parse()
                .map_err(|_| "Malformed relative PCRE backreference".to_string())?;
            return match (back > 0).then(|| self.group_count.checked_sub(back - 1)) {
                Some(Some(index)) if index > 0 => Ok(Node::Backreference(index)),
                _ => Err("reference to non-existent subpattern".into()),
            };
        }
        match reference.parse::<usize>() {
            Ok(index) if index > 0 => Ok(Node::Backreference(index)),
            Ok(_) => Err("a numbered reference must not be zero".into()),
            Err(_) => Ok(Node::NamedBackreference(reference.to_string())),
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
                    while let Some(cc) = self.peek() {
                        self.advance();
                        if cc == '\n' {
                            break;
                        }
                    }
                    continue;
                }
            }
            let atom = self.parse_atom()?;
            let quantified = self.parse_quantifier(atom)?;
            nodes.push(quantified);
        }
        if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(Node::Sequence(nodes))
        }
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
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
            Some(c) if c != '*' && c != '+' && c != '?' && c != '{' => {
                self.advance();
                Ok(Node::Literal(c))
            }
            Some(c) => Err(format!(
                "Unexpected quantifier '{}' without preceding element",
                c
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
            Some('b') => Ok(Node::WordBoundary(true)),
            Some('B') => Ok(Node::WordBoundary(false)),
            Some('A') => Ok(Node::Anchor(Anchor::AbsoluteStart)),
            Some('n') => Ok(Node::Literal('\n')),
            Some('r') => Ok(Node::Literal('\r')),
            Some('t') => Ok(Node::Literal('\t')),
            Some('g') => {
                // \g<name>, \g<n> and \g'name' call a subroutine; \g{n},
                // \g{-n}, \g{name} and \gn are backreferences.
                match self.peek() {
                    Some('<') => {
                        self.advance();
                        self.parse_subroutine_reference('>')
                    }
                    Some('\'') => {
                        self.advance();
                        self.parse_subroutine_reference('\'')
                    }
                    Some('{') => {
                        self.advance();
                        let mut reference = String::new();
                        while let Some(c) = self.peek() {
                            self.advance();
                            if c == '}' {
                                return self.backreference_by_reference(&reference);
                            }
                            reference.push(c);
                        }
                        Err("Unterminated \\g{} backreference".into())
                    }
                    Some(c) if c.is_ascii_digit() || c == '-' => {
                        let mut reference = String::new();
                        reference.push(c);
                        self.advance();
                        while let Some(next) = self.peek() {
                            if !next.is_ascii_digit() {
                                break;
                            }
                            reference.push(next);
                            self.advance();
                        }
                        self.backreference_by_reference(&reference)
                    }
                    _ => Err("\\g is not followed by a braced, angle-bracketed, or quoted name/number or by a plain number".into()),
                }
            }
            Some('k') => {
                // \k<name> or \k'name' — named backreference
                let delim = self.peek();
                if delim == Some('<') || delim == Some('\'') {
                    self.advance();
                    let close = if delim == Some('<') { '>' } else { '\'' };
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        if c == close {
                            self.advance();
                            break;
                        }
                        self.advance();
                        name.push(c);
                    }
                    Ok(Node::NamedBackreference(name))
                } else {
                    Ok(Node::Literal('k'))
                }
            }
            Some(c) if c.is_ascii_digit() && c != '0' => {
                // Multi-digit backreference: \1, \10, \123, etc.
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
                Ok(Node::Backreference(n))
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
            Some(c) => Ok(Node::Literal(c)), // \/, \\, \., etc.
            None => Err("Unexpected end after \\".into()),
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
                        let digit = c.to_digit(16).ok_or("Malformed \\x{} escape sequence")?;
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
            .ok_or_else(|| "unknown property name after \\P or \\p".to_string())
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
            Some('n') => Ok(ClassItem::Literal('\n')),
            Some('r') => Ok(ClassItem::Literal('\r')),
            Some('t') => Ok(ClassItem::Literal('\t')),
            Some('x') => Ok(ClassItem::Literal(self.parse_hex_escape()?)),
            Some('p') => Ok(ClassItem::Shorthand(Shorthand::Property(
                self.parse_property_escape(false)?,
            ))),
            Some('P') => Ok(ClassItem::Shorthand(Shorthand::Property(
                self.parse_property_escape(true)?,
            ))),
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
            let item = if c == '\\' {
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
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(item);
            }
        }
        Err("Unterminated character class".into())
    }

    fn parse_group(&mut self) -> Result<Node, String> {
        self.advance(); // consume '('

        // PCRE MARK control verb. Symfony's compiled route matcher uses the
        // short `(*:id)` spelling to identify the successful route branch.
        if self.peek() == Some('*') {
            self.advance();
            let mut verb = String::new();
            while let Some(c) = self.peek() {
                if c == ':' || c == ')' {
                    break;
                }
                verb.push(c);
                self.advance();
            }
            if !verb.is_empty() && !verb.eq_ignore_ascii_case("MARK") {
                return Err(format!("Unsupported PCRE control verb (*{verb})"));
            }
            if self.advance() != Some(':') {
                return Err("Expected ':' in PCRE MARK".into());
            }
            let mut name = String::new();
            while let Some(c) = self.peek() {
                if c == ')' {
                    self.advance();
                    return Ok(Node::Mark(name));
                }
                name.push(c);
                self.advance();
            }
            return Err("Unterminated PCRE MARK".into());
        }

        // Check for special group types
        if self.peek() == Some('?') {
            self.advance(); // consume '?'
            if self.chars[self.pos..].starts_with(&['(', 'D', 'E', 'F', 'I', 'N', 'E', ')']) {
                self.pos += 8;
                while self.peek() != Some(')') {
                    if self.peek().is_none() {
                        return Err("Unterminated PCRE DEFINE block".into());
                    }
                    let definition = self.parse_group()?;
                    let Node::Group {
                        name: Some(name), ..
                    } = &definition
                    else {
                        return Err("PCRE DEFINE entries must be named groups".into());
                    };
                    self.defined_subpatterns.insert(name.clone(), definition);
                }
                self.advance();
                return Ok(Node::Sequence(Vec::new()));
            }

            // PCRE scoped option groups such as `(?-i:...)` are valid syntax,
            // but changing matcher flags for only one subtree is not yet an
            // engine capability. Parse the body far enough to distinguish a
            // valid unsupported construct from a malformed group. Public
            // preg_* callers can then preserve the historical silent engine
            // limitation without publishing PHP's compilation warning for a
            // pattern that PCRE itself accepts.
            let option_start = self.pos;
            let mut saw_option = false;
            while let Some(option) = self.peek() {
                if matches!(option, 'i' | 'm' | 'n' | 'r' | 's' | 'x' | 'J' | 'U' | 'X') {
                    saw_option = true;
                    self.advance();
                    continue;
                }
                if option == '-' {
                    self.advance();
                    continue;
                }
                break;
            }
            if saw_option && self.peek() == Some(':') {
                self.advance();
                self.parse_alternation()?;
                if self.advance() != Some(')') {
                    return Err("Unterminated scoped PCRE option group".into());
                }
                return Err("Unsupported PCRE scoped option group".into());
            }
            self.pos = option_start;

            match self.peek() {
                Some('&') => {
                    // Named subroutine call (?&name). DEFINE blocks and
                    // completed groups publish immutable AST fragments, so
                    // expansion here keeps the matcher free of an extra
                    // runtime dispatch variant.
                    self.advance();
                    self.parse_subroutine_reference(')')
                }
                Some('R' | '+' | '-') => self.parse_subroutine_reference(')'),
                Some(c) if c.is_ascii_digit() => self.parse_subroutine_reference(')'),
                Some('|') => {
                    // Branch-reset group (?|...): capture numbering restarts
                    // at the same base for every alternative and continues
                    // after the largest branch number.
                    self.advance();
                    let base_group_count = self.group_count;
                    let mut max_group_count = base_group_count;
                    let mut branches = Vec::new();
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
                    self.group_count = max_group_count;
                    if branches.len() == 1 {
                        Ok(branches.pop().unwrap())
                    } else {
                        Ok(Node::Alternation(branches))
                    }
                }
                Some(':') => {
                    // Non-capturing group (?:...)
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated non-capturing group".into());
                    }
                    Ok(inner) // no wrapping Group node
                }
                Some('>') => {
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated atomic group".into());
                    }
                    Ok(Node::Atomic(Box::new(inner)))
                }
                Some('=') => {
                    // Positive lookahead (?=...)
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated lookahead".into());
                    }
                    Ok(Node::Lookahead {
                        positive: true,
                        inner: Box::new(inner),
                    })
                }
                Some('!') => {
                    // Negative lookahead (?!...)
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated lookahead".into());
                    }
                    Ok(Node::Lookahead {
                        positive: false,
                        inner: Box::new(inner),
                    })
                }
                Some('<') => {
                    self.advance(); // consume '<'
                    match self.peek() {
                        Some('=') => {
                            // Positive lookbehind (?<=...)
                            self.advance();
                            let inner = self.parse_alternation()?;
                            if self.advance() != Some(')') {
                                return Err("Unterminated lookbehind".into());
                            }
                            Ok(Node::Lookbehind {
                                positive: true,
                                inner: Box::new(inner),
                            })
                        }
                        Some('!') => {
                            // Negative lookbehind (?<!...)
                            self.advance();
                            let inner = self.parse_alternation()?;
                            if self.advance() != Some(')') {
                                return Err("Unterminated lookbehind".into());
                            }
                            Ok(Node::Lookbehind {
                                positive: false,
                                inner: Box::new(inner),
                            })
                        }
                        _ => {
                            // Named group (?<name>...)
                            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                                return Err(format!(
                                    "subpattern name must start with a non-digit at offset {}",
                                    self.pos
                                ));
                            }
                            let mut name = String::new();
                            while let Some(c) = self.peek() {
                                if c == '>' {
                                    self.advance();
                                    break;
                                }
                                self.advance();
                                name.push(c);
                            }
                            self.group_count += 1;
                            let idx = self.group_count;
                            self.named_groups.insert(name.clone(), idx);
                            let inner = self.parse_alternation()?;
                            if self.advance() != Some(')') {
                                return Err("Unterminated named group".into());
                            }
                            self.completed_groups.insert(idx, inner.clone());
                            Ok(Node::Group {
                                index: Some(idx),
                                name: Some(name),
                                inner: Box::new(inner),
                            })
                        }
                    }
                }
                Some('P') => {
                    self.advance(); // consume 'P'
                    if self.peek() == Some('=') {
                        // (?P=name) — named backreference
                        self.advance(); // consume '='
                        let mut name = String::new();
                        while let Some(c) = self.peek() {
                            if c == ')' {
                                break;
                            }
                            self.advance();
                            name.push(c);
                        }
                        if self.advance() != Some(')') {
                            return Err("Unterminated named backreference (?P=...)".into());
                        }
                        return Ok(Node::NamedBackreference(name));
                    }
                    if self.peek() == Some('>') {
                        // (?P>name) — named subroutine call
                        self.advance();
                        return self.parse_subroutine_reference(')');
                    }
                    // (?P<name>...)
                    if self.advance() != Some('<') {
                        return Err("Expected '<' or '=' after (?P".into());
                    }
                    if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        return Err(format!(
                            "subpattern name must start with a non-digit at offset {}",
                            self.pos
                        ));
                    }
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        if c == '>' {
                            self.advance();
                            break;
                        }
                        self.advance();
                        name.push(c);
                    }
                    self.group_count += 1;
                    let idx = self.group_count;
                    self.named_groups.insert(name.clone(), idx);
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated named group".into());
                    }
                    self.completed_groups.insert(idx, inner.clone());
                    Ok(Node::Group {
                        index: Some(idx),
                        name: Some(name),
                        inner: Box::new(inner),
                    })
                }
                _ => Err(format!(
                    "Unknown group modifier '?{}'",
                    self.peek().unwrap_or(' ')
                )),
            }
        } else {
            // Capturing group
            self.group_count += 1;
            let idx = self.group_count;
            let inner = self.parse_alternation()?;
            if self.advance() != Some(')') {
                return Err("Unterminated capturing group".into());
            }
            self.completed_groups.insert(idx, inner.clone());
            Ok(Node::Group {
                index: Some(idx),
                name: None,
                inner: Box::new(inner),
            })
        }
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        // Anchors and assertions can't be quantified
        match &atom {
            Node::Anchor(_)
            | Node::WordBoundary(_)
            | Node::Lookahead { .. }
            | Node::Lookbehind { .. }
            | Node::Mark(_) => {
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
                let saved = self.pos;
                self.advance();
                match self.parse_counted_quantifier() {
                    Ok((min, max)) => {
                        let (greedy, possessive) = self.quantifier_mode(default_greedy);
                        Ok(Node::Quantifier {
                            inner: Box::new(atom),
                            min,
                            max,
                            greedy,
                            possessive,
                        })
                    }
                    Err(_) => {
                        // Not a valid quantifier, treat '{' as literal — restore position
                        self.pos = saved;
                        Ok(atom)
                    }
                }
            }
            _ => Ok(atom),
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
                    Ok((min, None)) // {n,}
                } else {
                    let max: usize = max_str.parse().map_err(|_| "Invalid quantifier max")?;
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
    let mut unsupported_modifier = None;
    for ch in flags_str.chars() {
        match ch {
            'i' => flags.case_insensitive = true,
            'm' => flags.multiline = true,
            's' => flags.dotall = true,
            'x' => flags.extended = true,
            'U' => flags.ungreedy = true,
            'u' => flags.unicode = true,
            // PCRE's study modifier is an optimization hint and cannot alter
            // observable match or replacement results.
            'S' => {}
            'D' => flags.dollar_end_only = true,
            'A' => flags.anchored = true,
            // These are valid PHP/PCRE modifiers, but their engine semantics
            // are not implemented yet. Finish scanning first so a genuinely
            // unknown modifier later in the same suffix still wins and emits
            // PHP's compile warning.
            'J' | 'X' | 'n' | 'r' => {
                unsupported_modifier.get_or_insert(ch);
            }
            ' ' | '\n' | '\r' => {}
            '\0' => return Err("NUL byte is not a valid modifier".into()),
            _ => return Err(format!("Unknown modifier '{}'", ch)),
        }
    }
    if let Some(modifier) = unsupported_modifier {
        return Err(format!("Unsupported PCRE modifier '{modifier}'"));
    }
    Ok((pattern.to_string(), flags))
}

#[cfg(test)]
#[path = "regex/tests.rs"]
mod tests;
