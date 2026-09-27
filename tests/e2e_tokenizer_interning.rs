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
