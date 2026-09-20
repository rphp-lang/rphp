mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn static_trait_methods_resolve_self_and_abstract_walks_stop_at_overrides() {
    assert_eq!(
        run_php(
            r##"<?php
trait T {
    private static function imp(self $ref, array $cache): string { return get_class($ref) . count($cache); }
    public static function make(): static { return self::imp(new static(), [1]) === 'x' ? new static() : new static(); }
    public static function ident(self $r): self { return $r; }
    public static function scopes(): string { return self::class . '|' . static::class . '|' . __CLASS__; }
    public function same(self $other): bool { return $other instanceof static; }
}
class M { use T; }
class N extends M {}
echo get_class(M::make()), ',', get_class(N::make()), ',', get_class(N::ident(new N)), ',', get_class(M::ident(new N)), ',', (int) (new N)->same(new N), '|';
echo M::scopes(), ',', N::scopes(), '|';
try { M::ident(new stdClass); } catch (TypeError $e) { echo strstr($e->getMessage(), ', called', true), '|'; }
try { (new M)->same(new stdClass); } catch (TypeError $e) { echo strstr($e->getMessage(), ', called', true), '|'; }
abstract class Base { abstract protected function doWrite(string $m): string; public function write(string $m): string { return $this->doWrite($m); } }
class Stream extends Base { protected function doWrite(string $m): string { return "S:$m"; } }
class Section extends Stream { protected function doWrite(string $m): string { return 'C:' . parent::doWrite($m); } }
echo (new Section)->write('x'), '|';
class R { public function all(int $filter = 0): array { return [$filter]; } public function go(): string { return json_encode([$this->all(), $this->all(5)]); } }
echo (new R)->go(), "\n";
"##,
        ),
        r##"M,N,N,N,1|M|M|M,M|N|M|M::ident(): Argument #1 ($r) must be of type M, stdClass given|M::same(): Argument #1 ($other) must be of type M, stdClass given|C:S:x|[[0],[5]]
"##
    );
}

#[test]
fn array_column_reads_object_columns_through_the_calling_scope() {
    assert_eq!(
        run_php(
            r##"<?php
class L {
    private function __construct(private int $value, protected int $p = 2, public int $q = 3) {}
    public static function mk(int $v): self { return new self($v); }
    public static function inside(self ...$ops): array { return [array_column($ops, 'value'), array_column($ops, 'q', 'value')]; }
}
class W extends L { public static function sub(array $ops): array { return [array_column($ops, 'value'), array_column($ops, 'p')]; } }
$ops = [L::mk(1), L::mk(0), L::mk(-1)];
echo json_encode([L::inside(...$ops), array_column($ops, 'value'), array_column($ops, 'q'), W::sub($ops)]), "\n";
"##,
        ),
        r##"[[[1,0,-1],{"1":3,"0":3,"-1":3}],[],[3,3,3],[[],[2,2,2]]]
"##
    );
}

#[test]
fn reflection_reports_class_lines_doc_comments_modifier_constants_and_open_type_classes() {
    assert_eq!(
        run_php_with_source_context(
            r##"<?php

/** Doc for A */
class A
{
    public function m(
        int $x
    ): int {
        return $x;
    }
}
enum E { case X; }
interface I {}
trait T {
}
$r = new ReflectionClass('A');
echo $r->getStartLine(), ',', $r->getEndLine(), ',', (new ReflectionClass('E'))->getStartLine(), ',', (new ReflectionClass('I'))->getEndLine(), ',', (new ReflectionClass('T'))->getEndLine(), ',', var_export((new ReflectionClass('stdClass'))->getStartLine(), true), ',', (new ReflectionObject(new A))->getEndLine(), ',', (new ReflectionEnum('E'))->getEndLine(), '|';
$anon = new ReflectionClass(new class {
});
echo $anon->getStartLine(), ',', $anon->getEndLine(), '|', var_export($r->getDocComment(), true), ',', var_export((new ReflectionClass('E'))->getDocComment(), true), '|';
echo ReflectionClass::IS_FINAL, ',', ReflectionClass::IS_READONLY, ',', ReflectionClass::IS_EXPLICIT_ABSTRACT, ',', ReflectionProperty::IS_PROTECTED_SET, ',', ReflectionClassConstant::IS_FINAL, ',', ReflectionFunction::IS_DEPRECATED, '|';
echo (int) (new ReflectionClass('ReflectionNamedType'))->isFinal(), (int) (new ReflectionClass('ReflectionUnionType'))->isFinal(), (int) (new ReflectionClass('ReflectionIntersectionType'))->isFinal(), '|';
class NT extends ReflectionNamedType {
    public function __construct(private string $n) {}
    public function getName(): string { return $this->n; }
    public function allowsNull(): bool { return true; }
    public function isBuiltin(): bool { return false; }
    public function __toString(): string { return '?' . $this->n; }
}
$n = new NT('Foo');
echo $n, ',', (int) ($n instanceof ReflectionType), "\n";
"##,
            "reflection.php",
            "/srv",
        ),
        r##"4,11,12,13,15,false,11,12|18,19|'/** Doc for A */',false|32,65536,64,2048,32,2048|000|?Foo,1
"##
    );
}

#[test]
fn globals_probes_source_unpacking_and_runtime_declarations_follow_php() {
    assert_eq!(
        run_php(
            r##"<?php
$a = ['k' => 1];
function probe(): array { return [$GLOBALS['a'] ?? 'none', isset($GLOBALS['a']['k']), isset($GLOBALS['zz']['k']), $GLOBALS['zz'] ?? 'dflt']; }
echo json_encode(probe()), '|';
class Checker { public static function two(int $a, int $b): int { return $a + $b; } }
class Maker { public static function make(...$t): array { return $t; } public static function via(array $t): array { return self::make(1, ...$t); } public static function forward(array $t): array { return static::make(...$t); } }
function w(array $t): array { return Maker::make(...$t); }
$args = [2, 3];
echo Checker::two(...$args), ',', json_encode(Maker::via($args)), ',', json_encode(Maker::forward($args)), ',', json_encode(w($args)), ',', json_encode($args), '|';
spl_autoload_register(function ($c) { if ($c === 'Late') { eval('class Late { public static function sum(...$n) { return array_sum($n); } }'); } });
$callable = 'Late::sum';
echo $callable(...$args), ',', call_user_func_array('Late::sum', [1, 2]), '|';
class Str { public function __toString(): string { return 'str'; } }
echo new Str, ',', json_encode(array_keys($_SERVER) !== []), ',', isset($argv), isset($argc), ',', gettype($_ENV), gettype($_REQUEST), "\n";
"##,
        ),
        r##"[{"k":1},true,false,"dflt"]|5,[1,2,3],[2,3],[2,3],[2,3]|5,3|str,true,11,arrayarray
"##
    );
}
