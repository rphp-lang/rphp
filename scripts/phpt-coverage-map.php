#!/usr/bin/env php
<?php

declare(strict_types=1);

const STATUSES = ['pass', 'fail', 'skip', 'xfail', 'unsupported', 'timeout', 'crash'];

if ($argc < 2 || $argc > 3) {
    fwrite(STDERR, "usage: phpt-coverage-map.php MANIFEST [OUTPUT]\n");
    exit(2);
}

$manifest = $argv[1];
$handle = fopen($manifest, 'rb');
if ($handle === false) {
    fwrite(STDERR, "cannot read manifest: {$manifest}\n");
    exit(1);
}

$groups = [];
$hazards = [];
$total = 0;
while (($line = fgets($handle)) !== false) {
    $record = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
    $path = $record['path'] ?? null;
    $status = $record['status'] ?? null;
    $category = $record['category'] ?? null;
    if (!is_string($path) || !is_string($status) || !is_string($category)) {
        throw new RuntimeException('manifest record is missing path, status or category');
    }
    if (!in_array($status, STATUSES, true)) {
        throw new RuntimeException("unknown manifest status {$status} for {$path}");
    }

    $parts = explode('/', $path);
    if (($parts[0] ?? '') === 'Zend' && ($parts[1] ?? '') === 'tests') {
        $prefix = 'Zend/tests';
        $leaf = count($parts) > 3 ? $parts[2] : '_root';
    } elseif (($parts[0] ?? '') === 'tests' && ($parts[1] ?? '') === 'lang') {
        $prefix = 'tests/lang';
        $leaf = count($parts) > 3 ? $parts[2] : '_root';
    } else {
        $prefix = $parts[0] !== '' ? $parts[0] : '_root';
        $leaf = count($parts) > 1 ? $parts[1] : '_root';
    }
    $groupName = $prefix . '/' . $leaf;
    if (!isset($groups[$groupName])) {
        $groups[$groupName] = [
            'total' => 0,
            'statuses' => array_fill_keys(STATUSES, 0),
            'categories' => [],
        ];
    }
    $groups[$groupName]['total']++;
    $groups[$groupName]['statuses'][$status]++;
    $groups[$groupName]['categories'][$category] =
        ($groups[$groupName]['categories'][$category] ?? 0) + 1;
    $total++;

    if ($status === 'crash' || $status === 'timeout') {
        $hazards[] = compact('path', 'status', 'category');
    }
}
fclose($handle);

ksort($groups);
foreach ($groups as &$group) {
    ksort($group['categories']);
    $denominator = $group['statuses']['pass'] + $group['statuses']['fail'];
    $group['headline_pass_rate'] = $denominator === 0
        ? null
        : $group['statuses']['pass'] / $denominator;
}
unset($group);
usort($hazards, static fn(array $left, array $right): int => $left['path'] <=> $right['path']);

$result = [
    'schema_version' => 1,
    'manifest_sha256' => hash_file('sha256', $manifest),
    'total' => $total,
    'groups' => $groups,
    'hazards' => $hazards,
];
$encoded = json_encode($result, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n";
if ($argc === 3) {
    if (file_put_contents($argv[2], $encoded) === false) {
        fwrite(STDERR, "cannot write coverage map: {$argv[2]}\n");
        exit(1);
    }
} else {
    echo $encoded;
}
