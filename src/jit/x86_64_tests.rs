use super::*;

#[test]
fn invariant_operands_are_loaded_once_per_native_entry() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(3),
        result: 6,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 7,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 8,
        destination: 2,
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(100),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .unwrap();

    let slot_3_load = [0x4c, 0x8b, 0xaf, 0x18, 0x00, 0x00, 0x00];
    let slot_4_load = [0x4c, 0x8b, 0xb7, 0x20, 0x00, 0x00, 0x00];
    assert_eq!(
        program
            .code()
            .windows(slot_3_load.len())
            .filter(|window| *window == slot_3_load)
            .count(),
        5,
        "each of the five ABI entries should load invariant slot 3 once"
    );
    assert_eq!(
        program
            .code()
            .windows(slot_4_load.len())
            .filter(|window| *window == slot_4_load)
            .count(),
        5,
        "each of the five ABI entries should load invariant slot 4 once"
    );
    let dedicated_bound_load = [0x48, 0xb9, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(
        program
            .code()
            .windows(dedicated_bound_load.len())
            .filter(|window| *window == dedicated_bound_load)
            .count(),
        5,
        "constant bounds should stay in RCX unless a fourth resident value uses it"
    );

    let mut slots = [0i64; 64];
    slots[3] = 50;
    slots[4] = 7;
    let outcome = program.call_chunk(&mut slots, 128).unwrap();
    assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(
        (slots[0], slots[1], slots[2], slots[3], slots[4]),
        (100, 98, 105, 50, 7)
    );
}

#[test]
fn finite_string_hash_context_survives_signed_division_abi_registers() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::StringToken {
        token: 1,
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::StringLength {
        source: 2,
        lengths: [4, 5, 0, 0],
        token_count: 2,
        result: 3,
    };
    operations[2] = NativeStraightLongOperation::HashLoad {
        key: 2,
        entry_base: 0,
        token_count: 2,
        result: 4,
        destination: None,
    };
    operations[3] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(3),
        result: 5,
    };
    operations[4] = NativeStraightLongOperation::HashStore {
        key: 2,
        entry_base: 0,
        token_count: 2,
        source: QuickLongOperand::Slot(5),
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .expect("finite String and contextual hash operations should lower on x86");
    assert!(
        !program
            .code()
            .windows(2)
            .any(|window| window == [0x41, 0x55]),
        "mixed context entries must not pay the scalar invariant R13 prologue"
    );

    let mut left = 7i64;
    let mut right = 20i64;
    let mut entries =
        [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    let outcome = program
        .call_chunk_with_context(&mut slots, 8, &entries)
        .unwrap();

    assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 1);
    assert_eq!(slots[2], 1);
    assert_eq!(slots[3], 5);
    assert_eq!(slots[4], 20);
    assert_eq!(slots[5], 4);
    assert_eq!(left, 7);
    assert_eq!(right, 4);
}

#[test]
fn invalid_finite_string_token_side_exits_before_hash_mutation() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::StringLength {
        source: 2,
        lengths: [4, 5, 0, 0],
        token_count: 2,
        result: 3,
    };
    operations[1] = NativeStraightLongOperation::HashStore {
        key: 2,
        entry_base: 0,
        token_count: 2,
        source: QuickLongOperand::Const(99),
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 2,
        post_result: None,
    })
    .expect("guarded finite String operations should lower on x86");

    let mut left = 7i64;
    let mut right = 20i64;
    let mut entries =
        [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    slots[2] = 3;
    slots[3] = -1;
    let outcome = program
        .call_chunk_with_context(&mut slots, 8, &entries)
        .unwrap();

    assert_eq!(
        outcome.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(outcome.failed_operation, Some(0));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[3], -1);
    assert_eq!(left, 7);
    assert_eq!(right, 20);
}
use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
use crate::vm::function::{ScalarLongProgram, ScalarLongSelect};

fn additive_recurrence(bound: i64, reversed: bool) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let (lhs, rhs) = if reversed {
        (QuickLongOperand::Slot(0), QuickLongOperand::Slot(1))
    } else {
        (QuickLongOperand::Slot(1), QuickLongOperand::Slot(0))
    };
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs,
        rhs,
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 1,
        post_result: None,
    }
}

fn composed_add_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 4,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 2,
        post_result: Some(5),
    }
}

fn structured_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(10),
        result: 2,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(100),
        result: 2,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 5,
        post_result: None,
    }
}

fn structured_affine_expression(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 6,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(6),
        rhs: QuickLongOperand::Const(1),
        result: 7,
        destination: 1,
    };
    operations[3] = NativeStraightLongOperation::Jump { target: 6 };
    operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(5),
        result: 8,
    };
    operations[5] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(8),
        rhs: QuickLongOperand::Const(2),
        result: 9,
        destination: 1,
    };
    operations[6] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 10,
    };
    operations[7] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(10),
        rhs: QuickLongOperand::Const(11),
        result: 11,
        destination: 2,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 8,
        post_result: None,
    }
}

