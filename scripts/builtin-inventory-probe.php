#!/usr/bin/env php
<?php

declare(strict_types=1);

/*
 * Runtime-neutral builtin inventory probe.
 *
 * The script intentionally stays inside the PHP subset already exercised by
 * RPHP. Rich comparison and report generation live in audit-php-builtins.py.
 * Optional method probes are supplied one Class::method name per line through
 * RPHP_BUILTIN_AUDIT_METHODS_FILE because RPHP cannot enumerate every internal
 * method through ReflectionClass::getMethods() yet.
 */

function audit_safe_call($object, string $method, $fallback = null)
{
    try {
        return $object->{$method}();
    } catch (Throwable $error) {
        return $fallback;
    }
}

function audit_type_name($type): ?string
{
    if ($type === null) {
        return null;
    }

    try {
        return (string) $type;
    } catch (Throwable $error) {
        return null;
    }
}

function audit_default_value(ReflectionParameter $parameter): array
{
    $available = (bool) audit_safe_call($parameter, 'isDefaultValueAvailable', false);
    if (!$available) {
        return [
            'available' => false,
            'constant' => null,
            'value_type' => null,
            'value_export' => null,
        ];
    }

    $constant = audit_safe_call($parameter, 'getDefaultValueConstantName');
    try {
        $value = $parameter->getDefaultValue();

        return [
            'available' => true,
            'constant' => is_string($constant) ? $constant : null,
            'value_type' => gettype($value),
            'value_export' => var_export($value, true),
        ];
    } catch (Throwable $error) {
        return [
            'available' => true,
            'constant' => is_string($constant) ? $constant : null,
            'value_type' => 'unavailable',
            'value_export' => null,
        ];
    }
}

function audit_parameter_entry(ReflectionParameter $parameter, int $position, int $required): array
{
    $type = audit_type_name(audit_safe_call($parameter, 'getType'));

    return [
        'position' => $position,
        'name' => $parameter->getName(),
        'required' => $position < $required,
        'optional' => (bool) audit_safe_call($parameter, 'isOptional', $position >= $required),
        'variadic' => (bool) audit_safe_call($parameter, 'isVariadic', false),
        'by_reference' => (bool) audit_safe_call($parameter, 'isPassedByReference', false),
        'type' => $type,
        'allows_null' => audit_safe_call($parameter, 'allowsNull'),
        'default' => audit_default_value($parameter),
    ];
}

function audit_callable_parameters($reflection, int $required): array
{
    $entries = [];
    try {
        $parameters = $reflection->getParameters();
    } catch (Throwable $error) {
        return $entries;
    }

    foreach ($parameters as $position => $parameter) {
        $entries[] = audit_parameter_entry($parameter, $position, $required);
    }

    return $entries;
}

function audit_function_entry(ReflectionFunction $reflection): array
{
    $required = $reflection->getNumberOfRequiredParameters();
    $parameters = audit_callable_parameters($reflection, $required);
    $lastParameter = count($parameters) > 0 ? $parameters[count($parameters) - 1] : null;
    $derivedVariadic = is_array($lastParameter) && (bool) $lastParameter['variadic'];
    $reflectedVariadic = audit_safe_call($reflection, 'isVariadic');

    return [
        'name' => $reflection->getName(),
        'extension' => audit_safe_call($reflection, 'getExtensionName'),
        'required_parameters' => $required,
        'total_parameters' => $reflection->getNumberOfParameters(),
        // Older RPHP ReflectionFunction builds do not expose isVariadic(), but
        // ReflectionParameter still carries the actual variadic flag.
        'variadic' => is_bool($reflectedVariadic) ? $reflectedVariadic : $derivedVariadic,
        'returns_reference' => (bool) audit_safe_call($reflection, 'returnsReference', false),
        'deprecated' => (bool) audit_safe_call($reflection, 'isDeprecated', false),
        'return_type' => audit_type_name(audit_safe_call($reflection, 'getReturnType')),
        'parameters' => $parameters,
    ];
}

