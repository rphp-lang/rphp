mod common;

use common::run_php;
use rphp::lexer::Lexer;
use rphp::parser::Parser;

#[test]
fn reserved_members_keep_their_spelling_and_work_in_chains() {
    assert_eq!(
        run_php(
            r#"<?php
class KeywordBox {
    const return = 'r';
    const While = 'w';
    const ReTuRn = 17;
    const RETURN = 19;
    public static function eCho() { return self::return . self::While; }
    public static function IF() { return self::ReTuRn + self::RETURN; }
}
class RouteBox {
    const use = 37;
    public static function new() { return new self; }
    public function end() { return $this; }
}
echo KeywordBox::eCho(), "\n", KeywordBox::IF(), "\n";
echo RouteBox::new()->end()::use, "\n";
"#
        ),
        "rw\n36\n37\n"
    );
}

#[test]
fn global_names_and_parameter_boundaries_use_php_token_descriptions() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'const return = 37;',
    'const ReTuRn = 37;',
    'function switch() {}',
    'function parameter_slot($item extra) {}',
    'function parameter_operator($item /) {}',
    'strlen(16 23);',
    'global $$registry->entry;',
] as $source) {
    try { eval($source); } catch (ParseError $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        concat!(
            "syntax error, unexpected token \"return\", expecting identifier\n",
            "syntax error, unexpected token \"return\", expecting identifier\n",
            "syntax error, unexpected token \"switch\", expecting \"(\"\n",
            "syntax error, unexpected identifier \"extra\", expecting \")\"\n",
            "syntax error, unexpected token \"/\", expecting \")\"\n",
            "syntax error, unexpected integer \"23\", expecting \")\"\n",
            "syntax error, unexpected token \"->\", expecting \",\" or \";\"\n",
        )
    );
}

#[test]
fn statement_keywords_do_not_become_array_constants() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['$values = [1, FOREACH];', '$values = [1, foreach];', '$values = [2, WHILE];'] as $source) {
    try { eval($source); } catch (ParseError $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        concat!(
            "syntax error, unexpected token \"foreach\", expecting \"]\"\n",
            "syntax error, unexpected token \"foreach\", expecting \"]\"\n",
            "syntax error, unexpected token \"while\", expecting \"]\"\n",
        )
    );
}

#[test]
fn malformed_sigils_retain_raw_quotes_escapes_and_numeric_spelling() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['$"report: \\n";', "$'fixed';", '$123;', '$ /* label */ entry;'] as $source) {
    try { eval($source); } catch (ParseError $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        concat!(
            "syntax error, unexpected double-quoted string \"report: \\n\", expecting variable or \"{\" or \"$\"\n",
            "syntax error, unexpected single-quoted string \"fixed\", expecting variable or \"{\" or \"$\"\n",
            "syntax error, unexpected integer \"123\", expecting variable or \"{\" or \"$\"\n",
            "syntax error, unexpected identifier \"entry\", expecting variable or \"{\" or \"$\"\n",
        )
    );
}

#[test]
fn token_previews_only_shorten_content_longer_than_the_preview_with_ellipsis() {
    let actual = run_php(
        r#"<?php
foreach ([30, 31, 33, 34] as $length) {
    try { eval('$"' . str_repeat('p', $length) . '";'); }
    catch (ParseError $error) { echo $error->getMessage(), "\n"; }
}
"#,
    );
    let mut expected = String::new();
    for length in [30, 31, 33, 34] {
        let preview = if length > 33 {
            format!("{}...", "p".repeat(30))
        } else {
            "p".repeat(length)
        };
        expected.push_str(&format!("syntax error, unexpected double-quoted string \"{preview}\", expecting variable or \"{{\" or \"$\"\n"));
    }
    assert_eq!(actual, expected);
}

#[test]
fn indirect_variables_and_qualified_reserved_segments_remain_valid() {
    assert_eq!(
        run_php(
            r#"<?php
namespace While\Loop;
class Entry {}
echo (new Entry)::class, "\n";
$key = 'slot'; $slot = 71;
echo $$key, "\n", ${'slot'}, "\n";
"#
        ),
        "While\\Loop\\Entry\n71\n71\n"
    );
}

#[test]
fn namespace_marker_errors_preserve_the_argument_grammar_context() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['new namespace::$target;', 'new namespace;', 'strlen(new namespace::$target);'] as $source) {
    try { eval($source); } catch (ParseError $error) { echo $error->getMessage(), "\n"; }
}
"#
        ),
        concat!(
            "syntax error, unexpected token \"namespace\", expecting \"class\"\n",
            "syntax error, unexpected token \"namespace\", expecting \"class\"\n",
            "syntax error, unexpected token \"namespace\", expecting \":\"\n",
        )
    );
}

#[test]
fn malformed_source_cannot_execute_earlier_statements() {
    assert_eq!(
        run_php(
            r#"<?php
try { eval('echo "must-not-run"; function signature($entry /) {}'); }
catch (ParseError $error) { echo "caught\n"; }
echo "continued\n";
"#
        ),
        "caught\ncontinued\n"
    );
}

#[test]
fn diagnostic_source_locations_follow_the_offending_token() {
    for (source, expected) in [
        (
            "<?php\nfunction signature($entry\nextra) {}",
            "syntax error, unexpected identifier \"extra\", expecting \")\" in /virtual/token-errors.php on line 3",
        ),
        (
            "<?php\n$\n/* context */ 'text';",
            "syntax error, unexpected single-quoted string \"text\", expecting variable or \"{\" or \"$\" in /virtual/token-errors.php on line 3",
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let actual = Parser::new(tokens)
            .with_source_name("/virtual/token-errors.php")
            .parse()
            .unwrap_err();
        assert_eq!(actual, expected);
    }
}
