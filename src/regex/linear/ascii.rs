//! ASCII byte executor for capture-free linear regex shapes.

use super::super::{
    Anchor, CaptureView, ClassItem, Match, NewlineConvention, Node, Regex, RegexFlags, chars_equal,
    is_word_char, match_class_item, match_shorthand,
};

const PREFIX_LIMIT: usize = 32;

pub(super) fn try_first_match(regex: &Regex, subject: &str) -> Option<Option<Match>> {
    let bytes = subject.as_bytes();
    if starts_at_search_boundary(&regex.ast) || regex.flags.anchored {
        let inspected = maximum_consumption(&regex.ast)?.min(bytes.len());
        if !bytes[..inspected].is_ascii() {
            return None;
        }
    } else if !subject.is_ascii() {
        return None;
    }
    let mut pos = 0usize;
    while pos <= bytes.len() {
        if let Some(end) = match_no_capture(&regex.ast, pos, bytes, regex.flags, 0) {
            return Some(Some(Match { start: pos, end }));
        }
        if regex.flags.anchored {
            break;
        }
        pos += 1;
    }
    Some(None)
}

fn starts_at_search_boundary(node: &Node) -> bool {
    match node {
        Node::Anchor(Anchor::SearchStart | Anchor::AbsoluteStart) => true,
        Node::Sequence(nodes) => nodes.first().is_some_and(starts_at_search_boundary),
        Node::Group { inner, .. } => starts_at_search_boundary(inner),
        _ => false,
    }
}

fn maximum_consumption(node: &Node) -> Option<usize> {
    match node {
        Node::Literal(_)
        | Node::AnyChar
        | Node::ByteUnit
        | Node::NotNewline
        | Node::CharClass { .. }
        | Node::Shorthand(_) => Some(1),
        Node::Anchor(_) | Node::WordBoundary(_) => Some(0),
        Node::Group { inner, .. } => maximum_consumption(inner),
        Node::Sequence(nodes) => nodes.iter().try_fold(0usize, |total, node| {
            total.checked_add(maximum_consumption(node)?)
        }),
        Node::Quantifier { inner, max, .. } => maximum_consumption(inner)?.checked_mul((*max)?),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Tail<'a> {
    inner: &'a Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
}

#[derive(Clone, Copy)]
struct PrefixPlan<'a> {
    prefix: [u8; PREFIX_LIMIT],
    len: usize,
    tail: Option<Tail<'a>>,
}

#[derive(Clone, Copy)]
struct ClassTailPlan<'a> {
    negated: bool,
    items: &'a [ClassItem],
    min: usize,
    max: Option<usize>,
    greedy: bool,
}

/// Count fixed-prefix ASCII matches directly. The count-only `preg_match_all`
/// form never observes captures, so keeping its loop non-generic avoids both
/// the group-zero write and visitor code-layout sensitivity. Other shapes
/// return before scanning the subject and retain the existing visitor path.
#[inline(never)]
pub(super) fn try_count_matches(regex: &Regex, subject: &str) -> Option<usize> {
    if regex.flags.case_insensitive {
        return None;
    }
    let prefix_plan = prefix_plan(&regex.ast);
    let class_tail_plan = class_tail_plan(&regex.ast);
    if prefix_plan.is_none() && class_tail_plan.is_none() {
        return None;
    }
    if !subject.is_ascii() {
        return None;
    }
    let bytes = subject.as_bytes();
    let mut pos = 0;
    let mut count = 0;
    let start_literal = regex.start_literal;

    while pos <= bytes.len() {
        if let Some(literal) = start_literal {
            let Some(relative_pos) = find_literal(&bytes[pos..], literal) else {
                break;
            };
            pos += relative_pos;
        }
        let end = match prefix_plan {
            Some(plan) => match_prefix_plan(plan, pos, bytes, regex.flags),
            None => match_terminal_class(class_tail_plan.unwrap(), pos, bytes, regex.flags),
        };
        if let Some(end) = end {
            count += 1;
            pos = if end == pos { pos + 1 } else { end };
        } else {
            pos += 1;
        }
    }
    Some(count)
}

