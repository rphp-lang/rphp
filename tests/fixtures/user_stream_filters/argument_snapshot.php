<?php
function query_snapshot($kind) {
    $table = ['' => 'saved'];
    $alias =& $table;
    set_error_handler(function ($severity, $message) use (&$table) {
        $table = [];
        return true;
    });
    if ($kind === 'direct') {
        $found = array_key_exists(null, $table);
    } elseif ($kind === 'dynamic') {
        $query = 'array_key_exists';
        $found = $query(null, $table);
    } else {
        $found = array_key_exists(key: null, array: $table);
    }
    restore_error_handler();
    echo $kind, ':', (int)$found, ':', count($alias), "\n";
}
query_snapshot('direct');
query_snapshot('dynamic');
query_snapshot('named');

function materialized_value() { return 'saved'; }
function query_float_snapshot($kind, $keepCopy) {
    $table = [materialized_value()];
    if ($keepCopy) { $copy = $table; }
    set_error_handler(function ($severity, $message) use (&$table) {
        $table = null;
        return true;
    });
    if ($kind === 'direct') {
        $found = array_key_exists(1.0E+42, $table);
    } elseif ($kind === 'dynamic') {
        $query = 'array_key_exists';
        $found = $query(1.0E+42, $table);
    } elseif ($kind === 'alias') {
        $found = key_exists(1.0E+42, $table);
    } else {
        $found = array_key_exists(key: 1.0E+42, array: $table);
    }
    restore_error_handler();
    echo 'float-', $kind, ':', (int)$keepCopy, ':', (int)$found, ':', gettype($table), "\n";
}
query_float_snapshot('direct', false);
query_float_snapshot('direct', true);
query_float_snapshot('dynamic', false);
query_float_snapshot('named', false);
query_float_snapshot('alias', false);
