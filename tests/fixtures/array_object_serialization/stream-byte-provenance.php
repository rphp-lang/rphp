<?php
// Distinguish source UTF-8 strings from externally supplied binary storage.
$cases = ['ascii' => 'simple', 'utf8' => "caf\u{e9}", 'wide' => "\u{1f30d}", 'binary' => hex2bin('00ff807f'), 'binary-ascii' => hex2bin('41620043')];
foreach ($cases as $name => $value) {
    foreach ([null, 2, 0] as $length) {
        $stream = fopen('php://memory', 'w+');
        $written = fwrite($stream, $value, $length);
        rewind($stream);
        // The basic reader is available in every feature configuration.
        $bytes = fread($stream, 8192);
        echo $name, ':', $length === null ? 'all' : $length, ':', $written, ':', bin2hex($bytes), "\n";
        fclose($stream);
    }
}
