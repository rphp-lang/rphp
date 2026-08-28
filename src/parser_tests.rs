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
fn foreach_key_reference_is_a_located_deferred_compile_error() {
    let tokens =
        Lexer::new("<?php\n$items = [1];\nforeach (\n    $items as\n    &$key => $value\n) {}")
            .tokenize()
            .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 4 }))
            if message == "Key element cannot be a reference"
    ));
    assert!(matches!(
        &statements[1],
        Stmt::Foreach {
            key: Some(ForeachTarget::Variable(key)),
            value: ForeachTarget::Variable(value),
            by_ref: false,
            ..
        } if key == "key" && value == "value"
    ));
}

#[test]
fn foreach_alias_keyword_is_case_insensitive_without_reclassifying_names() {
    let tokens = Lexer::new(
        "<?php class Names { const AS = 'name'; } foreach ([1] AS $value) {} echo Names::AS;",
    )
    .tokenize()
    .unwrap();
    Parser::new(tokens).parse().unwrap();
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
fn array_append_reference_uses_the_reference_ast_without_displacing_plain_pushes() {
    let tokens = Lexer::new("<?php $items[] =& $source; $items[] = $value;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        &statements[0],
        Stmt::ExprStmt(Expr::ArrayAppendAssign {
            target,
            expr,
            by_ref: true,
        }) if matches!(target.as_ref(), Expr::Variable { name, .. } if name == "items")
            && matches!(expr.as_ref(), Expr::Variable { name, .. } if name == "source")
    ));
    assert!(matches!(
        &statements[1],
        Stmt::ArrayPush {
            var,
            expr: Expr::Variable { name, .. },
            ..
        } if var == "items" && name == "value"
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
    let tokens = Lexer::new("<?php $fn = static function() use (&$left, &$right,) {};")
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
fn parenthesized_static_property_retains_value_call_boundary() {
    let tokens = Lexer::new("<?php parent::$prop::get(); (parent::$prop)::get();")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    let parenthesized = |statement: &Stmt| match statement {
        Stmt::ExprStmt(Expr::DynamicStaticCall { class, .. }) => match class.as_ref() {
            Expr::StaticProperty { parenthesized, .. } => Some(*parenthesized),
            _ => None,
        },
        _ => None,
    };
    assert_eq!(parenthesized(&statements[0]), Some(false));
    assert_eq!(parenthesized(&statements[1]), Some(true));
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
fn nullsafe_forbidden_contexts_are_deferred_compile_errors() {
    for (source, expected) in [
        (
            "<?php $ref =& $foo?->bar()->baz;",
            "Cannot take reference of a nullsafe chain",
        ),
        (
            "<?php unset($foo?->bar->baz);",
            "Can't use nullsafe operator in write context",
        ),
        (
            "<?php foreach ([1] as $foo?->bar) {}",
            "Can't use nullsafe operator in write context",
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == expected && *line == 1
            ),
            "unexpected AST for {source}: {statements:#?}"
        );
    }
}

#[test]
fn pipe_requires_parentheses_around_an_arrow_rhs() {
    let tokens = Lexer::new("<?php\n42 |> fn($value) => $value;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
            if message == "Arrow functions on the right hand side of |> must be parenthesized"
                && *line == 2
    ));

    let tokens = Lexer::new("<?php\n42 |> (fn($value) => $value);")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(!matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { .. }))
    ));
}

#[test]
fn pipe_and_call_results_in_destructuring_are_deferred_write_errors() {
    for source in [
        "<?php\nlist(identity() ) = $source;",
        "<?php\nlist(input |> identity(...)) = $source;",
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                if message == "Can't use function return value in write context" && *line == 2
        ));
    }
}

#[test]
fn incdec_call_results_are_deferred_function_or_method_write_errors() {
    for (expression, expected) in [
        ("++named();", "function"),
        ("named()--;", "function"),
        ("++$callable();", "function"),
        ("[new Box(), 'method']()++;", "function"),
        ("--$object->method();", "method"),
        ("$object->method()++;", "method"),
        ("++$object->$method();", "method"),
        ("Box::method()--;", "method"),
        ("--$class::$method();", "method"),
    ] {
        let source = format!("<?php\nif (false) {{\n    {expression}\n}}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == &format!(
                        "Can't use {expected} return value in write context"
                    ) && *line == 3
            ),
            "unexpected AST for {expression}: {statements:#?}"
        );
    }
}

