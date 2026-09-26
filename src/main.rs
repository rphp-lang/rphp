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
  rphp [OPTIONS] [-f] <FILE> [ARGS...]
  rphp [OPTIONS] -r <CODE> [ARGS...]
  rphp [OPTIONS] -- [ARGS...]

Arguments:
  [FILE]       Read and execute a PHP source file
  [ARGS...]    Arguments passed to the script through $argv

Options:
  -f <FILE>    Parse and execute FILE
  -r <CODE>    Execute PHP code without requiring an opening tag
  -n, --no-php-ini
               Ignore php.ini (RPHP does not load one yet)
  -e           Generate extended information (accepted compatibility no-op)
  -a           Start an interactive shell (requires an admitted readline extension)
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
    InteractiveUnavailable,
}

#[derive(Debug, PartialEq, Eq)]
struct CliInvocation {
    action: CliAction,
    ini_settings: Vec<(String, String)>,
    /// Script arguments published through `$argv` after the script name.
    arguments: Vec<String>,
}

fn parse_ini_definition(definition: &str) -> Result<(String, String), String> {
    let (name, value) = definition
        .split_once('=')
        .map_or((definition, "1"), |(name, value)| (name, value));
    let name = name.trim();
    if name.is_empty() {
        return Err("option '-d' requires a non-empty INI name".to_string());
    }
    // A directive decides how to interpret its name and value. In particular,
    // GC publishes the raw numeric spelling and whitespace is significant for
    // boolean keywords; neither can be recovered after CLI normalization.
    Ok((name.to_string(), value.to_string()))
}

