use anyhow::Result;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassManager;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::{AddressSpace, OptimizationLevel};
use std::collections::HashMap;
use std::path::Path;
use tryzub_parser::{
    Program, Declaration, Statement, Expression, Literal, BinaryOp, UnaryOp,
    Type, Parameter, Visibility, AssignmentOp,
};

pub struct Compiler<'ctx> {
    context: &'ctx Context,
    builder: Builder<'ctx>,
    module: Module<'ctx>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    variables: HashMap<String, PointerValue<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        
        Self {
            context,
            builder,
            module,
            functions: HashMap::new(),
            variables: HashMap::new(),
            current_function: None,
        }
    }
    
    pub fn compile(&mut self, program: Program) -> Result<()> {
        // Спочатку декларуємо всі функції
        for decl in &program.declarations {
            if let Declaration::Function { name, params, return_type, .. } = decl {
                self.declare_function(name, params, return_type)?;
            }
        }
        
        // Потім компілюємо їх тіла
        for decl in program.declarations {
            self.compile_declaration(decl)?;
        }
        
        // Додаємо точку входу якщо є функція "головна"
        if self.functions.contains_key("головна") {
            self.create_main_wrapper()?;
        }
        
        Ok(())
    }
    
    fn declare_function(&mut self, name: &str, params: &[Parameter], return_type: &Option<Type>) -> Result<()> {
        let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
            .map(|p| self.get_llvm_type(&p.ty).into())
            .collect();
        
        let fn_type = if let Some(ret_ty) = return_type {
            let ret_type = self.get_llvm_type(ret_ty);
            ret_type.fn_type(&param_types, false)
        } else {
            self.context.void_type().fn_type(&param_types, false)
        };
        
        let function = self.module.add_function(name, fn_type, None);
        self.functions.insert(name.to_string(), function);
        
        Ok(())
    }
    
    fn create_main_wrapper(&mut self) -> Result<()> {
        let i32_type = self.context.i32_type();
        let main_type = i32_type.fn_type(&[], false);
        let main_fn = self.module.add_function("main", main_type, None);
        
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        
        // Викликаємо функцію "головна"
        let головна = self.functions.get("головна").unwrap();
        self.builder.build_call(*головна, &[], "call");
        
        // Повертаємо 0
        let zero = i32_type.const_int(0, false);
        self.builder.build_return(Some(&zero));
        
        Ok(())
    }
    
    fn compile_declaration(&mut self, decl: Declaration) -> Result<()> {
        match decl {
            Declaration::Variable { name, ty, value, is_mutable } => {
                let llvm_type = if let Some(ref t) = ty {
                    self.get_llvm_type(t)
                } else if let Some(ref val) = value {
                    self.infer_type_from_expression(val)
                } else {
                    return Err(anyhow::anyhow!("Не можу вивести тип змінної {}", name));
                };
                
                let alloca = self.builder.build_alloca(llvm_type, &name);
                
                if let Some(init_value) = value {
                    let value = self.compile_expression(init_value)?;
                    self.builder.build_store(alloca, value);
                }
                
                self.variables.insert(name, alloca);
            }
            
            Declaration::Function { name, params, return_type, body, .. } => {
                let function = *self.functions.get(&name).unwrap();
                self.current_function = Some(function);
                
                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);
                
                // Створюємо змінні для параметрів
                self.variables.clear();
                for (i, param) in params.iter().enumerate() {
                    let arg = function.get_nth_param(i as u32).unwrap();
                    let alloca = self.builder.build_alloca(arg.get_type(), &param.name);
                    self.builder.build_store(alloca, arg);
                    self.variables.insert(param.name.clone(), alloca);
                }
                
                // Компілюємо тіло функції
                let mut has_return = false;
                for stmt in body {
                    if matches!(stmt, Statement::Return(_)) {
                        has_return = true;
                    }
                    self.compile_statement(stmt)?;
                }
                
                // Додаємо неявний return якщо його немає
                if !has_return && return_type.is_none() {
                    self.builder.build_return(None);
                }
            }
            
            Declaration::Struct { .. } => {
                // TODO: Implement struct compilation
            }
            
            _ => {
                // TODO: Implement other declarations
            }
        }
        
        Ok(())
    }
    
    fn compile_statement(&mut self, stmt: Statement) -> Result<()> {
        match stmt {
            Statement::Expression(expr) => {
                self.compile_expression(expr)?;
            }
            
            Statement::Return(value) => {
                if let Some(expr) = value {
                    let val = self.compile_expression(expr)?;
                    self.builder.build_return(Some(&val));
                } else {
                    self.builder.build_return(None);
                }
            }
            
            Statement::Block(statements) => {
                for stmt in statements {
                    self.compile_statement(stmt)?;
                }
            }
            
            Statement::If { condition, then_branch, else_branch } => {
                let cond_value = self.compile_expression(condition)?;
                let cond = self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    cond_value.into_int_value(),
                    self.context.bool_type().const_zero(),
                    "ifcond"
                );
                
                let function = self.current_function.unwrap();
                let then_bb = self.context.append_basic_block(function, "then");
                let else_bb = self.context.append_basic_block(function, "else");
                let cont_bb = self.context.append_basic_block(function, "ifcont");
                
                self.builder.build_conditional_branch(cond, then_bb, else_bb);
                
                // Then branch
                self.builder.position_at_end(then_bb);
                self.compile_statement(*then_branch)?;
                self.builder.build_unconditional_branch(cont_bb);
                
                // Else branch
                self.builder.position_at_end(else_bb);
                if let Some(else_stmt) = else_branch {
                    self.compile_statement(*else_stmt)?;
                }
                self.builder.build_unconditional_branch(cont_bb);
                
                // Continue
                self.builder.position_at_end(cont_bb);
            }
            
            Statement::While { condition, body } => {
                let function = self.current_function.unwrap();
                let loop_bb = self.context.append_basic_block(function, "loop");
                let after_bb = self.context.append_basic_block(function, "afterloop");
                
                self.builder.build_unconditional_branch(loop_bb);
                self.builder.position_at_end(loop_bb);
                
                let cond_value = self.compile_expression(condition)?;
                let cond = self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    cond_value.into_int_value(),
                    self.context.bool_type().const_zero(),
                    "loopcond"
                );
                
                let body_bb = self.context.append_basic_block(function, "loopbody");
                self.builder.build_conditional_branch(cond, body_bb, after_bb);
                
                self.builder.position_at_end(body_bb);
                self.compile_statement(*body)?;
                self.builder.build_unconditional_branch(loop_bb);
                
                self.builder.position_at_end(after_bb);
            }
            
            Statement::For { variable, from, to, step, body } => {
                // Створюємо змінну циклу
                let i32_type = self.context.i32_type();
                let loop_var = self.builder.build_alloca(i32_type, &variable);
                
                // Ініціалізуємо змінну
                let from_value = self.compile_expression(from)?;
                self.builder.build_store(loop_var, from_value);
                self.variables.insert(variable.clone(), loop_var);
                
                // Створюємо блоки
                let function = self.current_function.unwrap();
                let loop_bb = self.context.append_basic_block(function, "loop");
                let body_bb = self.context.append_basic_block(function, "loopbody");
                let inc_bb = self.context.append_basic_block(function, "loopinc");
                let after_bb = self.context.append_basic_block(function, "afterloop");
                
                self.builder.build_unconditional_branch(loop_bb);
                
                // Перевірка умови
                self.builder.position_at_end(loop_bb);
                let current = self.builder.build_load(loop_var, "current").into_int_value();
                let to_value = self.compile_expression(to)?.into_int_value();
                let cond = self.builder.build_int_compare(
                    inkwell::IntPredicate::SLT,
                    current,
                    to_value,
                    "loopcond"
                );
                self.builder.build_conditional_branch(cond, body_bb, after_bb);
                
                // Тіло циклу
                self.builder.position_at_end(body_bb);
                self.compile_statement(*body)?;
                self.builder.build_unconditional_branch(inc_bb);
                
                // Інкремент
                self.builder.position_at_end(inc_bb);
                let step_value = if let Some(step_expr) = step {
                    self.compile_expression(step_expr)?.into_int_value()
                } else {
                    i32_type.const_int(1, false)
                };
                let new_value = self.builder.build_int_add(current, step_value, "nextval");
                self.builder.build_store(loop_var, new_value);
                self.builder.build_unconditional_branch(loop_bb);
                
                self.builder.position_at_end(after_bb);
                self.variables.remove(&variable);
            }
            
            Statement::Assignment { target, value, op } => {
                if let Expression::Identifier(name) = target {
                    let ptr = self.variables.get(&name)
                        .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                    
                    let new_value = match op {
                        AssignmentOp::Assign => self.compile_expression(value)?,
                        AssignmentOp::AddAssign => {
                            let current = self.builder.build_load(*ptr, "current");
                            let add_value = self.compile_expression(value)?;
                            self.builder.build_int_add(
                                current.into_int_value(),
                                add_value.into_int_value(),
                                "addtmp"
                            ).into()
                        }
                        AssignmentOp::SubAssign => {
                            let current = self.builder.build_load(*ptr, "current");
                            let sub_value = self.compile_expression(value)?;
                            self.builder.build_int_sub(
                                current.into_int_value(),
                                sub_value.into_int_value(),
                                "subtmp"
                            ).into()
                        }
                        AssignmentOp::MulAssign => {
                            let current = self.builder.build_load(*ptr, "current");
                            let mul_value = self.compile_expression(value)?;
                            self.builder.build_int_mul(
                                current.into_int_value(),
                                mul_value.into_int_value(),
                                "multmp"
                            ).into()
                        }
                        AssignmentOp::DivAssign => {
                            let current = self.builder.build_load(*ptr, "current");
                            let div_value = self.compile_expression(value)?;
                            self.builder.build_int_signed_div(
                                current.into_int_value(),
                                div_value.into_int_value(),
                                "divtmp"
                            ).into()
                        }
                    };
                    
                    self.builder.build_store(*ptr, new_value);
                } else {
                    return Err(anyhow::anyhow!("Присвоєння можливе тільки до змінних"));
                }
            }
            
            _ => {
                // TODO: Implement other statements
            }
        }
        
        Ok(())
    }
    
    fn compile_expression(&mut self, expr: Expression) -> Result<BasicValueEnum<'ctx>> {
        match expr {
            Expression::Literal(lit) => self.compile_literal(lit),
            
            Expression::Identifier(name) => {
                if name == "друк" {
                    // Створюємо декларацію printf якщо її ще немає
                    let printf = self.get_or_create_printf();
                    Ok(printf.as_global_value().as_pointer_value().into())
                } else if let Some(ptr) = self.variables.get(&name) {
                    Ok(self.builder.build_load(*ptr, &name))
                } else {
                    Err(anyhow::anyhow!("Невідома змінна: {}", name))
                }
            }
            
            Expression::Binary { left, op, right } => {
                let lhs = self.compile_expression(*left)?;
                let rhs = self.compile_expression(*right)?;
                
                match op {
                    BinaryOp::Add => {
                        if lhs.is_int_value() {
                            Ok(self.builder.build_int_add(
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "addtmp"
                            ).into())
                        } else {
                            Ok(self.builder.build_float_add(
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "faddtmp"
                            ).into())
                        }
                    }
                    BinaryOp::Sub => {
                        if lhs.is_int_value() {
                            Ok(self.builder.build_int_sub(
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "subtmp"
                            ).into())
                        } else {
                            Ok(self.builder.build_float_sub(
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "fsubtmp"
                            ).into())
                        }
                    }
                    BinaryOp::Mul => {
                        if lhs.is_int_value() {
                            Ok(self.builder.build_int_mul(
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "multmp"
                            ).into())
                        } else {
                            Ok(self.builder.build_float_mul(
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "fmultmp"
                            ).into())
                        }
                    }
                    BinaryOp::Div => {
                        if lhs.is_int_value() {
                            Ok(self.builder.build_int_signed_div(
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "divtmp"
                            ).into())
                        } else {
                            Ok(self.builder.build_float_div(
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "fdivtmp"
                            ).into())
                        }
                    }
                    BinaryOp::Lt => {
                        let cmp = if lhs.is_int_value() {
                            self.builder.build_int_compare(
                                inkwell::IntPredicate::SLT,
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "cmptmp"
                            )
                        } else {
                            self.builder.build_float_compare(
                                inkwell::FloatPredicate::OLT,
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "fcmptmp"
                            )
                        };
                        Ok(self.builder.build_int_z_extend(
                            cmp,
                            self.context.i32_type(),
                            "booltmp"
                        ).into())
                    }
                    BinaryOp::Gt => {
                        let cmp = if lhs.is_int_value() {
                            self.builder.build_int_compare(
                                inkwell::IntPredicate::SGT,
                                lhs.into_int_value(),
                                rhs.into_int_value(),
                                "cmptmp"
                            )
                        } else {
                            self.builder.build_float_compare(
                                inkwell::FloatPredicate::OGT,
                                lhs.into_float_value(),
                                rhs.into_float_value(),
                                "fcmptmp"
                            )
                        };
                        Ok(self.builder.build_int_z_extend(
                            cmp,
                            self.context.i32_type(),
                            "booltmp"
                        ).into())
                    }
                    _ => Err(anyhow::anyhow!("Оператор {:?} ще не реалізований", op)),
                }
            }
            
            Expression::Unary { op, operand } => {
                let val = self.compile_expression(*operand)?;
                
                match op {
                    UnaryOp::Neg => {
                        if val.is_int_value() {
                            Ok(self.builder.build_int_neg(val.into_int_value(), "negtmp").into())
                        } else {
                            Ok(self.builder.build_float_neg(val.into_float_value(), "fnegtmp").into())
                        }
                    }
                    UnaryOp::Not => {
                        let cmp = self.builder.build_int_compare(
                            inkwell::IntPredicate::EQ,
                            val.into_int_value(),
                            self.context.bool_type().const_zero(),
                            "nottmp"
                        );
                        Ok(self.builder.build_int_z_extend(
                            cmp,
                            self.context.i32_type(),
                            "booltmp"
                        ).into())
                    }
                }
            }
            
            Expression::Call { callee, args } => {
                if let Expression::Identifier(name) = *callee {
                    if name == "друк" {
                        // Спеціальна обробка для друку
                        self.compile_print_call(args)
                    } else if let Some(function) = self.functions.get(&name) {
                        let mut arg_values = Vec::new();
                        for arg in args {
                            arg_values.push(self.compile_expression(arg)?.into());
                        }
                        Ok(self.builder.build_call(*function, &arg_values, "calltmp")
                            .try_as_basic_value()
                            .left()
                            .unwrap_or_else(|| self.context.i32_type().const_zero().into()))
                    } else {
                        Err(anyhow::anyhow!("Невідома функція: {}", name))
                    }
                } else {
                    Err(anyhow::anyhow!("Непрямі виклики функцій ще не підтримуються"))
                }
            }
            
            _ => Err(anyhow::anyhow!("Вираз {:?} ще не реалізований", expr)),
        }
    }
    
    fn compile_literal(&self, lit: Literal) -> Result<BasicValueEnum<'ctx>> {
        match lit {
            Literal::Integer(n) => Ok(self.context.i32_type().const_int(n as u64, false).into()),
            Literal::Float(f) => Ok(self.context.f64_type().const_float(f).into()),
            Literal::String(s) => {
                let value = self.builder.build_global_string_ptr(&s, "str");
                Ok(value.as_pointer_value().into())
            }
            Literal::Char(c) => Ok(self.context.i8_type().const_int(c as u64, false).into()),
            Literal::Bool(b) => Ok(self.context.bool_type().const_int(b as u64, false).into()),
            Literal::Null => Ok(self.context.i32_type().ptr_type(AddressSpace::Generic).const_null().into()),
        }
    }
    
    fn compile_print_call(&mut self, args: Vec<Expression>) -> Result<BasicValueEnum<'ctx>> {
        let printf = self.get_or_create_printf();
        
        let mut print_args = Vec::new();
        let mut format_string = String::new();
        
        for arg in args {
            let value = self.compile_expression(arg)?;
            
            if value.is_int_value() {
                format_string.push_str("%d");
                print_args.push(value.into());
            } else if value.is_float_value() {
                format_string.push_str("%f");
                print_args.push(value.into());
            } else if value.is_pointer_value() {
                format_string.push_str("%s");
                print_args.push(value.into());
            }
        }
        
        format_string.push('\n');
        let format_str = self.builder.build_global_string_ptr(&format_string, "fmt");
        
        let mut all_args = vec![format_str.as_pointer_value().into()];
        all_args.extend(print_args);
        
        Ok(self.builder.build_call(printf, &all_args, "printf_call")
            .try_as_basic_value()
            .left()
            .unwrap_or_else(|| self.context.i32_type().const_zero().into()))
    }
    
    fn get_or_create_printf(&mut self) -> FunctionValue<'ctx> {
        if let Some(function) = self.module.get_function("printf") {
            function
        } else {
            let i32_type = self.context.i32_type();
            let str_type = self.context.i8_type().ptr_type(AddressSpace::Generic);
            let printf_type = i32_type.fn_type(&[str_type.into()], true);
            self.module.add_function("printf", printf_type, None)
        }
    }
    
    fn get_llvm_type(&self, ty: &Type) -> BasicTypeEnum<'ctx> {
        match ty {
            Type::Цл8 => self.context.i8_type().into(),
            Type::Цл16 => self.context.i16_type().into(),
            Type::Цл32 => self.context.i32_type().into(),
            Type::Цл64 => self.context.i64_type().into(),
            Type::Чс8 => self.context.i8_type().into(),
            Type::Чс16 => self.context.i16_type().into(),
            Type::Чс32 => self.context.i32_type().into(),
            Type::Чс64 => self.context.i64_type().into(),
            Type::Дрб32 => self.context.f32_type().into(),
            Type::Дрб64 => self.context.f64_type().into(),
            Type::Лог => self.context.bool_type().into(),
            Type::Сим => self.context.i8_type().into(),
            Type::Тхт => self.context.i8_type().ptr_type(AddressSpace::Generic).into(),
            Type::Array(elem_ty, size) => {
                let elem_type = self.get_llvm_type(elem_ty);
                elem_type.array_type(*size as u32).into()
            }
            Type::Slice(elem_ty) => {
                let elem_type = self.get_llvm_type(elem_ty);
                elem_type.ptr_type(AddressSpace::Generic).into()
            }
            Type::Reference(inner_ty, _) => {
                let inner_type = self.get_llvm_type(inner_ty);
                inner_type.ptr_type(AddressSpace::Generic).into()
            }
            _ => self.context.i32_type().into(), // Placeholder
        }
    }
    
    fn infer_type_from_expression(&self, expr: &Expression) -> BasicTypeEnum<'ctx> {
        match expr {
            Expression::Literal(Literal::Integer(_)) => self.context.i32_type().into(),
            Expression::Literal(Literal::Float(_)) => self.context.f64_type().into(),
            Expression::Literal(Literal::String(_)) => {
                self.context.i8_type().ptr_type(AddressSpace::Generic).into()
            }
            Expression::Literal(Literal::Char(_)) => self.context.i8_type().into(),
            Expression::Literal(Literal::Bool(_)) => self.context.bool_type().into(),
            _ => self.context.i32_type().into(), // Default
        }
    }
    
    pub fn generate_object_file(&self, path: &Path, opt_level: u8) -> Result<()> {
        Target::initialize_all(&InitializationConfig::default());
        
        let target_triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&target_triple)?;
        let target_machine = target
            .create_target_machine(
                &target_triple,
                "generic",
                "",
                self.get_opt_level(opt_level),
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| anyhow::anyhow!("Не вдалося створити target machine"))?;
        
        target_machine.write_to_file(&self.module, FileType::Object, path)?;
        
        Ok(())
    }
    
    fn get_opt_level(&self, level: u8) -> OptimizationLevel {
        match level {
            0 => OptimizationLevel::None,
            1 => OptimizationLevel::Less,
            2 => OptimizationLevel::Default,
            _ => OptimizationLevel::Aggressive,
        }
    }
}