#[test]
fn call_results_are_deferred_write_errors_across_assignment_unset_and_foreach() {
    for (expression, expected) in [
        ("named() = isset(named());", "function"),
        ("named() =& $target;", "function"),
        ("named() ??= isset(named());", "function"),
        ("named() += isset(named());", "function"),
        ("unset(named());", "function"),
        ("foreach ($values as named()) {}", "function"),
        ("foreach ($values as named() => $value) {}", "function"),
        ("foreach ($values as &named()) {}", "function"),
        ("$callable() = 1;", "function"),
        ("$object->method() = 1;", "method"),
        ("Box::method() ??= 1;", "method"),
        ("$class::$method() += 1;", "method"),
        ("unset($object->method());", "method"),
        ("foreach ($values as $object->method()) {}", "method"),
        ("list($object->method()) = $source;", "method"),
        ("list(Box::method()) = $source;", "method"),
    ] {
        let source = format!("<?php\nif (false) {{\n    {expression}\n}}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == &format!(
                        "Can't use {expected} return value in write context"
                    ) && *line == 3
            ),
            "unexpected AST for {expression}: {statements:#?}"
        );
    }
}

#[test]
fn dimensions_and_properties_of_call_results_remain_writable_targets() {
    let tokens = Lexer::new(
        "<?php\nresult()[0] = 1; result()->property = 3; \
         unset(result()[0]); foreach ($values as result()[0]) {}",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(!matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { .. }))
    ));
}

#[test]
fn braced_dynamic_nullsafe_property_retains_its_short_circuit_flag() {
    let tokens = Lexer::new("<?php $object?->{$property};")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        statements.as_slice(),
        [Stmt::ExprStmt(Expr::DynamicPropertyAccess {
            object,
            property,
            nullsafe: true,
            line: 1,
        })]
            if matches!(object.as_ref(), Expr::Variable { name, .. } if name == "object")
                && matches!(property.as_ref(), Expr::Variable { name, .. } if name == "property")
    ));
}