/// Scan an ASCII subject without materializing `Vec<char>`. ASCII character
/// indexes are exact UTF-8 byte offsets, so capture boundaries remain direct.
#[inline(never)]
pub(super) fn try_visit_captures<E, F>(
    regex: &Regex,
    subject: &str,
    visitor: &mut F,
) -> Result<Option<usize>, E>
where
    F: for<'capture> FnMut(CaptureView<'capture>) -> Result<bool, E>,
{
    if regex.flags.case_insensitive {
        return Ok(None);
    }
    let prefix_plan = prefix_plan(&regex.ast);
    let class_tail_plan = class_tail_plan(&regex.ast);
    let grouped = regex.num_groups > 0 && super::is_capture_visitor_supported(&regex.ast);
    if !grouped && prefix_plan.is_none() && class_tail_plan.is_none() {
        return Ok(None);
    }
    let bytes = subject.as_bytes();
    let mut groups = vec![None; regex.num_groups + 1];
    let mut pos = 0;
    let mut count = 0;
    let start_literal = regex.start_literal;

    while pos <= bytes.len() {
        if let Some(literal) = start_literal {
            let Some(relative_pos) = find_literal(&bytes[pos..], literal) else {
                break;
            };
            pos += relative_pos;
        }
        let end = if grouped {
            match_with_captures(&regex.ast, pos, bytes, regex.flags, 0, &mut groups, true)
        } else {
            match prefix_plan {
                Some(plan) => match_prefix_plan(plan, pos, bytes, regex.flags),
                None => match_terminal_class(class_tail_plan.unwrap(), pos, bytes, regex.flags),
            }
        };
        if let Some(end) = end {
            groups[0] = Some(Match { start: pos, end });
            count += 1;
            if !visitor(CaptureView {
                groups: &groups,
                symbols: &regex.symbols,
                mark: None,
            })? {
                break;
            }
            if end == pos {
                pos += 1;
            } else {
                pos = end;
            }
        } else {
            pos += 1;
        }
    }
    Ok(Some(count))
}

#[allow(clippy::too_many_arguments)]
fn match_with_captures(
    node: &Node,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
    search_start: usize,
    groups: &mut [Option<Match>],
    terminal: bool,
) -> Option<usize> {
    match node {
        Node::Sequence(nodes) => {
            let mut current = pos;
            for (index, node) in nodes.iter().enumerate() {
                current = match_with_captures(
                    node,
                    current,
                    bytes,
                    flags,
                    search_start,
                    groups,
                    terminal && index + 1 == nodes.len(),
                )?;
            }
            Some(current)
        }
        Node::Group { index, inner, .. } => {
            let end =
                match_with_captures(inner, pos, bytes, flags, search_start, groups, terminal)?;
            if let Some(index) = index {
                groups[*index] = Some(Match { start: pos, end });
            }
            Some(end)
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            ..
        } if terminal => match_terminal_quantifier_at(
            inner,
            *min,
            *max,
            *greedy,
            pos,
            bytes,
            flags,
            search_start,
        ),
        _ => match_atom(node, pos, bytes, flags, search_start),
    }
}

fn class_tail_plan(node: &Node) -> Option<ClassTailPlan<'_>> {
    let Node::Quantifier {
        inner,
        min,
        max,
        greedy,
        ..
    } = node
    else {
        return None;
    };
    let Node::CharClass { negated, items } = inner.as_ref() else {
        return None;
    };
    Some(ClassTailPlan {
        negated: *negated,
        items,
        min: *min,
        max: *max,
        greedy: *greedy,
    })
}

