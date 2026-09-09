<?php
foreach ([2, 12] as $width) {
    echo "width=", $width, "\n";
    $anchor = fopen('php://memory', 'w+');
    $alias = $anchor;
    fwrite($anchor, "alpha\nomega");
    $originalId = (int) $anchor;
    $streams = [];
    for ($i = 0; $i < $width; $i++) {
        $streams[] = fopen('php://memory', 'w+');
        fwrite($streams[$i], "item:$i");
    }
    $closed = $streams[0];
    foreach ($streams as $index => $stream) {
        if (($index % 2) === 0) {
            fclose($stream);
        }
    }
    var_dump($anchor === $alias, (int) $alias === $originalId);
    rewind($alias);
    var_dump(fgets($anchor), fread($alias, 5), ftell($anchor));
    $replacement = fopen('php://memory', 'w+');
    var_dump((int) $replacement > (int) $closed, is_resource($closed));
    try {
        fwrite($closed, 'must not reach replacement');
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
    var_dump(ftell($replacement));
    foreach ($streams as $index => $stream) {
        if (($index % 2) === 1) {
            rewind($stream);
            echo fread($stream, 20), "\n";
            fclose($stream);
        }
    }
    unset($anchor);
    var_dump(is_resource($alias), rewind($alias), fread($alias, 5));
    fclose($alias);
    fclose($replacement);
}
