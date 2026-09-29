use super::*;
use crate::compiler::compile::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::generator::{Generator, GeneratorState, new_generator_ref};

#[test]
fn force_close_discards_trace_link_before_reusing_frame() {
    let tokens = Lexer::new("<?php function suspendedValue($value) { yield $value; }")
        .tokenize()
        .unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let main = crate::compiler::make_user_function(compiled.main);
    let function = &compiled.functions[0].1;
    for started in [false, true] {
        for current_site in [false, true] {
            let mut eg = ExecutorGlobals::new();
            let caller = eg.vm_stack.push_call_frame(
                &main.common,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // SAFETY: this test owns the compiler-sized caller and keeps
            // its descriptor alive until the final explicit stack pop.
            unsafe { (*caller).opline = main.op_array.instructions.as_ptr() };
            eg.current_execute_data.set(caller);
            let generator = new_generator_ref(Generator::new(
                &function.common,
                vec![Value::long(1)],
                function.op_array.num_cvs,
                function.op_array.num_temps,
            ));
            if started {
                assert!(matches!(
                    resume_generator(&mut eg, &generator, Value::null()).unwrap(),
                    GeneratorResumeOutcome::Advanced,
                ));
                assert_eq!(generator.borrow().state, GeneratorState::Suspended);
            }
            force_close_generator(&mut eg, &generator, current_site).unwrap();
            assert_eq!(generator.borrow().state, GeneratorState::Completed);
            assert_eq!(eg.current_execute_data.get(), caller);

            // Reuse the just-retired generator's allocation. A logical
            // trace link must never migrate to this unrelated activation.
            let reused = eg.vm_stack.push_call_frame(
                &function.common,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(
                eg.trace_caller(reused as usize, std::ptr::null_mut())
                    .is_null(),
                "retired generator trace survives: started={started}, current_site={current_site}"
            );
            assert!(!eg.detached_trace_caller_is_current_site(reused as usize));
            // SAFETY: both test frames own only initialized scalar/undef
            // slots and are retired in the reverse allocation order.
            unsafe {
                cleanup_frame_slots(reused);
                cleanup_frame_slots(caller);
            }
            pop_vm_call_frame(&mut eg, reused);
            eg.current_execute_data.set(std::ptr::null_mut());
            pop_vm_call_frame(&mut eg, caller);
        }
    }
}
