mod common;

use common::run_php;

/// Runtime contracts that PHPStan's Nette DI container compiler exercises when
/// it builds a container from scratch (no PHP-generated cache): constructor
/// by-reference binding of property arguments, abstract methods in
/// `getMethods()`, the reflection accessors Nette's generator calls, on-demand
/// parameter default values, closure captures under `array_walk()` surplus
/// arguments and variadic all-`int` method calls. The expected output is what
/// reference PHP 8.5 prints for the same script.
#[test]
fn container_build_runtime_contracts_match_php() {
    assert_eq!(
        run_php(
            r##"<?php
namespace App;
// Constructor by-reference parameters bind property and static-property arguments.
class Holder { private array $configs = ['a' => 'x']; public static $shared = ['s' => 1]; public $arr = ['k' => 1];
  function bind() { $e = new Ext($this->configs); $e->set(); $this->configs['n'] = 2; return [$this->configs, $e->get()]; }
  function bindStatic() { $e = new Ext(self::$shared); $e->set(); return self::$shared; } }
class Ext { private $c; function __construct(&$c) { $this->c = &$c; } function set() { $this->c = is_array($this->c) ? ['r' => 1] + $this->c : 9; } function get() { return $this->c; } }
$h = new Holder; echo json_encode([$h->bind(), $h->bindStatic()]), "\n";
// Interface and abstract methods are listed by getMethods().
interface Loc { const K = 1; public function create(int $a, ?string $b = null): static; public static function s(); }
abstract class Base implements Loc { abstract function ab(); public function __construct(public readonly int $x = 5, protected array &$ref = [], string ...$rest) {} }
final class Impl extends Base { public static $st = 1; public ?Suit $suit = null; function ab() {} function create(int $a, ?string $b = \PHP_EOL, int|string $c = self::class, (Loc&\Countable)|null $d = null, int|null $e = 3): static { return $this; } private function gen(): \Generator { yield 1; } static function s() {} }
enum Suit: string { case H = 'h'; case S = 's'; }
foreach ([Loc::class, Base::class, Impl::class] as $c) { $rc = new \ReflectionClass($c); echo $rc->getShortName(), ': ', implode(',', array_map(fn($m) => $m->name . ($m->isAbstract() ? '!' : ''), $rc->getMethods())), ' | abstract=', implode(',', array_map(fn($m) => $m->name, $rc->getMethods(\ReflectionMethod::IS_ABSTRACT))), "\n"; }
// Reflection surface used by Nette DI's container compiler.
$calls = [
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'b'))->getPosition(),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'b'))->getDefaultValueConstantName(),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'c'))->getDefaultValueConstantName(),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'c'))->getDefaultValue(),
 fn() => (string) (new \ReflectionParameter([Impl::class, 'create'], 'c'))->getType(),
 fn() => array_map(fn($t) => $t->getName(), (new \ReflectionParameter([Impl::class, 'create'], 'c'))->getType()->getTypes()),
 fn() => (string) (new \ReflectionParameter([Impl::class, 'create'], 'd'))->getType(),
 fn() => get_class((new \ReflectionParameter([Impl::class, 'create'], 'd'))->getType()),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'd'))->getType()->allowsNull(),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'e'))->allowsNull(),
 fn() => (new \ReflectionParameter([Base::class, '__construct'], 'x'))->isPromoted(),
 fn() => (new \ReflectionParameter([Impl::class, 'create'], 'a'))->isPromoted(),
 fn() => (new \ReflectionParameter([Base::class, '__construct'], 'ref'))->canBePassedByValue(),
 fn() => (new \ReflectionParameter([Base::class, '__construct'], 'rest'))->canBePassedByValue(),
 fn() => (new \ReflectionMethod(Impl::class, 'gen'))->isGenerator(),
 fn() => (new \ReflectionMethod(Impl::class, 'create'))->isGenerator(),
 fn() => (new \ReflectionMethod(Impl::class, 'create'))->isVariadic(),
 fn() => (new \ReflectionMethod(Base::class, '__construct'))->isVariadic(),
 fn() => (new \ReflectionMethod(Impl::class, 'create'))->isInternal(),
 fn() => (new \ReflectionMethod(Impl::class, 'create'))->isUserDefined(),
 fn() => (new \ReflectionMethod(Impl::class, 'create'))->getClosureThis(),
 fn() => (new \ReflectionFunction('strlen'))->isInternal(),
 fn() => (new \ReflectionFunction('strlen'))->isUserDefined(),
 fn() => (new \ReflectionClass(Impl::class))->getShortName(),
 fn() => (new \ReflectionClass(Impl::class))->getNamespaceName(),
 fn() => (new \ReflectionClass(Impl::class))->inNamespace(),
 fn() => (new \ReflectionClass('stdClass'))->inNamespace(),
 fn() => (new \ReflectionClass(Impl::class))->isAnonymous(),
 fn() => (new \ReflectionClass(new class {}))->isAnonymous(),
 fn() => (new \ReflectionClass(Impl::class))->isCloneable(),
 fn() => (new \ReflectionClass(Base::class))->isCloneable(),
 fn() => (new \ReflectionClass(Impl::class))->isIterable(),
 fn() => (new \ReflectionClass(\ArrayObject::class))->isIterable(),
 fn() => (new \ReflectionClass(Impl::class))->getModifiers(),
 fn() => (new \ReflectionClass(Base::class))->getModifiers(),
 fn() => (new \ReflectionClass(Impl::class))->hasConstant('K'),
 fn() => (new \ReflectionClass(Impl::class))->hasConstant('NOPE'),
 fn() => (new \ReflectionClass(Suit::class))->hasConstant('H'),
 fn() => (new \ReflectionClass(Impl::class))->getStaticPropertyValue('st'),
 fn() => (new \ReflectionClass(Impl::class))->getStaticPropertyValue('nope', 'dflt'),
 fn() => (new \ReflectionClass(Impl::class))->getStaticProperties(),
 fn() => (new \ReflectionClass(Impl::class))->isInstance(new Impl()),
 fn() => (new \ReflectionClass(Loc::class))->isInstance(new Impl()),
 fn() => (new \ReflectionClass(Impl::class))->isInstance(new \stdClass()),
 fn() => (new \ReflectionClass(Impl::class))->isEnum(),
 fn() => (new \ReflectionClass(Suit::class))->isEnum(),
 fn() => (new \ReflectionClass(Impl::class))->getExtensionName(),
 fn() => (new \ReflectionClass(\ReflectionClass::class))->getExtensionName(),
 fn() => (new \ReflectionProperty(Impl::class, 'suit'))->isPromoted(),
 fn() => (new \ReflectionProperty(Impl::class, 'x'))->isPromoted(),
 fn() => (new \ReflectionProperty(Impl::class, 'suit'))->getDeclaringClass()->getName(),
 fn() => (new \ReflectionProperty(Impl::class, 'x'))->getDeclaringClass()->getName(),
 fn() => (new \ReflectionClassConstant(Impl::class, 'K'))->getValue(),
 fn() => (new \ReflectionClassConstant(Impl::class, 'K'))->isPublic(),
 fn() => (new \ReflectionClassConstant(Impl::class, 'K'))->getModifiers(),
 fn() => (new \ReflectionClassConstant(Impl::class, 'K'))->isEnumCase(),
 fn() => (new \ReflectionClassConstant(Suit::class, 'H'))->isEnumCase(),
];
foreach ($calls as $i => $call) { try { echo $i, ': ', json_encode($call()), "\n"; } catch (\Throwable $e) { echo $i, ': ', get_class($e), ': ', $e->getMessage(), "\n"; } }
// Default values of user parameters are evaluated on demand.
class D { const K = 'kk'; function __construct(bool $a = false, int $b = 0, ?string $c = null, array $d = [], string $e = '', float $f = 1.5, $g = \PHP_EOL, bool $h = true, $j = self::K, $k = ['x' => [1, 2]], $l = 1 + 2, $m = D::K . '!') {} }
foreach ((new \ReflectionMethod(D::class, '__construct'))->getParameters() as $p) echo $p->name, '=', json_encode($p->getDefaultValue()), ' ';
echo "\n";
// Closures with captures survive surplus callback arguments from array_walk.
class Walker { function run() { $d = []; $key = 'k'; $g = 'g'; $arr = [7]; array_walk($arr, function ($val) use ($key, &$d, $g): void { $d[$key] = [$val, $g, isset($this)]; }); array_walk_recursive($arr, function ($val) use (&$d): void { $d['r'] = $val; }); return $d; } }
echo json_encode((new Walker)->run()), "\n";
// Variadic all-int methods take the ordinary call path.
class Tokens { private array $tokens = [[1], [2]]; private int $index = 0;
  public function isCurrentTokenType(int ...$types): bool { return in_array($this->tokens[$this->index][0], $types, true); }
  public function run(): array { return [$this->isCurrentTokenType(1), $this->isCurrentTokenType(2, 1), $this->isCurrentTokenType()]; } }
