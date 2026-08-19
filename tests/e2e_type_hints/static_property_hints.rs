#[test]
fn typed_static_properties_initialize_coerce_and_keep_warm_checks() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedStaticValues {
    public static int $number;
    public static float $ratio = 1;
    public static string $label = "ready";
    public static ?int $optional = null;
    public static mixed $anything;
}

try { echo TypedStaticValues::$anything; } catch (Error $error) { echo "uninitialized:"; }
TypedStaticValues::$number = "40";
TypedStaticValues::$label = 42;
for ($i = 0; $i < 4; $i++) { TypedStaticValues::$number = $i; }
echo TypedStaticValues::$number . ":" . TypedStaticValues::$ratio . ":";
echo TypedStaticValues::$label . ":";
var_dump(TypedStaticValues::$optional);
"#,
        ),
        "uninitialized:3:1:42:NULL\n"
    );
}

#[test]
fn strict_typed_static_writes_throw_before_mutating_storage() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
class StrictTypedStatic {
    public static int $number = 7;
    public static float $ratio;
}

try { StrictTypedStatic::$number = "8"; } catch (TypeError $error) { echo "type:"; }
StrictTypedStatic::$ratio = 2;
echo StrictTypedStatic::$number . ":" . StrictTypedStatic::$ratio;
"#,
        ),
        "type:7:2"
    );
}

#[test]
fn typed_static_object_union_and_nullable_contracts_use_declaring_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticNode {
    public static self $current;
    public static self|null $optional = null;
}
class StaticNodeChild extends StaticNode {}
class StaticOther {}

StaticNodeChild::$current = new StaticNodeChild();
StaticNode::$optional = new StaticNode();
try { StaticNode::$current = new StaticOther(); } catch (TypeError $error) { echo "type:"; }
echo get_class(StaticNode::$current) . ":" . get_class(StaticNode::$optional);
"#,
        ),
        "type:StaticNodeChild:StaticNode"
    );
}

#[test]
fn typed_property_defaults_are_validated_during_compilation() {
    let error = run_php_expect_error(
        r#"<?php
class InvalidTypedStaticDefault {
    public static int $number = "1";
}
"#,
    );
    assert_eq!(
        error.to_string(),
        "Cannot use string as default value for property InvalidTypedStaticDefault::$number of type int on line 3"
    );
}

#[test]
fn trait_property_types_participate_in_composition_compatibility() {
    let error = run_php_expect_error(
        r#"<?php
trait TypedStaticTraitInt { public static int $value = 1; }
trait TypedStaticTraitString { public static string $value = "1"; }
class TypedStaticTraitConsumer {
    use TypedStaticTraitInt, TypedStaticTraitString;
}
"#,
    );
    let rendered = format!("{error:?}");
    assert!(rendered.contains("definition differs"), "{rendered}");
}

#[test]
fn inherited_mutable_property_types_are_invariant() {
    let static_error = run_php_expect_error(
        r#"<?php
class StaticPropertyParent { public static int $value; }
class StaticPropertyChild extends StaticPropertyParent { public static string $value; }
"#,
    );
    let rendered = format!("{static_error:?}");
    assert!(
        rendered.contains("Type of StaticPropertyChild::$value must be int"),
        "{rendered}"
    );

    let instance_error = run_php_expect_error(
        r#"<?php
class InstancePropertyParent { public int $value; }
class InstancePropertyChild extends InstancePropertyParent { public string $value; }
"#,
    );
    let rendered = format!("{instance_error:?}");
    assert!(
        rendered.contains("Type of InstancePropertyChild::$value must be int"),
        "{rendered}"
    );

    let self_error = run_php_expect_error(
        r#"<?php
class SelfPropertyParent { public static self $value; }
class SelfPropertyChild extends SelfPropertyParent { public static self $value; }
"#,
    );
    let rendered = format!("{self_error:?}");
    assert!(
        rendered.contains("Type of SelfPropertyChild::$value must be SelfPropertyParent"),
        "{rendered}"
    );

    assert_eq!(
        run_php(
            r#"<?php
class UnionPropertyParent { public static int|string|null $value; }
class UnionPropertyChild extends UnionPropertyParent { public static string|int|null $value; }
UnionPropertyChild::$value = "ok";
echo UnionPropertyChild::$value;
"#,
        ),
        "ok"
    );
}

#[test]
fn inherited_properties_preserve_staticness_and_visibility() {
    let staticness_error = run_php_expect_error(
        r#"<?php
class NonStaticPropertyParent { public int $value; }
class StaticPropertyChild extends NonStaticPropertyParent { public static int $value; }
"#,
    );
    let rendered = format!("{staticness_error:?}");
    assert!(rendered.contains("Cannot redeclare non static"), "{rendered}");

    let visibility_error = run_php_expect_error(
        r#"<?php
class PublicPropertyParent { public static int $value; }
class ProtectedPropertyChild extends PublicPropertyParent { protected static int $value; }
"#,
    );
    let rendered = format!("{visibility_error:?}");
    assert!(rendered.contains("Access level"), "{rendered}");

    assert_eq!(
        run_php(
            r#"<?php
class ProtectedPropertyParent { protected static int $value = 1; }
class PublicPropertyChild extends ProtectedPropertyParent { public static int $value = 2; }
echo PublicPropertyChild::$value;
"#,
        ),
        "2"
    );
}

#[test]
fn static_properties_cannot_be_readonly() {
    let error = run_php_expect_error(
        r#"<?php
class InvalidStaticReadonlyProperty { public static readonly int $value; }
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Static property InvalidStaticReadonlyProperty::$value cannot be readonly"),
        "{rendered}"
    );

    let trait_error = run_php_expect_error(
        r#"<?php
trait InvalidStaticReadonlyTrait { public static readonly int $value; }
"#,
    );
    let rendered = format!("{trait_error:?}");
    assert!(
        rendered.contains("Static property InvalidStaticReadonlyTrait::$value cannot be readonly"),
        "{rendered}"
    );
}
