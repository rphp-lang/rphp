//! Original PHP 8.5 trait constant composition regressions.

#[test]
fn consumer_magic_with_missing() {
    assert_cli(
        r###"trait Source{const NAME=__CLASS__;const PENDING=UNKNOWN;}class First{use Source;}echo First::NAME,'|';try{echo First::PENDING;}catch(Throwable $e){echo $e->getMessage(),'|';}"###,
        0,
        "First|Undefined constant \"UNKNOWN\"|",
        "",
    );
}

#[test]
fn consumer_multiple() {
    assert_cli(
        r###"trait Source{const VALUE=__CLASS__;}class First{use Source;const VALUE='First';}class Second{use Source;const VALUE='Second';}echo First::VALUE,'|',Second::VALUE,'|';"###,
        0,
        "First|Second|",
        "",
    );
}

#[test]
fn consumer_self_name() {
    assert_cli(
        r###"trait Source{const VALUE=self::class;}class First{use Source;const VALUE='First';}class Second{use Source;const VALUE='Second';}echo First::VALUE,'|',Second::VALUE,'|';"###,
        0,
        "First|Second|",
        "",
    );
}

#[test]
fn consumer_dependent_magic() {
    assert_cli(
        r###"trait Source{const NAME=__CLASS__;const VALUE=self::NAME;}class First{use Source;const VALUE='First';}class Second{use Source;const VALUE='Second';}echo First::VALUE,'|',Second::VALUE,'|';"###,
        0,
        "First|Second|",
        "",
    );
}

#[test]
fn self_other_trait() {
    assert_cli(
        r###"define('SEED',30);trait LeftSource{const VALUE=self::BASE+7;}trait RightSource{const BASE=SEED;}class Target{use LeftSource,RightSource;const VALUE=37;}echo Target::VALUE,'|';"###,
        255,
        "",
        "Warning: Uncaught Error: Undefined constant self::BASE in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\nFatal error: Target and LeftSource define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn self_prior_trait() {
    assert_cli(
        r###"define('SEED',30);trait LeftSource{const VALUE=self::BASE+7;}trait RightSource{const BASE=SEED;}class Target{use RightSource,LeftSource;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn self_prior_member() {
    assert_cli(
        r###"define('SEED',30);trait Source{const BASE=SEED;const VALUE=self::BASE+7;}class Target{use Source;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn self_later_member() {
    assert_cli(
        r###"define('SEED',30);trait Source{const VALUE=self::BASE+7;const BASE=SEED;}class Target{use Source;const VALUE=37;}echo Target::VALUE,'|';"###,
        255,
        "",
        "Warning: Uncaught Error: Undefined constant self::BASE in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\nFatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn autoload_two_traits_order() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo $n,'|';eval('class '.$n.'{const VALUE=37;}');});trait LeftSource{const VALUE=LeftDependency::VALUE;}trait RightSource{const VALUE=RightDependency::VALUE;}class Target{use LeftSource,RightSource;}echo Target::VALUE,'|';"###,
        0,
        "RightDependency|LeftDependency|37|",
        "",
    );
}

#[test]
fn consumer_missing_with_eager_magic() {
    assert_cli(
        r###"trait Source{const NAME=self::class;const PENDING=UNKNOWN;}class Target{use Source;const NAME='Target';}echo Target::NAME,'|';"###,
        0,
        "Target|",
        "",
    );
}

#[test]
fn autoload_order() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo $n,'|';eval('class '.$n.'{const VALUE=37;}');});trait Source{const VALUE=RightDependency::VALUE;}class Target{use Source;const VALUE=LeftDependency::VALUE;}echo Target::VALUE,'|';"###,
        0,
        "RightDependency|LeftDependency|37|",
        "",
    );
}

#[test]
fn inherited_nonfinal_override() {
    assert_cli(
        r###"define('SEED',37);class Ancestor{const VALUE=41;}trait Source{const VALUE=SEED;}class Target extends Ancestor{use Source;}echo Ancestor::VALUE,'|',Target::VALUE,'|';"###,
        0,
        "41|37|",
        "",
    );
}

