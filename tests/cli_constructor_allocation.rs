use std::process::Command;

fn contract(name: &str) {
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/constructor_allocation/expected.json"
    ))
    .unwrap();
    for disabled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rphp"));
        command
            .args(["-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/constructor_allocation/contract.php"
            ))
            .env("RPHP_CONSTRUCTOR_ALLOCATION_CASE", name)
            .env_remove("RPHP_DISABLE_JIT");
        if disabled {
            command.env("RPHP_DISABLE_JIT", "1");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(output.stderr.is_empty(), "{name}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected[name].as_str().unwrap(),
            "{name} JIT disabled={disabled}"
        );
    }
}
macro_rules! specimen {
    ($name:ident) => {
        #[test]
        fn $name() {
            contract(stringify!($name));
        }
    };
}
specimen!(nested);
specimen!(argument_callbacks);
specimen!(dynamic_owner);
specimen!(anonymous_owner);
specimen!(no_constructor);
specimen!(unpacked);
specimen!(named_references);
specimen!(unpacked_references);
specimen!(defaults);
specimen!(validation_priority);
specimen!(autoload_order);
specimen!(argument_throw);
specimen!(constructor_throw);
specimen!(no_constructor_throw);
specimen!(reused_handles);
specimen!(suspended_argument);
specimen!(collect_during_argument);
specimen!(diagnostic_allocation);
specimen!(argument_snapshot);

fn pipeline_op_array(source: &str) -> rphp::compiler::OpArray {
    use rphp::{compiler::compile::Compiler, lexer::Lexer, parser::Parser};
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    compilation
        .functions
        .into_iter()
        .find(|(name, _)| name == "runQuotePipeline")
        .unwrap()
        .1
        .op_array
}

#[test]
fn prepared_pipeline_resumes_before_argument_evaluation() {
    use rphp::vm::{instruction::NEW_FLAG_PREPARE_ONLY, opcode::OpCode, quick};
    let op_array = pipeline_op_array(include_str!("../benches/corpus_order_pipeline.php"));
    let (backedge, jump) = op_array
        .instructions
        .iter()
        .enumerate()
        .find(|(ip, entry)| {
            matches!(entry.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                && (entry.op1 as usize) < *ip
        })
        .unwrap();
    let plan = quick::detect_long_ops_loop(&op_array, jump.op1 as usize, backedge).unwrap();
    let resume = plan
        .ops
        .iter()
        .find_map(|operation| {
            if let quick::QuickLongOp::VirtualObjectArrayPipeline { resume_ip, .. } = operation {
                Some(*resume_ip)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(
        op_array.instructions[resume]._pad & NEW_FLAG_PREPARE_ONLY,
        0
    );
    assert!(quick::detect_virtual_object_array_pipeline_span(&op_array, resume).is_some());
}

#[test]
fn observable_or_fallible_constructor_prefixes_stay_canonical() {
    use rphp::vm::quick::detect_virtual_object_array_pipeline_span;
    let source = include_str!("../benches/corpus_order_pipeline.php");
    for replacement in [
        "$i % 0",
        "$i % -1",
        "($subtotal = 7)",
        "argumentCallback($i)",
    ] {
        let op_array = pipeline_op_array(&source.replace("$i % 5", replacement));
        assert!(
            (0..op_array.instructions.len())
                .all(|ip| { detect_virtual_object_array_pipeline_span(&op_array, ip).is_none() }),
            "{replacement}"
        );
    }
}

#[test]
fn prepared_argument_snapshot_cannot_escape_virtual_region() {
    use rphp::vm::{
        instruction::{Instruction, NEW_FLAG_PREPARE_ONLY, OpType},
        opcode::OpCode,
        quick,
    };
    let mut op_array = pipeline_op_array(include_str!("../benches/corpus_order_pipeline.php"));
    let prepare = (0..op_array.instructions.len())
        .find(|&ip| {
            op_array.instructions[ip]._pad & NEW_FLAG_PREPARE_ONLY != 0
                && quick::detect_virtual_object_array_pipeline_span(&op_array, ip).is_some()
        })
        .unwrap();
    let snapshot = op_array.instructions[prepare + 1];
    assert_eq!(snapshot.opcode, OpCode::FetchCvR);
    let mut observation = Instruction::new(OpCode::Echo);
    observation.op1 = snapshot.result;
    observation.op1_type = OpType::Tmp;
    op_array.instructions.push(observation);
    assert!(quick::detect_virtual_object_array_pipeline_span(&op_array, prepare).is_none());
}
