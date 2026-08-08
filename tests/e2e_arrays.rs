/// E2E tests: arrays — literals, access, assignment, push, nested, functions.
mod common;
use common::run_php;

include!("e2e_arrays/basic_operations.rs");

include!("e2e_arrays/copy_on_write.rs");

include!("e2e_arrays/mutation_and_hot_paths.rs");
