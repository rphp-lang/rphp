//! ASCII byte executor for capture-free linear regex shapes.

use super::super::{
    Anchor, CaptureView, ClassItem, Match, Node, Regex, RegexFlags, chars_equal, is_word_char,
    match_class_item, match_shorthand,
};

const PREFIX_LIMIT: usize = 32;

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
    if prefix_plan.is_none() && class_tail_plan.is_none() {
        return Ok(None);
    }
    let bytes = subject.as_bytes();
    let mut groups = [None];
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
        groups.fill(None);
        let end = match prefix_plan {
            Some(plan) => match_prefix_plan(plan, pos, bytes, regex.flags),
            None => match_terminal_class(class_tail_plan.unwrap(), pos, bytes),
        };
        if let Some(end) = end {
            groups[0] = Some(Match { start: pos, end });
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
    Ok(Some(count))
}

fn class_tail_plan(node: &Node) -> Option<ClassTailPlan<'_>> {
    let Node::Quantifier {
        inner,
        min,
        max,
        greedy,
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
        Some(tail) => match_terminal_quantifier(
            tail.inner,
            tail.min,
            tail.max,
            tail.greedy,
            prefix_end,
            bytes,
            flags,
        ),
        None => Some(prefix_end),
    }
}

#[inline]
fn match_terminal_class(plan: ClassTailPlan<'_>, pos: usize, bytes: &[u8]) -> Option<usize> {
    let limit = plan.max.unwrap_or(usize::MAX);
    let target = if plan.greedy { limit } else { plan.min };
    let mut current = pos;
    let mut repetitions = 0;

    while repetitions < target && current < bytes.len() {
        let candidate = char::from(bytes[current]);
        let in_class = plan
            .items
            .iter()
            .any(|item| match_class_item(item, candidate, false));
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
        let Some(next) = match_atom(inner, current, bytes, flags) else {
            break;
        };
        current = next;
        repetitions += 1;
    }

    (repetitions >= min).then_some(current)
}

#[inline]
fn match_atom(node: &Node, pos: usize, bytes: &[u8], flags: RegexFlags) -> Option<usize> {
    match node {
        Node::Literal(literal) => {
            if pos >= bytes.len() {
                return None;
            }
            let candidate = bytes[pos];
            let matches = if literal.is_ascii() && !flags.case_insensitive {
                candidate == *literal as u8
            } else {
                chars_equal(char::from(candidate), *literal, flags.case_insensitive)
            };
            matches.then_some(pos + 1)
        }
        Node::AnyChar => {
            (pos < bytes.len() && (flags.dotall || bytes[pos] != b'\n')).then_some(pos + 1)
        }
        Node::Anchor(Anchor::Start) => {
            let matches = if flags.multiline {
                pos == 0 || (pos > 0 && bytes[pos - 1] == b'\n')
            } else {
                pos == 0
            };
            matches.then_some(pos)
        }
        Node::Anchor(Anchor::End) => {
            let matches = if flags.multiline {
                pos == bytes.len() || bytes[pos] == b'\n'
            } else {
                pos == bytes.len()
            };
            matches.then_some(pos)
        }
        Node::WordBoundary(positive) => (is_word_boundary(bytes, pos) == *positive).then_some(pos),
        Node::CharClass { negated, items } => {
            if pos >= bytes.len() {
                return None;
            }
            let candidate = char::from(bytes[pos]);
            let in_class = items
                .iter()
                .any(|item| match_class_item(item, candidate, flags.case_insensitive));
            (in_class != *negated).then_some(pos + 1)
        }
        Node::Shorthand(shorthand) => (pos < bytes.len()
            && match_shorthand(*shorthand, char::from(bytes[pos])))
        .then_some(pos + 1),
        _ => None,
    }
}

#[inline]
fn is_word_boundary(bytes: &[u8], pos: usize) -> bool {
    let before = pos > 0 && is_word_char(char::from(bytes[pos - 1]));
    let after = pos < bytes.len() && is_word_char(char::from(bytes[pos]));
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
