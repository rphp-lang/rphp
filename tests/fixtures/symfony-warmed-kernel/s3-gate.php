<?php

require __DIR__.'/vendor/autoload.php';

use Rphp\SymfonyKernelFixture\Kernel;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpKernel\Exception\HttpExceptionInterface;
use Symfony\Component\HttpKernel\HttpKernelInterface;

function request_result(string $path): string
{
    $kernel = new Kernel('prod', false);
    $request = Request::create($path, 'GET');

    try {
        $response = $kernel->handle($request, HttpKernelInterface::MAIN_REQUEST, false);
        $result = $response->getStatusCode().'|'.($response->headers->get('X-RPHP-Fixture') ?? '-').'|'.$response->getContent();
        $kernel->terminate($request, $response);

        return $result;
    } catch (Throwable $error) {
        return get_class($error).'|'.($error instanceof HttpExceptionInterface ? $error->getStatusCode() : 0).'|'.$error->getMessage();
    } finally {
        $kernel->shutdown();
    }
}

function normalize_generated_name(string $value): string
{
    $value = preg_replace('/Container[A-Za-z0-9]+/', 'Container@', $value);
    $value = preg_replace('/Ghost[A-Fa-f0-9]+/', 'Ghost@', $value);

    return preg_replace('/_ServiceLocator_[A-Za-z0-9]+/', '_ServiceLocator_@', $value);
}

function normalize_resource(string $resource): string
{
    $resource = str_replace('\\', '/', $resource);
    $root = str_replace('\\', '/', __DIR__).'/';
    if (str_starts_with($resource, $root)) {
        return normalize_generated_name(substr($resource, strlen($root)));
    }

    foreach (['/vendor/', '/src/', '/var/cache/prod/'] as $marker) {
        $position = strpos($resource, $marker);
        if (false !== $position) {
            return normalize_generated_name(substr($resource, $position + 1));
        }
    }

    return normalize_generated_name(basename($resource));
}

function resource_manifest(string $path): string
{
    $metadata = json_decode(file_get_contents($path), true);
    $manifest = [];
    foreach ($metadata['resources'] ?? [] as $resource) {
        $type = $resource['@type'] ?? '?';
        $separator = strrpos($type, '\\');
        if (false !== $separator) {
            $type = substr($type, $separator + 1);
        }

        if (isset($resource['resource'])) {
            $subject = normalize_resource($resource['resource']);
        } elseif (isset($resource['className'])) {
            $subject = $resource['className'];
        } else {
            $subject = '-';
        }
        if (isset($resource['exists'])) {
            $subject .= ':exists='.($resource['exists'] ? '1' : '0');
        }
        $manifest[] = $type.':'.$subject;
    }
    return implode(',', $manifest);
}

echo 'health=', request_result('/health'), "\n";
echo 'missing=', request_result('/missing'), "\n";

$cache = __DIR__.'/var/cache/prod/';
$included = [];
foreach (get_included_files() as $file) {
    $normalized = str_replace('\\', '/', $file);
    $prefix = str_replace('\\', '/', $cache);
    if (str_starts_with($normalized, $prefix)) {
        $relative = normalize_generated_name(substr($normalized, strlen($prefix)));
        $basename = basename($relative);
        // Normalize the observable cache-load boundary rather than ancillary
        // env/secret loader services selected by the host process environment.
        if (in_array($basename, [
            'Rphp_SymfonyKernelFixture_KernelProdContainer.php',
            'getRouting_LoaderService.php',
            'url_matching_routes.php',
            'getHealthControllerService.php',
        ], true)) {
            $included[] = $relative;
        }
    }
}
echo 'included=', implode(',', $included), "\n";

$symbols = [];
foreach (get_declared_classes() as $class) {
    if (str_starts_with($class, 'Rphp\\SymfonyKernelFixture\\')
        || str_ends_with($class, '\\Rphp_SymfonyKernelFixture_KernelProdContainer')
        || str_ends_with($class, '\\getRouting_LoaderService')
        || str_ends_with($class, '\\getHealthControllerService')) {
        $symbols[] = normalize_generated_name($class);
    }
}
echo 'symbols=', implode(',', $symbols), "\n";

echo 'container-meta=', resource_manifest($cache.'Rphp_SymfonyKernelFixture_KernelProdContainer.php.meta.json'), "\n";
echo 'route-meta=', resource_manifest($cache.'url_matching_routes.php.meta.json'), "\n";
