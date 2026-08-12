#!/usr/bin/env php
<?php

declare(strict_types=1);

const RPHP_PHPT_SUPPORTED_SECTIONS = [
    'TEST',
    'DESCRIPTION',
    'CREDITS',
    'SKIPIF',
    'EXTENSIONS',
    'INI',
    'ENV',
    'ARGS',
    'STDIN',
    'CLEAN',
    'FILE',
    'FILEEOF',
    'FILE_EXTERNAL',
    'EXPECT',
    'EXPECTF',
    'EXPECTREGEX',
    'WHITESPACE_SENSITIVE',
    'XFAIL',
    'FLAKY',
];

function fail_usage(string $message = ''): never
{
    if ($message !== '') {
        fwrite(STDERR, "error: {$message}\n\n");
    }
    fwrite(STDERR, <<<'USAGE'
usage:
  phpt-runner.php run --suite-root DIR --target BIN --manifest FILE
      [--target-kind rphp|php] [--timeout SECONDS]
      [--shard-index N --shard-count N] PATH...
  phpt-runner.php merge --manifest FILE --summary FILE
      [--rphp-commit HASH] [--php-src-commit HASH] [--features LABEL]
      [--architecture LABEL] [--target-label LABEL] [--timeout SECONDS]
      SHARD.jsonl...

PATH is relative to --suite-root and may name a PHPT file or a directory.
USAGE);
    exit(2);
}

/** @return array{array<string, string>, list<string>} */
function parse_options(array $arguments): array
{
    $options = [];
    $positionals = [];
    for ($index = 0; $index < count($arguments); $index++) {
        $argument = $arguments[$index];
        if (!str_starts_with($argument, '--')) {
            $positionals[] = $argument;
            continue;
        }
        $name = substr($argument, 2);
        if ($name === '' || $index + 1 >= count($arguments)) {
            fail_usage("option {$argument} requires a value");
        }
        $options[$name] = $arguments[++$index];
    }
    return [$options, $positionals];
}

function required_option(array $options, string $name): string
{
    $value = $options[$name] ?? '';
    if ($value === '') {
        fail_usage("missing --{$name}");
    }
    return $value;
}

function positive_number(string $value, string $name, bool $allowZero = false): float
{
    if (!is_numeric($value)) {
        fail_usage("--{$name} must be numeric");
    }
    $number = (float) $value;
    if (($allowZero && $number < 0) || (!$allowZero && $number <= 0)) {
        fail_usage("--{$name} is outside its valid range");
    }
    return $number;
}

/** @return array<string, string> */
function parse_sections(string $path): array
{
    $source = file_get_contents($path);
    if ($source === false) {
        throw new RuntimeException("cannot read {$path}");
    }
    $source = str_replace(["\r\n", "\r"], "\n", $source);
    $sectionLines = [];
    $current = null;
    foreach (explode("\n", $source) as $line) {
        if (preg_match('/^--([A-Z_]+)--$/D', $line, $match) === 1) {
            $current = $match[1];
            if (isset($sectionLines[$current])) {
                throw new RuntimeException("duplicate --{$current}-- section");
            }
            $sectionLines[$current] = [];
            continue;
        }
        if ($current !== null) {
            $sectionLines[$current][] = $line;
        }
    }
    $sections = [];
    foreach ($sectionLines as $name => $lines) {
        $sections[$name] = implode("\n", $lines);
    }
    return $sections;
}

/** @return list<string> */
function collect_tests(string $root, array $paths): array
{
    $tests = [];
    foreach ($paths as $relative) {
        $path = $root . DIRECTORY_SEPARATOR . $relative;
        if (is_file($path)) {
            if (str_ends_with($path, '.phpt')) {
                $tests[] = realpath($path) ?: $path;
            }
            continue;
        }
        if (!is_dir($path)) {
            throw new RuntimeException("suite path does not exist: {$relative}");
        }
        $iterator = new RecursiveIteratorIterator(
            new RecursiveDirectoryIterator($path, FilesystemIterator::SKIP_DOTS),
        );
        foreach ($iterator as $entry) {
            if ($entry->isFile() && str_ends_with($entry->getFilename(), '.phpt')) {
                $tests[] = $entry->getRealPath() ?: $entry->getPathname();
            }
        }
    }
    sort($tests, SORT_STRING);
    return array_values(array_unique($tests));
}

