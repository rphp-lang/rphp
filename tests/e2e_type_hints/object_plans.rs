#[test]
fn test_exact_declared_object_argument_skips_repeated_boundary_validation() {
    let result = compile_types(
        r#"<?php
class Payload {}
class Service {
    function consume(Payload $payload): array { return []; }
    function forward(Payload $payload): array {
        return $this->consume($payload);
    }
}
"#,
    );
    let service = result
        .class_defs
        .iter()
        .find(|class| class.name == "Service")
        .unwrap();
    let consume = service
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "consume")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let forward = service
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "forward")
        .map(|(_, _, _, _, function)| function)
        .unwrap();

    assert_eq!(consume.common.plan.call, CallStrategy::Fast);
    assert_eq!(consume.common.plan.ret, ReturnStrategy::Fast);
    assert!(consume.common.plan.borrow_this());
    assert!(forward.op_array.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
    }));
}

#[test]
fn test_typed_object_property_long_method_gets_guarded_plan() {
    let result = compile_types(
        r#"<?php
class QuoteRequest {
    public int $level;
    public int $subtotal;
}
class DiscountPolicy {
    public function rate(QuoteRequest $request): int {
        $rate = 150;
        if ($request->level >= 3) {
            $rate = $rate + 250;
        }
        if ($request->subtotal >= 20000) {
            $rate = $rate + 175;
        }
        return $rate;
    }
}
class TaxPolicy {
    public function amount(int $net, string $region): int {
        if ($region == 'EU') return intdiv($net * 2100, 10000);
        if ($region == 'US') return intdiv($net * 725, 10000);
        return intdiv($net * 1200, 10000);
    }
}
"#,
    );
    let policy = result
        .class_defs
        .iter()
        .find(|class| class.name == "DiscountPolicy")
        .unwrap();
    let rate = policy
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "rate")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let plan = rate
        .object_long_plan
        .as_deref()
        .expect("typed property-reading Long plan");
    assert_eq!(plan.public_args, 1);
    assert_eq!(plan.object_argument_mask, 1);
    assert_eq!(plan.long_argument_mask, 0);

    let tax = result
        .class_defs
        .iter()
        .find(|class| class.name == "TaxPolicy")
        .unwrap()
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "amount")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let tax_plan = tax
        .object_long_plan
        .as_deref()
        .expect("typed String-guarded intdiv plan");
    assert_eq!(tax_plan.long_argument_mask, 1);
    assert_eq!(tax_plan.string_argument_mask, 2);
    assert!(tax_plan.string_intdiv_select.is_some());
    assert!(
        tax_plan.operations.iter().any(|operation| {
            matches!(operation, rphp::vm::function::ObjectLongOp::IntDiv { .. })
        })
    );
}

#[test]
fn test_small_object_array_method_composes_guarded_long_calls() {
    let result = compile_types(
        r#"<?php
class Request {
    public $subtotal = 0;
    public function __construct(int $subtotal) { $this->subtotal = $subtotal; }
}
class Policy {
    public function rate(Request $request): int { return $request->subtotal; }
}
class Service {
    public $policy;
    public function __construct(Policy $policy) { $this->policy = $policy; }
    public function quote(Request $request): array {
        $rate = $this->policy->rate($request);
        $net = $request->subtotal - $rate;
        return ['net' => $net, 'gross' => $net + 1];
    }
}
"#,
    );
    let quote = result
        .class_defs
        .iter()
        .find(|class| class.name == "Service")
        .unwrap()
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "quote")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let plan = quote
        .object_array_plan
        .as_deref()
        .expect("guarded object/Long array plan");
    assert_eq!(plan.public_args, 1);
    assert_eq!(plan.entries.len(), 2);
    assert!(
        plan.operations.iter().any(|operation| {
            matches!(operation, rphp::vm::function::ObjectArrayLongOp::Call(_))
        })
    );

    assert_eq!(
        run_php(
            r#"<?php
class Request {
    public $subtotal = 0;
    public function __construct(int $subtotal) { $this->subtotal = $subtotal; }
}
class Policy {
    public function rate(Request $request): int { return $request->subtotal - 2; }
}
class Service {
    public $policy;
    public function __construct(Policy $policy) { $this->policy = $policy; }
    public function quote(Request $request): array {
        $rate = $this->policy->rate($request);
        $net = $request->subtotal - $rate;
        return ['net' => $net, 'gross' => $net + 1];
    }
}
$service = new Service(new Policy());
$request = new Request(12);
$result = [];
for ($i = 0; $i < 40; $i++) { $result = $service->quote($request); }
echo $result['net'] . ':' . $result['gross'];
"#
        ),
        "2:3"
    );
}

