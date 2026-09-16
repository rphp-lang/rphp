#![cfg(target_os = "linux")]

mod common;

use std::sync::Mutex;

use common::run_php;

static GETTEXT_PROCESS_STATE: Mutex<()> = Mutex::new(());

fn run_gettext(source: &str) -> String {
    let _guard = GETTEXT_PROCESS_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    run_php(source)
}

#[test]
fn gettext_exports_php_85_signatures_and_extension_identity() {
    assert_eq!(
        run_gettext(
            r#"<?php
$functions = [
    'textdomain', 'gettext', '_', 'dgettext', 'dcgettext',
    'bindtextdomain', 'ngettext', 'dngettext', 'dcngettext',
    'bind_textdomain_codeset',
];
foreach ($functions as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', (string) $function->getReturnType(), '|';
}
echo "\n", (int) extension_loaded('gettext'),
    (int) extension_loaded('GetText'), '|', implode(',', get_loaded_extensions()), "\n";
"#,
        ),
        concat!(
            "textdomain:gettext:0/1:string|gettext:gettext:1/1:string|_:gettext:1/1:string|",
            "dgettext:gettext:2/2:string|dcgettext:gettext:3/3:string|",
            "bindtextdomain:gettext:1/2:string|false|ngettext:gettext:3/3:string|",
            "dngettext:gettext:4/4:string|dcngettext:gettext:5/5:string|",
            "bind_textdomain_codeset:gettext:1/2:string|false|\n",
            "11|calendar,gettext\n",
        )
    );
}

