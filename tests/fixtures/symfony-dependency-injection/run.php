<?php

require __DIR__ . '/vendor/autoload.php';
require __DIR__ . '/RphpPrebuiltContainer.php';

use Rphp\SymfonyDiFixture\Greeter;

$container = new RphpPrebuiltContainer();
$greeter = $container->get(Greeter::class);

echo $greeter->greet('rphp'), '|';
echo $greeter === $container->get('app.greeter') ? 'same' : 'different', '|';
echo $container->getParameter('app.prefix'), '|';
echo $container->has('missing.service') ? 'present' : 'missing';
echo "\n";
