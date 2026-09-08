<?php
function plain($label, $value) {
    echo $label, ':', $value, "\n";
    return $value + 1;
}
function plainThrow() {
    echo "plain-throw\n";
    throw new RuntimeException('replacement');
}
function pendingReturn() {
    try {
        return plain('return', 3);
    } finally {
        plain('finally-return', 8);
    }
}
echo 'result:', pendingReturn(), "\n";
try {
    try {
        throw new RuntimeException('original');
    } finally {
        plain('pending-exception', 10);
        try {
            plainThrow();
        } catch (RuntimeException $inner) {
            echo 'inner:', $inner->getMessage(), "\n";
        }
        plain('restored-exception', 20);
    }
} catch (RuntimeException $outer) {
    echo 'outer:', $outer->getMessage(), "\n";
}
try {
    try {
        pendingReturn();
        throw new RuntimeException('discarded');
    } finally {
        plainThrow();
    }
} catch (RuntimeException $outer) {
    echo 'replaced:', $outer->getMessage(), "\n";
}
class FinallyRelease {
    function __destruct() { plain('destructor', 30); }
}
function releasePlain() {
    $value = new FinallyRelease;
    return plain('release-return', 40);
}
try {
    try {
        throw new RuntimeException('through-release');
    } finally {
        try {
            echo 'released:', releasePlain(), "\n";
        } finally {
            plain('nested-finally', 50);
        }
    }
} catch (RuntimeException $outer) {
    echo 'final:', $outer->getMessage(), "\n";
}
echo "completion\n";
