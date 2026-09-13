<?php
function exerciseResourceSlotReuse() {
    // FRAME_LOCALS
    $kept = [];
    $closed = 0;
    for ($i = 0; $i < 32; $i++) {
        $stream = fopen('php://memory', 'w+');
        fwrite($stream, chr(65 + $i % 26));
        rewind($stream);
        $holder = [$stream];
        $reference =& $holder[0];
        if ($i % 7 === 0) {
            $kept[] = $holder;
        } else {
            $closed += (int) fclose($reference);
            if (is_resource($stream)) { echo 'unexpected-open'; }
        }
        unset($stream, $holder, $reference);
    }
    foreach ($kept as $holder) {
        echo fread($holder[0], 1);
        $closed += (int) fclose($holder[0]);
    }
    echo ':', $closed, "\n";
}
exerciseResourceSlotReuse();
