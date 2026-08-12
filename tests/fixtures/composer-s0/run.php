<?php

$loader = require __DIR__ . '/vendor/autoload.php';

echo $loader instanceof Composer\Autoload\ClassLoader ? 'loader|' : 'missing|';
echo Fixture\Service\Greeter::message(), '|', Fixture\fixture_suffix(), "\n";
