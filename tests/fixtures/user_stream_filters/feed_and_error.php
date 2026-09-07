<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class StatusFilter extends php_user_filter {
    function filter($in,$out,&$consumed,$closing): int {
        while($b=stream_bucket_make_writeable($in)){ $consumed += $b->datalen; stream_bucket_append($out,$b); }
        return $this->params;
    }
}
stream_filter_register('status',StatusFilter::class);
foreach([PSFS_PASS_ON,PSFS_FEED_ME,PSFS_ERR_FATAL] as $status) {
    $s=fopen('php://memory','w+'); $f=stream_filter_append($s,'status',STREAM_FILTER_WRITE,$status);
    echo 'status:',$status,"\n"; var_dump(fwrite($s,'xy')); var_dump(stream_filter_remove($f));
    rewind($s); var_dump(stream_get_contents($s)); fclose($s);
}
