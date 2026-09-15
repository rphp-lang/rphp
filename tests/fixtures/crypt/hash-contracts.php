<?php
// Original bounded contract specimens. These are synthetic inputs, not secrets.
$salts = [
    'des' => 'kL', 'des-dot' => './', 'extended' => '_0...aBcd',
    'md5' => '$1$seed.42$', 'md5-long' => '$1$abcdefghijklmnop$',
    'sha256' => '$5$rounds=1000$seed.42$',
    'sha512' => '$6$rounds=1000$seed.42$',
    'bcrypt-a' => '$2a$04$abcdefghijklmnopqrstuu',
    'bcrypt-b' => '$2b$04$abcdefghijklmnopqrstuu',
    'bcrypt-x' => '$2x$04$abcdefghijklmnopqrstuu',
    'bcrypt-y' => '$2y$04$abcdefghijklmnopqrstuu',
];
$inputs = ['empty'=>'', 'ascii'=>'probe-value', 'utf8'=>"caf\xc3\xa9", 'raw'=>"a\x80\xffz", 'nul'=>"probe\0tail"];
foreach ($salts as $saltName=>$salt) {
    foreach ($inputs as $inputName=>$input) {
        $hash = crypt($input,$salt);
        echo json_encode(['name'=>"$saltName/$inputName",'input'=>bin2hex($input),'salt'=>bin2hex($salt),'output'=>bin2hex($hash)]),"\n";
    }
}
$edges = [
    ['empty-salt','probe',''], ['short-salt','probe','x'],
    ['invalid-des','probe','@@'], ['failure-zero','probe','*0'],
    ['failure-one','probe','*1'], ['failure-other','probe','*2'],
    ['unknown-prefix','probe','$9$seed$'], ['nul-salt','probe',"kL\0ignored"],
    ['nul-first-salt','probe',"\0kL"], ['des-truncation','abcdefgh'.str_repeat('z',90),'kL'],
    ['bcrypt-truncation',str_repeat('A',72).'later','$2y$04$abcdefghijklmnopqrstuu'],
    ['bcrypt-bad-cost','probe','$2y$03$abcdefghijklmnopqrstuu'],
    ['bcrypt-bad-alphabet','probe','$2y$04$abcdefghijklmnopqrstu!'],
    ['bcrypt-short','probe','$2y$04$short'],
    ['sha-min-rounds','probe','$5$rounds=1$seed$'],
    ['sha-zero-rounds','probe','$6$rounds=0$seed$'],
    ['sha-leading-zero','probe','$5$rounds=01000$seed$'],
    ['sha-invalid-rounds','probe','$6$rounds=no$seed$'],
    ['sha-negative-rounds','probe','$5$rounds=-1$seed$'],
    ['md5-nul-salt','probe','$1$seed'."\0ignored"],
    ['md5-empty','probe','$1$$'], ['sha-empty','probe','$6$$'],
    ['salt-as-hash','probe','$1$seed.42$incorrecthash'],
];
foreach ($edges as [$name,$input,$salt]) {
    echo json_encode(['name'=>$name,'input'=>bin2hex($input),'salt'=>bin2hex($salt),'output'=>bin2hex(crypt($input,$salt))]),"\n";
}
