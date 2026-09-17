<?php
error_reporting(E_ALL);
class HookMap {
    public function __unserialize(array $data): void { var_dump($data); }
}
unserialize('O:7:"HookMap":4:{i:0;s:5:"first";s:1:"0";s:6:"second";s:2:"00";i:3;s:1:"x";i:4;}');
unserialize('O:7:"HookMap":3:{i:0;i:1;s:1:"0";i:2;i:0;i:3;}');
unserialize('O:7:"HookMap":3:{s:1:"0";i:1;i:0;i:2;s:1:"0";i:3;}');
foreach (['SplFixedArray' => ['__serialize', '__unserialize'], 'ArrayObject' => ['__debugInfo'], 'ArrayIterator' => ['__debugInfo']] as $class => $methods) {
    foreach ($methods as $name) {
        $method = new ReflectionMethod($class, $name);
        echo $class, '::', $method->getName(), ':', $method->getNumberOfRequiredParameters(), ':', $method->getNumberOfParameters(), ':';
        echo $method->hasTentativeReturnType() ? $method->getTentativeReturnType() : $method->getReturnType();
        echo ':', $method->hasTentativeReturnType() ? 'tentative' : 'declared', "\n";
        foreach ($method->getParameters() as $parameter) {
            echo $parameter->getName(), ':', $parameter->getType(), "\n";
        }
    }
}
