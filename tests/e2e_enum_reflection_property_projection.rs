mod common;

use common::run_php;

#[test]
fn enum_reflection_properties_expose_only_name_and_class() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitState { case Ready; }
enum BackedState: int { case Ready = 7; }

foreach ([UnitState::class, BackedState::class] as $class) {
    $properties = (new ReflectionClass($class))->getProperties();
    echo $class, ':', count($properties), ':';
    foreach ($properties as $property) {
        echo $property->getName(), '=', implode(',', array_keys(get_object_vars($property))), ';';
        ob_start();
        var_dump($property);
        $dump = ob_get_clean();
        echo (int) !str_contains($dump, '__reflection_'), ':';
        echo (int) str_contains($dump, ' (2) {'), ':';
        echo json_encode($property), '|';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "UnitState:1:name=name,class;1:1:{\"name\":\"name\",\"class\":\"UnitState\"}|\n",
            "BackedState:2:name=name,class;1:1:{\"name\":\"name\",\"class\":\"BackedState\"}|",
            "value=name,class;1:1:{\"name\":\"value\",\"class\":\"BackedState\"}|\n",
        )
    );
}

#[test]
fn reflection_property_sidecar_preserves_ordinary_method_metadata() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentState {
    public int $visible = 3;
    protected static string $shared = 'x';
}
class ChildState extends ParentState {
    public function __construct(public readonly string $promoted = 'ready') {}
}

foreach (['promoted', 'visible', 'shared'] as $name) {
    $property = new ReflectionProperty(ChildState::class, $name);
    echo $property->getName(), ':', $property->class, ':';
    echo (int) $property->isPublic(), (int) $property->isProtected(), (int) $property->isStatic(), (int) $property->isReadOnly(), ':';
    echo (int) $property->hasType(), ':', $property->getType(), ':';
    echo (int) $property->hasDefaultValue(), ':';
    if ($property->hasDefaultValue()) {
        var_export($property->getDefaultValue());
    } else {
        echo '-';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "promoted:ChildState:1001:1:string:0:-\n",
            "visible:ParentState:1000:1:int:1:3\n",
            "shared:ParentState:0110:1:string:1:'x'\n",
        )
    );
}

#[test]
fn reflection_property_clone_and_wire_serialization_stay_forbidden() {
    assert_eq!(
        run_php(
            r#"<?php
class StoredState { public int $value = 1; }
$property = new ReflectionProperty(StoredState::class, 'value');
foreach (['clone', 'serialize', 'unserialize'] as $operation) {
    try {
        if ($operation === 'clone') {
            $copy = clone $property;
        } elseif ($operation === 'serialize') {
            serialize($property);
        } else {
            unserialize('O:18:"ReflectionProperty":2:{s:4:"name";s:5:"value";s:5:"class";s:11:"StoredState";}');
        }
    } catch (Throwable $error) {
        echo $operation, '=', get_class($error), ':', $error->getMessage(), "\n";
    }
}
echo $property->getName(), ':', $property->class;
"#,
        ),
        concat!(
            "clone=Error:Trying to clone an uncloneable object of class ReflectionProperty\n",
            "serialize=Exception:Serialization of 'ReflectionProperty' is not allowed\n",
            "unserialize=Exception:Unserialization of 'ReflectionProperty' is not allowed\n",
            "value:StoredState",
        )
    );
}

#[test]
fn repeated_enum_property_reflections_release_stale_sidecar_owners() {
    assert_eq!(
        run_php(
            r#"<?php
enum RepeatedState: string { case Ready = 'r'; }
for ($index = 0; $index < 40; $index++) {
    $properties = (new ReflectionClass(RepeatedState::class))->getProperties();
    $property = $properties[$index % 2];
    if (count(get_object_vars($property)) !== 2 || $property->class !== RepeatedState::class) {
        echo 'bad';
    }
    unset($properties, $property);
}
$final = (new ReflectionClass(RepeatedState::class))->getProperties();
echo $final[0]->getName(), ':', $final[0]->getType(), ':';
echo $final[1]->getName(), ':', $final[1]->getType(), ':';
echo json_encode($final[0]), ':', RepeatedState::Ready->value;
"#,
        ),
        "name:string:value:string:{\"name\":\"name\",\"class\":\"RepeatedState\"}:r"
    );
}

#[test]
fn reflection_property_print_and_export_use_the_public_projection() {
    assert_eq!(
        run_php(
            r#"<?php
class VisibleState { public int $value = 1; }
$property = new ReflectionProperty(VisibleState::class, 'value');
echo print_r($property, true);
var_export($property);
"#,
        ),
        concat!(
            "ReflectionProperty Object\n",
            "(\n",
            "    [name] => value\n",
            "    [class] => VisibleState\n",
            ")\n",
            "\\ReflectionProperty::__set_state(array(\n",
            "   'name' => 'value',\n",
            "   'class' => 'VisibleState',\n",
            "))",
        )
    );
}

#[test]
fn dynamic_reflection_property_keeps_method_state_out_of_its_public_projection() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class DynamicState { public $declared = 1; }
$object = new DynamicState;
$object->dynamic = 2;
$property = new ReflectionProperty($object, 'dynamic');
echo implode(',', array_keys(get_object_vars($property))), ':';
echo $property->name, ':', $property->class, ':';
echo $property->getModifiers(), (int) $property->isPublic(), (int) $property->isDefault(), ':';
echo (int) $property->hasType(), (int) $property->hasDefaultValue(), ':';
echo json_encode($property);
"#,
        ),
        "name,class:dynamic:DynamicState:110:00:{\"name\":\"dynamic\",\"class\":\"DynamicState\"}"
    );
}

#[test]
fn enum_property_filters_and_case_reads_keep_their_state() {
    assert_eq!(
        run_php(
            r#"<?php
enum FilteredState: int { case Ready = 4; }
$reflection = new ReflectionClass(FilteredState::class);
foreach ([ReflectionProperty::IS_PUBLIC, ReflectionProperty::IS_STATIC, ReflectionProperty::IS_READONLY] as $filter) {
    echo implode(',', array_map(
        fn(ReflectionProperty $property) => $property->getName(),
        $reflection->getProperties($filter),
    )), ';';
}
echo FilteredState::Ready->name, ':', FilteredState::Ready->value;
"#,
        ),
        "name,value;;name,value;Ready:4"
    );
}
