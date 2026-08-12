<?php

require __DIR__ . '/vendor/autoload.php';

use Symfony\Component\Routing\Exception\MethodNotAllowedException;
use Symfony\Component\Routing\Exception\ResourceNotFoundException;
use Symfony\Component\Routing\Matcher\CompiledUrlMatcher;
use Symfony\Component\Routing\Matcher\Dumper\CompiledUrlMatcherDumper;
use Symfony\Component\Routing\RequestContext;
use Symfony\Component\Routing\Route;
use Symfony\Component\Routing\RouteCollection;

$routes = new RouteCollection();
$routes->add('health', new Route(
    '/health',
    ['_controller' => 'HealthController::show'],
    [],
    [],
    '',
    [],
    ['GET']
));
$routes->add('article_show', new Route(
    '/article/{slug}',
    ['_controller' => 'ArticleController::show', 'format' => 'html'],
    ['slug' => '[a-z0-9-]+'],
    [],
    '',
    [],
    ['GET', 'HEAD']
));

$compiled = (new CompiledUrlMatcherDumper($routes))->getCompiledRoutes();
$context = new RequestContext('', 'GET');
$matcher = new CompiledUrlMatcher($compiled, $context);

$static = $matcher->match('/health');
$dynamic = $matcher->match('/article/rphp-8');
echo $static['_route'], '|';
echo $dynamic['_route'], ':', $dynamic['slug'], ':', $dynamic['format'], '|';

$context->setMethod('POST');
try {
    $matcher->match('/article/rphp-8');
} catch (MethodNotAllowedException $error) {
    echo '405:', implode(',', $error->getAllowedMethods()), '|';
}

$context->setMethod('GET');
try {
    $matcher->match('/missing');
} catch (ResourceNotFoundException) {
    echo '404';
}

echo "\n";
