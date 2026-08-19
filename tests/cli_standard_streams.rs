use std::io::Write;
use std::process::{Command, Stdio};

fn run(source: &str, input: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(input)
        .expect("runtime stdin should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn cli_standard_streams_have_stable_identity_and_stdio_metadata() {
    let source = r#"
foreach (['STDIN' => STDIN, 'STDOUT' => STDOUT, 'STDERR' => STDERR] as $name => $stream) {
    $metadata = stream_get_meta_data($stream);
    echo $name, ':', (int) defined($name), ':', (int) ($stream === constant($name)), ':',
        get_resource_id($stream), ':', get_resource_type($stream), ':',
        $metadata['wrapper_type'], ':', $metadata['stream_type'], ':',
        $metadata['mode'], ':', (int) $metadata['seekable'], ':', $metadata['uri'], "\n";
}
"#;
    let (status, stdout, stderr) = run(source, b"");

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "STDIN:1:1:1:stream:PHP:STDIO:rb:0:php://stdin\n",
            "STDOUT:1:1:2:stream:PHP:STDIO:wb:0:php://stdout\n",
            "STDERR:1:1:3:stream:PHP:STDIO:wb:0:php://stderr\n",
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn standard_stream_io_uses_the_process_channels_and_runtime_stdin() {
    let source = r#"
fwrite(STDOUT, 'stdout>');
fwrite(STDERR, 'stderr>');
echo fread(STDIN, 13), ':', (int) feof(STDIN);
"#;
    let (status, stdout, stderr) = run(source, b"runtime-input");

    assert_eq!(status, 0);
    assert_eq!(stdout, "stdout>runtime-input:0");
    assert_eq!(stderr, "stderr>");
}

#[test]
fn standard_stream_direction_and_close_state_match_resources() {
    let source = r#"
var_dump(@fwrite(STDIN, 'x'), @fread(STDOUT, 1), @fread(STDERR, 1));
$alias = STDIN;
var_dump(fclose($alias), defined('STDIN'), is_resource(STDIN), get_resource_type(STDIN), STDIN === $alias);
"#;
    let (status, stdout, stderr) = run(source, b"");

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "bool(false)\n",
            "bool(false)\n",
            "bool(false)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "string(7) \"Unknown\"\n",
            "bool(true)\n",
        )
    );
    assert_eq!(stderr, "");
}

#[test]
fn standard_streams_resolve_in_runtime_constant_expression_positions() {
    let source = r#"
const GLOBAL_INPUT = STDIN;
class StandardDefaults {
    public const OUTPUT = STDOUT;
    public $error = STDERR;
}
#[Attribute]
class StandardStreamAttribute {
    public function __construct(public $stream) {}
}
#[StandardStreamAttribute(STDIN)]
class AttributedTarget {}
function parameter_default($stream = STDOUT) { return $stream; }
function static_default() { static $stream = STDIN; return $stream; }
$object = new StandardDefaults();
$attribute = (new ReflectionClass(AttributedTarget::class))->getAttributes()[0]->newInstance();
echo get_resource_id(GLOBAL_INPUT), ':', get_resource_id(StandardDefaults::OUTPUT), ':',
    get_resource_id($object->error), ':', get_resource_id(parameter_default()), ':',
    get_resource_id(static_default()), ':', get_resource_id($attribute->stream);
"#;
    let (status, stdout, stderr) = run(source, b"");

    assert_eq!(status, 0);
    assert_eq!(stdout, "1:2:3:2:1:1");
    assert_eq!(stderr, "");
}

#[test]
fn resource_array_keys_warn_and_use_their_integer_identity() {
    let source = r#"
set_error_handler(function ($severity, $message) { echo '[', $message, "]\n"; });
$array = [STDIN => 'input'];
$array[STDOUT] = 'output';
var_dump($array[STDIN], isset($array[STDOUT]));
unset($array[STDOUT]);
$GLOBALS[STDERR] = 'global';
var_dump($GLOBALS['Resource id #3']);
[$first, $second] = STDERR;
var_dump($first, $second);
"#;
    let (status, stdout, stderr) = run(source, b"");

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        concat!(
            "[Resource ID#1 used as offset, casting to integer (1)]\n",
            "[Resource ID#2 used as offset, casting to integer (2)]\n",
            "[Resource ID#1 used as offset, casting to integer (1)]\n",
            "[Resource ID#2 used as offset, casting to integer (2)]\n",
            "string(5) \"input\"\n",
            "bool(true)\n",
            "[Resource ID#2 used as offset, casting to integer (2)]\n",
            "string(6) \"global\"\n",
            "[Cannot use resource as array]\n",
            "[Cannot use resource as array]\n",
            "NULL\n",
            "NULL\n",
        )
    );
    assert_eq!(stderr, "");
}