#[test]
fn braced_dynamic_nullsafe_method_remains_an_explicit_boundary() {
    let tokens = Lexer::new("<?php $object?->{$method}();")
        .tokenize()
        .unwrap();
    assert_eq!(
        Parser::new(tokens).parse().unwrap_err(),
        "Dynamic nullsafe method calls are not supported yet"
    );
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
fn malformed_numeric_separators_report_the_source_identifier_and_line() {
    let cases = [
        ("100_", "_"),
        ("10__0", "__0"),
        ("100_.0", "_"),
        ("100._0", "_0"),
        ("0x_0123", "x_0123"),
        ("0b_0101", "b_0101"),
        ("1_e2", "_e2"),
        ("1e_2", "e_2"),
        ("0x0__F", "__F"),
        ("0b0__1", "__1"),
    ];

    for (literal, identifier) in cases {
        let source = format!("<?php\n{literal};");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/fixture/numeric-separator.php")
            .parse()
            .unwrap_err();

        assert_eq!(
            error,
            format!(
                "syntax error, unexpected identifier \"{identifier}\" in /fixture/numeric-separator.php on line 2"
            )
        );
    }
}

#[test]
fn group_use_comma_diagnostics_follow_the_active_item_state() {
    let cases = [
        (
            "use Vendor\\{};",
            "syntax error, unexpected token \"}\", expecting identifier or namespaced name or \"function\" or \"const\"",
        ),
        (
            "use function Vendor\\{};",
            "syntax error, unexpected token \"}\", expecting identifier or namespaced name",
        ),
        (
            "use Vendor\\{,Name};",
            "syntax error, unexpected token \",\", expecting identifier or namespaced name or \"function\" or \"const\"",
        ),
        (
            "use const Vendor\\{,NAME};",
            "syntax error, unexpected token \",\", expecting identifier or namespaced name",
        ),
        (
            "use Vendor\\{function ,};",
            "syntax error, unexpected token \",\", expecting identifier or namespaced name",
        ),
        (
            "use Vendor\\{Name,,Other};",
            "syntax error, unexpected token \",\", expecting \"}\"",
        ),
    ];

    for (declaration, expected) in cases {
        let source = format!("<?php\n{declaration}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/fixture/group-use.php")
            .parse()
            .unwrap_err();

        assert_eq!(
            error,
            format!("{expected} in /fixture/group-use.php on line 2")
        );
    }

    let tokens = Lexer::new("<?php use Vendor\\{Name, function helper, const VALUE,};")
        .tokenize()
        .unwrap();
    assert!(Parser::new(tokens).parse().is_ok());
}

#[test]
fn call_like_comma_lists_distinguish_leading_trailing_and_double_commas() {
    for (expression, expected) in [
        (
            "dispatch(, $value);",
            "syntax error, unexpected token \",\"",
        ),
        (
            "dispatch($value,, $other);",
            "syntax error, unexpected token \",\", expecting \")\"",
        ),
        (
            "dispatch(value: $value,,);",
            "syntax error, unexpected token \",\", expecting \")\"",
        ),
        (
            "dispatch(...$values,,);",
            "syntax error, unexpected token \",\", expecting \")\"",
        ),
        ("isset(, $value);", "syntax error, unexpected token \",\""),
        (
            "isset($value,, $other);",
            "syntax error, unexpected token \",\", expecting \")\"",
        ),
        ("unset(, $value);", "syntax error, unexpected token \",\""),
        (
            "unset($value,, $other);",
            "syntax error, unexpected token \",\", expecting \")\"",
        ),
    ] {
        let source = format!("<?php\n{expression}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/fixture/comma-list.php")
            .parse()
            .unwrap_err();
        assert_eq!(
            error,
            format!("{expected} in /fixture/comma-list.php on line 2")
        );
    }

    let tokens = Lexer::new(
        "<?php dispatch($value,); dispatch(value: $value,); dispatch(...$values,); \
         isset($value,); unset($value,);",
    )
    .tokenize()
    .unwrap();
    assert!(Parser::new(tokens).parse().is_ok());
}

#[test]
fn invalid_isset_results_are_deferred_compile_errors() {
    let tokens = Lexer::new("<?php\nif (false) { isset($valid, compute()); }")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 2 }))
            if message == "Cannot use isset() on the result of an expression (you can use \"null !== expression\" instead)"
    ));
}

#[test]
fn removed_unset_cast_is_a_deferred_compile_error_in_all_expression_contexts() {
    for source in [
        "<?php\n$value = (unset) source();",
        "<?php\n$value = (UnSeT) source();",
        "<?php\nif (false) { $value = (unset) source(); }",
        "<?php\nclass C { public $value = (unset) C::class; }",
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();

        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line: 2 }))
                if message == "The (unset) cast is no longer supported"
        ));
    }
}

#[test]
fn positional_after_named_is_a_deferred_compile_error() {
    let tokens = Lexer::new("<?php\nif (false) {\n    dispatch(first: 1, $second, third: 3);\n}")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 3 }))
            if message == "Cannot use positional argument after named argument"
    ));
}

#[test]
fn literal_this_write_targets_are_deferred_compile_errors() {
    for statement in [
        "$this = replacement();",
        "$this = isset(replacement());",
        "$this =& $replacement;",
        "$this ??= replacement();",
        "$this ??= isset(replacement());",
        "foreach ($values as $this) {}",
        "foreach ($values as $this => $value) {}",
        "foreach ($values as &$this) {}",
        "foreach ($values as list($this)) {}",
        "foreach ($values as [&$this]) {}",
        "try {} catch (Exception $this) {}",
    ] {
        let source = format!("<?php\nif (false) {{\n    {statement}\n}}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();

        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line: 3 }))
                    if message == "Cannot re-assign $this"
            ),
            "unexpected AST for {statement}: {statements:#?}"
        );
    }
}

