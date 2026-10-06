<?php
$functions = ['ctype_alnum','ctype_alpha','ctype_cntrl','ctype_digit','ctype_graph','ctype_lower','ctype_print','ctype_punct','ctype_space','ctype_upper','ctype_xdigit'];
foreach (['C','C.UTF-8','cs_CZ.UTF-8','C'] as $locale) {
    if (setlocale(LC_CTYPE, $locale) === false) { echo "unavailable:", $locale, "\n"; continue; }
    $bits = '';
    foreach ($functions as $function) for ($byte = 0; $byte < 256; $byte++) $bits .= (int) $function(chr($byte));
    echo $locale, ':', hash('sha256', $bits), '|';
    foreach (['~^[a-z]+$~i','~^[[:alpha:]]+$~i','~^\w+$~','~^\s+$~','~^([a-f]+):([0-9]+)$~i','~^ž+$~iu'] as $pattern) {
        foreach (['AbC','ABC:09', "\t\v", chr(0), chr(128), chr(255), 'žŽ'] as $text) {
            $matches = [];
            echo (int) preg_match($pattern, $text, $matches);
        }
    }
    echo '|', json_encode(preg_replace_callback('~([a-z]+)~i', fn($m) => '[' . $m[0] . ']', 'AbC 19 DEF')), "\n";
    setlocale(LC_NUMERIC, 'C');
    setlocale(LC_CTYPE, 'invalid_RPHP');
    setlocale(LC_ALL, '0');
    echo (int) ctype_alpha('A'), (int) preg_match('~^[a-z]$~i','A'), "\n";
}
setlocale(LC_ALL, 'C');
set_error_handler(function () { setlocale(LC_CTYPE, 'C.UTF-8'); return true; });
var_dump(ctype_alpha(65));
restore_error_handler();
echo (int) preg_match('~^[a-z]+$~i', 'ABC'), "\n";
setlocale(LC_ALL, 'C');