fn scheduled_increment_between_affine_pair(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::Jump { target: 2 };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Const(11),
        result: 5,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 4,
        post_result: None,
    }
}

fn scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    output: ScalarLongSource,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [output],
            output_count: 1,
        },
        None,
    )
}

fn conditional_scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    select: ScalarLongSelect,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [select.when_true],
            output_count: 1,
        },
        Some(select),
    )
}

#[test]
fn encoder_produces_exact_sysv_add_multiply_bytes() {
    let program = CompiledX86AddMultiply::compile().unwrap();
    assert_eq!(
        program.code(),
        [
            0x48, 0x8b, 0xc7, // MOV RAX, RDI
            0x48, 0x03, 0xc6, // ADD RAX, RSI
            0x48, 0x0f, 0xaf, 0xc2, // IMUL RAX, RDX
            0xc3, // RET
        ]
    );
}

#[test]
fn generated_code_executes_through_the_sysv_abi() {
    let program = CompiledX86AddMultiply::compile().unwrap();
    assert_eq!(program.call(12, -5, 9), 63);
    assert_eq!(program.call(-8, 3, -4), 20);
}

#[test]
fn standalone_scalar_program_executes_and_side_exits_on_overflow() {
    let plan = scalar_plan(
        3,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Input(2),
            },
        ],
        ScalarLongSource::Temporary(1),
    );
    let program = CompiledScalarLongProgram::compile(&plan).unwrap();
    assert_eq!(
        program.call(&[7, 5, 3]).unwrap(),
        ScalarLongJitOutcome::Value(36)
    );
    assert_eq!(
        program.call(&[i64::MAX, 1, 3]).unwrap(),
        ScalarLongJitOutcome::SideExit
    );
}

#[test]
fn standalone_scalar_lowering_embeds_imm32_multiply_and_keeps_overflow_exit() {
    let plan = scalar_plan(
        1,
        vec![ScalarLongOp {
            kind: ScalarLongOpKind::Multiply,
            lhs: ScalarLongSource::Input(0),
            rhs: ScalarLongSource::Constant(129),
        }],
        ScalarLongSource::Temporary(0),
    );
    let program = CompiledScalarLongProgram::compile(&plan).unwrap();
    let imul_imm32 = [0x48, 0x69, 0xc0, 0x81, 0x00, 0x00, 0x00];
    assert!(
        program
            .code()
            .windows(imul_imm32.len())
            .any(|window| window == imul_imm32),
        "constant multiply should lower directly to IMUL r64, r64, imm32"
    );
    assert_eq!(
        program.call(&[-7]).unwrap(),
        ScalarLongJitOutcome::Value(-903)
    );
    assert_eq!(
        program.call(&[i64::MAX]).unwrap(),
        ScalarLongJitOutcome::SideExit
    );
}

#[test]
fn standalone_conditional_scalar_program_executes_only_selected_edge() {
    let plan = conditional_scalar_plan(
        1,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(3),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(5),
            },
        ],
        ScalarLongSelect {
            kind: ScalarLongConditionKind::Equal,
            lhs: ScalarLongConditionOperand::BitwiseAnd {
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(1),
            },
            rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            shared_operation_count: 0,
            when_true_operation_count: 1,
            when_true: ScalarLongSource::Temporary(0),
            when_false: ScalarLongSource::Temporary(1),
        },
    );
    let program = CompiledScalarLongProgram::compile(&plan).unwrap();
    assert_eq!(program.call(&[4]).unwrap(), ScalarLongJitOutcome::Value(12));
    assert_eq!(program.call(&[5]).unwrap(), ScalarLongJitOutcome::Value(25));
    assert!(
        program
            .code()
            .windows(4)
            .any(|window| window == [0x48, 0x83, 0xe0, 0x01]),
        "bitwise condition should encode AND RAX, 1"
    );
    assert!(
        program
            .code()
            .windows(4)
            .any(|window| window == [0x48, 0x83, 0xf8, 0x00]),
        "condition should encode CMP RAX, 0"
    );
    assert!(
        !program
            .code()
            .windows(2)
            .any(|window| window == [0x49, 0xb8]),
        "constant condition rhs should not materialize in R8"
    );
}

