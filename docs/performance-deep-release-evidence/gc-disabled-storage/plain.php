<?php
gc_disable();
for ($i = 0; $i < 300000; ++$i) {
    $object = new stdClass;
    $object->index = $i;
    $alias = $object;
    unset($alias, $object);

}
echo "done
";
