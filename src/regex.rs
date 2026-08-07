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
//! Flags: `i` (case-insensitive), `m` (multiline), `s` (dotall), `x` (extended/comments), `U` (ungreedy)

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

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
    WordBoundary(bool), // true = \b, false = \B
}

#[derive(Debug, Clone)]
enum Anchor {
    Start, // ^
    End,   // $
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
}

// ── Flags ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct RegexFlags {
    pub case_insensitive: bool,
    pub multiline: bool,
    pub dotall: bool,
    pub extended: bool,
    pub ungreedy: bool,
}

impl Default for RegexFlags {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            multiline: false,
            dotall: false,
            extended: false,
            ungreedy: false,
        }
    }
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
    capacity: usize,
    entries: HashMap<String, Rc<Regex>>,
    insertion_order: VecDeque<String>,
}

impl RegexCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::with_capacity(capacity),
            insertion_order: VecDeque::with_capacity(capacity),
        }
    }

    /// Return a shared compiled regex, compiling and caching it on a miss.
    /// Invalid patterns are not cached so callers preserve their existing
    /// false/null error behavior and no failed entry consumes cache capacity.
    pub fn get_or_compile(&mut self, php_pattern: &str) -> Result<Rc<Regex>, String> {
        if let Some(regex) = self.entries.get(php_pattern) {
            return Ok(Rc::clone(regex));
        }

        let (pattern, flags) = parse_php_regex(php_pattern)?;
        let regex = Rc::new(Regex::new(&pattern, flags)?);

        if self.capacity == 0 {
            return Ok(regex);
        }

        if self.entries.len() == self.capacity {
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
}

impl Regex {
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
    #[inline]
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

        for start in 0..=chars.len() {
            if let Some(literal) = self.start_literal {
                if start == chars.len()
                    || !chars_equal(chars[start], literal, self.flags.case_insensitive)
                {
                    continue;
                }
            }
            groups.fill(None);
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
            };
            if match_seq_from(&self.ast, &[], start, &mut ctx).is_some() {
                return true;
            }
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
        // Try matching at every position
        for start in 0..=chars.len() {
            if let Some(literal) = self.start_literal {
                if start == chars.len()
                    || !chars_equal(chars[start], literal, self.flags.case_insensitive)
                {
                    continue;
                }
            }
            groups.fill(None);
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
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
                });
            }
        }
        None
    }

    /// Replace all occurrences.  Replacement can use `$1`, `$10`, `${2}`, `\\1` backrefs.
    pub fn replace_all(&self, subject: &str, replacement: &str) -> String {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut result = String::new();
        let mut pos = 0;

        while pos <= chars.len() {
            let mut groups = vec![None; self.num_groups + 1];
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
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

                if end == pos {
                    // Zero-length match — advance by one to avoid infinite loop
                    if pos < chars.len() {
                        result.push(chars[pos]);
                    }
                    pos += 1;
                } else {
                    pos = end;
                }
            } else {
                if pos < chars.len() {
                    result.push(chars[pos]);
                }
                pos += 1;
            }
        }
        result
    }

    /// Find all non-overlapping matches, returning a vector of Captures.
    pub fn captures_iter(&self, subject: &str) -> Vec<Captures> {
        let (chars, byte_offsets) = subject_chars(subject);
        let metadata = MatchMetadata {
            input: subject,
            byte_offsets: &byte_offsets,
        };
        let mut results = Vec::new();
        let mut pos = 0;

        while pos <= chars.len() {
            let mut groups = vec![None; self.num_groups + 1];
            let end = {
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
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
                results.push(Captures {
                    groups,
                    named_groups: self.named_groups.clone(),
                });
                if end == pos {
                    pos += 1; // avoid infinite loop on zero-length match
                } else {
                    pos = end;
                }
            } else {
                pos += 1;
            }
        }
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
                let mut groups = vec![None; self.num_groups + 1];
                let mut ctx = MatchCtx {
                    chars: &chars,
                    metadata: &metadata,
                    flags: self.flags,
                    groups: &mut groups,
                    named_groups: &self.named_groups,
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
            let mut ctx = MatchCtx {
                chars: &chars,
                metadata: &metadata,
                flags: self.flags,
                groups: &mut groups,
                named_groups: &self.named_groups,
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
                    | Node::Lookbehind { .. } => continue,
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
        | Node::Lookbehind { inner, .. } => contains_backreference(inner),
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
        out.push(bytes[i] as char);
        i += 1;
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
}

// ── Core matching (backtracking with continuation) ──────────────────────────
//
// The key insight: to support proper backtracking, every node match must know
// about the "rest" of the sequence that follows it.  When a quantifier tries
// N repetitions and the rest fails, it can backtrack to N-1, etc.
//
// `match_seq_from(node, rest, pos, ctx)` tries to match `node` followed by
// all nodes in `rest` starting at `pos`.  Returns Some(final_pos) on success.

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
        Node::Anchor(Anchor::End) => {
            let ok = if ctx.flags.multiline {
                pos == ctx.chars.len() || ctx.chars[pos] == '\n'
            } else {
                pos == ctx.chars.len()
            };
            if ok { match_rest(rest, pos, ctx) } else { None }
        }
        Node::WordBoundary(positive) => {
            let at_boundary = is_word_boundary(ctx.chars, pos);
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
                .any(|item| match_class_item(item, c, ctx.flags.case_insensitive));
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
            if match_shorthand(*sh, ctx.chars[pos]) {
                match_rest(rest, pos + 1, ctx)
            } else {
                None
            }
        }
        Node::Alternation(branches) => {
            for branch in branches {
                let saved_groups = ctx.groups.clone();
                if let Some(end) = match_seq_from(branch, rest, pos, ctx) {
                    return Some(end);
                }
                *ctx.groups = saved_groups;
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
            let saved_group = tracked_index.map(|idx| ctx.groups[idx].clone());
            let result =
                match_seq_from_with_group(inner, rest, pos, ctx, tracked_index, start_offset);
            if result.is_none() {
                // Restore group on failure
                if let Some(idx) = tracked_index {
                    ctx.groups[idx] = saved_group.unwrap();
                }
            }
            result
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
        } => match_quantifier(inner, *min, *max, *greedy, rest, pos, ctx),
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
            // Use empty rest — lookahead doesn't consume input, just checks
            let result = match_seq_from(inner, &[], pos, ctx);
            if *positive {
                if result.is_some() {
                    // Keep captures from inside lookahead (PHP behavior)
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    None
                }
            } else {
                if result.is_none() {
                    *ctx.groups = saved;
                    match_rest(rest, pos, ctx)
                } else {
                    *ctx.groups = saved;
                    None
                }
            }
        }
        Node::Lookbehind { positive, inner } => {
            // Try matching inner ending at `pos`.
            let found = (0..=pos).rev().any(|start| {
                let saved = ctx.groups.clone();
                let result = match_seq_from(inner, &[], start, ctx);
                if result == Some(pos) {
                    true
                } else {
                    *ctx.groups = saved;
                    false
                }
            });
            if found == *positive {
                match_rest(rest, pos, ctx)
            } else {
                None
            }
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
    // We need to match inner, then set group, then match rest.
    // Create a temporary "after group" rest that we handle specially.
    // Actually, the simplest approach: match inner with empty rest, get end pos,
    // set group, then match rest. But this doesn't allow backtracking into the group.
    //
    // For proper backtracking: we match inner with the rest passed through.
    // The group capture is set as soon as inner finishes.
    //
    // Use a wrapper approach: match inner+rest, where inner's continuation sets the group.

    // Simple approach that works for most cases:
    // Match inner against empty rest to find all possible end positions,
    // then for each, try the rest.
    // Actually, for proper backtracking we need to integrate rest into inner's matching.

    // The simplest correct approach: wrap into a sequence [inner, rest_nodes...]
    // and match, recording the group after inner.

    // Let's use a different strategy: match inner with rest, and we set the group
    // after inner matches but before rest starts. We do this by trying inner with
    // empty rest first to get a position, set group, then try rest.
    // If rest fails, we need to tell inner to try a different match.

    // For correctness with backtracking, we need to try all possible inner matches.
    // We implement this by using match_seq_from with the rest, but wrapping inner
    // such that when it succeeds, we record the group.

    // Practical approach: collect all possible end positions for inner, then try rest.
    let ends = collect_match_positions(inner, pos, ctx);
    for end_pos in ends {
        if let Some(idx) = group_idx {
            let end_offset = ctx.metadata.byte_offsets.get(end_pos);
            ctx.groups[idx] = Some(Match {
                start: start_offset,
                end: end_offset,
            });
        }
        if let Some(final_pos) = match_rest(rest, end_pos, ctx) {
            return Some(final_pos);
        }
    }
    None
}

/// Collect all possible end positions for a node match (for backtracking in groups).
fn collect_match_positions(node: &Node, pos: usize, ctx: &mut MatchCtx) -> Vec<usize> {
    let mut positions = Vec::new();
    collect_match_positions_inner(node, pos, ctx, &mut positions);
    positions
}

fn collect_match_positions_inner(
    node: &Node,
    pos: usize,
    ctx: &mut MatchCtx,
    out: &mut Vec<usize>,
) {
    match node {
        Node::Sequence(nodes) => {
            if nodes.is_empty() {
                out.push(pos);
                return;
            }
            // For each way the first node can match, try the rest of the sequence
            let first_positions = collect_match_positions(&nodes[0], pos, ctx);
            for fp in first_positions {
                if nodes.len() == 1 {
                    out.push(fp);
                } else {
                    let rest_seq = Node::Sequence(nodes[1..].to_vec());
                    collect_match_positions_inner(&rest_seq, fp, ctx, out);
                }
            }
        }
        Node::Alternation(branches) => {
            for branch in branches {
                collect_match_positions_inner(branch, pos, ctx, out);
            }
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
        } => {
            let mut reps_positions: Vec<(usize, usize)> = Vec::new(); // (reps, pos)
            // Collect all possible repetition counts
            fn collect_reps(
                inner: &Node,
                min: usize,
                max: Option<usize>,
                pos: usize,
                current_reps: usize,
                ctx: &mut MatchCtx,
                reps_positions: &mut Vec<(usize, usize)>,
            ) {
                if current_reps >= min {
                    reps_positions.push((current_reps, pos));
                }
                let limit = max.unwrap_or(usize::MAX);
                if current_reps >= limit {
                    return;
                }
                let next_positions = collect_match_positions(inner, pos, ctx);
                for np in next_positions {
                    if np == pos {
                        continue;
                    } // avoid infinite loop on zero-width
                    collect_reps(inner, min, max, np, current_reps + 1, ctx, reps_positions);
                }
            }
            collect_reps(inner, *min, *max, pos, 0, ctx, &mut reps_positions);
            // Sort by greedy preference
            if *greedy {
                reps_positions.sort_by(|a, b| b.0.cmp(&a.0)); // most reps first
            } else {
                reps_positions.sort_by(|a, b| a.0.cmp(&b.0)); // fewest reps first
            }
            for (_, p) in reps_positions {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        Node::Group {
            index,
            name: _,
            inner,
        } => {
            let tracked_index = index.filter(|idx| *idx < ctx.groups.len());
            let start_offset = tracked_index.map_or(0, |_| ctx.metadata.byte_offsets.get(pos));
            let inner_positions = collect_match_positions(inner, pos, ctx);
            for end_pos in inner_positions {
                if let Some(idx) = tracked_index {
                    let end_offset = ctx.metadata.byte_offsets.get(end_pos);
                    ctx.groups[idx] = Some(Match {
                        start: start_offset,
                        end: end_offset,
                    });
                }
                out.push(end_pos);
            }
        }
        // For simple nodes, delegate to match_seq_from with empty rest
        _ => {
            let saved = ctx.groups.clone();
            if let Some(end) = match_seq_from(node, &[], pos, ctx) {
                out.push(end);
            } else {
                *ctx.groups = saved;
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
    if greedy {
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

fn is_word_boundary(chars: &[char], pos: usize) -> bool {
    let before = if pos > 0 {
        is_word_char(chars[pos - 1])
    } else {
        false
    };
    let after = if pos < chars.len() {
        is_word_char(chars[pos])
    } else {
        false
    };
    before != after
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn match_shorthand(sh: Shorthand, c: char) -> bool {
    match sh {
        Shorthand::Digit => c.is_ascii_digit(),
        Shorthand::NonDigit => !c.is_ascii_digit(),
        Shorthand::Word => is_word_char(c),
        Shorthand::NonWord => !is_word_char(c),
        Shorthand::Space => c.is_ascii_whitespace(),
        Shorthand::NonSpace => !c.is_ascii_whitespace(),
    }
}

fn match_class_item(item: &ClassItem, c: char, case_insensitive: bool) -> bool {
    match item {
        ClassItem::Literal(l) => {
            if case_insensitive {
                c.to_lowercase().eq(l.to_lowercase())
            } else {
                c == *l
            }
        }
        ClassItem::Range(lo, hi) => {
            if case_insensitive {
                let cl = c.to_ascii_lowercase();
                let ll = lo.to_ascii_lowercase();
                let hl = hi.to_ascii_lowercase();
                cl >= ll && cl <= hl
            } else {
                c >= *lo && c <= *hi
            }
        }
        ClassItem::Shorthand(sh) => match_shorthand(*sh, c),
    }
}

// ── Parser ──────────────────────────────────────────────────────────────────

struct Parser {
    chars: Vec<char>,
    pos: usize,
    group_count: usize,
    named_groups: HashMap<String, usize>,
    flags: RegexFlags,
}

impl Parser {
    fn new(pattern: &str, flags: RegexFlags) -> Self {
        Self {
            chars: pattern.chars().collect(),
            pos: 0,
            group_count: 0,
            named_groups: HashMap::new(),
            flags,
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
            Some('n') => Ok(Node::Literal('\n')),
            Some('r') => Ok(Node::Literal('\r')),
            Some('t') => Ok(Node::Literal('\t')),
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
            Some(c) => Ok(Node::Literal(c)), // \/, \\, \., etc.
            None => Err("Unexpected end after \\".into()),
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
            if c == '\\' {
                self.advance();
                match self.advance() {
                    Some('d') => items.push(ClassItem::Shorthand(Shorthand::Digit)),
                    Some('D') => items.push(ClassItem::Shorthand(Shorthand::NonDigit)),
                    Some('w') => items.push(ClassItem::Shorthand(Shorthand::Word)),
                    Some('W') => items.push(ClassItem::Shorthand(Shorthand::NonWord)),
                    Some('s') => items.push(ClassItem::Shorthand(Shorthand::Space)),
                    Some('S') => items.push(ClassItem::Shorthand(Shorthand::NonSpace)),
                    Some('n') => items.push(ClassItem::Literal('\n')),
                    Some('r') => items.push(ClassItem::Literal('\r')),
                    Some('t') => items.push(ClassItem::Literal('\t')),
                    Some(ec) => items.push(ClassItem::Literal(ec)),
                    None => return Err("Unexpected end in character class escape".into()),
                }
            } else {
                self.advance();
                // Check for range: a-z
                if self.peek() == Some('-') && self.chars.get(self.pos + 1).copied() != Some(']') {
                    self.advance(); // consume '-'
                    let hi = self
                        .advance()
                        .ok_or("Unexpected end in character class range")?;
                    items.push(ClassItem::Range(c, hi));
                } else {
                    items.push(ClassItem::Literal(c));
                }
            }
        }
        Err("Unterminated character class".into())
    }

    fn parse_group(&mut self) -> Result<Node, String> {
        self.advance(); // consume '('

        // Check for special group types
        if self.peek() == Some('?') {
            self.advance(); // consume '?'
            match self.peek() {
                Some(':') => {
                    // Non-capturing group (?:...)
                    self.advance();
                    let inner = self.parse_alternation()?;
                    if self.advance() != Some(')') {
                        return Err("Unterminated non-capturing group".into());
                    }
                    Ok(inner) // no wrapping Group node
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
                    // (?P<name>...)
                    if self.advance() != Some('<') {
                        return Err("Expected '<' or '=' after (?P".into());
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
            | Node::Lookbehind { .. } => {
                return Ok(atom);
            }
            _ => {}
        }

        let default_greedy = !self.flags.ungreedy;

        match self.peek() {
            Some('*') => {
                self.advance();
                let greedy = self.check_lazy(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 0,
                    max: None,
                    greedy,
                })
            }
            Some('+') => {
                self.advance();
                let greedy = self.check_lazy(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 1,
                    max: None,
                    greedy,
                })
            }
            Some('?') => {
                self.advance();
                let greedy = self.check_lazy(default_greedy);
                Ok(Node::Quantifier {
                    inner: Box::new(atom),
                    min: 0,
                    max: Some(1),
                    greedy,
                })
            }
            Some('{') => {
                let saved = self.pos;
                self.advance();
                match self.parse_counted_quantifier() {
                    Ok((min, max)) => {
                        let greedy = self.check_lazy(default_greedy);
                        Ok(Node::Quantifier {
                            inner: Box::new(atom),
                            min,
                            max,
                            greedy,
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

    fn check_lazy(&mut self, default_greedy: bool) -> bool {
        if self.peek() == Some('?') {
            self.advance();
            !default_greedy // flip
        } else {
            default_greedy
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
    if bytes.is_empty() {
        return Err("Empty regular expression".into());
    }
    let open = bytes[0];
    let close = match open {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        _ => open, // symmetric delimiter like / ~ # !
    };

    let is_paired = matches!(open, b'{' | b'(' | b'[' | b'<');
    let close_pos = if is_paired {
        // Paired delimiters: first matching close after open
        match bytes[1..].iter().position(|&b| b == close) {
            Some(pos) => pos + 1,
            None => return Err(format!("No ending delimiter '{}' found", close as char)),
        }
    } else {
        // Symmetric delimiters: last occurrence (standard PHP behavior)
        match bytes[1..].iter().rposition(|&b| b == close) {
            Some(pos) => pos + 1,
            None => return Err(format!("No ending delimiter '{}' found", close as char)),
        }
    };
    let pattern = &input[1..close_pos];
    let flags_str = &input[close_pos + 1..];

    let mut flags = RegexFlags::default();
    for ch in flags_str.chars() {
        match ch {
            'i' => flags.case_insensitive = true,
            'm' => flags.multiline = true,
            's' => flags.dotall = true,
            'x' => flags.extended = true,
            'U' => flags.ungreedy = true,
            _ => return Err(format!("Unknown modifier '{}'", ch)),
        }
    }
    Ok((pattern.to_string(), flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        let re = Regex::new("abc", RegexFlags::default()).unwrap();
        let caps = re.captures("xabcy").unwrap();
        assert_eq!(caps.get(0).unwrap().as_str("xabcy"), "abc");
    }

    #[test]
    fn test_dot() {
        let re = Regex::new("a.c", RegexFlags::default()).unwrap();
        assert!(re.captures("axc").is_some());
        assert!(re.captures("ac").is_none());
    }

    #[test]
    fn test_char_class() {
        let re = Regex::new("[aeiou]", RegexFlags::default()).unwrap();
        assert!(re.captures("hello").is_some());
        assert!(re.captures("rhythm").is_none());
    }

    #[test]
    fn test_quantifier_star() {
        let re = Regex::new("ab*c", RegexFlags::default()).unwrap();
        assert!(re.captures("ac").is_some());
        assert!(re.captures("abc").is_some());
        assert!(re.captures("abbbbc").is_some());
    }

    #[test]
    fn test_terminal_quantifier_greedy_lazy_and_minimum() {
        let greedy = Regex::new("a+", RegexFlags::default()).unwrap();
        let lazy = Regex::new("a+?", RegexFlags::default()).unwrap();
        let minimum = Regex::new("a{2}", RegexFlags::default()).unwrap();

        assert_eq!(
            greedy
                .captures("aaa")
                .unwrap()
                .get(0)
                .unwrap()
                .as_str("aaa"),
            "aaa"
        );
        assert_eq!(
            lazy.captures("aaa").unwrap().get(0).unwrap().as_str("aaa"),
            "a"
        );
        assert!(minimum.captures("a").is_none());
        assert!(minimum.captures("aa").is_some());
    }

    #[test]
    fn test_capture_group() {
        let re = Regex::new("(\\d+)-(\\d+)", RegexFlags::default()).unwrap();
        let caps = re.captures("foo 123-456 bar").unwrap();
        assert_eq!(caps.get(0).unwrap().as_str("foo 123-456 bar"), "123-456");
        assert_eq!(caps.get(1).unwrap().as_str("foo 123-456 bar"), "123");
        assert_eq!(caps.get(2).unwrap().as_str("foo 123-456 bar"), "456");
    }

    #[test]
    fn test_is_match_preserves_groups_needed_by_backreferences() {
        let re = Regex::new("(a)\\1", RegexFlags::default()).unwrap();

        assert!(re.uses_backreferences);
        assert!(re.is_match("aa"));
        assert!(!re.is_match("ab"));
    }

    #[test]
    fn test_is_match_does_not_track_unused_capture_contents() {
        let re = Regex::new("(needle)", RegexFlags::default()).unwrap();

        assert!(!re.uses_backreferences);
        assert!(re.is_match("haystack needle"));
        assert!(!re.is_match("haystack"));
    }

    #[test]
    fn test_alternation() {
        let re = Regex::new("cat|dog", RegexFlags::default()).unwrap();
        assert!(re.captures("I have a cat").is_some());
        assert!(re.captures("I have a dog").is_some());
        assert!(re.captures("I have a fish").is_none());
    }

    #[test]
    fn test_anchors() {
        let re = Regex::new("^hello$", RegexFlags::default()).unwrap();
        assert!(re.captures("hello").is_some());
        assert!(re.captures("hello world").is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let flags = RegexFlags {
            case_insensitive: true,
            ..Default::default()
        };
        let re = Regex::new("hello", flags).unwrap();
        assert!(re.captures("HELLO").is_some());
    }

    #[test]
    fn test_required_start_literal_skips_zero_width_prefixes() {
        let re = Regex::new("^hello", RegexFlags::default()).unwrap();
        assert_eq!(re.start_literal, Some('h'));
        assert!(re.captures("hello").is_some());
        assert!(re.captures("xhello").is_none());
    }

    #[test]
    fn test_required_start_literal_handles_groups_and_alternation() {
        let common = Regex::new("(hello|hi)", RegexFlags::default()).unwrap();
        let different = Regex::new("hello|world", RegexFlags::default()).unwrap();
        let optional = Regex::new("a?hello", RegexFlags::default()).unwrap();

        assert_eq!(common.start_literal, Some('h'));
        assert_eq!(different.start_literal, None);
        assert_eq!(optional.start_literal, None);
        assert!(common.captures("say hi").is_some());
        assert!(different.captures("world").is_some());
        assert!(optional.captures("hello").is_some());
    }

    #[test]
    fn test_word_boundary() {
        let re = Regex::new("\\bword\\b", RegexFlags::default()).unwrap();
        assert!(re.captures("a word here").is_some());
        assert!(re.captures("password").is_none());
    }

    #[test]
    fn test_replace_all() {
        let re = Regex::new("\\d+", RegexFlags::default()).unwrap();
        assert_eq!(re.replace_all("a1b2c3", "X"), "aXbXcX");
    }

    #[test]
    fn test_subject_chars_maps_utf8_boundaries() {
        let (chars, offsets) = subject_chars("až🙂");

        assert_eq!(chars, vec!['a', 'ž', '🙂']);
        assert_eq!(offsets.get(0), 0);
        assert_eq!(offsets.get(1), 1);
        assert_eq!(offsets.get(2), 3);
        assert_eq!(offsets.get(3), 7);
    }

    #[test]
    fn test_subject_chars_uses_identity_offsets_for_ascii() {
        let (chars, offsets) = subject_chars("ascii");

        assert_eq!(chars, vec!['a', 's', 'c', 'i', 'i']);
        assert!(matches!(offsets, ByteOffsets::Identity));
        assert_eq!(offsets.get(chars.len()), chars.len());
    }

    #[test]
    fn test_captures_iter_preserves_utf8_and_named_capture_offsets() {
        let subject = "🙂 ž1 x č2";
        let re = Regex::new("(?P<letter>ž|č)(?P<digit>\\d)", RegexFlags::default()).unwrap();
        let captures = re.captures_iter(subject);

        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].get(0).unwrap().as_str(subject), "ž1");
        assert_eq!(
            captures[0].get_named("letter").unwrap().as_str(subject),
            "ž"
        );
        assert_eq!(captures[1].get(0).unwrap().as_str(subject), "č2");
        assert_eq!(captures[1].get_named("digit").unwrap().as_str(subject), "2");
    }

    #[test]
    fn test_captures_iter_advances_zero_width_matches_on_utf8_subject() {
        let subject = "aéa";
        let re = Regex::new("(?=a)", RegexFlags::default()).unwrap();
        let captures = re.captures_iter(subject);

        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].get(0).unwrap().start, 0);
        assert_eq!(captures[1].get(0).unwrap().start, 3);
        assert_eq!(captures[0].get(0).unwrap().as_str(subject), "");
        assert_eq!(captures[1].get(0).unwrap().as_str(subject), "");
    }

    #[test]
    fn test_replace_backreference() {
        let re = Regex::new("(\\w+)@(\\w+)", RegexFlags::default()).unwrap();
        assert_eq!(re.replace_all("user@host", "$2/$1"), "host/user");
    }

    #[test]
    fn test_lookahead() {
        let re = Regex::new("foo(?=bar)", RegexFlags::default()).unwrap();
        assert!(re.captures("foobar").is_some());
        assert!(re.captures("foobaz").is_none());
    }

    #[test]
    fn test_php_delimiter() {
        let (pattern, flags) = parse_php_regex("/hello/i").unwrap();
        assert_eq!(pattern, "hello");
        assert!(flags.case_insensitive);
    }

    // ── Compiled regex cache ──────────────────────────────────────────────

    #[test]
    fn test_regex_cache_reuses_compiled_pattern() {
        let mut cache = RegexCache::new(2);
        let first = cache.get_or_compile("/hello/").unwrap();
        let second = cache.get_or_compile("/hello/").unwrap();

        assert!(std::rc::Rc::ptr_eq(&first, &second));
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn test_regex_cache_distinguishes_modifiers() {
        let mut cache = RegexCache::new(2);
        let case_sensitive = cache.get_or_compile("/hello/").unwrap();
        let case_insensitive = cache.get_or_compile("/hello/i").unwrap();

        assert!(!std::rc::Rc::ptr_eq(&case_sensitive, &case_insensitive));
        assert!(case_sensitive.captures("HELLO").is_none());
        assert!(case_insensitive.captures("HELLO").is_some());
    }

    #[test]
    fn test_regex_cache_evicts_oldest_entry_at_capacity() {
        let mut cache = RegexCache::new(2);
        let first = cache.get_or_compile("/first/").unwrap();
        cache.get_or_compile("/second/").unwrap();
        cache.get_or_compile("/third/").unwrap();
        let recompiled_first = cache.get_or_compile("/first/").unwrap();

        assert!(!std::rc::Rc::ptr_eq(&first, &recompiled_first));
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.insertion_order.len(), 2);
    }

    #[test]
    fn test_regex_cache_does_not_store_invalid_patterns() {
        let mut cache = RegexCache::new(2);

        assert!(cache.get_or_compile("/(/").is_err());
        assert!(cache.entries.is_empty());
        assert!(cache.insertion_order.is_empty());
    }

    #[test]
    fn test_zero_capacity_disables_regex_caching() {
        let mut cache = RegexCache::new(0);
        let first = cache.get_or_compile("/hello/").unwrap();
        let second = cache.get_or_compile("/hello/").unwrap();

        assert!(!std::rc::Rc::ptr_eq(&first, &second));
        assert!(cache.entries.is_empty());
    }

    // ── P1: Backtracking through sequence ──────────────────────────────────

    #[test]
    fn test_backtrack_greedy_plus() {
        // a+ must give back one 'a' so that 'ab' can match
        let re = Regex::new("a+ab", RegexFlags::default()).unwrap();
        assert!(re.captures("aaab").is_some());
    }

    #[test]
    fn test_backtrack_alternation_in_group() {
        // (ab|a)b — first branch "ab" matches but then "b" fails;
        // must backtrack to second branch "a" so "b" succeeds
        let re = Regex::new("(ab|a)b", RegexFlags::default()).unwrap();
        assert!(re.captures("ab").is_some());
    }

    #[test]
    fn test_backtrack_greedy_star_gives_back() {
        // .*b must give back the 'b' at the end
        let re = Regex::new("^.*b$", RegexFlags::default()).unwrap();
        assert!(re.captures("xxxb").is_some());
    }

    // ── P2: Lookahead captures ─────────────────────────────────────────────

    #[test]
    fn test_lookahead_captures() {
        // (?=(a))a — capture group 1 from inside lookahead should survive
        let re = Regex::new("(?=(a))a", RegexFlags::default()).unwrap();
        let caps = re.captures("a").unwrap();
        assert_eq!(caps.get(1).unwrap().as_str("a"), "a");
    }

    // ── P2: Paired delimiters ──────────────────────────────────────────────

    #[test]
    fn test_paired_delimiter_braces() {
        let (pattern, _) = parse_php_regex("{a}").unwrap();
        assert_eq!(pattern, "a");
    }

    #[test]
    fn test_paired_delimiter_parens() {
        let (pattern, _) = parse_php_regex("(a)").unwrap();
        assert_eq!(pattern, "a");
    }

    #[test]
    fn test_paired_delimiter_brackets() {
        let (pattern, _) = parse_php_regex("[a]").unwrap();
        assert_eq!(pattern, "a");
    }

    #[test]
    fn test_paired_delimiter_angles() {
        let (pattern, _) = parse_php_regex("<a>i").unwrap();
        assert_eq!(pattern, "a");
    }

    #[test]
    fn test_paired_delimiter_malformed_trailing_text() {
        // {a}b} — 'b' is an unknown modifier, should error
        assert!(parse_php_regex("{a}b}").is_err());
    }

    // ── P2: Multi-digit backreferences ─────────────────────────────────────

    #[test]
    fn test_multi_digit_backref_in_replacement() {
        // 10 capture groups, $10 refers to the 10th
        let re = Regex::new("(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)", RegexFlags::default()).unwrap();
        assert_eq!(re.replace_all("abcdefghij", "$10"), "j");
    }

    #[test]
    fn test_multi_digit_backref_in_pattern() {
        // \10 refers to group 10
        let re = Regex::new("(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10", RegexFlags::default()).unwrap();
        assert!(re.captures("abcdefghijj").is_some());
    }

    // ── P2: Named groups ───────────────────────────────────────────────────

    #[test]
    fn test_named_group_p_syntax() {
        let re = Regex::new("(?P<name>abc)", RegexFlags::default()).unwrap();
        let caps = re.captures("abc").unwrap();
        assert_eq!(caps.get_named("name").unwrap().as_str("abc"), "abc");
    }

    #[test]
    fn test_named_group_angle_syntax() {
        let re = Regex::new("(?<word>\\w+)", RegexFlags::default()).unwrap();
        let caps = re.captures("hello").unwrap();
        assert_eq!(caps.get_named("word").unwrap().as_str("hello"), "hello");
    }

    // ── P1: Unknown modifiers are rejected ─────────────────────────────────

    #[test]
    fn test_unknown_modifier_rejected() {
        assert!(parse_php_regex("/a/z").is_err());
    }

    // ── P2: Named backreferences ───────────────────────────────────────────

    #[test]
    fn test_named_backref_k_syntax() {
        // (?<x>a)\k<x> matches "aa"
        let re = Regex::new("(?<x>a)\\k<x>", RegexFlags::default()).unwrap();
        assert!(re.captures("aa").is_some());
        assert!(re.captures("ab").is_none());
    }

    #[test]
    fn test_named_backref_p_equals_syntax() {
        // (?P<x>a)(?P=x) matches "aa"
        let re = Regex::new("(?P<x>a)(?P=x)", RegexFlags::default()).unwrap();
        assert!(re.captures("aa").is_some());
        assert!(re.captures("ab").is_none());
    }
}
