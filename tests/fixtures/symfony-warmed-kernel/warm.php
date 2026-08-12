<?php

require __DIR__ . '/vendor/autoload.php';

use Rphp\SymfonyKernelFixture\Kernel;

$kernel = new Kernel('prod', false);
$kernel->boot();
$kernel->shutdown();
