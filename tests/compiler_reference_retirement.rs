use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::instruction::{OpType, RELEASE_TEMPS_INTERNAL_CVS};
use rphp::vm::opcode::OpCode;

#[test]
fn completed_reference_ranges_contain_only_private_cvs() {
    let source = "<?php $tree=['branch'=>[]];$tree['branch']['loop'] =& $tree['branch'];$aliases=[&$tree['branch']];unset($tree,$aliases);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let op = compiled.main;
    let private: Vec<_> = op
        .all_cvs
        .iter()
        .filter(|(_, name)| name.starts_with('\0'))
        .map(|(index, _)| *index as u16)
        .collect();
    assert!(!private.is_empty());
    for cv in &private {
        assert_eq!(
            op.instructions
                .iter()
                .filter(|instruction| {
                    instruction.opcode == OpCode::ReleaseTemps
                        && instruction._pad & RELEASE_TEMPS_INTERNAL_CVS != 0
                        && instruction.op1 <= *cv
                        && instruction.op2 > *cv
                })
                .count(),
            1
        );
    }
    for instruction in &op.instructions {
        if instruction.opcode == OpCode::ReleaseTemps
            && instruction._pad & RELEASE_TEMPS_INTERNAL_CVS != 0
        {
            assert_eq!(instruction.op1_type, OpType::Cv);
            assert_eq!(instruction.op2_type, OpType::Cv);
            assert!(!instruction.is_plain_subexpression_release());
            assert!((instruction.op1..instruction.op2).all(|cv| private.contains(&cv)));
        }
    }
}

#[cfg(feature = "quick-loops")]
#[test]
fn invariant_projection_cleanup_cannot_retire_the_public_json_root() {
    let source = r#"<?php
function accumulateProjected($receiver, $document) {
    $count = 0;
    for ($step = 0; $step < 40; $step++) {
        $entry = json_decode($document, true);
        $receiver->consume($entry['amount']);
        $count += $step;
    }
    return $count;
}
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let mut compiled = Compiler::new().compile(&statements).unwrap();
    let (_, function) = compiled.functions.remove(0);
    let mut op = function.op_array;
    let (backedge, header) = op
        .instructions
        .iter()
        .enumerate()
        .find_map(|(ip, instruction)| {
            (matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                && usize::from(instruction.op1) < ip)
                .then_some((ip, usize::from(instruction.op1)))
        })
        .unwrap();
    assert!(rphp::vm::quick::detect_long_ops_loop(&op, header, backedge).is_some());
    let root = op
        .all_cvs
        .iter()
        .find(|(_, name)| name == "entry")
        .unwrap()
        .0 as u16;
    let release = op
        .instructions
        .iter_mut()
        .find(|instruction| instruction.is_completed_internal_cv_release())
        .unwrap();
    release.op1 = root;
    release.op2 = root + 1;
    assert!(rphp::vm::quick::detect_long_ops_loop(&op, header, backedge).is_none());
}

#[test]
fn virtual_property_cleanup_requires_a_private_argument_projection() {
    let source = r#"<?php
class ProjectionDispatch {
    public $mapper;
    public function read($entry) {
        $number = $this->mapper->convert($entry->amount);
        return ['number' => $number];
    }
}
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let mut compiled = Compiler::new().compile(&statements).unwrap();
    let class = &mut compiled.class_defs[0];
    let method = class
        .methods
        .iter()
        .position(|(name, ..)| name == "read")
        .unwrap();
    let (_, _, _, _, mut function) = class.methods.remove(method);
    assert!(function.object_array_plan.is_some());
    let release = function
        .op_array
        .instructions
        .iter_mut()
        .find(|instruction| instruction.is_completed_internal_cv_release())
        .unwrap();
    release.op1 = 1;
    release.op2 = 2;
    let replanned = rphp::compiler::finalize_user_method(function, "read", false);
    assert!(replanned.object_array_plan.is_none());
}
