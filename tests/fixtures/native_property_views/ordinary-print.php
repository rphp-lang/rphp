<?php
#[AllowDynamicProperties]
class DeclaredPrintOwner {
    public $first;
    public $last = 'before';
}
#[AllowDynamicProperties]
class DynamicPrintOwner {
    public $first;
}
class MutatingPrintChild {
    public function __debugInfo(): array {
        global $root;
        $root->last = 'after';
        $root->late = 'added';
        return ['child' => 1];
    }
}
$root = new DeclaredPrintOwner;
$root->first = new MutatingPrintChild;
echo print_r($root, true);
echo 'state:', $root->last, ',', $root->late, "\n";
$root = new DynamicPrintOwner;
$root->first = new MutatingPrintChild;
$root->last = 'before';
echo print_r($root, true);
echo 'state:', $root->last, ',', $root->late, "\n";
echo print_r((object) ['left' => 2, 'right' => [3, 4]], true);
trait PrintedTrait {
    public function __debugInfo(): array { return ['trait' => 5]; }
}
class TraitPrintOwner { use PrintedTrait; }
class InheritedPrintOwner extends TraitPrintOwner {}
class CasePrintOwner {
    public function __DeBuGiNfO(): array { return ['case' => 6]; }
}
foreach ([new TraitPrintOwner, new InheritedPrintOwner, new CasePrintOwner] as $view) {
    echo print_r($view, true);
}
