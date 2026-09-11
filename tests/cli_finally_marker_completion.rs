#[test]
fn completion_markers_preserve_nested_effects_and_frame_retirement() {
    let expected = concat!(
        "7:abcdeg\nold:abcdeg\n12:abcdefg\n",
        "loop:01\nfinally;destructor;retired\n",
        "called;6:i\n",
    );
    for disable_jit in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rphp"));
        command.arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/finally_marker_completion.php"
        ));
        if disable_jit {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes());
    }
}

#[test]
fn compiled_finally_ranges_always_end_at_completion_opcodes() {
    use rphp::compiler::compile::Compiler;
    use rphp::lexer::Lexer;
    use rphp::parser::Parser;
    use rphp::vm::instruction::JMP_FLAG_FINALLY_END;
    use rphp::vm::opcode::OpCode;
    let source = include_str!("fixtures/finally_marker_completion.php");
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let mut count = 0;
    for op_array in std::iter::once(&compiled.main).chain(
        compiled
            .functions
            .iter()
            .map(|(_, function)| &function.op_array),
    ) {
        for entry in &op_array.try_entries {
            if entry.finally_start == u32::MAX {
                continue;
            }
            let instruction = &op_array.instructions[entry.finally_end as usize];
            assert_eq!(instruction.opcode, OpCode::JmpFinally);
            assert_ne!(instruction._pad & JMP_FLAG_FINALLY_END, 0);
            count += 1;
        }
    }
    assert!(
        count >= 8,
        "exercise main, nested, return and generator ranges"
    );
}
