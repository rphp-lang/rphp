//! Bounded iterative executor for capture-free linear regex shapes.
//!
//! This stays in a separate module so admitting the fast path does not enlarge
//! or perturb the canonical backtracking loop used by the full PCRE subset.

use super::{
    Anchor, CaptureView, Match, Node, Regex, RegexFlags, chars_equal, is_word_boundary,
    match_class_item, match_shorthand, subject_chars,
};

/// Prove a shape that never needs continuation backtracking or capture state.
/// A terminal quantifier is safe because no later atom can ask it to give
/// characters back; every preceding atom has exactly one outcome.
pub(super) fn is_supported(node: &Node) -> bool {
    match node {
        Node::Sequence(nodes) => nodes.iter().enumerate().all(|(index, node)| match node {
            Node::Quantifier { inner, .. } => {
                index + 1 == nodes.len() && is_linear_consuming_atom(inner)
            }
            _ => is_linear_atom(node),
        }),
        Node::Quantifier { inner, .. } => is_linear_consuming_atom(inner),
        _ => is_linear_atom(node),
    }
}

/// Scan non-overlapping matches with one reusable group-zero slot. Keeping the
/// generic visitor boundary out of `regex.rs` isolates this monomorphized loop
/// from the canonical matcher codegen unit.
#[inline(never)]
pub(super) fn try_visit_captures<E, F>(
    regex: &Regex,
    subject: &str,
    mut visitor: F,
) -> Result<usize, E>
where
    F: for<'capture> FnMut(CaptureView<'capture>) -> Result<bool, E>,
{
    let (chars, byte_offsets) = subject_chars(subject);
    let mut groups = vec![None];
    let mut pos = 0;
    let mut count = 0;
    let start_literal = regex.start_literal;

    while pos <= chars.len() {
        if let Some(literal) = start_literal {
            let Some(relative_pos) = chars[pos..].iter().position(|&candidate| {
                chars_equal(candidate, literal, regex.flags.case_insensitive)
            }) else {
                break;
            };
            pos += relative_pos;
        }
        groups.fill(None);
        if let Some(end) = match_no_capture(&regex.ast, pos, &chars, regex.flags) {
            groups[0] = Some(Match {
                start: byte_offsets.get(pos),
                end: byte_offsets.get(end),
            });
            count += 1;
            if !visitor(CaptureView {
                groups: &groups,
                named_groups: &regex.named_groups,
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
    Ok(count)
}

fn is_linear_atom(node: &Node) -> bool {
    matches!(
        node,
        Node::Literal(_)
            | Node::AnyChar
            | Node::Anchor(_)
            | Node::CharClass { .. }
            | Node::Shorthand(_)
            | Node::WordBoundary(_)
    )
}

fn is_linear_consuming_atom(node: &Node) -> bool {
    matches!(
        node,
        Node::Literal(_) | Node::AnyChar | Node::CharClass { .. } | Node::Shorthand(_)
    )
}

#[inline]
fn match_no_capture(node: &Node, pos: usize, chars: &[char], flags: RegexFlags) -> Option<usize> {
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
                    } => match_terminal_quantifier(
                        inner, *min, *max, *greedy, current, chars, flags,
                    )?,
                    _ => match_atom(node, current, chars, flags)?,
                };
            }
            Some(current)
        }
        Node::Quantifier {
            inner,
            min,
            max,
            greedy,
        } => match_terminal_quantifier(inner, *min, *max, *greedy, pos, chars, flags),
        _ => match_atom(node, pos, chars, flags),
    }
}

#[inline]
fn match_terminal_quantifier(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    pos: usize,
    chars: &[char],
    flags: RegexFlags,
) -> Option<usize> {
    let limit = max.unwrap_or(usize::MAX);
    let target = if greedy { limit } else { min };
    let mut current = pos;
    let mut repetitions = 0;

    while repetitions < target {
        let Some(next) = match_atom(inner, current, chars, flags) else {
            break;
        };
        current = next;
        repetitions += 1;
    }

    (repetitions >= min).then_some(current)
}

#[inline]
fn match_atom(node: &Node, pos: usize, chars: &[char], flags: RegexFlags) -> Option<usize> {
    match node {
        Node::Literal(literal) => (pos < chars.len()
            && chars_equal(chars[pos], *literal, flags.case_insensitive))
        .then_some(pos + 1),
        Node::AnyChar => {
            (pos < chars.len() && (flags.dotall || chars[pos] != '\n')).then_some(pos + 1)
        }
        Node::Anchor(Anchor::Start) => {
            let matches = if flags.multiline {
                pos == 0 || (pos > 0 && chars[pos - 1] == '\n')
            } else {
                pos == 0
            };
            matches.then_some(pos)
        }
        Node::Anchor(Anchor::End) => {
            let matches = if flags.multiline {
                pos == chars.len() || chars[pos] == '\n'
            } else {
                pos == chars.len()
            };
            matches.then_some(pos)
        }
        Node::WordBoundary(positive) => (is_word_boundary(chars, pos) == *positive).then_some(pos),
        Node::CharClass { negated, items } => {
            if pos >= chars.len() {
                return None;
            }
            let in_class = items
                .iter()
                .any(|item| match_class_item(item, chars[pos], flags.case_insensitive));
            (in_class != *negated).then_some(pos + 1)
        }
        Node::Shorthand(shorthand) => {
            (pos < chars.len() && match_shorthand(*shorthand, chars[pos])).then_some(pos + 1)
        }
        _ => None,
    }
}
