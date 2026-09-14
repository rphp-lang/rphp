<?php
// Original iterator projection/retirement contract, independently observed in PHP.
function regex_show($value) { echo json_encode($value, JSON_INVALID_UTF8_SUBSTITUTE), "\n"; }
function regex_attempt($action) {
    try { regex_show($action()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
function regex_state($it) { regex_show([$it->valid(), $it->key(), $it->current()]); }
set_error_handler(function($level, $message) { echo 'diagnostic:', $level, ':', $message, "\n"; return true; });
switch (getenv('RPHP_REGEX_ITERATOR_CASE')) {
case 'metadata':
    foreach (['__construct','accept','getMode','setMode','getFlags','setFlags','getPregFlags','setPregFlags','getRegex'] as $name) {
        $m = new ReflectionMethod(RegexIterator::class, $name);
        echo $name, ':', $m->getNumberOfRequiredParameters(), '/', $m->getNumberOfParameters(), ':', $m->getTentativeReturnType(), "\n";
        foreach ($m->getParameters() as $p) echo $p->getName(), ':', $p->getType(), ':', (int)$p->isOptional(), "\n";
    }
    regex_show([RegexIterator::MATCH, RegexIterator::GET_MATCH, RegexIterator::ALL_MATCHES, RegexIterator::SPLIT, RegexIterator::REPLACE, RegexIterator::USE_KEY, RegexIterator::INVERT_MATCH]);
    $it = new RegexIterator(new ArrayIterator([]), '//');
    regex_show([$it instanceof FilterIterator, $it instanceof OuterIterator, $it->replacement]);
    break;
case 'initial':
    $inner = new ArrayIterator(['north7', 'south8']);
    $it = new RegexIterator($inner, '/[0-9]/');
    regex_show([$it->getMode(), $it->getFlags(), $it->getPregFlags(), $it->getRegex(), $it->getInnerIterator() === $inner]);
    regex_state($it); regex_show($it->accept());
    $it->next(); regex_state($it);
    $it->rewind(); regex_state($it); $it->next(); regex_state($it); $it->next(); regex_state($it);
    break;
case 'modes':
    foreach ([0,1,2,3,4] as $mode) {
        echo 'mode:', $mode, "\n";
        $inner = new ArrayIterator(['a'=>'x7 y9', 'b'=>'none', 'c'=>'v2']);
        $it = new RegexIterator($inner, '/([a-z])([0-9])/', $mode);
        $it->replacement = '${2}-${1}';
        regex_show(iterator_to_array($it));
        regex_show($inner->getArrayCopy());
    }
    break;
case 'keys':
    foreach ([0,1,2,3,4] as $mode) {
        echo 'mode:', $mode, "\n";
        $it = new RegexIterator(new ArrayIterator(['r4'=>['kept'], 'no'=>null, 12=>'t8']), '/[0-9]/', $mode, RegexIterator::USE_KEY);
        $it->replacement = 'digit';
        regex_show(iterator_to_array($it));
    }
    break;
case 'invert':
    foreach ([0,1,2,3,4] as $mode) {
        echo 'mode:', $mode, "\n";
        $it = new RegexIterator(new ArrayIterator(['good5','plain',null,[]]), '/[0-9]/', $mode, RegexIterator::INVERT_MATCH);
        $it->replacement = 'N'; regex_show(iterator_to_array($it));
    }
    break;
case 'mutation':
    $it = new RegexIterator(new ArrayIterator(['aa3','bb4','cc5']), '/([a-z]+)([0-9])/', RegexIterator::GET_MATCH);
    $it->rewind(); regex_state($it); $it->setMode(RegexIterator::MATCH); regex_state($it);
    $it->setFlags(RegexIterator::USE_KEY); regex_state($it); $it->next(); regex_state($it);
    $it->setFlags(0); $it->setMode(RegexIterator::REPLACE); $it->replacement = '$2';
    regex_show($it->accept()); regex_state($it); regex_show($it->accept()); regex_state($it);
    regex_show(iterator_to_array($it));
    break;
case 'replacement':
    $inner = new ArrayIterator(['id7','id8']);
    $it = new RegexIterator($inner, '/([a-z]+)([0-9])/', RegexIterator::REPLACE);
    $text = '$2:$1'; $it->replacement =& $text;
    $it->rewind(); regex_state($it); $text = 'changed-$0'; $it->next(); regex_state($it);
    regex_show($it->replacement); regex_show($inner->getArrayCopy());
    foreach ([null,9,false,[],new stdClass()] as $replacement) {
        regex_attempt(function() use ($it,$replacement) { $it->replacement = $replacement; return iterator_to_array($it); });
    }
    break;
case 'preg_flags':
    foreach ([0,PREG_OFFSET_CAPTURE,PREG_UNMATCHED_AS_NULL,PREG_OFFSET_CAPTURE|PREG_UNMATCHED_AS_NULL] as $flags) {
        $it = new RegexIterator(new ArrayIterator(['a7 b8','x']), '/(?<letter>[a-z])([0-9])?/', RegexIterator::GET_MATCH, 0, $flags);
        regex_show(iterator_to_array($it));
    }
    foreach ([PREG_PATTERN_ORDER,PREG_SET_ORDER,PREG_SET_ORDER|PREG_OFFSET_CAPTURE] as $flags) {
        $it = new RegexIterator(new ArrayIterator(['a7 b8','x']), '/([a-z])([0-9])?/', RegexIterator::ALL_MATCHES, 0, $flags);
        regex_show(iterator_to_array($it));
    }
    foreach ([0,PREG_SPLIT_NO_EMPTY,PREG_SPLIT_DELIM_CAPTURE,PREG_SPLIT_OFFSET_CAPTURE,7] as $flags) {
        $it = new RegexIterator(new ArrayIterator(['a,,b,','plain']), '/(,)/', RegexIterator::SPLIT, 0, $flags);
        regex_show(iterator_to_array($it));
    }
    break;
case 'arguments':
    $inner = new ArrayIterator(['one']);
    foreach ([null,false,'2',2.5,-1,5,[]] as $mode) regex_attempt(fn() => new RegexIterator($inner, '/o/', $mode));
    foreach (['','abc','/[/', '/ok/z'] as $pattern) regex_attempt(fn() => new RegexIterator($inner, $pattern));
    $it = new RegexIterator(iterator: $inner, pattern: '/o/', mode: 0, flags: 0, pregFlags: 0);
    foreach ([-1,5,'2',2.5,[]] as $mode) { regex_attempt(fn() => $it->setMode($mode)); regex_show($it->getMode()); }
    foreach ([-1,999,null,'3',[]] as $flag) { regex_attempt(fn() => $it->setFlags($flag)); regex_show($it->getFlags()); }
    foreach ([-1,999,null,'3',[]] as $flag) { regex_attempt(fn() => $it->setPregFlags($flag)); regex_show($it->getPregFlags()); }
    regex_attempt(fn() => $it->__construct($inner, '/other/')); regex_show($it->getRegex());
    break;
case 'strict':
    $strict = eval('declare(strict_types=1); return function($i,$n) { $i->setMode($n); };');
    $it = new RegexIterator(new ArrayIterator([]), '//');
    foreach ([null,false,1.0,'1',1] as $mode) { regex_attempt(fn() => $strict($it,$mode)); regex_show($it->getMode()); }
    class EmptyRegex extends RegexIterator { function __construct() {} }
    $it = new EmptyRegex();
    foreach (['getMode','getFlags','getPregFlags','getRegex','accept','rewind','current','key','valid','next','getInnerIterator'] as $method) regex_attempt(fn() => $it->$method());
    regex_attempt(fn() => $it->setMode(0));
    break;
case 'override':
    class ChosenRegex extends RegexIterator {
        function accept(): bool {
            regex_state($this);
            $accepted = parent::accept(); regex_show([$accepted,$this->current()]);
            return $accepted && $this->key() !== 2;
        }
    }
    $it = new ChosenRegex(new ArrayIterator(['r4','no','s5',[]]), '/([a-z])([0-9])/', RegexIterator::GET_MATCH);
    regex_show(iterator_to_array($it));
    break;
case 'order':
case 'throw':
    class ObservedRegexInput implements Iterator {
        public $position = 0;
        public static $throw = '';
        function event($name) { echo $name, ':', $this->position, "\n"; if(self::$throw === $name) throw new Exception('input stopped'); }
        function rewind(): void { $this->event('rewind'); $this->position = 0; }
        function valid(): bool { $this->event('valid'); return $this->position < 3; }
        function current(): mixed { $this->event('current'); return ['r4','no','s5'][$this->position]; }
        function key(): mixed { $this->event('key'); return $this->position; }
        function next(): void { $this->event('next'); ++$this->position; }
        function __destruct() { echo "input destroyed\n"; }
    }
    if (getenv('RPHP_REGEX_ITERATOR_CASE') === 'order') {
        $input = new ObservedRegexInput(); $it = new RegexIterator($input, '/[0-9]/');
        regex_show(iterator_to_array($it)); unset($input,$it);
    } else {
        foreach (['rewind','valid','current','key','next'] as $failure) {
            echo 'fail:', $failure, "\n";
            $input = new ObservedRegexInput(); $it = new RegexIterator($input, '/[0-9]/');
            ObservedRegexInput::$throw = $failure;
            regex_attempt(fn() => iterator_to_array($it));
            ObservedRegexInput::$throw = ''; regex_state($it); unset($input,$it);
        }
    }
    break;
case 'casting':
    class RegexText { function __toString(): string { echo "cast\n"; return 'h6'; } }
    $it = new RegexIterator(new ArrayIterator([[],null,false,true,9,2.5,new RegexText()]), '/[0-9]/');
    foreach ($it as $k=>$v) echo $k, ':', gettype($v), "\n";
    $it = new RegexIterator(new ArrayIterator([new stdClass()]), '/./');
    regex_attempt(fn() => iterator_to_array($it));
    break;
case 'bytes':
    $it = new RegexIterator(new ArrayIterator(["a\xffb", "\xc3\xa9", "x\x00z"]), '/./', RegexIterator::GET_MATCH);
    foreach ($it as $key=>$value) { echo $key, ':'; foreach ($value as $part) echo bin2hex($part), ':'; echo "\n"; }
    $it = new RegexIterator(new ArrayIterator(["\xc3\xa9Z"]), '/./u', RegexIterator::ALL_MATCHES);
    regex_show(iterator_to_array($it));
    break;
case 'empty_columns':
    foreach (['/(a)(b)?/', '/(?<piece>x)(y)?/'] as $pattern) {
        foreach ([0, PREG_PATTERN_ORDER, PREG_SET_ORDER] as $flags) {
            $matches = ['old'];
            regex_show(preg_match_all($pattern, 'none', $matches, $flags));
            regex_show($matches);
            $it = new RegexIterator(new ArrayIterator(['none']), $pattern, RegexIterator::ALL_MATCHES, RegexIterator::INVERT_MATCH, $flags);
            regex_show(iterator_to_array($it));
        }
    }
    break;
case 'raw_pattern':
    foreach (["/\xff/", "/\xc3\xa9/", "/\xc3\xa9/u"] as $pattern) {
        $it = new RegexIterator(new ArrayIterator(["\xff", "\xc3\xa9", 'plain']), $pattern, RegexIterator::GET_MATCH);
        echo bin2hex($it->getRegex()), "\n";
        foreach ($it as $k=>$v) echo $k, ':', bin2hex($v[0]), "\n";
    }
    break;
case 'reentry':
    class ReenteredRegexText {
        public $owner;
        public $action;
        function __toString(): string {
            echo "convert\n";
            if ($this->action === 'throw') throw new Exception('conversion failed');
            $this->owner->setMode(RegexIterator::REPLACE);
            $this->owner->setFlags(RegexIterator::INVERT_MATCH);
            $this->owner->replacement = 'replaced';
            return 'a6';
        }
    }
    foreach (['change','throw'] as $action) {
        $text = new ReenteredRegexText(); $text->action = $action;
        $it = new RegexIterator(new ArrayIterator([$text,'plain']), '/[0-9]/', RegexIterator::GET_MATCH);
        $text->owner = $it;
        regex_attempt(fn() => iterator_to_array($it));
        regex_show([$it->valid(), $it->key(), gettype($it->current()), $it->getMode(), $it->getFlags()]);
        unset($text->owner, $it, $text); gc_collect_cycles();
    }
    break;
case 'filter_base':
    class OriginalPositiveFilter extends FilterIterator {
        function accept(): bool { return $this->current() > 0; }
    }
    $inner = new ArrayIterator([-2,4,0,7]); $it = new OriginalPositiveFilter($inner);
    regex_state($it); regex_show(iterator_to_array($it)); regex_show($it->getInnerIterator() === $inner);
    regex_attempt(fn() => clone $it);
    break;
case 'cycle':
    class CycledRegexText { function __destruct() { echo "text retired\n"; } }
    class CycledRegex extends RegexIterator { function __destruct() { echo "filter retired\n"; } }
    $text = new CycledRegexText();
    $it = new CycledRegex(new ArrayIterator([$text]), '/./', RegexIterator::MATCH, RegexIterator::USE_KEY);
    $text->owner = $it;
    $it->rewind(); unset($text,$it);
    gc_collect_cycles(); echo "after collection\n";
    break;
case 'lifetime':
    class KeptRegexInput extends ArrayIterator { function __destruct() { echo "retired\n"; } }
    $input = new KeptRegexInput(['r4','s5']);
    $it = new RegexIterator($input, '/./'); $alias = $it; unset($input,$it);
    regex_show(iterator_to_array($alias));
    regex_attempt(fn() => clone $alias); unset($alias); gc_collect_cycles();
    break;
default: throw new Exception('unknown original regex iterator case');
}
restore_error_handler();
