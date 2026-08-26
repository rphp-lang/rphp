mod common;

use common::{run_php, run_php_expect_error_with_source_context};

#[test]
fn source_highlighter_preserves_ordinary_utf8_output_bytes() {
    assert_eq!(
        run_php("<?php echo highlight_string(\"<?php echo 'žluťoučký kůň'; ?>\", true);"),
        concat!(
            "<pre><code style=\"color: #000000\">",
            "<span style=\"color: #0000BB\">&lt;?php </span>",
            "<span style=\"color: #007700\">echo </span>",
            "<span style=\"color: #DD0000\">'žluťoučký kůň'</span>",
            "<span style=\"color: #007700\">; </span>",
            "<span style=\"color: #0000BB\">?&gt;</span>",
            "</code></pre>",
        )
    );
}

#[test]
fn source_highlighter_matches_generated_php_85_token_and_byte_boundaries() {
    assert_eq!(
        run_php(
            r###"<?php
$snippets = [
    '',
    'plain <&> html',
    "<?php ?>",
    "<?php\n?>\n",
    "<?php\r\n?>\r\n",
    "a<?php echo 1;?>b<?= \$x ?>c",
    "<?PHP echo 1; ?>",
    "<?php abstract and array as break callable case catch class clone const continue declare default die do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum eval exit extends final finally fn for foreach from function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require require_once return static switch throw trait try unset use var while xor yield __halt_compiler; ?>",
    "<?php true TRUE false FALSE null NULL self parent static; ?>",
    "<?php (int)\$x; ( integer ) \$x; (bool)\$x; (boolean)\$x; (float)\$x; (double)\$x; (real)\$x; (string)\$x; (binary)\$x; (array)\$x; (object)\$x; (unset)\$x; ?>",
    "<?php 0 1 09 0xFF 0b10 0o77 1.25 .5 1e3 1_000; ?>",
    "<?php + - * / % ** = == === != !== <=> < <= > >= && || ! & | ^ ~ << >> ?? ??= ? : -> ?-> :: => ... @; ?>",
    "<?php \$a \$_b \$a1 \$\$x; ?>",
    "<?php 'a\\\'b\\\\&<\$x'; ?>",
    "<?php \"plain & < >\"; ?>",
    "<?php \"a\$x:b\$y\"; ?>",
    "<?php \"a\\\$x:b\\\"c\"; ?>",
    "<?php \"a{\$x}b\"; ?>",
    "<?php \"a\${x}b\"; ?>",
    "<?php \"a\$x[0]b\"; ?>",
    "<?php \"a\$obj->prop b\"; ?>",
    "<?php `plain`; `a\$x:b`; ?>",
    "<?php # comment\n// line ?>html",
    "<?php /* block\n & < > */ echo 1; ?>",
    "<?php #[Attr] class C {} ?>",
    "<?php # [not attribute]\necho 1; ?>",
    "<?php \$x = <<<TXT\nhello & < \$x\nTXT;\necho \$x; ?>",
    "<?php \$x = <<<'TXT'\nhello & < \$x\nTXT;\necho \$x; ?>",
    "<?php \$x = <<<\"TXT\"\r\nhello\r\nTXT;\r\n?>",
    "<?php echo '" . chr(0) . chr(127) . chr(128) . chr(255) . "'; ?>",
    chr(0) . chr(1) . chr(127) . chr(128) . chr(255) . "<?php echo 1; ?>" . chr(255),
];
$path = '/tmp/rphp-source-highlighting-generated.php';
foreach ($snippets as $index => $source) {
    $highlighted = highlight_string($source, true);
    file_put_contents($path, $source);
    $stripped = php_strip_whitespace($path);
    echo $index, ':', strlen($highlighted), ':', md5($highlighted), ':', strlen($stripped), ':', md5($stripped), "\n";
}
unlink($path);
"###,
        ),
        r#"0:47:2885518eae219ef656196b400a066a70:0:d41d8cd98f00b204e9800998ecf8427e
1:71:4e02316a84abf92efb5e47c38b21c85f:14:61bd52c2741c524e039d18789fbb022f
2:97:054da056fcc907032fbe6041a99b214f:8:f309d694485da86750abf1a1d7d426d5
3:98:1f5e287cac76a1fa881caab80cdf4d19:9:718d8596a14d123d83afb0d5d6d6fd96
4:100:990ff989f49191df4c064e1201e394ca:11:a53d7d825cad06c4915bd6970e5d1b8b
5:302:8b5aaab27cb3463f81a9dd1537a70ca0:27:4fb4bd3017536cea7d6ed4940d92af3c
6:249:2e3253c6bac7a316da3d051aadb20f6d:16:79430b1e187d75923b45cc9d83ceb5fa
7:739:1779ab92c9582c23138cd7e14298ee12:506:3f05fd08318396baa79d5cd34a9d3618
8:221:401574fb6ae50c9c96c84b320c0229d7:60:b7d25f98d8078d6f3bfd455abf9969cd
9:1172:8eac36d1216741da996875fda9d6eea3:147:3b0bbd2604df84cdded3dd79e64096fe
10:210:862f0b213002761a6eca3b35ab9014f3:49:c53ed1d3d0e89b3dd450bd203979a58e
11:316:187fdd5721c3b1538e3b95ad696e42ba:104:0da02a1a039d4f37a5567c585e075975
12:257:37fc81142710ce35ff93b42975687af8:24:acd2fdfa0b753d14af9950b946eb27b0
13:299:c00a38ef1591aaaa9543329fd6a72b63:23:a9a08cb3c057a7b53a73126ee30ea101
14:230:c6cfdeb47d8aa5bd7c849e4a6986d9a4:23:c8d38e9f3a3ccdb14c343b5c245965ce
15:360:ef82e51f1a052c91d4a307f3b6001e12:19:084071f7b6a0e98f204cf5dd521dfd89
16:218:8167a60933ca6ad6e14808017c09c751:21:91d019ff452312cfe3fe27fa075bdccc
17:359:7004539ee913362e9077274d9a7aeab2:18:a03ab91bc3102f8be1f7efde4fa52980
18:359:aa20b1c8e62676a26924e1439988dce1:18:5ab3dd1b1e1049f745162b00a695f540
19:396:4020b58828a352a9dda01bbcc22a9f41:19:d6689d1fda1a85ef75c51a77cb9e7d42
20:369:cccfef91aa86b1f36c4e0e08f2296243:25:cd8b92ed552a052613a4280e3f8ea188
21:403:2ca7d329cc6a634efc1768416a0fceef:26:3311742ab6c00432b163d9134e86556f
22:191:95172f5bd1c177316f90069d32b686dd:13:384d4752c42048ee7b9b2e11afe57217
23:314:60e79ec713dee38f723317e48414ee41:17:9acfa1ca11a51729568c1723534eaf59
24:332:249c08334741be051de80729cf051a95:27:d095d092bb0e1d5ababeff37857e1877
25:303:de9a03af485c8a7bf36d38531b3bb8b4:17:9acfa1ca11a51729568c1723534eaf59
26:440:7e4e31fe38c3407ef20d14d5634443d9:47:d320d85fbb570ff2d69a6e00c11ce923
27:370:bfd5defa69da3b900aee3fc7cf8ccadf:49:bc3886660f9d1bc02765bed530874190
28:278:b7293ff06cea9a087922e2004f4e2748:35:b7f0d5d58f0d94a9b07428e1901a4ec7
29:254:057a28842c8f7987a0bcbe866d1aebe9:21:7ef971ff87a99cfd72e0eca83a381932
30:255:15a861288ea42b9f3669d56d7880b3fa:22:2375e39ca0806fcaf9c019dffcf5851b
"#,
    );
}

