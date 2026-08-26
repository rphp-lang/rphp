mod common;

use common::run_php;
use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
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

#[test]
fn suppressed_assignments_cover_value_reference_compound_append_and_magic_properties() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message:mask=", error_reporting(), "\n";
    return true;
});
@$assigned = $missing;
$compound = 1;
@$compound += $missingCompound;
$coalesced = null;
@$coalesced ??= $missingCoalesce;
$array = [];
@$array[] = $missingAppend;
$source = 9;
@$alias =& $source;
$alias = 11;
class SuppressedMagicTarget {
    public function __set($name, $value) { trigger_error("set:$name", E_USER_WARNING); }
    public function __get($name) { trigger_error("get:$name", E_USER_WARNING); return 7; }
}
$target = new SuppressedMagicTarget;
@$target->quiet = 1;
echo 'read=', @$target->quiet, "\n";
var_dump($assigned, $compound, $coalesced, $array, $source);
echo 'after=', error_reporting(), "\n";
"#,
        ),
        concat!(
            "diag=2:Undefined variable $missing:mask=4437\n",
            "diag=2:Undefined variable $missingCompound:mask=4437\n",
            "diag=2:Undefined variable $missingCoalesce:mask=4437\n",
            "diag=2:Undefined variable $missingAppend:mask=4437\n",
            "diag=512:set:quiet:mask=4437\n",
            "read=diag=512:get:quiet:mask=4437\n",
            "7\n",
            "NULL\n",
            "int(1)\n",
            "NULL\n",
            "array(1) {\n  [0]=>\n  NULL\n}\n",
            "int(11)\n",
            "after=30719\n",
        )
    );
}

#[test]
fn string_offset_reads_and_writes_share_php_85_key_conversion_and_suppression() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message:mask=", error_reporting(), "\n";
    return true;
});
$value = 'abc';
$value[3.5] = 'Z';
echo $value, "\n";
@$value['0idx'] = 'Q';
echo @$value['0idx'], ':', $value, "\n";
$value[6] = 'XY';
echo $value, "\n";
try { $value['1.5'] = 'N'; } catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "diag=2:String offset cast occurred:mask=30719\n",
            "abcZ\n",
            "diag=2:Illegal string offset \"0idx\":mask=4437\n",
            "diag=2:Illegal string offset \"0idx\":mask=4437\n",
            "Q:QbcZ\n",
            "diag=2:Only the first byte will be assigned to the string offset:mask=30719\n",
            "QbcZ  X\n",
            "TypeError:Cannot access offset of type string on string\n",
        )
    );
}

#[test]
fn suppression_does_not_make_non_lvalues_assignable() {
    let (status, stdout, stderr) = run_stdin(
        "<?php\nfunction source_value() { return 1; }\n@source_value() = 2;\necho 'unreachable';\n",
    );
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "Fatal error: Can't use function return value in write context in Standard input code on line 3\n"
    );

    let (status, stdout, stderr) = run_stdin("<?php\n$a = 1; $b = 2;\n@($a + $b) = 3;\n");
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("Parse error: Invalid assignment target: BinaryOp"));
}
