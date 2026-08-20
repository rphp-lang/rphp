mod common;
use common::{run_php, run_php_expect_error_with_source_context};

// ─── Basic namespace ──────────────────────────────────────────────

#[test]
fn namespace_basic_function() {
    let out = run_php(
        r#"<?php
namespace App\Utils;

function greet() {
    echo "Hello from App\\Utils\n";
}

greet();
"#,
    );
    assert_eq!(out, "Hello from App\\Utils\n");
}

#[test]
fn namespace_basic_class() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}

$u = new User("Alice");
echo $u->name;
"#,
    );
    assert_eq!(out, "Alice");
}

// ─── Use declaration ──────────────────────────────────────────────

#[test]
fn declarations_reject_same_kind_local_import_aliases() {
    for (source, expected) in [
        (
            "<?php\nnamespace Fixture;\nuse Vendor\\Thing as Shared;\ninterface sHARED {}",
            "Cannot redeclare class Fixture\\sHARED (previously declared as local import) in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nuse function Vendor\\helper as Shared;\nfunction sHARED() {}",
            "Cannot redeclare function Fixture\\sHARED() (previously declared as local import) in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nuse const Vendor\\VALUE as Shared;\nconst Shared = 42;",
            "Cannot declare const Fixture\\Shared because the name is already in use in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nuse const Vendor\\VALUE as Shared;\nconst First = 1,\n    Shared = 42;",
            "Cannot declare const Fixture\\Shared because the name is already in use in /virtual/import-collision.php on line 4",
        ),
    ] {
        assert_eq!(
            run_php_expect_error_with_source_context(
                source,
                "/virtual/import-collision.php",
                "/virtual",
            )
            .to_string(),
            expected
        );
    }
}

#[test]
fn imports_reject_prior_and_duplicate_same_kind_symbols() {
    for (source, expected) in [
        (
            "<?php\nnamespace Fixture;\nclass Shared {}\nuse Vendor\\Thing as Shared;",
            "Cannot use Vendor\\Thing as Shared because the name is already in use in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nif (false) { function Shared() {} }\nuse function Vendor\\helper as SHARED;",
            "Cannot use function Vendor\\helper as SHARED because the name is already in use in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nconst Shared = 42;\nuse const Vendor\\VALUE as Shared;",
            "Cannot use const Vendor\\VALUE as Shared because the name is already in use in /virtual/import-collision.php on line 4",
        ),
        (
            "<?php\nnamespace Fixture;\nuse function Vendor\\first as Shared, Vendor\\second as SHARED;",
            "Cannot use function Vendor\\second as SHARED because the name is already in use in /virtual/import-collision.php on line 3",
        ),
    ] {
        assert_eq!(
            run_php_expect_error_with_source_context(
                source,
                "/virtual/import-collision.php",
                "/virtual",
            )
            .to_string(),
            expected
        );
    }
}

#[test]
fn import_collision_namespaces_reset_and_remain_kind_sensitive() {
    let output = run_php(
        r#"<?php
namespace Fixture {
    class Shared {}
    function shared() {}
    const Shared = 1;
}
namespace Fixture {
    use Vendor\Thing as Shared;
    use function Vendor\helper as Shared;
    use const Vendor\VALUE as Shared;
}
namespace SelfImport {
    use selfimport\ImportedClass as IMPORTEDCLASS;
    class ImportedClass {}
    function DeclaredFunction() {}
    use function SelfImport\DeclaredFunction;
    use const SelfImport\ImportedConstant;
    const ImportedConstant = 1;
    class DeclaredClass {}
    use SelfImport\DeclaredClass;
    const DeclaredConstant = 2;
    use const SelfImport\DeclaredConstant;
}
namespace Control {
    use Vendor\Thing as Shared;
    use function Vendor\helper as Shared;
    use const Vendor\VALUE as Shared;
    const shared = 42;
    echo shared;
}
"#,
    );

    assert_eq!(output, "42");
}

