<?php
// Pure scalar method with a masked branch and checked arithmetic on both arms.
class BranchKernel
{
    public function route(int $value): int
    {
        if (($value & 1) == 0) {
            return ($value * 3) + 1;
        }
        return ($value * 5) - 2;
    }
}

$kernel = new BranchKernel();
$sum = 0;
$t = microtime(true);
for ($i = 0; $i < 10000000; $i++) {
    $sum += $kernel->route($i);
}
$elapsed = microtime(true) - $t;
echo $sum . '|' . $elapsed;
