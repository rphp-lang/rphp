<?php

namespace Rphp\SymfonyDiFixture;

final class Formatter
{
    public function format(string $name): string
    {
        return strtoupper($name);
    }
}