#[test]
fn standalone_scalar_cache_compiles_at_shared_hotness_threshold() {
    let plan = scalar_plan(
        2,
        vec![
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Input(1),
            },
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Temporary(0),
                rhs: ScalarLongSource::Constant(3),
            },
        ],
        ScalarLongSource::Temporary(1),
    );
    let mut arguments = [0_i64; MAX_SCALAR_LONG_INPUTS];
    arguments[0] = 7;
    arguments[1] = 5;
    for _ in 1..SCALAR_LONG_JIT_HOT_THRESHOLD {
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarLongJitDispatch::Interpret
        );
    }
    assert_eq!(
        plan.native_jit().dispatch(&plan, &arguments),
        ScalarLongJitDispatch::Value(36)
    );
    assert!(plan.native_jit().is_compiled());
    assert_eq!(plan.native_jit().native_entries(), 1);
}

#[test]
fn encoder_sets_rex_extensions_for_high_registers() {
    let mut assembler = X86_64Assembler::new();
    assembler.move_register(X86_64Register::R8, X86_64Register::R9);
    assert_eq!(&*assembler.finish(), &[0x4d, 0x8b, 0xc1]);
}

#[test]
fn encoder_relaxes_forward_branches_and_repatches_remaining_rel32() {
    let mut assembler = X86_64Assembler::new();
    let first = assembler.jump_not_equal_rel32();
    assembler.allow_short_branch(first);
    assembler.bytes.resize(124, 0x90);
    let second = assembler.jump_rel32();
    assembler.allow_short_branch(second);
    assembler.bytes.resize(134, 0x90);
    assembler.patch_rel32(first, 134);
    assembler.patch_rel32(second, 134);
    let backward = assembler.jump_rel32();
    assembler.patch_rel32(backward, 0);
    let far = assembler.jump_equal_rel32();
    assembler.allow_short_branch(far);
    assembler.bytes.resize(273, 0x90);
    assembler.patch_rel32(far, 273);

    let code = assembler.finish();
    assert_eq!(code.len(), 266);
    assert_eq!(&code[..2], &[0x75, 0x7d]);
    assert_eq!(&code[120..122], &[0xeb, 0x05]);
    assert_eq!(&code[127..132], &[0xe9, 0x7c, 0xff, 0xff, 0xff]);
    assert_eq!(&code[132..138], &[0x0f, 0x84, 0x80, 0x00, 0x00, 0x00]);
}

#[test]
fn encoder_uses_the_shortest_exact_signed_immediate_forms() {
    let mut assembler = X86_64Assembler::new();
    assert!(assembler.add_immediate(X86_64Register::R13, 127));
    assert!(assembler.subtract_immediate(X86_64Register::R14, -129));
    assert!(assembler.xor_immediate(X86_64Register::R15, -1));
    assert!(assembler.and_immediate(X86_64Register::R12, 127));
    assert!(assembler.compare_immediate(X86_64Register::R11, -129));
    assert!(assembler.multiply_immediate(X86_64Register::R13, X86_64Register::R11, 3,));
    assert!(assembler.multiply_immediate(X86_64Register::R14, X86_64Register::R13, -129,));
    assert!(assembler.affine_scale_add_immediate(X86_64Register::R14, X86_64Register::R13, 3, 11,));
    assert_eq!(
        &*assembler.finish(),
        &[
            0x49, 0x83, 0xc5, 0x7f, // ADD R13, 127 (imm8)
            0x49, 0x81, 0xee, 0x7f, 0xff, 0xff, 0xff, // SUB R14, -129 (imm32)
            0x49, 0x83, 0xf7, 0xff, // XOR R15, -1 (imm8)
            0x49, 0x83, 0xe4, 0x7f, // AND R12, 127 (imm8)
            0x49, 0x81, 0xfb, 0x7f, 0xff, 0xff, 0xff, // CMP R11, -129 (imm32)
            0x4d, 0x6b, 0xeb, 0x03, // IMUL R13, R11, 3 (imm8)
            0x4d, 0x69, 0xf5, 0x7f, 0xff, 0xff, 0xff, // IMUL R14, R13, -129
            0x4f, 0x8d, 0x74, 0x6d, 0x0b, // LEA R14, [R13 + R13*2 + 11]
        ]
    );

    let mut too_wide = X86_64Assembler::new();
    assert!(!too_wide.add_immediate(X86_64Register::RAX, 1_i64 << 40));
    assert!(too_wide.finish().is_empty());
}

#[test]
fn range_proven_loop_executes_and_publishes_exact_slots() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(result.failed_operation, None);
    assert_eq!(slots[0], 100);
    assert_eq!(slots[1], 4_955);
    assert_eq!(slots[2], 4_955);

    assert!(
        program
            .code()
            .windows(6)
            .any(|bytes| bytes == [0x0f, 0x8d, 0x10, 0, 0, 0])
    );
    assert!(
        program
            .code()
            .windows(6)
            .any(|bytes| bytes == [0x0f, 0x8c, 0xf0, 0xff, 0xff, 0xff])
    );
    assert!(
        program
            .code()
            .windows(7)
            .any(|bytes| bytes == [0x48, 0x89, 0x97, 0x10, 0, 0, 0])
    );
}

