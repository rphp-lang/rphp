mod common;

use std::process::Command;

const OBSERVE: &str =
    "echo (int)gc_enabled(), '|', json_encode(ini_get('zend.enable_gc')), \"\\n\";";

fn assert_cli(settings: &[&str], source: &str, expected: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-d", "display_errors=1", "-d", "log_errors=0"])
        .args(settings)
        .args(["-r", source])
        .output()
        .expect("run GC configuration probe");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn default_and_explicit_enabled_startup_agree() {
    assert_cli(&[], OBSERVE, "1|\"1\"\n");
    assert_cli(&["-dzend.enable_gc=1"], OBSERVE, "1|\"1\"\n");
    assert_cli(&["-d", "zend.enable_gc"], OBSERVE, "1|\"1\"\n");
}

#[test]
fn startup_numeric_zero_disables_collection_and_preserves_spelling() {
    assert_cli(&["-dzend.enable_gc=0"], OBSERVE, "0|\"0\"\n");
    assert_cli(&["-dzend.enable_gc=00"], OBSERVE, "0|\"00\"\n");
}

#[test]
fn startup_boolean_words_publish_ini_scanner_values() {
    for word in ["On", "true", "yes"] {
        assert_cli(&[&format!("-dzend.enable_gc={word}")], OBSERVE, "1|\"1\"\n");
    }
    for word in ["Off", "false", "no", ""] {
        assert_cli(&[&format!("-dzend.enable_gc={word}")], OBSERVE, "0|\"\"\n");
    }
}

#[test]
fn startup_numeric_prefix_is_not_a_float_or_full_integer_parse() {
    for (value, expected) in [
        ("2", "1|\"2\"\n"),
        ("-1", "1|\"-1\"\n"),
        ("1junk", "1|\"1junk\"\n"),
        ("+2x", "1|\"+2x\"\n"),
        ("0.1", "0|\"0.1\"\n"),
        ("garbage", "0|\"garbage\"\n"),
    ] {
        assert_cli(&[&format!("-dzend.enable_gc={value}")], OBSERVE, expected);
    }
}

#[test]
fn significant_startup_whitespace_survives_cli_parsing() {
    assert_cli(&["-dzend.enable_gc= on "], OBSERVE, "0|\" on \"\n");
    assert_cli(&["-dzend.enable_gc= \t-2x"], OBSERVE, "1|\" \\t-2x\"\n");
}

#[test]
fn unknown_uppercase_directive_does_not_override_gc() {
    assert_cli(&["-dZEND.ENABLE_GC=0"], OBSERVE, "1|\"1\"\n");
}

#[test]
fn repeated_startup_definitions_take_the_last_value() {
    assert_cli(
        &["-dzend.enable_gc=0", "-dzend.enable_gc=On"],
        OBSERVE,
        "1|\"1\"\n",
    );
    assert_cli(
        &["-dzend.enable_gc=1", "-dzend.enable_gc=Off"],
        OBSERVE,
        "0|\"\"\n",
    );
}

#[test]
fn runtime_control_writers_replace_existing_ini_override() {
    assert_cli(
        &["-dzend.enable_gc=2"],
        r#"
function showGc() { echo (int)gc_enabled(), ':', json_encode(ini_get('zend.enable_gc')), '|'; }
showGc(); gc_enable(); showGc(); gc_disable(); showGc();
foreach (['on', 'off', '2junk', '00', 'garbage', ' 1x', ' on ', '+2x', '0.1', ''] as $setting) {
 echo json_encode(ini_set('zend.enable_gc', $setting)), '>';
 showGc(); gc_disable(); showGc(); gc_enable(); showGc();
}
"#,
        concat!(
            "1:\"2\"|1:\"1\"|0:\"0\"|",
            "\"0\">1:\"on\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\"off\"|0:\"0\"|1:\"1\"|",
            "\"1\">1:\"2junk\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\"00\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\"garbage\"|0:\"0\"|1:\"1\"|",
            "\"1\">1:\" 1x\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\" on \"|0:\"0\"|1:\"1\"|",
            "\"1\">1:\"+2x\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\"0.1\"|0:\"0\"|1:\"1\"|",
            "\"1\">0:\"\"|0:\"0\"|1:\"1\"|",
        ),
    );
}

#[test]
fn explicit_collection_while_disabled_only_sees_admitted_roots() {
    assert_cli(
        &["-dzend.enable_gc=0"],
        r#"
$before = []; $before[] =& $before; unset($before);
echo (int)gc_enabled(), ':', gc_collect_cycles(), '|';
gc_enable(); $later = []; $later[] =& $later; unset($later); gc_disable();
echo (int)gc_enabled(), ':', gc_collect_cycles(), '|';
"#,
        "0:0|0:1|",
    );
}

#[test]
fn first_enablement_keeps_the_buffer_available_after_runtime_disable() {
    let source = r#"
$item = []; $item[] =& $item; unset($item);
echo gc_status()['roots'], '/', gc_collect_cycles(), '/', gc_status()['runs'], '|';
gc_enable(); echo gc_status()['roots'], '/', gc_collect_cycles();
"#;
    for startup in ["0", "1"] {
        for (prefix, enabled) in [
            ("", startup == "1"),
            ("gc_disable();", startup == "1"),
            ("gc_enable();gc_disable();", true),
            ("gc_collect_cycles();", startup == "1"),
            ("ini_set('zend.enable_gc', '1');gc_disable();", true),
        ] {
            assert_cli(
                &[&format!("-dzend.enable_gc={startup}")],
                &format!("{prefix}{source}"),
                if enabled { "1/1/1|0/0" } else { "0/0/0|0/0" },
            );
        }
    }
}

#[test]
fn intermediate_startup_enablement_does_not_initialize_the_root_buffer() {
    assert_cli(
        &[
            "-dzend.enable_gc=0",
            "-dzend.enable_gc=1",
            "-dzend.enable_gc=0",
        ],
        "$item=[];$item[]=&$item;unset($item);echo gc_status()['roots'],'/',gc_collect_cycles();",
        "0/0",
    );
}

#[test]
fn status_exposes_buffer_initialization_not_just_automatic_enablement() {
    assert_cli(
        &["-dzend.enable_gc=0"],
        r#"
for ($phase=0; $phase<3; ++$phase) {
 $status=gc_status();
 foreach (['running','protected','full','runs','collected','threshold','buffer_size','roots'] as $key) echo json_encode($status[$key]), '|';
 if (!$phase) gc_enable(); else gc_disable();
}
"#,
        concat!(
            "false|true|false|0|0|0|0|0|",
            "false|false|false|0|0|10001|16384|0|",
            "false|false|false|0|0|10001|16384|0|",
        ),
    );
}

const AUTOMATIC: &str = r#"
class ConfigCycle { public static $drops = 0; public $self; function __construct() { $this->self = $this; } function __destruct() { ++self::$drops; } }
for ($index = 0; $index < 12000; ++$index) { $item = new ConfigCycle; unset($item); }
echo 'before:', (int)gc_enabled(), ':', ConfigCycle::$drops, '|';
echo 'collect:', gc_collect_cycles(), ':', ConfigCycle::$drops, '|';
gc_enable(); echo 'enabled:', gc_collect_cycles(), ':', ConfigCycle::$drops, '|';
"#;

#[test]
fn disabled_startup_prevents_automatic_root_admission() {
    assert_cli(
        &["-dzend.enable_gc=0"],
        AUTOMATIC,
        "before:0:0|collect:0:0|enabled:0:0|",
    );
}

#[test]
fn enabled_startup_retains_automatic_collection_control() {
    assert_cli(
        &["-dzend.enable_gc=1"],
        AUTOMATIC,
        "before:1:10000|collect:2000:12000|enabled:0:12000|",
    );
}

#[test]
fn gc_configuration_does_not_escape_an_embedded_request() {
    assert_eq!(
        common::run_php("<?php ini_set('zend.enable_gc', '0'); echo (int)gc_enabled();"),
        "0"
    );
    assert_eq!(common::run_php("<?php echo (int)gc_enabled();"), "1");
}
