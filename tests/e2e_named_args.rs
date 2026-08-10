/// Tests for named arguments (PHP 8 style)
mod common;
use common::run_php;

include!("e2e_named_args/basic_calls.rs");

include!("e2e_named_args/references_and_internal_functions.rs");

include!("e2e_named_args/duplicates_keywords_and_variadics.rs");

include!("e2e_named_args/variadic_errors_and_recovery.rs");

include!("e2e_named_args/reused_and_nested_frames.rs");