#[test]
fn dynamic_bound_is_loaded_from_shadow_on_every_native_entry() {
    let mut config = additive_recurrence(0, false);
    config.bound = QuickLongOperand::Slot(3);
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();

    let mut first = [0_i64; 64];
    first[1] = 10;
    first[3] = 4;
    program.call(&mut first).unwrap();
    assert_eq!(&first[..4], &[4, 16, 16, 4]);

    let mut second = [0_i64; 64];
    second[1] = 1;
    second[3] = 6;
    program.call(&mut second).unwrap();
    assert_eq!(&second[..4], &[6, 16, 16, 6]);
}

#[test]
fn linear_lowering_executes_composed_operations_and_post_result() {
    let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(4)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..6], &[4, 20, 20, 0, 4, 3]);
}

#[test]
fn linear_lowering_supports_subtract_and_multiply() {
    let mut config = composed_add_recurrence(3);
    config.operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(2),
        result: 2,
        destination: 1,
    };
    config.operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 2;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..5], &[3, 16, 16, 0, 13]);
}

#[test]
fn linear_checked_exit_reports_exact_failed_operation() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(2),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 2,
        destination: 1,
    };
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(1),
        }
    );
    assert_eq!(&slots[..5], &[0, i64::MAX, 77, 0, 2]);
}

#[test]
fn linear_polling_entry_preserves_composed_state_at_safepoint() {
    let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(5_000)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    let interrupt = true;
    let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(slots[0], 1_024);
    assert_eq!(slots[1], 524_810);
    assert_eq!(slots[2], 524_810);
    assert_eq!(slots[4], 1_024);
    assert_eq!(slots[5], 1_023);
}

#[test]
fn range_proven_polling_schedules_induction_before_a_common_scalar_suffix() {
    let config = structured_recurrence(5_000);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            config.body_output_mask(),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 103_244, 103_244]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 504_820, 504_820]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let induction_increment = [0x49, 0x83, 0xc3, 0x01];
    let common_suffix_add = [0x49, 0x83, 0xc5, 0x01];
    assert_eq!(
        polling_code
            .windows(induction_increment.len())
            .filter(|window| *window == induction_increment)
            .count(),
        1
    );
    let increment_offset = polling_code
        .windows(induction_increment.len())
        .position(|window| window == induction_increment)
        .unwrap();
    assert_eq!(
        &polling_code[increment_offset + induction_increment.len()
            ..increment_offset + induction_increment.len() + common_suffix_add.len()],
        &common_suffix_add
    );
}

#[test]
fn range_proven_polling_fuses_immediate_affine_scalar_pair() {
    let config = structured_affine_expression(5_000);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            (1u64 << 1) | (1u64 << 2),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 5_113, 15_350]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 24_993, 74_990]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let scheduled_affine = [
        0x49, 0x83, 0xc3, 0x01, // ADD R11, 1
        0x4f, 0x8d, 0x74, 0x6d, 0x0b, // LEA R14, [R13 + R13*2 + 11]
    ];
    assert_eq!(
        polling_code
            .windows(scheduled_affine.len())
            .filter(|window| *window == scheduled_affine)
            .count(),
        1
    );
}

#[test]
fn range_proven_polling_preserves_published_affine_intermediate() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 1,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(11),
        result: 2,
        destination: 2,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(5_000),
        operations,
        operation_count: 2,
        post_result: None,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            config,
            (1u64 << 1) | (1u64 << 2),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!(&slots[..3], &[1_024, 3_069, 3_080]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 14_997, 15_008]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(polling_code.windows(2).all(|window| window != [0x4f, 0x8d]));
}

#[test]
fn range_proven_polling_does_not_fuse_across_scheduled_induction_increment() {
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            scheduled_increment_between_affine_pair(5_000),
            (1u64 << 1) | (1u64 << 5),
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[5]), (5_000, 15_008, 15_008));
    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(polling_code.windows(2).all(|window| window != [0x4f, 0x8d]));
}

#[test]
fn range_proven_polling_keeps_three_recurrences_resident_and_publishes_them() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 4,
        destination: 1,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(2),
        result: 5,
        destination: 2,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::BitwiseXor,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Slot(0),
        result: 6,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(5_000),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    slots[2] = 7;
    slots[3] = 9;

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    let mut expected_xor = 9;
    for value in 0..1_024 {
        expected_xor ^= value;
    }
    assert_eq!(&slots[..4], &[1_024, 523_781, 2_055, expected_xor]);
    assert_eq!(&slots[4..7], &[0, 0, 0]);

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    for value in 1_024..5_000 {
        expected_xor ^= value;
    }
    assert_eq!(slots[0], 5_000);
    assert_eq!(slots[1], 12_497_505);
    assert_eq!(slots[2], 10_007);
    assert_eq!(slots[3], expected_xor);
    assert_eq!(&slots[4..7], &[0, 0, 0]);
}

