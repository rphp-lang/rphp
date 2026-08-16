use super::*;
use crate::lexer::Lexer;

fn echo(expressions: Vec<Expr>) -> Stmt {
    Stmt::Echo {
        expressions,
        line: 1,
    }
}

#[test]
fn test_parse_echo_42() {
    let tokens = Lexer::new("<?php echo 42;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(stmts, vec![echo(vec![Expr::Integer(42)])]);
}

#[test]
fn echo_statement_preserves_its_source_line() {
    let tokens = Lexer::new("<?php\n\n echo 42;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();

    assert_eq!(
        stmts,
        vec![Stmt::Echo {
            expressions: vec![Expr::Integer(42)],
            line: 3,
        }]
    );
}

#[test]
fn test_parse_assign_echo() {
    let tokens = Lexer::new("<?php $a = 42; echo $a;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![
            Stmt::Assign {
                var: "a".into(),
                expr: Expr::Integer(42),
            },
            echo(vec![Expr::Variable {
                name: "a".into(),
                line: 1,
            }]),
        ]
    );
}

#[test]
fn test_parse_null_coalescing_assignments() {
    let tokens = Lexer::new("<?php $value ??= 42; $items[1] ??= 'fallback';")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![
            Stmt::CoalesceAssign {
                target: Expr::Variable {
                    name: "value".into(),
                    line: 1,
                },
                expr: Expr::Integer(42),
            },
            Stmt::CoalesceAssign {
                target: Expr::ArrayAccess {
                    array: Box::new(Expr::Variable {
                        name: "items".into(),
                        line: 1,
                    }),
                    index: Box::new(Expr::Integer(1)),
                    line: 1,
                },
                expr: Expr::StringLiteral("fallback".into()),
            },
        ]
    );
}

#[test]
fn test_parse_foreach_value_reference() {
    let tokens = Lexer::new("<?php foreach ($items as $key => &$value) { $value += 1; }")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let Stmt::Foreach {
        value, key, by_ref, ..
    } = &stmts[0]
    else {
        panic!("expected foreach statement");
    };
    assert_eq!(value, &ForeachTarget::Variable("value".into()));
    assert_eq!(key, &Some(ForeachTarget::Variable("key".into())));
    assert!(*by_ref);
}

#[test]
fn test_parse_foreach_destructuring_target() {
    let tokens = Lexer::new("<?php foreach ($rows as $key => [$left, $right]) {}")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::Foreach {
            key: Some(ForeachTarget::Variable(key)),
            value: ForeachTarget::Destructure(targets),
            by_ref: false,
            ..
        } if key == "key" && targets == &vec![
            ListTarget::Variable("left".into()),
            ListTarget::Variable("right".into()),
        ]
    ));
}

#[test]
fn destructuring_spread_is_preserved_as_a_deferred_compile_error() {
    for source in [
        "<?php\n[$first, ...$remaining] = $row;",
        "<?php\nlist(...$remaining) = $row;",
        "<?php\nforeach ($rows as [$first, ...$remaining]) {}",
        "<?php\n[[$first, ...$remaining]] = $rows;",
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                if message == "Spread operator is not supported in assignments" && *line == 2
        ));
    }
}

#[test]
fn array_unpack_preserves_the_spread_line_for_compile_validation() {
    let tokens = Lexer::new("<?php\n$result = [\n    ...42,\n];")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        &statements[0],
        Stmt::Assign {
            expr: Expr::ArrayLiteral(elements),
            ..
        } if elements.len() == 1
            && elements[0].unpack
            && elements[0].unpack_line == Some(3)
    ));
}

#[test]
fn throwable_creation_and_throw_keep_distinct_source_lines_in_the_ast() {
    let tokens = Lexer::new("<?php\n$stored = new Exception();\nthrow $stored;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        &statements[0],
        Stmt::Assign {
            expr: Expr::New { line: 2, .. },
            ..
        }
    ));
    assert!(matches!(&statements[1], Stmt::Throw { line: 3, .. }));
}

