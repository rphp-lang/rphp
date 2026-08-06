mod common;
use common::run_php;

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