fn prefix_plan(node: &Node) -> Option<PrefixPlan<'_>> {
    let mut plan = PrefixPlan {
        prefix: [0; PREFIX_LIMIT],
        len: 0,
        tail: None,
    };

    let mut push_literal = |literal: char| {
        if !literal.is_ascii() || plan.len == PREFIX_LIMIT {
            return false;
        }
        plan.prefix[plan.len] = literal as u8;
        plan.len += 1;
        true
    };

    match node {
        Node::Sequence(nodes) => {
            for (index, node) in nodes.iter().enumerate() {
                match node {
                    Node::Literal(literal) if push_literal(*literal) => {}
                    Node::Quantifier {
                        inner,
                        min,
                        max,
                        greedy,
                        ..
                    } if index + 1 == nodes.len() => {
                        plan.tail = Some(Tail {
                            inner,
                            min: *min,
                            max: *max,
                            greedy: *greedy,
                        });
                    }
                    _ => return None,
                }
            }
        }
        Node::Literal(literal) if push_literal(*literal) => {}
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            ..
        } => {
            plan.tail = Some(Tail {
                inner,
                min: *min,
                max: *max,
                greedy: *greedy,
            });
        }
        _ => return None,
    }
    (plan.len > 0).then_some(plan)
}

#[inline]
fn find_literal(bytes: &[u8], literal: char) -> Option<usize> {
    debug_assert!(literal.is_ascii());
    let literal = literal as u8;
    bytes.iter().position(|&candidate| candidate == literal)
}

#[inline]
fn match_prefix_plan(
    plan: PrefixPlan<'_>,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
) -> Option<usize> {
    let prefix_end = pos.checked_add(plan.len)?;
    if bytes.get(pos..prefix_end)? != &plan.prefix[..plan.len] {
        return None;
    }
    match plan.tail {
        Some(tail) => {
            if let Node::CharClass { negated, items } = tail.inner {
                match_terminal_class(
                    ClassTailPlan {
                        negated: *negated,
                        items,
                        min: tail.min,
                        max: tail.max,
                        greedy: tail.greedy,
                    },
                    prefix_end,
                    bytes,
                    flags,
                )
            } else {
                match_terminal_quantifier(
                    tail.inner,
                    tail.min,
                    tail.max,
                    tail.greedy,
                    prefix_end,
                    bytes,
                    flags,
                )
            }
        }
        None => Some(prefix_end),
    }
}

#[inline]
fn match_terminal_class(
    plan: ClassTailPlan<'_>,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
) -> Option<usize> {
    let limit = plan.max.unwrap_or(usize::MAX);
    let target = if plan.greedy { limit } else { plan.min };
    let mut current = pos;
    let mut repetitions = 0;

    if let [ClassItem::Range(lower, upper)] = plan.items
        && lower.is_ascii()
        && upper.is_ascii()
    {
        let (lower, upper) = if flags.case_insensitive {
            (
                (*lower as u8).to_ascii_lowercase(),
                (*upper as u8).to_ascii_lowercase(),
            )
        } else {
            (*lower as u8, *upper as u8)
        };
        while repetitions < target && current < bytes.len() {
            let candidate = if flags.case_insensitive {
                bytes[current].to_ascii_lowercase()
            } else {
                bytes[current]
            };
            if (candidate >= lower && candidate <= upper) == plan.negated {
                break;
            }
            current += 1;
            repetitions += 1;
        }
        return (repetitions >= plan.min).then_some(current);
    }

    while repetitions < target && current < bytes.len() {
        let in_class = plan
            .items
            .iter()
            .any(|item| match_ascii_class_item(item, bytes[current], flags));
        if in_class == plan.negated {
            break;
        }
        current += 1;
        repetitions += 1;
    }

    (repetitions >= plan.min).then_some(current)
}

#[inline]
fn match_terminal_quantifier(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
) -> Option<usize> {
    let limit = max.unwrap_or(usize::MAX);
    let target = if greedy { limit } else { min };
    let mut current = pos;
    let mut repetitions = 0;

    while repetitions < target {
        let Some(next) = match_atom(inner, current, bytes, flags, 0) else {
            break;
        };
        current = next;
        repetitions += 1;
    }

    (repetitions >= min).then_some(current)
}