#[test]
fn metadata_final_priority() {
    assert_cli(
        r###"trait Source{final const VALUE=UNKNOWN;}class Target{use Source;const VALUE=37;}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn array_cow() {
    assert_cli(
        r###"define('SEED',['key'=>[37]]);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=['key'=>[37]];}$copy=Target::VALUE;$copy['key'][]=41;echo count($copy['key']),'|',count(Target::VALUE['key']),'|',count(SEED['key']),'|';"###,
        0,
        "2|1|1|",
        "",
    );
}

#[test]
fn warning_handlers() {
    assert_cli(
        r###"set_error_handler(function(){echo 'handler|';});set_exception_handler(function($e){echo 'exception|';});register_shutdown_function(function(){echo 'shutdown:',(int)class_exists('Target',false),'|';});trait Source{const VALUE=UNKNOWN;}class Target{use Source;const VALUE=37;}"###,
        255,
        "shutdown:0|",
        "Warning: Uncaught Error: Undefined constant \"UNKNOWN\" in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\nFatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn raw_typed_deferred_both() {
    assert_cli(
        r###"define('SEED',37);trait Source{const float VALUE=SEED;}class Target{use Source;const float VALUE=SEED;}var_dump(Target::VALUE);"###,
        0,
        "float(37)\n",
        "",
    );
}

#[test]
fn array_float_unequal() {
    assert_cli(
        r###"define('SEED',[37]);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=[37.0];}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

use std::process::Command;

fn assert_cli(source: &str, status: i32, stdout: &str, stderr: &str) {
    let mut binaries = vec![env!("CARGO_BIN_EXE_rphp").to_string()];
    if let Ok(reference) = std::env::var("RPHP_REFERENCE_PHP") {
        binaries.push(reference);
    }
    for binary in binaries {
        let output = Command::new(&binary)
            .args([
                "-n",
                "-d",
                "display_errors=stderr",
                "-d",
                "fatal_error_backtraces=0",
                "-d",
                "log_errors=0",
                "-r",
                source,
            ])
            .output()
            .expect("run original trait constant case");
        assert_eq!(output.status.code(), Some(status), "{binary}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            stdout,
            "{binary}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            stderr,
            "{binary}"
        );
    }
}

#[test]
fn literal_control() {
    assert_cli(
        r###"trait Source{const VALUE=37;}class Target{use Source;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn global_equal() {
    assert_cli(
        r###"define('SEED',37);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn global_unequal() {
    assert_cli(
        r###"define('SEED',37);trait Source{const VALUE=SEED;}echo 'before|';class Target{use Source;const VALUE=41;}echo 'after|';"###,
        255,
        "before|",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn class_global_equal() {
    assert_cli(
        r###"define('SEED',37);trait Source{const VALUE=37;}class Target{use Source;const VALUE=SEED;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn both_global_equal() {
    assert_cli(
        r###"define('LEFT_SEED',37);define('RIGHT_SEED',37);trait Source{const VALUE=LEFT_SEED;}class Target{use Source;const VALUE=RIGHT_SEED;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn two_traits_equal() {
    assert_cli(
        r###"define('SEED',37);trait LeftSource{const VALUE=SEED;}trait RightSource{const VALUE=37;}class Target{use LeftSource,RightSource;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn two_traits_unequal() {
    assert_cli(
        r###"define('LEFT_SEED',37);define('RIGHT_SEED',41);trait LeftSource{const VALUE=LEFT_SEED;}trait RightSource{const VALUE=RIGHT_SEED;}echo 'before|';class Target{use LeftSource,RightSource;}echo 'after|';"###,
        255,
        "before|",
        "Fatal error: LeftSource and RightSource define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn array_equal() {
    assert_cli(
        r###"define('SEED',['key'=>37]);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=['key'=>37];}var_dump(Target::VALUE);"###,
        0,
        "array(1) {\n  [\"key\"]=>\n  int(37)\n}\n",
        "",
    );
}

#[test]
fn array_order_unequal() {
    assert_cli(
        r###"define('SEED',['a'=>1,'b'=>2]);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=['b'=>2,'a'=>1];}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn expression_equal() {
    assert_cli(
        r###"define('SEED',30);trait Source{const VALUE=SEED+7;}class Target{use Source;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn self_equal() {
    assert_cli(
        r###"trait Source{const VALUE=self::BASE+7;}class Target{use Source;const BASE=30;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn self_nested_traits() {
    assert_cli(
        r###"trait Source{const VALUE=self::BASE+7;}trait Nested{use Source;}class Target{use Nested;const BASE=30;const VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn missing_collision() {
    assert_cli(
        r###"trait Source{const VALUE=UNKNOWN_SEED;}echo 'before|';try{class Target{use Source;const VALUE=37;}}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}echo (int)class_exists('Target',false),'|after|';"###,
        255,
        "before|",
        "Warning: Uncaught Error: Undefined constant \"UNKNOWN_SEED\" in Command line code:1\nStack trace:\n#0 {main}\n  thrown in Command line code on line 1\nFatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn missing_no_collision() {
    assert_cli(
        r###"trait Source{const VALUE=UNKNOWN_SEED;}class Target{use Source;}echo 'linked|';try{echo Target::VALUE;}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}"###,
        0,
        "linked|Error:Undefined constant \"UNKNOWN_SEED\"|",
        "",
    );
}

#[test]
fn missing_retry() {
    assert_cli(
        r###"trait Source{const VALUE=UNKNOWN_SEED;}try{eval('class Target{use Source;const VALUE=37;}');}catch(Throwable $e){echo get_class($e),'|';}define('UNKNOWN_SEED',37);eval('class Target{use Source;const VALUE=37;}');echo Target::VALUE,'|';"###,
        255,
        "",
        "Warning: Uncaught Error: Undefined constant \"UNKNOWN_SEED\" in Command line code(1) : eval()'d code:1\nStack trace:\n#0 Command line code(1): eval()\n#1 {main}\n  thrown in Command line code(1) : eval()'d code on line 1\nFatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code(1) : eval()'d code on line 1\n",
    );
}

#[test]
fn autoload_equal() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo 'load:',$n,'|';eval('class Dependency{const VALUE=37;}');});trait Source{const VALUE=Dependency::VALUE;}echo 'before|';class Target{use Source;const VALUE=37;}echo 'after|',Target::VALUE,'|';"###,
        0,
        "before|load:Dependency|after|37|",
        "",
    );
}

#[test]
fn autoload_throw() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo 'load:',$n,'|';throw new Exception('loader');});trait Source{const VALUE=Dependency::VALUE;}try{class Target{use Source;const VALUE=37;}}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}echo (int)class_exists('Target',false),'|';"###,
        255,
        "load:Dependency|",
        "Warning: Uncaught Exception: loader in Command line code:1\nStack trace:\n#0 Command line code(1): {closure:Command line code:1}('Dependency')\n#1 {main}\n  thrown in Command line code on line 1\nFatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn autoload_publication() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo (int)class_exists('Target',false),'|';eval('class Dependency{const VALUE=37;}');});trait Source{const VALUE=Dependency::VALUE;}class Target{use Source;const VALUE=37;}echo (int)class_exists('Target',false),'|';"###,
        0,
        "0|1|",
        "",
    );
}

#[test]
fn visibility_priority() {
    assert_cli(
        r###"trait Source{public const VALUE=UNKNOWN_SEED;}class Target{use Source;private const VALUE=37;}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn type_metadata_priority() {
    assert_cli(
        r###"trait Source{public const int VALUE=UNKNOWN_SEED;}class Target{use Source;public const VALUE=37;}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn typed_equal() {
    assert_cli(
        r###"define('SEED',37);trait Source{const int VALUE=SEED;}class Target{use Source;const int VALUE=37;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn typed_float_normalized() {
    assert_cli(
        r###"define('SEED',37);trait Source{const float VALUE=SEED;}class Target{use Source;const float VALUE=37.0;}var_dump(Target::VALUE);"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn typed_invalid() {
    assert_cli(
        r###"define('SEED','bad');trait Source{const int VALUE=SEED;}try{class Target{use Source;const int VALUE=37;}}catch(Throwable $e){echo get_class($e),':',$e->getMessage(),'|';}echo (int)class_exists('Target',false),'|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn strict_scalar_unequal() {
    assert_cli(
        r###"define('SEED',37);trait Source{const VALUE=SEED;}class Target{use Source;const VALUE=37.0;}echo 'after|';"###,
        255,
        "",
        "Fatal error: Target and Source define the same constant (VALUE) in the composition of Target. However, the definition differs and is considered incompatible. Class was composed in Command line code on line 1\n",
    );
}

#[test]
fn scope_class() {
    assert_cli(
        r###"trait Source{const VALUE=__CLASS__;}class Target{use Source;const VALUE='Target';}echo Target::VALUE,'|';"###,
        0,
        "Target|",
        "",
    );
}

#[test]
fn enum_consumer() {
    assert_cli(
        r###"define('SEED',37);trait Source{const VALUE=SEED;}enum Target{use Source;const VALUE=37;case One;}echo Target::VALUE,'|';"###,
        0,
        "37|",
        "",
    );
}

#[test]
fn noncollision_laziness() {
    assert_cli(
        r###"spl_autoload_register(function($n){echo 'load|';eval('class Dependency{const VALUE=37;}');});trait Source{const VALUE=Dependency::VALUE;}class Target{use Source;}echo 'linked|';echo Target::VALUE,'|';"###,
        0,
        "linked|load|37|",
        "",
    );
}
