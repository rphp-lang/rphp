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
