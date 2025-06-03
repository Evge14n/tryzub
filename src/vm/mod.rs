pub mod interpreter;
pub mod value;
pub mod environment;

pub use interpreter::VM;
pub use value::Value;

use anyhow::Result;
use crate::parser::Program;

pub fn execute(program: Program, args: Vec<String>) -> Result<()> {
    let mut vm = VM::new();
    vm.execute_program(program, args)
}