#[test]
fn source_file_filters_share_binary_calls_failures_and_reflection() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(static function (int $level, string $message): bool {
    echo "diag=$level:$message\n";
    return true;
});
$path = '/tmp/rphp-source-filter-e2e.php';
$source = "head<&>\r\n<?php\r\n#[Flag]\r\n\$text = <<<TXT\r\nA&< \$text\r\nTXT;\r\n/* remove */ echo '" . chr(255) . "';\r\n?>\r\ntail" . chr(128);
file_put_contents($path, $source);
foreach (['highlight_file', 'show_source', 'php_strip_whitespace'] as $name) {
    $value = $name($path, ...($name === 'php_strip_whitespace' ? [] : [true]));
    echo $name, '=', get_debug_type($value), ':', strlen($value), ':', md5($value), "\n";
}
ob_start();
$printedReturn = highlight_file($path, false);
$printed = ob_get_clean();
echo 'print=', get_debug_type($printedReturn), ':', $printedReturn ? 'true' : 'false', ':', strlen($printed), ':', md5($printed), "\n";
$dynamic = 'show_source';
$first = highlight_file(...);
foreach ([
    'named' => static fn () => highlight_file(return: true, filename: $GLOBALS['path']),
    'dynamic' => static fn () => ($GLOBALS['dynamic'])($GLOBALS['path'], true),
    'first' => static fn () => ($GLOBALS['first'])($GLOBALS['path'], true),
    'call-user' => static fn () => call_user_func('php_strip_whitespace', $GLOBALS['path']),
] as $label => $call) {
    $value = $call();
    echo $label, '=', strlen($value), ':', md5($value), "\n";
}
foreach (['highlight_string', 'highlight_file', 'show_source', 'php_strip_whitespace'] as $name) {
    $function = new ReflectionFunction($name);
    echo 'reflection=', $name, ':', $function->getName(), ':', $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
    foreach ($function->getParameters() as $parameter) {
        echo 'param=', $parameter->getName(), ':', $parameter->getType(), ':', $parameter->isOptional() ? 'optional' : 'required', ':';
        echo $parameter->isDefaultValueAvailable() ? var_export($parameter->getDefaultValue(), true) : '-', "\n";
    }
    $reflectionText = (string) $function;
    echo 'reflection-text=', strlen($reflectionText), ':', md5($reflectionText), "\n";
}
var_dump(highlight_file('/rphp/definitely/missing/source-filter.php', true));
var_dump(show_source('/rphp/definitely/missing/source-filter.php', true));
var_dump(php_strip_whitespace('/rphp/definitely/missing/source-filter.php'));
unlink($path);
restore_error_handler();
"#,
        ),
        r#"highlight_file=string:710:f210e33784f4742121f577d17be3ccc9