#[test]
fn namespace_use_class() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}

namespace App\Controllers;

use App\Models\User;

$u = new User("Bob");
echo $u->name;
"#,
    );
    assert_eq!(out, "Bob");
}

#[test]
fn namespace_use_alias() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}

namespace App\Controllers;

use App\Models\User as U;

$u = new U("Charlie");
echo $u->name;
"#,
    );
    assert_eq!(out, "Charlie");
}

#[test]
fn namespace_function_imports_are_case_insensitive_and_separate_from_classes() {
    let output = run_php(
        r#"<?php
namespace Library;
function imported() { return "function"; }
class imported { public static function value() { return "class"; } }

namespace Application;
use Library\imported as Shared;
use function Library\imported as Shared;

echo Shared(), "|", SHARED(), "|", Shared::value();
"#,
    );

    assert_eq!(output, "function|function|class");
}

#[test]
fn class_import_does_not_rewrite_a_same_named_function_declaration() {
    let output = run_php(
        r#"<?php
namespace Library;
class Shared { public static function value() { return "class"; } }

namespace Application;
use Library\Shared;
function Shared() { return "function"; }

echo Shared(), "|", Shared::value();
"#,
    );

    assert_eq!(output, "function|class");
}

#[test]
fn namespace_function_imports_support_global_builtins_and_comma_aliases() {
    let output = run_php(
        r#"<?php
namespace Helpers;
function first() { return "first"; }
function second() { return "second"; }

namespace Application;
use function strlen as width;
use function Helpers\first, Helpers\second as renamed;

echo width("abc"), "|", first(), "|", renamed(), "|";
var_dump(function_exists("width"));
"#,
    );

    assert_eq!(output, "3|first|second|bool(false)\n");
}

#[test]
fn namespace_function_imports_are_preserved_in_methods_and_closures() {
    let output = run_php(
        r#"<?php
namespace Helpers;
function decorate($value) { return $value . "!"; }

namespace Application;
use function Helpers\decorate as finish;

class Runner {
    public static function run() { return FINISH("method"); }
}
$closure = static fn($value) => finish($value);
echo Runner::run(), "|", $closure("closure");
"#,
    );

    assert_eq!(output, "method!|closure!");
}

#[test]
fn missing_imported_function_does_not_fall_back_to_global_builtin() {
    let output = run_php(
        r#"<?php
namespace Application;
use function Missing\strlen as strlen;

try {
    strlen("abc");
} catch (\Error $error) {
    echo "caught";
}
"#,
    );

    assert_eq!(output, "caught");
}

// ─── Fully qualified names ────────────────────────────────────────

#[test]
fn namespace_fully_qualified() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}

namespace App\Controllers;

$u = new \App\Models\User("Dave");
echo $u->name;
"#,
    );
    assert_eq!(out, "Dave");
}

#[test]
fn namespace_and_use_aliases_are_preserved_inside_method_compilers() {
    let output = run_php(
        r#"<?php
namespace Library;
class Service {
    public static function value(): string { return "resolved"; }
}

namespace Application;
use Library\Service as Alias;
class Runner {
    public static function run(): string { return Alias::value(); }
}

echo Runner::run();
"#,
    );
    assert_eq!(output, "resolved");
}

// ─── Global function fallback ─────────────────────────────────────

#[test]
fn namespace_global_function_fallback() {
    let out = run_php(
        r#"<?php
namespace App\Utils;

echo strlen("hello");
"#,
    );
    assert_eq!(out, "5");
}

#[test]
fn namespace_echo_and_builtins() {
    let out = run_php(
        r#"<?php
namespace App;

$arr = [3, 1, 2];
echo count($arr);
"#,
    );
    assert_eq!(out, "3");
}

// ─── Braced namespace ─────────────────────────────────────────────