#[test]
fn constant_bound_frees_rcx_for_a_fourth_resident_value() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    for (index, slot) in [1u16, 2, 3].into_iter().enumerate() {
        operations[index] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(slot),
            rhs: QuickLongOperand::Slot(7),
            result: slot + 3,
            destination: slot,
        };
    }
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1..=3].copy_from_slice(&[1, 2, 3]);
    slots[7] = 5;

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..4], &[4, 21, 22, 23]);

    let polling_code = &program.code()[program.polling_entry_offset..];
    let slot_7_rcx_load = [0x48, 0x8b, 0x8f, 0x38, 0x00, 0x00, 0x00];
    assert_eq!(
        polling_code
            .windows(slot_7_rcx_load.len())
            .filter(|window| *window == slot_7_rcx_load)
            .count(),
        1,
        "the freed bound register should cache invariant slot 7 once"
    );
    assert_eq!(
        polling_code
            .windows(4)
            .filter(|window| *window == [0x49, 0x83, 0xfb, 0x04])
            .count(),
        2,
        "entry and backedge should compare induction against the embedded bound"
    );
    for direct_add in [[0x4c, 0x03, 0xe9], [0x4c, 0x03, 0xf1], [0x4c, 0x03, 0xf9]] {
        assert!(
            polling_code
                .windows(direct_add.len())
                .any(|window| window == direct_add),
            "each carried recurrence should consume invariant RCX directly"
        );
    }
}

#[test]
fn wide_constant_bound_keeps_the_dedicated_bound_register() {
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX) + 1)),
        None
    );
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MIN) - 1)),
        None
    );
    assert_eq!(
        x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX))),
        Some(i64::from(i32::MAX))
    );
    assert_eq!(x86_embedded_loop_bound(QuickLongOperand::Slot(1)), None);
}

#[test]
fn range_proven_structured_polling_merges_carried_register_values() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(5_000),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[3] = -5;

    let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!((slots[0], slots[1], slots[3]), (1_024, 31, 1_019));

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[3]), (5_000, 31, 4_995));
}

#[test]
fn range_proven_structured_polling_forwards_branch_local_temporaries() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 5,
    };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(5),
        rhs: QuickLongOperand::Const(7),
        result: 6,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(6),
        result: 2,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 3,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 3);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 5,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 10;
    slots[3] = -5;
    slots[5] = 77;
    slots[6] = 88;

    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[3]), (4, 27, -1));
    assert_eq!((slots[5], slots[6]), (77, 88));
}

#[test]
fn range_proven_structured_polling_defers_visible_phi_publication() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 5,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(5),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    operations[3] = NativeStraightLongOperation::Jump { target: 6 };
    operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(5),
        result: 6,
    };
    operations[5] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(6),
        rhs: QuickLongOperand::Const(2),
        result: 2,
        destination: 1,
    };
    operations[6] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 7,
    };
    operations[7] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(7),
        rhs: QuickLongOperand::Const(11),
        result: 4,
        destination: 3,
    };
    // Result/destination aliases are defined by the same operation and
    // therefore share one fixed publication register per pair.
    let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3) | (1u64 << 4);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 8,
                post_result: None,
            },
            publication_mask,
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(
        (slots[0], slots[1], slots[2], slots[3], slots[4]),
        (4, 13, 13, 50, 50)
    );

    let polling_code = &program.code()[program.polling_entry_offset..];
    for eliminated_copy in [[0x4c, 0x8b, 0xe8], [0x4c, 0x8b, 0xf0]] {
        assert!(
            !polling_code
                .windows(eliminated_copy.len())
                .any(|window| window == eliminated_copy),
            "structured result should be generated directly in its fixed register"
        );
    }
    for eliminated_forward in [[0x49, 0x8b, 0xd5], [0x49, 0x8b, 0xd6]] {
        assert!(
            !polling_code
                .windows(eliminated_forward.len())
                .any(|window| window == eliminated_forward),
            "fully represented fixed result should not be copied to RDX"
        );
    }
    for direct_affine in [
        [0x4f, 0x8d, 0x6c, 0x5b, 0x01],
        [0x4f, 0x8d, 0x6c, 0x9b, 0xfe],
        [0x4f, 0x8d, 0x74, 0x6d, 0x0b],
    ] {
        assert!(
            polling_code
                .windows(direct_affine.len())
                .any(|window| window == direct_affine),
            "expected fused affine arithmetic in its fixed publication register"
        );
    }
    for slot in [1_i32, 2_i32, 3_i32, 4_i32] {
        let mut rax_store = vec![0x48, 0x89, 0x87];
        rax_store.extend_from_slice(&(slot * 8).to_le_bytes());
        assert!(
            !polling_code
                .windows(rax_store.len())
                .any(|window| window == rax_store),
            "visible phi slot {slot} should publish from its fixed register"
        );
    }

    slots[1..=4].copy_from_slice(&[101, 102, 103, 104]);
    let empty = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(empty.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[1..=4], &[101, 102, 103, 104]);
}

