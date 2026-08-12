<?php

namespace Rphp\SymfonyDiFixture;

abstract class PrebuiltContainerBase
{
    protected $services = [];
    protected $privates = [];
    protected $methodMap = [];
    protected $aliases = [];
    protected $parameters = [];

    public function get(string $id): object
    {
        $id = $this->aliases[$id] ?? $id;

        return $this->services[$id] ?? $this->{$this->methodMap[$id]}($this);
    }

    public function has(string $id): bool
    {
        $id = $this->aliases[$id] ?? $id;

        return isset($this->services[$id]) || isset($this->methodMap[$id]);
    }

    public function getParameter(string $name): mixed
    {
        return $this->parameters[$name];
    }
}
