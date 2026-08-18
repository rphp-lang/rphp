use std::process::Command;

fn run(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(arguments)
        .output()
        .expect("rphp subprocess should start");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn startup_precision_controls_scalar_string_conversion_paths() {
    let source = r#"
$value = 1.2345678901234567;
echo ini_get('precision'), '|', $value, '|', (string) 1e16, '|', 'x' . $value, '|';
print_r($value);
"#;
    let (status, stdout, stderr) = run(&["-dprecision=3", "-r", source]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "3|1.23|1.0E+16|x1.23|1.23");
    assert_eq!(stderr, "");
}

#[test]
fn default_precision_preserves_php_fixed_and_scientific_boundaries() {
    let source = r#"
$values = [0.0, -0.0, 1.25, 99999.99, 0.0001, 0.00001, 1e13, 1e14, 1e15, 1.2345678901234567];
foreach ($values as $value) { echo '[', $value, "]\n"; }
"#;
    let (status, stdout, stderr) = run(&["-r", source]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "[0]\n[-0]\n[1.25]\n[99999.99]\n[0.0001]\n[1.0E-5]\n",
            "[10000000000000]\n[1.0E+14]\n[1.0E+15]\n[1.2345678901235]\n",
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn precision_uses_significant_digits_and_php_exponent_spelling() {
    let source = r#"
$value = 2.2250738585072012e-308;
foreach ([14, 17, 32, -1] as $precision) {
    ini_set('precision', (string) $precision);
    echo $precision, ':', $value, "\n";
}
"#;
    let (status, stdout, stderr) = run(&["-r", source]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "14:2.2250738585072E-308\n",
            "17:2.2250738585072014E-308\n",
            "32:2.2250738585072013830902327173324E-308\n",
            "-1:2.2250738585072014E-308\n",
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn var_dump_keeps_round_trip_float_rendering_independent_of_precision() {
    let (status, stdout, stderr) = run(&[
        "-dprecision=3",
        "-r",
        "var_dump(9.223372036854776e18); var_dump(1.2345678901234567);",
    ]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "float(9.223372036854776E+18)\nfloat(1.2345678901234567)\n"
    );
    assert_eq!(stderr, "");
}

#[test]
fn huge_precision_is_bounded_by_the_exact_binary64_expansion() {
    let (status, stdout, stderr) = run(&[
        "-r",
        "ini_set('precision', 1100000000); echo -1 * (2 ** -10), '|', ini_get('precision');",
    ]);
    assert_eq!(status, 0);
    assert_eq!(stdout, "-0.0009765625|1100000000");
    assert_eq!(stderr, "");
}

#[test]
fn invalid_precision_updates_match_startup_and_runtime_contracts() {
    let (status, stdout, stderr) = run(&[
        "-dprecision=17junk",
        "-dprecision=-2",
        "-r",
        r#"
echo ini_get('precision'), '|', 1 / 3, '|';
var_dump(ini_set('precision', '16junk'));
echo ini_get('precision'), '|', 1 / 3, '|';
var_dump(ini_set('precision', '-2'));
echo ini_get('precision'), '|', 1 / 3;
"#,
    ]);
    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "14|0.33333333333333|string(2) \"14\"\n",
            "16junk|0.3333333333333333|bool(false)\n",
            "16junk|0.3333333333333333",
        )
    );
    assert_eq!(stderr, "");
}
