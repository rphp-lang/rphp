<?php

namespace Rphp\SymfonyKernelFixture;

use Symfony\Component\HttpFoundation\Response;

final class HealthController
{
    public function __invoke(): Response
    {
        return new Response('OK', 200, ['X-RPHP-Fixture' => 'warmed']);
    }
}
