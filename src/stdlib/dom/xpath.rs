use super::model::{NodeId, NodeKind, Tree};

pub(super) fn query(tree: &Tree, context: NodeId, expression: &[u8]) -> Option<Vec<NodeId>> {
    let expression = trim_ascii(expression);
    if expression.is_empty() {
        return None;
    }
    if expression == b"//comment()" {
        let mut result = Vec::new();
        collect_kind(
            tree,
            0,
            |kind| matches!(kind, NodeKind::Comment(_)),
            &mut result,
        );
        return Some(result);
    }
    if let Some(name) = expression.strip_prefix(b"//") {
        if name.is_empty() || name.contains(&b'/') {
            return None;
        }
        if name == b"*" {
            let mut result = Vec::new();
            collect_kind(
                tree,
                0,
                |kind| matches!(kind, NodeKind::Element { .. }),
                &mut result,
            );
            return Some(result);
        }
        return Some(tree.descendants_named(0, name));
    }

    let absolute = expression.starts_with(b"/");
    let expression = expression.strip_prefix(b"/").unwrap_or(expression);
    let segments = expression
        .split(|byte| *byte == b'/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Some(vec![0]);
    }

    let relative_context = if !absolute && matches!(tree.nodes[context].kind, NodeKind::Document) {
        tree.document_element().unwrap_or(context)
    } else {
        context
    };
    let mut current = vec![if absolute { 0 } else { relative_context }];
    for segment in segments {
        let mut next = Vec::new();
        for node in current {
            for child in &tree.nodes[node].children {
                if matches_segment(tree, *child, segment) {
                    next.push(*child);
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    Some(current)
}

fn matches_segment(tree: &Tree, node: NodeId, segment: &[u8]) -> bool {
    match (&tree.nodes[node].kind, segment) {
        (NodeKind::Comment(_), b"comment()") => true,
        (NodeKind::Text(_), b"text()") => true,
        (NodeKind::Element { .. }, b"*") => true,
        (
            NodeKind::Element {
                qualified_name,
                local_name,
                ..
            },
            name,
        ) => qualified_name == name || local_name == name,
        _ => false,
    }
}

fn collect_kind(
    tree: &Tree,
    node: NodeId,
    predicate: impl Copy + Fn(&NodeKind) -> bool,
    result: &mut Vec<NodeId>,
) {
    for child in &tree.nodes[node].children {
        if predicate(&tree.nodes[*child].kind) {
            result.push(*child);
        }
        collect_kind(tree, *child, predicate, result);
    }
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::dom::parser::parse_document;

    #[test]
    fn evaluates_phpunit_relative_paths_and_comment_descendants() {
        let tree = parse_document(
            b"<phpunit><testsuites><testsuite/><testsuite/></testsuites><!--x--></phpunit>",
            false,
        )
        .unwrap();
        assert_eq!(query(&tree, 0, b"testsuites/testsuite").unwrap().len(), 2);
        assert_eq!(query(&tree, 0, b"//comment()").unwrap().len(), 1);
    }
}
