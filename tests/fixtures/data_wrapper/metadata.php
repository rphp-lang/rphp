<?php
foreach (['', ';base64', 'text/plain;k=first;k=last;x=a%20b', 'text/plain;mediatype=ignored;base64=no;mode=X;seekable=0;uri=changed'] as $header) {
    $s = fopen("data:$header,YQ==", 'rb');
    $first = stream_get_meta_data($s);
    foreach ($first as $key => $value) { echo $key, '='; var_dump($value); }
    $first['k'] = 'local';
    $second = stream_get_meta_data($s);
    echo 'detached:', (int) (!isset($second['k']) || $second['k'] !== 'local'), "\n";
    fclose($s);
}
