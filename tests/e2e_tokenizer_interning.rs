mod common;

use common::run_php;

/// `token_get_all()` shares one string per repeated short spelling within a
/// call. PHP strings are values, so mutating one token's text, byte-level
/// edits, binary (non-UTF-8) texts and `PhpToken::tokenize()` all behave as
/// if every text were built separately.
#[test]
fn interned_token_texts_behave_as_independent_values() {
    assert_eq!(
        run_php(
            r#"<?php
$src = "<?php\nfunction foo(\$bar, \$bar2) {\n    return \$bar . \$bar . foo(\$bar2, \$bar);\n}\n    \$x = 'lit'; \$y = 'lit';\n";
$t = token_get_all($src);
$t[3][1] .= '_changed'; $t[7][1][0] = 'X';
echo json_encode(array_map(fn($k) => is_array($k) ? [$k[0], $k[1], $k[2]] : $k, $t)), "\n";
$u = token_get_all($src, TOKEN_PARSE);
$names = []; foreach ($u as $tok) { if (is_array($tok) && $tok[0] === T_VARIABLE) { $names[] = $tok[1]; } }
echo implode(',', $names), ' ', count(array_unique($names, SORT_STRING)), "\n";
$w = token_get_all("<?php \$a = \"\xff\"; \$b = \"\xff\";"); echo strlen($w[5][1]), bin2hex($w[5][1]), ' ', $w[5][1] === $w[12][1] ? 'eq' : 'ne', "\n";
$p = PhpToken::tokenize($src); $p[3]->text .= '!'; echo $p[3]->text, '|', $p[9]->text, '|', count($p), "\n";
echo md5(serialize(token_get_all(str_repeat("<?php echo \$longVariableNameThatRepeats_0123456789_abcdefghij_klmnopqrstuvwxyz;\n", 3)))), "\n";
"#
        ),
        r#"[[394,"<?php\n",1],[310,"function",2],[397," ",2],[262,"foo_changed",2],"(",[266,"$bar",2],",",[397,"X",2],[266,"$bar2",2],")",[397," ",2],"{",[397,"\n    ",2],[313,"return",3],[397," ",3],[266,"$bar",3],[397," ",3],".",[397," ",3],[266,"$bar",3],[397," ",3],".",[397," ",3],[262,"foo",3],"(",[266,"$bar2",3],",",[397," ",3],[266,"$bar",3],")",";",[397,"\n",3],"}",[397,"\n    ",4],[266,"$x",5],[397," ",5],"=",[397," ",5],[269,"'lit'",5],";",[397," ",5],[266,"$y",5],[397," ",5],"=",[397," ",5],[269,"'lit'",5],";",[397,"\n",5]]
$bar,$bar2,$bar,$bar,$bar2,$bar,$x,$y 4
322ff22 eq
foo!|)|48
edbca4ac5af20524213af15444f7cfbf
"#
    );
}

/// `token_get_all(..., TOKEN_PARSE)` skips its syntax check for a source the
/// request already parsed cleanly (an included file or an earlier call) and
/// still raises ParseError for sources it has not, repeatedly.
#[test]
fn token_parse_reuses_the_request_parse_verdict() {
    assert_eq!(
        run_php(
            r#"<?php
$dir = sys_get_temp_dir() . '/rphp-tokparse-' . getmypid(); @mkdir($dir);
file_put_contents("$dir/good.php", "<?php\nnamespace Demo;\nuse Foo\\Bar as Baz;\nclass Good { public function f(int \$x): int { return \$x + 1; } }\n");
file_put_contents("$dir/bad.php", "<?php\nnamespace Demo;\nclass Bad { public function f( { } }\n");
require "$dir/good.php";
$t = token_get_all(file_get_contents("$dir/good.php"), TOKEN_PARSE);
echo count($t), ' ', token_name($t[2][0]), ' ', $t[8][1], "\n";
try { token_get_all(file_get_contents("$dir/bad.php"), TOKEN_PARSE); echo "no error\n"; } catch (ParseError $e) { echo 'ParseError: ', (str_starts_with($e->getMessage(), 'syntax error, unexpected token "{"') ? 'syntax error at {' : $e->getMessage()), "\n"; }
try { token_get_all(file_get_contents("$dir/bad.php"), TOKEN_PARSE); echo "no error\n"; } catch (ParseError $e) { echo 'again: ', (str_starts_with($e->getMessage(), 'syntax error, unexpected token "{"') ? 'syntax error at {' : $e->getMessage()), "\n"; }
echo count(token_get_all(file_get_contents("$dir/bad.php"))), "\n";
$t2 = token_get_all(file_get_contents("$dir/good.php"), TOKEN_PARSE); echo $t2 === $t ? "same\n" : "differ\n";
try { token_get_all("<?php class { }", TOKEN_PARSE); echo "no error\n"; } catch (ParseError $e) { echo 'inline: ', get_class($e), "\n"; }
echo token_name(token_get_all("<?php echo Demo\\Good::class;", TOKEN_PARSE)[3][0]), "\n";
unlink("$dir/good.php"); unlink("$dir/bad.php"); rmdir($dir);
"#
        ),
        r#"50 T_WHITESPACE Foo\Bar
ParseError: syntax error at {
again: syntax error at {
25
same
inline: ParseError
T_NAME_QUALIFIED
"#
    );
}