#[test]
fn namespace_braced() {
    let out = run_php(
        r#"<?php
namespace App\Models {
    class Product {
        public $title;
        public function __construct($t) {
            $this->title = $t;
        }
    }
}
namespace App\Controllers {
    use App\Models\Product;
    $p = new Product("Widget");
    echo $p->title;
}
"#,
    );
    assert_eq!(out, "Widget");
}

#[test]
fn bracketed_global_namespace_restores_global_resolution() {
    let out = run_php(
        r#"<?php
namespace Library {
    function value() { return "namespaced"; }
}
namespace {
    function value() { return "global"; }
    echo value(), "|", \Library\value(), "|", __NAMESPACE__;
}
"#,
    );
    assert_eq!(out, "global|namespaced|");
}

#[test]
fn constant_imports_are_case_sensitive_and_separate_from_other_imports() {
    let out = run_php(
        r#"<?php
namespace Library {
    const VALUE = 41;
    const value = 42;
}
namespace Application {
    const LOCAL = 1;
    use const Library\VALUE as IMPORTED;
    use const Library\value as imported;
    echo IMPORTED, "|", imported, "|", LOCAL, "|";
}
namespace {
    const FALLBACK = 43;
}
namespace Consumer {
    echo FALLBACK;
}
"#,
    );
    assert_eq!(out, "41|42|1|43");
}

#[test]
fn group_use_supports_mixed_kinds_aliases_compound_names_and_trailing_comma() {
    let out = run_php(
        r#"<?php
namespace Vendor\Package {
    class Item { public static function name() { return "item"; } }
    class NestedItem { public static function name() { return "nested"; } }
    function helper() { return "helper"; }
    const VALUE = 42;
}
namespace Vendor\Package\Nested {
    class Entry { public static function name() { return "entry"; } }
}
namespace Application {
    use Vendor\Package\{
        Item as Product,
        Nested\Entry,
        function helper as assist,
        const VALUE as ANSWER,
    };
    echo Product::name(), "|", Entry::name(), "|", assist(), "|", ANSWER;
}
"#,
    );
    assert_eq!(out, "item|entry|helper|42");
}

#[test]
fn group_use_rejects_empty_nested_and_kind_overrides() {
    for source in [
        "<?php use Vendor\\Package\\{};",
        "<?php use Vendor\\Package\\{Item\\{Nested}};",
        "<?php use function Vendor\\Package\\{helper, const VALUE};",
    ] {
        let tokens = rphp::lexer::Lexer::new(source).tokenize().unwrap();
        assert!(rphp::parser::Parser::new(tokens).parse().is_err());
    }
}

// ─── Namespace with interfaces/traits ─────────────────────────────

#[test]
fn namespace_with_trait() {
    let out = run_php(
        r#"<?php
namespace App\Traits;

trait Greet {
    public function hello() {
        echo "Hi!";
    }
}

namespace App\Models;

use App\Traits\Greet;

class User {
    use Greet;
}

$u = new User();
$u->hello();
"#,
    );
    assert_eq!(out, "Hi!");
}

#[test]
fn namespace_with_interface() {
    let out = run_php(
        r#"<?php
namespace App\Contracts;

interface Printable {
    public function display();
}

namespace App\Models;

use App\Contracts\Printable;

class Item implements Printable {
    public function display() {
        echo "Item displayed";
    }
}

$i = new Item();
$i->display();
"#,
    );
    assert_eq!(out, "Item displayed");
}

// ─── Multiple use declarations ────────────────────────────────────

#[test]
fn namespace_multiple_use() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public function name() { echo "User"; }
}

class Post {
    public function title() { echo "Post"; }
}

namespace App\Controllers;

use App\Models\User;
use App\Models\Post;

$u = new User();
$u->name();
$p = new Post();
$p->title();
"#,
    );
    assert_eq!(out, "UserPost");
}

// ─── Namespace with type hints ────────────────────────────────────