#[inline]
fn match_no_capture(
    node: &Node,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
    search_start: usize,
) -> Option<usize> {
    match node {
        Node::Sequence(nodes) => {
            let mut current = pos;
            for node in nodes {
                current = match node {
                    Node::Quantifier {
                        inner,
                        min,
                        max,
                        greedy,
                        ..
                    } => match_terminal_quantifier_at(
                        inner,
                        *min,
                        *max,
                        *greedy,
                        current,
                        bytes,
                        flags,
                        search_start,
                    )?,
                    _ => match_atom(node, current, bytes, flags, search_start)?,
                };
            }
            Some(current)
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
            ..
        } => match_terminal_quantifier_at(
            inner,
            *min,
            *max,
            *greedy,
            pos,
            bytes,
            flags,
            search_start,
        ),
        _ => match_atom(node, pos, bytes, flags, search_start),
    }
}

#[inline]
fn match_terminal_quantifier_at(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
    search_start: usize,
) -> Option<usize> {
    let limit = max.unwrap_or(usize::MAX);
    let target = if greedy { limit } else { min };
    let mut current = pos;
    let mut repetitions = 0;
    while repetitions < target {
        let Some(next) = match_atom(inner, current, bytes, flags, search_start) else {
            break;
        };
        current = next;
        repetitions += 1;
    }
    (repetitions >= min).then_some(current)
}

fn match_atom(
    node: &Node,
    pos: usize,
    bytes: &[u8],
    flags: RegexFlags,
    search_start: usize,
) -> Option<usize> {
    match node {
        Node::Literal(literal) => {
            if pos >= bytes.len() {
                return None;
            }
            let candidate = bytes[pos];
            let matches = if literal.is_ascii() && !flags.case_insensitive {
                candidate == *literal as u8
            } else {
                chars_equal(char::from(candidate), *literal, flags)
            };
            matches.then_some(pos + 1)
        }
        Node::AnyChar => (pos < bytes.len()
            && (flags.dotall
                || newline_len_at_bytes(bytes, pos, flags.line_options.newline()).is_none()))
        .then_some(pos + 1),
        Node::ByteUnit => (pos < bytes.len()).then_some(pos + 1),
        Node::NotNewline => (pos < bytes.len()
            && newline_len_at_bytes(bytes, pos, flags.line_options.newline()).is_none())
        .then_some(pos + 1),
        Node::Anchor(Anchor::Start) => {
            let matches = if flags.multiline {
                pos == 0 || newline_ends_at_bytes(bytes, pos, flags.line_options.newline())
            } else {
                pos == 0
            };
            matches.then_some(pos)
        }
        Node::Anchor(Anchor::AbsoluteStart) => (pos == 0).then_some(pos),
        Node::Anchor(Anchor::AbsoluteEnd) => (pos == bytes.len()).then_some(pos),
        Node::Anchor(Anchor::FinalEnd) => (pos == bytes.len()
            || newline_len_at_bytes(bytes, pos, flags.line_options.newline())
                .is_some_and(|length| pos + length == bytes.len()))
        .then_some(pos),
        Node::Anchor(Anchor::SearchStart) => (pos == search_start).then_some(pos),
        Node::Anchor(Anchor::End) => {
            let matches = if pos == bytes.len() {
                true
            } else if flags.multiline {
                newline_len_at_bytes(bytes, pos, flags.line_options.newline()).is_some()
            } else if flags.line_options.dollar_end_only() {
                false
            } else {
                newline_len_at_bytes(bytes, pos, flags.line_options.newline())
                    .is_some_and(|length| pos + length == bytes.len())
            };
            matches.then_some(pos)
        }
        Node::WordBoundary(positive) => {
            (is_word_boundary(bytes, pos, flags.unicode_mode.ucp()) == *positive).then_some(pos)
        }
        Node::CharClass { negated, items } => {
            if pos >= bytes.len() {
                return None;
            }
            let in_class = items
                .iter()
                .any(|item| match_ascii_class_item(item, bytes[pos], flags));
            (in_class != *negated).then_some(pos + 1)
        }
        Node::Shorthand(shorthand) => (pos < bytes.len()
            && match_shorthand(*shorthand, char::from(bytes[pos]), flags.unicode_mode.ucp()))
        .then_some(pos + 1),
        Node::Group { inner, .. } => match_no_capture(inner, pos, bytes, flags, search_start),
        _ => None,
    }
}

