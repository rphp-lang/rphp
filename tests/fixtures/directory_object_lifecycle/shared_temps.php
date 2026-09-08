<?php
class LifetimeNote {
    public function __construct(public string $label) {}
    public function peek() { echo 'peek:', $this->label, "\n"; }
    public function __destruct() { echo 'drop:', $this->label, "\n"; }
}
function same_owner($value) { return $value; }
function shared_scope() {
    $owner = new LifetimeNote('shared');
    $alias = $owner;
    same_owner($owner)->peek();
    same_owner($alias)->peek();
    var_dump(same_owner($owner) === same_owner($alias));
    echo "end shared scope\n";
}
shared_scope();
echo "after shared scope\n";
(new LifetimeNote('last'))->peek();
echo "after last\n";
$container = new stdClass;
$container->child = new LifetimeNote('nested');
same_owner($container)->child->peek();
unset($container);
echo "after nested\n";
class ReenterNote {
    public function __destruct() {
        echo "reenter\n";
        (new LifetimeNote('inside'))->peek();
    }
}
$reenter = new ReenterNote;
same_owner($reenter);
echo "before reenter\n";
unset($reenter);
echo "done\n";
