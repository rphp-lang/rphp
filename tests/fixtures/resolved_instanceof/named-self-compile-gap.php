<?php
function outsideClassScope() {
    try {
        $value = 42;
        $value instanceof self;
    } catch (Error $error) {
        echo 'caught';
    }
}
outsideClassScope();
