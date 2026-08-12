<?php

require __DIR__ . '/vendor/autoload.php';

use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;

$request = Request::create(
    '/hello?name=RPHP',
    'POST',
    ['form' => 'value'],
    [],
    [],
    ['HTTP_X_TRACE' => 'alpha']
);

$response = new Response(
    implode('|', [
        $request->getMethod(),
        $request->getPathInfo(),
        $request->query->get('name'),
        $request->request->get('form'),
        $request->headers->get('x-trace'),
    ]),
    Response::HTTP_CREATED,
    ['X-Fixture' => 'http-foundation']
);

echo $response->getStatusCode(), '|';
echo $response->headers->get('x-fixture'), '|';
echo $response->getContent(), "\n";
