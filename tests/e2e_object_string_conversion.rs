mod common;

use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};

#[test]
fn canonical_conversion_preserves_reference_returns_and_method_strictness() {
    assert_eq!(
        run_php(
            r#"<?php
class RefText {
    private string $value = 'alpha';
    public int $calls = 0;
    public function &__toString(): string { $this->calls++; return $this->value; }
}

function accept(string $value): string { return $value; }
$value = new RefText;
var_dump((string) $value, strlen($value), strtoupper($value), accept($value));
echo "calls={$value->calls}\n";

class NoText { public function __toString() {} }
class ArrayText { public function __toString() { return []; } }
class WeakScalarText { public function __toString() { return 42; } }
foreach ([new NoText, new ArrayText] as $bad) {
    try { echo $bad; } catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage(), "\n";
    }
}
echo (string) new WeakScalarText, "\n";
"#,
        ),
        concat!(
            "string(5) \"alpha\"\n",
            "int(5)\n",
            "string(5) \"ALPHA\"\n",
            "string(5) \"alpha\"\n",
            "calls=4\n",
            "TypeError:NoText::__toString(): Return value must be of type string, none returned\n",
            "TypeError:ArrayText::__toString(): Return value must be of type string, array returned\n",
            "42\n",
        )
    );

    assert_eq!(
        run_php(
            r#"<?php declare(strict_types=1);
class StrictScalarText { public function __toString() { return 42; } }
try { echo new StrictScalarText; } catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage();
}
"#,
        ),
        "TypeError:StrictScalarText::__toString(): Return value must be of type string, int returned"
    );
}

#[test]
fn constructor_reference_arguments_remain_live_for_later_string_conversion() {
    assert_eq!(
        run_php(
            r#"<?php
class LinkedText {
    public $value;
    public function __construct(&$value) { $this->value =& $value; }
    public function __toString(): string { return $this->value; }
}
$value = 'world';
$text = new LinkedText($value);
var_dump(strlen($text), $text->value, $value);
$value = 'foobar';
var_dump(strlen($text), $text->value);
"#,
        ),
        concat!(
            "int(5)\n",
            "string(5) \"world\"\n",
            "string(5) \"world\"\n",
            "int(6)\n",
            "string(6) \"foobar\"\n",
        )
    );
}

#[test]
fn uncaught_throwable_rendering_keeps_the_replacement_conversion_frame() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function accepts_iterable(?iterable $value): void {}
try { accepts_iterable(1); } catch (TypeError $error) { echo $error; }
"#,
            "/virtual/caught-type-error-string.php",
            "/virtual",
        ),
        concat!(
            "TypeError: accepts_iterable(): Argument #1 ($value) must be of type Traversable|array|null, int given, called in /virtual/caught-type-error-string.php on line 3 and defined in /virtual/caught-type-error-string.php:2\n",
            "Stack trace:\n",
            "#0 /virtual/caught-type-error-string.php(3): accepts_iterable(1)\n",
            "#1 {main}",
        )
    );

    let uncaught_type_error = run_php_expect_error_with_source_context(
        r#"<?php
function requires_iterable(?iterable $value): void {}
requires_iterable(1);
"#,
        "/virtual/uncaught-type-error-string.php",
        "/virtual",
    );
    assert_eq!(
        uncaught_type_error.to_string(),
        concat!(
            "Uncaught TypeError: requires_iterable(): Argument #1 ($value) must be of type Traversable|array|null, int given, called in /virtual/uncaught-type-error-string.php on line 3 and defined in /virtual/uncaught-type-error-string.php:2\n",
            "Stack trace:\n",
            "#0 /virtual/uncaught-type-error-string.php(3): requires_iterable(1)\n",
            "#1 {main}\n",
            "  thrown in /virtual/uncaught-type-error-string.php on line 2",
        )
    );

    let replacement = run_php_expect_error_with_source_context(
        r#"<?php
class RenderReplacement extends Exception {}
class RenderSource extends Exception {
    public function __TOSTRING(): string { throw new RenderReplacement('render failed'); }
}
throw new RenderSource('source');
"#,
        "/virtual/uncaught-string-render.php",
        "/virtual",
    );
    assert_eq!(
        replacement.to_string(),
        concat!(
            "Uncaught RenderReplacement: render failed in /virtual/uncaught-string-render.php:4\n",
            "Stack trace:\n",
            "#0 [internal function]: RenderSource->__TOSTRING()\n",
            "#1 {main}\n",
            "  thrown in /virtual/uncaught-string-render.php on line 4",
        )
    );

    let invalid_message = run_php_expect_error_with_source_context(
        r#"<?php
class InvalidMessage extends Exception {
    public function __construct() { $this->message = new stdClass; }
}
throw new InvalidMessage;
"#,
        "/virtual/invalid-exception-message.php",
        "/virtual",
    );
    assert_eq!(
        invalid_message.to_string(),
        concat!(
            "Uncaught Error: Object of class stdClass could not be converted to string in [no active file]:0\n",
            "Stack trace:\n",
            "#0 [internal function]: Exception->__toString()\n",
            "#1 {main}\n",
            "  thrown in [no active file] on line 0",
        )
    );
}

