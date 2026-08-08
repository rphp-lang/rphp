/// Tests for array_map, array_filter and array_reduce callback invocation.
mod common;
use common::{run_php, run_php_expect_error};

include!("e2e_array_map_filter/map_basics.rs");

include!("e2e_array_map_filter/filter_and_pipelines.rs");

include!("e2e_array_map_filter/combined_usage_and_errors.rs");

include!("e2e_array_map_filter/error_hierarchy.rs");
