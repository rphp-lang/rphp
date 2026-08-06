use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::ExecutorGlobals;
use rphp::stdlib;
use rphp::vm::execute::{self};
use rphp::vm::function::FunctionCommon;
use rphp::vm::stats;

fn parse_cli_args(args: &[String]) -> String {
    let i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-r" => {
                // php -r 'code' — inline code (no <?php tag required)
                if i + 1 >= args.len() {
                    eprintln!("No code specified for -r");
                    std::process::exit(1);
                }
                let code = &args[i + 1];
                // Wrap in <?php if not already present
                return if code.starts_with("<?php") || code.starts_with("<?") {
                    code.clone()
                } else {
                    format!("<?php {}", code)
                };
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                std::process::exit(1);
            }
            _ => {
                // Positional arg — treat as filename
                return std::fs::read_to_string(&args[i]).unwrap_or_else(|e| {
                    eprintln!("Could not read file '{}': {}", args[i], e);
                    std::process::exit(1);
                });
            }
        }
    }
    // No args — read from stdin
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .unwrap_or_else(|e| {
            eprintln!("Could not read stdin: {}", e);
            std::process::exit(1);
        });
    buf
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    stats::configure_from_env();
    if stats::enabled() {
        stats::reset();
    }
    let source = parse_cli_args(&args);

    let tokens = Lexer::new(&source).tokenize().unwrap_or_else(|e| {
        eprintln!("Parse error: {}", e);
        std::process::exit(1);
    });

    let stmts = Parser::new(tokens).parse().unwrap_or_else(|e| {
        eprintln!("Parse error: {}", e);
        std::process::exit(1);
    });

    let result = Compiler::new().compile(&stmts).unwrap_or_else(|e| {
        eprintln!("Fatal error: {}", e);
        std::process::exit(1);
    });
    let main_func = make_user_function(result.main);
    let mut eg = ExecutorGlobals::new();

    // Register stdlib
    let _stdlib = stdlib::register_stdlib(&mut eg);

    // Register declared functions
    for (name, func) in &result.functions {
        eg.register_function(name, &func.common as *const FunctionCommon)
            .unwrap_or_else(|e| {
                eprintln!("Fatal error: {}", e);
                std::process::exit(255);
            });
    }

    // Register class definitions
    for class_def in result.class_defs {
        if let Err(e) = eg.register_class(class_def) {
            eprintln!("Fatal error: {}", e);
            std::process::exit(255);
        }
    }

    let exec_result = execute::execute(&mut eg, &main_func);
    if stats::enabled() {
        stats::dump_to_stderr();
    }

    match exec_result {
        Ok(_) => {}
        Err(execute::VmError::Exit(code)) => {
            std::process::exit(code);
        }
        Err(e) => {
            eprintln!("Fatal error: {:?}", e);
            std::process::exit(255);
        }
    }
}
