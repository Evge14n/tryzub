use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Module, Linkage, FuncId};
use std::collections::HashMap;
use tryzub_parser::*;

pub struct CraneliftCompiler {
    builder_ctx: FunctionBuilderContext,
    ctx: codegen::Context,
    module: JITModule,
    functions: HashMap<String, FuncId>,
}

impl CraneliftCompiler {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("помилка host ISA: {}", msg);
        });
        let isa = isa_builder.finish(settings::Flags::new(flag_builder)).unwrap();
        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        builder.symbol("__tryzub_print", tryzub_print_i64 as *const u8);
        let module = JITModule::new(builder);
        Self {
            builder_ctx: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            module,
            functions: HashMap::new(),
        }
    }

    pub fn compile_and_run(mut self, program: &Program) -> anyhow::Result<()> {
        let mut func_decls: Vec<(String, Vec<String>, Vec<Statement>)> = Vec::new();
        for decl in &program.declarations {
            if let Declaration::Function { name, params, body, .. } = decl {
                let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                func_decls.push((name.clone(), param_names, body.clone()));
            }
        }

        for (name, params, _) in &func_decls {
            let mut sig = self.module.make_signature();
            for _ in params {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module.declare_function(name, Linkage::Local, &sig)?;
            self.functions.insert(name.clone(), func_id);
        }

        let print_sig = {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig
        };
        let print_id = self.module.declare_function("__tryzub_print", Linkage::Import, &print_sig)?;
        self.functions.insert("друк".to_string(), print_id);

        for (name, params, body) in &func_decls {
            self.compile_function(name, params, body)?;
        }

        self.module.finalize_definitions().map_err(|e| anyhow::anyhow!("{}", e))?;

        if let Some(main_id) = self.functions.get("головна") {
            let code_ptr = self.module.get_finalized_function(*main_id);
            let func = unsafe { std::mem::transmute::<_, fn() -> i64>(code_ptr) };
            func();
        }
        Ok(())
    }

    fn compile_function(&mut self, name: &str, params: &[String], body: &[Statement]) -> anyhow::Result<()> {
        let func_id = *self.functions.get(name).unwrap();

        let sig = {
            let decl = self.module.declarations().get_function_decl(func_id);
            decl.signature.clone()
        };
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let mut env = FuncEnv::new();
            for (i, param_name) in params.iter().enumerate() {
                let val = builder.block_params(entry_block)[i];
                let var = env.declare_var(param_name);
                builder.declare_var(var, types::I64);
                builder.def_var(var, val);
            }

            let functions = self.functions.clone();
            let mut translator = FuncTranslator {
                builder: &mut builder,
                module: &mut self.module,
                env: &mut env,
                functions: &functions,
                returned: false,
            };

            translator.translate_body(body);

            if !translator.returned {
                let zero = translator.builder.ins().iconst(types::I64, 0);
                translator.builder.ins().return_(&[zero]);
            }

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)
            .map_err(|e| anyhow::anyhow!("помилка компіляції {}: {}", name, e))?;
        self.module.clear_context(&mut self.ctx);
        Ok(())
    }
}

struct FuncEnv {
    vars: HashMap<String, Variable>,
    next_var: usize,
}

impl FuncEnv {
    fn new() -> Self { Self { vars: HashMap::new(), next_var: 0 } }

    fn declare_var(&mut self, name: &str) -> Variable {
        if let Some(v) = self.vars.get(name) { return *v; }
        let var = Variable::new(self.next_var);
        self.next_var += 1;
        self.vars.insert(name.to_string(), var);
        var
    }

    fn get_var(&self, name: &str) -> Option<Variable> {
        self.vars.get(name).copied()
    }
}

struct FuncTranslator<'a, 'b> {
    builder: &'a mut FunctionBuilder<'b>,
    module: &'a mut JITModule,
    env: &'a mut FuncEnv,
    functions: &'a HashMap<String, FuncId>,
    returned: bool,
}

impl<'a, 'b> FuncTranslator<'a, 'b> {
    fn translate_body(&mut self, stmts: &[Statement]) {
        for (i, stmt) in stmts.iter().enumerate() {
            if self.returned { return; }
            if i == stmts.len() - 1 {
                if let Statement::Expression(expr) = stmt {
                    let val = self.translate_expr(expr);
                    self.builder.ins().return_(&[val]);
                    self.returned = true;
                    return;
                }
            }
            self.translate_stmt(stmt);
        }
    }

