#[test]
fn argument_unpack_crosses_dynamic_method_static_and_constructor_boundaries() {
    let output = run_php(
        r#"<?php
class ParcelSpread {
    public function __construct(public string $name, public int $size) {}
    public function describe(string $prefix, ...$parts): string {
        return $prefix . ':' . implode(',', $parts);
    }
    public static function combine(string $left, string $right): string {
        return $left . '-' . $right;
    }
}
function parcelParts() { yield 4; yield 5; }

$parcel = new ParcelSpread(...['size' => 3, 'name' => 'box']);
echo $parcel->describe('M', ...parcelParts()), '|';
echo ParcelSpread::combine(...['L', 'R']), '|';
$dynamic = [$parcel, 'describe'];
echo $dynamic(...['D', 8, 9]);
"#,
    );

    assert_eq!(output, "M:4,5|L-R|D:8,9");
}

#[test]
fn array_unpack_references_detach_copies_and_update_each_original_segment() {
    let output = run_php(
        r#"<?php
function raiseSlots(&...$slots): void {
    foreach ($slots as &$slot) { $slot += 10; }
}
$left = [1, 2];
$snapshot = $left;
$right = [3];
raiseSlots(...$left, ...$right);
echo $left[0], ',', $left[1], '|', $snapshot[0], ',', $snapshot[1], '|', $right[0];
"#,
    );

    assert_eq!(output, "11,12|1,2|13");
}

#[test]
fn unpack_validates_iterator_keys_and_preserves_iterator_exceptions() {
    let output = run_php(
        r#"<?php
function receiveSpread(...$values): void {}
function invalidSpreadKeys() { yield [] => 'bad'; }
function interruptedSpread() { yield 1; throw new Exception('stream-stopped'); }

try { receiveSpread(...invalidSpreadKeys()); }
catch (Error $error) { echo $error->getMessage(), '|'; }
try { receiveSpread(...interruptedSpread()); }
catch (Exception $error) { echo $error->getMessage(), '|'; }
try { receiveSpread(...false); }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );

    assert_eq!(
        output,
        "Keys must be of type int|string during argument unpacking|stream-stopped|Only arrays and Traversables can be unpacked, bool given"
    );
}

#[test]
fn unpack_preserves_named_variadics_and_internal_null_mapping() {
    let output = run_php(
        r#"<?php
function ledgerSpread(string $head, ...$entries): string {
    $rendered = [];
    foreach ($entries as $key => $value) { $rendered[] = $key . '=' . $value; }
    return $head . ':' . implode(',', $rendered);
}
echo ledgerSpread(...['head' => 'H'], alpha: 1, beta: 2), '|';
echo json_encode(array_map(null, ...[[2, 4], [3, 5]]));
"#,
    );

    assert_eq!(output, "H:alpha=1,beta=2|[[2,3],[4,5]]");
}
