#![cfg(all(
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::UserFunction;
use rphp::vm::planner::BlockPlan;

fn execute_source(source: &str) -> (UserFunction, String) {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compilation = Compiler::new().compile(&statements).unwrap();
    let main = make_user_function(compilation.main);
    let (mut globals, output) = common::make_eg_with_capture();

    execute::execute(&mut globals, &main).unwrap();
    drop(globals);
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    (main, output)
}

fn native_array_plans(
    main: &UserFunction,
) -> impl Iterator<Item = &rphp::vm::quick::QuickLongOpsLoop> {
    main.op_array
        .block_plans
        .iter()
        .filter_map(|plan| match plan {
            BlockPlan::QuickLongOps(plan) if plan.native_jit().native_entries() != 0 => Some(plan),
            _ => None,
        })
}

#[test]
fn irregular_integer_reads_enter_native_indexed_region() {
    let source = "<?php
$n = 4096;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
echo $sum . '|' . $i;
";
    let (main, output) = execute_source(source);

    assert_eq!(output, "8386560|4096");
    let plans = native_array_plans(&main).collect::<Vec<_>>();
    assert_eq!(plans.len(), 2);
    assert!(
        plans
            .iter()
            .all(|plan| plan.native_jit().native_entries() == 1)
    );
    assert!(plans.iter().all(|plan| plan.native_jit().side_exits() == 0));
}

#[test]
fn native_indexed_type_miss_resumes_exact_fetch() {
    let source = "<?php
$n = 4096;
$values = [];
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$lastKey = ((4095 * 1103515245) & 2147483647) + 1000000;
$values[$lastKey] = 1.5;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
echo $sum . '|' . $i;
";
    let (main, output) = execute_source(source);

    assert_eq!(output, "8382466.5|4096");
    let plans = native_array_plans(&main).collect::<Vec<_>>();
    assert_eq!(plans.len(), 2);
    assert!(
        plans
            .iter()
            .all(|plan| plan.native_jit().native_entries() == 1)
    );
    assert_eq!(
        plans
            .iter()
            .map(|plan| plan.native_jit().side_exits())
            .sum::<u64>(),
        1
    );
}

#[test]
fn native_structural_writes_preserve_cow_and_grow_exactly() {
    let source = "<?php
$n = 4096;
$values = [];
$values[1000000] = -1;
$alias = $values;
for ($i = 0; $i < $n; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $values[$key] = $i;
}
$lastKey = ((4095 * 1103515245) & 2147483647) + 1000000;
echo $alias[1000000] . '|' . $values[1000000] . '|' . $values[$lastKey] . '|' . $i;
";
    let (main, output) = execute_source(source);

    assert_eq!(output, "-1|0|4095|4096");
    let plans = native_array_plans(&main).collect::<Vec<_>>();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].native_jit().native_entries(), 1);
    assert_eq!(plans[0].native_jit().side_exits(), 0);
}
