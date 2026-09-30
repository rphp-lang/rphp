<?php
class MemoGlobLeft extends GlobIterator {
    function __construct($ready) { if ($ready) parent::__construct('rphp-memo-no-match-*'); }
    function visit($n) { echo 'left:', $n, '|'; }
}
class MemoGlobRight extends GlobIterator {
    function __construct() { parent::__construct('rphp-memo-no-match-*'); }
    function visit($n) { echo 'right:', $n, '|'; }
}
class MemoGlobDispatch { function invoke($receiver) { $receiver->visit(print 'arg|'); } }
$left = new MemoGlobLeft(true);
$right = new MemoGlobRight;
$bare = new MemoGlobLeft(false);
$dispatch = new MemoGlobDispatch;
foreach ([$left, $right, $bare, $left] as $receiver) {
    try { $dispatch->invoke($receiver); } catch (Error $error) { echo 'blocked|'; }
}
