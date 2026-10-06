<?php

class CoercingReturnOwner {
    function __toString(): string {
        echo 'convert|';
        $GLOBALS['source'] = new stdClass;
        gc_collect_cycles();
        echo 'snapshot:', $GLOBALS['weak']->get() === $this, '|';
        return 'converted';
    }
    function __destruct() { echo 'drop|'; }
}
function coercingReturn(): string { return $GLOBALS['source']; }
$GLOBALS['source'] = new CoercingReturnOwner;
$GLOBALS['weak'] = WeakReference::create($GLOBALS['source']);
$result = coercingReturn();
echo $result, ':', $GLOBALS['weak']->get() === null, '|';
unset($GLOBALS['source'], $GLOBALS['weak']);
