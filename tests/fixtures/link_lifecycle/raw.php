<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
foreach (["missing-\xff\x80", 'missing-é'] as $index => $target) {
    $name = "entry-\xff-" . $index;
    var_dump(symlink($target, $name));
    echo bin2hex(readlink($name)), ':', bin2hex(readlink($name)) === bin2hex($target) ? 'same' : 'changed', "\n";
    var_dump(link($name, 'hard-' . $index));
    echo bin2hex(readlink('hard-' . $index)), "\n";
}
