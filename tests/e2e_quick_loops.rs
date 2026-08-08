mod common;

use common::run_php;

include!("e2e_quick_loops/basic_loops.rs");
include!("e2e_quick_loops/scalar_and_object_calls.rs");
include!("e2e_quick_loops/conditional_kernels.rs");
include!("e2e_quick_loops/array_reads.rs");
include!("e2e_quick_loops/array_mutations.rs");
include!("e2e_quick_loops/hash_kernels.rs");
include!("e2e_quick_loops/foreach_and_double.rs");
