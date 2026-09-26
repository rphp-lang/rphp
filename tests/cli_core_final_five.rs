//! Independent PHP 8.5 contracts for the final stable-core failure sweep.
use std::process::Command;

#[test]
fn image_headers() {
    contract(
        "image-headers",
        concat!(
            "{\"0\":3,\"1\":2,\"2\":3,\"3\":\"width=\\\"3\\\" height=\\\"2\\\"\",\"bits\":8,\"mime\":\"image\\/png\",\"width_unit\":\"px\",\"height_unit\":\"px\"}|[]\n1\n",
            "{\"0\":5,\"1\":4,\"2\":1,\"3\":\"width=\\\"5\\\" height=\\\"4\\\"\",\"bits\":1,\"channels\":3,\"mime\":\"image\\/gif\",\"width_unit\":\"px\",\"height_unit\":\"px\"}|[]\n1\n",
            "{\"0\":9,\"1\":7,\"2\":2,\"3\":\"width=\\\"9\\\" height=\\\"7\\\"\",\"bits\":8,\"channels\":3,\"mime\":\"image\\/jpeg\",\"width_unit\":\"px\",\"height_unit\":\"px\"}|{\"APP1\":\"demo\"}\n1\n",
        ),
    );
}

fn contract(case: &str, expected: &str) {
    let candidate =
        std::env::var_os("RPHP_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_rphp").into());
    let mut binaries = vec![candidate];
    if let Some(reference) = std::env::var_os("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let output = Command::new(&binary)
            .args(["-n", "-d", "display_errors=1", "-d", "log_errors=0"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/core_final_five/contract.php"
            ))
            .env("RPHP_FINAL_FIVE_CASE", case)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{binary:?} {case}: {output:?}"
        );
        assert!(output.stderr.is_empty(), "{binary:?} {case}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{binary:?} {case}"
        );
    }
}

macro_rules! specimen {
    ($name:ident, $case:literal, $expected:literal) => {
        #[test]
        fn $name() {
            contract($case, $expected);
        }
    };
}
specimen!(
    regex_fiber_ref_warning,
    "regex-fiber-ref-warning",
    "2:referenceMatch(): Argument #1 ($match) must be passed by reference, value given|a:7|X|1\n"
);
specimen!(
    random_construction_guard,
    "random-construction-guard",
    "Class Random\\Engine\\Secure is an internal class marked as final that cannot be instantiated without invoking its constructor\nClass Random\\Engine\\Xoshiro256StarStar is an internal class marked as final that cannot be instantiated without invoking its constructor\n"
);
specimen!(
    image_typed_output,
    "image-typed-output",
    "Cannot assign array to reference held by property ImageTyped::$info of type int|7\n"
);
specimen!(regex_fiber_nested, "regex-fiber-nested", "a|b|AB|1\n");
specimen!(
    regex_fiber_arrays,
    "regex-fiber-arrays",
    "{\"0\":[\"a\",0],\"1\":[\"a\",0],\"tail\":[null,-1],\"2\":[null,-1]}|[[\"X\",0]]|{\"0\":[\"b\",0],\"1\":[\"b\",0],\"tail\":[null,-1],\"2\":[null,-1]}|{\"first\":\"A\",\"9\":\"B\"}:3:1\n"
);
specimen!(
    image_output_release,
    "image-output-release",
    "NULL|released|[]\nTypeError|23\nPath must not be empty|[]\n"
);
specimen!(
    image_invalid_data,
    "image-invalid-data",
    "8:getimagesizefromstring(): Error reading from !|bool(false)\n[]\n8:getimagesizefromstring(): Error reading from bad!|bool(false)\n[]\nbool(false)\n[]\nbool(false)\n[]\nbool(false)\n[]\n"
);

specimen!(
    relative_ancestors,
    "relative-ancestors",
    "Domain\\Base|1|trait\n"
);
specimen!(
    relative_doc_comments,
    "relative-doc-comments",
    "bool(false)\n/** base-only */|/** child-only */\n"
);
specimen!(
    integer_members,
    "integer-members",
    "{\"7\":\"c\",\"-2\":\"b\"}|c|b\nO:8:\"stdClass\":2:{s:1:\"7\";s:1:\"d\";s:2:\"-2\";s:1:\"b\";}\n"
);
specimen!(integer_member_cycle, "integer-member-cycle", "1|1\nNULL\n");
specimen!(
    integer_member_deprecation,
    "integer-member-deprecation",
    "8192:Creation of dynamic property MemberTarget::$3 is deprecated\n10|1\n"
);
specimen!(
    regex_fiber_resume,
    "regex-fiber-resume",
    "start:enter:a|a|next:leave:1|enter:b|b|leave:2|result:A1B2:2|done:1\n"
);
specimen!(
    regex_fiber_throw,
    "regex-fiber-throw",
    "z|callback-finally|caught:injected|finished|1\n"
);
specimen!(
    regex_fiber_cycle,
    "regex-fiber-cycle",
    "parked|live|released|end\n"
);
specimen!(
    random_state,
    "random-state",
    "cae2e678e62c72a8|cae2e678e62c72a8\n[[],[\"b16ef11458c8e1dd\",\"3e85df98a5e6bdb0\",\"ef6895f716704c2a\",\"4207b5b7ac4bba43\"]]\n1\naf5c766999c0ddd5\n8a6cf58b9ce7f27d\n8016000000000000\n"
);
specimen!(
    random_capabilities,
    "random-capabilities",
    "8|1|1\nError:Trying to clone an uncloneable object of class Random\\Engine\\Secure\n0\nError:Cannot create dynamic property Random\\Engine\\Xoshiro256StarStar::$note\n0\nTrying to clone an uncloneable object of class Random\\Engine\\Secure\nSerialization of 'Random\\Engine\\Secure' is not allowed\n"
);
specimen!(
    random_invalid_state,
    "random-invalid-state",
    "ValueError:Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be a 32 byte (256 bit) string\nValueError:Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must not consist entirely of NUL bytes\nTypeError:Random\\Engine\\Xoshiro256StarStar::__construct(): Argument #1 ($seed) must be of type string|int|null, array given\nException:Invalid serialization data for Random\\Engine\\Xoshiro256StarStar object\n1\n"
);
