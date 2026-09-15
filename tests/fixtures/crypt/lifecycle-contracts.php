<?php
// Independent synthetic boundaries, including the system backend's 512-byte
// input limit which must not truncate PHP's extended-DES contract.
foreach ([0, 1, 7, 8, 9, 71, 72, 73, 255, 511, 512, 513, 1024] as $length) {
    $input = str_repeat('a', $length);
    foreach (['kL', '_0...aBcd', '$2y$04$abcdefghijklmnopqrstuu'] as $salt) {
        echo $length, ':', $salt, ':', crypt($input, $salt), "\n";
    }
}
foreach (['_....aBcd', '_/...aBcd', '_0...aBcd', '_0...aB!d', '_0..', '_0...abcdignored'] as $salt) {
    echo $salt, ':', crypt("a\x80\xffz", $salt), "\n";
}
$input = "first\0later";
$reference =& $input;
$copy = $input;
$salt = "kL\0later";
$saltCopy = $salt;
echo crypt($reference, $salt), '|', bin2hex($input), '|', bin2hex($copy), '|', bin2hex($saltCopy), "\n";
class HashSource {
    public function __toString() {
        echo 'nested:', crypt('inner', 'xY'), "\n";
        return 'outer';
    }
}
echo 'outer:', crypt(new HashSource, 'kL'), "\n";
class FailingHashSource {
    public function __toString() { echo "first conversion\n"; throw new Exception('stop conversion'); }
}
class UnreachedSaltSource {
    public function __toString() { echo "unexpected salt conversion\n"; return 'kL'; }
}
try { crypt(new FailingHashSource, new UnreachedSaltSource); }
catch (Exception $e) { echo $e->getMessage(), "\n"; }
echo 'after:', crypt('outer', 'kL'), "\n";
$parameter = (new ReflectionFunction('crypt'))->getParameters()[0];
$attribute = $parameter->getAttributes('SensitiveParameter')[0];
echo $attribute->getName(), ':', $attribute->getTarget(), ':', (int)$attribute->isRepeated(),
    ':', count($attribute->getArguments()), ':', get_class($attribute->newInstance()), "\n";
echo count((new ReflectionFunction('strlen'))->getParameters()[0]->getAttributes()), "\n";
set_error_handler(function($level, $message) { throw new Exception($message); });
try { crypt(null, 'kL'); }
catch (Exception $e) {
    echo $e->getMessage(), "\n";
    foreach ($e->getTrace() as $frame) {
        if (($frame['function'] ?? '') === 'crypt') {
            echo get_class($frame['args'][0]), ':', gettype($frame['args'][0]->getValue()), ':', $frame['args'][1], "\n";
        }
    }
}
restore_error_handler();
echo 'recovered:', crypt('outer', 'kL'), "\n";