#[test]
fn gettext_parameter_names_types_and_nullable_defaults_are_exact() {
    assert_eq!(
        run_gettext(
            r#"<?php
foreach (['textdomain', 'bindtextdomain', 'bind_textdomain_codeset', 'dcngettext'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, '(';
    foreach ($function->getParameters() as $parameter) {
        echo $parameter->getName(), ':', (string) $parameter->getType();
        if ($parameter->isDefaultValueAvailable()) {
            echo '=', var_export($parameter->getDefaultValue(), true);
        }
        echo ',';
    }
    echo ")\n";
}
"#,
        ),
        concat!(
            "textdomain(domain:?string=NULL,)\n",
            "bindtextdomain(domain:string,directory:?string=NULL,)\n",
            "bind_textdomain_codeset(domain:string,codeset:?string=NULL,)\n",
            "dcngettext(domain:string,singular:string,plural:string,count:int,category:int,)\n",
        )
    );
}

#[test]
fn untranslated_messages_and_plural_fallback_preserve_php_bytes() {
    assert_eq!(
        run_gettext(
            r#"<?php
setlocale(LC_ALL, 'C');
textdomain('messages');
echo gettext('hello-rphp'), '|', _('alias-rphp'), '|',
    dgettext('missing-rphp-domain', 'domain-rphp'), '|',
    dcgettext('missing-rphp-domain', 'category-rphp', LC_MESSAGES), "\n";
foreach ([-1, 0, 1, 2, PHP_INT_MAX] as $count) {
    echo ngettext('one-rphp', 'many-rphp', $count), '|';
}
echo "\n", bin2hex(gettext("a\0b")), '|',
    bin2hex(dgettext('missing-rphp-domain', "c\0d")), "\n";
"#,
        ),
        concat!(
            "hello-rphp|alias-rphp|domain-rphp|category-rphp\n",
            "many-rphp|many-rphp|one-rphp|many-rphp|many-rphp|\n",
            "610062|630064\n",
        )
    );
}

#[test]
fn domain_directory_and_codeset_state_follow_native_query_contracts() {
    assert_eq!(
        run_gettext(
            r#"<?php
setlocale(LC_ALL, 'C');
$domain = 'rphp-state-contract';
echo (int) is_string(bindtextdomain($domain, null)), '|';
$bound = bindtextdomain($domain, '.');
echo (int) ($bound === realpath('.')), '|',
    (int) (bindtextdomain($domain, null) === $bound), '|',
    (int) (bindtextdomain($domain, './missing-rphp-directory') === false), "\n";
echo (int) (bind_textdomain_codeset($domain, null) === false), '|',
    bind_textdomain_codeset($domain, 'UTF-8'), '|',
    bind_textdomain_codeset($domain, null), "\n";
echo textdomain('rphp-state-contract'), '|', textdomain(null), '|',
    textdomain('messages'), "\n";
"#,
        ),
        "1|1|1|1\n1|UTF-8|UTF-8\nrphp-state-contract|rphp-state-contract|messages\n"
    );
}

#[test]
fn domain_validation_reports_php_value_errors_before_native_lookup() {
    assert_eq!(
        run_gettext(
            r#"<?php
$calls = [
    static fn() => textdomain('0'),
    static fn() => textdomain(''),
    static fn() => bindtextdomain('', '.'),
    static fn() => bindtextdomain("a\0b", '.'),
    static fn() => bindtextdomain('rphp-errors', "a\0b"),
    static fn() => bind_textdomain_codeset('', 'UTF-8'),
    static fn() => dgettext('', 'message'),
    static fn() => dngettext('', 'one', 'many', 2),
];
foreach ($calls as $call) {
    try { $call(); }
    catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "textdomain(): Argument #1 ($domain) cannot be zero\n",
            "textdomain(): Argument #1 ($domain) must not be empty\n",
            "bindtextdomain(): Argument #1 ($domain) must not be empty\n",
            "bindtextdomain(): Argument #1 ($domain) must not contain any null bytes\n",
            "bindtextdomain(): Argument #2 ($directory) must not contain any null bytes\n",
            "bind_textdomain_codeset(): Argument #1 ($domain) must not be empty\n",
            "dgettext(): Argument #1 ($domain) must not be empty\n",
            "dngettext(): Argument #1 ($domain) must not be empty\n",
        )
    );
}

#[test]
fn category_validation_rejects_only_lc_all_at_the_php_boundary() {
    assert_eq!(
        run_gettext(
            r#"<?php
foreach ([
    static fn() => dcgettext('rphp', 'message', LC_ALL),
    static fn() => dcngettext('rphp', 'one', 'many', 2, LC_ALL),
] as $call) {
    try { $call(); }
    catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
echo dcgettext('rphp', 'message', -1), '|',
    dcngettext('rphp', 'one', 'many', 2, -1), "\n";
"#,
        ),
        concat!(
            "dcgettext(): Argument #3 ($category) cannot be LC_ALL\n",
            "dcngettext(): Argument #5 ($category) cannot be LC_ALL\n",
            "message|many\n",
        )
    );
}

#[test]
fn gettext_length_limit_is_observed_for_each_message_role() {
    assert_eq!(
        run_gettext(
            r#"<?php
$ok = str_repeat('x', 4096);
$tooLong = $ok . 'x';
$domain = str_repeat('d', 1024);
$domainTooLong = $domain . 'd';
echo strlen(gettext($ok)), '|', strlen(textdomain($domain)), "\n";
$calls = [
    static fn() => gettext($tooLong),
    static fn() => dgettext('domain', $tooLong),
    static fn() => dngettext('domain', $tooLong, 'plural', 1),
    static fn() => dngettext('domain', 'singular', $tooLong, 2),
    static fn() => textdomain($domainTooLong),
    static fn() => dgettext($domainTooLong, 'message'),
];
foreach ($calls as $call) {
    try { $call(); }
    catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "4096|1024\n",
            "gettext(): Argument #1 ($message) is too long\n",
            "dgettext(): Argument #2 ($message) is too long\n",
            "dngettext(): Argument #2 ($singular) is too long\n",
            "dngettext(): Argument #3 ($plural) is too long\n",
            "textdomain(): Argument #1 ($domain) is too long\n",
            "dgettext(): Argument #1 ($domain) is too long\n",
        )
    );
}

#[test]
fn bindtextdomain_empty_and_zero_directories_select_the_current_directory() {
    assert_eq!(
        run_gettext(
            r#"<?php
$domain = 'rphp-cwd-contract';
$cwd = realpath('.');
echo (int) (bindtextdomain($domain, '') === $cwd), '|',
    (int) (bindtextdomain($domain, '0') === $cwd), '|',
    (int) (bindtextdomain($domain, null) === $cwd), "\n";
"#,
        ),
        "1|1|1\n"
    );
}

#[test]
fn gettext_weak_scalar_conversion_matches_internal_calls() {
    assert_eq!(
        run_gettext(
            r#"<?php
echo dcngettext(1, 1, 1, 1, LC_MESSAGES), '|',
    dcngettext('domain', 'one', 'many', 0, LC_MESSAGES), "\n";
"#,
        ),
        "1|many\n"
    );
}

#[test]
fn gettext_strict_types_are_enforced_before_native_calls() {
    assert_eq!(
        run_gettext(
            r#"<?php declare(strict_types=1);
try { gettext(1); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
try { dcngettext('domain', 'one', 'many', '1', LC_MESSAGES); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "gettext(): Argument #1 ($message) must be of type string, int given\n",
            "dcngettext(): Argument #4 ($count) must be of type int, string given\n",
        )
    );
}

#[test]
fn setlocale_uses_native_query_reset_and_candidate_fallback() {
    assert_eq!(
        run_gettext(
            r#"<?php
echo setlocale(LC_ALL, 'C'), '|', setlocale(LC_ALL, null), '|',
    setlocale(LC_ALL, '0'), "\n";
echo setlocale(LC_ALL, ['rphp-locale-does-not-exist', 'POSIX']), '|',
    setlocale(LC_MESSAGES, 'C'), "\n";
"#,
        ),
        "C|C|C\nC|C\n"
    );
}
