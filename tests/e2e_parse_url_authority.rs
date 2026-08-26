mod common;

use common::run_php;

#[test]
fn parse_url_separates_schemeless_ports_opaque_paths_and_empty_fields() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    '',
    'host:0',
    'host:65535/path',
    'host:65536/path',
    'host:999999',
    'x:12?query',
    'http://1.2.3.4:/p',
    'x://::6.5',
    'file:///a:/',
    'file:///ab:/',
    'http://[::1]:/p',
    'http:///p',
];
foreach ($cases as $url) {
    echo json_encode([$url, parse_url($url)], JSON_UNESCAPED_SLASHES), "\n";
}
"#,
        ),
        concat!(
            "[\"\",{\"path\":\"\"}]\n",
            "[\"host:0\",{\"host\":\"host\",\"port\":0}]\n",
            "[\"host:65535/path\",{\"host\":\"host\",\"port\":65535,\"path\":\"/path\"}]\n",
            "[\"host:65536/path\",false]\n",
            "[\"host:999999\",{\"scheme\":\"host\",\"path\":\"999999\"}]\n",
            "[\"x:12?query\",{\"scheme\":\"x\",\"path\":\"12\",\"query\":\"query\"}]\n",
            "[\"http://1.2.3.4:/p\",{\"scheme\":\"http\",\"host\":\"1.2.3.4\",\"path\":\"/p\"}]\n",
            "[\"x://::6.5\",{\"scheme\":\"x\",\"host\":\":\",\"port\":6}]\n",
            "[\"file:///a:/\",{\"scheme\":\"file\",\"path\":\"a:/\"}]\n",
            "[\"file:///ab:/\",{\"scheme\":\"file\",\"path\":\"/ab:/\"}]\n",
            "[\"http://[::1]:/p\",{\"scheme\":\"http\",\"host\":\"[::1]\",\"path\":\"/p\"}]\n",
            "[\"http:///p\",false]\n",
        )
    );
}

#[test]
fn parse_url_keeps_call_shapes_references_and_component_precedence() {
    assert_eq!(
        run_php(
            r#"<?php
$url = 'user@host:80/path?x#f';
$copy = $url;
$reference =& $url;
$dynamic = 'parse_url';
$firstClass = parse_url(...);
$calls = [
    parse_url($url),
    $dynamic($url),
    $firstClass($url),
    call_user_func('parse_url', $url),
    parse_url(url: $url, component: PHP_URL_HOST),
    parse_url(...[$url, PHP_URL_PORT]),
];
foreach ($calls as $value) {
    echo json_encode($value, JSON_UNESCAPED_SLASHES), "\n";
}
echo "$url|$copy|$reference\n";

$reflection = new ReflectionFunction('parse_url');
echo $reflection->getNumberOfParameters(), '|',
    $reflection->getNumberOfRequiredParameters(), '|',
    implode(',', array_map(fn($parameter) => $parameter->getName(), $reflection->getParameters())),
    "\n";

foreach ([['http://h', 8], ['http://h:65536', 8]] as [$wire, $component]) {
    try {
        var_dump(parse_url($wire, $component));
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "{\"host\":\"host\",\"port\":80,\"user\":\"user\",\"path\":\"/path\",\"query\":\"x\",\"fragment\":\"f\"}\n",
            "{\"host\":\"host\",\"port\":80,\"user\":\"user\",\"path\":\"/path\",\"query\":\"x\",\"fragment\":\"f\"}\n",
            "{\"host\":\"host\",\"port\":80,\"user\":\"user\",\"path\":\"/path\",\"query\":\"x\",\"fragment\":\"f\"}\n",
            "{\"host\":\"host\",\"port\":80,\"user\":\"user\",\"path\":\"/path\",\"query\":\"x\",\"fragment\":\"f\"}\n",
            "\"host\"\n",
            "80\n",
            "user@host:80/path?x#f|user@host:80/path?x#f|user@host:80/path?x#f\n",
            "2|1|url,component\n",
            "ValueError:parse_url(): Argument #2 ($component) must be a valid URL component identifier, 8 given\n",
            "bool(false)\n",
        )
    );
}