#[test]
fn literal_this_global_and_lexical_bindings_are_deferred_compile_errors() {
    for (source, expected_message, expected_line) in [
        (
            "<?php\nfunction invalid() {\n    global $this;\n}",
            "Cannot use $this as global variable",
            3,
        ),
        (
            "<?php\n$invalid = function () use (\n    $this\n) {};",
            "Cannot use $this as lexical variable",
            3,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();

        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == expected_message && *line == expected_line
            ),
            "unexpected AST for {source}: {statements:#?}"
        );
    }
}

#[test]
fn duplicate_switch_and_match_defaults_are_deferred_compile_errors() {
    for (source, expected_message, expected_line) in [
        (
            "<?php\nswitch (1) {\n    default: break;\n    default: break;\n}",
            "Switch statements may only contain one default clause",
            4,
        ),
        (
            "<?php\n$value = match (1) {\n    default => 'first',\n    default => 'second',\n};",
            "Match expressions may only contain one default arm",
            4,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();

        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == expected_message && *line == expected_line
            ),
            "unexpected AST for {source}: {statements:#?}"
        );
    }
}

#[test]
fn use_declarations_preserve_first_name_line_and_explicit_alias_provenance() {
    let tokens = Lexer::new("<?php\nuse\n    Plain,\n    Same as Same;")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        &statements[0],
        Stmt::UseDecl {
            line: 2,
            name_line: 3,
            imports,
        } if imports == &vec![
            (UseKind::Class, "Plain".to_string(), "Plain".to_string(), false),
            (UseKind::Class, "Same".to_string(), "Same".to_string(), true),
        ]
    ));
}

#[test]
fn misplaced_strict_types_declarations_are_deferred_compile_errors() {
    for (source, expected_line) in [
        ("<?php\nfunction earlier() {}\ndeclare(strict_types=1);", 3),
        ("<?php\nnamespace Example;\ndeclare(strict_types=1);", 3),
        (
            "<?php\nfunction earlier() {}\ndeclare(strict_types=1) {\n    isset(compute());\n}",
            3,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();

        assert!(
            matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                    if message == "strict_types declaration must be the very first statement in the script"
                        && *line == expected_line
            ),
            "unexpected AST for {source}: {statements:#?}"
        );
    }
}

#[test]
fn first_strict_types_and_non_strict_declarations_remain_valid() {
    for source in [
        "<?php\ndeclare(strict_types=1);\nfunction strict() {}",
        "<?php\ndeclare(strict_types=0);\nfunction weak() {}",
        "<?php\ndeclare(ticks=1);\nfunction ticked() {}",
        "<?php\n;\ndeclare(strict_types=1);",
        "<?php\ndeclare(ticks=1);\ndeclare(strict_types=1);",
        "<?php\ndeclare(strict_types=1);\ndeclare(strict_types=1);",
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(
            !matches!(
                statements.last(),
                Some(Stmt::ExprStmt(Expr::CompileError { .. }))
            ),
            "unexpected compile error for {source}: {statements:#?}"
        );
    }
}

#[test]
fn first_strict_types_block_is_a_deferred_compile_error() {
    let tokens = Lexer::new("<?php\ndeclare(strict_types=1) {\n    isset(compute());\n}")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 2 }))
            if message == "strict_types declaration must not use block mode"
    ));
}

