pub mod opcode;
pub mod instruction;
pub mod function;
pub mod frame;
pub mod stack;
pub mod execute;
pub mod generator;
pub mod stats;
pub mod planner;
pub mod hot;
pub mod quick;
#[cfg(feature = "quick-loops")]
mod quick_foreach;
