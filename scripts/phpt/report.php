<?php

declare(strict_types=1);

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
            $records[] = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
        }
        fclose($handle);
    }
    usort($records, static fn(array $left, array $right): int => $left['path'] <=> $right['path']);

    write_published_manifest($outputPath, $records);
    $summary = summarize_results($options, $records);
    file_put_contents(
        $summaryPath,
        json_encode($summary, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n",
    );
    printf(
        "total=%d pass=%d fail=%d skip=%d xfail=%d unsupported=%d timeout=%d crash=%d headline=%.3f%% attempted=%.3f%%\n",
        $summary['total'],
        $summary['statuses']['pass'],
        $summary['statuses']['fail'],
        $summary['statuses']['skip'],
        $summary['statuses']['xfail'],
        $summary['statuses']['unsupported'],
        $summary['statuses']['timeout'],
        $summary['statuses']['crash'],
        100 * ($summary['headline_pass_rate'] ?? 0),
        100 * ($summary['attempted_pass_rate'] ?? 0),
    );
}

function write_published_manifest(string $path, array $records): void
{
    $handle = fopen($path, 'wb');
    if ($handle === false) {
        throw new RuntimeException("cannot create merged manifest {$path}");
    }
    foreach ($records as $record) {
        $published = [
            'path' => $record['path'],
            'status' => $record['status'],
            'category' => $record['category'],
        ];
        if (in_array($record['status'], ['skip', 'xfail', 'unsupported', 'timeout', 'crash'], true)) {
            $published['reason'] = $record['reason'];
        }
        fwrite($handle, json_encode($published, JSON_UNESCAPED_SLASHES) . "\n");
    }
    fclose($handle);
}

/** @return array<string, mixed> */
function summarize_results(array $options, array $records): array
{
    $statuses = array_fill_keys(
        ['pass', 'fail', 'skip', 'xfail', 'unsupported', 'timeout', 'crash'],
        0,
    );
    $categories = [];
    $suites = [];
    foreach ($records as $record) {
        $statuses[$record['status']] = ($statuses[$record['status']] ?? 0) + 1;
        $categories[$record['category']] = ($categories[$record['category']] ?? 0) + 1;
        $suite = str_starts_with($record['path'], 'Zend/tests/') ? 'Zend/tests' : 'tests/lang';
        if (!isset($suites[$suite])) {
            $suites[$suite] = array_fill_keys(
                ['pass', 'fail', 'skip', 'xfail', 'unsupported', 'timeout', 'crash'],
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
    return [
        'schema_version' => 2,
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
}
