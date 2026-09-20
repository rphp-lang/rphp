<?php
// Regenerate hello.phar with: php -d phar.readonly=0 build.php
// The stub maps the archive, includes members through phar:// and reports
// Phar::running() from inside a member.
chdir(__DIR__);
@unlink("hello.phar");
$p = new Phar("hello.phar");
$p->startBuffering();
$p->buildFromDirectory("src");
$p->addEmptyDir("empty");
$stub = <<<'STUB'
#!/usr/bin/env php
<?php
Phar::mapPhar("hello.phar");
require "phar://hello.phar/lib/greet.php";
$info = require "phar://hello.phar/lib/info.php";
echo \Hello\greet($argv[1] ?? "world"), "|", basename($info["dir"]), "|", $info["running"] === "phar://" . realpath(__FILE__) ? "run-ok" : $info["running"], "|", $info["plain"] === realpath(__FILE__) ? "plain-ok" : "plain-bad", "\n";
__HALT_COMPILER();
STUB;
$p->setStub($stub);
$p->setSignatureAlgorithm(Phar::SHA256);
$p->stopBuffering();
echo count($p), " files, sig=", $p->getSignature()["hash_type"], "\n";
