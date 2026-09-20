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
fn dollar_anchor_uses_the_selected_final_newline_unless_end_only() {
    let regular = Regex::new("^payload$", RegexFlags::default()).unwrap();
    assert!(regular.is_match("payload\n"));
    assert!(!regular.is_match("payload\r\n"));
    assert!(!regular.is_match("payload\nnext"));

    let crlf = Regex::new("(*CRLF)^payload$", RegexFlags::default()).unwrap();
    assert!(crlf.is_match("payload\r\n"));
    assert!(!crlf.is_match("payload\n"));

    let mut cache = RegexCache::default();
    let end_only = cache.get_or_compile("/^payload$/D").unwrap();
    assert!(end_only.is_match("payload"));
    assert!(!end_only.is_match("payload\n"));
    assert!(!end_only.is_match("payload\r\n"));
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
fn caseless_matching_uses_ascii_tables_without_unicode_and_simple_fold_with_it() {
    assert!(php_regex("/^s$/iu").is_match("ſ"));
    assert!(php_regex("/^σ$/iu").is_match("ς"));
    assert!(php_regex("/^[a-z]$/iu").is_match("K"));
    assert!(!php_regex("/^é$/i").is_match("É"));

    let (pattern, flags) = parse_php_regex("/^\\xC9$/i").unwrap();
    assert!(!Regex::new(&pattern, flags).unwrap().is_match("é"));
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
fn capture_backtracking_keeps_the_registers_for_each_candidate_path() {
    let repeated = Regex::new("^(/([a-z]*))*$", RegexFlags::default()).unwrap();
    let captures = repeated.captures("//abcde").unwrap();
    assert_eq!(captures.get(1).unwrap().as_str("//abcde"), "/abcde");
    assert_eq!(captures.get(2).unwrap().as_str("//abcde"), "abcde");

    let nested = Regex::new(
        "(?P<date>(?P<year>(\\d{2})?\\d{2})-(?P<month>\\d{2}|[a-z]{3})-(?P<day>\\d{2}))",
        RegexFlags {
            case_insensitive: true,
            ..RegexFlags::default()
        },
    )
    .unwrap();
    let captures = nested.captures("2006-05-13").unwrap();
    assert_eq!(
        captures.get_named("year").unwrap().as_str("2006-05-13"),
        "2006"
    );
    assert_eq!(
        captures.get_named("month").unwrap().as_str("2006-05-13"),
        "05"
    );
    assert_eq!(
        captures.get_named("day").unwrap().as_str("2006-05-13"),
        "13"
    );

    let alternative = Regex::new("(a)|(b)", RegexFlags::default()).unwrap();
    let captures = alternative.captures("b").unwrap();
    assert!(captures.get(1).is_none());
    assert_eq!(captures.get(2).unwrap().as_str("b"), "b");
}

#[test]
fn a_final_zero_width_repetition_publishes_its_capture_registers() {
    let regex = php_regex(r"/([^;]*)*(;|$)/");
    let captures = regex.captures("abc").unwrap();
    assert_eq!(captures.get(0).unwrap().as_str("abc"), "abc");
    assert_eq!(captures.get(1).unwrap().as_str("abc"), "");
    assert_eq!(captures.get(2).unwrap().as_str("abc"), "");
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
fn test_absolute_start_anchor_and_atomic_group() {
    let start = Regex::new(r"\A.", RegexFlags::default()).unwrap();
    assert!(start.captures("ab").is_some());

    let atomic = Regex::new(r"(?>ab|a)b", RegexFlags::default()).unwrap();
    assert!(atomic.captures("abb").is_some());
    assert!(atomic.captures("ab").is_none());
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

#[test]
fn php_delimiter_scanner_preserves_escaped_and_nested_boundaries() {
    let (pattern, _) = parse_php_regex(r"@\@\@@").unwrap();
    assert_eq!(pattern, r"\@\@");

    let (pattern, _) = parse_php_regex("{a{b}c}").unwrap();
    assert_eq!(pattern, "a{b}c");

    let (pattern, _) = parse_php_regex(r"{a\}b}").unwrap();
    assert_eq!(pattern, r"a\}b");

    assert_eq!(
        parse_php_regex("{").unwrap_err(),
        "No ending matching delimiter '}' found"
    );
}

#[test]
fn php_modifier_scanner_ignores_only_php_line_spacing() {
    let (_, flags) = parse_php_regex("/a/  S\r\n").unwrap();
    assert!(!flags.unicode_mode.utf());
    assert_eq!(
        parse_php_regex("/a/\t").unwrap_err(),
        "Unknown modifier '\t'"
    );
    assert_eq!(
        parse_php_regex("/a/\0i").unwrap_err(),
        "NUL byte is not a valid modifier"
    );
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
fn test_regex_cache_error_slot_preserves_capacity_and_entries() {
    let mut cache = RegexCache::new(2);
    cache.set_last_error(6);
    assert_eq!(cache.last_error(), 6);
    cache.get_or_compile("/first/").unwrap();
    cache.get_or_compile("/second/").unwrap();
    cache.get_or_compile("/third/").unwrap();
    assert_eq!(cache.last_error(), 6);
    assert_eq!(cache.entries.len(), 2);
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
fn replacement_backreferences_consume_at_most_two_digits() {
    let regex = php_regex("/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)/");
    assert_eq!(regex.replace_all("abcdefghijkl", "$103"), "j3l");
    assert_eq!(regex.replace_all("abcdefghijkl", r"\103"), "j3l");
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

#[test]
fn named_group_names_cannot_start_with_a_digit() {
    assert_eq!(
        Regex::new("(?P<3>)", RegexFlags::default()).unwrap_err(),
        "subpattern name must start with a non-digit at offset 4"
    );
    assert_eq!(
        Regex::new("(?<7>)", RegexFlags::default()).unwrap_err(),
        "subpattern name must start with a non-digit at offset 3"
    );
}

// ── P1: Unknown modifiers are rejected ─────────────────────────────────

#[test]
fn test_unknown_modifier_rejected() {
    assert!(parse_php_regex("/a/z").is_err());
}

#[test]
fn newer_pcre_modifier_is_unknown_at_the_reported_10_42_level() {
    assert_eq!(parse_php_regex("/a/r").unwrap_err(), "Unknown modifier 'r'");
    assert_eq!(
        parse_php_regex("/a/Jz").unwrap_err(),
        "Unknown modifier 'z'"
    );
}

#[test]
fn extra_modifier_is_ignored_and_no_auto_capture_changes_only_plain_groups() {
    let (pattern, flags) = parse_php_regex("/(a)/Xn").unwrap();
    let regex = Regex::new(&pattern, flags).unwrap();
    let captures = regex.captures("a").unwrap();
    assert_eq!(captures.len(), 1);

    let named = php_regex("/(?n)(a)(?<kept>b)(?-n:(c))/");
    let captures = named.captures("abc").unwrap();
    assert_eq!(captures.len(), 3);
    assert_eq!(captures.get(1).unwrap().as_str("abc"), "b");
    assert_eq!(captures.get_named("kept").unwrap().as_str("abc"), "b");
    assert_eq!(captures.get(2).unwrap().as_str("abc"), "c");
}

#[test]
fn extra_modifier_does_not_turn_unknown_letter_escapes_into_literals() {
    let (pattern, flags) = parse_php_regex(r"/\y/X").unwrap();
    assert_eq!(
        Regex::new(&pattern, flags).unwrap_err(),
        "unrecognized character follows \\ at offset 1"
    );
}

#[test]
fn octal_escapes_and_absolute_end_anchor_follow_pcre_spelling() {
    let regex = php_regex(r"/a\000b\z/");
    assert!(regex.is_match("a\0b"));
    assert!(!regex.is_match("a\0b\n"));

    let braced = php_regex(r"/[\o{77}]/");
    assert!(braced.is_match("?"));
}

#[test]
fn a_leading_non_quantifier_brace_is_literal() {
    let regex = php_regex(r"{{\D+}}");
    assert!(regex.is_match("{abcd}"));
}

#[test]
fn bounded_repetition_respects_the_pcre_compiled_program_limit() {
    let alternatives = (0..64)
        .map(|index| format!("token{index}"))
        .collect::<Vec<_>>()
        .join("|");
    let pattern = format!("(?:{alternatives}){{1,300}}");
    assert!(
        Regex::new(&pattern, RegexFlags::default())
            .unwrap_err()
            .starts_with("regular expression is too large at offset ")
    );
}

#[test]
fn pcre_10_42_compile_boundaries_reject_invalid_counts_names_and_references() {
    assert!(Regex::new("a{0,65535}", RegexFlags::default()).is_ok());
    assert!(
        Regex::new("^a{99$", RegexFlags::default())
            .unwrap()
            .is_match("a{99")
    );
    assert!(
        Regex::new("^a{99x}$", RegexFlags::default())
            .unwrap()
            .is_match("a{99x}")
    );
    assert_eq!(
        Regex::new("a{3,2}", RegexFlags::default()).unwrap_err(),
        "numbers out of order in {} quantifier at offset 5"
    );
    assert_eq!(
        Regex::new("a{0,65536}", RegexFlags::default()).unwrap_err(),
        "number too big in {} quantifier at offset 9"
    );

    let maximum_name = "a".repeat(32);
    assert!(Regex::new(&format!("(?<{maximum_name}>x)"), RegexFlags::default()).is_ok());
    let oversized_name = "a".repeat(33);
    assert!(
        Regex::new(&format!("(?<{oversized_name}>x)"), RegexFlags::default())
            .unwrap_err()
            .starts_with("subpattern name is too long (maximum 32 code units)")
    );

    assert!(
        Regex::new("(?|(?<x>a)|(?<y>b))", RegexFlags::default())
            .unwrap_err()
            .starts_with("different names for subpatterns of the same number")
    );
    for pattern in [r"\g{99}", r"(?99)", r"(?&missing)"] {
        assert!(
            Regex::new(pattern, RegexFlags::default())
                .unwrap_err()
                .starts_with("reference to non-existent subpattern")
        );
    }
}

#[test]
fn quantified_assertions_and_possessive_compounds_follow_pcre_backtracking() {
    assert!(php_regex(r"/^a\Eb$/").is_match("ab"));
    assert!(php_regex(r"/^(?=a)*a$/").is_match("a"));
    let captures = php_regex(r"/^(?=(a))+a$/").captures("a").unwrap();
    assert_eq!(captures.get(1).unwrap().as_str("a"), "a");
    assert!(php_regex(r"/^a(?<=a)*$/").is_match("a"));

    assert!(!php_regex(r"/^(a|ab)++c$/").is_match("abc"));
    assert!(
        Regex::new("[z-a]", RegexFlags::default())
            .unwrap_err()
            .starts_with("range out of order in character class")
    );

    assert!(php_regex("/(*CR)(?x)^a#comment\rb$/").is_match("ab"));
}

#[test]
fn recursion_conditions_prefer_existing_capture_names() {
    assert!(php_regex(r"/^(?<R>a)?(?(R)b|c)$/").is_match("ab"));
    assert!(php_regex(r"/^(?<R1>a)?(?(R1)b|c)$/").is_match("ab"));
    assert!(php_regex(r"/^(?<x>(?(R&x)a|b)(?&x)?)$/").is_match("ba"));
}

#[test]
fn search_start_anchor_is_fixed_during_bump_along_and_advances_after_a_match() {
    let anchor = php_regex(r"/\Gx/");
    assert!(!anchor.is_match("ax"));
    assert!(anchor.is_match("x"));

    let contiguous = php_regex(r"/\G./");
    assert_eq!(
        contiguous
            .captures_iter("abc")
            .iter()
            .map(|captures| captures.get(0).unwrap().as_str("abc"))
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
}

#[test]
fn duplicate_named_groups_follow_compile_match_and_alias_rules() {
    assert!(
        Regex::new("(?<g>a)(?<g>b)", RegexFlags::default())
            .unwrap_err()
            .contains("PCRE2_DUPNAMES not set")
    );

    let alternatives = php_regex("/(?J)(?:(?<g>a)|(?<g>b))/");
    let left = alternatives.captures("a").unwrap();
    assert_eq!(left.get_named("g").unwrap().as_str("a"), "a");
    assert_eq!(left.named_group_slot("g"), Some(1));
    let right = alternatives.captures("b").unwrap();
    assert_eq!(right.get_named("g").unwrap().as_str("b"), "b");
    assert_eq!(right.named_group_slot("g"), Some(2));

    // Named backreferences use the first participating physical group even
    // though the public alias projects the last participating group.
    let backreference = php_regex(r"/(?J)(?<g>a)(?<g>b)\k<g>/");
    assert!(backreference.is_match("aba"));
    assert!(!backreference.is_match("abb"));

    let modifier = php_regex("/(?<g>a)(?<g>b)/J");
    assert_eq!(
        modifier
            .captures("ab")
            .unwrap()
            .get_named("g")
            .unwrap()
            .as_str("ab"),
        "b"
    );
    assert!(php_regex("/(?J:(?<g>a)(?<g>b))/").is_match("ab"));
    assert!(Regex::new("(?J)(?<g>a)(?-J)(?<g>b)", RegexFlags::default()).is_err());
}

#[test]
fn reset_start_preserves_consumption_and_changes_only_the_full_match_span() {
    let reset = Regex::new(r".{3}\K", RegexFlags::default()).unwrap();
    let captures = reset.captures_iter("abcdefghijklm");
    assert_eq!(
        captures
            .iter()
            .map(|captures| {
                let full = captures.get(0).unwrap();
                (full.start, full.end)
            })
            .collect::<Vec<_>>(),
        vec![(3, 3), (6, 6), (9, 9), (12, 12)]
    );
    assert_eq!(reset.replace_all("abcdefghijklm", "|"), "abc|def|ghi|jkl|m");
    assert_eq!(
        reset.split("abcdefghijklm", 3),
        vec!["abc", "def", "ghijklm"]
    );

    let suffix = Regex::new(r"abc\Kdef", RegexFlags::default()).unwrap();
    let full = suffix.captures("abcdef").unwrap();
    assert_eq!(full.get(0).unwrap().as_str("abcdef"), "def");

    assert_eq!(
        Regex::new(r"(?=xyz\K)", RegexFlags::default()).unwrap_err(),
        r"\K is not allowed in lookarounds (but see PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK) at offset 9"
    );
    assert_eq!(
        Regex::new(r"(a(?=xyz\K))", RegexFlags::default()).unwrap_err(),
        r"\K is not allowed in lookarounds (but see PCRE2_EXTRA_ALLOW_LOOKAROUND_BSK) at offset 12"
    );
}

#[test]
fn global_empty_matches_retry_a_nonempty_alternative_before_advancing() {
    for pattern in [r"\K|.", "a*|b"] {
        let regex = Regex::new(pattern, RegexFlags::default()).unwrap();
        let matches = regex
            .captures_iter(if pattern == "a*|b" { "b" } else { "a" })
            .into_iter()
            .map(|captures| captures.get(0).unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(
            matches,
            [
                Match { start: 0, end: 0 },
                Match { start: 0, end: 1 },
                Match { start: 1, end: 1 },
            ]
        );
        assert_eq!(
            regex.replace_all(if pattern == "a*|b" { "b" } else { "a" }, "X"),
            "XXX"
        );
    }
    assert_eq!(
        Regex::new("a*", RegexFlags::default())
            .unwrap()
            .replace_all("b", "X"),
        "XbX"
    );
}

fn php_regex(pattern: &str) -> Regex {
    let (pattern, flags) = parse_php_regex(pattern).unwrap();
    Regex::new(&pattern, flags).unwrap()
}

#[test]
fn anchored_modifier_matches_only_at_the_search_position() {
    let anchored = php_regex("/a/A");
    assert!(anchored.is_match("ab"));
    assert!(!anchored.is_match("ba"));
    assert!(anchored.captures("ba").is_none());
    let words = php_regex("/\\w+|\\s+/A");
    assert_eq!(words.captures_iter("ab cd").len(), 3);
    let repeated = php_regex("/a/A");
    assert_eq!(repeated.captures_iter("aab").len(), 2);
    assert_eq!(
        repeated.replace_all_with("aab", |_, _| "x".to_string()),
        "xxb"
    );
    assert_eq!(
        repeated.replace_all_with("baa", |_, _| "x".to_string()),
        "baa"
    );
    assert_eq!(php_regex("/,/A").split(",,a,", -1), vec!["", "", "a,"]);
}

#[test]
fn unicode_properties_hex_escapes_and_grapheme_clusters() {
    let letters = php_regex("/\\p{L}+/u");
    assert_eq!(
        letters.captures("café 42").unwrap().groups[0]
            .as_ref()
            .map(|m| m.end),
        Some(5)
    );
    assert!(php_regex("/\\P{L}/u").is_match("1"));
    assert!(php_regex("/\\p{^L}/u").is_match("1"));
    assert!(!php_regex("/\\p{^L}/u").is_match("a"));
    assert!(php_regex("/[\\p{Mn}\\x{200D}]/u").is_match("\u{301}"));
    assert!(php_regex("/[\\x{1F1E6}-\\x{1F1FF}]{2}/u").is_match("\u{1F1E8}\u{1F1FF}"));
    assert!(php_regex("/\\x41\\x{42}/").is_match("AB"));
    assert!(php_regex("/^\\X$/u").is_match("e\u{301}"));
    assert_eq!(php_regex("/\\X/u").captures_iter("a\u{301}b\r\nc").len(), 4);
    let (pattern, flags) = parse_php_regex("/\\p{Nope}/u").unwrap();
    assert!(Regex::new(&pattern, flags).is_err());
}

#[test]
fn grapheme_clusters_follow_unicode_14_break_properties_and_emoji_rules() {
    let clusters = |subject: &str| {
        php_regex("/\\X/u")
            .captures_iter(subject)
            .into_iter()
            .map(|captures| captures.get(0).unwrap().as_str(subject).to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(clusters("👍🏽x"), ["👍🏽", "x"]);
    assert_eq!(clusters("\u{0600}Ax"), ["\u{0600}A", "x"]);
    assert_eq!(clusters("क\u{093E}x"), ["का", "x"]);
    assert_eq!(clusters("a\u{200D}😀"), ["a\u{200D}", "😀"]);
    assert_eq!(
        clusters("😀\u{FE0F}\u{200D}😀x"),
        ["😀\u{FE0F}\u{200D}😀", "x"]
    );
    assert_eq!(clusters("🇨🇿🇨"), ["🇨🇿", "🇨"]);
}

#[test]
fn test_php_rejects_alphanumeric_backslash_and_nul_delimiters() {
    for pattern in ["aba", "1b1", "\\b\\", "\0b\0"] {
        assert_eq!(
            parse_php_regex(pattern).unwrap_err(),
            "Delimiter must not be alphanumeric, backslash, or NUL byte"
        );
    }
}

#[test]
fn inline_and_scoped_options_follow_lexical_group_boundaries() {
    let scoped = php_regex("/^(?i:a|(?-i:B))$/");
    assert!(scoped.is_match("a"));
    assert!(scoped.is_match("A"));
    assert!(scoped.is_match("B"));
    assert!(!scoped.is_match("b"));

    assert!(php_regex("/^(?i)a(?-i)b$/").is_match("Ab"));
    assert!(!php_regex("/^(?i)a(?-i)b$/").is_match("AB"));
    assert!(php_regex("/^(a(?i)b)c$/").is_match("aBc"));
    assert!(!php_regex("/^(a(?i)b)c$/").is_match("aBC"));
    assert!(php_regex("/^(?i)a(?^)b$/").is_match("Ab"));
    assert!(php_regex("/^(?x) a \\x20 b $/").is_match("a b"));
    assert!(php_regex("/^(?xx:[a b]+)$/").is_match("ab"));

    let (pattern, flags) = parse_php_regex("/(?i:a/").unwrap();
    assert_eq!(
        Regex::new(&pattern, flags).unwrap_err(),
        "Unterminated scoped PCRE option group"
    );
}

#[test]
fn pcre_character_escapes_and_quoting_cover_the_documented_byte_surface() {
    assert!(php_regex("/^\\a\\e\\f\\cA$/").is_match("\u{7}\u{1b}\u{c}\u{1}"));
    assert!(php_regex("/^\\h+$/u").is_match("\t \u{a0}"));
    assert!(php_regex("/^\\v+$/u").is_match("\n\u{2028}"));
    assert!(php_regex("/^\\N+$/").is_match("abc"));
    assert!(!php_regex("/^\\N+$/").is_match("a\nb"));
    assert!(php_regex("/^\\N{U+41}$/u").is_match("A"));
    assert!(php_regex("/foo\\Z/").is_match("foo\n"));
    assert!(!php_regex("/foo\\Z/").is_match("foo\nx"));
    assert!(php_regex("/^\\Q.a+$\\E$/").is_match(".a+$"));
    assert!(php_regex("/^\\Qab\\E+$/").is_match("abb"));
    assert!(!php_regex("/^\\Qab\\E+$/").is_match("abab"));
    assert!(php_regex("/^[\\b]$/").is_match("\u{8}"));
    assert!(php_regex("/^[\\h\\v]+$/u").is_match(" \n"));

    let (pattern, flags) = parse_php_regex("/[\\B]/").unwrap();
    assert!(Regex::new(&pattern, flags).is_err());
}

#[test]
fn alternate_backreference_subroutine_comment_and_callout_spellings_work() {
    assert!(php_regex("/^(a)(b)\\g{1}\\g2$/").is_match("abab"));
    assert!(php_regex("/^(a)(b)\\g{-1}\\g{-2}$/").is_match("abba"));
    assert!(php_regex("/^(?<x>a)\\g{x}\\k{x}$/").is_match("aaa"));
    assert!(php_regex("/^(?<x>a)\\g<x>$/").is_match("aa"));
    assert!(php_regex("/^(a)(?-1)$/").is_match("aa"));
    assert!(php_regex("/^(?+1)(a)$/").is_match("aa"));
    assert!(php_regex("/^(?<x>a)(?P>x)$/").is_match("aa"));
    assert!(php_regex("/^a(?# ignored)b$/").is_match("ab"));
    assert!(php_regex("/^a(?C)b$/").is_match("ab"));
    assert!(php_regex("/^a(?C17)b$/").is_match("ab"));
    assert!(php_regex("/^a(?C'hello')b$/").is_match("ab"));
    assert!(php_regex("/^(a)\\11$/").is_match("a\t"));
}

#[test]
fn leading_directives_select_unicode_newline_bsr_and_resource_modes() {
    assert!(php_regex("/(*UTF)^é$/").is_match("é"));
    assert!(php_regex("/(*UCP)^\\w+$/").is_match("č"));
    assert!(php_regex("/(*CR)^b$/m").is_match("a\rb"));
    assert!(!php_regex("/(*BSR_ANYCRLF)^\\R$/u").is_match("\u{2028}"));
    assert!(php_regex("/(*BSR_UNICODE)^\\R$/u").is_match("\u{2028}"));

    // PHP accepts the PCRE2 NOTEMPTY start items but does not propagate them
    // to preg_match() as runtime options, so their observable PHP behavior is
    // deliberately a no-op.
    let captures = php_regex("/(*NOTEMPTY)a*/").captures("bbb").unwrap();
    assert_eq!(captures.get(0).unwrap().start, 0);
    assert_eq!(captures.get(0).unwrap().end, 0);
    assert!(php_regex("/(*NO_AUTO_POSSESS)^a+a$/").is_match("aa"));

    let limited = Regex::new("(*LIMIT_MATCH=1)^(a)+b$", RegexFlags::default()).unwrap();
    assert_eq!(
        limited.is_match_with_limits(
            "aaaaaaaa",
            MatchLimits {
                backtrack: 1_000,
                recursion: 1_000,
                heap_frames: usize::MAX,
                jit: false,
            },
        ),
        Err(MatchLimitError::Backtrack)
    );

    let heap_limited = Regex::new("(*LIMIT_HEAP=1)^(a|b)+$", RegexFlags::default()).unwrap();
    assert_eq!(
        heap_limited.is_match_with_limits("aaaaaaaaaa", MatchLimits::default()),
        Err(MatchLimitError::Heap)
    );
    assert_eq!(
        heap_limited.is_match_with_limits("aaa", MatchLimits::default()),
        Ok(true)
    );
}

#[test]
fn control_verbs_long_groups_and_extended_conditions_follow_pcre_order() {
    assert!(php_regex("/^a(*FAIL)b|ac$/").is_match("ac"));
    assert!(php_regex("/^a(*F)b|ac$/").is_match("ac"));
    let accepted = php_regex("/^a(*ACCEPT)b$/").captures("a").unwrap();
    assert_eq!(accepted.get(0).unwrap().as_str("a"), "a");
    assert!(!php_regex("/^a+(*COMMIT)b|aac$/").is_match("aac"));
    assert!(!php_regex("/a(*COMMIT)b|c/").is_match("ac"));
    assert!(!php_regex("/^a+(*PRUNE)b|aac$/").is_match("aac"));
    assert!(php_regex("/a(*PRUNE)b|c/").is_match("ac"));
    assert!(!php_regex("/a+(*SKIP)b|a/").is_match("aac"));
    assert!(php_regex("/^(?:a(*THEN)b|ac)$/").is_match("ac"));
    let named_skip = php_regex("/a(*MARK:X)b(*SKIP:X)c|b/")
        .captures("abx")
        .unwrap();
    assert_eq!(named_skip.get(0).unwrap().as_str("abx"), "b");
    assert_eq!(named_skip.get(0).unwrap().start, 1);
    assert!(php_regex("/a(*SKIP:X)b|a/").is_match("a"));

    let assertion_accept = php_regex("/^(?=a(*ACCEPT)b)a$/").captures("a").unwrap();
    assert_eq!(assertion_accept.get(0).unwrap().as_str("a"), "a");
    let group_accept = php_regex("/^((?:a(*ACCEPT)b)c)d$/").captures("a").unwrap();
    assert_eq!(group_accept.get(0).unwrap().as_str("a"), "a");
    assert_eq!(group_accept.get(1).unwrap().as_str("a"), "a");

    assert!(!php_regex("/^(*atomic:a+)ab$/").is_match("aaab"));
    assert!(php_regex("/^(*positive_lookahead:a)a$/").is_match("a"));
    assert!(php_regex("/^a(*positive_lookbehind:a)$/").is_match("a"));
    assert!(php_regex("/^(*script_run:\\w+)$/u").is_match("abc"));

    let subject = "word1 word2 word3 word2 word3 word4";
    let non_atomic = php_regex("/^(?x)(*napla: .* \\b(\\w++)) (?> .*? \\b\\1\\b ){2}/")
        .captures(subject)
        .unwrap();
    assert_eq!(non_atomic.get(1).unwrap().as_str(subject), "word3");
    assert!(php_regex("/^(?*.*\\b(\\w++))(?>.*?\\b\\1\\b){2}/").is_match(subject));

    assert!(php_regex("/^(?(?=a)a|b)$/").is_match("a"));
    assert!(php_regex("/^(a)?(?(-1)b|c)$/").is_match("ab"));
    assert!(php_regex("/^((?(R1)a|(?1)b))$/").is_match("ab"));
    assert!(php_regex("/^(?(VERSION>=10.42)yes|no)$/").is_match("yes"));

    assert_eq!(
        Regex::new("^(a)?(?(<-1>)b|c)$", RegexFlags::default()).unwrap_err(),
        "subpattern name expected at offset 9"
    );
    assert_eq!(
        Regex::new("(?(<missing>)a|b)", RegexFlags::default()).unwrap_err(),
        "reference to non-existent subpattern at offset 4"
    );
    assert!(Regex::new("(?(<later>)a|b)(?<later>x)", RegexFlags::default()).is_ok());
}

#[test]
fn malformed_star_groups_report_pcre_compile_diagnostics() {
    assert_eq!(
        Regex::new("(*FOO)a", RegexFlags::default()).unwrap_err(),
        "(*VERB) not recognized or malformed at offset 5"
    );
    assert_eq!(
        Regex::new("(*FOO:x)a", RegexFlags::default()).unwrap_err(),
        "(*VERB) not recognized or malformed at offset 5"
    );
    assert_eq!(
        Regex::new("(*MARK)a", RegexFlags::default()).unwrap_err(),
        "(*MARK) must have an argument at offset 6"
    );
    assert_eq!(
        Regex::new("(*atomic)a", RegexFlags::default()).unwrap_err(),
        "(*alpha_assertion) not recognized at offset 8"
    );
}

#[test]
fn lookbehind_compile_rules_match_pcre_10_42_fixed_length_contracts() {
    assert!(php_regex("/(?<=a|bc)x/").is_match("bcx"));
    assert!(Regex::new("(?<=(a|bc))x", RegexFlags::default()).is_err());
    assert!(Regex::new("(?<=a+)x", RegexFlags::default()).is_err());
    assert!(Regex::new("(?<=\\R)x", RegexFlags::default()).is_err());
    assert!(Regex::new("(?<=(a)\\1)x", RegexFlags::default()).is_ok());
}

#[test]
fn script_run_groups_reject_mixed_scripts_and_digit_sets() {
    let run = php_regex("/^(*sr:\\S+)$/u");
    assert!(run.is_match("paypal.com"));
    assert!(!run.is_match("paypаl.com"));
    assert!(run.is_match("漢ひカ"));
    assert!(run.is_match("漢한ㄅ"));
    assert!(!run.is_match("한ㄅ"));
    assert!(run.is_match("a\u{301}"));
    assert!(run.is_match("ا،"));
    assert!(run.is_match("ܐ،"));
    assert!(!run.is_match("a،"));
    assert!(!run.is_match("ܐ۔"));
    assert!(run.is_match("ا۔"));
    assert!(run.is_match("١٢"));
    assert!(!run.is_match("a1٢"));

    let atomic = php_regex("/^(*asr:\\S+)$/u");
    assert!(atomic.is_match("google.com"));
    assert!(!atomic.is_match("gооgle.com"));
}

#[test]
fn unicode_script_and_script_extension_properties_are_distinct() {
    assert!(!php_regex("/\\p{sc:Arabic}/u").is_match("،"));
    assert!(php_regex("/\\p{scx:Arabic}/u").is_match("،"));
    assert!(php_regex("/\\p{Arabic}/u").is_match("،"));
    assert!(php_regex("/\\p{Script=Latin}/u").is_match("A"));
    assert!(php_regex("/\\p{Script_Extensions=Latin}/u").is_match("A"));
    assert!(php_regex("/\\p{Xuc}/u").is_match("@"));
    assert!(!php_regex("/\\p{Xuc}/u").is_match("A"));
    assert!(php_regex("/\\p{Xuc}/u").is_match("é"));
    assert!(Regex::new("\\p{gc=Lu}", RegexFlags::default()).is_err());
}

#[test]
fn pcre_10_42_binary_property_inventory_is_compiled_from_unicode_14() {
    for property in unicode_binary_properties::BINARY_PROPERTY_NAMES {
        assert!(
            Regex::new(&format!("\\p{{{property}}}"), RegexFlags::default()).is_ok(),
            "binary property {property} must compile"
        );
    }
    for unsupported in ["CE", "CWKCF", "Hyphen", "OAlpha", "XO_NFC"] {
        assert!(Regex::new(&format!("\\p{{{unsupported}}}"), RegexFlags::default()).is_err());
    }

    assert!(php_regex("/^\\p{Alphabetic}+$/u").is_match("Žluťoučký"));
    assert!(php_regex("/^\\p{Bidi_C}$/u").is_match("\u{061c}"));
    assert!(php_regex("/^\\p{Emoji}$/u").is_match("😀"));
    assert!(php_regex("/^\\p{Pat_Syn}$/u").is_match("+"));
    assert!(php_regex("/^\\p{CWU}$/u").is_match("a"));
    assert!(php_regex("/^\\p{WSpace}$/u").is_match("\u{2003}"));

    // Kawi was assigned in Unicode 15.0. PCRE2 10.42 ships Unicode 14.0,
    // so both its script name and its later letter assignment are absent.
    assert!(Regex::new("\\p{Kawi}", RegexFlags::default()).is_err());
    assert!(!php_regex("/^\\p{L}$/u").is_match("\u{11f02}"));
}

#[test]
fn bidi_class_property_uses_the_pcre_short_class_inventory() {
    assert!(php_regex("/^\\p{Bidi_Class:R}$/u").is_match("א"));
    assert!(php_regex("/^\\p{BC=AL}$/u").is_match("ا"));
    assert!(php_regex("/^\\p{bc:L}$/u").is_match("A"));
    assert!(php_regex("/^\\p{bc:EN}$/u").is_match("1"));
    assert!(!php_regex("/^\\p{bc:R}$/u").is_match("A"));
    assert!(Regex::new("\\p{bc:Right_To_Left}", RegexFlags::default()).is_err());
}

#[test]
fn inline_ungreedy_options_shape_quantifiers_without_runtime_flags() {
    let re = Regex::new("(?U)<.*>", RegexFlags::default()).unwrap();
    assert_eq!(re.captures("<aa> <bb>").unwrap().get(0).unwrap().end, 4);

    let re = Regex::new("(?U)<.*?>", RegexFlags::default()).unwrap();
    assert_eq!(re.captures("<aa> <bb>").unwrap().get(0).unwrap().end, 9);

    let re = Regex::new("(?U:<.*>)-<.*>", RegexFlags::default()).unwrap();
    assert!(re.is_match("<a>-<b> <c>"));

    let mut flags = RegexFlags::default();
    flags.ungreedy = true;
    let re = Regex::new("(?-U)<.*>", flags).unwrap();
    assert_eq!(re.captures("<aa> <bb>").unwrap().get(0).unwrap().end, 9);
}

#[test]
fn nested_lazy_repetitions_deduplicate_paths_and_keep_the_last_capture() {
    let re = php_regex("/(['\"])((.*(\\\\\\1)*)*)\\1/U");
    let subject = "key='abc' tail";
    let captures = re.captures(subject).unwrap();
    assert_eq!(captures.get(0).unwrap().as_str(subject), "'abc'");
    assert_eq!(captures.get(2).unwrap().as_str(subject), "abc");
    assert_eq!(captures.get(3).unwrap().as_str(subject), "c");
    assert!(captures.get(4).is_none());
}

#[test]
fn capture_conditionals_select_the_participating_branch() {
    let re = Regex::new("^(a)?(?(1)b|c)$", RegexFlags::default()).unwrap();
    assert!(re.is_match("ab"));
    assert!(re.is_match("c"));
    assert!(!re.is_match("ac"));
    assert!(!re.is_match("b"));
}

#[test]
fn symbolic_subroutines_support_forward_definitions_and_recursion() {
    let re = Regex::new(
        r"(?(DEFINE)(?<value>\d+|(?&pair))(?<pair>\((?&value),(?&value)\)))(?&pair)",
        RegexFlags::default(),
    )
    .unwrap();
    assert!(re.is_match("(1,(2,3))"));
    assert!(!re.is_match("(1,(x,3))"));

    let left_recursive = Regex::new("((?1)?z)", RegexFlags::default()).unwrap();
    assert_eq!(
        left_recursive.is_match_with_limits("", MatchLimits::default()),
        Ok(false)
    );
}

#[test]
fn subroutine_calls_restore_the_callers_capture_registers() {
    let ordinary = php_regex("/^(?<x>a)(?&x)$/").captures("aa").unwrap();
    assert_eq!(ordinary.get(1).unwrap().as_str("aa"), "a");
    assert_eq!(ordinary.get(1).unwrap().start, 0);

    let define = php_regex("/^(?(DEFINE)(?<x>a))(?&x)$/")
        .captures("a")
        .unwrap();
    assert!(define.get(1).is_none());

    let nested = php_regex("/^((a))(?2)$/").captures("aa").unwrap();
    assert_eq!(nested.get(2).unwrap().start, 0);
}

#[test]
fn branching_quantifiers_try_the_preferred_partition_before_collecting_fallbacks() {
    let quoted = php_regex(r#"/^"([^"\\]*|\\.)*"$/"#);
    let subject = format!("\"{}\\\"tail\"", "value".repeat(128));
    assert!(quoted.is_match(&subject));

    // The preferred branch can still be rejected by the continuation; the
    // exhaustive fallback must then find a shorter inner alternative.
    assert!(php_regex("/^(?:ab|a)*b$/").is_match("ab"));
}

#[test]
fn nested_delimiter_unions_use_the_linear_reachability_path() {
    let re = Regex::new(
        r#"^\[((?:[^]]*|\[(?:[^]]*|\[[^]]*\])*\]|(?:[^{}]*|\{[^{}]*\})*)*)\]$"#,
        RegexFlags::default(),
    )
    .unwrap();
    let body = format!(
        "{}[inner]{{\"key\":\"value\"}}tail",
        "property=value,".repeat(256)
    );
    let subject = format!("[{body}]");
    let captures = re.captures(&subject).unwrap();
    assert_eq!(captures.get(1).unwrap().as_str(&subject), body);
}

#[test]
fn posix_classes_and_apostrophe_named_groups_follow_unicode_mode() {
    let hexadecimal = php_regex("/^[[:xdigit:]]+$/");
    assert!(hexadecimal.is_match("19aF"));
    assert!(!hexadecimal.is_match("19g"));

    let unicode_word = php_regex("/^[[:alpha:]][[:alnum:]_]*$/u");
    assert!(unicode_word.is_match("Žluťoučký2"));
    assert!(!unicode_word.is_match("2Žluťoučký"));
    assert!(php_regex("/^[[:^digit:]]+$/u").is_match("abc"));
    assert!(!php_regex("/^[[:^digit:]]+$/u").is_match("٣"));

    let named = php_regex("/^(?'word'[[:alpha:]]+)\\k'word'$/u");
    let captures = named.captures("ahaaha").unwrap();
    assert_eq!(captures.get_named("word").unwrap().as_str("ahaaha"), "aha");
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

// ── Subroutine calls to completed groups ─────────────────────────────────

#[test]
fn subroutine_calls_inline_completed_groups_without_publishing_captures() {
    let re = Regex::new("(?<a>x(?<b>y))(?&a)", RegexFlags::default()).unwrap();
    let caps = re.captures("xyxy").unwrap();
    assert_eq!(caps.get(0).unwrap().as_str("xyxy"), "xyxy");
    assert_eq!(caps.get_named("a").unwrap().as_str("xyxy"), "xy");
    assert_eq!(caps.get_named("b").unwrap().as_str("xyxy"), "y");
    for (pattern, subject) in [
        ("(x)(?1)", "xx"),
        ("(?<a>x)(?P>a)", "xx"),
        ("(?<a>x)\\g<a>", "xx"),
        ("(?<a>x)\\g'a'", "xx"),
        ("(a)(b)(?-2)(?-1)", "abab"),
        ("(?<n>[0-9]+)-(?&n)", "12-345"),
    ] {
        assert!(
            Regex::new(pattern, RegexFlags::default())
                .unwrap()
                .is_match(subject),
            "{pattern}"
        );
    }
    assert!(
        !Regex::new("(?<n>[0-9]+)-(?&n)", RegexFlags::default())
            .unwrap()
            .is_match("12-x")
    );
}

#[test]
fn braced_and_bare_g_escapes_are_backreferences() {
    for pattern in ["(a)\\g{1}", "(a)\\g1", "(a)\\g{-1}", "(?<q>a)\\g{q}"] {
        let re = Regex::new(pattern, RegexFlags::default()).unwrap();
        assert!(re.is_match("aa"), "{pattern}");
        assert!(!re.is_match("ab"), "{pattern}");
    }
}

// ── Bounded lookbehind window ────────────────────────────────────────────

#[test]
fn lookbehind_only_tries_starts_within_its_length_range() {
    let flags = RegexFlags::default();
    let mixed = Regex::new("(?<=ab|cd)x", flags).unwrap();
    assert!(mixed.is_match("abx"));
    assert!(mixed.is_match("cdx"));
    assert!(!mixed.is_match("bx"));
    let negative = Regex::new("(?<![\"'])[:-]\\w", flags).unwrap();
    assert!(negative.is_match("key:v"));
    assert!(!negative.is_match("\"key\":v".trim_start_matches("\"key")));
    assert!(!Regex::new("(?<!\"):x", flags).unwrap().is_match("\":x"));
    // A lookbehind at the subject start sees nothing before it.
    assert!(!Regex::new("(?<=a)x", flags).unwrap().is_match("x"));
    assert!(Regex::new("(?<!a)x", flags).unwrap().is_match("x"));
    // Fixed quantified and grouped bodies retain the bounded window.
    assert!(Regex::new("(?<=a{3})x", flags).unwrap().is_match("aaax"));
    assert!(!Regex::new("(?<=a{3})x", flags).unwrap().is_match("aax"));
    assert!(
        Regex::new("(?<=(?:ab){2})x", flags)
            .unwrap()
            .is_match("ababx")
    );
}

#[test]
fn lookbehind_cost_does_not_grow_with_subject_position() {
    // 20,000 tokens with a lookbehind at each: quadratic scanning would take
    // seconds, the bounded window finishes far below the assertion budget.
    let subject = "key:v ".repeat(20_000);
    let re = Regex::new("(?<![\"'])[:-][^\\s]", RegexFlags::default()).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(re.captures_iter(&subject).len(), 20_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn execution_limits_stop_quantified_and_nested_paths_without_stack_growth() {
    let long_linear = Regex::new(&".".repeat(4_097), RegexFlags::default()).unwrap();
    assert!(long_linear.is_match(&"x".repeat(4_097)));

    let repeated = Regex::new("^(a)+b$", RegexFlags::default()).unwrap();
    assert_eq!(
        repeated.is_match_with_limits(
            &"a".repeat(64),
            MatchLimits {
                backtrack: 8,
                recursion: 100,
                heap_frames: usize::MAX,
                jit: false,
            },
        ),
        Err(MatchLimitError::Backtrack)
    );

    let branching = Regex::new(r"^(?:\D+|<\d+>)*[!?]$", RegexFlags::default()).unwrap();
    assert_eq!(
        branching.is_match_with_limits(
            "foobar foobar foobar",
            MatchLimits {
                backtrack: 1,
                recursion: 100,
                heap_frames: usize::MAX,
                jit: false,
            },
        ),
        Err(MatchLimitError::Backtrack)
    );

    let nested = Regex::new("^((a))$", RegexFlags::default()).unwrap();
    assert!(matches!(
        nested.captures_with_limits(
            "a",
            MatchLimits {
                backtrack: 100,
                recursion: 1,
                heap_frames: usize::MAX,
                jit: false,
            },
        ),
        Err(MatchLimitError::Recursion)
    ));

    let jit_repeated = Regex::new("^(a)+$", RegexFlags::default()).unwrap();
    assert_eq!(
        jit_repeated.is_match_with_limits(
            &"a".repeat(MatchBudget::JIT_STEP_LIMIT + 1),
            MatchLimits {
                backtrack: 1_000_000,
                recursion: 100,
                heap_frames: usize::MAX,
                jit: true,
            },
        ),
        Err(MatchLimitError::JitStack)
    );
}
