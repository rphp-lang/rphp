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

/**
 * Separate exact PHP diagnostics from ordinary program-output expectations.
 *
 * This is deliberately conservative: only a diagnostic label at the start of
 * an expected output line qualifies. User data that happens to contain words
 * such as "Error" remains ordinary output.
 */
function expectation_profile(array $sections): string
{
    $expected = $sections['EXPECT']
        ?? $sections['EXPECTF']
        ?? $sections['EXPECTREGEX']
        ?? '';
    $expected = str_replace(["\r\n", "\r"], "\n", $expected);
    return preg_match(
        '/(?:\A|\n)(?:PHP )?(?:Fatal error|Parse error|Warning|Deprecated|Notice|Strict Standards|Recoverable fatal error):/i',
        $expected,
    ) === 1
        ? 'diagnostic'
        : 'ordinary';
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
        'expectation_profile' => expectation_profile($sections),
        // Internal runner fields. The merged public manifest omits them, but
        // the aggregate execution profile needs to distinguish FILE failures
        // from SKIPIF/runner failures and exact negative-test passes.
        'test_file_executed' => false,
        'front_end_rejected' => false,
    ];
}

function classify_failure(string $output, int $exitCode): string
{
    if ($exitCode !== 0 && preg_match('/(?:^|\n)Parse error:/i', $output) === 1) {
        return 'parse';
    }
    // A rendered uncaught Throwable necessarily reached runtime. Check this
    // before compile-diagnostic keywords: source paths such as
    // `type_declarations/foo.php` must not turn a runtime Error into a compile
    // failure merely because the conservative fallback scans the full line.
    if ($exitCode !== 0 && preg_match('/(?:^|\n)Fatal error:\s+Uncaught\s+/i', $output) === 1) {
        return 'runtime';
    }
    if ($exitCode !== 0
        && preg_match('/(?:^|\n)Fatal error:.*(?:compile|unsupported|declaration|default)/i', $output) === 1
    ) {
        return 'compile';
    }
    return $exitCode === 0 ? 'output' : 'runtime';
}
