<?php
class SlotLifetime {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, ';'; }
}
function slot_transition($seed) {
    $held = new SlotLifetime('held');
    $value = 0;
    $alias =& $value;
    $copy = null;
    for ($i = 0; $i < 4; ++$i) {
        $value = $seed + $i;
        echo $alias, ',';
        $value = new SlotLifetime('v' . $i);
        $copy = $value;
        $value = $i;
        echo $alias, ',';
        unset($copy);
        $value = ['entry' => $i];
        $copy = $value;
        $alias['entry'] += 10;
        echo $copy['entry'], '/', $value['entry'], ';';
        $value = false;
        $value = null;
    }
    unset($alias);
    echo 'return;';
    return $value;
}
var_dump(slot_transition(7));
function slot_unwind() {
    $kept = new SlotLifetime('unwind');
    $value = $kept;
    $value = 19;
    $value = 'old';
    $value = 23;
    throw new Exception('finish');
}
try { slot_unwind(); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
