<?php
class UriTextFilter extends php_user_filter {
    public function onCreate(): bool {
        echo 'create:', $this->filtername, "\n";
        return true;
    }
    public function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            $bucket->data = str_replace('before', 'after', $bucket->data);
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
    public function onClose(): void { echo "closed\n"; }
}
stream_filter_register('uri.text', UriTextFilter::class);
$path = tempnam(sys_get_temp_dir(), 'filter-uri-');
try {
    file_put_contents($path, "before\x00end");
    $uri = 'php://filter/read=uri.text/resource=' . $path;
    echo 'contents:', bin2hex(file_get_contents($uri)), "\n";
    $stream = fopen($uri, 'rb');
    echo 'read:', bin2hex(fread($stream, 2)), ':', bin2hex(stream_get_contents($stream)), "\n";
    fclose($stream);
    ob_start();
    $count = readfile($uri);
    $output = ob_get_clean();
    echo 'output:', bin2hex($output), ':count=', $count, "\n";
    echo 'offset:', bin2hex(file_get_contents($uri, false, null, 2, 3)), "\n";
    file_put_contents($path, '<?php echo "before-code\n"; return 37;');
    echo 'include:', include($uri), "\n";
    echo 'once:', (int)include_once($uri), "\n";
    $stream = fopen('php://filter/write=uri.text/resource=' . $path, 'wb');
    echo 'written:', fwrite($stream, 'before-write'), "\n";
    fclose($stream);
    echo 'native:', file_get_contents($path), "\n";
    echo 'data:', bin2hex(file_get_contents('PHP://FILTER/READ=string%2Etoupper/resource=data://text/plain,ab%00z')), "\n";
} finally {
    unlink($path);
}