#[test]
fn test_parse_nested_array_append() {
    let tokens = Lexer::new("<?php $store->listeners['event'][10][] = 'listener';")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::ArrayAppend {
            target: Expr::ArrayAccess { .. },
            expr: Expr::StringLiteral(value),
        } if value == "listener"
    ));
}

#[test]
fn excessive_mixed_syntax_nesting_reports_memory_exhaustion() {
    let pairs = 140;
    let source = format!(
        "<?php\nfunction shelter() {{\nreturn {}0{};\n}}",
        "([".repeat(pairs),
        "])".repeat(pairs),
    );
    let tokens = Lexer::new(&source).tokenize().unwrap();
    let error = Parser::new(tokens)
        .with_source_name("/fixture/nesting.php")
        .parse()
        .unwrap_err();

    assert_eq!(error, "memory exhausted in /fixture/nesting.php on line 3");
}

#[test]
fn ordinary_mixed_syntax_nesting_remains_accepted() {
    let pairs = 16;
    let source = format!(
        "<?php function shelter() {{ return {}0{}; }}",
        "([".repeat(pairs),
        "])".repeat(pairs),
    );
    let tokens = Lexer::new(&source).tokenize().unwrap();

    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_bind_appended_array_element_reference() {
    let tokens = Lexer::new("<?php $slot = &$store->items['group'][];")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::BindArrayAppendReference {
            var,
            target: Expr::ArrayAccess { .. },
        } if var == "slot"
    ));
}

#[test]
fn test_parse_closure_reference_captures() {
    let tokens = Lexer::new("<?php $fn = static function() use (&$left, &$right) {};")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let Stmt::Assign {
        expr: Expr::Closure { use_vars, .. },
        ..
    } = &stmts[0]
    else {
        panic!("expected closure assignment");
    };
    assert_eq!(
        use_vars,
        &vec![
            ("left".to_string(), true, 1),
            ("right".to_string(), true, 1),
        ]
    );
}

#[test]
fn test_parse_first_class_callable_and_argument_unpack() {
    let tokens = Lexer::new("<?php ($callable = $listener(...))(...$args);")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::ExprStmt(Expr::DynamicCall { args, .. })
            if matches!(args.as_slice(), [CallArg::Unpack(Expr::Variable { name, .. })] if name == "args")
    ));
}

#[test]
fn expression_static_method_calls_retain_class_and_method_operands() {
    let tokens = Lexer::new("<?php $provider?->target()::$method($argument);")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &statements[0],
        Stmt::ExprStmt(Expr::DynamicStaticCall {
            class,
            method,
            args,
            ..
        }) if matches!(class.as_ref(), Expr::MethodCall { nullsafe: true, method, .. } if method == "target")
            && matches!(method.as_ref(), Expr::Variable { name, .. } if name == "method")
            && matches!(args.as_slice(), [CallArg::Positional(Expr::Variable { name, .. })] if name == "argument")
    ));
}

#[test]
fn nullsafe_nested_write_target_is_a_deferred_compile_error() {
    let tokens = Lexer::new("<?php $foo?->bar->baz = sideEffect();")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
            if message == "Can't use nullsafe operator in write context" && *line == 1
    ));
}

#[test]
fn positional_source_argument_after_unpack_is_a_compile_time_error() {
    let tokens = Lexer::new("<?php dispatch(...$batch, 7);")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens).parse().unwrap_err();

    assert_eq!(
        error,
        "Cannot use positional argument after argument unpacking"
    );
}

#[test]
fn source_unpack_after_named_argument_is_a_compile_time_error() {
    let tokens = Lexer::new("<?php dispatch(mode: 'safe', ...$batch);")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens).parse().unwrap_err();

    assert_eq!(error, "Cannot use argument unpacking after named arguments");
}

#[test]
fn test_parse_named_first_class_function_callable() {
    let tokens = Lexer::new("<?php namespace App; $check = is_int(...);")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let Stmt::Namespace { body, .. } = &stmts[0] else {
        panic!("expected namespace statement");
    };
    assert!(matches!(
        &body[0],
        Stmt::Assign {
            expr: Expr::FirstClassFunctionCallable(name),
            ..
        } if name == "is_int"
    ));
}