function audit_method_entry(ReflectionMethod $reflection, ?string $requestedClass = null): array
{
    $required = $reflection->getNumberOfRequiredParameters();
    $parameters = audit_callable_parameters($reflection, $required);
    $lastParameter = count($parameters) > 0 ? $parameters[count($parameters) - 1] : null;
    $derivedVariadic = is_array($lastParameter) && (bool) $lastParameter['variadic'];
    $reflectedVariadic = audit_safe_call($reflection, 'isVariadic');
    $declaring = audit_safe_call($reflection, 'getDeclaringClass');
    $declaringName = is_object($declaring)
        ? audit_safe_call($declaring, 'getName')
        : null;
    $class = $requestedClass ?? (is_string($declaringName) ? $declaringName : '');

    return [
        'class' => $class,
        'declaring_class' => is_string($declaringName) ? $declaringName : null,
        'name' => $reflection->getName(),
        'full_name' => $class . '::' . $reflection->getName(),
        'required_parameters' => $required,
        'total_parameters' => $reflection->getNumberOfParameters(),
        'variadic' => is_bool($reflectedVariadic) ? $reflectedVariadic : $derivedVariadic,
        'returns_reference' => (bool) audit_safe_call($reflection, 'returnsReference', false),
        'deprecated' => (bool) audit_safe_call($reflection, 'isDeprecated', false),
        'static' => (bool) audit_safe_call($reflection, 'isStatic', false),
        'public' => (bool) audit_safe_call($reflection, 'isPublic', true),
        'protected' => (bool) audit_safe_call($reflection, 'isProtected', false),
        'private' => (bool) audit_safe_call($reflection, 'isPrivate', false),
        'abstract' => (bool) audit_safe_call($reflection, 'isAbstract', false),
        'final' => (bool) audit_safe_call($reflection, 'isFinal', false),
        'return_type' => audit_type_name(audit_safe_call($reflection, 'getReturnType')),
        'parameters' => $parameters,
    ];
}

function audit_type_entry(string $name, string $kind): array
{
    try {
        $reflection = new ReflectionClass($name);
    } catch (Throwable $error) {
        return [
            'name' => $name,
            'kind' => $kind,
            'reflection_error' => get_class($error) . ': ' . $error->getMessage(),
        ];
    }

    $parent = audit_safe_call($reflection, 'getParentClass', false);
    $parentName = is_object($parent) ? audit_safe_call($parent, 'getName') : null;
    $interfaces = audit_safe_call($reflection, 'getInterfaceNames', []);
    if (!is_array($interfaces)) {
        $interfaces = [];
    }
    sort($interfaces, SORT_STRING);

    return [
        'name' => $name,
        'kind' => $kind,
        'extension' => audit_safe_call($reflection, 'getExtensionName'),
        'internal' => (bool) audit_safe_call($reflection, 'isInternal', true),
        'abstract' => (bool) audit_safe_call($reflection, 'isAbstract', false),
        'final' => (bool) audit_safe_call($reflection, 'isFinal', false),
        'readonly' => (bool) audit_safe_call($reflection, 'isReadOnly', false),
        'enum' => (bool) audit_safe_call($reflection, 'isEnum', false),
        'parent' => is_string($parentName) ? $parentName : null,
        'interfaces' => $interfaces,
    ];
}

function audit_declared_methods(array $typeNames): array
{
    $entries = [];
    foreach ($typeNames as $class) {
        try {
            $reflection = new ReflectionClass($class);
            $methods = $reflection->getMethods();
        } catch (Throwable $error) {
            continue;
        }
        foreach ($methods as $method) {
            $declaring = audit_safe_call($method, 'getDeclaringClass');
            $declaringName = is_object($declaring)
                ? audit_safe_call($declaring, 'getName')
                : null;
            if (is_string($declaringName) && strcasecmp($declaringName, $class) !== 0) {
                continue;
            }
            $entry = audit_method_entry($method, $class);
            try {
                new ReflectionMethod($class, $method->getName());
                $entry['probeable_by_name'] = true;
            } catch (Throwable $error) {
                // Native PHP itself exposes a small number of synthetic
                // methods through getMethods() that cannot be reconstructed
                // through ReflectionMethod(Class::method).
                $entry['probeable_by_name'] = false;
            }
            $entries[] = $entry;
        }
    }

    usort($entries, static function (array $left, array $right): int {
        return strcasecmp($left['full_name'], $right['full_name']);
    });

    return $entries;
}

