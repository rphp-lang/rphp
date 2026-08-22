use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

fn assert_run(args: &[&str], source: &str, stdout: &str) {
    assert_eq!(run(args, source), (0, stdout.to_string(), String::new()));
}

#[test]
fn dynamic_float_to_nonnumeric_string_comparisons_use_current_precision() {
    assert_run(
        &[],
        r#"<?php
$value = 1.75;
foreach ([14, 0] as $precision) {
    ini_set('precision', $precision);
    echo $value <=> '1.75abc', ',', '1.75abc' <=> $value, "\n";
    var_dump(
        $value < '1.75abc',
        $value <= '1.75abc',
        $value > '1.75abc',
        $value >= '1.75abc',
        $value == '1.75abc',
        $value != '1.75abc',
    );
}
"#,
        concat!(
            "-1,1\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(true)\n",
            "1,-1\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "bool(true)\n",
        ),
    );
}

#[test]
fn nested_compound_comparisons_keep_the_same_precision_contract() {
    assert_run(
        &[],
        r#"<?php
ini_set('precision', 0);
$arrayNumber = [1.75];
$arrayString = ['1.75abc'];
$objectNumber = (object) ['value' => 1.75];
$objectString = (object) ['value' => '1.75abc'];
echo $arrayNumber <=> $arrayString, ',', $arrayNumber == $arrayString ? 'T' : 'F', "\n";
echo $objectNumber <=> $objectString, ',', $objectNumber == $objectString ? 'T' : 'F', "\n";
"#,
        "1,F\n1,F\n",
    );
}

#[test]
fn numeric_strings_and_nonfinite_values_retain_their_distinct_paths() {
    assert_run(
        &[],
        r#"<?php
$infinity = INF;
ini_set('precision', 14);
echo $infinity <=> 'IE', ',';
ini_set('precision', 0);
echo $infinity <=> 'IE', ',', $infinity <=> '1e999', "\n";
$value = 1.75;
var_dump($value == '1.75', $value <=> '1.75');
"#,
        "1,-1,0\nbool(true)\nint(0)\n",
    );
}

#[test]
fn startup_precision_is_snapshotted_for_constant_comparisons() {
    assert_run(
        &["-d", "precision=0"],
        r#"<?php
const RESULT = 1.75 <=> '1.75abc';
class Box { const RESULT = 1.75 <=> '1.75abc'; }
define('DYNAMIC_RESULT', 1.75 <=> '1.75abc');
ini_set('precision', 14);
$value = 1.75;
echo RESULT, ',', Box::RESULT, ',', DYNAMIC_RESULT, ',';
echo $value <=> '1.75abc', ',', 1.75 <=> '1.75abc', "\n";
"#,
        "1,1,1,-1,1\n",
    );
}

#[test]
fn eval_and_include_compile_against_the_current_precision() {
    let include_path = std::env::temp_dir().join(format!(
        "rphp-float-string-comparison-precision-{}.php",
        std::process::id()
    ));
    fs::write(
        &include_path,
        "<?php const INCLUDED_RESULT = 1.75 <=> '1.75abc'; echo INCLUDED_RESULT, \",\";",
    )
    .expect("include fixture should be written");
    let include_literal = format!("{:?}", include_path.to_string_lossy());
    let source = format!(
        "<?php\nini_set('precision', 0);\ninclude {include_literal};\n\
         eval(\"const EVAL_ZERO = 1.75 <=> '1.75abc';\");\n\
         echo EVAL_ZERO, ',';\nini_set('precision', 14);\n\
         eval(\"const EVAL_FOURTEEN = 1.75 <=> '1.75abc';\");\n\
         echo EVAL_FOURTEEN, \"\\n\";\n"
    );
    let result = run(&[], &source);
    fs::remove_file(&include_path).expect("include fixture should be removed");
    assert_eq!(result, (0, "1,1,-1\n".to_string(), String::new()));
}
