#[test]
fn quick_conditional_add_assign_deoptimizes_at_overflow() {
    assert_eq!(
        run_php(
            "<?php
$n = 1000;
$sum = PHP_INT_MAX - 100000;
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $i;
"
        ),
        "float|1000"
    );
}

#[test]
fn quick_foreach_long_accumulation_finishes_packed_array_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
"
        ),
        "500500|1000"
    );
}

#[test]
fn quick_foreach_long_accumulation_finishes_hash_array_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 1000);
$values['last'] = 7;
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo $sum;
echo '|';
echo $value;
"
        ),
        "500507|7"
    );
}

#[test]
fn quick_foreach_long_accumulation_deoptimizes_non_long_value_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$values[80] = 1.5;
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $sum;
echo '|';
echo $value;
"
        ),
        "float|4970.5|100"
    );
}

#[test]
fn quick_foreach_long_accumulation_deoptimizes_overflow_exactly() {
    assert_eq!(
        run_php(
            "<?php
$values = range(1, 100);
$sum = PHP_INT_MAX - 1000;
foreach ($values as $value) {
    $sum += $value;
}
echo is_float($sum) ? 'float' : 'int';
echo '|';
echo $value;
"
        ),
        "float|100"
    );
}

#[test]
fn quick_foreach_object_property_term_overflow_resumes_exact_add() {
    assert_eq!(
        run_php(
            "<?php
class ForeachProjectionOverflowRow {
    public $value;
    public $increment;
    public function __construct($value, $increment) {
        $this->value = $value;
        $this->increment = $increment;
    }
}
$rows = [];
for ($i = 0; $i < 100; $i++) {
    if ($i == 80) {
        $rows[] = new ForeachProjectionOverflowRow(PHP_INT_MAX, 1);
    } else {
        $rows[] = new ForeachProjectionOverflowRow(1, 1);
    }
}
$sum = 0;
foreach ($rows as $row) {
    $sum += $row->value + $row->increment;
}
echo gettype($sum) . '|' . $row->value;
"
        ),
        "double|1"
    );
}

#[test]
fn quick_foreach_object_property_accumulator_overflow_resumes_exact_add() {
    assert_eq!(
        run_php(
            "<?php
class ForeachAccumulatorOverflowRow { public $value = 1; }
$rows = [];
for ($i = 0; $i < 100; $i++) {
    $rows[] = new ForeachAccumulatorOverflowRow();
}
$sum = PHP_INT_MAX - 50;
foreach ($rows as $row) {
    $sum += $row->value;
}
echo gettype($sum) . '|' . $row->value;
"
        ),
        "double|1"
    );
}

#[test]
fn nested_double_method_loop_revalidates_overridden_inner_target() {
    assert_eq!(
        run_php(
            "<?php
class FloatPipeline {
    public function scaleAndShift(float $value, float $scale): float {
        return ($value * $scale) + 1.0;
    }
    public function calculate(float $value, float $scale): float {
        return ($this->scaleAndShift($value, $scale) * 0.5) + 2.0;
    }
}
class ChildPipeline extends FloatPipeline {
    public function scaleAndShift(float $value, float $scale): float {
        return ($value * $scale) + 3.0;
    }
}
function accumulate(FloatPipeline $pipeline): float {
    $scale = 2.0;
    $total = 0.0;
    for ($i = 0; $i < 1000; $i++) {
        $total += $pipeline->calculate($i * 0.5, $scale);
    }
    return $total;
}
$base = new FloatPipeline();
$child = new ChildPipeline();
echo accumulate($base) . ':' . accumulate($child);
"
        ),
        "252250:253250"
    );
}
