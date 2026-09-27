mod common;

use common::{run_php, run_php_with_source_context};
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn phpunit_remaining_builtin_signatures_and_method_source_origin_match_php_85() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class ParentSource {
    public function inherited(): void {}
}
class ChildSource extends ParentSource {
    public function own(): void {}
}
$closure = function (): void {};
foreach ([
    new ReflectionMethod(ChildSource::class, 'inherited'),
    new ReflectionMethod(ChildSource::class, 'own'),
    new ReflectionMethod($closure, '__invoke'),
    new ReflectionMethod(ReflectionMethod::class, 'getStartLine'),
] as $method) {
    var_export($method->getFileName());
    echo ': '; var_export($method->getStartLine()); echo '|';
}
echo (new ReflectionMethod(ReflectionMethod::class, 'getStartLine'))->getDeclaringClass()->getName(), "\n";
foreach (['fsockopen', 'pcntl_alarm', 'pcntl_async_signals', 'pcntl_signal'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->hasReturnType() ? (string) $function->getReturnType() : '-', ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), '|';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':', $parameter->isPassedByReference() ? '&' : '-', ':',
            $parameter->hasType() ? (string) $parameter->getType() : '-', ':';
        echo $parameter->isDefaultValueAvailable() ? var_export($parameter->getDefaultValue(), true) : '-';
        echo '|';
    }
    echo "\n";
}
echo (int) extension_loaded('pcntl'), ':', SIGINT, ':', SIGALRM, ':', SIG_DFL, ':', SIG_IGN;
"#,
            "/spec/phpunit-runtime.php",
            "/spec",
        ),
        concat!(
            "'/spec/phpunit-runtime.php': 3|",
            "'/spec/phpunit-runtime.php': 6|",
            "'/spec/phpunit-runtime.php': 8|",
            "false: false|ReflectionFunctionAbstract\n",
            "fsockopen:standard:-:1/5|hostname:-:string:-|port:-:int:-1|error_code:&:-:NULL|error_message:&:-:NULL|timeout:-:?float:NULL|\n",
            "pcntl_alarm:pcntl:int:1/1|seconds:-:int:-|\n",
            "pcntl_async_signals:pcntl:bool:0/1|enable:-:?bool:NULL|\n",
            "pcntl_signal:pcntl:bool:2/3|signal:-:int:-|handler:-:-:-|restart_syscalls:-:bool:true|\n",
            "1:2:14:0:1",
        ),
    );
}

#[test]
fn fsockopen_connects_a_php_stream_and_publishes_output_parameters() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().expect("accept fsockopen client");
        let mut request = [0_u8; 4];
        peer.read_exact(&mut request).expect("read client bytes");
        assert_eq!(&request, b"ping");
        peer.write_all(b"pong").expect("write client bytes");
    });

    let output = run_php(&format!(
        r#"<?php
$errorCode = 999;
$errorMessage = 'seed';
$stream = fsockopen('127.0.0.1', {port}, $errorCode, $errorMessage, 2.0);
$metadata = stream_get_meta_data($stream);
echo get_resource_type($stream), ':', $errorCode, ':', $errorMessage, ':',
    $metadata['stream_type'], ':', $metadata['mode'], ':', (int) $metadata['seekable'], ':',
    (int) array_key_exists('wrapper_type', $metadata), '|';
echo fwrite($stream, 'ping'), ':', fread($stream, 4), ':', (int) fclose($stream);
"#,
    ));
    server.join().expect("join loopback server");
    assert_eq!(output, "stream:0::tcp_socket/ssl:r+:0:0|4:pong:1");
}

#[test]
fn pcntl_alarm_dispatches_a_catchable_php_handler_and_cancels_cleanly() {
    let unrelated_request = std::thread::spawn(|| {
        for _ in 0..16 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(run_php("<?php echo 'peer';"), "peer");
        }
    });
    let output = run_php(
        r#"<?php
echo (int) pcntl_async_signals(), ':';
pcntl_signal(SIGALRM, static function (int $signal): void {
    throw new RuntimeException("alarm:$signal");
});
echo (int) pcntl_async_signals(true), ':', (int) pcntl_async_signals(), ':';
try {
    pcntl_alarm(1);
    $until = microtime(true) + 2.5;
    while (microtime(true) < $until) {}
    echo 'miss';
} catch (RuntimeException $error) {
    echo $error->getMessage();
} finally {
    echo ':', pcntl_alarm(0);
}
"#,
    );
    unrelated_request.join().expect("join unrelated request");
    assert_eq!(output, "0:0:1:alarm:14:0");

    // Request shutdown must also cancel an alarm when no user handler was
    // installed; otherwise SIGALRM would terminate this test process.
    assert_eq!(run_php("<?php echo pcntl_alarm(1);"), "0");
    std::thread::sleep(std::time::Duration::from_millis(1_200));
}
