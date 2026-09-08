<?php
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
$directory = dir(__DIR__);
$view = new ArrayObject($directory);
$view['handle'] = null;
try { $directory->read(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
unset($view['path']);
try { var_dump($directory->path); } catch (Error $error) { echo $error->getMessage(), "\n"; }
