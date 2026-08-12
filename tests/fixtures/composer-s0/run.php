<?php

$loader = require __DIR__ . '/vendor/autoload.php';

echo PHP_VERSION_ID, '|', PHP_VERSION, '|', phpversion(), '|', PHP_INT_SIZE, '|', PHP_SAPI, '|';
echo $loader instanceof Composer\Autoload\ClassLoader ? 'loader|' : 'missing|';
echo Fixture\Service\Greeter::message(), '|', Fixture\fixture_suffix(), "\n";