#[test]
fn test_parse_array_element_reference_assignment_without_bitwise_ambiguity() {
    let tokens = Lexer::new("<?php $loops[$key][] = &$pathInLoop;")
        .tokenize()
        .unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_trailing_commas_in_call_arguments() {
    let source = "<?php sink(1,); sink(named: 2,); $object->run(3,); Thing::make(4,);";
    let tokens = Lexer::new(source).tokenize().unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_reference_returning_arrow_and_reference_call_assignment() {
    let source = "<?php $reference = &bind(fn &() => $object->value)();";
    let tokens = Lexer::new(source).tokenize().unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_reference_returning_named_function_and_methods() {
    let source = "<?php function &globalReference() { static $value; return $value; } class ReferenceMethods { public function &value() { static $value; return $value; } } interface ReferenceContract { public function &value(); } trait ReferenceTrait { public function &value() { static $value; return $value; } } enum ReferenceEnum { public function &value() { static $value; return $value; } } $anonymous = new class { public function &value() { static $value; return $value; } };";
    let tokens = Lexer::new(source).tokenize().unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_reference_assignment_to_dynamic_property() {
    let tokens = Lexer::new("<?php $object->$name = &$value;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &statements[0],
        Stmt::ExprStmt(Expr::AssignTargetReference { target, source })
            if matches!(target.as_ref(), Expr::DynamicPropertyAccess { .. })
                && matches!(source.as_ref(), Expr::Variable { name, .. } if name == "value")
    ));
}

#[test]
fn test_parse_unobserved_attribute_groups_without_hash_comment_confusion() {
    let source = "<?php #[Marker(values: [']'])] class C { public function run(#[Sensitive] string $value): void {} }";
    let tokens = Lexer::new(source).tokenize().unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_coalesce_assignment_on_comparison_rhs() {
    let tokens = Lexer::new("<?php $result = 0 > $value ??= 1;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let Stmt::Assign { expr, .. } = &statements[0] else {
        panic!("unexpected statement: {:?}", statements[0]);
    };
    let Expr::BinaryOp { right, .. } = expr else {
        panic!("unexpected assignment expression: {expr:?}");
    };
    assert!(matches!(right.as_ref(), Expr::CoalesceAssign { .. }));
}

#[test]
fn test_parse_empty_anonymous_class_ancestry() {
    let source =
        "<?php $error = new class('message') extends RuntimeException implements Throwable {};";
    let tokens = Lexer::new(source).tokenize().unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_scientific_float_without_decimal_point() {
    let tokens = Lexer::new("<?php echo 1e4, 2E-2;").tokenize().unwrap();
    assert!(
        tokens
            .iter()
            .any(|token| matches!(token, Token::Float(value) if *value == 10_000.0))
    );
    assert!(
        tokens
            .iter()
            .any(|token| matches!(token, Token::Float(value) if *value == 0.02))
    );
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_static_first_class_callable() {
    let tokens = Lexer::new("<?php $callable = self::handleError(...);")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::Assign {
            expr: Expr::FirstClassCallable(_),
            ..
        }
    ));
}

#[test]
fn test_parse_error_control_operator() {
    let tokens = Lexer::new("<?php @trigger_error('hidden');")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::ExprStmt(Expr::ErrorSuppress(inner))
            if matches!(inner.as_ref(), Expr::FunctionCall { name, .. } if name == "trigger_error")
    ));
}

#[test]
fn test_parse_keyword_method_name() {
    let tokens = Lexer::new("<?php $matcher->match('/health'); $renderer->include('view.php');")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        &stmts[0],
        Stmt::ExprStmt(Expr::MethodCall { method, .. }) if method == "match"
    ));
    assert!(matches!(
        &stmts[1],
        Stmt::ExprStmt(Expr::MethodCall { method, .. }) if method == "include"
    ));
}

