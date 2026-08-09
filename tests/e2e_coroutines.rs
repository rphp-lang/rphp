#![cfg(feature = "coroutines")]

mod common;

use std::time::Duration;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::coroutine;
use rphp::stdlib;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;

fn run(source: &str) -> Result<String, execute::VmError> {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let main_function = make_user_function(compiled.main);
    let (mut eg, output) = common::make_eg_with_capture();
    let _stdlib = stdlib::register_stdlib(&mut eg);
    let _coroutines = coroutine::register_api(&mut eg);
    for (name, function) in &compiled.functions {
        eg.register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class in compiled.class_defs {
        eg.register_class(class).unwrap();
    }

    execute::execute(&mut eg, &main_function)?;
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    Ok(output)
}

#[test]
fn php_api_resumes_and_joins_a_suspended_closure() {
    let output = run(r#"<?php
$result = coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        echo "A";
        coroutine_suspend();
        echo "B";
        return 42;
    });

    echo "R";
    echo coroutine_resume($task) ? "S" : "D";
    echo "M";
    $value = coroutine_join($task);
    echo $value;
    return $value;
});
echo ":";
echo $result;
"#)
    .unwrap();

    assert_eq!(output, "RASM B42:42".replace(' ', ""));
}

#[test]
fn join_rethrows_child_exception_through_parent_catch_and_finally() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        try {
            coroutine_suspend();
            throw new Exception("boom");
        } finally {
            echo "F";
        }
    });

    coroutine_resume($task);
    try {
        coroutine_join($task);
    } catch (Exception $error) {
        echo $error->getMessage();
    } finally {
        echo "P";
    }
});
"#)
    .unwrap();

    assert_eq!(output, "FboomP");
}

#[test]
fn leaving_scope_cancels_an_unjoined_suspended_child() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        $payload = "owned";
        echo $payload;
        coroutine_suspend();
        echo "unreachable";
    });
    coroutine_resume($task);
    echo ":root";
});
echo ":done";
"#)
    .unwrap();

    assert_eq!(output, "owned:root:done");
}

#[test]
fn scope_propagates_an_unjoined_child_failure() {
    let output = run(r#"<?php
try {
    coroutine_scope(function () {
        $task = coroutine_spawn(function () {
            throw new Exception("unjoined");
        });
        coroutine_resume($task);
        echo "root";
    });
} catch (Exception $error) {
    echo ":";
    echo $error->getMessage();
}
"#)
    .unwrap();

    assert_eq!(output, "root:unjoined");
}

#[test]
fn scope_propagates_the_oldest_unjoined_failure_deterministically() {
    let output = run(r#"<?php
try {
    coroutine_scope(function () {
        $first = coroutine_spawn(function () {
            throw new Exception("first");
        });
        $second = coroutine_spawn(function () {
            throw new Exception("second");
        });
        coroutine_resume($first);
        coroutine_resume($second);
    });
} catch (Exception $error) {
    echo $error->getMessage();
}
"#)
    .unwrap();

    assert_eq!(output, "first");
}

#[test]
fn child_spawn_is_owned_by_the_same_scope() {
    let output = run(r#"<?php
coroutine_scope(function () {
    $parent = coroutine_spawn(function () {
        $nested = coroutine_spawn(function () {
            echo "C";
            return 7;
        });
        echo "P";
        coroutine_suspend();
        echo "Q";
        return $nested;
    });

    coroutine_resume($parent);
    $nested = coroutine_join($parent);
    echo coroutine_join($nested);
});
"#)
    .unwrap();

    assert_eq!(output, "PQC7");
}

#[test]
fn suspension_preserves_a_multi_frame_php_call_chain() {
    let output = run(r#"<?php
function deepest() {
    echo "D";
    coroutine_suspend();
    echo "R";
    return 3;
}
function middle() {
    return deepest() + 4;
}

coroutine_scope(function () {
    $task = coroutine_spawn(function () {
        return middle();
    });
    coroutine_resume($task);
    echo ":";
    echo coroutine_join($task);
});
"#)
    .unwrap();

    assert_eq!(output, "D:R7");
}

#[test]
#[ignore = "run explicitly in release mode as the PHP coroutine API benchmark"]
fn benchmark_one_million_php_suspend_resume_cycles() {
    const ITERATIONS: u64 = 1_000_000;

    let output = run(&format!(
        r#"<?php
coroutine_scope(function () {{
    $task = coroutine_spawn(function () {{
        for ($i = 0; $i < {ITERATIONS}; $i++) {{
            coroutine_suspend();
        }}
        return {ITERATIONS};
    }});

    $started = hrtime(true);
    for ($i = 0; $i < {ITERATIONS}; $i++) {{
        coroutine_resume($task);
    }}
    $result = coroutine_join($task);
    echo (hrtime(true) - $started) . ":" . $result;
}});
"#
    ))
    .unwrap();
    let (elapsed, result) = output.split_once(':').unwrap();
    let elapsed = Duration::from_nanos(elapsed.parse().unwrap());
    let ns_per_cycle = elapsed.as_nanos() as f64 / ITERATIONS as f64;
    eprintln!(
        "PHP coroutine API: {ITERATIONS} suspend/resume cycles in {elapsed:?} ({ns_per_cycle:.2} ns/cycle)"
    );

    assert_eq!(result, ITERATIONS.to_string());
    assert!(ns_per_cycle < 5_000.0);
}
