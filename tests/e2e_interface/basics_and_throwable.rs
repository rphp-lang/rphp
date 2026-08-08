// ── Interface basics ──

#[test]
fn test_interface_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
interface Greetable {
    public function greet();
}
class Person implements Greetable {
    public function greet() {
        echo "Hello!";
    }
}
$p = new Person();
$p->greet();
"#
        ),
        "Hello!"
    );
}

#[test]
fn test_interface_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
interface Printable {
    public function display();
}
class Document implements Printable {
    public function display() {
        echo "doc";
    }
}
$d = new Document();
echo $d instanceof Printable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_interface_instanceof_negative() {
    assert_eq!(
        run_php(
            r#"<?php
interface Printable {
    public function display();
}
interface Editable {
    public function edit();
}
class Document implements Printable {
    public function display() {
        echo "doc";
    }
}
$d = new Document();
echo $d instanceof Editable ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_multiple_implements() {
    assert_eq!(
        run_php(
            r#"<?php
interface Readable {
    public function read();
}
interface Writable {
    public function write();
}
class File implements Readable, Writable {
    public function read() { echo "R"; }
    public function write() { echo "W"; }
}
$f = new File();
$f->read();
$f->write();
echo " ";
echo ($f instanceof Readable ? "r" : "n") . ($f instanceof Writable ? "w" : "n");
"#
        ),
        "RW rw"
    );
}

#[test]
fn test_class_extends_and_implements() {
    assert_eq!(
        run_php(
            r#"<?php
interface Describable {
    public function describe();
}
class Animal {
    public function type() { echo "animal"; }
}
class Dog extends Animal implements Describable {
    public function describe() { echo "woof"; }
}
$d = new Dog();
$d->describe();
echo " ";
echo ($d instanceof Animal ? "a" : "n") . ($d instanceof Describable ? "d" : "n");
"#
        ),
        "woof ad"
    );
}

// ── Throwable as interface (PHP 8 compatible) ──

#[test]
fn test_throwable_is_interface() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("test");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "test"
    );
}

#[test]
fn test_error_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Error("oops");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "oops"
    );
}

#[test]
fn test_typeerror_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new TypeError("type fail");
} catch (Throwable $e) {
    echo $e->getMessage();
}
"#
        ),
        "type fail"
    );
}

#[test]
fn test_exception_not_instanceof_error() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception("x");
echo $e instanceof Error ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_error_not_instanceof_exception() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Error("x");
echo $e instanceof Exception ? "yes" : "no";
"#
        ),
        "no"
    );
}

#[test]
fn test_exception_instanceof_throwable() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Exception("x");
echo $e instanceof Throwable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_error_instanceof_throwable_via_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new Error("x");
echo $e instanceof Throwable ? "yes" : "no";
"#
        ),
        "yes"
    );
}

#[test]
fn test_typeerror_instanceof_throwable_via_instanceof() {
    assert_eq!(
        run_php(
            r#"<?php
$e = new TypeError("x");
echo ($e instanceof Throwable ? "T" : "n") . ($e instanceof Error ? "E" : "n");
"#
        ),
        "TE"
    );
}
