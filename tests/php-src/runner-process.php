<?php

declare(strict_types=1);

require __DIR__ . '/../../scripts/phpt/process.php';

function check(bool $condition, string $message): void
{
    if (!$condition) {
        throw new RuntimeException($message);
    }
}

$environment = is_array(getenv()) ? getenv() : [];
$cwd = __DIR__;
$ordinary = run_process([PHP_BINARY, '-r', 'echo "out"; fwrite(STDERR, "err"); exit(7);'], $cwd, $environment, '', 2);
check($ordinary['output'] === 'outerr' && $ordinary['exit_code'] === 7, 'ordinary output and status changed');
check(!$ordinary['timeout'] && !$ordinary['crash'], 'ordinary process was misclassified');

$timeout = run_process([PHP_BINARY, '-r', 'usleep(2000000);'], $cwd, $environment, '', 0.1);
check($timeout['timeout'] && $timeout['duration_ms'] < 1500, 'direct target deadline was not enforced');

if (PHP_OS_FAMILY === 'Linux') {
    // The grandchild ignores TERM and keeps the output pipe open. Its delayed
    // marker must never appear, including when the parent exits on its own.
    $marker = tempnam(sys_get_temp_dir(), 'rphp-runner-child-');
    check($marker !== false, 'cannot create descendant test marker');
    try {
        $child = <<<'PHP'
pcntl_async_signals(true);
pcntl_signal(SIGTERM, SIG_IGN);
echo "ready\n";
usleep(800000);
file_put_contents($argv[1], "survived");
PHP;
        $parent = <<<'PHP'
$child = proc_open([PHP_BINARY, '-r', $argv[1], $argv[2]], [0 => ['pipe', 'r'], 1 => STDOUT, 2 => STDERR], $pipes);
fclose($pipes[0]);
proc_close($child);
PHP;
        $result = run_process([PHP_BINARY, '-r', $parent, $child, $marker], $cwd, $environment, '', 0.2);
        check($result['timeout'] && $result['duration_ms'] < 700, 'descendant retained the runner after timeout');
        check($result['output'] === "ready\n", 'descendant did not initialize before timeout');
        usleep(850000);
        check(file_get_contents($marker) === '', 'grandchild survived process-group termination');

        $orphan = <<<'PHP'
$pid = pcntl_fork();
if ($pid === 0) {
    usleep(800000);
    file_put_contents($argv[1], "survived");
    exit;
}
echo "parent done\n";
PHP;
        $result = run_process([PHP_BINARY, '-r', $orphan, $marker], $cwd, $environment, '', 2);
        check(!$result['timeout'] && $result['exit_code'] === 0 && $result['duration_ms'] < 700, 'exited parent retained a descendant pipe');
        check($result['output'] === "parent done\n", 'parent output changed during descendant cleanup');
        usleep(850000);
        check(file_get_contents($marker) === '', 'orphan survived normal parent cleanup');

        $previousLimit = getenv('RPHP_PHPT_MAX_MEMORY_MB');
        putenv('RPHP_PHPT_MAX_MEMORY_MB=128');
        try {
            $limits = run_process([PHP_BINARY, '-r', 'echo posix_getrlimit()["soft totalmem"];'], $cwd, $environment, '', 2);
            check($limits['exit_code'] === 0 && $limits['output'] === '134217728', 'target address-space cap missing');
            $allocation = run_process([PHP_BINARY, '-d', 'memory_limit=-1', '-r', '$s = str_repeat("x", 268435456); echo strlen($s);'], $cwd, $environment, '', 2);
            check(!$allocation['timeout'] && $allocation['exit_code'] !== 0, 'target allocation escaped address-space cap');
        } finally {
            putenv($previousLimit === false ? 'RPHP_PHPT_MAX_MEMORY_MB' : 'RPHP_PHPT_MAX_MEMORY_MB=' . $previousLimit);
        }
    } finally {
        unlink($marker);
    }
}

echo "PHPT process isolation tests passed\n";
