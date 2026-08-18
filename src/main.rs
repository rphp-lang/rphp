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
  -d <NAME[=VALUE]>
               Define a per-process INI setting
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

#[derive(Debug, PartialEq, Eq)]
struct CliInvocation {
    action: CliAction,
    ini_settings: Vec<(String, String)>,
}

fn parse_ini_definition(definition: &str) -> Result<(String, String), String> {
    let (name, value) = definition
        .split_once('=')
        .map_or((definition, "1"), |(name, value)| (name, value));
    let name = name.trim();
    if name.is_empty() {
        return Err("option '-d' requires a non-empty INI name".to_string());
    }
    Ok((name.to_ascii_lowercase(), value.trim().to_string()))
}

fn parse_cli_args(args: &[String]) -> Result<CliInvocation, String> {
    let mut action_args = Vec::new();
    let mut ini_settings = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            action_args.extend_from_slice(&args[index..]);
            break;
        }
        if argument == "-d" {
            let Some(definition) = args.get(index + 1) else {
                return Err("option '-d' requires an INI definition".to_string());
            };
            ini_settings.push(parse_ini_definition(definition)?);
            index += 2;
            continue;
        }
        if let Some(definition) = argument.strip_prefix("-d")
            && !definition.is_empty()
        {
            ini_settings.push(parse_ini_definition(definition)?);
            index += 1;
            continue;
        }
        // Once a script or `-r` is selected, later values belong to that
        // invocation rather than the process option list.
        action_args.extend_from_slice(&args[index..]);
        break;
    }

    let action = match action_args.as_slice() {
        [] => CliAction::Stdin,
        [flag] if flag == "-h" || flag == "--help" => CliAction::Help,
        [flag] if flag == "-v" || flag == "--version" => CliAction::Version,
        [flag] if flag == "-r" => {
            return Err("option '-r' requires a code argument".to_string());
        }
        [flag, code] if flag == "-r" => CliAction::Inline(code.clone()),
        [separator, file] if separator == "--" => CliAction::File(file.clone()),
        [arg] if arg.starts_with('-') => return Err(format!("unsupported option '{arg}'")),
        [file] => CliAction::File(file.clone()),
        [first, ..] if first == "-r" => {
            return Err("script arguments after '-r' are not supported yet".to_string());
        }
        [first, ..] if first.starts_with('-') => {
            return Err(format!("unsupported option '{first}'"));
        }
        _ => return Err("script arguments are not supported yet".to_string()),
    };
    Ok(CliInvocation {
        action,
        ini_settings,
    })
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
        CliAction::File(file) => std::fs::read(&file)
            .map(|bytes| rphp::lexer::decode_php_source(&bytes))
            .map_err(|error| format!("could not read file '{file}': {error}")),
        CliAction::Stdin => read_stdin(),
        CliAction::Help | CliAction::Version => unreachable!("handled before reading input"),
    }
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|error| format!("could not read standard input: {error}"))?;
    Ok(rphp::lexer::decode_php_source(&buf))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = parse_cli_args(&args).unwrap_or_else(|error| {
        eprintln!("error: {error}\n\nTry 'rphp --help' for more information.");
        std::process::exit(2);
    });
    let CliInvocation {
        action,
        ini_settings,
    } = invocation;
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

    let source_directory = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (source_file, source_directory) = match &action {
        CliAction::File(file) => {
            let absolute = std::fs::canonicalize(file)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| file.clone());
            let directory = std::path::Path::new(&absolute)
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| source_directory.clone());
            (absolute, directory)
        }
        CliAction::Inline(_) => ("Command line code".to_string(), source_directory),
        CliAction::Stdin => ("Standard input code".to_string(), source_directory),
        CliAction::Help | CliAction::Version => unreachable!("handled above"),
    };
    let executed_file = matches!(action, CliAction::File(_)).then(|| source_file.clone());

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

    let stmts = Parser::new(tokens)
        .with_source_name(source_file.clone())
        .parse()
        .unwrap_or_else(|e| {
            if e.starts_with("Multiple access type modifiers are not allowed") {
                eprintln!("Fatal error: {e}");
                std::process::exit(255);
            }
            if matches!(
                e.as_str(),
                "Cannot use positional argument after argument unpacking"
                    | "Cannot use argument unpacking after named arguments"
            ) {
                eprintln!("Fatal error: {} in {} on line 1", e, source_file);
                std::process::exit(255);
            }
            eprintln!("Parse error: {}", e);
            std::process::exit(1);
        });

    let result = Compiler::new()
        .with_zend_assertions(stdlib::startup_zend_assertions(&ini_settings))
        .with_source_context(source_file, source_directory)
        .compile(&stmts)
        .unwrap_or_else(|failure| {
            let mut eg = ExecutorGlobals::new();
            stdlib::apply_startup_ini_settings(&mut eg, &ini_settings);
            eg.emit_compile_deprecations(&failure.deprecations);
            if failure.deprecations.is_empty() {
                eprintln!("Fatal error: {}", failure.message);
            } else {
                eprintln!("\nFatal error: {}", failure.message);
            }
            std::process::exit(255);
        });
    let main_func = make_user_function(result.main);
    let mut eg = ExecutorGlobals::new();
    stdlib::apply_startup_ini_settings(&mut eg, &ini_settings);
    eg.generic_metadata = result.generic_metadata;
    let emitted_compile_deprecations = !result.deprecations.is_empty();
    eg.emit_compile_deprecations(&result.deprecations);
    if let Some(executed_file) = executed_file {
        eg.record_included_file(executed_file);
    }

    // Register stdlib
    let _stdlib = stdlib::register_stdlib(&mut eg);
    let _coroutines = register_coroutine_api(&mut eg);
    // Register declared functions
    for (name, func) in &result.functions {
        eg.register_function(name, &func.common as *const FunctionCommon)
            .unwrap_or_else(|e| {
                if emitted_compile_deprecations {
                    eprintln!("\nFatal error: {}", e);
                } else {
                    eprintln!("Fatal error: {}", e);
                }
                std::process::exit(255);
            });
    }

    // Register class definitions
    for class_def in result.class_defs {
        if let Err(e) = eg.register_compiled_class(class_def) {
            if emitted_compile_deprecations {
                eprintln!("\nFatal error: {}", e);
            } else {
                eprintln!("Fatal error: {}", e);
            }
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
        Err(execute::VmError::Parse(message)) => {
            eprintln!("\nParse error: {message}");
            std::process::exit(255);
        }
        Err(e) => {
            // PHP's displayed runtime fatal begins on a fresh diagnostic line.
            // The leading boundary is trimmed by PHPT when no program output
            // precedes it and remains observable as the required blank line
            // after output that was already emitted.
            eprintln!("\nFatal error: {e}");
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
    use super::{CliAction, CliInvocation, parse_cli_args, read_source};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_help_version_file_and_stdin() {
        let invocation = |action| {
            Ok(CliInvocation {
                action,
                ini_settings: Vec::new(),
            })
        };
        assert_eq!(parse_cli_args(&args(&[])), invocation(CliAction::Stdin));
        assert_eq!(
            parse_cli_args(&args(&["--help"])),
            invocation(CliAction::Help)
        );
        assert_eq!(
            parse_cli_args(&args(&["--version"])),
            invocation(CliAction::Version)
        );
        assert_eq!(
            parse_cli_args(&args(&["example.php"])),
            invocation(CliAction::File("example.php".to_string()))
        );
        assert_eq!(
            parse_cli_args(&args(&["--", "-example.php"])),
            invocation(CliAction::File("-example.php".to_string()))
        );
    }

    #[test]
    fn parses_separate_attached_and_repeated_ini_definitions() {
        assert_eq!(
            parse_cli_args(&args(&[
                "-d",
                "zend.assertions=0",
                "-dassert.exception=1",
                "example.php",
            ])),
            Ok(CliInvocation {
                action: CliAction::File("example.php".to_string()),
                ini_settings: vec![
                    ("zend.assertions".to_string(), "0".to_string()),
                    ("assert.exception".to_string(), "1".to_string()),
                ],
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["-d", "display_errors", "-r", "echo 1;"])),
            Ok(CliInvocation {
                action: CliAction::Inline("echo 1;".to_string()),
                ini_settings: vec![("display_errors".to_string(), "1".to_string())],
            })
        );
    }

    #[test]
    fn rejects_unsupported_options_and_script_arguments() {
        assert_eq!(
            parse_cli_args(&args(&["-d"])),
            Err("option '-d' requires an INI definition".to_string())
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
