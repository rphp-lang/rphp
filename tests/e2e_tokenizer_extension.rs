mod common;

use common::run_php;

#[test]
fn tokenizer_publishes_extension_identity_constants_and_reflection() {
    assert_eq!(
        run_php(
            r##"<?php
echo (int) extension_loaded('tokenizer'), (int) extension_loaded('TOKENIZER'), (int) in_array('tokenizer', get_loaded_extensions(), true), '|';
echo (int) defined('T_FN'), (int) defined('T_NOPE'), '|', T_PAAMAYIM_NEKUDOTAYIM === T_DOUBLE_COLON ? 'alias' : 'distinct', '|';
echo T_LNUMBER, ',', T_STRING, ',', T_ATTRIBUTE, ',', T_PIPE, ',', T_BAD_CHARACTER, ',', TOKEN_PARSE, '|';
echo token_name(T_YIELD_FROM), ',', token_name(T_DOUBLE_COLON), ',', token_name(59), ',', token_name(-1), ',', token_name(1000), '|';
$constants = get_defined_constants(true)['tokenizer'];
echo count($constants), ',', array_key_first($constants), ',', array_key_last($constants), '|';
echo (new ReflectionFunction('token_get_all'))->getExtensionName(), ',', (new ReflectionFunction('token_name'))->getNumberOfRequiredParameters(), '|';
$class = new ReflectionClass('PhpToken');
echo implode(',', array_map(fn ($m) => $m->getName() . ($m->isStatic() ? '*' : '') . ($m->isFinal() ? '!' : ''), $class->getMethods())), '|';
echo implode(',', array_map(fn ($p) => $p->getType() . ' $' . $p->getName(), $class->getProperties())), '|';
echo (int) $class->implementsInterface('Stringable'), (int) $class->isFinal(), (int) $class->isInternal(), "\n";
"##,
        ),
        r##"111|10|alias|260,262,355,408,411,1|T_YIELD_FROM,T_DOUBLE_COLON,UNKNOWN,UNKNOWN,UNKNOWN|154,T_LNUMBER,TOKEN_PARSE|tokenizer,1|tokenize*,__construct!,is,isIgnorable,getTokenName,__toString|int $id,string $text,int $line,int $pos|101
"##
    );
}

#[test]
fn token_get_all_reproduces_php_token_shapes_lines_and_source() {
    assert_eq!(
        run_php(
            r##"<?php
$source = <<<'SRC'
head
<?php
#[Attr]
function f(int $a = 0x1F): ?string {
    return "v=$a {$a[0]} ${b}" . <<<EOT
      x $a->y
      EOT . (int) $c?->d . PHP_EOL;
}
?>
tail <?= 1 ?>
SRC;
foreach (token_get_all($source) as $token) {
    if (is_string($token)) {
        echo $token, ' ';
    } else {
        echo token_name($token[0]), ':', $token[2], ':', json_encode($token[1]), ' ';
    }
}
echo "\n";
$rebuilt = implode('', array_map(fn ($t) => is_array($t) ? $t[1] : $t, token_get_all($source)));
var_dump($rebuilt === $source, token_get_all(''), token_get_all('no php here'), count(token_get_all('<?php')));
"##,
        ),
        r##"T_INLINE_HTML:1:"head\n" T_OPEN_TAG:2:"<?php\n" T_ATTRIBUTE:3:"#[" T_STRING:3:"Attr" ] T_WHITESPACE:3:"\n" T_FUNCTION:4:"function" T_WHITESPACE:4:" " T_STRING:4:"f" ( T_STRING:4:"int" T_WHITESPACE:4:" " T_VARIABLE:4:"$a" T_WHITESPACE:4:" " = T_WHITESPACE:4:" " T_LNUMBER:4:"0x1F" ) : T_WHITESPACE:4:" " ? T_STRING:4:"string" T_WHITESPACE:4:" " { T_WHITESPACE:4:"\n    " T_RETURN:5:"return" T_WHITESPACE:5:" " " T_ENCAPSED_AND_WHITESPACE:5:"v=" T_VARIABLE:5:"$a" T_ENCAPSED_AND_WHITESPACE:5:" " T_CURLY_OPEN:5:"{" T_VARIABLE:5:"$a" [ T_LNUMBER:5:"0" ] } T_ENCAPSED_AND_WHITESPACE:5:" " T_DOLLAR_OPEN_CURLY_BRACES:5:"${" T_STRING_VARNAME:5:"b" } " T_WHITESPACE:5:" " . T_WHITESPACE:5:" " T_START_HEREDOC:5:"<<<EOT\n" T_ENCAPSED_AND_WHITESPACE:6:"      x " T_VARIABLE:6:"$a" T_OBJECT_OPERATOR:6:"->" T_STRING:6:"y" T_ENCAPSED_AND_WHITESPACE:6:"\n" T_END_HEREDOC:7:"      EOT" T_WHITESPACE:7:" " . T_WHITESPACE:7:" " T_INT_CAST:7:"(int)" T_WHITESPACE:7:" " T_VARIABLE:7:"$c" T_NULLSAFE_OBJECT_OPERATOR:7:"?->" T_STRING:7:"d" T_WHITESPACE:7:" " . T_WHITESPACE:7:" " T_STRING:7:"PHP_EOL" ; T_WHITESPACE:7:"\n" } T_WHITESPACE:8:"\n" T_CLOSE_TAG:9:"?>\n" T_INLINE_HTML:10:"tail " T_OPEN_TAG_WITH_ECHO:10:"<?=" T_WHITESPACE:10:" " T_LNUMBER:10:"1" T_WHITESPACE:10:" " T_CLOSE_TAG:10:"?>" 
bool(true)
array(0) {
}
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(267)
    [1]=>
    string(11) "no php here"
    [2]=>
    int(1)
  }
}
int(1)
"##
    );
}

