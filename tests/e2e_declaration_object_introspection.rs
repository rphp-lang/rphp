mod common;

use common::{run_php, run_php_expect_error, run_php_with_source_context};

#[test]
fn duplicate_parameter_names_are_rejected_at_declaration_time() {
    let error = run_php_expect_error("<?php function repeated($value, $value) {}");
    assert!(
        error
            .to_string()
            .contains("Redefinition of parameter $value"),
        "{error}"
    );
}

#[test]
fn promoted_properties_preserve_references_and_trait_declarations() {
    assert_eq!(
        run_php(
            r#"<?php
class DirectPromotion {
    public function __construct(public array &$items) {}
}
$items = [];
$direct = new DirectPromotion($items);
$items[] = 41;
var_dump($direct->items);

trait PromotedConstructor {
    public function __construct(public int $value) {}
}
class UsesPromotion { use PromotedConstructor; }
$trait = new UsesPromotion(42);
var_dump($trait, property_exists($trait, 'value'));
"#,
        ),
        concat!(
            "array(1) {\n  [0]=>\n  int(41)\n}\n",
            "object(UsesPromotion)#2 (1) {\n  [\"value\"]=>\n  int(42)\n}\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn reflection_renders_user_parameter_defaults_and_inheritance_context() {
    let output = run_php_with_source_context(
        r#"<?php
interface FirstContract { public function inherited(); public function local(); }
interface LastContract { public function local(); }
abstract class ParentDeclaration { public function inherited() {} }
class ConcreteDeclaration extends ParentDeclaration implements FirstContract, LastContract {
    public function __construct(public string $name = '', $suffix = '') {}
    public function local() {}
}
echo new ReflectionClass(ConcreteDeclaration::class);
"#,
        "/virtual/declaration-introspection.php",
        "/virtual",
    );
    assert!(
        output.contains(
            "class ConcreteDeclaration extends ParentDeclaration implements FirstContract, LastContract"
        ),
        "{output}"
    );
    assert!(
        output.contains("<user, prototype LastContract>"),
        "{output}"
    );
    assert!(
        output.contains("<user, inherits ParentDeclaration, prototype FirstContract>"),
        "{output}"
    );
    assert!(output.contains("string $name = ''"), "{output}");
    assert!(output.contains("$suffix = ''"), "{output}");
}

#[test]
fn this_array_elements_do_not_publish_reference_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceProbe {
    public function inspect(): void {
        $values = [&$this];
        var_dump(ReflectionReference::fromArrayElement($values, 0));
    }
}
(new ReferenceProbe)->inspect();
"#,
        ),
        "NULL\n"
    );
}

#[test]
fn object_projections_use_php_key_and_declaration_order() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class ProjectionBase { public $base = 1; protected $guarded = 2; private $hidden = 3; }
#[AllowDynamicProperties]
class ProjectionChild extends ProjectionBase { private $child = 4; }
$object = new ProjectionChild;
$object->dynamic = 5;
$object->{'6'} = 6;
var_export(get_mangled_object_vars($object)); echo "\n";
var_export((array) (object) [7, 8]); echo "\n";

#[AllowDynamicProperties]
class CustomArrayObject extends ArrayObject { private $secret = 9; }
$arrayObject = new CustomArrayObject(['backing' => 10]);
$arrayObject->dynamic = 11;
var_export(get_mangled_object_vars($arrayObject)); echo "\n";
"#,
        ),
        concat!(
            "array (\n",
            "  'base' => 1,\n",
            "  '' . \"\\0\" . '*' . \"\\0\" . 'guarded' => 2,\n",
            "  '' . \"\\0\" . 'ProjectionBase' . \"\\0\" . 'hidden' => 3,\n",
            "  '' . \"\\0\" . 'ProjectionChild' . \"\\0\" . 'child' => 4,\n",
            "  'dynamic' => 5,\n",
            "  6 => 6,\n",
            ")\n",
            "array (\n  0 => 7,\n  1 => 8,\n)\n",
            "array (\n",
            "  '' . \"\\0\" . 'CustomArrayObject' . \"\\0\" . 'secret' => 9,\n",
            "  'dynamic' => 11,\n",
            ")\n",
        )
    );
}

#[test]
fn object_comparison_reads_ancestor_slots_before_child_slots() {
    assert_eq!(
        run_php(
            r#"<?php
class OrderedBase { public $first; }
class OrderedChild extends OrderedBase { public $second; }
$left = new OrderedChild; $left->first = 1; $left->second = 9;
$right = new OrderedChild; $right->first = 2; $right->second = 0;
var_dump($left < $right);
"#,
        ),
        "bool(true)\n"
    );
}

#[test]
fn property_exists_reports_literal_boolean_type_names() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([true, false] as $value) {
    try { property_exists($value, 'x'); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, true given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, false given\n",
        )
    );
}