#[test]
fn range_proven_direct_result_preserves_old_right_resident() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 2,
        destination: 1,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 1,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 3, 3));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x2b, 0xe8]),
        "subtract should write directly to R13"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x4c, 0x8b, 0xe8]),
        "direct subtract should not copy RAX into R13"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x8b, 0xd5]),
        "dead local result should not be forwarded from R13 to RDX"
    );
}

#[test]
fn range_proven_direct_result_forwards_untracked_immediate_alias() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    operations[1] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Slot(2),
        result: 3,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 2,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2) | (1u64 << 3),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    slots[2] = 99;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2], slots[3]), (4, 7, 7, 7));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x8b, 0xd5]),
        "untracked result alias should be forwarded from R13 to RDX"
    );
}

#[test]
fn range_proven_resident_operands_feed_branch_and_rebank_fixed_arithmetic() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(1)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(2)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(2),
        result: 3,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Const(1),
        result: 4,
        destination: 2,
    };
    let publication_mask = (1u64 << 1) | (1u64 << 2);
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 3,
                post_result: None,
            },
            publication_mask,
            publication_mask,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 1;
    slots[2] = 2;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 7, 6));

    let polling_code = &program.code()[program.polling_entry_offset..];
    let initial_jge = polling_code
        .windows(2)
        .position(|window| window == [0x0f, 0x8d])
        .expect("polling entry should reject an empty range");
    let mut loop_offset = initial_jge + 6;
    while polling_code.get(loop_offset) == Some(&0x90) {
        loop_offset += 1;
    }
    assert_eq!(
        (program.polling_entry_offset + loop_offset) % X86_STRUCTURED_LOOP_ALIGNMENT,
        0,
        "structured polling loop should start on its cache-line boundary"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x3b, 0xee]),
        "branch should compare R13 and R14 directly"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x8b, 0xc6]),
        "fixed-to-fixed arithmetic should re-bank R14 through R8"
    );
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x03, 0xe8]),
        "re-banked add should write R13 from R8"
    );
}

#[test]
fn range_proven_resident_rhs_feeds_scratch_result_directly() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 2,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 1,
                post_result: None,
            },
            (1u64 << 1) | (1u64 << 2),
            1u64 << 1,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[1], slots[2]), (4, 5, 8));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(3)
            .any(|window| window == [0x49, 0x03, 0xc5]),
        "scratch result should consume resident R13 directly"
    );
    assert!(
        !polling_code
            .windows(3)
            .any(|window| window == [0x4d, 0x8b, 0xc5]),
        "resident R13 should not be copied into R8 for an RAX result"
    );
}

#[test]
fn range_proven_division_moves_latest_rdx_divisor_before_cqo() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Const(100),
        rhs: QuickLongOperand::Slot(2),
        result: 3,
    };
    let program =
        CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
            NativeStraightLongLoopConfig {
                induction_slot: 0,
                bound: QuickLongOperand::Const(4),
                operations,
                operation_count: 2,
                post_result: None,
            },
            1u64 << 3,
            0,
        )
        .unwrap();
    let mut slots = [0_i64; 64];
    let completed = program.call_proven_polling(&mut slots, &false).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3]), (4, 25));

    let polling_code = &program.code()[program.polling_entry_offset..];
    assert!(
        polling_code
            .windows(5)
            .any(|window| window == [0x4c, 0x8b, 0xc2, 0x48, 0x99]),
        "RDX divisor must move to R8 immediately before CQO"
    );
}

#[test]
fn structured_phi_rejects_nonlocal_read_before_merge() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(1),
        result: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(9),
        result: 5,
    };
    operations[4] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Slot(1),
        result: 2,
    };
    operations[5] = NativeStraightLongOperation::Move {
        source: QuickLongOperand::Const(2),
        result: 1,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(4),
        operations,
        operation_count: 6,
        post_result: None,
    };
    let block_starts = straight_long_structured_block_starts(&config);
    let (definitely_written_before, definitely_written_exit) =
        straight_long_structured_definitely_written(&config);
    assert_ne!(definitely_written_exit & (1u64 << 1), 0);
    assert!(!structured_phi_candidate_is_safe(
        &config,
        1u64 << 1,
        &block_starts,
        &definitely_written_before,
    ));
}

