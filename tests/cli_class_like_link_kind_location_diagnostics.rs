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

#[test]
fn class_like_link_errors_use_the_php_kind_and_declaration_location() {
    for (source, expected) in [
        (
            "<?php\nenum ParentEnum {}\nclass ChildClass extends ParentEnum {}\n",
            "Fatal error: Class ChildClass cannot extend enum ParentEnum in Standard input code on line 3\n",
        ),
        (
            "<?php\ninterface First { const VALUE = 1; }\ninterface Second { const VALUE = 1; }\ninterface Combined extends First, Second {}\n",
            "Fatal error: Interface Combined inherits both First::VALUE and Second::VALUE, which is ambiguous in Standard input code on line 4\n",
        ),
        (
            "<?php\ninterface First { const VALUE = 1; }\ninterface Second { const VALUE = 1; }\nenum Combined implements First, Second {}\n",
            "Fatal error: Enum Combined inherits both First::VALUE and Second::VALUE, which is ambiguous in Standard input code on line 4\n",
        ),
        (
            "<?php\ninterface First { const VALUE = 1; }\ninterface Second { const VALUE = 1; }\nclass Combined implements First, Second {}\n",
            "Fatal error: Class Combined inherits both First::VALUE and Second::VALUE, which is ambiguous in Standard input code on line 4\n",
        ),
    ] {
        let (status, stdout, stderr) = run_stdin(source);
        assert_eq!(status, 255);
        assert_eq!(stdout, "");
        assert_eq!(stderr, expected);
    }
}

#[test]
fn valid_class_interface_and_enum_constant_links_remain_executable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
interface First { const FIRST = 1; }
interface Second { const SECOND = 2; }
interface CombinedInterface extends First, Second {}
class CombinedClass implements First, Second {}
enum CombinedEnum implements First, Second { case Value; }
echo CombinedInterface::FIRST + CombinedInterface::SECOND, '|';
echo CombinedClass::FIRST + CombinedClass::SECOND, '|';
echo CombinedEnum::FIRST + CombinedEnum::SECOND, '|', CombinedEnum::Value->name;
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "3|3|3|Value");
    assert_eq!(stderr, "");
}