echo json_encode((new Tokens)->run()), "\n";
"##
        ),
        r##"[[{"r":1,"a":"x","n":2},{"r":1,"a":"x","n":2}],{"r":1,"s":1}]
Loc: create!,s! | abstract=create,s
Base: ab!,__construct,create!,s! | abstract=ab,create,s
Impl: ab,create,gen,s,__construct | abstract=
0: 1
1: "PHP_EOL"
2: null
3: "App\\Impl"
4: "string|int"
5: ["string","int"]
6: "(App\\Loc&Countable)|null"
7: "ReflectionUnionType"
8: true
9: true
10: true
11: false
12: false
13: true
14: true
15: false
16: false
17: true
18: false
19: true
20: null
21: true
22: false
23: "Impl"
24: "App"
25: true
26: false
27: false
28: true
29: true
30: false
31: false
32: true
33: 32
34: 64
35: true
36: false
37: true
38: 1
39: "dflt"
40: {"st":1}
41: true
42: true
43: false
44: false
45: true
46: false
47: "Reflection"
48: false
49: true
50: "App\\Impl"
51: "App\\Base"
52: 1
53: true
54: 1
55: false
56: true
a=false b=0 c=null d=[] e="" f=1.5 g="\n" h=true j="kk" k={"x":[1,2]} l=3 m="kk!" 
{"k":[7,"g",true],"r":7}
[true,true,false]
"##
    );
}

