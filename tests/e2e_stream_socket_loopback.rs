mod common;

use common::run_php;

#[test]
fn tcp_stream_server_select_accept_and_shutdown_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
$errorCode = 999;
$errorMessage = 'seed';
$server = stream_socket_server(
    'tcp://127.0.0.1:0',
    $errorCode,
    $errorMessage,
    STREAM_SERVER_BIND | STREAM_SERVER_LISTEN,
);
$address = stream_socket_get_name($server, false);
echo (int) is_resource($server), ':', $errorCode, ':', $errorMessage === '', ':';
echo (int) stream_set_blocking($server, false), ':';

$client = stream_socket_client('tcp://' . $address, $errorCode, $errorMessage, 1.0);
$read = [$server];
$write = [];
$except = [];
echo stream_select($read, $write, $except, 1), ':';
$peerName = null;
$peer = stream_socket_accept($server, 0.0, $peerName);
echo (int) is_resource($peer), ':', (int) is_string($peerName), ':';
echo fwrite($client, 'ping'), ':';
$read = [$peer];
$write = [];
$except = [];
echo stream_select($read, $write, $except, 1), ':', fread($peer, 4), ':';
echo (int) stream_socket_shutdown($client, STREAM_SHUT_RDWR);
"#,
        ),
        "1:0:1:1:1:1:1:4:1:ping:1",
    );
}

#[test]
#[cfg(feature = "stream-contents")]
fn nonblocking_stream_contents_returns_the_available_chunk_without_eof() {
    assert_eq!(
        run_php(
            r#"<?php
$server = stream_socket_server('tcp://127.0.0.1:0');
$address = stream_socket_get_name($server, false);
$client = stream_socket_client('tcp://' . $address);
$peer = stream_socket_accept($server);
stream_set_blocking($peer, false);
fwrite($client, "ndjson\n");
$read = [$peer];
$write = [];
$except = [];
echo stream_select($read, $write, $except, 1), ':';
echo bin2hex(stream_get_contents($peer, 65536)), ':', (int) feof($peer);
"#,
        ),
        "1:6e646a736f6e0a:0",
    );
}
