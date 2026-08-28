mod common;

use common::run_php;

#[test]
fn enum_case_property_default_is_published_before_its_owner() {
    assert_eq!(
        run_php(
            r#"<?php
class Holder { public $value = UnitMode::Ready; }
enum UnitMode { case Ready; }
var_dump(new Holder);
"#,
        ),
        concat!(
            "object(Holder)#2 (1) {\n",
            "  [\"value\"]=>\n",
            "  enum(UnitMode::Ready)\n",
            "}\n",
        )
    );
}

#[test]
fn unit_backed_and_cases_fetches_publish_handles_in_php_order() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitMode { case First; case Second; case Third; }
$third = UnitMode::Third;
$first = UnitMode::First;
$second = UnitMode::Second;
$after = new stdClass;
echo spl_object_id($third), ':', spl_object_id($first), ':', spl_object_id($second), ':', spl_object_id($after), "\n";
"#,
        ),
        "1:2:3:4\n"
    );

    assert_eq!(
        run_php(
            r#"<?php
enum BackedMode: int { case First = 1; case Second = 2; case Third = 3; }
$second = BackedMode::Second;
$after = new stdClass;
echo spl_object_id($second), ':', spl_object_id(BackedMode::First), ':', spl_object_id(BackedMode::Third), ':', spl_object_id($after), "\n";
"#,
        ),
        "2:1:3:4\n"
    );

    for declaration in [
        "enum Listed { case First; case Second; case Third; }",
        "enum Listed: int { case First = 1; case Second = 2; case Third = 3; }",
    ] {
        let source = format!(
            "<?php {declaration} $cases = Listed::cases(); $after = new stdClass; echo spl_object_id($cases[0]), ':', spl_object_id($cases[1]), ':', spl_object_id($cases[2]), ':', spl_object_id($after), \"\\n\";"
        );
        assert_eq!(run_php(&source), "1:2:3:4\n", "{declaration}");
    }
}

#[test]
fn declaration_defaults_publish_nested_cases_once_before_following_objects() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitMode { case First; case Second; }
class Holder {
    public $direct = UnitMode::Second;
    public $nested = [UnitMode::First];
}
class Defaults {
    public const VALUE = UnitMode::Second;
    public static $value = UnitMode::First;
}
function acceptMode($value = UnitMode::Second) { return $value; }

$prior = new stdClass;
$holder = new Holder;
$constant = Defaults::VALUE;
$static = Defaults::$value;
$parameter = acceptMode();
$following = new stdClass;
echo spl_object_id($prior), ':', spl_object_id($holder->direct), ':', spl_object_id($holder->nested[0]), ':', spl_object_id($holder), ':';
echo spl_object_id($constant), ':', spl_object_id($static), ':', spl_object_id($parameter), ':', spl_object_id($following), "\n";
"#,
        ),
        "1:2:3:4:2:3:2:5\n"
    );
}

#[test]
fn static_property_publication_terminates_on_later_reference_cycles() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitMode { case First; }
class Defaults { public static $value = [UnitMode::First]; }
$cycle =& Defaults::$value;
$cycle[] =& $cycle;
$copy = Defaults::$value;
$following = new stdClass;
echo spl_object_id($copy[0]), ':', spl_object_id($following), ':', (int) ($copy[1][0] === UnitMode::First), "\n";
"#,
        ),
        "1:2:1\n"
    );
}

#[test]
fn unreachable_and_failed_declarations_do_not_consume_case_handles() {
    assert_eq!(
        run_php(
            r#"<?php
$prior = new stdClass;
if (false) { enum ColdMode { case First; case Second; } }
function skipMode(): void { return; enum ReturnedMode { case First; case Second; } }
skipMode();
try { if (true) { enum FailedMode implements MissingContract { case First; case Second; } } }
catch (Throwable $error) { echo get_class($error), "\n"; }
echo enum_exists('ColdMode', false) ? 'visible:' : 'hidden:';
echo enum_exists('ReturnedMode', false) ? 'visible:' : 'hidden:';
echo enum_exists('FailedMode', false) ? 'visible:' : 'hidden:';
$following = new stdClass;
echo spl_object_id($prior), ':', spl_object_id($following), "\n";
"#,
        ),
        "Error\nhidden:hidden:hidden:1:3\n"
    );
}

#[test]
fn eval_declarations_publish_only_reached_valid_enum_cases() {
    assert_eq!(
        run_php(
            r#"<?php
$prior = new stdClass;
eval('enum EvalMode { case First; case Second; }');
eval('if (false) { enum EvalCold { case First; case Second; } }');
$second = EvalMode::Second;
$first = EvalMode::First;
try { eval('if (true) { enum EvalFailed implements MissingContract { case First; case Second; } }'); }
catch (Throwable $error) { echo get_class($error), "\n"; }
$following = new stdClass;
echo spl_object_id($prior), ':', spl_object_id($second), ':', spl_object_id($first), ':';
echo enum_exists('EvalCold', false) ? 'visible:' : 'hidden:';
echo enum_exists('EvalFailed', false) ? 'visible:' : 'hidden:';
echo spl_object_id($following), "\n";
"#,
        ),
        "Error\n1:2:3:hidden:hidden:5\n"
    );
}

#[test]
fn invalid_backed_fetch_publishes_cases_before_the_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
enum Broken: int { case First = 1; case Second = 1; }
$prior = new stdClass;
echo spl_object_id($prior), "\n";
try { Broken::First; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
$following = new stdClass;
echo spl_object_id($following), "\n";
"#,
        ),
        concat!(
            "1\n",
            "Error:Duplicate value in enum Broken for cases First and Second\n",
            "5\n",
        )
    );
}

#[test]
fn enum_singletons_preserve_identity_serialization_and_handle_reuse() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitMode { case First; case Second; }
$temporary = new stdClass;
$first = UnitMode::First;
$second = UnitMode::Second;
$firstId = spl_object_id($first);
$secondId = spl_object_id($second);
$temporaryId = spl_object_id($temporary);
unset($temporary);
$replacement = new stdClass;
$copy = unserialize(serialize($first));
echo $firstId, ':', $secondId, ':', $temporaryId, ':', spl_object_id($replacement), ':';
echo spl_object_id($copy), ':', (int) ($copy === $first), "\n";
"#,
        ),
        "2:3:1:1:2:1\n"
    );

    assert_eq!(
        run_php(
            r#"<?php
enum UnitMode { case First; case Second; }
$case = unserialize('E:14:"UnitMode:First";');
$following = new stdClass;
echo spl_object_id($case), ':', spl_object_id($following), ':', (int) ($case === UnitMode::First), "\n";
"#,
        ),
        "1:2:1\n"
    );
}
