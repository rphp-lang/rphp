<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class BusyFilter extends php_user_filter {
    function filter($in,$out,&$consumed,$closing): int {
        echo 'filter:',(int)$closing,':',gettype($this->stream),"\n";
        try { var_dump(fclose($this->stream)); } catch(Throwable $e){echo $e->getMessage(),"\n";}
        while($b=stream_bucket_make_writeable($in)){ $consumed += $b->datalen; stream_bucket_append($out,$b); }
        return PSFS_PASS_ON;
    }
    function onClose(): void { echo 'close:',gettype($this->stream),"\n"; }
}
stream_filter_register('busy',BusyFilter::class); $s=fopen('php://memory','w+'); stream_filter_append($s,'busy',STREAM_FILTER_WRITE);
fwrite($s,'x'); fclose($s);
