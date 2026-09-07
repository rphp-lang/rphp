<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class RefFilter extends php_user_filter {
    function change(&$text): void { $text = "\xff" . $text . '!'; }
    function filter($in,$out,&$consumed,$closing): int {
        while($b=stream_bucket_make_writeable($in)){ $consumed += $b->datalen; $this->change($b->data); echo 'len:',$b->datalen,"\n"; stream_bucket_append($out,$b); }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('reference',RefFilter::class);
$a=fopen('php://memory','w+'); $b=fopen('php://memory','w+'); stream_filter_append($a,'reference',STREAM_FILTER_WRITE);
fwrite($a,'ab'); fwrite($b,'cd'); rewind($a); rewind($b); echo bin2hex(stream_get_contents($a)),':',stream_get_contents($b),"\n";
fclose($a); fclose($b);
