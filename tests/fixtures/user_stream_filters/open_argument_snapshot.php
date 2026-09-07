<?php
class OpenSnapshotFilter extends php_user_filter {
    function onCreate(): bool {
        $GLOBALS['path'] = str_repeat('changed-path', 700);
        $GLOBALS['mode'] = 'changed-mode';
        echo "factory\n";
        return true;
    }
    function filter($in, $out, &$consumed, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            $consumed += $b->datalen;
            stream_bucket_append($out, $b);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('open.snapshot', OpenSnapshotFilter::class);
$path = 'php://filter/write=open.snapshot|string.toupper/resource=php://memory';
$mode = 'w+';
$pathAlias =& $path;
$modeAlias =& $mode;
$s = fopen($pathAlias, $modeAlias);
fwrite($s, 'saved'); rewind($s);
echo stream_get_contents($s), ':', stream_get_meta_data($s)['mode'], ':', $mode, "\n";
fclose($s);
$native = fopen('php://memory', 'w+');
fwrite($native, "a\0b"); rewind($native);
echo bin2hex(fread($native, 3)), "\n";
fclose($native);
