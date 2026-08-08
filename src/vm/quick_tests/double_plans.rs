    fn dynamic_double_argument(register: u8) -> QuickDoubleArgumentProgram {
        QuickDoubleArgumentProgram {
            operations: vec![QuickDoubleArgumentOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: QuickDoubleSource::Induction,
                rhs: QuickDoubleSource::Constant(0.5),
            }]
            .into_boxed_slice(),
            outputs: [
                QuickDoubleSource::Temporary(register),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
            ],
            output_count: 1,
            input_slots: [u16::MAX; 8],
            input_count: 0,
        }
    }

    #[test]
    fn forwards_dynamic_double_argument_used_before_register_overwrite() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 1);
    }

    #[test]
    fn retains_buffer_when_x86_rhs_would_be_overwritten() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Subtract,
                    lhs: ScalarDoubleSource::Constant(10.0),
                    rhs: ScalarDoubleSource::Input(0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 0);
    }

    #[test]
    fn flattens_guarded_double_leaf_with_target_neutral_source_remapping() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(2.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .unwrap();
        assert_eq!(flattened.program.operations.len(), 2);
        assert!(matches!(
            flattened.program.operations[0],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Multiply,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Constant(2.0),
            }
        ));
        assert!(matches!(
            flattened.program.operations[1],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Temporary(0),
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(1));
        assert!(flattened.select.is_none());
    }

    fn conditional_double_leaf() -> ScalarDoubleFunctionPlan {
        ScalarDoubleFunctionPlan::new_conditional(
            2,
            ScalarDoubleProgram {
                operations: vec![
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Multiply,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.5),
                    },
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Subtract,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.0),
                    },
                ]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
            ScalarDoubleSelect {
                kind: ScalarLongConditionKind::LessThan,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Input(1),
                shared_operation_count: 0,
                when_true_operation_count: 1,
                when_false_operation_count: 1,
                when_true: ScalarDoubleSource::Temporary(0),
                when_false: ScalarDoubleSource::Temporary(1),
                merge_result: false,
            },
        )
    }

    #[test]
    fn flattens_one_conditional_double_leaf_into_a_common_suffix() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = conditional_double_leaf();

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .expect("one conditional callee should flatten");

        let select = flattened.select.expect("flattened merge select");
        assert!(select.merge_result);
        assert_eq!(select.operation_ranges(3), Some((0, 1, 2)));
        assert_eq!(select.when_true, ScalarDoubleSource::Temporary(0));
        assert_eq!(select.when_false, ScalarDoubleSource::Temporary(1));
        assert!(matches!(
            flattened.program.operations[2],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Selection,
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(2));
    }

    #[test]
    fn rejects_two_conditional_double_callees_from_one_flattened_region() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 1 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Temporary(1),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(2),
        };
        let leaf = conditional_double_leaf();
        let resolved = ResolvedScalarDoubleProgram {
            public_args: leaf.public_args,
            program: &leaf.program,
            select: leaf.select,
        };

        assert!(
            compose_scalar_double_program(&composed, &[Some(resolved), Some(resolved), None],)
                .is_none()
        );
    }

    #[test]
    fn rejects_flattened_double_body_beyond_shared_register_capacity() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let mut leaf_operations = Vec::new();
        for index in 0..8 {
            leaf_operations.push(ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: if index == 0 {
                    ScalarDoubleSource::Input(0)
                } else {
                    ScalarDoubleSource::Temporary(index - 1)
                },
                rhs: ScalarDoubleSource::Constant(1.0),
            });
        }
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: leaf_operations.into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(7),
            },
        );

        assert!(
            compose_scalar_double_program(
                &composed,
                &[
                    Some(ResolvedScalarDoubleProgram {
                        public_args: leaf.public_args,
                        program: &leaf.program,
                        select: leaf.select,
                    }),
                    None,
                ],
            )
            .is_none()
        );
    }
