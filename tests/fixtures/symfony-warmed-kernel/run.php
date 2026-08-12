<?php

require __DIR__ . '/vendor/autoload.php';

use Rphp\SymfonyKernelFixture\Kernel;
use Symfony\Component\HttpFoundation\Request;

$kernel = new Kernel('prod', false);
$request = Request::create('/health', 'GET');
$response = $kernel->handle($request);

echo $response->getStatusCode(), '|';
echo $response->headers->get('X-RPHP-Fixture'), '|';
echo $response->getContent(), "\n";

$kernel->terminate($request, $response);
$kernel->shutdown();
