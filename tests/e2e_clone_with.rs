mod common;
use common::run_php;

#[test]
fn clone_with_updates_in_array_order_after_clone_hook() {
    assert_eq!(
        run_php(
            r#"<?php
class CloneWithOrder {
    public string $first = 'initial' {
        set { echo "set:$value\n"; $this->first = strtoupper($value); }
    }
    public string $second = 'initial';
    public function __clone() { echo "clone\n"; $this->second = 'from-clone'; }
}
function source() { echo "source\n"; return new CloneWithOrder(); }
function updates() { echo "updates\n"; return ['first' => 'one', 'second' => 'two']; }
$copy = clone(source(), updates());
echo $copy->first, ':', $copy->second, "\n";
$plain = clone(new stdClass(), ['zero', 'named' => 'value'],);
echo $plain->{0}, ':', $plain->named, "\n";
$same = clone(new stdClass(),);
echo $same::class, "\n";
"#,
        ),
        "source\nupdates\nclone\nset:one\nONE:two\nzero:value\nstdClass\n"
    );
}

#[test]
fn clone_with_uses_ordinary_visibility_types_and_property_hooks() {
    assert_eq!(
        run_php(
            r#"<?php
class CloneWithVisibility {
    protected string $hidden = 'old';
    public int $number = 1;
    public string $hooked = 'old' {
        set { $this->hooked = strtoupper($value); }
    }
    public function inside() { return clone($this, ['hidden' => 'new']); }
    public function hidden() { return $this->hidden; }
}
$value = new CloneWithVisibility();
echo $value->inside()->hidden(), "\n";
try { clone($value, ['hidden' => 'bad']); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { clone($value, ['number' => []]); } catch (TypeError $e) { echo $e::class, "\n"; }
echo (clone($value, ['hooked' => 'changed']))->hooked, "\n";
"#,
        ),
        "new\nCannot access protected property CloneWithVisibility::$hidden\nTypeError\nCHANGED\n"
    );
}

#[test]
fn clone_with_validates_array_and_live_references_before_cloning() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class CloneWithValidation {
    public function __clone() { echo "cloned\n"; }
}
$source = new CloneWithValidation();
try { clone($source, 42); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$reference = 'kept';
$updates = ['value' => &$reference];
try { clone($source, $updates); } catch (Error $e) { echo $e->getMessage(), "\n"; }
unset($reference);
$copy = clone($source, $updates);
echo $copy->value, "\n";
try { clone(42); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "clone(): Argument #2 ($withProperties) must be of type array, int given\n",
            "Cannot assign by reference when cloning with updated properties\n",
            "cloned\nkept\n",
            "clone(): Argument #1 ($object) must be of type object, int given\n",
        )
    );
}
