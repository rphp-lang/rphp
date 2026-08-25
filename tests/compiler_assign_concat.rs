mod common;

use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

#[test]
fn direct_compound_string_append_uses_the_cow_opcode() {
    let source = r#"<?php $rows = [['a']]; $text = ''; foreach ($rows AS $row) { $text .= implode(',', $row) . 'x'; }"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();

    assert!(
        compiled
            .main
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::AssignConcat)
    );
}

#[test]
fn optimized_compound_append_snapshots_rhs_before_an_undefined_target_handler() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 'left';
$y = 'source';
set_error_handler(function (int $level, string $message): bool {
    echo "diag=$message\n";
    $GLOBALS['x'] = 'handler';
    $GLOBALS['y'] = 'changed';
    return true;
});
function rhs(): string {
    unset($GLOBALS['x']);
    return $GLOBALS['y'];
}
$x .= rhs();
echo "result=$x/", $y, "\n";
$fresh .= 'a';
echo "fresh=$fresh\n";
$bytes = '';
for ($index = 0; $index < 512; $index++) {
    $bytes .= chr($index % 256);
}
echo 'bytes=', strlen($bytes), '/', sha1($bytes), '/',
    $bytes === stripslashes(addslashes($bytes)) ? 'roundtrip' : 'changed', "\n";
"#,
        ),
        "diag=Undefined variable $x\nresult=source/changed\ndiag=Undefined variable $fresh\nfresh=a\nbytes=512/dbe649daba340bce7a44b809016d914839b99f10/roundtrip\n",
    );
}
