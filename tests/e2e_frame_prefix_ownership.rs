mod common;

use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::instruction::RELEASE_TEMPS_NESTED_OBJECTS;
use rphp::vm::opcode::OpCode;

fn check_retirement(wide: bool, after_padding: bool, count: usize, throws: bool) {
    let mut source = String::from(
        "<?php class SlotLeaf { function __construct(public $id) {} \
         function __destruct() { echo 'd', $this->id, '|'; \
         if ($this->id === 0) { $GLOBALS['observed'] += 1; }",
    );
    if throws {
        source.push_str("if ($this->id === 2) { throw new Exception('temporary'); }");
    }
    source.push_str("} } function retireSlots($shared) {");
    // Reuse one PHP local: enlarge only the TMP envelope, so an early
    // statement still lies inside the bitmap prefix of a wide frame.
    let padding = "$scratch = $scratch + 1;".repeat(if wide { 80 } else { 0 });
    source.push_str("$scratch = 0;");
    if after_padding {
        source.push_str(&padding);
    }
    source.push_str("in_array(new SlotLeaf(-1), [");
    for index in 0..count {
        source.push_str(&format!("new SlotLeaf({index}),"));
    }
    source.push_str("$shared, $shared], true); ");
    if !after_padding {
        source.push_str(&padding);
    }
    source.push_str(
        "echo 'body|'; } \
         $GLOBALS['observed'] = 0; $kept = new SlotLeaf(1000); \
         try { retireSlots($kept); } \
         catch (Exception $e) { echo $e->getMessage(), '|'; } \
         echo 'after:', $GLOBALS['observed'], ':', $kept->id, '|'; unset($kept);",
    );

    // Check that the behavioral fixture really crosses the ownership metadata
    // boundaries; otherwise a compiler change could silently remove coverage.
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let (_, function) = compiled
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retireSlots"))
        .unwrap();
    let op = &function.op_array;
    assert_eq!(op.num_cvs + op.num_temps > 64, wide);
    let range = op
        .instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == OpCode::ReleaseTemps
                && instruction._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0
        })
        .max_by_key(|instruction| instruction.op2 - instruction.op1)
        .unwrap();
    if wide && !after_padding && count < 64 {
        assert!(range.op2 <= 64, "wide-frame release must fit the prefix");
    }
    if after_padding {
        assert!(range.op1 >= 64, "release must lie in the tail");
    }
    if count > 64 {
        assert!(range.op1 < 64 && range.op2 > 64);
    }
    let width = op
        .instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == OpCode::ReleaseTemps
                && instruction._pad & RELEASE_TEMPS_NESTED_OBJECTS != 0
        })
        .map(|instruction| instruction.op2 - instruction.op1)
        .max()
        .unwrap();
    assert_eq!(width > 64, count > 64);

    // Reference PHP retires the needle and each final nested owner in order,
    // including siblings after a throwing destructor. The shared owner stays
    // alive through the callback and retires only at the final explicit unset.
    let mut expected = String::from("d-1|");
    for index in 0..count {
        expected.push_str(&format!("d{index}|"));
    }
    expected.push_str(if throws { "temporary|" } else { "body|" });
    expected.push_str("after:1:1000|d1000|");
    assert_eq!(common::run_php(&source), expected);
}

#[test]
fn compact_statement_keeps_external_aliases_and_callback_order() {
    check_retirement(false, false, 3, false);
}

#[test]
fn wide_frame_short_statement_keeps_external_aliases_and_callback_order() {
    check_retirement(true, false, 3, false);
}

#[test]
fn long_statement_retires_prefix_and_tail_in_order() {
    check_retirement(true, false, 75, false);
}

#[test]
fn long_statement_exception_keeps_sibling_and_external_lifetimes() {
    check_retirement(true, false, 75, true);
}

#[test]
fn untracked_tail_keeps_external_aliases_and_callback_order() {
    check_retirement(true, true, 3, false);
}

#[test]
fn surplus_arguments_keep_static_scope_separate_from_owned_slots() {
    for count in [2, 40, 80] {
        for argument in ["3", "'owned'"] {
            let arguments = vec![argument; count].join(",");
            let source = format!(
                "<?php
                class ScopePrefixBase {{
                    public static $value = 11;
                    public static function read() {{ return static::$value; }}
                    public static function write($x) {{
                        static::$value = $x;
                        return static::$value;
                    }}
                }}
                class ScopePrefixChild extends ScopePrefixBase {{
                    public static $value = 17;
                }}
                for ($i = 0; $i < 2; $i++) {{
                    echo ScopePrefixChild::read({arguments}), '|';
                    echo ScopePrefixChild::write(23, {arguments}), '|';
                    echo ScopePrefixBase::$value, '|', ScopePrefixChild::$value, '|';
                }}"
            );
            assert_eq!(common::run_php(&source), "17|23|11|23|23|23|11|23|");
        }
    }
}
