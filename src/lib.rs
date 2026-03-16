#![allow(unsafe_op_in_unsafe_fn)]
#![allow(unused_imports)]

pub mod value;
pub mod vm;
pub mod compiler;
pub mod runtime;
pub mod lexer;
pub mod parser;
#[allow(unused_unsafe)]
pub mod stdlib;