#[test]
fn test_object_array_region_side_exits_on_polymorphic_nested_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Request { public $subtotal = 7; }
class Policy {
    public function rate(Request $request): int { return $request->subtotal; }
}
class LoudPolicy extends Policy {
    public function rate(Request $request): int {
        echo '!';
        return $request->subtotal + 5;
    }
}
class Service {
    public $policy;
    public function __construct($policy) { $this->policy = $policy; }
    public function quote(Request $request): array {
        $rate = $this->policy->rate($request);
        return ['value' => $rate];
    }
}
$service = new Service(new Policy());
$request = new Request();
for ($i = 0; $i < 30; $i++) { $service->quote($request); }
$service->policy = new LoudPolicy();
$result = $service->quote($request);
echo $result['value'];
"#
        ),
        "!12"
    );
}

#[test]
fn test_object_array_region_side_exits_before_overflowed_array_result() {
    assert_eq!(
        run_php(
            r#"<?php
class Request { public $value = 9223372036854775807; }
class Policy {
    public function value(Request $request): int { return $request->value; }
}
class Service {
    public $policy;
    public function __construct(Policy $policy) { $this->policy = $policy; }
    public function collect(Request $request): array {
        $value = $this->policy->value($request);
        return ['value' => $value + 1];
    }
}
$service = new Service(new Policy());
$request = new Request();
for ($i = 0; $i < 30; $i++) { $service->collect($request); }
$result = $service->collect($request);
echo gettype($result['value']);
    "#
        ),
        "double"
    );
}

#[cfg(feature = "quick-loops")]
#[test]
fn test_dead_object_array_result_and_request_get_scalar_pipeline_markers() {
    let source = r#"<?php
class Request {
    public $value = 0;
    public $bonus = 3;
    public function __construct(int $value) { $this->value = $value; }
}
class Policy {
    public function amount(Request $request): int {
        return $request->value + $request->bonus;
    }
}
class Service {
    public $policy;
    public function __construct(Policy $policy) { $this->policy = $policy; }
    public function quote(Request $request): array {
        $value = $this->policy->amount($request);
        return ['value' => $value];
    }
}
function runPipeline(int $iterations): int {
    $service = new Service(new Policy());
    $sum = 0;
    for ($i = 0; $i < $iterations; $i++) {
        $request = new Request(2);
        $result = $service->quote($request);
        $sum = $sum + $result['value'];
    }
    return $sum;
}
echo runPipeline(100);
"#;
    let compiled = compile_types(source);
    let run = compiled
        .functions
        .iter()
        .find(|(name, _)| name == "runPipeline")
        .map(|(_, function)| function)
        .unwrap();
    assert!(run.op_array.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::InitMethodCall
            && instruction._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS != 0
    }));
    assert!(run.op_array.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::NewObj
            && instruction._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE != 0
    }));
    assert!(run.op_array.block_plans.iter().any(|block| {
        matches!(
            block,
            BlockPlan::QuickLongOps(plan)
                if plan.ops.iter().any(|operation| {
                    matches!(operation, QuickLongOp::VirtualObjectArrayPipeline { .. })
                })
        )
    }));
    assert_eq!(run_php(source), "500");
}

