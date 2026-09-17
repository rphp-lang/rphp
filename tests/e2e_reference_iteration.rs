mod common;

#[test]
fn aggregate_generator_keeps_its_last_array_element_alias() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/method.php")),
        "[[14,60],[7,9]]\n75\n[14,75]\n"
    );
}

#[test]
fn reference_yield_empty_assignment_scope_and_notice_edges() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/edges.php")),
        concat!(
            "NULL\nretained\nlocal:8\n[13]\nsource:13\nreused:10\n[10,10]\n",
            "notice:Only variable references should be yielded by reference\ncall:4\nnested:4\n",
            "outer:publication\nbool(false)\n"
        )
    );
}

#[test]
fn nullsafe_property_yield_is_a_compile_error_even_in_dead_code() {
    for expression in ["$source?->value", "$source?->value[0]", "$source?->{$name}"] {
        let source = format!(
            "<?php\nfunction &invalid($source) {{\nif (false) {{ yield {expression}; }}\n}}"
        );
        let tokens = rphp::lexer::Lexer::new(&source).tokenize().unwrap();
        let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
        let error = rphp::compiler::compile::Compiler::new()
            .compile(&statements)
            .err()
            .expect("nullsafe properties cannot supply a reference");
        assert!(
            error.contains("Cannot take reference of a nullsafe chain"),
            "{error}"
        );
    }
}

#[test]
fn yielded_cells_preserve_array_cow_and_local_identity() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/cells.php")),
        "head:11\ntail:23\n[[18,42],[11,23]]\nresume:8\nlast:11\n"
    );
}

#[test]
fn reference_calls_and_nonvariable_notices_precede_suspension() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/calls.php")),
        concat!(
            "value:13\ncall\nnotice:Only variable references should be yielded by reference\nvalue:27\n",
            "notice:Only variable references should be yielded by reference\nvalue:30\n",
            "notice:Only variable references should be yielded by reference\nvalue:41\n",
            "source:14\nblocked yield\nbool(false)\n"
        )
    );
}

#[test]
fn yielded_property_alias_keeps_type_constraints() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/typed.php")),
        "Cannot assign array to reference held by property TypedCells::$second of type int\n19:31:31\n50\n"
    );
}

#[test]
fn yield_key_value_order_and_reentry_match_php() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/order.php")),
        concat!(
            "key\nvalue\nchosen:7\nresumed:9\n",
            "You can only iterate a generator by-reference if it declared that it yields by-reference\n",
            "Cannot resume an already running generator\nfinal:11\n"
        )
    );
}

#[test]
fn suspended_reference_retirement_and_collection_keep_owned_cells() {
    assert_eq!(
        common::run_php(include_str!("fixtures/reference_iteration/retirement.php")),
        "detached\n[\"kept\",\"updated\"]\ndone\n12:21\n[44,44]\n"
    );
}

#[test]
fn reference_generator_delegation_is_rejected_even_in_dead_code() {
    let tokens = rphp::lexer::Lexer::new(include_str!(
        "fixtures/reference_iteration/invalid-delegation.php"
    ))
    .tokenize()
    .unwrap();
    let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
    let error = rphp::compiler::compile::Compiler::new()
        .compile(&statements)
        .err()
        .expect("reference delegation must fail at compilation");
    assert!(
        error
            .to_string()
            .contains("Cannot use \"yield from\" inside a by-reference generator"),
        "{error}"
    );
}
