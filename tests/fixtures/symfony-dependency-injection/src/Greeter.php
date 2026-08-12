<?php

namespace Rphp\SymfonyDiFixture;

final class Greeter
{
    public function __construct(
        private Formatter $formatter,
        private string $prefix,
    ) {
    }

    public function greet(string $name): string
    {
        return $this->prefix . ' ' . $this->formatter->format($name);
    }
}
