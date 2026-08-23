#![cfg(feature = "formatted-io")]

mod common;

use common::run_php;

#[test]
fn formatted_stream_writes_share_flags_widths_and_return_lengths() {
    assert_eq!(
        run_php(
            r#"<?php
$stream = fopen('php://memory', 'w+');
echo fprintf($stream, '[%07.2f][%+d][%4s]', -1.0, 4, 'xy'), ':';
echo vfprintf($stream, '[%E][%X]', [1000, 255]), ':';
rewind($stream);
echo stream_get_contents($stream);
"#,
        ),
        "19:17:[-001.00][+4][  xy][1.000000E+3][FF]"
    );
}

#[test]
fn sscanf_returns_typed_fields_suppression_scansets_and_positions() {
    assert_eq!(
        run_php(
            r#"<?php
$values = sscanf('id=2a name=codex!', 'id=%x name=%[a-z]%c');
echo $values[0], ':', $values[1], ':', $values[2], "\n";
$values = sscanf('one two 17', '%2$s %1$s %*d');
echo $values[0], ':', $values[1];
"#,
        ),
        "42:codex:!\ntwo:one"
    );
}

#[test]
fn scanf_variadic_references_write_all_successes_and_preserve_failures() {
    assert_eq!(
        run_php(
            r#"<?php
$first = 'old-first'; $second = 'old-second'; $third = 'old-third';
echo sscanf('12 word', '%d %s%d', $first, $second, $third), ':';
echo $first, ':', $second, ':', $third, "\n";
try { sscanf('12 word', '%d %s', $first); }
catch (ValueError $error) { echo $error->getMessage(); }
"#,
        ),
        "2:12:word:old-third\nDifferent numbers of variable names and field specifiers"
    );
}

#[test]
fn fscanf_consumes_physical_lines_handles_blank_input_and_reaches_eof() {
    assert_eq!(
        run_php(
            r#"<?php
$stream = fopen('php://memory', 'w+');
fwrite($stream, "7 seven\n\nff last\n"); rewind($stream);
$values = fscanf($stream, '%d %s'); echo $values[0], ':', $values[1], "\n";
var_dump(fscanf($stream, '%s'));
$hex = 0; $word = '';
echo fscanf($stream, '%x %s', $hex, $word), ':', $hex, ':', $word, "\n";
var_dump(fscanf($stream, '%s'));
"#,
        ),
        "7:seven\nNULL\n2:255:last\nbool(false)\n"
    );
}

#[test]
fn argument_count_errors_remain_catchable_as_type_errors() {
    assert_eq!(
        run_php(
            r#"<?php
try { fprintf(); }
catch (TypeError $error) { echo get_class($error), ':caught'; }
"#,
        ),
        "ArgumentCountError:caught"
    );
}
