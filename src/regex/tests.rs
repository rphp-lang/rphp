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
fn test_possessive_quantifier_does_not_backtrack() {
    let ordinary = Regex::new("a+a", RegexFlags::default()).unwrap();
    let possessive = Regex::new("a++a", RegexFlags::default()).unwrap();
    assert!(ordinary.captures("aa").is_some());
    assert!(possessive.captures("aa").is_none());

    let symfony_group = Regex::new(r"\?P<([^>]++)>", RegexFlags::default()).unwrap();
    assert_eq!(
        symfony_group
            .captures("?P<slug>")
            .unwrap()
            .get(1)
            .unwrap()
            .as_str("?P<slug>"),
        "slug"
    );
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
fn test_is_match_scans_later_required_literal_candidates() {
    let re = Regex::new("(needle)", RegexFlags::default()).unwrap();

    assert!(re.is_match("not here, then needle"));
}

#[test]
fn test_is_match_keeps_anchor_and_end_position_semantics() {
    let anchored = Regex::new("^hello", RegexFlags::default()).unwrap();
    let end = Regex::new("$", RegexFlags::default()).unwrap();

    assert!(!anchored.is_match("xhello"));
    assert!(end.is_match("abc"));
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
fn test_capture_visitor_streams_named_utf8_matches() {
    let subject = "🙂 ž1 x č2";
    let re = Regex::new("(?P<letter>ž|č)(?P<digit>\\d)", RegexFlags::default()).unwrap();
    let mut seen = Vec::new();

    let visited: Result<usize, std::convert::Infallible> =
        re.try_visit_captures(subject, |captures| {
            seen.push((
                captures.get(0).unwrap().as_str(subject).to_string(),
                captures
                    .get(*captures.named_groups().get("letter").unwrap())
                    .unwrap()
                    .as_str(subject)
                    .to_string(),
            ));
            Ok(true)
        });

    assert_eq!(visited.unwrap(), 2);
    assert_eq!(
        seen,
        vec![
            ("ž1".to_string(), "ž".to_string()),
            ("č2".to_string(), "č".to_string())
        ]
    );
}

#[test]
fn test_count_matches_uses_ascii_and_capture_fallbacks() {
    let ascii = Regex::new("user[0-9]+", RegexFlags::default()).unwrap();
    let utf8 = Regex::new("uživatel[0-9]+", RegexFlags::default()).unwrap();
    let grouped = Regex::new("(a)", RegexFlags::default()).unwrap();

    assert_eq!(ascii.count_matches("user1 x user22"), 2);
    assert_eq!(utf8.count_matches("uživatel1 uživatel22"), 2);
    assert_eq!(grouped.count_matches("a a"), 2);
}

#[test]
fn test_capture_visitor_scans_case_insensitive_literal_candidates() {
    let flags = RegexFlags {
        case_insensitive: true,
        ..Default::default()
    };
    let re = Regex::new("hello", flags).unwrap();
    let mut starts = Vec::new();

    let visited: Result<usize, std::convert::Infallible> =
        re.try_visit_captures("xHELLO yhello", |captures| {
            starts.push(captures.get(0).unwrap().start);
            Ok(true)
        });

    assert_eq!(visited.unwrap(), 2);
    assert_eq!(starts, vec![1, 8]);
}

#[test]
fn test_capture_visitor_literal_scan_preserves_anchor_semantics() {
    let anchored = Regex::new("^hello", RegexFlags::default()).unwrap();
    let visited: Result<usize, std::convert::Infallible> =
        anchored.try_visit_captures("xhello", |_| Ok(true));
    assert_eq!(visited.unwrap(), 0);

    let multiline = Regex::new(
        "^hello",
        RegexFlags {
            multiline: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut starts = Vec::new();
    let visited: Result<usize, std::convert::Infallible> =
        multiline.try_visit_captures("x\nhello\nhello", |captures| {
            starts.push(captures.get(0).unwrap().start);
            Ok(true)
        });
    assert_eq!(visited.unwrap(), 2);
    assert_eq!(starts, vec![2, 8]);
}

#[test]
fn test_linear_capture_visitor_matches_fixed_prefix_and_terminal_class() {
    let subject = "xuser12 user3";
    let re = Regex::new("user[0-9]+", RegexFlags::default()).unwrap();

    assert!(linear::is_supported(&re.ast));
    let matches = re.captures_iter(subject);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].get(0).unwrap().as_str(subject), "user12");
    assert_eq!(matches[1].get(0).unwrap().as_str(subject), "user3");
}

#[test]
fn test_linear_capture_visitor_preserves_utf8_byte_offsets() {
    let subject = "🙂 uživatel12 x uživatel3";
    let re = Regex::new("uživatel[0-9]+", RegexFlags::default()).unwrap();

    assert!(linear::is_supported(&re.ast));
    let matches = re.captures_iter(subject);
    let offsets = matches
        .iter()
        .map(|captures| {
            let matched = captures.get(0).unwrap();
            (matched.start, matched.end, matched.as_str(subject))
        })
        .collect::<Vec<_>>();

    assert_eq!(offsets, vec![(5, 16, "uživatel12"), (19, 29, "uživatel3")]);
}

#[test]
fn test_linear_capture_visitor_matches_prefix_beyond_ascii_plan_limit() {
    let prefix = "a".repeat(33);
    let pattern = format!("{prefix}[0-9]+");
    let subject = format!("x{prefix}12 y{prefix}3");
    let re = Regex::new(&pattern, RegexFlags::default()).unwrap();

    assert!(linear::is_supported(&re.ast));
    let matches = re.captures_iter(&subject);
    assert_eq!(matches.len(), 2);
    assert_eq!(
        matches[0].get(0).unwrap().as_str(&subject),
        format!("{prefix}12")
    );
    assert_eq!(
        matches[1].get(0).unwrap().as_str(&subject),
        format!("{prefix}3")
    );
}

#[test]
fn test_linear_capture_visitor_preserves_greedy_lazy_and_bounded_tails() {
    let greedy = Regex::new("a{2,3}", RegexFlags::default()).unwrap();
    let lazy = Regex::new("a+?", RegexFlags::default()).unwrap();

    assert!(linear::is_supported(&greedy.ast));
    assert!(linear::is_supported(&lazy.ast));
    let greedy_lengths = greedy
        .captures_iter("aaaaa")
        .into_iter()
        .map(|captures| captures.get(0).unwrap().end - captures.get(0).unwrap().start)
        .collect::<Vec<_>>();
    let lazy_lengths = lazy
        .captures_iter("aaaa")
        .into_iter()
        .map(|captures| captures.get(0).unwrap().end - captures.get(0).unwrap().start)
        .collect::<Vec<_>>();

    assert_eq!(greedy_lengths, vec![3, 2]);
    assert_eq!(lazy_lengths, vec![1, 1, 1, 1]);
}

#[test]
fn test_linear_capture_visitor_rejects_continuations_and_captures() {
    let quantified_middle = Regex::new("a+ab", RegexFlags::default()).unwrap();
    let capture = Regex::new("(user)[0-9]+", RegexFlags::default()).unwrap();
    let alternation = Regex::new("user|admin", RegexFlags::default()).unwrap();

    assert!(!linear::is_supported(&quantified_middle.ast));
    assert!(!linear::is_supported(&capture.ast));
    assert!(!linear::is_supported(&alternation.ast));
}

#[test]
fn test_capture_visitor_stops_without_scanning_later_matches() {
    let re = Regex::new("\\d", RegexFlags::default()).unwrap();
    let mut seen = Vec::new();

    let visited: Result<usize, std::convert::Infallible> =
        re.try_visit_captures("1 2 3", |captures| {
            seen.push(captures.get(0).unwrap().start);
            Ok(seen.len() < 2)
        });

    assert_eq!(visited.unwrap(), 2);
    assert_eq!(seen, vec![0, 2]);
}

#[test]
fn test_capture_visitor_propagates_errors_without_later_visits() {
    let re = Regex::new("\\d", RegexFlags::default()).unwrap();
    let mut seen = Vec::new();

    let visited: Result<usize, &'static str> = re.try_visit_captures("1 2 3", |captures| {
        seen.push(captures.get(0).unwrap().start);
        if seen.len() == 2 {
            Err("stop")
        } else {
            Ok(true)
        }
    });

    assert_eq!(visited, Err("stop"));
    assert_eq!(seen, vec![0, 2]);
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

#[test]
fn test_php_delimiter_ignores_surrounding_whitespace() {
    let (pattern, flags) = parse_php_regex("\n    / a+ /x   \n").unwrap();
    assert_eq!(pattern, " a+ ");
    assert!(flags.extended);
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
