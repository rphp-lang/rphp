--TEST--
Request startup settings affect FILE but cannot execute in SKIPIF or CLEAN
--INI--
precision=8
auto_prepend_file={PWD}/ini-prepend.inc
auto_append_file={PWD}/ini-prepend.inc
--SKIPIF--
<?php
if (ini_get('precision') !== '14' || ini_get('auto_prepend_file') !== '') {
    echo 'test INI leaked into SKIPIF';
}
?>
--FILE--
<?php echo ini_get('precision'), ':'; ?>
--CLEAN--
<?php
if (ini_get('precision') !== '14' || ini_get('auto_append_file') !== '') {
    echo 'test INI leaked into CLEAN';
}
?>
--EXPECT--
prelude-8:prelude-