pub fn optimize(ast: Program, opt_level: u8) -> Result<Program> {
    // TODO: Implement AST optimizations
    Ok(ast)
}

pub fn generate_executable(ast: Program, output: std::path::PathBuf, _target: Option<String>) -> Result<()> {
    let context = Context::create();
    let mut compiler = Compiler::new(&context, "tryzub_module");
    
    compiler.compile(ast)?;
    
    // Генеруємо об'єктний файл
    let obj_path = output.with_extension("o");
    compiler.generate_object_file(&obj_path, 2)?;
    
    // Лінкуємо в виконуваний файл
    let status = std::process::Command::new("clang")
        .args(&[
            obj_path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-lm", // Математична бібліотека
        ])
        .status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!("Помилка лінкування"));
    }
    
    // Видаляємо тимчасовий об'єктний файл
    std::fs::remove_file(obj_path)?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tryzub_lexer::tokenize;
    use tryzub_parser::parse;
    
    #[test]
    fn test_compile_simple_function() {
        let source = r#"
функція головна() {
    змінна x: цл32 = 10
    змінна y: цл32 = 20
    друк(x + y)
}
"#;
        
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        
        let context = Context::create();
        let mut compiler = Compiler::new(&context, "test");
        
        assert!(compiler.compile(program).is_ok());
    }
}
