pub mod ast;
pub mod parser;
pub mod error;

pub use ast::*;
pub use parser::{Parser, parse};
pub use error::ParseError;

pub fn format_ast(_ast: Program) -> anyhow::Result<String> {
    // TODO: Implement AST formatting
    Ok("// Відформатований код\n".to_string())
}
