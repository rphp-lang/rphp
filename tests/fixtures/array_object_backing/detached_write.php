<?php
set_error_handler(static fn() => true);
class CounterRecord { public int $count = 5; public $note = 7; }
$record = new CounterRecord;
$count =& $record->count;
$note =& $record->note;
$view = new ArrayObject($record);
$view['count'] = 'replacement';
$view['note'] = 29;
echo $count, ':', $note, ':', $record->count, ':', $record->note, "\n";
try { $count = 'free'; }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
foreach ([false, true] as $nested) {
    $number = 3;
    $root = new ArrayObject(['n' => &$number]);
    $view = $nested ? new ArrayObject($root) : $root;
    $view['n'] = 13;
    echo $number, ':', $root['n'], "\n";
}
class RetainedEntry { function __destruct() { echo "drop\n"; } }
$entry = new RetainedEntry;
$alias =& $entry;
$record = (object)['slot' => &$entry];
$view = new ArrayObject($record);
$view['slot'] = null;
echo "replaced\n";
$alias = null;
echo "released\n";
