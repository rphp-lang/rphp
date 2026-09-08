<?php
set_error_handler(function ($level, $message) { echo "$level:$message\n"; });
class PolicyLocal {
    public $context;
    function stream_open($path, $mode, $options, &$opened): bool { echo "local-open\n"; return true; }
    function dir_opendir($path, $options): bool { echo "local-directory\n"; return true; }
}
class PolicyRemote extends PolicyLocal {
    function stream_open($path, $mode, $options, &$opened): bool { echo "unexpected-remote-open\n"; return true; }
    function dir_opendir($path, $options): bool { echo "unexpected-remote-directory\n"; return true; }
}
stream_wrapper_register('policylocal', PolicyLocal::class);
stream_wrapper_register('policyremote', PolicyRemote::class, STREAM_IS_URL);
var_dump(fopen('policyremote://item', 'r'));
var_dump(opendir('policyremote://item'));
$s = fopen('policylocal://item', 'r'); echo get_resource_type($s), "\n"; fclose($s);
$d = opendir('policylocal://item'); echo get_resource_type($d), "\n"; closedir($d);