#[test]
fn noncanonical_cast_deprecations_survive_dead_code_and_real_is_removed() {
    let tokens = Lexer::new(
        "<?php\nif (false) { (integer) '42'; }\n(binary) 42;\n(boolean) 42;\n(double) 42;",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let diagnostics: Vec<_> = statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::ExprStmt(Expr::CompileDeprecation { message, line }) => {
                Some((message.as_str(), *line))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        diagnostics,
        [
            (
                "Non-canonical cast (integer) is deprecated, use the (int) cast instead",
                2,
            ),
            (
                "Non-canonical cast (binary) is deprecated, use the (string) cast instead",
                3,
            ),
            (
                "Non-canonical cast (boolean) is deprecated, use the (bool) cast instead",
                4,
            ),
            (
                "Non-canonical cast (double) is deprecated, use the (float) cast instead",
                5,
            ),
        ]
    );

    let tokens = Lexer::new("<?php\n(real) 42;").tokenize().unwrap();
    let error = Parser::new(tokens)
        .with_source_name("/virtual/removed-real-cast.php")
        .parse()
        .unwrap_err();
    assert_eq!(
        error,
        "The (real) cast has been removed, use (float) instead in /virtual/removed-real-cast.php on line 2"
    );
}

#[test]
fn removed_curly_string_offsets_have_contextual_parse_diagnostics() {
    for (source, expected) in [
        (
            "<?php\n$value = 'text';\nconsume($value\n{0});",
            "syntax error, unexpected token \"{\", expecting \")\" in /virtual/curly-offset.php on line 4",
        ),
        (
            "<?php\nconst VALUE = 'text'\n{0};",
            "syntax error, unexpected token \"{\", expecting \",\" or \";\" in /virtual/curly-offset.php on line 3",
        ),
        (
            "<?php\n\"{$value\n{'key'}}\";",
            "syntax error, unexpected token \"{\", expecting \"->\" or \"?->\" or \"[\" in /virtual/curly-offset.php on line 3",
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/virtual/curly-offset.php")
            .parse()
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn bracket_offsets_blocks_and_dynamic_interpolated_properties_remain_valid() {
    for source in [
        "<?php $value = 'text'; consume($value[0]);",
        "<?php const VALUE = 'text'[0];",
        "<?php if (true) { echo 'block'; }",
        "<?php \"{$object->{$property}}\";",
        "<?php \"{$value[0]}\";",
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens)
            .parse()
            .unwrap_or_else(|error| panic!("unexpected parse error for {source}: {error}"));
    }
}

#[test]
fn readonly_classlike_modifiers_have_php_declaration_diagnostics() {
    for (keyword, expected) in [
        ("enum", "enum"),
        ("interface", "interface"),
        ("trait", "trait"),
    ] {
        let source = format!("<?php\nreadonly {keyword} Example {{}}");
        let tokens = Lexer::new(&source).tokenize().unwrap();
        let error = Parser::new(tokens)
            .with_source_name("/virtual/readonly-declaration.php")
            .parse()
            .unwrap_err();
        assert_eq!(
            error,
            format!(
                "syntax error, unexpected token \"{expected}\", expecting \"abstract\" or \"final\" or \"readonly\" or \"class\" in /virtual/readonly-declaration.php on line 2"
            )
        );
    }

    for (source, message, line) in [
        (
            "<?php\nreadonly\nreadonly class Example {}",
            "Multiple readonly modifiers are not allowed",
            3,
        ),
        (
            "<?php\nclass Example {\nreadonly\nconst VALUE = 1;\n}",
            "Cannot use the readonly modifier on a class constant",
            3,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError {
                message: actual,
                line: actual_line,
            })) if actual == message && *actual_line == line
        ));
    }
}

#[test]
fn readonly_method_modifiers_are_located_deferred_compile_errors() {
    for (source, expected_line) in [
        (
            "<?php\nclass Example {\nreadonly\nfunction method() {}\n}",
            3,
        ),
        (
            "<?php\nclass Example {\nuse MissingTrait { method as\nreadonly; }\n}",
            4,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line }))
                if message == "Cannot use the readonly modifier on a method"
                    && *line == expected_line
        ));
    }
}

#[test]
fn source_aware_punctuation_keeps_internal_fallbacks_and_trait_diagnostics() {
    let tokens = Lexer::new("<?php declare(ticks=1) {}").tokenize().unwrap();
    assert_eq!(
        Parser::new(tokens).parse().unwrap_err(),
        "Expected Semicolon, got LBrace"
    );

    let tokens = Lexer::new("<?php trait Example extends Base {}")
        .tokenize()
        .unwrap();
    assert_eq!(
        Parser::new(tokens).parse().unwrap_err(),
        "syntax error, unexpected token \"extends\", expecting \"{\" on line 1"
    );

    let tokens = Lexer::new("<?php declare(ticks=1) ?").tokenize().unwrap();
    assert_eq!(
        Parser::new(tokens).parse().unwrap_err(),
        "Expected Semicolon, got Question"
    );
}

