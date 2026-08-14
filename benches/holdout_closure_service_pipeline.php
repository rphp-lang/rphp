<?php

// Independent closure-call holdout. This is the pre-existing "closure +
// object mix" from object_audit_workloads.php isolated and scaled so execution
// coverage and sampling can attribute its general call boundary. It is not a
// training workload for region selection.

final class HoldoutClosureTransform
{
    public $callback;

    public function __construct($callback)
    {
        $this->callback = $callback;
    }

    public function apply($value)
    {
        $callback = $this->callback;
        return $callback($value);
    }
}

function runHoldoutClosureServicePipeline($iterations)
{
    $transform = new HoldoutClosureTransform(function ($value) {
        return $value + 1;
    });
    $result = 0;

    for ($index = 0; $index < $iterations; $index++) {
        $result = $transform->apply($result);
    }

    return $result;
}

$start = microtime(true);
$result = runHoldoutClosureServicePipeline(750000);
$elapsed = microtime(true) - $start;
echo $result . '|' . $elapsed;