function audit_method_probes(?string $path): array
{
    if ($path === null || $path === '' || !is_file($path)) {
        return ['present' => [], 'missing' => []];
    }

    $contents = file_get_contents($path);
    if (!is_string($contents)) {
        return ['present' => [], 'missing' => []];
    }

    $names = explode("\n", $contents);
    $present = [];
    $missing = [];
    foreach ($names as $fullName) {
        $fullName = trim($fullName);
        if ($fullName === '') {
            continue;
        }
        $parts = explode('::', $fullName, 2);
        if (count($parts) !== 2) {
            continue;
        }
        try {
            $reflection = new ReflectionMethod($parts[0], $parts[1]);
            $present[] = audit_method_entry($reflection, $parts[0]);
        } catch (Throwable $error) {
            $missing[] = [
                'full_name' => $fullName,
                'error_class' => get_class($error),
                'error_message' => $error->getMessage(),
            ];
        }
    }

    usort($present, static function (array $left, array $right): int {
        return strcasecmp($left['full_name'], $right['full_name']);
    });
    usort($missing, static function (array $left, array $right): int {
        return strcasecmp($left['full_name'], $right['full_name']);
    });

    return ['present' => $present, 'missing' => $missing];
}

function audit_previous_significant_token(array $tokens, int $index)
{
    for ($cursor = $index - 1; $cursor >= 0; --$cursor) {
        $token = $tokens[$cursor];
        if (
            is_array($token)
            && in_array($token[0], [T_WHITESPACE, T_COMMENT, T_DOC_COMMENT], true)
        ) {
            continue;
        }

        return $token;
    }

    return null;
}

function audit_next_significant_token(array $tokens, int $index)
{
    $count = count($tokens);
    for ($cursor = $index + 1; $cursor < $count; ++$cursor) {
        $token = $tokens[$cursor];
        if (
            is_array($token)
            && in_array($token[0], [T_WHITESPACE, T_COMMENT, T_DOC_COMMENT], true)
        ) {
            continue;
        }

        return $token;
    }

    return null;
}

function audit_vendor_call_name(string $raw): ?string
{
    if ($raw === '') {
        return null;
    }
    if ($raw[0] === '\\') {
        $name = substr($raw, 1);

        return strpos($name, '\\') === false ? $name : null;
    }
    if (strpos($raw, '\\') !== false) {
        return null;
    }

    return $raw;
}