#[test]
fn document_string_parse_tokens_receive_the_parser_source_location() {
    let tokens = Lexer::new("<?php\necho <<<DOC\n  first\nsecond\n  DOC;")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens)
        .with_source_name("/fixture/document.php")
        .parse()
        .unwrap_err();

    assert_eq!(
        error,
        "Invalid body indentation level (expecting an indentation level of at least 2) in /fixture/document.php on line 4"
    );
}

#[test]
fn standalone_document_string_errors_use_the_same_source_diagnostic() {
    let tokens = Lexer::new("<?php\n<<<DOC\n\\tvalue\n DOC);")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens)
        .with_source_name("/fixture/standalone-document.php")
        .parse()
        .unwrap_err();

    assert_eq!(
        error,
        "Invalid body indentation level (expecting an indentation level of at least 1) in /fixture/standalone-document.php on line 3"
    );
}

#[test]
fn document_start_errors_win_over_synthetic_call_parentheses() {
    let tokens = Lexer::new("<?php\n$value = factory<<<DOC\nbody\nDOC;")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens)
        .with_source_name("/fixture/adjacent-document.php")
        .parse()
        .unwrap_err();

    assert_eq!(
        error,
        "syntax error, unexpected heredoc start \"<<<DOC\" in /fixture/adjacent-document.php on line 2"
    );
}

#[test]
fn source_less_parser_keeps_structural_unexpected_identifier_errors() {
    let tokens = Lexer::new("<?php 100_;").tokenize().unwrap();

    assert_eq!(
        Parser::new(tokens).parse().unwrap_err(),
        "Expected Semicolon, got Identifier(\"_\", 1)"
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
            expr: Expr::FirstClassFunctionCallable { name, line: 1 },
            ..
        } if name == "is_int"
    ));
}

#[test]
fn dynamic_and_namespace_relative_first_class_callables_keep_source_lines() {
    let tokens = Lexer::new(
        "<?php\nnamespace Fixture;\n$local = namespace\\run(...);\n$class = Target::class;\n$method = 'run';\n$static = $class::$method(...);\n$target = new Target;\n$instance = $target->$method(...);",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let Stmt::Namespace { body, .. } = &statements[0] else {
        panic!("expected namespace statement");
    };

    assert!(matches!(
        &body[0],
        Stmt::Assign {
            expr: Expr::FirstClassFunctionCallable { name, line: 3 },
            ..
        } if name == "namespace\\run"
    ));
    assert!(matches!(
        &body[3],
        Stmt::Assign {
            expr: Expr::FirstClassCallable { line: 6, .. },
            ..
        }
    ));
    assert!(matches!(
        &body[5],
        Stmt::Assign {
            expr: Expr::FirstClassCallable { line: 8, .. },
            ..
        }
    ));
}

#[test]
fn forbidden_new_and_nullsafe_first_class_callables_are_compile_errors() {
    for (source, expected) in [
        (
            "<?php\nnew Example(...);",
            "Cannot create Closure for new expression",
        ),
        (
            "<?php\n$class = 'Example'; new $class(...);",
            "Cannot create Closure for new expression",
        ),
        (
            "<?php\n$object?->method(...);",
            "Cannot combine nullsafe operator with Closure creation",
        ),
        (
            "<?php\n$object?->{$method}(...);",
            "Cannot combine nullsafe operator with Closure creation",
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line: 2 }))
                if message == expected
        ));
    }
}

