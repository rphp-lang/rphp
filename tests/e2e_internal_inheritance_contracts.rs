mod common;

use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};

#[test]
fn internal_parent_contracts_render_defaults_and_hard_errors() {
    let cases = [
        (
            "<?php\nclass BrokenZone extends DateTimeZone { public static function listIdentifiers(): array {} }\n",
            "Declaration of BrokenZone::listIdentifiers(): array must be compatible with DateTimeZone::listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): array",
        ),
        (
            "<?php\nclass BrokenDate extends DateTime { public function setTime(int $hour, int $minute, int $second = 0, bool $microsecond = false): DateTime {} }\n",
            "Declaration of BrokenDate::setTime(int $hour, int $minute, int $second = 0, bool $microsecond = false): DateTime must be compatible with DateTime::setTime(int $hour, int $minute, int $second = 0, int $microsecond = 0): DateTime",
        ),
        (
            "<?php\nclass BrokenAccess implements ArrayAccess { public function offsetSet(): void {} }\n",
            "Declaration of BrokenAccess::offsetSet(): void must be compatible with ArrayAccess::offsetSet(mixed $offset, mixed $value): void",
        ),
    ];

    for (source, message) in cases {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/internal-contract.php",
            "/virtual",
        );
        assert_eq!(
            format!("{error:?}"),
            format!("Fatal(\"{message} in /virtual/internal-contract.php on line 2\")")
        );
    }
}

#[test]
fn early_tentative_returns_emit_once_and_attribute_suppresses() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function($code, $message) { echo $code, ':', $message, "\n"; return true; });
class IncompatibleZone extends DateTimeZone {
    public static function listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): string { return ''; }
}
class SuppressedZone extends DateTimeZone {
    #[ReturnTypeWillChange]
    public static function listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null) {}
}
echo "linked\n";
"#,
            "/virtual/tentative-return.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Return type of IncompatibleZone::listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): string should either be compatible with DateTimeZone::listIdentifiers(int $timezoneGroup = DateTimeZone::ALL, ?string $countryCode = null): array, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in /virtual/tentative-return.php on line 4\n",
            "linked\n",
        ),
    );
}

#[test]
fn handler_exception_during_linking_is_catchable_after_class_publication() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($code, $message) { throw new Exception($message); });
try {
    class CaughtDate extends DateTime { public function getTimezone() {} }
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
echo (int) class_exists(CaughtDate::class, false), ':', get_class(new CaughtDate()), "\n";
"#,
        ),
        concat!(
            "Return type of CaughtDate::getTimezone() should either be compatible with DateTime::getTimezone(): DateTimeZone|false, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice\n",
            "1:CaughtDate\n",
        )
    );
}

#[test]
fn inherited_serialization_magic_suppresses_only_the_legacy_notice() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function($code, $message) { echo $code, ':', $message, "\n"; return true; });
class StorageChild extends SplObjectStorage {}
class LegacyStorage implements Serializable {
    public function serialize() {}
    public function unserialize($serialized) {}
}
echo "linked\n";
"#,
            "/virtual/serializable-link.php",
            "/virtual",
        ),
        concat!(
            "8192:LegacyStorage implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary)\n",
            "linked\n",
        )
    );
}

#[test]
fn date_link_skeletons_publish_constants_without_callable_bodies() {
    assert_eq!(
        run_php(
            r#"<?php
echo DateTimeZone::ALL, ':';
var_dump(class_exists(DateTime::class, false), interface_exists(DateTimeInterface::class, false));
"#,
        ),
        "2047:bool(true)\nbool(true)\n"
    );
}