    fn translate_stmt(&mut self, stmt: &Statement) {
        if self.returned { return; }
        match stmt {
            Statement::Return(Some(expr)) => {
                let val = self.translate_expr(expr);
                self.builder.ins().return_(&[val]);
                self.returned = true;
            }
            Statement::Return(None) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.builder.ins().return_(&[zero]);
                self.returned = true;
            }
            Statement::Expression(expr) => {
                self.translate_expr(expr);
            }
            Statement::Declaration(Declaration::Variable { name, value, .. }) => {
                let var = self.env.declare_var(name);
                self.builder.declare_var(var, types::I64);
                let val = if let Some(expr) = value {
                    self.translate_expr(expr)
                } else {
                    self.builder.ins().iconst(types::I64, 0)
                };
                self.builder.def_var(var, val);
            }
            Statement::Assignment { target, value, op } => {
                if let Expression::Identifier(name) = target {
                    if let Some(var) = self.env.get_var(name) {
                        let new_val = self.translate_expr(value);
                        let final_val = match op {
                            AssignmentOp::Assign => new_val,
                            AssignmentOp::AddAssign => {
                                let old = self.builder.use_var(var);
                                self.builder.ins().iadd(old, new_val)
                            }
                            AssignmentOp::SubAssign => {
                                let old = self.builder.use_var(var);
                                self.builder.ins().isub(old, new_val)
                            }
                            AssignmentOp::MulAssign => {
                                let old = self.builder.use_var(var);
                                self.builder.ins().imul(old, new_val)
                            }
                            AssignmentOp::DivAssign => {
                                let old = self.builder.use_var(var);
                                self.builder.ins().sdiv(old, new_val)
                            }
                            AssignmentOp::ModAssign => {
                                let old = self.builder.use_var(var);
                                self.builder.ins().srem(old, new_val)
                            }
                        };
                        self.builder.def_var(var, final_val);
                    }
                }
            }
            Statement::If { condition, then_branch, else_branch } => {
                let cond_val = self.translate_expr(condition);
                let then_block = self.builder.create_block();
                let else_block = self.builder.create_block();
                let merge_block = self.builder.create_block();

                let cond_bool = self.builder.ins().icmp_imm(IntCC::NotEqual, cond_val, 0);
                self.builder.ins().brif(cond_bool, then_block, &[], else_block, &[]);

                self.builder.switch_to_block(then_block);
                self.builder.seal_block(then_block);
                self.translate_single_stmt(then_branch);
                if !self.returned {
                    self.builder.ins().jump(merge_block, &[]);
                }
                let then_returned = self.returned;
                self.returned = false;

                self.builder.switch_to_block(else_block);
                self.builder.seal_block(else_block);
                if let Some(else_stmt) = else_branch {
                    self.translate_single_stmt(else_stmt);
                }
                if !self.returned {
                    self.builder.ins().jump(merge_block, &[]);
                }
                let else_returned = self.returned;
                self.returned = false;

                if then_returned && else_returned {
                    self.returned = true;
                } else {
                    self.builder.switch_to_block(merge_block);
                    self.builder.seal_block(merge_block);
                }
            }
            Statement::While { condition, body } => {
                let header = self.builder.create_block();
                let body_block = self.builder.create_block();
                let exit = self.builder.create_block();

                self.builder.ins().jump(header, &[]);
                self.builder.switch_to_block(header);

                let cond_val = self.translate_expr(condition);
                let cond_bool = self.builder.ins().icmp_imm(IntCC::NotEqual, cond_val, 0);
                self.builder.ins().brif(cond_bool, body_block, &[], exit, &[]);

                self.builder.switch_to_block(body_block);
                self.builder.seal_block(body_block);
                self.translate_single_stmt(body);
                if !self.returned {
                    self.builder.ins().jump(header, &[]);
                }
                self.returned = false;
                self.builder.seal_block(header);

                self.builder.switch_to_block(exit);
                self.builder.seal_block(exit);
            }
            Statement::For { variable, from, to, body, .. } => {
                let var = self.env.declare_var(variable);
                self.builder.declare_var(var, types::I64);
                let from_val = self.translate_expr(from);
                self.builder.def_var(var, from_val);

                let header = self.builder.create_block();
                let body_block = self.builder.create_block();
                let exit = self.builder.create_block();

                self.builder.ins().jump(header, &[]);
                self.builder.switch_to_block(header);

                let to_val = self.translate_expr(to);
                let i_val = self.builder.use_var(var);
                let cond = self.builder.ins().icmp(IntCC::SignedLessThan, i_val, to_val);
                self.builder.ins().brif(cond, body_block, &[], exit, &[]);

                self.builder.switch_to_block(body_block);
                self.builder.seal_block(body_block);
                self.translate_single_stmt(body);
                let cur = self.builder.use_var(var);
                let one = self.builder.ins().iconst(types::I64, 1);
                let next = self.builder.ins().iadd(cur, one);
                self.builder.def_var(var, next);
                self.builder.ins().jump(header, &[]);
                self.builder.seal_block(header);

                self.builder.switch_to_block(exit);
                self.builder.seal_block(exit);
            }
            Statement::Block(stmts) => {
                self.translate_body(stmts);
            }
            _ => {}
        }
    }

    fn translate_single_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Block(stmts) => self.translate_body(stmts),
            _ => self.translate_stmt(stmt),
        }
    }

    fn translate_expr(&mut self, expr: &Expression) -> Value {
        match expr {
            Expression::Literal(Literal::Integer(n)) => {
                self.builder.ins().iconst(types::I64, *n)
            }
            Expression::Literal(Literal::Bool(b)) => {
                self.builder.ins().iconst(types::I64, *b as i64)
            }
            Expression::Identifier(name) => {
                if let Some(var) = self.env.get_var(name) {
                    self.builder.use_var(var)
                } else {
                    self.builder.ins().iconst(types::I64, 0)
                }
            }
            Expression::Binary { left, op, right } => {
                let lhs = self.translate_expr(left);
                let rhs = self.translate_expr(right);
                match op {
                    BinaryOp::Add => self.builder.ins().iadd(lhs, rhs),
                    BinaryOp::Sub => self.builder.ins().isub(lhs, rhs),
                    BinaryOp::Mul => self.builder.ins().imul(lhs, rhs),
                    BinaryOp::Div => self.builder.ins().sdiv(lhs, rhs),
                    BinaryOp::Mod => self.builder.ins().srem(lhs, rhs),
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let cc = match op {
                            BinaryOp::Eq => IntCC::Equal,
                            BinaryOp::Ne => IntCC::NotEqual,
                            BinaryOp::Lt => IntCC::SignedLessThan,
                            BinaryOp::Le => IntCC::SignedLessThanOrEqual,
                            BinaryOp::Gt => IntCC::SignedGreaterThan,
                            BinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
                            _ => IntCC::Equal,
                        };
                        let c = self.builder.ins().icmp(cc, lhs, rhs);
                        self.builder.ins().uextend(types::I64, c)
                    }
                    BinaryOp::And => self.builder.ins().band(lhs, rhs),
                    BinaryOp::Or => self.builder.ins().bor(lhs, rhs),
                    _ => self.builder.ins().iconst(types::I64, 0),
                }
            }
            Expression::Unary { op, operand } => {
                let val = self.translate_expr(operand);
                match op {
                    UnaryOp::Neg => self.builder.ins().ineg(val),
                    UnaryOp::Not => {
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let c = self.builder.ins().icmp(IntCC::Equal, val, zero);
                        self.builder.ins().uextend(types::I64, c)
                    }
                    _ => val,
                }
            }
            Expression::Call { callee, args } => {
                if let Expression::Identifier(name) = callee.as_ref() {
                    if let Some(func_id) = self.functions.get(name.as_str()) {
                        let local_callee = self.module.declare_func_in_func(*func_id, self.builder.func);
                        let arg_vals: Vec<Value> = args.iter().map(|a| self.translate_expr(a)).collect();
                        let call = self.builder.ins().call(local_callee, &arg_vals);
                        return self.builder.inst_results(call)[0];
                    }
                }
                self.builder.ins().iconst(types::I64, 0)
            }
            _ => self.builder.ins().iconst(types::I64, 0),
        }
    }
}

extern "C" fn tryzub_print_i64(val: i64) -> i64 {
    println!("{}", val);
    0
}
