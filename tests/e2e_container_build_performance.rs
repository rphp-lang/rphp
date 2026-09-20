mod common;

use common::run_php;

/// Shapes from PHPStan's cold Nette DI container build that were super-linear
/// under RPHP: releasing a foreach temporary over a shared property array
/// planned destructors for every element, and a lookbehind scanned back to the
/// subject start. The budgets are far above the fixed cost (tens of
/// milliseconds) and far below the regressed cost (many seconds).
#[test]
fn foreach_over_a_shared_property_array_releases_in_constant_time() {
    let started = std::time::Instant::now();
    let output = run_php(
        r#"<?php
class Definition { public $tags = []; public $creator; function __construct(public string $name) { $this->creator = ['entity' => $name, 'arguments' => [1, 2, 3, ['nested' => [4, 5]]]]; } }
class Builder {
    private array $definitions = [];
    function add(string $name): void {
        $lower = strtolower($name);
        foreach ($this->definitions as $existing => $definition) {
            if ($lower === strtolower($existing)) { throw new LogicException($name); }
        }
        $this->definitions[$name] = new Definition($name);
    }
    function count(): int { return count($this->definitions); }
}
$builder = new Builder();
for ($i = 0; $i < 1500; $i++) { $builder->add("phpstan.service.Name$i"); }
echo $builder->count(), "\n";
"#,
    );
    assert_eq!(output, "1500\n");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "quadratic release planning is back: {:?}",
        started.elapsed()
    );
}

#[test]
fn shared_container_release_still_runs_destructors_when_final() {
    assert_eq!(
        run_php(
            r#"<?php
class Tracked { function __construct(public string $id) {} function __destruct() { echo "destroy:$this->id|"; } }
class Holder { public array $items = []; }
$holder = new Holder();
$holder->items = ['a' => new Tracked('a'), 'b' => new Tracked('b')];
foreach ($holder->items as $key => $item) { echo "visit:$key|"; }
echo "after-loop|";
$holder->items = [];
echo "cleared|";
$local = [new Tracked('c')];
foreach ($local as $item) { echo "visit:c|"; }
unset($item);
$local = null;
echo "end\n";
"#,
        ),
        "visit:a|visit:b|after-loop|destroy:a|cleared|destroy:b|visit:c|destroy:c|end\n"
    );
}

#[test]
fn lookbehind_heavy_tokenizing_stays_linear() {
    let started = std::time::Instant::now();
    let output = run_php(
        r#"<?php
$input = str_repeat("services:\n\t-\n\t\tclass: PHPStan\\Rules\\Foo\\BarRule\n\t\ttags:\n\t\t\t- phpstan.rules.rule\n\t\targuments:\n\t\t\tcheck: %check%\n", 300);
$pattern = '~((?: [^#"\',:=[\]{}()\n\t\ `-] | (?<!["\']) [:-] [^"\',=[\]{}()\n\t\ ] )(?:[^,:=\]})(\n\t\ ]++ | :(?! [\n\t\ ,\]})] | $ ) | [\ \t]++ [^#,:=\]})(\n\t\ ])*+)|([,:=[\]{}()-])|(\#.*+)|(\n++)|([\t\ ]++)~Amixu';
$count = preg_match_all($pattern, $input, $tokens, PREG_SET_ORDER);
$offset = 0;
foreach ($tokens as $token) { $offset += strlen($token[0]); }
echo $count, ':', $offset === strlen($input) ? 'complete' : 'partial', "\n";
"#,
    );
    assert_eq!(output, "9300:complete\n");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "quadratic lookbehind is back: {:?}",
        started.elapsed()
    );
}
