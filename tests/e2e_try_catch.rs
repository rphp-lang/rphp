/// Tests for try/catch/finally
mod common;
use common::{
    run_php, run_php_expect_error, run_php_expect_error_with_source_context,
    run_php_with_source_context,
};

include!("e2e_try_catch/basic_flow.rs");

include!("e2e_try_catch/finally_control_flow.rs");

include!("e2e_try_catch/error_hierarchy.rs");

include!("e2e_try_catch/throw_validation.rs");

#[test]
fn ordinary_return_keeps_another_frames_pending_finally_exception() {
    assert_eq!(
        run_php(include_str!("fixtures/try_return_empty_state.php")),
        "2\n3\n4\nouter\n9\n5\n"
    );
}
