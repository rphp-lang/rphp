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
fn false_autovivifies_across_storage_and_mutation_forms() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, "\n";
    return true;
});

$direct = false;
$direct['named'] = 11;
$alias =& $direct;
$alias[] = 12;

$nested = false;
$nested[3]['leaf'] = 13;

class FalseBoxes {
    public $item = false;
    public static $shared = false;
    function fill() {
        $this->item[] = 14;
        self::$shared['slot'] = 15;
    }
    function show() {
        echo $this->item[0], ':', self::$shared['slot'];
    }
}
$box = new FalseBoxes;
$box->fill();

$loop = false;
foreach ([16, 17] as $loop[]) {}
$list = false;
[$list[]] = [18];
$compound = false;
$compound[] &= 3;
$linked = false;
$value = 19;
$linked[] =& $value;
$value = 20;
$removed = false;
unset($removed[0]);

echo $direct['named'], ':', $direct[0], '|', $nested[3]['leaf'], '|';
$box->show();
echo '|', $loop[0], ':', $loop[1], '|', $list[0], '|', $compound[0], '|';
echo $linked[0], '|', ($removed === false ? 'false' : 'changed'), "\n";
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "8192:Automatic conversion of false to array is deprecated\n",
            "11:12|13|14:15|16:17|18|0|20|false\n",
        )
    );
}

#[test]
fn reentrant_handler_clobbering_wins_before_later_dimension_evaluation() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$mode = 'whole';
set_error_handler(function ($level, $message) use (&$mode) {
    echo $mode, ':', $message, "\n";
    if ($mode === 'whole') {
        $GLOBALS['clobber'] = 'replaced';
    } elseif ($mode === 'element') {
        $GLOBALS['nested'][0] = 'changed';
    } elseif ($mode === 'other') {
        $GLOBALS['side'] = 'seen';
    }
});

$clobber = [false];
$clobber[0][$undefined] = 7;
echo 'whole:', $clobber, "\n";

$mode = 'element';
$nested = [false];
$nested[0]['key'] = 8;
echo 'element:', $nested[0], "\n";

$mode = 'other';
$ordinary = false;
$ordinary[] = 9;
echo 'other:', $ordinary[0], ':', $side, "\n";
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "whole:Automatic conversion of false to array is deprecated\n",
            "whole:replaced\n",
            "element:Automatic conversion of false to array is deprecated\n",
            "element:changed\n",
            "other:Automatic conversion of false to array is deprecated\n",
            "other:9:seen\n",
        )
    );
}

#[test]
fn direct_append_deprecation_uses_the_target_source_line() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
set_error_handler(function ($level, $message, $file, $line) {
    echo $line, ':', $message, "\n";
    return true;
});
$container = false;
$container[] = 1;
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        "7:Automatic conversion of false to array is deprecated\n"
    );
}

#[test]
fn foreach_append_targets_cover_values_keys_and_nested_destinations() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
set_error_handler(function ($level, $message) { echo $message, "\n"; });
$plain = false;
foreach ([21, 22] as $plain[]) {}
$keys = false;
foreach ([41 => 1, 42 => 2] as $keys[] => $discard) {}
$nested = false;
foreach ([51, 52] as $nested['row'][]) {}
echo implode(':', $plain), '|', implode(':', $keys), '|';
echo implode(':', $nested['row']), "\n";
"#,
    );

    assert_eq!(status, 0, "stderr={stderr:?}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "Automatic conversion of false to array is deprecated\n",
            "Automatic conversion of false to array is deprecated\n",
            "Automatic conversion of false to array is deprecated\n",
            "21:22|41:42|51:52\n",
        )
    );
}

#[test]
fn other_non_array_scalars_do_not_enter_false_autovivification() {
    for source in [
        "<?php $value = true; $value[] = 1;",
        "<?php $value = 42; $value['key'] = 1;",
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255, "stdout={stdout:?}; stderr={stderr:?}");
        assert!(!stdout.contains("Automatic conversion of false to array"));
        assert!(!stderr.contains("Automatic conversion of false to array"));
    }
}
