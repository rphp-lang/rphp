<?php

require __DIR__ . '/vendor/autoload.php';

use Rphp\SymfonyDiFixture\Formatter;
use Rphp\SymfonyDiFixture\Greeter;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Dumper\PhpDumper;
use Symfony\Component\DependencyInjection\Reference;

$container = new ContainerBuilder();
$container->setParameter('app.prefix', 'hello');
$container->register(Formatter::class, Formatter::class);
$container->register('app.greeter', Greeter::class)
    ->setPublic(true)
    ->setArguments([
        new Reference(Formatter::class),
        '%app.prefix%',
    ]);
$container->setAlias(Greeter::class, 'app.greeter')->setPublic(true);
$container->compile();

$code = (new PhpDumper($container))->dump([
    'class' => 'RphpPrebuiltContainer',
    'base_class' => \Rphp\SymfonyDiFixture\PrebuiltContainerBase::class,
    'debug' => false,
]);
file_put_contents(__DIR__ . '/RphpPrebuiltContainer.php', $code);