/** @return array<string, string> */
function process_environment(string $section): array
{
    $environment = getenv();
    if (!is_array($environment)) {
        $environment = [];
    }
    foreach (explode("\n", $section) as $line) {
        $line = trim($line);
        if ($line === '' || str_starts_with($line, ';')) {
            continue;
        }
        $separator = strpos($line, '=');
        if ($separator === false) {
            throw new RuntimeException("invalid ENV entry: {$line}");
        }
        $environment[substr($line, 0, $separator)] = substr($line, $separator + 1);
    }
    return $environment;
}

/** @return list<string> */
function ini_arguments(string $section): array
{
    $arguments = [];
    foreach (explode("\n", $section) as $line) {
        $line = trim($line);
        if ($line === '' || str_starts_with($line, ';')) {
            continue;
        }
        $arguments[] = '-d';
        $arguments[] = $line;
    }
    return $arguments;
}

/** @return list<string> */
function script_arguments(string $section): array
{
    $section = trim($section);
    if ($section === '') {
        return [];
    }
    $arguments = str_getcsv($section, ' ', '"', '\\');
    return array_values(array_filter($arguments, static fn(string $value): bool => $value !== ''));
}

/**
 * @return array{output: string, exit_code: int, timeout: bool, crash: bool, duration_ms: int}
 */
function run_process(
    array $command,
    string $cwd,
    array $environment,
    string $stdin,
    float $timeout,
): array {
    $descriptors = [
        0 => ['pipe', 'r'],
        1 => ['pipe', 'w'],
        2 => ['redirect', 1],
    ];
    $start = hrtime(true);
    $process = proc_open(
        $command,
        $descriptors,
        $pipes,
        $cwd,
        $environment,
        ['bypass_shell' => true],
    );
    if (!is_resource($process)) {
        throw new RuntimeException('cannot start target process');
    }
    fwrite($pipes[0], $stdin);
    fclose($pipes[0]);
    stream_set_blocking($pipes[1], false);
    $output = '';
    $timedOut = false;
    $lastStatus = proc_get_status($process);
    while ($lastStatus['running']) {
        $chunk = stream_get_contents($pipes[1]);
        if ($chunk !== false) {
            $output .= $chunk;
        }
        if ((hrtime(true) - $start) / 1_000_000_000 > $timeout) {
            $timedOut = true;
            proc_terminate($process);
            usleep(20_000);
            $status = proc_get_status($process);
            if ($status['running']) {
                proc_terminate($process, 9);
            }
            break;
        }
        usleep(5_000);
        $lastStatus = proc_get_status($process);
    }
    stream_set_blocking($pipes[1], true);
    $tail = stream_get_contents($pipes[1]);
    if ($tail !== false) {
        $output .= $tail;
    }
    fclose($pipes[1]);
    $terminalStatus = $lastStatus;
    $afterReadStatus = proc_get_status($process);
    if ($terminalStatus['running'] && !$afterReadStatus['running']) {
        $terminalStatus = $afterReadStatus;
    }
    $closedCode = proc_close($process);
    $exitCode = $terminalStatus['exitcode'] >= 0 ? $terminalStatus['exitcode'] : $closedCode;
    $crash = !$timedOut && (
        ($terminalStatus['signaled'] ?? false)
        || ($terminalStatus['termsig'] ?? 0) !== 0
    );
    return [
        'output' => $output,
        'exit_code' => $exitCode,
        'timeout' => $timedOut,
        'crash' => $crash,
        'duration_ms' => (int) round((hrtime(true) - $start) / 1_000_000),
    ];
}

/** @return list<string> */
function target_command(
    string $target,
    string $kind,
    string $file,
    string $ini,
    string $args,
): array {
    if ($kind === 'rphp') {
        return [$target, $file, ...script_arguments($args)];
    }
    return [
        $target,
        '-d',
        'display_errors=1',
        '-d',
        'display_startup_errors=0',
        '-d',
        'html_errors=0',
        '-d',
        'log_errors=0',
        '-d',
        'error_reporting=-1',
        ...ini_arguments($ini),
        $file,
        ...script_arguments($args),
    ];
}

