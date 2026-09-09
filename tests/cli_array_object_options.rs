use std::process::Command;

fn contract(name: &str, expected: &[u8]) {
    let result = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-d",
            "display_errors=1",
            "-d",
            "log_errors=0",
            "-d",
            "error_reporting=32767",
        ])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/array_object_options/contract.php"
        ))
        .env("RPHP_ARRAY_OBJECT_CASE", name)
        .output()
        .expect("run native view policy specimen");
    assert_eq!(
        result.status.code(),
        Some(0),
        "{name}: {:?} {:?}",
        result.stdout,
        result.stderr
    );
    assert!(result.stderr.is_empty(), "{name}: {:?}", result.stderr);
    assert_eq!(
        result.stdout,
        expected,
        "{name}: {}",
        String::from_utf8_lossy(&result.stdout)
    );
}
macro_rules! specimen {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            contract(
                $case,
                include_bytes!(concat!(
                    "fixtures/array_object_options/",
                    $case,
                    ".expected"
                )),
            );
        }
    };
}
specimen!(metadata, "metadata");
specimen!(flags, "flags");
specimen!(nested, "nested");
specimen!(property, "property");
specimen!(projection, "projection");
specimen!(references, "references");
specimen!(clone, "clone");
specimen!(weak, "weak");
specimen!(iterator_class, "iterator-class");
specimen!(ordering, "ordering");
specimen!(autoload, "autoload");
specimen!(strict, "strict");
specimen!(debug_order, "debug-order");
specimen!(write_operations, "write-operations");
specimen!(typed_properties, "typed-properties");
specimen!(overloaded_properties, "overloaded-properties");
specimen!(scoped_calls, "scoped-calls");
specimen!(self_replacement, "self-replacement");
specimen!(cold_projection, "cold-projection");
specimen!(cold_clone, "cold-clone");
