use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

fn assert_link_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("\nFatal error: {message} in Standard input code on line {line}\n")
    );
}

fn assert_eager_link_fatal(source: &str, message: &str, line: usize) {
    let (status, stdout, stderr) = run_stdin(source);
    assert_eq!(status, 255);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("Fatal error: {message} in Standard input code on line {line}\n")
    );
}

#[test]
fn direct_trait_method_collisions_preserve_source_order_and_spelling() {
    for (method, declarations) in [
        (
            "clash",
            "private function clash() {} private function spare() {}",
        ),
        ("__clone", "public function __clone() {}"),
        ("M1", "public function M1() {} public function M2() {}"),
    ] {
        let source = format!(
            "<?php\ntrait First {{ {declarations} }}\ntrait Second {{ {declarations} }}\nclass Consumer {{\n    use First, Second;\n}}\n"
        );
        assert_link_fatal(
            &source,
            &format!(
                "Trait method Second::{method} has not been applied as Consumer::{method}, because of collision with First::{method}"
            ),
            4,
        );
    }
}

#[test]
fn aliases_participate_at_their_source_trait_position() {
    assert_link_fatal(
        "<?php\ntrait Hello { public function hello() {} }\ntrait World { public function world() {} }\nclass Consumer {\n    use Hello, World { hello as world; }\n}\n",
        "Trait method World::world has not been applied as Consumer::world, because of collision with Hello::world",
        4,
    );
    assert_link_fatal(
        "<?php\ntrait Hello { public function hello() {} }\ntrait World { public function world() {} }\nclass Consumer {\n    use Hello, World { world as hello; }\n}\n",
        "Trait method World::world has not been applied as Consumer::hello, because of collision with Hello::hello",
        4,
    );
}

#[test]
fn adding_a_distinct_alias_does_not_hide_the_original_collision() {
    assert_link_fatal(
        "<?php\ntrait First { public function choose() {} }\ntrait Second { public function choose() {} }\nclass Consumer {\n    use First, Second { Second::choose as alternate; }\n}\n",
        "Trait method Second::choose has not been applied as Consumer::choose, because of collision with First::choose",
        4,
    );
}

#[test]
fn an_ambiguous_trait_composition_is_rejected_before_it_can_be_reused() {
    assert_eager_link_fatal(
        "<?php\ntrait First { public function choose() {} }\ntrait Second { public function choose() {} }\ntrait Ambiguous {\n    use First, Second;\n}\n",
        "Trait method Second::choose has not been applied as Ambiguous::choose, because of collision with First::choose",
        4,
    );
}

#[test]
fn resolved_abstract_duplicate_and_same_origin_compositions_remain_valid() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Preferred { public function choose() { return 'preferred'; } }
trait Other { public function choose() { return 'other'; } }
class Resolved {
    use Preferred, Other {
        Preferred::choose insteadof Other;
        Other::choose as alternate;
    }
}
trait Requirement { abstract public function required(); }
trait Implementation { public function required() { return 'implemented'; } }
class AbstractResolved { use Requirement, Implementation; }
trait Origin { public function shared() { static $calls = 0; return ++$calls; } }
trait Nested { use Origin; }
class Diamond { use Origin, Nested; }
trait Duplicate { public function duplicate() { return 'duplicate'; } }
class Repeated { use Duplicate, Duplicate; }
trait Left { public function owned() { return 'left'; } }
trait Right { public function owned() { return 'right'; } }
class OwnWins { use Left, Right; public function owned() { return 'own'; } }
trait FinalModifiers {
    public function firstFinal() { return 'first'; }
    public function secondFinal() { return 'second'; }
}
class FinalModifierConsumer {
    use FinalModifiers {
        firstFinal as final;
        secondFinal as final;
    }
}
$resolved = new Resolved();
$diamond = new Diamond();
echo $resolved->choose(), '|', $resolved->alternate(), '|';
echo (new AbstractResolved())->required(), '|';
echo $diamond->shared(), ',', $diamond->shared(), '|';
echo (new Repeated())->duplicate(), '|', (new OwnWins())->owned(), '|';
$final = new FinalModifierConsumer();
echo $final->firstFinal(), ',', $final->secondFinal();
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(
        stdout,
        "preferred|other|implemented|1,2|duplicate|own|first,second"
    );
    assert_eq!(stderr, "");
}
