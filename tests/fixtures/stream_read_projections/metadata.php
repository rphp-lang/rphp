<?php
foreach (['fgetc', 'fpassthru'] as $name) {
    $function = new ReflectionFunction($name);
    $parameter = $function->getParameters()[0];
    echo $name, ':', $function->getNumberOfParameters(), ':', $function->getNumberOfRequiredParameters(), ':', $function->getReturnType(), "\n";
    var_dump($parameter->getName(), $parameter->hasType(), $parameter->isPassedByReference(), $parameter->isOptional(), $parameter->isDefaultValueAvailable(), $parameter->isVariadic());
}
