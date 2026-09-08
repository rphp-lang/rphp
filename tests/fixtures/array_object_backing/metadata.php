<?php
foreach (['exchangeArray', 'getArrayCopy', 'getIterator'] as $name) {
    $method = new ReflectionMethod(ArrayObject::class, $name);
    echo $name, ':', $method->getNumberOfRequiredParameters(), ':', $method->getNumberOfParameters(), "\n";
    foreach ($method->getParameters() as $parameter) echo $parameter->getName(), ':', $parameter->getType(), "\n";
    echo $method->getTentativeReturnType(), "\n";
}
