<?php
class ProjectionText {
    public function __toString(): string {
        echo 'string|';
        $GLOBALS['held'] = 'changed';
        eval('class ProjectionLoaded {}');
        return 'converted';
    }
}
class ProjectionContract {
    public function text($value): string { return $value; }
    public function number($value): int {
        try { return $value; }
        finally { echo 'finally-number|'; }
    }
    public static function &reference(&$value): self {
        try { return $value; }
        finally { echo 'finally-reference|'; }
    }
}
$contract = new ProjectionContract;
$held = new ProjectionText;
echo $contract->text($held), ':', $held, ':', (int) class_exists('ProjectionLoaded'), '|';
set_error_handler(function($level, $message) use ($contract) {
    echo 'warning:', $level, ':', $contract->text('nested'), '|';
    return true;
});
echo $contract->number(2.5), '|';
restore_error_handler();
try { $contract->number([]); } catch (TypeError $error) { echo $error->getMessage(), '|'; }
$value = new ProjectionContract;
$alias =& ProjectionContract::reference($value);
echo (int) ($alias === $value), '|';
$alias = new ProjectionContract;
echo (int) ($alias === $value), "\n";
