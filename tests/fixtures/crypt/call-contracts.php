<?php
set_error_handler(function($level,$message) { echo 'notice:', $level, ':', $message, "\n"; return true; });
function observe($name,$call) {
    echo $name, "\n";
    try { echo json_encode($call()), "\n"; }
    catch (Throwable $e) {
        echo get_class($e), ':', $e->getMessage(), "\n";
        foreach ($e->getTrace() as $frame) {
            if (($frame['function'] ?? '') === 'crypt') {
                echo 'crypt-args:', json_encode(array_map(fn($v)=>is_object($v)?get_class($v):gettype($v),$frame['args'] ?? [])), "\n";
            }
        }
    }
}
observe('zero',fn()=>crypt());
observe('one',fn()=>crypt('specimen'));
observe('three',fn()=>crypt('specimen','kL',false));
observe('unknown-name',fn()=>crypt(string:'specimen',salt:'kL',extra:1));
observe('reversed-named',fn()=>crypt(salt:'kL',string:'specimen'));
observe('integer',fn()=>crypt(123,'kL'));
observe('float',fn()=>crypt(12.5,'kL'));
observe('boolean',fn()=>crypt(true,'kL'));
observe('null-string',fn()=>crypt(null,'kL'));
observe('null-salt',fn()=>crypt('specimen',null));
observe('array-string',fn()=>crypt([],'kL'));
observe('array-salt',fn()=>crypt('specimen',[]));
class StringSpecimen { public function __toString() { echo "convert\n"; return 'specimen'; } }
observe('object-string',fn()=>crypt(new StringSpecimen,'kL'));
observe('two-objects',fn()=>crypt(new StringSpecimen,new StringSpecimen));
observe('call-user',fn()=>call_user_func('crypt','specimen','kL'));
observe('first-class',fn()=>(crypt(...))('specimen','kL'));
observe('unpack',fn()=>crypt(...['salt'=>'kL','string'=>'specimen']));
observe('strict',fn()=>eval('declare(strict_types=1); return crypt(123, "kL");'));
observe('strict-object',fn()=>eval('declare(strict_types=1); return crypt(new StringSpecimen, "kL");'));
observe('reflection',function() {
    $f = new ReflectionFunction('crypt');
    return [$f->getName(),$f->getNumberOfParameters(),$f->getNumberOfRequiredParameters(),(string)$f->getReturnType(),array_map(fn($p)=>[$p->getName(),(string)$p->getType(),$p->isOptional(),$p->isPassedByReference(),array_map(fn($a)=>$a->getName(),$p->getAttributes())],$f->getParameters())];
});
observe('constants',fn()=>[CRYPT_SALT_LENGTH,CRYPT_STD_DES,CRYPT_EXT_DES,CRYPT_MD5,CRYPT_BLOWFISH,CRYPT_SHA256,CRYPT_SHA512]);