#[test]
fn structured_lowering_executes_both_forward_control_flow_edges() {
    let program = CompiledX86StraightLongLoop::compile(structured_recurrence(4)).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 224, 224]);

    assert_eq!(
        program
            .code()
            .windows(5)
            .filter(|window| *window == [0x49, 0x83, 0xfb, 0x02, 0x7d])
            .count(),
        5,
        "each ABI entry should use short JGE for the structured false edge"
    );
    assert_eq!(
        program.code().iter().filter(|byte| **byte == 0xeb).count(),
        5,
        "each ABI entry should use a short unconditional join jump"
    );
}

#[test]
fn structured_lowering_elides_control_flow_to_the_immediate_successor() {
    let mut config = composed_add_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(-1)),
        false_target: 1,
    };
    config.operations[1] = NativeStraightLongOperation::Jump { target: 2 };
    config.operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    config.operation_count = 3;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 6, 6]);

    let fast_code = &program.code()[..program.checked_entry_offset];
    assert!(
        !fast_code.windows(2).any(|window| window == [0x0f, 0x85]),
        "a predicate whose false edge is fallthrough should not be emitted"
    );
    assert!(
        !fast_code.contains(&0xe9),
        "an unconditional jump to fallthrough should not be emitted"
    );
}

#[test]
fn structured_bitwise_condition_executes_in_private_shadow() {
    let mut config = structured_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::BitwiseAnd {
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
        },
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 3,
    };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[4, 224, 224]);
}

#[test]
fn guard_side_exit_reports_exact_operation_after_completed_iterations() {
    let mut config = structured_recurrence(4);
    config.operations[0] = NativeStraightLongOperation::Guard {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        expected: true,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(10),
        result: 2,
        destination: 1,
    };
    config.operations[2] = NativeStraightLongOperation::Jump { target: 5 };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(&slots[..3], &[2, 20, 20]);
}

#[test]
fn scalar_lowering_executes_divide_modulo_and_bitwise_ops() {
    let mut config = composed_add_recurrence(5);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Modulo,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(2),
        result: 5,
    };
    config.operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseAnd,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 6,
    };
    config.operations[3] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseOr,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 7,
    };
    config.operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::BitwiseXor,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(5),
        result: 8,
    };
    config.operation_count = 5;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 5);
    assert_eq!(&slots[4..9], &[1, 2, 0, 3, 3]);
}

#[test]
fn checked_division_side_exit_prevents_native_zero_divide() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(7),
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[4] = 91;
    slots[7] = 0;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(slots[0], 0);
    assert_eq!(slots[4], 91);
}

#[test]
fn checked_operations_share_cold_side_exit_publication() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(7),
        result: 4,
    };
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    config.operation_count = 2;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();

    let mut divide_by_zero = [0_i64; 64];
    divide_by_zero[7] = 0;
    assert_eq!(
        program.call(&mut divide_by_zero).unwrap(),
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    let mut sum_overflow = [0_i64; 64];
    sum_overflow[1] = i64::MAX;
    sum_overflow[7] = 1;
    assert_eq!(
        program.call(&mut sum_overflow).unwrap(),
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(1),
        }
    );

    let checked_code = &program.code()[program.checked_entry_offset..program.chunk_entry_offset];
    for selector in [
        [0xb8, 0x06, 0x00, 0x00, 0x00, 0xe9],
        [0xb8, 0x06, 0x01, 0x00, 0x00, 0xe9],
    ] {
        assert_eq!(
            checked_code
                .windows(selector.len())
                .filter(|window| *window == selector)
                .count(),
            1,
            "each failed operation should select one shared cold epilogue"
        );
    }
}

#[test]
fn standalone_modulo_preserves_signed_remainder_semantics() {
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(6),
        divisor: 2,
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[6] = -5;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[4], -1);
    assert!(
        program
            .code()
            .windows(4)
            .any(|window| window == [0x48, 0x83, 0xe0, 0x01]),
        "small remainder mask should encode directly in AND"
    );
    let mut divisor_load = vec![0x49, 0xb8];
    divisor_load.extend_from_slice(&2_i64.to_le_bytes());
    assert!(
        !program
            .code()
            .windows(divisor_load.len())
            .any(|window| window == divisor_load),
        "power-of-two divisor should not be materialized before mask lowering"
    );
}

