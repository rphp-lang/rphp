<?php

declare(strict_types=1);

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
        '-n',
        '-d',
        'output_handler=',
        '-d',
        'open_basedir=',
        '-d',
        'disable_functions=',
        '-d',
        'output_buffering=Off',
        '-d',
        'display_errors=1',
        '-d',
        'display_startup_errors=1',
        '-d',
        'html_errors=0',
        '-d',
        'log_errors=0',
        '-d',
        'error_reporting=E_ALL',
        '-d',
        'docref_root=',
        '-d',
        'docref_ext=.html',
        '-d',
        'error_prepend_string=',
        '-d',
        'error_append_string=',
        '-d',
        'auto_prepend_file=',
        '-d',
        'auto_append_file=',
        '-d',
        'ignore_repeated_errors=0',
        '-d',
        'precision=14',
        '-d',
        'serialize_precision=-1',
        '-d',
        'memory_limit=128M',
        '-d',
        'zend.assertions=1',
        '-d',
        'zend.exception_ignore_args=0',
        '-d',
        'zend.exception_string_param_max_len=15',
        '-d',
        'short_open_tag=0',
        '-d',
        'date.timezone=UTC',
        // PHP 8.5 enables compile-time fatal backtraces by default. The
        // pinned PHP 8.4 suite predates that output change, so keep reference
        // runs on the suite's diagnostic profile as run-tests.php would.
        '-d',
        'fatal_error_backtraces=0',
        ...ini_arguments($ini),
        '-f',
        $file,
        ...script_arguments($args),
    ];
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
