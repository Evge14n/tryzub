// AST → Bytecode компілятор для Тризуб
// Перетворює AST дерево в послідовність інструкцій bytecode VM

use tryzub_parser::*;
use super::bytecode::*;

pub struct Compiler {
    chunk: Chunk,
    locals: Vec<String>,
    scope_depth: usize,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
        }
    }

    pub fn compile_program(mut self, program: &Program) -> Chunk {
        for decl in &program.declarations {
            self.compile_declaration(decl);
        }
        self.chunk.emit(Op::Halt, 0);
        self.chunk.local_count = self.locals.len();
        self.chunk
    }

    fn compile_declaration(&mut self, decl: &Declaration) {
        match decl {
            Declaration::Variable { name, value, .. } => {
                if let Some(expr) = value {
                    self.compile_expression(expr);
                } else {
                    let c = self.chunk.add_constant(BcValue::Null);
                    self.chunk.emit(Op::Const, c);
                }
                let slot = self.add_local(name.clone());
                self.chunk.emit(Op::StoreLocal, slot as u32);
            }
            Declaration::Function { name: _, params: _, body, .. } => {
                // Inline компіляція тіла функції
                for stmt in body {
                    self.compile_statement(stmt);
                }
            }
            _ => {}
        }
    }

    fn compile_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Expression(expr) => {
                self.compile_expression(expr);
                self.chunk.emit(Op::Pop, 0);
            }
            Statement::Return(Some(expr)) => {
                self.compile_expression(expr);
                self.chunk.emit(Op::Return, 0);
            }
            Statement::Return(None) => {
                let c = self.chunk.add_constant(BcValue::Null);
                self.chunk.emit(Op::Const, c);
                self.chunk.emit(Op::Return, 0);
            }
            Statement::Block(stmts) => {
                self.scope_depth += 1;
                for s in stmts {
                    self.compile_statement(s);
                }
                self.scope_depth -= 1;
            }
            Statement::If { condition, then_branch, else_branch } => {
                self.compile_expression(condition);
                let jump_false = self.chunk.emit(Op::JumpIfFalse, 0);

                self.compile_statement(then_branch.as_ref());
                let jump_end = self.chunk.emit(Op::Jump, 0);

                let else_target = self.chunk.code.len() as u32;
                self.chunk.patch_jump(jump_false, else_target);

                if let Some(else_stmt) = else_branch {
                    self.compile_statement(else_stmt.as_ref());
                }
                let end_target = self.chunk.code.len() as u32;
                self.chunk.patch_jump(jump_end, end_target);
            }
            Statement::While { condition, body } => {
                let loop_start = self.chunk.code.len();

                self.compile_expression(condition);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse, 0);

                self.compile_statement(body.as_ref());

                let loop_body_size = (self.chunk.code.len() - loop_start + 1) as u32;
                self.chunk.emit(Op::Loop, loop_body_size);

                let exit_target = self.chunk.code.len() as u32;
                self.chunk.patch_jump(exit_jump, exit_target);
            }
            Statement::For { variable, from, to, step: _, body } => {
                self.compile_expression(from);
                let i_slot = self.add_local(variable.clone());
                self.chunk.emit(Op::StoreLocal, i_slot as u32);

                let loop_start = self.chunk.code.len();

                self.chunk.emit(Op::LoadLocal, i_slot as u32);
                self.compile_expression(to);
                self.chunk.emit(Op::Lt, 0);
                let exit_jump = self.chunk.emit(Op::JumpIfFalse, 0);

                self.compile_statement(body.as_ref());

                self.chunk.emit(Op::Inc, i_slot as u32);

                let loop_body_size = (self.chunk.code.len() - loop_start + 1) as u32;
                self.chunk.emit(Op::Loop, loop_body_size);

                let exit_target = self.chunk.code.len() as u32;
                self.chunk.patch_jump(exit_jump, exit_target);
            }
            Statement::Assignment { target, value, op } => {
                match op {
                    AssignmentOp::Assign => {
                        self.compile_expression(value);
                        if let Expression::Identifier(name) = target {
                            if let Some(slot) = self.resolve_local(name) {
                                self.chunk.emit(Op::StoreLocal, slot as u32);
                            }
                        }
                    }
                    AssignmentOp::AddAssign => {
                        if let Expression::Identifier(name) = target {
                            if let Some(slot) = self.resolve_local(name) {
                                self.compile_expression(value);
                                self.chunk.emit(Op::AddAssign, slot as u32);
                            }
                        }
                    }
                    _ => {
                        // Для інших ops: load, compute, store
                        if let Expression::Identifier(name) = target {
                            if let Some(slot) = self.resolve_local(name) {
                                self.chunk.emit(Op::LoadLocal, slot as u32);
                                self.compile_expression(value);
                                match op {
                                    AssignmentOp::SubAssign => self.chunk.emit(Op::Sub, 0),
                                    AssignmentOp::MulAssign => self.chunk.emit(Op::Mul, 0),
                                    AssignmentOp::DivAssign => self.chunk.emit(Op::Div, 0),
                                    AssignmentOp::ModAssign => self.chunk.emit(Op::Mod, 0),
                                    _ => self.chunk.emit(Op::Add, 0),
                                };
                                self.chunk.emit(Op::StoreLocal, slot as u32);
                            }
                        }
                    }
                }
            }
            Statement::Declaration(decl) => {
                self.compile_declaration(decl);
            }
            _ => {}
        }
    }

    fn compile_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Literal(lit) => {
                let c = match lit {
                    Literal::Integer(n) => self.chunk.add_constant(BcValue::Int(*n)),
                    Literal::Float(f) => self.chunk.add_constant(BcValue::Float(*f)),
                    Literal::String(s) => self.chunk.add_constant(BcValue::Str(s.clone().into_boxed_str())),
                    Literal::Bool(b) => self.chunk.add_constant(BcValue::Bool(*b)),
                    Literal::Char(ch) => self.chunk.add_constant(BcValue::Str(ch.to_string().into_boxed_str())),
                    Literal::Null => self.chunk.add_constant(BcValue::Null),
                };
                self.chunk.emit(Op::Const, c);
            }
            Expression::Identifier(name) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.chunk.emit(Op::LoadLocal, slot as u32);
                } else {
                    // Невідома змінна — null
                    let c = self.chunk.add_constant(BcValue::Null);
                    self.chunk.emit(Op::Const, c);
                }
            }
            Expression::Binary { left, op, right } => {
                self.compile_expression(left);
                self.compile_expression(right);
                match op {
                    BinaryOp::Add => self.chunk.emit(Op::Add, 0),
                    BinaryOp::Sub => self.chunk.emit(Op::Sub, 0),
                    BinaryOp::Mul => self.chunk.emit(Op::Mul, 0),
                    BinaryOp::Div => self.chunk.emit(Op::Div, 0),
                    BinaryOp::Mod => self.chunk.emit(Op::Mod, 0),
                    BinaryOp::Pow => self.chunk.emit(Op::Pow, 0),
                    BinaryOp::Eq => self.chunk.emit(Op::Eq, 0),
                    BinaryOp::Ne => self.chunk.emit(Op::Ne, 0),
                    BinaryOp::Lt => self.chunk.emit(Op::Lt, 0),
                    BinaryOp::Le => self.chunk.emit(Op::Le, 0),
                    BinaryOp::Gt => self.chunk.emit(Op::Gt, 0),
                    BinaryOp::Ge => self.chunk.emit(Op::Ge, 0),
                    BinaryOp::And => self.chunk.emit(Op::And, 0),
                    BinaryOp::Or => self.chunk.emit(Op::Or, 0),
                    BinaryOp::BitAnd => self.chunk.emit(Op::BitAnd, 0),
                    BinaryOp::BitOr => self.chunk.emit(Op::BitOr, 0),
                    BinaryOp::BitXor => self.chunk.emit(Op::BitXor, 0),
                    BinaryOp::Shl => self.chunk.emit(Op::Shl, 0),
                    BinaryOp::Shr => self.chunk.emit(Op::Shr, 0),
                    _ => self.chunk.emit(Op::Add, 0),
                };
            }
            Expression::Unary { op, operand } => {
                self.compile_expression(operand);
                match op {
                    UnaryOp::Neg => self.chunk.emit(Op::Neg, 0),
                    UnaryOp::Not => self.chunk.emit(Op::Not, 0),
                    UnaryOp::BitNot => self.chunk.emit(Op::BitNot, 0),
                };
            }
            Expression::Call { callee, args } => {
                if let Expression::Identifier(name) = callee.as_ref() {
                    if name == "друк" && args.len() == 1 {
                        self.compile_expression(&args[0]);
                        self.chunk.emit(Op::Print, 0);
                        let c = self.chunk.add_constant(BcValue::Null);
                        self.chunk.emit(Op::Const, c);
                        return;
                    }
                }
                let c = self.chunk.add_constant(BcValue::Null);
                self.chunk.emit(Op::Const, c);
            }
            _ => {
                let c = self.chunk.add_constant(BcValue::Null);
                self.chunk.emit(Op::Const, c);
            }
        }
    }

    fn add_local(&mut self, name: String) -> usize {
        if let Some(idx) = self.locals.iter().position(|n| n == &name) {
            return idx;
        }
        self.locals.push(name);
        self.locals.len() - 1
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals.iter().position(|n| n == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tryzub_lexer::tokenize;
    use tryzub_parser::parse;

    fn compile_and_run(source: &str) -> BcValue {
        let tokens = tokenize(source).unwrap();
        let ast = parse(tokens).unwrap();
        let compiler = Compiler::new();
        let chunk = compiler.compile_program(&ast);
        let mut vm = BytecodeVM::new(chunk.local_count);
        vm.execute(&chunk)
    }

    #[test]
    fn test_compile_variable() {
        let result = compile_and_run("стала х = 42");
        // Halt returns last popped value
    }

    #[test]
    fn test_compile_arithmetic() {
        let result = compile_and_run("стала х = 2 + 3 * 4");
        // x = 14
    }

    #[test]
    fn test_compile_sum_loop() {
        let (result, duration) = crate::bytecode::benchmark_sum_bytecode(10001);
        assert_eq!(result, 50005000);
        println!("Handwritten bytecode sum 1..10000: {:.3} мс", duration.as_secs_f64() * 1000.0);
    }
}