#[test]
fn catch_types_accept_namespace_relative_names_in_every_union_position() {
    let tokens = Lexer::new(
        "<?php namespace Fixture; try {} catch (namespace\\First|\\RuntimeException|namespace\\Last $error) {} try {} catch (namespace\\Silent) {}",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let Stmt::Namespace { body, .. } = &statements[0] else {
        panic!("expected namespace statement");
    };

    let Stmt::TryCatch { catches, .. } = &body[0] else {
        panic!("expected first try/catch");
    };
    assert_eq!(
        catches[0].types,
        ["namespace\\First", "\\RuntimeException", "namespace\\Last"]
    );
    assert_eq!(catches[0].var.as_deref(), Some("error"));

    let Stmt::TryCatch { catches, .. } = &body[1] else {
        panic!("expected second try/catch");
    };
    assert_eq!(catches[0].types, ["namespace\\Silent"]);
    assert_eq!(catches[0].var, None);
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
fn declaration_attributes_are_retained_on_members_and_parameters() {
    let source = "<?php #[First(1), Second(name: 'class')] class Subject { #[Member('constant')] public const TOKEN = 1; #[Member('property')] public string $value; #[Member('method')] public function run(#[Member('parameter')] $input): void {} } #[Top('function')] function helper() {} #[Top('constant')] const GLOBAL_TOKEN = 1;";
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();

    let Stmt::Class {
        attributes,
        properties,
        constants,
        methods,
        ..
    } = &statements[0]
    else {
        panic!("expected attributed class");
    };
    assert_eq!(
        attributes
            .iter()
            .map(|attribute| attribute.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(properties[0].attributes[0].name, "Member");
    assert_eq!(constants[0].attributes[0].name, "Member");
    assert_eq!(methods[0].attributes[0].name, "Member");
    assert_eq!(methods[0].params[0].attributes[0].name, "Member");
    assert!(matches!(
        &statements[1],
        Stmt::Function { attributes, .. } if attributes[0].name == "Top"
    ));
    assert!(matches!(
        &statements[2],
        Stmt::Const { attributes, .. } if attributes[0].name == "Top"
    ));
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
            expr: Expr::FirstClassCallable { .. },
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
fn semi_reserved_members_and_trait_adaptations_parse_contextually() {
    let source = r#"<?php
namespace Fixture;
trait Primary {
    public function try() {}
    public function insteadof() {}
}
trait Secondary {
    public function insteadof() {}
}
trait Nested {
    use Primary { try as and; }
}
class Keywords {
    use Primary, Secondary {
        Primary::insteadof insteadof namespace\Secondary;
        try as public or;
    }
    var $keyword = 'legacy';
    public function and() {}
    public static function throw() {}
    public function __CLASS__() {}
}
$anonymous = new class {
    use Primary { try as or; }
};
enum Choice {
    use Primary { try as insteadof; }
}
(new Keywords())->and();
(new Keywords())->or();
Keywords::throw();
(new Keywords())->__CLASS__();
"#;

    let tokens = Lexer::new(source).tokenize().unwrap();
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
fn binary_operators_bind_an_assignment_inside_their_right_operand() {
    let tokens = Lexer::new("<?php echo 1 + $value = 3;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![echo(vec![Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(Expr::Assign {
                var: "value".into(),
                expr: Box::new(Expr::Integer(3)),
            }),
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
fn test_abstract_method_modifier_boundaries() {
    let parse = |source: &str| {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    };

    assert!(
        parse("<?php class C { abstract public function run(); }")
            .unwrap_err()
            .contains("must therefore be declared abstract")
    );
    let statements =
        parse("<?php abstract class C { final abstract public function run(); }").unwrap();
    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 1 }))
            if message == "Cannot use the final modifier on an abstract method"
    ));
    assert!(
        parse("<?php abstract class C { abstract private function run(); }")
            .unwrap_err()
            .contains("cannot be declared private")
    );

    let statements = parse("<?php trait T { abstract private function run(self $value): self; }")
        .expect("traits may declare private abstract method requirements");
    let Stmt::Trait { methods, .. } = &statements[0] else {
        panic!("expected trait declaration");
    };
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "run");
    assert_eq!(methods[0].visibility, Visibility::Private);
    assert!(methods[0].is_abstract);
    assert!(methods[0].body.is_empty());
}