#[test]
fn engine_string_sites_run_conversion_before_commit_and_retain_call_site() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class TextName {
    public function __construct(private string $value) {}
    public function __toString(): string {
        $frame = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS)[0];
        echo "site={$frame['file']}:{$frame['line']}|";
        return $this->value;
    }
}
$name = new TextName('dynamic');
${$name} = 7;
$object = new stdClass;
$object->{new TextName('property')} = 9;
echo $dynamic, ':', $object->property, "\n";

class BadText {
    public int $calls = 0;
    public function __toString(): string { $this->calls++; throw new Exception('stop'); }
}
$bad = new BadText;
$left = 'stable';
try { $left .= $bad; } catch (Throwable $error) { echo $error->getMessage(), '|'; }
try { $left[0] = $bad; } catch (Throwable $error) { echo $error->getMessage(), '|'; }
echo "$left:{$bad->calls}";
"#,
            "/virtual/object-string-sites.php",
            "/virtual",
        ),
        concat!(
            "site=/virtual/object-string-sites.php:11|",
            "site=/virtual/object-string-sites.php:13|",
            "7:9\n",
            "stop|stop|stable:2",
        )
    );
}

#[test]
fn include_paths_and_reentrant_array_sort_use_live_storage() {
    assert_eq!(
        run_php(
            r#"<?php
$path = tempnam(sys_get_temp_dir(), 'rphp-tostring-');
file_put_contents($path, '<?php echo "included|"; return 73;');
class IncludePath {
    public function __construct(private string $path) {}
    public function __toString(): string { return $this->path; }
}
var_dump(include new IncludePath($path));
unlink($path);

function resize_array(): void {
    global $array;
    for ($index = 0; $index < 4; $index++) { $array[$index] = $index; }
}
class ReentrantSortText {
    public function __toString(): string { resize_array(); return '3'; }
}
$array = ['a' => '1', '3' => new ReentrantSortText, '2' => '2'];
asort($array);
echo implode(',', array_keys($array)), '|', implode(',', array_values($array));
"#,
        ),
        concat!("included|int(73)\n", "a,3,2,0,1|1,3,2,0,1",)
    );
}

#[test]
fn deferred_defaults_do_not_publish_a_reentrant_partial_result() {
    assert_eq!(
        run_php(
            r#"<?php
class Token {
    public int $calls = 0;
    public function __toString(): string {
        $this->calls++;
        if ($this->calls === 1) { return 'T'; }
        throw new Exception('second conversion');
    }
}
const TOKEN = new Token;
class DeferredHolder { public $value = LaterName::VALUE . TOKEN; }
spl_autoload_register(function(string $name): void {
    class LaterName { public const VALUE = 'L'; }
    echo 'inner=', (new DeferredHolder)->value, "\n";
});
try { new DeferredHolder; } catch (Throwable $error) {
    echo $error->getMessage(), '|calls=', TOKEN->calls;
}
"#,
        ),
        "inner=LT\nsecond conversion|calls=2"
    );
}
