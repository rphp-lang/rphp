<?php
class ScalarDestinationFilter extends php_user_filter {
    public function filter($in, $out, &$consumed, $closing): int {
        while ($bucket = stream_bucket_make_writeable($in)) {
            $consumed += $bucket->datalen;
            stream_bucket_append($out, $bucket);
        }
        return PSFS_PASS_ON;
    }
    public function onClose(): void { echo 'close:', $this->params, "\n"; }
}
stream_filter_register('scalar.destination', ScalarDestinationFilter::class);
function assignment_stream($label) {
    $stream = fopen('php://memory', 'w+');
    stream_filter_append($stream, 'scalar.destination', STREAM_FILTER_WRITE, $label);
    return $stream;
}

$target = 0;
$target = assignment_stream('plain');
echo 'stored:', (int)is_resource($target), "\n";
unset($target);
echo "after-plain\n";

$target = false;
$copy = ($target = assignment_stream('expression'));
unset($target);
echo 'copy:', (int)is_resource($copy), "\n";
unset($copy);

$target = null;
$alias =& $target;
$target = assignment_stream('reference');
echo 'reference:', (int)is_resource($alias), "\n";
unset($target);
echo "reference-kept\n";
unset($alias);

class AssignmentTypeBox { public int $value = 4; }
$box = new AssignmentTypeBox;
$typed =& $box->value;
try { $typed = 'invalid'; }
catch (TypeError $error) { echo "type-rejected\n"; }
echo 'typed:', $box->value, ':', $typed, "\n";
$typed = 7;
echo 'typed-write:', $box->value, "\n";
unset($typed, $box);

$target = 9;
$array = [$target];
$target = ['value' => 2];
$copy = $target;
$target['value'] = 3;
echo 'copy-on-write:', $array[0], ':', $copy['value'], ':', $target['value'], "\n";
