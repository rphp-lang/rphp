/// Tests for interface support and visibility enforcement
mod common;
use common::{run_php, run_php_expect_error};

include!("e2e_interface/basics_and_throwable.rs");
include!("e2e_interface/visibility_and_instantiation.rs");
include!("e2e_interface/contracts_and_private_scope.rs");
include!("e2e_interface/parameter_compatibility.rs");
include!("e2e_interface/return_compatibility.rs");