fn parse_cli_args(args: &[String]) -> Result<CliInvocation, String> {
    let mut action_args = Vec::new();
    let mut ini_settings = Vec::new();
    let mut interactive = false;
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
        if argument == "-n" || argument == "--no-php-ini" || argument == "-e" {
            index += 1;
            continue;
        }
        if argument == "-a" {
            interactive = true;
            index += 1;
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

    let mut arguments = Vec::new();
    let action = if interactive {
        CliAction::InteractiveUnavailable
    } else {
        match action_args.as_slice() {
            [] => CliAction::Stdin,
            [flag] if flag == "-h" || flag == "--help" => CliAction::Help,
            [flag] if flag == "-v" || flag == "--version" => CliAction::Version,
            [flag] if flag == "-r" => {
                return Err("option '-r' requires a code argument".to_string());
            }
            // PHP drops one `--` after the code; everything else is argv.
            [flag, code, rest @ ..] if flag == "-r" => {
                arguments = match rest {
                    [separator, rest @ ..] if separator == "--" => rest.to_vec(),
                    rest => rest.to_vec(),
                };
                CliAction::Inline(code.clone())
            }
            // `--` ends the option list; the code then comes from standard
            // input and everything after the separator belongs to the script.
            [separator, rest @ ..] if separator == "--" => {
                arguments = rest.to_vec();
                CliAction::Stdin
            }
            [flag] if flag == "-f" => {
                return Err("option '-f' requires a file argument".to_string());
            }
            [flag, file, rest @ ..] if flag == "-f" => {
                arguments = match rest {
                    [separator, rest @ ..] if separator == "--" => rest.to_vec(),
                    rest => rest.to_vec(),
                };
                CliAction::File(file.clone())
            }
            [first, ..] if first.starts_with('-') => {
                return Err(format!("unsupported option '{first}'"));
            }
            [file, rest @ ..] => {
                arguments = rest.to_vec();
                CliAction::File(file.clone())
            }
        }
    };
    Ok(CliInvocation {
        action,
        ini_settings,
        arguments,
    })
}

fn exit_after_pending_shutdown(eg: &mut ExecutorGlobals, default_code: i32) -> ! {
    eg.flush_post_fatal_output();
    let logical_caller = eg.current_execute_data.get();
    match stdlib::run_shutdown_functions(eg, logical_caller) {
        Ok(()) => std::process::exit(default_code),
        Err(execute::VmError::Exit(code)) => std::process::exit(code),
        Err(execute::VmError::Parse(message)) => {
            eprintln!("\nParse error: {message}");
            std::process::exit(255);
        }
        Err(error) => {
            eprintln!("\nFatal error: {error}");
            std::process::exit(255);
        }
    }
}

fn startup_display_errors_uses_stderr(settings: &[(String, String)]) -> bool {
    settings
        .iter()
        .rev()
        .find(|(name, _)| name.eq_ignore_ascii_case("display_errors"))
        .is_some_and(|(_, value)| value.trim().eq_ignore_ascii_case("stderr"))
}

fn emit_disabled_function_startup_warnings(settings: &[(String, String)]) {
    use rphp::runtime::startup::{disable_function_warnings, setting};
    if stdlib::startup_error_reporting(settings) & 2 == 0 {
        return;
    }
    let warnings = disable_function_warnings(setting(settings, "disable_functions").unwrap_or(""));
    let enabled = |name, default| {
        setting(settings, name).map_or(default, |value| value != "" && value != "0")
    };
    let display = enabled("display_errors", true) && enabled("display_startup_errors", true);
    for message in warnings {
        if enabled("log_errors", false) || !display {
            eprintln!("PHP Warning:  {message} in Unknown on line 0");
        }
        if display {
            if startup_display_errors_uses_stderr(settings) {
                eprintln!("Warning: {message} in Unknown on line 0");
            } else {
                println!("\nWarning: {message} in Unknown on line 0");
            }
        }
    }
}

fn emit_request_startup_warning(settings: &[(String, String)], message: &str) {
    use rphp::runtime::startup::setting;
    if stdlib::startup_error_reporting(settings) & 2 == 0 {
        return;
    }
    let enabled = |name, default| {
        setting(settings, name).map_or(default, |value| value != "" && value != "0")
    };
    if enabled("log_errors", false) {
        eprintln!("PHP Warning:  {message} in Unknown on line 0");
    }
    if enabled("display_errors", true) && enabled("display_startup_errors", true) {
        if startup_display_errors_uses_stderr(settings) {
            eprintln!("Warning: {message} in Unknown on line 0");
        } else {
            println!("\nWarning: {message} in Unknown on line 0");
        }
    }
}

fn read_source(action: CliAction) -> Result<Vec<u8>, String> {
    match action {
        CliAction::Inline(code) => {
            if code.starts_with("<?php") || code.starts_with("<?") {
                Ok(code.into_bytes())
            } else {
                Ok(format!("<?php {code}").into_bytes())
            }
        }
        CliAction::File(file) => {
            std::fs::read(&file).map_err(|error| format!("could not read file '{file}': {error}"))
        }
        CliAction::Stdin => read_stdin(),
        CliAction::Help | CliAction::Version | CliAction::InteractiveUnavailable => {
            unreachable!("handled before reading input")
        }
    }
}

fn read_stdin() -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|error| format!("could not read standard input: {error}"))?;
    Ok(buf)
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
        arguments,
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
        CliAction::InteractiveUnavailable => {
            println!("Interactive shell (-a) requires the readline extension.");
            return;
        }
        _ => {}
    }
    if let Some(warning) = stdlib::startup_date_timezone_warning(&ini_settings) {
        eprintln!("PHP Warning:  PHP Startup: {warning} in Unknown on line 0");
    }
    emit_disabled_function_startup_warnings(&ini_settings);

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
        CliAction::Help | CliAction::Version | CliAction::InteractiveUnavailable => {
            unreachable!("handled above")
        }
    };
    let executed_file = matches!(action, CliAction::File(_)).then(|| source_file.clone());
    let script_name = match &action {
        CliAction::File(file) => Some(file.clone()),
        _ => None,
    };
    let source_offset_base = match &action {
        CliAction::Inline(code) if !code.starts_with("<?php") && !code.starts_with("<?") => 6,
        _ => 0,
    };

    let source = read_source(action).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(1);
    });
    stats::configure_from_env();
    if stats::enabled() {
        stats::reset();
    }

    // A configured handler resolves at request startup, before parsing and
    // user declarations. Keep descriptor owners alive with the one executor;
    // empty/default requests retain their existing initialization order.
    let initialize_request = || {
        let mut eg = ExecutorGlobals::new();
        stdlib::apply_startup_ini_settings(&mut eg, &ini_settings);
        stdlib::set_startup_config(&ini_settings);
        let stdlib = stdlib::register_stdlib(&mut eg);
        stdlib::register_request_globals(&mut eg, script_name.as_deref(), &arguments);
        let coroutines = register_coroutine_api(&mut eg);
        eg.apply_disabled_functions();
        (eg, stdlib, coroutines)
    };
    let output_handler = stdlib::startup_output_handler(&ini_settings);
    let mut startup_request = if output_handler.is_empty() {
        None
    } else {
        let mut request = initialize_request();
        if let Some(warning) = stdlib::start_output_handler(&mut request.0, output_handler) {
            emit_request_startup_warning(&ini_settings, &warning);
        }
        Some(request)
    };

    let tokens = Lexer::new_bytes(&source)
        .with_source_offset_base(source_offset_base)
        .tokenize_included_source()
        .unwrap_or_else(|e| {
            eprintln!("Parse error: {}", e);
            std::process::exit(255);
        });

    let stmts = Parser::new(tokens)
        .with_source_name(source_file.clone())
        .parse()
        .unwrap_or_else(|e| {
            if matches!(
                e.as_str(),
                "Cannot use positional argument after argument unpacking"
                    | "Cannot use argument unpacking after named arguments"
            ) {
                eprintln!("Fatal error: {} in {} on line 1", e, source_file);
                std::process::exit(255);
            }
            eprintln!("Parse error: {}", e);
            std::process::exit(255);
        });

    let result = Compiler::new()
        .with_zend_assertions(stdlib::startup_zend_assertions(&ini_settings))
        .with_disabled_functions(
            rphp::runtime::startup::setting(&ini_settings, "disable_functions").unwrap_or(""),
        )
        .with_precision(stdlib::startup_precision(&ini_settings))
        .with_source_context(source_file, source_directory)
        .compile(&stmts)
        .unwrap_or_else(|failure| {
            let mut fallback;
            let eg = if let Some(request) = startup_request.as_mut() {
                &mut request.0
            } else {
                fallback = ExecutorGlobals::new();
                stdlib::apply_startup_ini_settings(&mut fallback, &ini_settings);
                &mut fallback
            };
            eg.emit_compile_deprecations(&failure.deprecations);
            if failure.deprecations.is_empty() {
                eprintln!("Fatal error: {}", failure.message);
            } else {
                eprintln!("\nFatal error: {}", failure.message);
            }
            std::process::exit(255);
        });
    let compiler_halt_source = result.main.source_file.to_string();
    let compiler_halt_offset = result.compiler_halt_offset;
    let main_func = make_user_function(result.main);
    let (mut eg, _stdlib, _coroutines) = startup_request.unwrap_or_else(initialize_request);
    if let Some(offset) = compiler_halt_offset {
        eg.register_compiler_halt_offset(compiler_halt_source, offset);
    }
    eg.generic_metadata = result.generic_metadata;
    eg.constant_attributes = result.constant_attributes;
    eg.constant_expressions = result.constant_expressions;
    eg.refresh_constant_deprecation_metadata_presence();
    let emitted_compile_deprecations = !result.deprecations.is_empty();
    eg.emit_compile_deprecations(&result.deprecations);
    if let Some(executed_file) = executed_file {
        eg.record_included_file(executed_file);
    }

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
    for (declaration_key, name, function) in result.runtime_functions {
        eg.register_runtime_function_declaration(declaration_key, name, function)
            .unwrap_or_else(|error| {
                eprintln!("Fatal error: {error}");
                std::process::exit(255);
            });
    }

    for (declaration_key, class_def) in result.runtime_class_defs {
        if let Err(e) = eg.register_runtime_class_declaration(declaration_key, class_def) {
            if emitted_compile_deprecations {
                eprintln!("\nFatal error: {}", e);
            } else {
                eprintln!("Fatal error: {}", e);
            }
            std::process::exit(255);
        }
    }

    // Register class definitions
    for class_def in result.class_defs {
        if let Err(e) = eg.register_compiled_class(class_def) {
            if emitted_compile_deprecations || eg.has_recorded_php_error() {
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
            exit_after_pending_shutdown(&mut eg, code);
        }
        Err(execute::VmError::Parse(message)) => {
            stdlib::publish_cli_fatal(&eg, "Parse error", &message);
            exit_after_pending_shutdown(&mut eg, 255);
        }
        Err(execute::VmError::CompileFatal(message)) => {
            stdlib::publish_cli_fatal(&eg, "Fatal error", &message);
            exit_after_pending_shutdown(&mut eg, 255);
        }
        Err(e) => {
            stdlib::publish_cli_fatal(&eg, "Fatal error", &e.to_string());
            exit_after_pending_shutdown(&mut eg, 255);
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
                arguments: Vec::new(),
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
            parse_cli_args(&args(&["-f", "example.php"])),
            invocation(CliAction::File("example.php".to_string()))
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
                arguments: Vec::new(),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["-d", "display_errors", "-r", "echo 1;"])),
            Ok(CliInvocation {
                action: CliAction::Inline("echo 1;".to_string()),
                ini_settings: vec![("display_errors".to_string(), "1".to_string())],
                arguments: Vec::new(),
            })
        );
    }

    #[test]
    fn preserves_ini_case_and_value_whitespace_for_directive_semantics() {
        assert_eq!(
            super::parse_ini_definition("ZEND.ENABLE_GC=0"),
            Ok(("ZEND.ENABLE_GC".to_string(), "0".to_string()))
        );
        assert_eq!(
            super::parse_ini_definition("zend.enable_gc= on "),
            Ok(("zend.enable_gc".to_string(), " on ".to_string()))
        );
    }

    #[test]
    fn parses_php_ini_extended_info_and_unavailable_interactive_flags() {
        assert_eq!(
            parse_cli_args(&args(&["-n", "--no-php-ini", "-e", "example.php",])),
            Ok(CliInvocation {
                action: CliAction::File("example.php".to_string()),
                ini_settings: Vec::new(),
                arguments: Vec::new(),
            })
        );
        assert_eq!(
            parse_cli_args(&args(
                &["-n", "-d", "memory_limit=4M", "-a", "ignored.php",]
            )),
            Ok(CliInvocation {
                action: CliAction::InteractiveUnavailable,
                ini_settings: vec![("memory_limit".to_string(), "4M".to_string())],
                arguments: Vec::new(),
            })
        );
    }

    #[test]
    fn passes_script_arguments_through_to_the_script() {
        assert_eq!(
            parse_cli_args(&args(&["script.php", "argument", "--flag", "-x"])),
            Ok(CliInvocation {
                action: CliAction::File("script.php".to_string()),
                ini_settings: Vec::new(),
                arguments: args(&["argument", "--flag", "-x"]),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["--", "-script.php", "-n"])),
            Ok(CliInvocation {
                action: CliAction::Stdin,
                ini_settings: Vec::new(),
                arguments: args(&["-script.php", "-n"]),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["script.php", "--", "kept"])),
            Ok(CliInvocation {
                action: CliAction::File("script.php".to_string()),
                ini_settings: Vec::new(),
                arguments: args(&["--", "kept"]),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["-f", "script.php", "--", "first"])),
            Ok(CliInvocation {
                action: CliAction::File("script.php".to_string()),
                ini_settings: Vec::new(),
                arguments: args(&["first"]),
            })
        );
        assert_eq!(
            parse_cli_args(&args(&["-r", "echo 1;", "first", "second"])),
            Ok(CliInvocation {
                action: CliAction::Inline("echo 1;".to_string()),
                ini_settings: Vec::new(),
                arguments: args(&["first", "second"]),
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
    }

    #[test]
    fn inline_code_gets_an_opening_tag_when_needed() {
        assert_eq!(
            read_source(CliAction::Inline("echo 42;".to_string())),
            Ok(b"<?php echo 42;".to_vec())
        );
        assert_eq!(
            read_source(CliAction::Inline("<?php echo 42;".to_string())),
            Ok(b"<?php echo 42;".to_vec())
        );
    }
}