#[test]
fn test_relative_static_scope_is_deferred_to_the_right_phase() {
    let parse = |source: &str| {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    };
    let assert_static_return_error = |statements: Vec<Stmt>| {
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, .. }))
                if message == "Cannot use \"static\" when no class scope is active"
        ));
    };

    assert_static_return_error(parse("<?php function invalid(): static {}").unwrap());
    parse("<?php class C { public function valid(): static { return $this; } }").unwrap();
    parse(
        "<?php class C { public function valid(): static { $f = function(): static { return $this; }; return $f(); } }",
    )
    .unwrap();
    parse(
        "<?php class C { public function valid(): static { $f = static function(): static { return new static(); }; return $f(); } }",
    )
    .unwrap();
    assert_static_return_error(
        parse(
            "<?php class C { public function invalid(): static { function nested(): static {} return $this; } }",
        )
        .unwrap(),
    );
    parse("<?php $closure = function (): static {}; $arrow = fn(): static => new static;").unwrap();

    parse("<?php static::value(); static::$value; static::VALUE; new static;").unwrap();
    parse("<?php class C { public static function call() { return static::value(); } }").unwrap();
    parse(
        "<?php class C { public static $value = 1; public static function read() { return static::$value; } }",
    )
    .unwrap();
    parse(
        "<?php class C { public static $value = 1; public static function write() { static::$value = 2; static::$value += 1; } } C::$value = 3; C::$value .= 'x';",
    )
    .unwrap();
    parse(
        "<?php class C { public static function call() { function nested() { return static::value(); } } }",
    )
    .unwrap();
}

#[test]
fn test_static_parameter_and_property_diagnostics_keep_their_php_stage() {
    let parse = |source: &str| {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    };

    for source in [
        "<?php class C { function invalid(static $value) {} }",
        "<?php class C { function invalid(C|static $value) {} }",
    ] {
        let statements = parse(source).expect("parameter diagnostics are compile errors");
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, .. }))
                if message == "Cannot use the static modifier on a parameter"
        ));
    }

    for source in [
        "<?php class C { function invalid(?static $value) {} }",
        "<?php class C { public ?static $value; }",
    ] {
        assert_eq!(
            parse(source).unwrap_err(),
            "syntax error, unexpected token \"static\" on line 1"
        );
    }
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

#[test]
fn forbidden_assert_and_first_class_new_forms_are_deferred_compile_errors() {
    for (source, expected, line) in [
        (
            "<?php\nnamespace Example; function assert() {}",
            "Defining a custom assert() function is not allowed, as the function has special semantics",
            2,
        ),
        (
            "<?php\nif (false) { new class(...) {}; }",
            "Cannot create Closure for new expression",
            2,
        ),
        (
            "<?php\n#[Example(...)] class Subject {}",
            "Cannot create Closure as attribute argument",
            2,
        ),
    ] {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            statements.last(),
            Some(Stmt::ExprStmt(Expr::CompileError { message, line: actual }))
                if message == expected && *actual == line
        ));
    }
}

#[test]
fn asymmetric_property_visibility_is_retained_separately_for_reads_and_writes() {
    let tokens = Lexer::new(
        "<?php class Box { public private(set) int $value; protected(set) string $label; }",
    )
    .tokenize()
    .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let Stmt::Class { properties, .. } = &statements[0] else {
        panic!("expected class declaration");
    };
    assert_eq!(properties[0].visibility, Visibility::Public);
    assert_eq!(properties[0].set_visibility, Some(Visibility::Private));
    assert_eq!(properties[1].visibility, Visibility::Public);
    assert_eq!(properties[1].set_visibility, Some(Visibility::Protected));
    assert_eq!(properties[0].line, 1);
}

#[test]
fn duplicate_asymmetric_visibility_is_a_deferred_compile_error() {
    let tokens =
        Lexer::new("<?php\nclass Box {\n    public private(set) protected(set) int $value;\n}")
            .tokenize()
            .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        statements.last(),
        Some(Stmt::ExprStmt(Expr::CompileError { message, line: 3 }))
            if message == "Multiple access type modifiers are not allowed"
    ));
}
