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
        "total=%d pass=%d fail=%d skip=%d xfail=%d unsupported=%d timeout=%d crash=%d headline=%.3f%% attempted=%.3f%% runtime-reach=%.3f%%\n",
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
        100 * ($summary['execution_profile']['runtime_reach_rate'] ?? 0),
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
    $expectationProfiles = [];
    $executionCounts = ['front_end_rejected' => 0, 'pre_execution_failed' => 0];
    foreach ($records as $record) {
        $statuses[$record['status']] = ($statuses[$record['status']] ?? 0) + 1;
        $categories[$record['category']] = ($categories[$record['category']] ?? 0) + 1;
        $profile = $record['expectation_profile'] ?? 'ordinary';
        if (!isset($expectationProfiles[$profile])) {
            $expectationProfiles[$profile] = array_fill_keys(
                ['pass', 'fail', 'skip', 'xfail', 'unsupported', 'timeout', 'crash'],
                0,
            );
        }
        $expectationProfiles[$profile][$record['status']]++;
        $suite = str_starts_with($record['path'], 'Zend/tests/') ? 'Zend/tests' : 'tests/lang';
        if (!isset($suites[$suite])) {
            $suites[$suite] = array_fill_keys(
                ['pass', 'fail', 'skip', 'xfail', 'unsupported', 'timeout', 'crash'],
                0,
            );
            $suites[$suite]['categories'] = [];
            $suites[$suite]['_execution'] = [
                'front_end_rejected' => 0,
                'pre_execution_failed' => 0,
            ];
        }
        $suites[$suite][$record['status']]++;
        $suites[$suite]['categories'][$record['category']] =
            ($suites[$suite]['categories'][$record['category']] ?? 0) + 1;
        if (in_array($record['status'], ['pass', 'fail', 'timeout', 'crash'], true)) {
            if (!($record['test_file_executed'] ?? false)) {
                $executionCounts['pre_execution_failed']++;
                $suites[$suite]['_execution']['pre_execution_failed']++;
            } elseif ($record['front_end_rejected'] ?? false) {
                $executionCounts['front_end_rejected']++;
                $suites[$suite]['_execution']['front_end_rejected']++;
            }
        }
    }
    ksort($categories);
    ksort($suites);
    foreach ($suites as &$suiteStatuses) {
        ksort($suiteStatuses['categories']);
        $suiteHeadline = $suiteStatuses['pass'] + $suiteStatuses['fail'];
        $suiteAttempted = $suiteHeadline + $suiteStatuses['timeout'] + $suiteStatuses['crash'];
        $suiteStatuses['total'] = $suiteAttempted
            + $suiteStatuses['skip']
            + $suiteStatuses['xfail']
            + $suiteStatuses['unsupported'];
        $suiteStatuses['headline_pass_rate'] = $suiteHeadline === 0
            ? null
            : $suiteStatuses['pass'] / $suiteHeadline;
        $suiteStatuses['attempted_pass_rate'] = $suiteAttempted === 0
            ? null
            : $suiteStatuses['pass'] / $suiteAttempted;
        $suiteStatuses['execution_profile'] = execution_profile(
            $suiteStatuses,
            $suiteStatuses['_execution']['front_end_rejected'],
            $suiteStatuses['_execution']['pre_execution_failed'],
        );
        unset($suiteStatuses['_execution']);
    }
    unset($suiteStatuses);
    ksort($expectationProfiles);
    foreach ($expectationProfiles as &$profileStatuses) {
        $profileStatuses = status_profile($profileStatuses);
    }
    unset($profileStatuses);

    $headlineDenominator = $statuses['pass'] + $statuses['fail'];
    $attemptedDenominator = $headlineDenominator + $statuses['timeout'] + $statuses['crash'];
    return [
        'schema_version' => 5,
        'rphp_commit' => $options['rphp-commit'] ?? '',
        'runner_commit' => $options['runner-commit'] ?? '',
        'php_src_commit' => $options['php-src-commit'] ?? '',
        'features' => $options['features'] ?? '',
        'architecture' => $options['architecture'] ?? php_uname('m'),
        'target' => $options['target-label'] ?? 'rphp',
        'timeout_seconds' => (float) ($options['timeout'] ?? 3),
        'total' => count($records),
        'statuses' => $statuses,
        'categories' => $categories,
        'expectation_profiles' => $expectationProfiles,
        'suites' => $suites,
        'headline_pass_rate' => $headlineDenominator === 0
            ? null
            : $statuses['pass'] / $headlineDenominator,
        'attempted_pass_rate' => $attemptedDenominator === 0
            ? null
            : $statuses['pass'] / $attemptedDenominator,
        'execution_profile' => execution_profile(
            $statuses,
            $executionCounts['front_end_rejected'],
            $executionCounts['pre_execution_failed'],
        ),
    ];
}

/** @return array<string, mixed> */
function status_profile(array $statuses): array
{
    $headlineDenominator = $statuses['pass'] + $statuses['fail'];
    $attemptedDenominator = $headlineDenominator + $statuses['timeout'] + $statuses['crash'];
    return [
        ...$statuses,
        'total' => array_sum($statuses),
        'headline_pass_rate' => $headlineDenominator === 0
            ? null
            : $statuses['pass'] / $headlineDenominator,
        'attempted_pass_rate' => $attemptedDenominator === 0
            ? null
            : $statuses['pass'] / $attemptedDenominator,
    ];
}

/**
 * Execution reach is descriptive, not a compatibility score. A PHPT may
 * intentionally expect a front-end rejection, while a runtime-reaching case
 * may still behave incorrectly.
 *
 * @return array{attempted: int, pre_execution_failed: int, front_end_rejected: int, runtime_reached: int, runtime_reach_rate: ?float}
 */
function execution_profile(
    array $statuses,
    int $frontEndRejected,
    int $preExecutionFailed,
): array
{
    $attempted = $statuses['pass']
        + $statuses['fail']
        + $statuses['timeout']
        + $statuses['crash'];
    $runtimeReached = max(0, $attempted - $frontEndRejected - $preExecutionFailed);
    return [
        'attempted' => $attempted,
        'pre_execution_failed' => $preExecutionFailed,
        'front_end_rejected' => $frontEndRejected,
        'runtime_reached' => $runtimeReached,
        'runtime_reach_rate' => $attempted === 0 ? null : $runtimeReached / $attempted,
    ];
}
