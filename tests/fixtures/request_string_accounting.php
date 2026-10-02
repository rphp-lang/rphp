<?php
$s = 'literal'; $alias = $s; $s .= '!'; var_dump($s, $alias);
$bytes = "\0\x80\xff"; $copy = $bytes; $bytes .= 'x'; echo bin2hex($copy), ':', bin2hex($bytes), "\n";
function retained($text) { return static fn () => $text; }
$closure = retained(str_repeat('r', 129)); var_dump(strlen($closure()));
function namedCallback() { return 'named'; }
$name = 'namedCallback'; $first = $name(); $second = $name(); $name .= 'missing'; var_dump($first, $second);
$key = str_repeat('k', 33); $array = [$key => $alias]; $key .= '!'; var_dump(array_keys($array), $array);
$base = memory_get_usage(); $large = str_repeat('m', 262144); $live = memory_get_usage(); unset($large); $freed = memory_get_usage();
var_dump($live > $base, $freed < $live);
$reflection = new ReflectionFunction('namedCallback'); var_dump($reflection->getFileName() === __FILE__);