#[test]
fn wide_power_of_two_remainder_materializes_only_the_exact_mask() {
    let divisor = 1_i64 << 40;
    let mask = divisor - 1;
    let mut config = composed_add_recurrence(1);
    config.operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(6),
        divisor,
        result: 4,
    };
    config.operation_count = 1;
    config.post_result = None;
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[6] = -(divisor + 5);
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[4], -5);

    let mut mask_load = vec![0x49, 0xb8];
    mask_load.extend_from_slice(&mask.to_le_bytes());
    assert!(
        program
            .code()
            .windows(mask_load.len())
            .any(|window| window == mask_load),
        "mask outside sign-extended imm32 must retain MOVABS fallback"
    );
    let mut divisor_load = vec![0x49, 0xb8];
    divisor_load.extend_from_slice(&divisor.to_le_bytes());
    assert!(
        !program
            .code()
            .windows(divisor_load.len())
            .any(|window| window == divisor_load),
        "recognized divisor itself is dead even when the mask needs MOVABS"
    );
}

#[test]
fn modulo_conditional_accumulate_matches_quick_ops_shape() {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Modulo {
        value: QuickLongOperand::Slot(2),
        divisor: 2,
        result: 4,
    };
    operations[1] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::Equal,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(4)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 3,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(2),
        result: 6,
        destination: 1,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 2,
        bound: QuickLongOperand::Slot(0),
        operations,
        operation_count: 3,
        post_result: None,
    };
    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = 100_000;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[2], 100_000);
    assert_eq!(slots[1], 2_499_950_000);
    assert!(
        program.code().windows(4).any(|window| {
            matches!(window[0], 0x48 | 0x49)
                && window[1] == 0x83
                && window[2] & 0xf8 == 0xf8
                && window[3] == 0
        }),
        "comparison against zero should use CMP r64, imm8"
    );
}

#[test]
fn chunk_entry_publishes_exact_safepoint_and_resumes_to_completion() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(10, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;

    let first = program.call_chunk(&mut slots, 3).unwrap();
    assert_eq!(
        first,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[3, 8, 8]);

    let second = program.call_chunk(&mut slots, 7).unwrap();
    assert_eq!(
        second,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::Completed,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[10, 50, 50]);

    let mut exact = [0_i64; 64];
    exact[1] = 5;
    let exact_result = program.call_chunk(&mut exact, 10).unwrap();
    assert_eq!(
        exact_result.outcome,
        NativeStraightLongLoopOutcome::Completed
    );
    assert_eq!(&exact[..3], &[10, 50, 50]);
}

#[test]
fn chunk_entry_rejects_zero_budget_and_retains_checked_side_exit() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    assert!(matches!(
        program.call_chunk(&mut slots, 0),
        Err(X86StraightLongLoopError::ZeroIterationBudget)
    ));

    slots[0] = 1;
    slots[1] = i64::MAX;
    slots[2] = 77;
    let side_exit = program.call_chunk(&mut slots, 1).unwrap();
    assert_eq!(
        side_exit,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::OperationSideExit,
            failed_operation: Some(0),
        }
    );
    assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
}

#[test]
fn polling_entry_stays_native_until_interrupt_or_completion() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(5_000, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let interrupt = true;
    let interrupted = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(
        interrupted,
        NativeStraightLongLoopResult {
            outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
            failed_operation: None,
        }
    );
    assert_eq!(&slots[..3], &[1_024, 523_781, 523_781]);

    let interrupt = false;
    let completed = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[5_000, 12_497_505, 12_497_505]);
}

#[test]
fn polling_entry_gives_completion_priority_over_pending_interrupt() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = 5;
    let interrupt = true;
    let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
    assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(&slots[..3], &[100, 4_955, 4_955]);
}

#[test]
fn checked_side_exit_preserves_state_before_first_failed_operation() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = 1;
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
    assert!(
        !program.code()[..program.checked_entry_offset]
            .windows(2)
            .any(|bytes| bytes == [0x0f, 0x80])
    );
    assert!(
        program.code()[program.checked_entry_offset..]
            .windows(2)
            .any(|bytes| bytes == [0x0f, 0x80])
    );
}

#[test]
fn checked_side_exit_publishes_last_successful_iteration() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
    let mut slots = [0_i64; 64];
    slots[1] = i64::MAX;
    slots[2] = 77;
    let result = program.call(&mut slots).unwrap();
    assert_eq!(
        result.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(result.failed_operation, Some(0));
    assert_eq!(&slots[..3], &[1, i64::MAX, i64::MAX]);
}

#[test]
fn reversed_addition_and_empty_range_preserve_semantics() {
    let program = CompiledX86StraightLongLoop::compile(additive_recurrence(4, true)).unwrap();
    let mut slots = [0_i64; 64];
    slots[0] = -2;
    slots[1] = 10;
    program.call(&mut slots).unwrap();
    assert_eq!(&slots[..3], &[4, 13, 13]);

    let mut empty = [0_i64; 64];
    empty[0] = 4;
    empty[1] = 9;
    empty[2] = 81;
    program.call(&mut empty).unwrap();
    assert_eq!(&empty[..3], &[4, 9, 81]);
}
