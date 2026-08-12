<?php

declare(strict_types=1);

function normalized_output(string $output, bool $_whitespaceSensitive): string
{
    $output = str_replace(["\r\n", "\r"], "\n", $output);
    // run-tests.php trims only the outer boundary for every expectation. The
    // WHITESPACE_SENSITIVE section protects whitespace inside the output.
    return trim($output);
}

function expectf_pattern(string $expected): string
{
    $expected = str_replace(["\r\n", "\r"], "\n", $expected);
    $quoted = '';
    $offset = 0;
    while ($offset < strlen($expected)) {
        $start = strpos($expected, '%r', $offset);
        if ($start === false) {
            $quoted .= preg_quote(substr($expected, $offset), '~');
            break;
        }
        $end = strpos($expected, '%r', $start + 2);
        if ($end === false) {
            $quoted .= preg_quote(substr($expected, $offset), '~');
            break;
        }
        $quoted .= preg_quote(substr($expected, $offset, $start - $offset), '~');
        $raw = substr($expected, $start + 2, $end - $start - 2);
        $quoted .= '(?:' . str_replace('~', '\\~', $raw) . ')';
        $offset = $end + 2;
    }
    return strtr($quoted, [
        '%e' => preg_quote(DIRECTORY_SEPARATOR, '~'),
        '%s' => '[^\r\n]+',
        '%S' => '[^\r\n]*',
        '%a' => '.+',
        '%A' => '.*',
        '%w' => '\\s*',
        '%i' => '[+-]?\\d+',
        '%d' => '\\d+',
        '%x' => '[0-9a-fA-F]+',
        '%f' => '[+-]?(?:\\d+|(?=\\.\\d))(?:\\.\\d+)?(?:[Ee][+-]?\\d+)?',
        '%c' => '.',
        '%0' => '\\x00',
        '%%' => '%',
    ]);
}

/** @return array{bool, string, string} */
function compare_output(array $sections, string $actual): array
{
    $whitespaceSensitive = isset($sections['WHITESPACE_SENSITIVE']);
    $actual = normalized_output($actual, $whitespaceSensitive);
    if (array_key_exists('EXPECT', $sections)) {
        $expected = normalized_output($sections['EXPECT'], $whitespaceSensitive);
        return [$actual === $expected, 'EXPECT', $expected];
    }
    if (array_key_exists('EXPECTF', $sections)) {
        $expected = normalized_output($sections['EXPECTF'], $whitespaceSensitive);
        $pattern = '~\\A' . expectf_pattern($expected) . '\\z~sD';
        return [preg_match($pattern, $actual) === 1, 'EXPECTF', $expected];
    }
    if (array_key_exists('EXPECTREGEX', $sections)) {
        $expected = normalized_output($sections['EXPECTREGEX'], $whitespaceSensitive);
        $pattern = '~\\A(?:' . str_replace('~', '\\~', $expected) . ')\\z~sD';
        return [preg_match($pattern, $actual) === 1, 'EXPECTREGEX', $expected];
    }
    throw new RuntimeException('missing EXPECT, EXPECTF or EXPECTREGEX section');
}

function output_excerpt(string $output): string
{
    $output = str_replace(["\r\n", "\r"], "\n", trim($output));
    if (strlen($output) > 240) {
        return substr($output, 0, 237) . '...';
    }
    return $output;
}

function normalized_runtime_output(string $output): string
{
    return preg_replace(
        "/thread 'main' \\(\\d+\\)/",
        "thread 'main' (<id>)",
        $output,
    ) ?? $output;
}

/** @return array<string, mixed> */
function base_result(string $path, array $sections): array
{
    return [
        'path' => $path,
        'test' => trim($sections['TEST'] ?? ''),
        'status' => 'fail',
        'category' => 'runner',
        'expectation' => '',
        'duration_ms' => 0,
        'exit_code' => null,
        'reason' => '',
        'actual_sha256' => null,
        'expected_sha256' => null,
        'actual_excerpt' => '',
    ];
}

function classify_failure(string $output, int $exitCode): string
{
    if (preg_match('/(?:^|\n)Parse error:/i', $output) === 1) {
        return 'parse';
    }
    if (preg_match('/(?:^|\n)Fatal error:.*(?:compile|unsupported|declaration|default)/i', $output) === 1) {
        return 'compile';
    }
    return $exitCode === 0 ? 'output' : 'runtime';
}
