/// E2E tests: hot tier — promotion, bailout, and tier transition correctness.
///
/// These tests exercise the tiering mechanics in `hot.rs`:
/// - Promotion: functions crossing call threshold become Hot
/// - Bailout: hot executor returns to baseline on unsupported patterns
/// - Correctness: hot path produces identical results to baseline
/// - Eligibility: ineligible functions (typed params, globals, generators) stay Cold
///
/// Test strategy: functions are called >FUNC_HOT_THRESHOLD times to ensure
/// both cold and hot paths are exercised. Correctness is verified by output.
mod common;
use common::run_php;
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::function::ScalarLongOpKind;
use rphp::vm::function::{CallStrategy, ComposedScalarLongOp, ObjectLongOp, ScalarLongCallGuard};

include!("e2e_hot_tier/promotion_and_scalar_plans.rs");
include!("e2e_hot_tier/bailout_and_transitions.rs");
include!("e2e_hot_tier/property_tier.rs");
include!("e2e_hot_tier/method_tier.rs");
