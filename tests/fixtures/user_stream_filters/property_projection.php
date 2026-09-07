<?php
error_reporting(E_ALL);
set_error_handler(function($level,$message){ echo "diag:",$level,":",$message,"\n"; return true; });

class NoStream {
    public $filtername; public $params;
    function filter($in,$out,&$consumed,$closing): int { echo 'field:', (int)property_exists($this,'stream'), "\n"; while($b=stream_bucket_make_writeable($in)){ $consumed += $b->datalen; stream_bucket_append($out,$b); } return PSFS_PASS_ON; }
}
class TypedStream extends NoStream { public int $stream = 4; }
class PrivateStream extends NoStream { private $stream; function onClose() { echo 'private:', gettype($this->stream), "\n"; } }
foreach ([NoStream::class,TypedStream::class,PrivateStream::class] as $c) {
    stream_filter_register($c,$c); $s=fopen('php://memory','w+'); stream_filter_append($s,$c,STREAM_FILTER_WRITE);
    try { var_dump(fwrite($s,'x')); } catch(Throwable $e) { echo $e->getMessage(), "\n"; }
    try { var_dump(fclose($s)); } catch(Throwable $e) { echo $e->getMessage(), "\n"; }
    unset($s);
}
