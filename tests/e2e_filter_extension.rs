mod common;

use common::run_php;

#[test]
fn filter_extension_identity_and_signatures_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('filter'), (int) extension_loaded('FILTER'),
    (int) in_array('filter', get_loaded_extensions(), true), "\n";
foreach ([
    'filter_has_var', 'filter_input', 'filter_var', 'filter_input_array',
    'filter_var_array', 'filter_list', 'filter_id',
] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), ':', $function->getReturnType(), "\n";
}
foreach ((new ReflectionFunction('filter_input'))->getParameters() as $parameter) {
    echo $parameter->getName(), ':', $parameter->getType(), ':';
    if (!$parameter->isDefaultValueAvailable()) {
        echo 'required';
    } elseif ($parameter->isDefaultValueConstant()) {
        echo $parameter->getDefaultValueConstantName();
    } else {
        var_export($parameter->getDefaultValue());
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "111\n",
            "filter_has_var:filter:2/2:bool\n",
            "filter_input:filter:2/4:mixed\n",
            "filter_var:filter:1/3:mixed\n",
            "filter_input_array:filter:1/3:array|false|null\n",
            "filter_var_array:filter:1/3:array|false|null\n",
            "filter_list:filter:0/0:array\n",
            "filter_id:filter:1/1:int|false\n",
            "type:int:required\n",
            "var_name:string:required\n",
            "filter:int:FILTER_DEFAULT\n",
            "options:array|int:0\n",
        )
    );
}

#[test]
fn filter_input_uses_the_immutable_request_snapshot() {
    assert_eq!(
        run_php(
            r#"<?php
$self = filter_input(INPUT_SERVER, 'PHP_SELF');
echo (int) filter_has_var(INPUT_SERVER, 'PHP_SELF'), ':',
    (int) is_string($self), ':',
    (int) isset(filter_input_array(INPUT_SERVER)['PHP_SELF']), "\n";
$_SERVER['PHP_SELF'] = 'changed';
unset($_SERVER['SCRIPT_NAME']);
echo (int) (filter_input(INPUT_SERVER, 'PHP_SELF') === $self), ':',
    (int) filter_has_var(INPUT_SERVER, 'SCRIPT_NAME'), "\n";

$_GET['userland'] = 'not raw input';
var_dump(filter_has_var(INPUT_GET, 'userland'));
var_dump(filter_input(INPUT_GET, 'userland'));
var_dump(filter_input_array(INPUT_GET));

$path = filter_input(INPUT_ENV, 'PATH');
$_ENV['PATH'] = 'changed';
echo (int) is_string($path), ':',
    (int) (filter_input(INPUT_ENV, 'PATH') === $path), "\n";
"#,
        ),
        concat!(
            "1:1:1\n",
            "1:1\n",
            "bool(false)\n",
            "NULL\n",
            "NULL\n",
            "1:1\n",
        )
    );
}

#[test]
fn filter_input_rejects_non_input_constants_at_the_public_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn () => filter_has_var(-1, 'x'),
    static fn () => filter_input(3, 'x'),
    static fn () => filter_input_array(6),
    static fn () => filter_input(-1, 'x', FILTER_DEFAULT, new stdClass()),
    static fn () => filter_input_array(-1, 'bad options'),
] as $call) {
    try { $call(); }
    catch (Throwable $error) {
        echo $error::class, ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        concat!(
            "ValueError:filter_has_var(): Argument #1 ($input_type) must be an INPUT_* constant\n",
            "ValueError:filter_input(): Argument #1 ($type) must be an INPUT_* constant\n",
            "ValueError:filter_input_array(): Argument #1 ($type) must be an INPUT_* constant\n",
            "TypeError:filter_input(): Argument #4 ($options) must be of type array|int, stdClass given\n",
            "TypeError:filter_input_array(): Argument #2 ($options) must be of type array|int, string given\n",
        )
    );
}
