use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;
use rphp::runtime::ExecutorGlobals;
use rphp::stdlib;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let source = if args.len() > 1 {
        std::fs::read_to_string(&args[1]).unwrap_or_else(|e| {
            eprintln!("Could not read file '{}': {}", args[1], e);
            std::process::exit(1);
        })
    } else {
        // Read from stdin
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
            eprintln!("Could not read stdin: {}", e);
            std::process::exit(1);
        });
        buf
    };

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
        eg.register_function(name, &func.common as *const FunctionCommon).unwrap_or_else(|e| {
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

    match execute::execute(&mut eg, &main_func) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Fatal error: {:?}", e);
            std::process::exit(255);
        }
    }
}
