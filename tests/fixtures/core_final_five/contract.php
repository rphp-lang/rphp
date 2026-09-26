<?php
switch (getenv('RPHP_FINAL_FIVE_CASE')) {
case 'relative-ancestors':
    eval('namespace Domain; interface ParentContract {} interface ChildContract extends namespace\\ParentContract {} trait Shared { public function name(){return "trait";} } class Base {} class Child extends namespace\\Base implements namespace\\ChildContract { use namespace\\Shared; }');
    $child = new Domain\Child;
    echo get_parent_class($child), '|', (int)($child instanceof Domain\ChildContract), '|', $child->name(), "\n";
    break;
case 'relative-doc-comments':
    eval('/** namespace-only */ namespace Docs { function plain(){} /** base-only */ class Base {} /** child-only */ class Child extends namespace\\Base {} }');
    var_dump((new ReflectionFunction('Docs\\plain'))->getDocComment());
    echo (new ReflectionClass('Docs\\Base'))->getDocComment(), '|', (new ReflectionClass('Docs\\Child'))->getDocComment(), "\n";
    break;
case 'integer-members':
    $object = unserialize('O:8:"stdClass":3:{i:7;s:1:"a";i:-2;s:1:"b";s:1:"7";s:1:"c";}');
    echo json_encode((array)$object), '|', $object->{'7'}, '|', $object->{'-2'}, "\n";
    $object->{'7'} = 'd';
    echo serialize($object), "\n";
    break;
case 'integer-member-cycle':
    function numericCycle() {
        $object = unserialize('O:8:"stdClass":1:{i:2;r:1;}');
        echo (int)($object->{'2'} === $object), '|', count((array)$object), "\n";
        return WeakReference::create($object);
    }
    $weak = numericCycle();
    gc_collect_cycles();
    var_dump($weak->get());
    break;
case 'integer-member-deprecation':
    class MemberTarget {}
    set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
    $value = unserialize('O:12:"MemberTarget":2:{i:3;i:9;s:1:"3";i:10;}');
    echo $value->{'3'}, '|', count((array)$value), "\n";
    restore_error_handler();
    break;
case 'regex-fiber-resume':
    $fiber = new Fiber(function() {
        $count = -1;
        $result = preg_replace_callback('/[ab]/', function($match) {
            echo 'enter:', $match[0], '|';
            $suffix = Fiber::suspend($match[0]);
            echo 'leave:', $suffix, '|';
            return strtoupper($match[0]) . $suffix;
        }, 'ab', -1, $count);
        echo 'result:', $result, ':', $count, '|';
    });
    echo 'start:', $fiber->start(), '|';
    echo 'next:', $fiber->resume('1'), '|';
    $fiber->resume('2');
    echo 'done:', (int)$fiber->isTerminated(), "\n";
    break;
case 'regex-fiber-throw':
    $fiber = new Fiber(function() {
        try {
            preg_replace_callback('/(.)/', function($match) {
                try { Fiber::suspend($match[1]); }
                finally { echo 'callback-finally|'; }
            }, 'z');
        } catch (Exception $error) { echo 'caught:', $error->getMessage(), '|'; }
        echo 'finished|';
    });
    echo $fiber->start(), '|';
    $fiber->throw(new Exception('injected'));
    echo (int)$fiber->isTerminated(), "\n";
    break;
case 'regex-fiber-cycle':
    class CallbackOwner { function __destruct(){echo 'released|';} }
    $fiber = new Fiber(function() {
        $owner = new CallbackOwner;
        preg_replace_callback('/./', function($match) {
            $self = Fiber::getCurrent();
            Fiber::suspend();
        }, 'x');
    });
    $fiber->start();
    echo 'parked|';
    gc_collect_cycles();
    echo 'live|';
    $fiber = null;
    gc_collect_cycles();
    echo 'end', "\n";
    break;
case 'regex-fiber-nested':
    $fiber = new Fiber(function() {
        $result = preg_replace_callback('/./', function($outer) {
            return preg_replace_callback('/./', function($inner) { return Fiber::suspend($inner[0]); }, $outer[0]);
        }, 'ab');
        echo $result, '|';
    });
    echo $fiber->start(), '|', $fiber->resume('A'), '|';
    $fiber->resume('B');
    echo (int)$fiber->isTerminated(), "\n";
    break;
case 'regex-fiber-arrays':
    $fiber = new Fiber(function() {
        $subjects = ['first'=>'a', 9=>'b'];
        $copy = $subjects;
        $count = 7;
        $result = preg_replace_callback(['/([ab])(?<tail>z)?/', '/X/'], function($match) {
            echo json_encode($match), '|';
            return Fiber::suspend('match');
        }, $subjects, 1, $count, PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL);
        echo json_encode($result), ':', $count, ':', (int)($copy === $subjects), "\n";
    });
    $fiber->start(); $fiber->resume('X'); $fiber->resume('A'); $fiber->resume('B');
    break;
case 'image-typed-output':
    class ImageTyped { public int $info = 7; }
    $object = new ImageTyped;
    try { getimagesize('', $object->info); }
    catch (Throwable $error) { echo $error->getMessage(), '|', $object->info, "\n"; }
    break;
case 'regex-fiber-ref-warning':
    function referenceMatch(&$match) { return Fiber::suspend($match[0]); }
    set_error_handler(function($level, $message){ echo $level, ':', $message, '|'; });
    $count = 7;
    $fiber = new Fiber(function() use (&$count) { echo preg_replace_callback('/./', 'referenceMatch', 'a', -1, $count), '|'; });
    echo $fiber->start(), ':', $count, '|';
    $fiber->resume('X');
    echo $count, "\n";
    break;
case 'random-construction-guard':
    foreach ([Random\Engine\Secure::class, Random\Engine\Xoshiro256StarStar::class] as $name) {
        try { (new ReflectionClass($name))->newInstanceWithoutConstructor(); }
        catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    }
    break;
case 'random-state':
    $engine = new Random\Engine\Xoshiro256StarStar(17);
    $copy = clone $engine;
    echo bin2hex($engine->generate()), '|', bin2hex($copy->generate()), "\n";
    echo json_encode($engine->__serialize()), "\n";
    $restored = unserialize(serialize($engine));
    echo (int)($restored->generate() === $engine->generate()), "\n";
    $copy->jump(); echo bin2hex($copy->generate()), "\n";
    $copy->jumpLong(); echo bin2hex($copy->generate()), "\n";
    $seed = str_repeat("\x01\x00\x00\x00\x00\x00\x00\x00", 4);
    echo bin2hex((new Random\Engine\Xoshiro256StarStar($seed))->generate()), "\n";
    break;
case 'random-capabilities':
    $secure = new Random\Engine\Secure;
    echo strlen($secure->generate()), '|', (int)($secure instanceof Random\Engine), '|', (int)($secure instanceof Random\CryptoSafeEngine), "\n";
    foreach ([$secure, new Random\Engine\Xoshiro256StarStar(3)] as $engine) {
        try { $copy = clone($engine, ['note' => 1]); }
        catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
        echo count((array)$engine), "\n";
    }
    try { clone $secure; } catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    try { serialize($secure); } catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    break;
case 'random-invalid-state':
    foreach (['short', str_repeat("\0", 32), []] as $seed) {
        try { new Random\Engine\Xoshiro256StarStar($seed); }
        catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    }
    $engine = new Random\Engine\Xoshiro256StarStar(7);
    $before = $engine->__serialize();
    try { $engine->__unserialize([[], []]); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    echo (int)($before === $engine->__serialize()), "\n";
    break;
case 'image-headers':
    $png = "\x89PNG\r\n\x1a\n" . pack('N', 13) . 'IHDR' . pack('NNC5', 3, 2, 8, 6, 0, 0, 0);
    $gif = 'GIF89a' . pack('vvCCC', 5, 4, 0x80, 0, 0);
    $jpg = "\xff\xd8\xff\xe1\x00\x06demo\xff\xc0\x00\x11\x08\x00\x07\x00\x09\x03\x01\x11\x00\x02\x11\x00\x03\x11\x00\xff\xd9";
    foreach ([$png, $gif, $jpg] as $bytes) {
        $info = 'old';
        echo json_encode(getimagesizefromstring($bytes, $info)), '|', json_encode($info), "\n";
        $file = tempnam(sys_get_temp_dir(), 'image-contract-');
        file_put_contents($file, $bytes);
        echo (int)(getimagesize($file) === getimagesizefromstring($bytes)), "\n";
        unlink($file);
    }
    break;
case 'image-output-release':
    class ImageOutput {
        function __destruct() { global $output; echo gettype($output), '|'; $output = 'reentrant'; throw new Exception('released'); }
    }
    $output = [new ImageOutput];
    try { getimagesize('not-opened', $output); }
    catch (Throwable $error) { echo $error->getMessage(), '|', json_encode($output), "\n"; }
    $output = 23;
    try { getimagesize([], $output); }
    catch (Throwable $error) { echo $error::class, '|', $output, "\n"; }
    try { getimagesize('', $output); }
    catch (Throwable $error) { echo $error->getMessage(), '|', json_encode($output), "\n"; }
    break;
case 'image-invalid-data':
    set_error_handler(function($level, $message) { echo $level, ':', $message, '|'; });
    foreach (['', 'bad', 'not an image', "GIF89a", "\x89PNG\r\n\x1a\n"] as $bytes) {
        $info = ['previous'];
        var_dump(getimagesizefromstring($bytes, $info));
        echo json_encode($info), "\n";
    }
    break;
}