#[test]
fn pcre_subroutine_calls_inline_completed_groups() {
    // Recursion into an open group and forward calls stay engine non-claims
    // (they compile to `false` without a warning); see `src/regex/tests.rs`.
    assert_eq!(
        run_php(
            r##"<?php
$re = <<<'XX'
~(
	\?? (?<type> \\? (?<name> [a-zA-Z_\x7f-\xff][\w\x7f-\xff]*) (\\ (?&name))* ) |
	(?<intersection> (?&type) (& (?&type))+ ) |
	(?<upart> (?&type) | \( (?&intersection) \) )  (\| (?&upart))+
)$~xAD
XX;
foreach (['null', 'int|string', 'Foo\\Bar', 'A&B', '(A&B)|null', '?int', 'bad type', '(A&B)|(C&D)|E'] as $t) echo $t, '=', (int) preg_match($re, $t), ' ';
echo "\n";
foreach ([['~(x)(?1)~', 'xx'], ['~(?<a>x)(?P>a)~', 'xx'], ['~(?<a>x)(?&a)~', 'xx'], ['~(?<a>x)\g<a>~', 'xx'], ["~(?<a>x)\\g'a'~", 'xx'], ['~(a)(b)(?-2)(?-1)~', 'abab'], ['~(?<a>x(?<b>y))(?&a)~', 'xyxy'], ['~(?<a>(?<b>q)|r)(?&a)~', 'rq'], ['~(a)\g{1}~', 'aa'], ['~(a)\g{-1}~', 'aa'], ['~(?<q>a)\g{q}~', 'aa'], ['~(a)\g1~', 'ab'], ['~(?<a>x)(?&nope)~', 'xx']] as [$re, $s]) { $r = @preg_match($re, $s, $m); echo json_encode([$r, $m ?? null]), ' '; }
echo "\n", json_encode(preg_replace('~(?<d>\d)(?&d)~', '#', 'a12b345c6')), json_encode(preg_split('~(?<s>[,;])(?&s)?~', 'a,,b;c')), "\n";
"##
        ),
        r##"null=1 int|string=1 Foo\Bar=1 A&B=1 (A&B)|null=1 ?int=1 bad type=0 (A&B)|(C&D)|E=1 
[1,["xx","x"]] [1,{"0":"xx","a":"x","1":"x"}] [1,{"0":"xx","a":"x","1":"x"}] [1,{"0":"xx","a":"x","1":"x"}] [1,{"0":"xx","a":"x","1":"x"}] [1,["abab","a","b"]] [1,{"0":"xyxy","a":"xy","1":"xy","b":"y","2":"y"}] [1,{"0":"rq","a":"r","1":"r"}] [1,["aa","a"]] [1,["aa","a"]] [1,{"0":"aa","q":"a","1":"a"}] [0,[]] [false,[]] 
"a#b#5c6"["a","b","c"]
"##
    );
}
