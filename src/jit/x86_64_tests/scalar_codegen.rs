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
