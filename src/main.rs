use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::ExecutorGlobals;
use rphp::stdlib;
use rphp::vm::execute::{self};
use rphp::vm::function::FunctionCommon;
use rphp::vm::stats;

const HELP: &str = "\
RPHP - experimental PHP-compatible runtime

Usage:
  rphp [OPTIONS] [FILE]
  rphp -r <CODE>

Arguments:
  [FILE]       Read and execute a PHP source file

Options:
  -r <CODE>    Execute PHP code without requiring an opening tag
  -h, --help   Print help
  -v, --version
               Print the RPHP version

With no FILE or -r option, RPHP reads PHP source from standard input.
RPHP is experimental pre-alpha software; do not run untrusted code.
";

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Help,
    Version,
    Inline(String),
    File(String),
    Stdin,
}

fn parse_cli_args(args: &[String]) -> Result<CliAction, String> {
    match args {
        [] => Ok(CliAction::Stdin),
        [flag] if flag == "-h" || flag == "--help" => Ok(CliAction::Help),
        [flag] if flag == "-v" || flag == "--version" => Ok(CliAction::Version),
        [flag] if flag == "-r" => Err("option '-r' requires a code argument".to_string()),
        [flag, code] if flag == "-r" => Ok(CliAction::Inline(code.clone())),
        [separator, file] if separator == "--" => Ok(CliAction::File(file.clone())),
        [arg] if arg.starts_with('-') => Err(format!("unsupported option '{arg}'")),
        [file] => Ok(CliAction::File(file.clone())),
        [first, ..] if first == "-r" => {
            Err("script arguments after '-r' are not supported yet".to_string())
        }
        [first, ..] if first.starts_with('-') => Err(format!("unsupported option '{first}'")),
        _ => Err("script arguments are not supported yet".to_string()),
    }
}

fn read_source(action: CliAction) -> Result<String, String> {
    match action {
        CliAction::Inline(code) => {
            if code.starts_with("<?php") || code.starts_with("<?") {
                Ok(code)
            } else {
                Ok(format!("<?php {code}"))
            }
        }
        CliAction::File(file) => std::fs::read_to_string(&file)
            .map_err(|error| format!("could not read file '{file}': {error}")),
        CliAction::Stdin => read_stdin(),
        CliAction::Help | CliAction::Version => unreachable!("handled before reading input"),
    }
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|error| format!("could not read standard input: {error}"))?;
    Ok(buf)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = parse_cli_args(&args).unwrap_or_else(|error| {
        eprintln!("error: {error}\n\nTry 'rphp --help' for more information.");
        std::process::exit(2);
    });
    match &action {
        CliAction::Help => {
            print!("{HELP}");
            return;
        }
        CliAction::Version => {
            println!("rphp {} (pre-alpha)", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }

    let source = read_source(action).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(1);
    });
    stats::configure_from_env();
    if stats::enabled() {
        stats::reset();
    }

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
    eg.generic_metadata = result.generic_metadata;

    // Register stdlib
    let _stdlib = stdlib::register_stdlib(&mut eg);
    let _coroutines = register_coroutine_api(&mut eg);
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

#[cfg(feature = "coroutines")]
fn register_coroutine_api(
    eg: &mut ExecutorGlobals,
) -> Vec<Box<rphp::vm::function::InternalFunction>> {
    rphp::runtime::coroutine::register_api(eg)
}

#[cfg(not(feature = "coroutines"))]
#[inline(always)]
fn register_coroutine_api(_eg: &mut ExecutorGlobals) {}

#[cfg(test)]
mod tests {
    use super::{CliAction, parse_cli_args, read_source};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_help_version_file_and_stdin() {
        assert_eq!(parse_cli_args(&args(&[])), Ok(CliAction::Stdin));
        assert_eq!(parse_cli_args(&args(&["--help"])), Ok(CliAction::Help));
        assert_eq!(
            parse_cli_args(&args(&["--version"])),
            Ok(CliAction::Version)
        );
        assert_eq!(
            parse_cli_args(&args(&["example.php"])),
            Ok(CliAction::File("example.php".to_string()))
        );
        assert_eq!(
            parse_cli_args(&args(&["--", "-example.php"])),
            Ok(CliAction::File("-example.php".to_string()))
        );
    }

    #[test]
    fn rejects_unsupported_options_and_script_arguments() {
        assert_eq!(
            parse_cli_args(&args(&["-d"])),
            Err("unsupported option '-d'".to_string())
        );
        assert_eq!(
            parse_cli_args(&args(&["-r"])),
            Err("option '-r' requires a code argument".to_string())
        );
        assert_eq!(
            parse_cli_args(&args(&["script.php", "argument"])),
            Err("script arguments are not supported yet".to_string())
        );
    }

    #[test]
    fn inline_code_gets_an_opening_tag_when_needed() {
        assert_eq!(
            read_source(CliAction::Inline("echo 42;".to_string())),
            Ok("<?php echo 42;".to_string())
        );
        assert_eq!(
            read_source(CliAction::Inline("<?php echo 42;".to_string())),
            Ok("<?php echo 42;".to_string())
        );
    }
}
