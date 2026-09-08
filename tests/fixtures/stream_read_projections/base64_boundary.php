<?php
foreach ([false, true] as $strict) {
    $observed = '';
    for ($byte = 0; $byte < 256; ++$byte) {
        $character = chr($byte);
        foreach ([$character . 'YQ==', 'Y' . $character . 'Q==', 'YQ' . $character . '==', 'YQ==' . $character] as $input) {
            $value = base64_decode($input, $strict);
            $observed .= $value === false ? 'false;' : bin2hex($value) . ';';
        }
    }
    echo $strict ? 'strict:' : 'loose:', strlen($observed), ':', md5($observed), "\n";
}
