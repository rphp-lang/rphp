/// Return the final namespace segment that determines whether a class-like
/// name collides with PHP's built-in type and pseudo-class vocabulary.
fn terminal_segment(name: &str) -> &str {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
}

/// Names accepted by the grammar as identifiers but forbidden for class-like
/// declarations and class import aliases.
pub(crate) fn is_semantically_reserved(name: &str) -> bool {
    let candidate = terminal_segment(name);
    match candidate.len() {
        3 => candidate.eq_ignore_ascii_case("int"),
        4 => matches_ignore_ascii_case(candidate, &["self", "bool", "true", "null", "void"]),
        5 => matches_ignore_ascii_case(candidate, &["float", "false", "mixed", "never"]),
        6 => matches_ignore_ascii_case(candidate, &["parent", "string", "object"]),
        8 => candidate.eq_ignore_ascii_case("iterable"),
        _ => false,
    }
}

/// Runtime strings passed to class_alias() bypass the grammar, so they must
/// additionally reject names whose declaration spelling is a lexical keyword.
pub(crate) fn is_reserved_alias(name: &str) -> bool {
    if is_semantically_reserved(name) {
        return true;
    }
    let candidate = terminal_segment(name);
    match candidate.len() {
        5 => candidate.eq_ignore_ascii_case("array"),
        6 => candidate.eq_ignore_ascii_case("static"),
        8 => candidate.eq_ignore_ascii_case("callable"),
        _ => false,
    }
}

fn matches_ignore_ascii_case(candidate: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| candidate.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::{is_reserved_alias, is_semantically_reserved};

    #[test]
    fn semantic_names_are_case_insensitive_and_use_the_terminal_segment() {
        for name in [
            "self", "parent", "int", "float", "bool", "string", "true", "false", "null", "void",
            "iterable", "object", "mixed", "never",
        ] {
            assert!(is_semantically_reserved(name), "missing {name}");
            assert!(
                is_semantically_reserved(&format!("\\Vendor\\{}", name.to_ascii_uppercase())),
                "missing qualified {name}"
            );
        }
        for name in [
            "resource",
            "numeric",
            "scalar",
            "binary",
            "integer",
            "double",
            "boolean",
            "real",
            "OrdinaryClass",
        ] {
            assert!(!is_semantically_reserved(name), "unexpected {name}");
        }
    }

    #[test]
    fn runtime_aliases_add_lexical_class_keywords() {
        for name in ["array", "callable", "static", "Vendor\\ARRAY"] {
            assert!(is_reserved_alias(name), "missing {name}");
        }
        for name in ["resource", "Vendor\\Resource", "_"] {
            assert!(!is_reserved_alias(name), "unexpected {name}");
        }
    }
}