#[test]
fn namespace_class_type_hint() {
    let out = run_php(
        r#"<?php
namespace App\Models;

class User {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}

function greet_user(User $u) {
    echo "Hello, " . $u->name;
}

$u = new User("Eve");
greet_user($u);
"#,
    );
    assert_eq!(out, "Hello, Eve");
}

// ─── FQ name with wrong namespace prefix must NOT fall back to global ─────

#[test]
fn namespace_fq_wrong_prefix_no_fallback() {
    // \Nope\strlen("hi") must be a fatal error — FQ names don't get global fallback.
    // Only unqualified names in a namespace fall back to global.
    let out = run_php(
        r#"<?php
namespace App;
try {
    \Nope\strlen("hi");
    echo "SHOULD_NOT_REACH";
} catch (\Error $e) {
    echo "caught";
}
"#,
    );
    assert_eq!(out, "caught");
}

#[test]
fn namespace_qualified_name_no_fallback() {
    // B\foo() in namespace A resolves to A\B\foo() — no global fallback for qualified names.
    let out = run_php(
        r#"<?php
namespace App;
function strlen($s) { return "custom"; }
try {
    Nope\strlen("hi");
    echo "SHOULD_NOT_REACH";
} catch (\Error $e) {
    echo "caught";
}
"#,
    );
    assert_eq!(out, "caught");
}

// ─── Namespace fallback edge cases ─────────────────────────────────

#[test]
fn namespace_unqualified_global_fallback_works() {
    // Unqualified function in namespace SHOULD fall back to global
    let out = run_php(
        r#"<?php
namespace App;
echo strlen("hello");
"#,
    );
    assert_eq!(out, "5");
}

#[test]
fn namespace_unqualified_local_takes_priority() {
    // Local namespace function takes priority over global fallback
    let out = run_php(
        r#"<?php
namespace App;
function strlen($s) { return "custom:" . \strlen($s); }
echo strlen("hi");
"#,
    );
    assert_eq!(out, "custom:2");
}

#[test]
fn namespace_fq_global_function_works() {
    // \strlen() — fully qualified global function always works
    let out = run_php(
        r#"<?php
namespace App\Deep\Nested;
echo \strlen("test");
"#,
    );
    assert_eq!(out, "4");
}

#[test]
fn namespace_deeply_qualified_no_fallback() {
    // A\B\C\foo() in namespace X → resolves to X\A\B\C\foo(), no global fallback
    let out = run_php(
        r#"<?php
namespace X;
try {
    A\B\C\foo();
    echo "SHOULD_NOT_REACH";
} catch (\Error $e) {
    echo "caught";
}
"#,
    );
    assert_eq!(out, "caught");
}

#[test]
fn namespace_relative_names() {
    let out = run_php(
        r#"<?php
namespace Demo;
const VALUE = "constant";
function value() { return "function"; }
class Box { const VALUE = "class"; }
function accepts(namespace\Box $box) { return $box::VALUE; }
echo namespace\VALUE, "\n";
echo namespace\value(), "\n";
echo namespace\Box::VALUE, "\n";
echo namespace\accepts(new namespace\Box()), "\n";
"#,
    );
    assert_eq!(out, "constant\nfunction\nclass\nclass\n");
}

#[test]
fn namespace_relative_catch_types_resolve_with_unions_and_without_variables() {
    let out = run_php(
        r#"<?php
namespace CatchScope;
class FirstProblem extends \Exception {}
class LastProblem extends \Exception {}

try {
    throw new FirstProblem('first');
} catch (namespace\LastProblem|\RuntimeException|namespace\FirstProblem $error) {
    echo $error->getMessage(), "\n";
}

try {
    throw new LastProblem('last');
} catch (namespace\LastProblem) {
    echo "caught-without-variable\n";
}
"#,
    );
    assert_eq!(out, "first\ncaught-without-variable\n");
}
