<?php
chdir(getenv('RPHP_TOUCH_SPEC_DIR'));
foreach (["raw-\xff\x80", 'raw-é'] as $index => $path) {
    var_dump(touch($path, 123 + $index, 456 + $index));
    symlink($path, 'alias' . $index);
    echo bin2hex(readlink('alias' . $index)), ':', filemtime('alias' . $index), ':', fileatime('alias' . $index), "\n";
}
