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

require __DIR__ . '/phpt/case.php';
require __DIR__ . '/phpt/process.php';
require __DIR__ . '/phpt/expectation.php';
require __DIR__ . '/phpt/execution.php';
require __DIR__ . '/phpt/report.php';

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
Known upstream XFAIL outcomes are reported separately from compatibility fails.
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