#[test]
fn test_virtual_request_pipeline_preserves_nontrivial_constructor_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class Request {
    public $value = 0;
    public function __construct($value) { $this->value = $value + 0; }
}
class Policy {
    public function amount($request) { return $request->value; }
}
class Service {
    public $policy;
    public function __construct($policy) { $this->policy = $policy; }
    public function quote($request) {
        $value = $this->policy->amount($request);
        return ['value' => $value];
    }
}
$service = new Service(new Policy());
$source = 4;
$sum = 0;
for ($i = 0; $i < 50; $i++) {
    $request = new Request($source);
    $result = $service->quote($request);
    $sum = $sum + $result['value'];
}
echo $sum;
"#
        ),
        "200"
    );
}

#[test]
fn test_object_array_consumer_overflow_replays_canonical_addition() {
    assert_eq!(
        run_php(
            r#"<?php
class Request { public $value = 1; }
class Policy { public function amount($request) { return $request->value; } }
class Service {
    public $policy;
    public function __construct($policy) { $this->policy = $policy; }
    public function quote($request) {
        $value = $this->policy->amount($request);
        return ['value' => $value];
    }
}

$service = new Service(new Policy());
$request = new Request();
$sum = 9223372036854775780;
for ($i = 0; $i < 50; $i++) {
    $result = $service->quote($request);
    $sum = $sum + $result['value'];
}
echo gettype($sum);
"#
        ),
        "double"
    );
}

#[test]
fn test_virtual_pipeline_loop_overflow_side_exits_to_canonical_addition() {
    assert_eq!(
        run_php(
            r#"<?php
class Request {
    public $value = 0;
    public function __construct($value) { $this->value = $value; }
}
class Policy { public function amount($request) { return $request->value; } }
class Service {
    public $policy;
    public function __construct($policy) { $this->policy = $policy; }
    public function quote($request) {
        $value = $this->policy->amount($request);
        return ['value' => $value];
    }
}
$service = new Service(new Policy());
$sum = 9223372036854775767;
for ($i = 0; $i < 50; $i++) {
    $request = new Request(1);
    $result = $service->quote($request);
    $sum = $sum + $result['value'];
}
echo gettype($sum);
"#
        ),
        "double"
    );
}

#[test]
fn test_request_and_array_escape_disable_scalar_pipeline_markers() {
    let compiled = compile_types(
        r#"<?php
class Request {
    public $value = 0;
    public function __construct($value) { $this->value = $value; }
}
class Policy { public function amount($request) { return $request->value; } }
class Service {
    public $policy;
    public function __construct($policy) { $this->policy = $policy; }
    public function quote($request) {
        $value = $this->policy->amount($request);
        return ['value' => $value];
    }
}
function escaped($service) {
    $sum = 0;
    $request = new Request(2);
    $result = $service->quote($request);
    $sum = $sum + $result['value'];
    echo $request->value;
    return $result;
}
"#,
    );
    let escaped = compiled
        .functions
        .iter()
        .find(|(name, _)| name == "escaped")
        .map(|(_, function)| function)
        .unwrap();
    assert!(!escaped.op_array.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::InitMethodCall
            && instruction._pad & CALL_FLAG_OBJECT_ARRAY_CONSUMERS != 0
    }));
    assert!(!escaped.op_array.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::NewObj
            && instruction._pad & NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE != 0
    }));
}

#[test]
fn test_monomorphic_class_guard_rechecks_a_different_runtime_class() {
    assert_eq!(
        run_php(
            r#"<?php
class Accepted {}
class ChildAccepted extends Accepted {}
class Rejected {}
function consume(Accepted $value): int { return 1; }
$accepted = new ChildAccepted();
for ($i = 0; $i < 20; $i++) { consume($accepted); }
try { consume(new Rejected()); } catch (TypeError $error) { echo "caught"; }
"#
        ),
        "caught"
    );
}