function normalized_output(string $output, bool $whitespaceSensitive): string
{
    $output = str_replace(["\r\n", "\r"], "\n", $output);
    return $whitespaceSensitive ? $output : trim($output);
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

function normalized_runtime_output(string $output, array $pathReplacements): string
{
    if ($pathReplacements !== []) {
        $output = str_replace(array_keys($pathReplacements), array_values($pathReplacements), $output);
    }
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

/** @return list<string> */
function required_extensions(string $section): array
{
    $extensions = preg_split('/\s+/', trim($section)) ?: [];
    return array_values(array_filter(array_map('strtolower', $extensions)));
}

/** @return array<string, true> */
function loaded_extensions(string $target, string $kind, float $timeout): array
{
    if ($kind === 'rphp') {
        return [];
    }
    $result = run_process(
        [$target, '-r', 'echo implode("\\n", get_loaded_extensions());'],
        getcwd() ?: '.',
        is_array(getenv()) ? getenv() : [],
        '',
        $timeout,
    );
    if ($result['exit_code'] !== 0 || $result['timeout']) {
        throw new RuntimeException('cannot query target extensions');
    }
    $loaded = [];
    foreach (explode("\n", strtolower(trim($result['output']))) as $extension) {
        if ($extension !== '') {
            $loaded[$extension] = true;
        }
    }
    return $loaded;
}

/** @return array<string, mixed> */
function run_test(
    string $absolutePath,
    string $relativePath,
    string $target,
    string $kind,
    float $timeout,
    array $loadedExtensions,
    int $shard,
): array {
    try {
        $sections = parse_sections($absolutePath);
    } catch (Throwable $error) {
        return [
            'path' => $relativePath,
            'test' => '',
            'status' => 'unsupported',
            'category' => 'runner-parse',
            'expectation' => '',
            'duration_ms' => 0,
            'exit_code' => null,
            'reason' => $error->getMessage(),
            'actual_sha256' => null,
            'expected_sha256' => null,
            'actual_excerpt' => '',
        ];
    }
    $result = base_result($relativePath, $sections);
    $unknown = array_values(array_diff(array_keys($sections), RPHP_PHPT_SUPPORTED_SECTIONS));
    if ($unknown !== []) {
        $result['status'] = 'unsupported';
        $result['category'] = 'section';
        $result['reason'] = 'unsupported PHPT section(s): ' . implode(', ', $unknown);
        return $result;
    }
    if ($kind === 'rphp' && isset($sections['INI']) && trim($sections['INI']) !== '') {
        $result['status'] = 'unsupported';
        $result['category'] = 'runtime-cli-ini';
        $result['reason'] = 'RPHP CLI does not yet expose per-process INI settings';
        return $result;
    }
    if ($kind === 'rphp' && isset($sections['ARGS']) && trim($sections['ARGS']) !== '') {
        $result['status'] = 'unsupported';
        $result['category'] = 'runtime-cli-args';
        $result['reason'] = 'RPHP CLI does not yet expose script arguments';
        return $result;
    }
    if (isset($sections['EXTENSIONS'])) {
        $missing = [];
        foreach (required_extensions($sections['EXTENSIONS']) as $extension) {
            if (!isset($loadedExtensions[$extension])) {
                $missing[] = $extension;
            }
        }
        if ($missing !== []) {
            $result['status'] = 'skip';
            $result['category'] = 'extension';
            $result['reason'] = 'required extension unavailable: ' . implode(', ', $missing);
            return $result;
        }
    }

    $directory = dirname($absolutePath);
    $stem = basename($absolutePath, '.phpt') . ".rphp-phpt-" . getmypid() . "-{$shard}";
    $temporaryFiles = [];
    $environment = process_environment($sections['ENV'] ?? '');
    $ini = $sections['INI'] ?? '';
    $args = $sections['ARGS'] ?? '';
    $totalDuration = 0;

    try {
        if (isset($sections['SKIPIF'])) {
            $skipFile = $directory . DIRECTORY_SEPARATOR . $stem . '.skip.php';
            if (file_put_contents($skipFile, $sections['SKIPIF']) === false) {
                throw new RuntimeException('cannot create SKIPIF file');
            }
            $temporaryFiles[] = $skipFile;
            $skip = run_process(
                target_command($target, $kind, $skipFile, $ini, ''),
                $directory,
                $environment,
                '',
                $timeout,
            );
            $totalDuration += $skip['duration_ms'];
            if ($skip['timeout']) {
                $result['status'] = 'timeout';
                $result['category'] = 'skipif';
                $result['reason'] = 'SKIPIF timed out';
                return $result;
            }
            if ($skip['crash']) {
                $result['status'] = 'crash';
                $result['category'] = 'skipif';
                $result['reason'] = 'SKIPIF target crashed';
                return $result;
            }
            $skipOutput = normalized_output(
                normalized_runtime_output($skip['output'], [$skipFile => $relativePath]),
                false,
            );
            if (preg_match('/^skip(?:\s|$)/i', $skipOutput) === 1) {
                $result['status'] = 'skip';
                $result['category'] = 'skipif';
                $result['reason'] = trim(preg_replace('/^skip\s*/i', '', $skipOutput) ?? '');
                return $result;
            }
            if ($skip['exit_code'] !== 0 || $skipOutput !== '') {
                $result['status'] = 'fail';
                $result['category'] = 'skipif';
                $result['exit_code'] = $skip['exit_code'];
                $result['reason'] = 'SKIPIF did not return an empty or skip result';
                $result['actual_excerpt'] = output_excerpt($skipOutput);
                return $result;
            }
        }

        if (isset($sections['FILE_EXTERNAL'])) {
            $testFile = realpath($directory . DIRECTORY_SEPARATOR . trim($sections['FILE_EXTERNAL']));
            if ($testFile === false || !is_file($testFile)) {
                throw new RuntimeException('FILE_EXTERNAL target does not exist');
            }
        } else {
            $fileSection = $sections['FILE'] ?? $sections['FILEEOF'] ?? null;
            if ($fileSection === null) {
                throw new RuntimeException('missing FILE, FILEEOF or FILE_EXTERNAL section');
            }
            $testFile = $directory . DIRECTORY_SEPARATOR . $stem . '.php';
            if (file_put_contents($testFile, $fileSection) === false) {
                throw new RuntimeException('cannot create FILE section');
            }
            $temporaryFiles[] = $testFile;
        }

        $execution = run_process(
            target_command($target, $kind, $testFile, $ini, $args),
            $directory,
            $environment,
            $sections['STDIN'] ?? '',
            $timeout,
        );
        $totalDuration += $execution['duration_ms'];
        $actual = normalized_runtime_output(
            $execution['output'],
            [$testFile => $relativePath, $absolutePath => $relativePath],
        );
        $result['duration_ms'] = $totalDuration;
        $result['exit_code'] = $execution['exit_code'];
        $result['actual_sha256'] = hash('sha256', normalized_output($actual, false));
        $result['actual_excerpt'] = output_excerpt($actual);

        if ($execution['timeout']) {
            $result['status'] = 'timeout';
            $result['category'] = 'runtime';
            $result['reason'] = 'test timed out';
        } elseif ($execution['crash']) {
            $result['status'] = 'crash';
            $result['category'] = 'runtime';
            $result['reason'] = 'target terminated by signal or crash exit';
        } else {
            [$matched, $expectation, $expected] = compare_output($sections, $actual);
            $result['expectation'] = $expectation;
            $result['expected_sha256'] = hash('sha256', $expected);
            if ($matched) {
                $result['status'] = 'pass';
                $result['category'] = 'pass';
                $result['reason'] = '';
                $result['actual_excerpt'] = '';
            } else {
                $result['status'] = 'fail';
                $result['category'] = isset($sections['XFAIL'])
                    ? 'xfail'
                    : classify_failure($actual, $execution['exit_code']);
                $result['reason'] = isset($sections['XFAIL'])
                    ? trim($sections['XFAIL'])
                    : 'actual output does not match ' . $expectation;
            }
        }

        if (isset($sections['CLEAN'])) {
            $cleanFile = $directory . DIRECTORY_SEPARATOR . $stem . '.clean.php';
            if (file_put_contents($cleanFile, $sections['CLEAN']) === false) {
                throw new RuntimeException('cannot create CLEAN file');
            }
            $temporaryFiles[] = $cleanFile;
            $clean = run_process(
                target_command($target, $kind, $cleanFile, $ini, ''),
                $directory,
                $environment,
                '',
                $timeout,
            );
            $result['duration_ms'] += $clean['duration_ms'];
            $cleanOutput = normalized_output(
                normalized_runtime_output($clean['output'], [$cleanFile => $relativePath]),
                false,
            );
            if ($clean['timeout'] || $clean['crash'] || $clean['exit_code'] !== 0 || $cleanOutput !== '') {
                if ($result['status'] === 'pass') {
                    $result['status'] = $clean['timeout'] ? 'timeout' : ($clean['crash'] ? 'crash' : 'fail');
                    $result['category'] = 'clean';
                    $result['reason'] = 'CLEAN did not complete silently';
                    $result['actual_excerpt'] = output_excerpt($cleanOutput);
                }
            }
        }
    } catch (Throwable $error) {
        $result['status'] = 'unsupported';
        $result['category'] = 'runner';
        $result['reason'] = $error->getMessage();
    } finally {
        foreach ($temporaryFiles as $temporaryFile) {
            @unlink($temporaryFile);
        }
    }
    return $result;
}

function run_command(array $options, array $paths): void
{
    $root = realpath(required_option($options, 'suite-root'));
    if ($root === false || !is_dir($root)) {
        fail_usage('--suite-root is not a directory');
    }
    $target = realpath(required_option($options, 'target'));
    if ($target === false || !is_executable($target)) {
        fail_usage('--target is not executable');
    }
    $manifest = required_option($options, 'manifest');
    $kind = $options['target-kind'] ?? 'rphp';
    if (!in_array($kind, ['rphp', 'php'], true)) {
        fail_usage('--target-kind must be rphp or php');
    }
    $timeout = positive_number($options['timeout'] ?? '3', 'timeout');
    $shardIndex = (int) positive_number($options['shard-index'] ?? '0', 'shard-index', true);
    $shardCount = (int) positive_number($options['shard-count'] ?? '1', 'shard-count');
    if ($shardIndex >= $shardCount) {
        fail_usage('--shard-index must be less than --shard-count');
    }
    if ($paths === []) {
        fail_usage('at least one suite path is required');
    }
    $tests = collect_tests($root, $paths);
    $loaded = loaded_extensions($target, $kind, $timeout);
    $handle = fopen($manifest, 'wb');
    if ($handle === false) {
        throw new RuntimeException("cannot create manifest {$manifest}");
    }
    $completed = 0;
    foreach ($tests as $index => $test) {
        if ($index % $shardCount !== $shardIndex) {
            continue;
        }
        $relative = str_replace(DIRECTORY_SEPARATOR, '/', substr($test, strlen($root) + 1));
        $result = run_test($test, $relative, $target, $kind, $timeout, $loaded, $shardIndex);
        fwrite($handle, json_encode($result, JSON_UNESCAPED_SLASHES) . "\n");
        $completed++;
        if ($completed % 100 === 0) {
            fwrite(STDERR, "shard {$shardIndex}: {$completed} cases\n");
        }
    }
    fclose($handle);
    fwrite(STDERR, "shard {$shardIndex}: completed {$completed} cases\n");
}

function merge_command(array $options, array $manifests): void
{
    if ($manifests === []) {
        fail_usage('at least one shard manifest is required');
    }
    $outputPath = required_option($options, 'manifest');
    $summaryPath = required_option($options, 'summary');
    $records = [];
    foreach ($manifests as $manifest) {
        $handle = fopen($manifest, 'rb');
        if ($handle === false) {
            throw new RuntimeException("cannot read shard manifest {$manifest}");
        }
        while (($line = fgets($handle)) !== false) {
            $record = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
            $records[] = $record;
        }
        fclose($handle);
    }
    usort($records, static fn(array $left, array $right): int => $left['path'] <=> $right['path']);
    $handle = fopen($outputPath, 'wb');
    if ($handle === false) {
        throw new RuntimeException("cannot create merged manifest {$outputPath}");
    }
    foreach ($records as $record) {
        $published = [
            'path' => $record['path'],
            'status' => $record['status'],
            'category' => $record['category'],
        ];
        if (in_array($record['status'], ['skip', 'unsupported', 'timeout', 'crash'], true)) {
            $published['reason'] = $record['reason'];
        }
        fwrite($handle, json_encode($published, JSON_UNESCAPED_SLASHES) . "\n");
    }
    fclose($handle);

    $statuses = array_fill_keys(['pass', 'fail', 'skip', 'unsupported', 'timeout', 'crash'], 0);
    $categories = [];
    $suites = [];
    foreach ($records as $record) {
        $statuses[$record['status']] = ($statuses[$record['status']] ?? 0) + 1;
        $categories[$record['category']] = ($categories[$record['category']] ?? 0) + 1;
        $suite = str_starts_with($record['path'], 'Zend/tests/') ? 'Zend/tests' : 'tests/lang';
        if (!isset($suites[$suite])) {
            $suites[$suite] = array_fill_keys(
                ['pass', 'fail', 'skip', 'unsupported', 'timeout', 'crash'],
                0,
            );
        }
        $suites[$suite][$record['status']]++;
    }
    ksort($categories);
    ksort($suites);
    foreach ($suites as &$suiteStatuses) {
        $suiteHeadline = $suiteStatuses['pass'] + $suiteStatuses['fail'];
        $suiteAttempted = $suiteHeadline + $suiteStatuses['timeout'] + $suiteStatuses['crash'];
        $suiteStatuses['total'] = array_sum($suiteStatuses);
        $suiteStatuses['headline_pass_rate'] = $suiteHeadline === 0
            ? null
            : $suiteStatuses['pass'] / $suiteHeadline;
        $suiteStatuses['attempted_pass_rate'] = $suiteAttempted === 0
            ? null
            : $suiteStatuses['pass'] / $suiteAttempted;
    }
    unset($suiteStatuses);
    $headlineDenominator = $statuses['pass'] + $statuses['fail'];
    $attemptedDenominator = $headlineDenominator + $statuses['timeout'] + $statuses['crash'];
    $summary = [
        'schema_version' => 1,
        'rphp_commit' => $options['rphp-commit'] ?? '',
        'php_src_commit' => $options['php-src-commit'] ?? '',
        'features' => $options['features'] ?? '',
        'architecture' => $options['architecture'] ?? php_uname('m'),
        'target' => $options['target-label'] ?? 'rphp',
        'timeout_seconds' => (float) ($options['timeout'] ?? 3),
        'total' => count($records),
        'statuses' => $statuses,
        'categories' => $categories,
        'suites' => $suites,
        'headline_pass_rate' => $headlineDenominator === 0
            ? null
            : $statuses['pass'] / $headlineDenominator,
        'attempted_pass_rate' => $attemptedDenominator === 0
            ? null
            : $statuses['pass'] / $attemptedDenominator,
    ];
    file_put_contents(
        $summaryPath,
        json_encode($summary, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n",
    );
    printf(
        "total=%d pass=%d fail=%d skip=%d unsupported=%d timeout=%d crash=%d headline=%.3f%% attempted=%.3f%%\n",
        count($records),
        $statuses['pass'],
        $statuses['fail'],
        $statuses['skip'],
        $statuses['unsupported'],
        $statuses['timeout'],
        $statuses['crash'],
        100 * ($summary['headline_pass_rate'] ?? 0),
        100 * ($summary['attempted_pass_rate'] ?? 0),
    );
}

if ($argc < 2) {
    fail_usage();
}
$command = $argv[1];
[$options, $positionals] = parse_options(array_slice($argv, 2));
try {
    match ($command) {
        'run' => run_command($options, $positionals),
        'merge' => merge_command($options, $positionals),
        default => fail_usage("unknown command {$command}"),
    };
} catch (Throwable $error) {
    fwrite(STDERR, 'error: ' . $error->getMessage() . "\n");
    exit(1);
}
