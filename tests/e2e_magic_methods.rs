mod common;
use common::run_php;

#[test]
fn test_tostring_echo() {
    assert_eq!(
        run_php(
            r#"<?php
class Money {
    private int $cents;
    public function __construct(int $cents) {
        $this->cents = $cents;
    }
    public function __toString(): string {
        return "USD:" . $this->cents;
    }
}
$m = new Money(1550);
echo $m;
"#
        ),
        "USD:1550"
    );
}

#[test]
fn test_tostring_concat() {
    assert_eq!(
        run_php(
            r#"<?php
class Tag {
    private string $name;
    public function __construct(string $name) { $this->name = $name; }
    public function __toString(): string { return "<" . $this->name . ">"; }
}
$t = new Tag("div");
echo "HTML: " . $t;
"#
        ),
        "HTML: <div>"
    );
}

#[test]
fn test_get_set() {
    assert_eq!(
        run_php(
            r#"<?php
class Bag {
    private $data;
    public function __construct() {
        $this->data = [];
    }
    public function __get($name) {
        return $this->data[$name] ?? "none";
    }
    public function __set($name, $value) {
        $this->data[$name] = $value;
    }
}
$b = new Bag();
$b->color = "red";
echo $b->color;
"#
        ),
        "red"
    );
}

#[test]
fn test_get_undefined_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Flex {
    public function __get($name) {
        return "default_" . $name;
    }
}
$f = new Flex();
echo $f->whatever;
"#
        ),
        "default_whatever"
    );
}

#[test]
fn test_invoke() {
    assert_eq!(
        run_php(
            r#"<?php
class Multiplier {
    private int $factor;
    public function __construct(int $factor) { $this->factor = $factor; }
    public function __invoke(int $x): int { return $this->factor * $x; }
}
$double = new Multiplier(2);
echo $double(21);
"#
        ),
        "42"
    );
}

#[test]
fn test_invoke_with_closure_like_usage() {
    assert_eq!(
        run_php(
            r#"<?php
class Greeter {
    private string $greeting;
    public function __construct(string $greeting) { $this->greeting = $greeting; }
    public function __invoke(string $name): string { return $this->greeting . " " . $name; }
}
$hi = new Greeter("Hello");
echo $hi("World");
"#
        ),
        "Hello World"
    );
}
