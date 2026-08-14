pub(crate) mod callback_pipeline;
pub mod execute;
pub mod frame;
pub mod function;
pub mod generator;
pub mod hot;
pub mod instruction;
pub mod opcode;
pub mod planner;
pub mod quick;
#[cfg(feature = "quick-loops")]
mod quick_foreach;
mod quick_foreach_plan;
pub mod stack;
pub mod stats;
pub(crate) mod trace;
pub(crate) mod virtual_aggregate_cache;