#[test]
fn test_parse_keyword_method_declaration() {
    let tokens = Lexer::new(
        "<?php class Matcher { public function match(string $path): array { return []; } private function include(string $path): string { return $path; } }",
    )
    .tokenize()
    .unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_fully_qualified_compound_type_hints() {
    let tokens = Lexer::new(
        "<?php class Fixture { protected SessionInterface|\\Closure|null $session = null; protected static ?\\Closure $factory = null; public function run(\\DateTimeInterface $date): \\Stringable {} }",
    )
    .tokenize()
    .unwrap();
    Parser::new(tokens).parse().unwrap();
}

#[test]
fn test_parse_add() {
    let tokens = Lexer::new("<?php echo 20 + 22;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Integer(20)),
            right: Box::new(Expr::Integer(22)),
        }])]
    );
}

#[test]
fn test_parse_assignment_on_comparison_rhs() {
    let tokens = Lexer::new("<?php echo false !== $position = find_position();")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![Expr::BinaryOp {
            op: BinOp::NotIdentical,
            left: Box::new(Expr::Bool(false)),
            right: Box::new(Expr::Assign {
                var: "position".into(),
                expr: Box::new(Expr::FunctionCall {
                    name: "find_position".into(),
                    args: vec![],
                    generic_args: vec![],
                    line: 1,
                }),
            }),
        }])]
    );
}

#[test]
fn test_parse_assignment_on_logical_rhs() {
    let tokens = Lexer::new("<?php echo $enabled && $file = find_file();")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![Expr::BinaryOp {
            op: BinOp::And,
            left: Box::new(Expr::Variable {
                name: "enabled".into(),
                line: 1,
            }),
            right: Box::new(Expr::Assign {
                var: "file".into(),
                expr: Box::new(Expr::FunctionCall {
                    name: "find_file".into(),
                    args: vec![],
                    generic_args: vec![],
                    line: 1,
                }),
            }),
        }])]
    );
}

#[test]
fn test_parse_function_call() {
    let tokens = Lexer::new("<?php echo my_double(21);").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![Expr::FunctionCall {
            name: "my_double".into(),
            args: vec![CallArg::Positional(Expr::Integer(21))],
            generic_args: vec![],
            line: 1,
        }])]
    );
}

#[test]
fn test_parse_comma_separated_echo() {
    let tokens = Lexer::new("<?php echo 1, $value, 2 + 3;")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![
            Expr::Integer(1),
            Expr::Variable {
                name: "value".into(),
                line: 1,
            },
            Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(Expr::Integer(2)),
                right: Box::new(Expr::Integer(3)),
            },
        ])]
    );
}

#[test]
fn test_parse_standalone_print_statement() {
    let tokens = Lexer::new("<?php print 'value';").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::ExprStmt(Expr::Print(Box::new(Expr::StringLiteral(
            "value".into()
        ),)))]
    );
}

#[test]
fn test_parse_empty_statement() {
    let tokens = Lexer::new("<?php ; echo 1; ;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::Noop, echo(vec![Expr::Integer(1)]), Stmt::Noop,]
    );
}

#[test]
fn test_parse_general_expression_statements() {
    let tokens = Lexer::new("<?php (1 + 2); 42; 'unused'; ++$i; fn(): int => 1;")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();

    assert_eq!(stmts.len(), 5);
    assert!(stmts.iter().all(|stmt| matches!(stmt, Stmt::ExprStmt(_))));
}

#[test]
fn test_parse_abstract_method_contract() {
    let tokens = Lexer::new(
        "<?php abstract class Shape { abstract protected static function area(int $scale): int; }",
    )
    .tokenize()
    .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    let Stmt::Class {
        is_abstract,
        methods,
        ..
    } = &stmts[0]
    else {
        panic!("expected class declaration");
    };
    assert!(*is_abstract);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "area");
    assert_eq!(methods[0].visibility, Visibility::Protected);
    assert!(methods[0].is_static);
    assert!(methods[0].is_abstract);
    assert!(methods[0].body.is_empty());
}

