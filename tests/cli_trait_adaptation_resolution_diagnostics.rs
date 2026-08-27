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

#[test]
fn missing_alias_sources_distinguish_aliases_modifiers_and_absolute_owners() {
    assert_link_fatal(
        "<?php\ntrait Source { function present() {} }\nclass Consumer { use Source { missing as renamed; } }\n",
        "An alias (renamed) was defined for method missing(), but this method does not exist",
        3,
    );
    assert_link_fatal(
        "<?php\ntrait Source { function present() {} }\nclass Consumer { use Source { missing as private; } }\n",
        "The modifiers of the trait method missing() are changed, but this method does not exist. Error",
        3,
    );
    assert_link_fatal(
        "<?php\ntrait Source { function present() {} }\nclass Consumer { use Source { Source::missing as renamed; } }\n",
        "An alias was defined for Source::missing but this method does not exist",
        3,
    );
}

#[test]
fn aliases_cannot_refer_to_an_alias_declared_earlier_in_the_same_consumer() {
    assert_link_fatal(
        "<?php\ntrait Source { function present() {} }\nclass Consumer {\n    use Source {\n        present as firstAlias;\n        firstAlias as protected;\n    }\n}\n",
        "The modifiers of the trait method firstAlias() are changed, but this method does not exist. Error",
        3,
    );
}

#[test]
fn unqualified_aliases_are_ambiguous_before_consumer_methods_can_override_them() {
    assert_link_fatal(
        "<?php\ntrait First { function choose() {} }\ntrait Second { function choose() {} }\nclass Consumer {\n    function choose() {}\n    use First { choose as firstChoice; }\n    use Second { choose as secondChoice; }\n}\n",
        "An alias was defined for method choose(), which exists in both First and Second. Use First::choose or Second::choose to resolve the ambiguity",
        4,
    );
}

#[test]
fn precedence_rules_validate_source_before_exclusions_and_aliases() {
    assert_link_fatal(
        "<?php\ntrait First {}\ntrait Second { function choose() {} }\nclass Consumer { use First, Second {\n    missing as renamed;\n    First::choose insteadof UnknownTrait;\n} }\n",
        "A precedence rule was defined for First::choose but this method does not exist",
        4,
    );
    assert_link_fatal(
        "<?php\ntrait First { function choose() {} }\ntrait Second { function choose() {} }\nclass Consumer { use First, Second { First::choose insteadof First; } }\n",
        "Inconsistent insteadof definition. The method choose is to be used from First, but First is also on the exclude list",
        4,
    );
}

#[test]
fn adaptation_owners_must_exist_and_participate_in_the_use_list() {
    assert_link_fatal(
        "<?php\ntrait Present { function choose() {} }\nclass Consumer { use Present { Missing::choose as other; } }\n",
        "Could not find trait Missing",
        3,
    );
    assert_link_fatal(
        "<?php\ntrait Used { function choose() {} }\ntrait Unused { function choose() {} }\nclass Consumer { use Used { Unused::choose as other; } }\n",
        "Required Trait Unused wasn't added to Consumer",
        4,
    );
}

#[test]
fn valid_precedence_alias_and_nested_method_resolution_remain_composable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
trait Origin { function choose() { return 'origin'; } }
trait Nested { use Origin { choose as nestedChoice; } }
trait Other { function choose() { return 'other'; } }
class Consumer {
    use Nested, Other {
        Nested::choose insteadof Other;
        Other::choose as otherChoice;
        nestedChoice as copiedNestedChoice;
    }
}
$value = new Consumer();
echo $value->choose(), '|', $value->otherChoice(), '|', $value->copiedNestedChoice();
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "origin|other|origin");
    assert_eq!(stderr, "");
}
