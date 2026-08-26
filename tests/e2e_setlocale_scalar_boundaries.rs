mod common;
use common::run_php;

#[test]
fn setlocale_queries_and_fallbacks_share_the_portable_request_state() {
    assert_eq!(
        run_php(
            r#"<?php
class LocaleName {
    public function __toString(): string { echo "stringable\n"; return 'C'; }
}
class ThrowingLocale {
    public function __toString(): string { echo "throwing\n"; throw new Error('stop'); }
}

var_dump(setlocale(LC_ALL, null));
var_dump(setlocale(LC_ALL, '0'));
var_dump(setlocale(LC_ALL, ['invalid_RPHP', 'C']));
var_dump(setlocale(LC_ALL, 'invalid_RPHP', 'POSIX'));
$name = 'setlocale';
$first = setlocale(...);
var_dump($name(LC_ALL, new LocaleName));
var_dump($first(LC_ALL, ['invalid_RPHP', new LocaleName]));
var_dump(call_user_func('setlocale', LC_ALL, 'C'));
var_dump(setlocale(category: LC_ALL, locales: 'C'));

try { setlocale(LC_ALL, new ThrowingLocale); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { setlocale(LC_ALL, 'C', new ThrowingLocale); }
catch (Error $error) { echo $error->getMessage(), "\n"; }

$locale = 'C';
$alias =& $locale;
$copy = $locale;
var_dump(setlocale(LC_ALL, $alias));
echo $locale, ':', $alias, ':', $copy, "\n";

$reflection = new ReflectionFunction('setlocale');
foreach ($reflection->getParameters() as $parameter) {
    echo $parameter->getName(), ':', ($parameter->hasType() ? $parameter->getType() : 'none'), ':';
    echo $parameter->isVariadic() ? 'variadic' : 'fixed', "\n";
}
echo $reflection->getReturnType(), "\n";
"#,
        ),
        concat!(
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "stringable\nstring(1) \"C\"\n",
            "stringable\nstring(1) \"C\"\n",
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "throwing\nstop\n",
            "throwing\nstop\n",
            "string(1) \"C\"\nC:C:C\n",
            "category:int:fixed\n",
            "locales:none:fixed\n",
            "rest:none:variadic\n",
            "string|false\n",
        )
    );
}

#[test]
fn setlocale_validates_top_level_scalars_before_lazy_array_candidate_selection() {
    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictLocale {
    public function __toString(): string { echo "must-not-run\n"; return 'C'; }
}
function report(callable $operation): void {
    try { var_dump($operation()); }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
}

report(static fn () => setlocale(LC_ALL, 0, '0'));
report(static fn () => setlocale(LC_ALL, 'invalid_RPHP', 0));
report(static fn () => setlocale(LC_ALL, new StrictLocale));
report(static fn () => setlocale('6', 'C'));
report(static fn () => setlocale(LC_ALL, null));
report(static fn () => setlocale(LC_ALL, 'invalid_RPHP', null));
report(static fn () => setlocale(LC_ALL, [0, 'C']));
report(static fn () => setlocale(LC_ALL, 'C', new StrictLocale));
"#,
        ),
        concat!(
            "TypeError:setlocale(): Argument #2 ($locales) must be of type array|string|null, int given\n",
            "TypeError:setlocale(): Argument #3 must be of type array|string|null, int given\n",
            "TypeError:setlocale(): Argument #2 ($locales) must be of type array|string|null, StrictLocale given\n",
            "TypeError:setlocale(): Argument #1 ($category) must be of type int, string given\n",
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "string(1) \"C\"\n",
            "TypeError:setlocale(): Argument #3 must be of type array|string|null, StrictLocale given\n",
        )
    );
}

#[test]
fn setlocale_warns_at_the_php_name_limit_and_continues_in_order() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function (int $level, string $message): bool {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(setlocale(LC_ALL, str_repeat('A', 254)));
var_dump(setlocale(LC_ALL, str_repeat('B', 255)));
var_dump(setlocale(LC_ALL, str_repeat('C', 255), 'C'));
var_dump(setlocale(LC_ALL, [str_repeat('D', 255), 'C']));
restore_error_handler();

set_error_handler(function (int $level, string $message): never {
    echo $level, ':', $message, "\n";
    throw new Error('handler-stop');
});
try { setlocale(LC_ALL, str_repeat('E', 255), 'C'); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
restore_error_handler();
"#,
        ),
        concat!(
            "bool(false)\n",
            "2:setlocale(): Specified locale name is too long\nbool(false)\n",
            "2:setlocale(): Specified locale name is too long\nstring(1) \"C\"\n",
            "2:setlocale(): Specified locale name is too long\nstring(1) \"C\"\n",
            "2:setlocale(): Specified locale name is too long\nhandler-stop\n",
        )
    );
}
