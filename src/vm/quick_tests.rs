#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::compiler::make_user_function;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::function::{
        ComposedScalarDoubleFunctionPlan, ComposedScalarDoubleOp, ScalarDoubleCall,
        ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
        ScalarDoubleSelect, ScalarDoubleSource, ScalarLongConditionKind,
    };
    use crate::vm::planner::BlockPlan;

    include!("quick_tests/double_plans.rs");
    include!("quick_tests/plan_selection.rs");
    include!("quick_tests/scalar_loops.rs");
    include!("quick_tests/array_and_hash_loops.rs");
    include!("quick_tests/dynamic_control_flow.rs");
    include!("quick_tests/direct_call_regions.rs");
}
