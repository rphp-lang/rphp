<?php
declare(strict_types=1);
foreach ([false, 1, null, [], new stdClass] as $arg) {
    try { dir($arg); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
