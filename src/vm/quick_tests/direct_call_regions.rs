    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    #[test]
    fn builds_one_indirect_scalar_abi_for_property_and_argument_closures() {
        let tokens = Lexer::new(
            "<?php
class Transform {
    public $callback;
    public function apply($value) {
        $callback = $this->callback;
        return $callback($value);
    }
}
function invokeClosure(Closure $callback, int $value): int {
    return $callback($value);
}
",
        )
        .tokenize()
        .unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();

        let apply = compilation.class_defs[0]
            .methods
            .iter()
            .find(|(name, ..)| name == "apply")
            .map(|(_, _, _, _, method)| method)
            .unwrap();
        let apply_plan = apply.indirect_scalar_long_plan().unwrap();
        assert_eq!(apply_plan.public_args, 1);
        assert!(matches!(
            apply_plan.callable,
            crate::vm::function::IndirectScalarLongCallable::ReceiverProperty { cache_ip: 0 }
        ));
        assert_eq!(apply_plan.arguments.as_ref(), [ScalarLongSource::Input(0)]);

        let invoke = compilation
            .functions
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("invokeClosure"))
            .map(|(_, function)| function)
            .unwrap();
        let invoke_plan = invoke.indirect_scalar_long_plan().unwrap();
        assert_eq!(invoke_plan.public_args, 2);
        assert!(matches!(
            invoke_plan.callable,
            crate::vm::function::IndirectScalarLongCallable::PublicArgument(0)
        ));
        assert_eq!(invoke_plan.arguments.as_ref(), [ScalarLongSource::Input(1)]);
    }

    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    #[test]
    fn builds_captured_closure_plan_and_dead_alias_call_region() {
        let tokens = Lexer::new(
            "<?php
function invokeCaptured(Closure $callback, int $value): int {
    return $callback($value);
}
function runCaptured(int $iterations): int {
    $prefix = 'kept';
    $offset = 7;
    $callback = static function (int $value) use ($prefix, $offset): int {
        return strlen($prefix) + $offset + $value;
    };
    $sum = 0;
    for ($index = 0; $index < $iterations; ++$index) {
        $copy = $callback;
        $sum += invokeCaptured($copy, $index & 255);
    }
    return $sum;
}
",
        )
        .tokenize()
        .unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let compilation = Compiler::new().compile(&statements).unwrap();

        let closure = compilation
            .functions
            .iter()
            .find(|(name, _)| name.starts_with("__closure_"))
            .map(|(_, function)| function)
            .unwrap();
        let captured = closure.captured_typed_long_plan().unwrap();
        assert_eq!(captured.public_args, 1);
        assert_eq!(captured.capture_count, 2);
        assert_eq!(captured.long_input_mask, 0b101);
        assert_eq!(captured.string_input_mask, 0b010);

        let run = compilation
            .functions
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("runCaptured"))
            .map(|(_, function)| function)
            .unwrap();
        assert!(run.op_array.block_plans.iter().any(|plan| {
            matches!(plan, BlockPlan::QuickLongOps(plan) if plan.ops.iter().any(|operation| {
                matches!(operation, QuickLongOp::IndirectScalarFunctionCall { .. })
            }))
        }));
    }