show_source=string:710:f210e33784f4742121f577d17be3ccc9
php_strip_whitespace=string:75:4d79c41b042ad3cd97f8f02278fee956
print=bool:true:710:f210e33784f4742121f577d17be3ccc9
named=710:f210e33784f4742121f577d17be3ccc9
dynamic=710:f210e33784f4742121f577d17be3ccc9
first=710:f210e33784f4742121f577d17be3ccc9
call-user=75:4d79c41b042ad3cd97f8f02278fee956
reflection=highlight_string:highlight_string:1/2:string|true
param=string:string:required:-
param=return:bool:optional:false
reflection-text=216:75a3e1c016b70445164e45957c2e49c1
reflection=highlight_file:highlight_file:1/2:string|bool
param=filename:string:required:-
param=return:bool:optional:false
reflection-text=216:25d0b5ef21a8094cdefcc67a9771da2f
reflection=show_source:show_source:1/2:string|bool
param=filename:string:required:-
param=return:bool:optional:false
reflection-text=213:0a34199cca81df9b1ad0f5ed783de1dd
reflection=php_strip_whitespace:php_strip_whitespace:1/1:string
param=filename:string:required:-
reflection-text=164:7a5e1777c292f706687b2217437b1530
diag=2:highlight_file(/rphp/definitely/missing/source-filter.php): Failed to open stream: No such file or directory
diag=2:highlight_file(): Failed opening '/rphp/definitely/missing/source-filter.php' for highlighting
bool(false)
diag=2:show_source(/rphp/definitely/missing/source-filter.php): Failed to open stream: No such file or directory
diag=2:show_source(): Failed opening '/rphp/definitely/missing/source-filter.php' for highlighting
bool(false)
diag=2:php_strip_whitespace(/rphp/definitely/missing/source-filter.php): Failed to open stream: No such file or directory
string(0) ""
"#,
    );
}

#[test]
fn source_filter_strict_types_reject_nonmatching_scalars() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
function attempt(string $label, callable $call): void {
    echo $label, '=';
    try { $value = $call(); echo get_debug_type($value), ':', is_string($value) ? strlen($value) : (string) $value; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(); }
    echo "\n";
}
attempt('string/int', static fn () => highlight_string(123, true));
attempt('string/return-int', static fn () => highlight_string('<?php', 1));
attempt('file/int', static fn () => highlight_file(123, true));
attempt('file/return-int', static fn () => highlight_file(__FILE__, 1));
attempt('show/int', static fn () => show_source(123, true));
attempt('strip/int', static fn () => php_strip_whitespace(123));
"#,
        ),
        r#"string/int=TypeError:highlight_string(): Argument #1 ($string) must be of type string, int given
string/return-int=TypeError:highlight_string(): Argument #2 ($return) must be of type bool, int given
file/int=TypeError:highlight_file(): Argument #1 ($filename) must be of type string, int given
file/return-int=TypeError:highlight_file(): Argument #2 ($return) must be of type bool, int given
show/int=TypeError:show_source(): Argument #1 ($filename) must be of type string, int given
strip/int=TypeError:php_strip_whitespace(): Argument #1 ($filename) must be of type string, int given
"#,
    );
}

#[test]
fn source_filter_rejects_output_handler_reentry_at_the_physical_callsite() {
    let error = run_php_expect_error_with_source_context(
        r#"<?php
ob_start(function (string $buffer): string {
    highlight_string('<?php echo 1;', true);
    return $buffer;
});
echo 'x';
ob_end_flush();
"#,
        "/tmp/rphp-source-filter-reentry.php",
        "/tmp",
    );
    assert_eq!(
        error.to_string(),
        "highlight_string(): Cannot use output buffering in output buffering display handlers in /tmp/rphp-source-filter-reentry.php on line 3"
    );
}
