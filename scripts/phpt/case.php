<?php

declare(strict_types=1);

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

/** @return list<string> */
function required_extensions(string $section): array
{
    $extensions = preg_split('/\s+/', trim($section)) ?: [];
    return array_values(array_filter(array_map('strtolower', $extensions)));
}