#[test]
fn php_token_objects_follow_php_construction_matching_and_errors() {
    assert_eq!(
        run_php(
            r##"<?php
class Tok extends PhpToken {
    public int $extra = 7;
    public function summary(): string { return $this->getTokenName() . '@' . $this->line . ':' . $this->pos; }
}
$tokens = Tok::tokenize("<?php\n/** doc */ echo 'x';");
foreach ($tokens as $t) {
    echo get_class($t), ' ', $t->summary(), ' ', (int) $t->isIgnorable(), ' ', json_encode((string) $t), ' ', $t->extra, "\n";
}
$echo = $tokens[3];
var_dump($echo->is(T_ECHO), $echo->is('echo'), $echo->is([T_STRING, 'echo']), $echo->is([T_STRING, 'print']), $echo->is(T_PRINT));
foreach ([1.5, [1.5], null] as $bad) {
    try { $echo->is($bad); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
$plain = new PhpToken(T_STRING, 'name');
var_dump($plain);
$plain = new PhpToken(ord(';'), ';', 4);
echo $plain->getTokenName(), ' ', $plain->line, ' ', $plain->pos, ' ', var_export((new PhpToken(9999, 'x'))->getTokenName(), true), "\n";
unset($plain->text);
try { echo (string) $plain; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { $plain->is('x'); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
echo $plain->is(ord(';')) ? 'id-only' : 'no', "\n";
try { new PhpToken(1); } catch (ArgumentCountError $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { new PhpToken('x', 'y'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { new PhpToken(1, []); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { PhpToken::tokenize(new stdClass); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { token_get_all('<?php', 'flags'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { token_name([]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$coerced = new PhpToken('12', 34, '5', 6.0);
var_dump($coerced->id, $coerced->text, $coerced->line, $coerced->pos);
var_dump(PhpToken::tokenize(''), count(PhpToken::tokenize('<?php ')));
"##,
        ),
        r##"Tok T_OPEN_TAG@1:0 1 "<?php\n" 7
Tok T_DOC_COMMENT@2:6 1 "\/** doc *\/" 7
Tok T_WHITESPACE@2:16 1 " " 7
Tok T_ECHO@2:17 0 "echo" 7
Tok T_WHITESPACE@2:21 1 " " 7
Tok T_CONSTANT_ENCAPSED_STRING@2:22 0 "'x'" 7
Tok ;@2:25 0 ";" 7
bool(true)
bool(true)
bool(true)
bool(false)
bool(false)
PhpToken::is(): Argument #1 ($kind) must be of type string|int|array, float given
PhpToken::is(): Argument #1 ($kind) must only have elements of type string|int, float given
PhpToken::is(): Argument #1 ($kind) must be of type string|int|array, null given
object(PhpToken)#9 (4) {
  ["id"]=>
  int(262)
  ["text"]=>
  string(4) "name"
  ["line"]=>
  int(-1)
  ["pos"]=>
  int(-1)
}
; 4 -1 NULL
Error: Typed property PhpToken::$text must not be accessed before initialization
Error: Typed property PhpToken::$text must not be accessed before initialization
id-only
ArgumentCountError: PhpToken::__construct() expects at least 2 arguments, 1 given
PhpToken::__construct(): Argument #1 ($id) must be of type int, string given
PhpToken::__construct(): Argument #2 ($text) must be of type string, array given
PhpToken::tokenize(): Argument #1 ($code) must be of type string, stdClass given
token_get_all(): Argument #2 ($flags) must be of type int, string given
token_name(): Argument #1 ($id) must be of type int, array given
int(12)
string(2) "34"
int(5)
int(6)
array(0) {
}
int(1)
"##
    );
}

#[test]
fn token_parse_flag_admits_contextual_identifiers_and_reports_parse_errors() {
    assert_eq!(
        run_php(
            r##"<?php
$source = '<?php class A { function list() {} const NEW = 1; } A::class; A::new; f(list: 1); enum E { case Foo; case list; }';
$plain = array_map(fn ($t) => is_array($t) ? token_name($t[0]) . ':' . $t[1] : $t, array_filter(token_get_all($source), fn ($t) => !is_array($t) || $t[0] !== T_WHITESPACE));
$parsed = array_map(fn ($t) => $t->getTokenName() . ':' . $t->text, array_filter(PhpToken::tokenize($source, TOKEN_PARSE), fn ($t) => !$t->is(T_WHITESPACE)));
echo implode(' ', $plain), "\n", implode(' ', $parsed), "\n";
foreach (['<?php $x = ;', '<?php /* open', '<?php "$a[b c]"'] as $broken) {
    echo count(token_get_all($broken)), ' ';
    try { token_get_all($broken, TOKEN_PARSE); echo "no error\n"; } catch (ParseError $e) { echo get_class($e), "\n"; }
}
"##,
        ),
        r##"T_OPEN_TAG:<?php  T_CLASS:class T_STRING:A { T_FUNCTION:function T_LIST:list ( ) { } T_CONST:const T_NEW:NEW = T_LNUMBER:1 ; } T_STRING:A T_DOUBLE_COLON::: T_CLASS:class ; T_STRING:A T_DOUBLE_COLON::: T_NEW:new ; T_STRING:f ( T_LIST:list : T_LNUMBER:1 ) ; T_ENUM:enum T_STRING:E { T_CASE:case T_STRING:Foo ; T_CASE:case T_LIST:list ; }
T_OPEN_TAG:<?php  T_CLASS:class T_STRING:A {:{ T_FUNCTION:function T_STRING:list (:( ):) {:{ }:} T_CONST:const T_STRING:NEW =:= T_LNUMBER:1 ;:; }:} T_STRING:A T_DOUBLE_COLON::: T_STRING:class ;:; T_STRING:A T_DOUBLE_COLON::: T_STRING:new ;:; T_STRING:f (:( T_STRING:list ::: T_LNUMBER:1 ):) ;:; T_ENUM:enum T_STRING:E {:{ T_CASE:case T_STRING:Foo ;:; T_CASE:case T_STRING:list ;:; }:}
6 ParseError
2 ParseError
8 ParseError
"##
    );
}

#[test]
fn tokenizer_is_binary_safe_and_counts_every_newline_style() {
    assert_eq!(
        run_php(
            r##"<?php
$tokens = token_get_all("<?php \xff\xfe = '\xc3\xa9'; \$\xff;");
foreach ($tokens as $t) {
    if (is_array($t)) echo token_name($t[0]), ':', strlen($t[1]), ':', bin2hex($t[1]), ' ';
}
echo "\n";
foreach (PhpToken::tokenize("<?php \"\xff\"; b\"\$x\";") as $t) {
    echo $t->getTokenName() === null ? 'null' : bin2hex($t->getTokenName()), ':', bin2hex($t->text), ':', $t->pos, ' ';
}
echo "\n";
echo implode(' ', array_map(fn ($t) => is_array($t) ? token_name($t[0]) . ':' . $t[2] : $t, token_get_all("<?php\r\n\$a\r=\n1;\r\n// c\r?>\r\nx"))), "\n";
"##,
        ),
        r##"T_OPEN_TAG:6:3c3f70687020 T_STRING:2:fffe T_WHITESPACE:1:20 T_WHITESPACE:1:20 T_CONSTANT_ENCAPSED_STRING:4:27c3a927 T_WHITESPACE:1:20 T_VARIABLE:2:24ff 
545f4f50454e5f544147:3c3f70687020:0 545f434f4e5354414e545f454e4341505345445f535452494e47:22ff22:6 3b:3b:9 545f57484954455350414345:20:10 22:6222:11 545f5641524941424c45:2478:13 22:22:15 3b:3b:16 
T_OPEN_TAG:1 T_VARIABLE:2 T_WHITESPACE:2 = T_WHITESPACE:3 T_LNUMBER:4 ; T_WHITESPACE:4 T_COMMENT:5 T_WHITESPACE:5 T_CLOSE_TAG:6 T_INLINE_HTML:7
"##
    );
}
