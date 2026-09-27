mod common;

use common::run_php;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn curl_composer_surface_has_real_extension_metadata_and_signatures() {
    let output = run_php(
        r#"<?php
echo (int) extension_loaded('curl'), ':',
    (new ReflectionClass(CurlHandle::class))->getExtensionName(), ':',
    (new ReflectionClass(CurlMultiHandle::class))->getExtensionName(), ':',
    (new ReflectionClass(CurlShareHandle::class))->getExtensionName(), "\n";
foreach (['curl_init', 'curl_setopt_array', 'curl_exec', 'curl_getinfo', 'curl_multi_exec', 'curl_multi_info_read', 'curl_share_setopt'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':',
        $function->getReturnType(), "\n";
}
$version = curl_version();
echo (int) is_string($version['version']), ':', (int) is_array($version['protocols']), ':',
    (int) in_array('https', $version['protocols'], true), ':',
    (int) ((CURL_VERSION_LIBZ & $version['features']) !== 0), "\n";
echo (int) (CURLOPT_ENCODING === 10102), ':', CURLPROTO_HTTP | CURLPROTO_HTTPS, ':',
    CURLM_OK, ':', CURLE_OK;
"#,
    );
    assert_eq!(
        output,
        concat!(
            "1:curl:curl:curl\n",
            "curl_init:curl:0/1:CurlHandle|false\n",
            "curl_setopt_array:curl:2/2:bool\n",
            "curl_exec:curl:1/1:string|bool\n",
            "curl_getinfo:curl:1/2:mixed\n",
            "curl_multi_exec:curl:2/2:int\n",
            "curl_multi_info_read:curl:1/2:array|false\n",
            "curl_share_setopt:curl:3/3:bool\n",
            "1:1:1:1\n",
            "1:3:0:0",
        )
    );
}

#[test]
fn curl_multi_writes_composer_header_and_body_streams() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback HTTP listener");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().expect("accept cURL client");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = peer.read(&mut chunk).expect("read HTTP request");
            request.extend_from_slice(&chunk[..read]);
            if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        assert!(request.starts_with(b"GET /package.zip HTTP/"));
        assert!(
            request
                .windows(20)
                .any(|window| window == b"X-Composer: contract")
        );
        peer.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: 7\r\nX-Test: rphp\r\n\r\npayload",
        )
        .expect("write HTTP response");
    });

    let output = run_php(&format!(
        r#"<?php
$header = fopen('php://temp', 'w+b');
$body = fopen('php://temp', 'w+b');
$easy = curl_init('http://127.0.0.1:{port}/package.zip');
var_dump(curl_setopt_array($easy, [
    CURLOPT_WRITEHEADER => $header,
    CURLOPT_FILE => $body,
    CURLOPT_HTTPHEADER => ['X-Composer: contract'],
    CURLOPT_FOLLOWLOCATION => false,
    CURLOPT_CONNECTTIMEOUT => 2,
    CURLOPT_TIMEOUT => 2,
    CURLOPT_ENCODING => '',
    CURLOPT_PROTOCOLS => CURLPROTO_HTTP | CURLPROTO_HTTPS,
]));
$share = curl_share_init();
var_dump(curl_share_setopt($share, CURLSHOPT_SHARE, CURL_LOCK_DATA_DNS));
var_dump(curl_setopt($easy, CURLOPT_SHARE, $share));
$multi = curl_multi_init();
echo curl_multi_setopt($multi, CURLMOPT_PIPELINING, 0), ':',
    curl_multi_add_handle($multi, $easy), ':';
do {{
    $status = curl_multi_exec($multi, $running);
}} while ($status === CURLM_CALL_MULTI_PERFORM);
echo $status, ':', $running, "\n";
$queued = -1;
$message = curl_multi_info_read($multi, $queued);
echo $message['msg'], ':', $message['result'], ':', $queued, ':',
    (int) ($message['handle'] === $easy), ':', curl_errno($easy), ':', curl_error($easy), "\n";
$info = curl_getinfo($easy);
echo $info['http_code'], ':', $info['content_type'], ':', (int) str_ends_with($info['url'], '/package.zip'), ':',
    (int) ((int) $easy > 0), "\n";
rewind($header); rewind($body);
$headers = fread($header, 4096);
echo (int) str_contains($headers, 'HTTP/1.1 200 OK'), ':',
    (int) str_contains($headers, 'X-Test: rphp'), ':', fread($body, 4096);
"#,
    ));
    server.join().expect("join loopback HTTP server");
    assert_eq!(
        output,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "1:0:0:0\n",
            "1:0:0:1:0:\n",
            "200:application/zip:1:1\n",
            "1:1:payload",
        )
    );
}

#[test]
fn curl_header_callback_lifetime_survives_error_handler_cycles() {
    assert_eq!(
        run_php(
            r#"<?php
class CurlCallbackOwner {
    public static function header($handle = null, $header = null) { return strlen((string) $header); }
}
$callback = [CurlCallbackOwner::class, 'header'];
$handle = curl_init();
var_dump(curl_setopt($handle, CURLOPT_HEADERFUNCTION, $callback));
set_error_handler($callback);
set_error_handler(function () use ($handle) {});
set_error_handler(function () {});
echo "ok\n";
"#,
        ),
        "bool(true)\nok\n"
    );
}