function audit_scan_vendor_paths(?string $encodedPaths): array
{
    if ($encodedPaths === null || $encodedPaths === '' || !function_exists('token_get_all')) {
        return ['files_scanned' => 0, 'calls' => []];
    }

    $nameTokenIds = [T_STRING];
    foreach (['T_NAME_FULLY_QUALIFIED', 'T_NAME_QUALIFIED', 'T_NAME_RELATIVE'] as $constant) {
        if (defined($constant)) {
            $nameTokenIds[] = constant($constant);
        }
    }
    $blockedPrevious = [T_FUNCTION, T_NEW, T_OBJECT_OPERATOR, T_DOUBLE_COLON];
    foreach (['T_FN', 'T_NULLSAFE_OBJECT_OPERATOR'] as $constant) {
        if (defined($constant)) {
            $blockedPrevious[] = constant($constant);
        }
    }

    $files = [];
    $roots = explode(PATH_SEPARATOR, $encodedPaths);
    foreach ($roots as $rootIndex => $root) {
        if ($root === '' || !is_dir($root)) {
            continue;
        }
        $prefix = rtrim(str_replace('\\', '/', $root), '/') . '/';
        $iterator = new RecursiveIteratorIterator(
            new RecursiveDirectoryIterator($root, FilesystemIterator::SKIP_DOTS),
        );
        foreach ($iterator as $file) {
            if (!$file->isFile() || strtolower($file->getExtension()) !== 'php') {
                continue;
            }
            $absolute = str_replace('\\', '/', $file->getPathname());
            $relative = str_starts_with($absolute, $prefix)
                ? substr($absolute, strlen($prefix))
                : $file->getFilename();
            $files[] = [
                'absolute' => $file->getPathname(),
                'display' => 'vendor' . ($rootIndex + 1) . '/' . $relative,
            ];
        }
    }
    usort(
        $files,
        static fn(array $left, array $right): int => $left['display'] <=> $right['display']
    );

    $calls = [];
    foreach ($files as $file) {
        $source = file_get_contents($file['absolute']);
        if (!is_string($source)) {
            continue;
        }
        $tokens = token_get_all($source);
        foreach ($tokens as $index => $token) {
            if (!is_array($token) || !in_array($token[0], $nameTokenIds, true)) {
                continue;
            }
            $next = audit_next_significant_token($tokens, $index);
            if ($next !== '(') {
                continue;
            }
            $previous = audit_previous_significant_token($tokens, $index);
            if (is_array($previous) && in_array($previous[0], $blockedPrevious, true)) {
                continue;
            }
            $resolved = audit_vendor_call_name($token[1]);
            if ($resolved === null) {
                continue;
            }
            $key = strtolower($resolved);
            if (!isset($calls[$key])) {
                $calls[$key] = [
                    'name' => $resolved,
                    'occurrences' => 0,
                    'files' => [],
                ];
            }
            ++$calls[$key]['occurrences'];
            $calls[$key]['files'][$file['display']] = true;
        }
    }

    ksort($calls, SORT_STRING);
    $entries = [];
    foreach ($calls as $call) {
        $call['files'] = array_keys($call['files']);
        sort($call['files'], SORT_STRING);
        $entries[] = $call;
    }

    return ['files_scanned' => count($files), 'calls' => $entries];
}

$functionNames = get_defined_functions()['internal'];
sort($functionNames, SORT_STRING);
$functions = [];
$functionErrors = [];
foreach ($functionNames as $name) {
    try {
        $functions[] = audit_function_entry(new ReflectionFunction($name));
    } catch (Throwable $error) {
        $functionErrors[] = [
            'name' => $name,
            'error_class' => get_class($error),
            'error_message' => $error->getMessage(),
        ];
    }
}

$typeGroups = [
    'class' => get_declared_classes(),
    'interface' => get_declared_interfaces(),
    'trait' => get_declared_traits(),
];
$types = [];
$allTypeNames = [];
foreach ($typeGroups as $kind => $names) {
    sort($names, SORT_STRING);
    foreach ($names as $name) {
        $types[] = audit_type_entry($name, $kind);
        $allTypeNames[] = $name;
    }
}
usort($types, static function (array $left, array $right): int {
    return strcasecmp($left['name'], $right['name']);
});
sort($allTypeNames, SORT_STRING);

$methodProbesPath = getenv('RPHP_BUILTIN_AUDIT_METHODS_FILE');
$methodProbes = audit_method_probes(is_string($methodProbesPath) ? $methodProbesPath : null);
$vendorPaths = getenv('RPHP_BUILTIN_AUDIT_VENDOR_PATHS');

$result = [
    'schema_version' => 1,
    'php_version' => PHP_VERSION,
    'php_version_id' => PHP_VERSION_ID,
    'php_sapi' => PHP_SAPI,
    'functions' => $functions,
    'function_errors' => $functionErrors,
    'types' => $types,
    'declared_methods' => audit_declared_methods($allTypeNames),
    'probed_methods' => $methodProbes['present'],
    'missing_probed_methods' => $methodProbes['missing'],
    'vendor_scan' => audit_scan_vendor_paths(is_string($vendorPaths) ? $vendorPaths : null),
];

$encoded = json_encode($result);
if (!is_string($encoded)) {
    fwrite(STDERR, "builtin inventory JSON encoding failed\n");
    exit(1);
}
echo $encoded, "\n";