/// Keep ordinary ASCII literal/range classes out of the Unicode property and
/// case-folding machinery. Complex items still delegate to the canonical
/// matcher, so extending the public PCRE property surface does not lengthen
/// the overwhelmingly common `[0-9]`/`[A-Z]` scan loop.
#[inline(always)]
fn match_ascii_class_item(item: &ClassItem, candidate: u8, flags: RegexFlags) -> bool {
    match item {
        ClassItem::Literal(literal) if literal.is_ascii() => {
            let literal = *literal as u8;
            if flags.case_insensitive {
                candidate.eq_ignore_ascii_case(&literal)
            } else {
                candidate == literal
            }
        }
        ClassItem::Range(lower, upper) if lower.is_ascii() && upper.is_ascii() => {
            let (candidate, lower, upper) = if flags.case_insensitive {
                (
                    candidate.to_ascii_lowercase(),
                    (*lower as u8).to_ascii_lowercase(),
                    (*upper as u8).to_ascii_lowercase(),
                )
            } else {
                (candidate, *lower as u8, *upper as u8)
            };
            candidate >= lower && candidate <= upper
        }
        _ => match_class_item(item, char::from(candidate), flags),
    }
}

fn newline_len_at_bytes(bytes: &[u8], pos: usize, convention: NewlineConvention) -> Option<usize> {
    let first = *bytes.get(pos)?;
    match convention {
        NewlineConvention::Lf => (first == b'\n').then_some(1),
        NewlineConvention::Cr => (first == b'\r').then_some(1),
        NewlineConvention::CrLf => {
            (first == b'\r' && bytes.get(pos + 1) == Some(&b'\n')).then_some(2)
        }
        NewlineConvention::AnyCrLf | NewlineConvention::Any => {
            if first == b'\r' && bytes.get(pos + 1) == Some(&b'\n') {
                Some(2)
            } else if matches!(first, b'\r' | b'\n')
                || (convention == NewlineConvention::Any && matches!(first, 0x0b | 0x0c | 0x85))
            {
                Some(1)
            } else {
                None
            }
        }
        NewlineConvention::Nul => (first == 0).then_some(1),
    }
}

fn newline_ends_at_bytes(bytes: &[u8], pos: usize, convention: NewlineConvention) -> bool {
    (pos > 0
        && newline_len_at_bytes(bytes, pos - 1, convention)
            .is_some_and(|length| pos - 1 + length == pos))
        || (pos > 1
            && newline_len_at_bytes(bytes, pos - 2, convention)
                .is_some_and(|length| pos - 2 + length == pos))
}

#[inline]
fn is_word_boundary(bytes: &[u8], pos: usize, unicode: bool) -> bool {
    let before = pos > 0 && is_word_char(char::from(bytes[pos - 1]), unicode);
    let after = pos < bytes.len() && is_word_char(char::from(bytes[pos]), unicode);
    before != after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_admits_bounded_prefixes_and_terminal_classes() {
        let short = Regex::new("user[0-9]+", RegexFlags::default()).unwrap();
        let no_prefix = Regex::new("[0-9]+", RegexFlags::default()).unwrap();
        let long_pattern = format!("{}[0-9]+", "a".repeat(PREFIX_LIMIT + 1));
        let long = Regex::new(&long_pattern, RegexFlags::default()).unwrap();

        assert!(prefix_plan(&short.ast).is_some());
        assert!(prefix_plan(&no_prefix.ast).is_none());
        assert!(prefix_plan(&long.ast).is_none());
        assert!(class_tail_plan(&no_prefix.ast).is_some());
        assert!(class_tail_plan(&short.ast).is_none());
    }
}
