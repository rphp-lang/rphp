<?php

declare(strict_types=1);

/** @return list<string> */
function unsupported_rphp_ini_directives(string $section): array
{
    $supported = [
        'assert.exception' => true,
        'error_reporting' => true,
        'zend.assertions' => true,
        'zend.exception_ignore_args' => true,
        'zend.exception_string_param_max_len' => true,
    ];
    $unsupported = [];
    $arguments = ini_arguments($section);
    for ($index = 1; $index < count($arguments); $index += 2) {
        $definition = $arguments[$index];
        $separator = strpos($definition, '=');
        $name = strtolower($separator === false
            ? $definition
            : substr($definition, 0, $separator));
        if (!isset($supported[$name])) {
            $unsupported[$name] = true;
        }
    }
    return array_keys($unsupported);
}

/** @return array<string, mixed> */
function run_test(
    string $absolutePath,
    string $relativePath,
    string $target,
    string $kind,
    float $timeout,
    array $loadedExtensions,
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
    $unsupportedIni = $kind === 'rphp'
        ? unsupported_rphp_ini_directives($sections['INI'] ?? '')
        : [];
    if ($unsupportedIni !== []) {
        $result['status'] = 'unsupported';
        $result['category'] = 'runtime-cli-ini';
        $result['reason'] = 'unsupported RPHP CLI INI directive(s): '
            . implode(', ', $unsupportedIni);
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
    // php-src expectations intentionally mention the generated basename, such
    // as `%stest.php`. Match run-tests.php's `test.phpt` -> `test.php` naming.
    // Shards partition paths, so the canonical name remains collision-free.
    $stem = basename($absolutePath, '.phpt');
    $temporaryFiles = [];
    $environment = process_environment($sections['ENV'] ?? '');
    foreach ([
        'REDIRECT_STATUS',
        'QUERY_STRING',
        'PATH_TRANSLATED',
        'SCRIPT_FILENAME',
        'REQUEST_METHOD',
        'CONTENT_TYPE',
        'CONTENT_LENGTH',
        'TZ',
    ] as $name) {
        $environment[$name] = '';
    }
    $environment['TEST_PHP_EXECUTABLE'] = $target;
    $environment['TEST_PHP_EXECUTABLE_ESCAPED'] = escapeshellarg($target);
    $ini = str_replace(
        ['{PWD}', '{TMP}'],
        [$directory, sys_get_temp_dir()],
        $sections['INI'] ?? '',
    );
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
            $skipOutput = normalized_output(normalized_runtime_output($skip['output']), false);
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
            if (isset($sections['FILEEOF'])) {
                $fileSection = preg_replace('/[\r\n]+$/', '', $fileSection) ?? $fileSection;
            }
            $testFile = $directory . DIRECTORY_SEPARATOR . $stem . '.php';
            if (file_put_contents($testFile, $fileSection) === false) {
                throw new RuntimeException('cannot create FILE section');
            }
            $temporaryFiles[] = $testFile;
        }

        $result['test_file_executed'] = true;
        $execution = run_process(
            target_command($target, $kind, $testFile, $ini, $args),
            $directory,
            $environment,
            $sections['STDIN'] ?? '',
            $timeout,
        );
        $totalDuration += $execution['duration_ms'];
        $actual = normalized_runtime_output($execution['output']);
        $result['duration_ms'] = $totalDuration;
        $result['exit_code'] = $execution['exit_code'];
        $result['actual_sha256'] = hash('sha256', normalized_output($actual, false));
        $result['actual_excerpt'] = output_excerpt($actual);
        $failureCategory = classify_failure($actual, $execution['exit_code']);
        $result['front_end_rejected'] = !$execution['timeout']
            && !$execution['crash']
            && in_array($failureCategory, ['parse', 'compile'], true);

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
            } elseif (isset($sections['XFAIL'])) {
                $result['status'] = 'xfail';
                $result['category'] = 'xfail';
                $result['reason'] = trim($sections['XFAIL']);
            } else {
                $result['status'] = 'fail';
                $result['category'] = $failureCategory;
                $result['reason'] = 'actual output does not match ' . $expectation;
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
            $cleanOutput = normalized_output(normalized_runtime_output($clean['output']), false);
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
        $result = run_test($test, $relative, $target, $kind, $timeout, $loaded);
        fwrite($handle, json_encode($result, JSON_UNESCAPED_SLASHES) . "\n");
        $completed++;
        if ($completed % 100 === 0) {
            fwrite(STDERR, "shard {$shardIndex}: {$completed} cases\n");
        }
    }
    fclose($handle);
    fwrite(STDERR, "shard {$shardIndex}: completed {$completed} cases\n");
}
