pub mod codegen;
pub mod optimizer;
pub mod target;

pub use codegen::Compiler;
pub use optimizer::optimize;

use anyhow::Result;
use std::path::PathBuf;
use crate::parser::Program;

pub fn generate_executable(ast: Program, output: PathBuf, target: Option<String>) -> Result<()> {
    codegen::generate_executable(ast, output, target)
}
