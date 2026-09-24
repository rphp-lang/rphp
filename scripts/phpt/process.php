<?php

declare(strict_types=1);

/** @return array{list<string>, bool} */
function isolated_target_command(array $command): array
{
    if (PHP_OS_FAMILY !== 'Linux') {
        return [$command, false];
    }
    if (!function_exists('posix_kill')) {
        throw new RuntimeException('Linux PHPT isolation requires the PHP POSIX extension');
    }
    $tools = [];
    foreach (['setsid', 'prlimit'] as $name) {
        foreach (explode(PATH_SEPARATOR, getenv('PATH') ?: '/usr/bin:/bin') as $directory) {
            $path = $directory . DIRECTORY_SEPARATOR . $name;
            if (is_file($path) && is_executable($path)) {
                $tools[$name] = $path;
                break;
            }
        }
        if (!isset($tools[$name])) {
            throw new RuntimeException("Linux PHPT isolation requires {$name}");
        }
    }
    $memoryMiB = getenv('RPHP_PHPT_MAX_MEMORY_MB');
    $memoryMiB = $memoryMiB === false ? '2048' : $memoryMiB;
    if (!ctype_digit($memoryMiB) || (int) $memoryMiB < 16 || (int) $memoryMiB > 32768) {
        throw new RuntimeException('RPHP_PHPT_MAX_MEMORY_MB must be between 16 and 32768');
    }
    // The limit is inherited by shell_exec()/PHP_BINARY children as well.
    // PHP's own memory_limit cannot protect the host from runtime bugs.
    return [[
        $tools['setsid'],
        $tools['prlimit'],
        '--as=' . ((int) $memoryMiB * 1024 * 1024),
        '--core=0',
        '--',
        ...$command,
    ], true];
}

function terminate_target_tree($process, ?int $group, int $signal): void
{
    if ($group !== null) {
        // setsid makes the proc_open child the leader of this private group.
        // Signal it even after the leader exits: descendants may retain pipes.
        @posix_kill(-$group, $signal);
    }
    if (proc_get_status($process)['running']) {
        proc_terminate($process, $signal);
    }
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
    [$command, $isolated] = isolated_target_command($command);
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
    $group = $isolated ? $lastStatus['pid'] : null;
    while ($lastStatus['running']) {
        // Bound each drain so a continuously writing target cannot postpone
        // the wall-clock deadline indefinitely.
        $chunk = stream_get_contents($pipes[1], 65536);
        if ($chunk !== false) {
            $output .= $chunk;
        }
        if ((hrtime(true) - $start) / 1_000_000_000 > $timeout) {
            $timedOut = true;
            terminate_target_tree($process, $group, 15);
            usleep(20_000);
            // Escalate for the entire group even if its leader already died.
            terminate_target_tree($process, $group, 9);
            break;
        }
        usleep(5_000);
        $lastStatus = proc_get_status($process);
    }
    if ($group !== null) {
        terminate_target_tree($process, $group, 9);
    }
    // Never block waiting for EOF from an inherited descriptor after timeout
    // or normal parent exit. All available bytes are still collected.
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
        return [$target, ...ini_arguments($ini), $file, ...script_arguments($args)];
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
    $result = run_process(
        [$target, '-r', 'echo implode("\\n", get_loaded_extensions());'],
        getcwd() ?: '.',
        is_array(getenv()) ? getenv() : [],
        '',
        $timeout,
    );
    if ($kind === 'rphp'
        && !$result['timeout']
        && $result['exit_code'] !== 0
        && str_contains($result['output'], 'Call to undefined function get_loaded_extensions()')
    ) {
        // Immutable pre-discovery baselines do not expose
        // get_loaded_extensions(). Keep their historical empty extension set
        // so parent/candidate comparisons remain runnable.
        return [];
    }
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
