use super::*;
use crate::lexer::Lexer;

#[test]
fn test_parse_echo_42() {
    let tokens = Lexer::new("<?php echo 42;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(stmts, vec![Stmt::Echo(Expr::Integer(42))]);
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
            Stmt::Echo(Expr::Variable("a".into())),
        ]
    );
}

#[test]
fn test_parse_add() {
    let tokens = Lexer::new("<?php echo 20 + 22;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::Echo(Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Integer(20)),
            right: Box::new(Expr::Integer(22)),
        })]
    );
}

#[test]
fn test_parse_function_call() {
    let tokens = Lexer::new("<?php echo my_double(21);").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::Echo(Expr::FunctionCall {
            name: "my_double".into(),
            args: vec![CallArg::Positional(Expr::Integer(21))],
            generic_args: vec![],
        })]
    );
}

#[test]
fn test_parse_if() {
    let tokens = Lexer::new("<?php if (1) echo 42;").tokenize().unwrap();
    let stmts = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        stmts,
        vec![Stmt::If {
            condition: Expr::Integer(1),
            then_body: vec![Stmt::Echo(Expr::Integer(42))],
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
            then_body: vec![Stmt::Echo(Expr::Integer(1))],
            else_body: vec![Stmt::Echo(Expr::Integer(2))],
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
                left: Box::new(Expr::Variable("x".into())),
                right: Box::new(Expr::Integer(3)),
            },
            body: vec![Stmt::Echo(Expr::Variable("x".into()))],
        }]
    );
}