#[test]
fn test_reject_invalid_abstract_method_modifiers() {
    let parse = |source: &str| {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    };

    assert!(
        parse("<?php class C { abstract public function run(); }")
            .unwrap_err()
            .contains("must therefore be declared abstract")
    );
    assert!(
        parse("<?php abstract class C { final abstract public function run(); }")
            .unwrap_err()
            .contains("final modifier on an abstract method")
    );
    assert!(
        parse("<?php abstract class C { abstract private function run(); }")
            .unwrap_err()
            .contains("cannot be declared private")
    );
}

#[test]
fn test_static_return_type_requires_a_real_class_scope() {
    let parse = |source: &str| {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    };

    assert_eq!(
        parse("<?php function invalid(): static {}").unwrap_err(),
        "Cannot use \"static\" when no class scope is active"
    );
    parse("<?php class C { public function valid(): static { return $this; } }").unwrap();
    parse(
        "<?php class C { public function valid(): static { $f = function(): static { return $this; }; return $f(); } }",
    )
    .unwrap();
    parse(
        "<?php class C { public function valid(): static { $f = static function(): static { return new static(); }; return $f(); } }",
    )
    .unwrap();
    assert_eq!(
        parse(
            "<?php class C { public function invalid(): static { function nested(): static {} return $this; } }",
        )
        .unwrap_err(),
        "Cannot use \"static\" when no class scope is active"
    );

    assert_eq!(
        parse("<?php static::value();").unwrap_err(),
        "Cannot use \"static\" when no class scope is active"
    );
    parse("<?php class C { public static function call() { return static::value(); } }").unwrap();
    parse(
        "<?php class C { public static $value = 1; public static function read() { return static::$value; } }",
    )
    .unwrap();
    parse(
        "<?php class C { public static $value = 1; public static function write() { static::$value = 2; static::$value += 1; } } C::$value = 3; C::$value .= 'x';",
    )
    .unwrap();
    assert_eq!(
        parse(
            "<?php class C { public static function call() { function nested() { return static::value(); } } }",
        )
        .unwrap_err(),
        "Cannot use \"static\" when no class scope is active"
    );
}

#[test]
fn test_static_anonymous_function_forms_are_expressions() {
    let tokens = Lexer::new(
        "<?php $closure = static function($value) { return $value; }; $arrow = static fn($value) => $value; static function() {}; static fn() => null;",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    let Stmt::Assign {
        expr: Expr::Closure { is_static, .. },
        ..
    } = &statements[0]
    else {
        panic!("expected a static closure assignment");
    };
    assert!(*is_static);
    let Stmt::Assign {
        expr: Expr::Closure { is_static, .. },
        ..
    } = &statements[1]
    else {
        panic!("expected a static arrow assignment");
    };
    assert!(*is_static);
    assert_eq!(statements.len(), 4);
}

#[test]
fn test_parse_if() {
    let tokens = Lexer::new("<?php if (1) echo 42;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::If {
            condition: Expr::Integer(1),
            then_body: vec![echo(vec![Expr::Integer(42)])],
            else_body: vec![],
        }]
    );
}

#[test]
fn test_parse_if_else() {
    let tokens = Lexer::new("<?php if (0) { echo 1; } else { echo 2; }")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::If {
            condition: Expr::Integer(0),
            then_body: vec![echo(vec![Expr::Integer(1)])],
            else_body: vec![echo(vec![Expr::Integer(2)])],
        }]
    );
}

#[test]
fn test_parse_while() {
    let tokens = Lexer::new("<?php while ($x < 3) { echo $x; }")
        .tokenize()
        .unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::While {
            condition: Expr::BinaryOp {
                op: BinOp::Less,
                left: Box::new(Expr::Variable {
                    name: "x".into(),
                    line: 1,
                }),
                right: Box::new(Expr::Integer(3)),
            },
            body: vec![echo(vec![Expr::Variable {
                name: "x".into(),
                line: 1,
            }])],
        }]
    );
}
