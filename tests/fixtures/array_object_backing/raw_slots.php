<?php
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
class StorageRecord {
    private $secret = 9;
    public readonly int $stamp;
    public string $title { get => strtoupper($this->title); }
    public string $summary { get => $this->title . '!'; }
    public ?int $empty = null;
    public function __construct() { $this->stamp = 2; $this->title = 'oak'; }
}
$record = new StorageRecord;
$view = new ArrayObject($record);
echo $view['stamp'], ':', $view['title'], ':', $record->title, "\n";
$view['stamp'] = 'unchecked';
$view['title'] = 'fir';
echo $record->stamp, ':', $record->title, "\n";
var_dump(isset($view['empty']), $view->offsetExists('empty'));
var_dump($view['summary']);
unset($view['stamp']);
try { var_dump($record->stamp); } catch (Error $error) { echo $error->getMessage(), "\n"; }
echo count($view), "\n";
