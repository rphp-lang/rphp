<?php
$stream = fopen('php://memory', 'w+');
fwrite($stream, "\x00\x7f\x80\xffZ");
rewind($stream);
$alias = $stream;
for ($i = 0; $i < 7; ++$i) {
    $byte = fgetc($alias);
    echo $byte === false ? 'false' : bin2hex($byte), ':', ftell($stream), ':', (int)feof($stream), "\n";
}
fseek($stream, 2);
echo bin2hex(fgetc(stream: $stream)), ':', ftell($alias), "\n";
rewind($alias);
echo bin2hex(fgetc($stream)), "\n";
fclose($alias);
try { fgetc($stream); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
$stream = fopen('data:,ab%00%ff', 'r');
echo bin2hex(fgetc($stream)), bin2hex(fgetc($stream)), bin2hex(fgetc($stream)), bin2hex(fgetc($stream)), "\n";
var_dump(fgetc($stream));
fclose($stream);
