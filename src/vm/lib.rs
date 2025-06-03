// Віртуальна машина мови Тризуб
// Автор: Мартинюк Євген
// Створено: 06.04.2025

use anyhow::Result;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use tryzub_parser::{
    Program, Declaration, Statement, Expression, Literal, BinaryOp, UnaryOp,
    Type, Parameter, AssignmentOp,
};

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Array(Vec<Value>),
    Struct(HashMap<String, Value>),
    Function {
        params: Vec<Parameter>,
        body: Vec<Statement>,
        closure: Environment,
    },
    Null,
}

impl Value {
    fn to_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Integer(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Null => false,
            _ => true,
        }
    }
    
    fn to_string(&self) -> String {
        match self {
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Char(c) => c.to_string(),
            Value::Bool(b) => if *b { "істина" } else { "хиба" }.to_string(),
            Value::Array(arr) => {
                let elements: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                format!("[{}]", elements.join(", "))
            }
            Value::Null => "нуль".to_string(),
            _ => "<значення>".to_string(),
        }
    }
}

type Environment = Rc<RefCell<Scope>>;

#[derive(Debug, Clone)]
struct Scope {
    variables: HashMap<String, Value>,
    parent: Option<Environment>,
}

impl Scope {
    fn new(parent: Option<Environment>) -> Self {
        Self {
            variables: HashMap::new(),
            parent,
        }
    }
    
    fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.variables.get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }
    
    fn set(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }
    
    fn update(&mut self, name: &str, value: Value) -> Result<()> {
        if self.variables.contains_key(name) {
            self.variables.insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().update(name, value)
        } else {
            Err(anyhow::anyhow!("Змінна '{}' не знайдена", name))
        }
    }
}

pub struct VM {
    global_env: Environment,
    current_env: Environment,
    return_value: Option<Value>,
    break_flag: bool,
    continue_flag: bool,
}

impl VM {
    pub fn new() -> Self {
        let global_scope = Rc::new(RefCell::new(Scope::new(None)));
        
        // Додаємо вбудовані функції
        let mut scope = global_scope.borrow_mut();
        
        // Функція для конвертації цілого числа в рядок
        scope.set("цілеврядок".to_string(), Value::Function {
            params: vec![Parameter {
                name: "n".to_string(),
                ty: Type::Цл64,
                default: None,
            }],
            body: vec![],
            closure: global_scope.clone(),
        });
        
        drop(scope);
        
        Self {
            global_env: global_scope.clone(),
            current_env: global_scope,
            return_value: None,
            break_flag: false,
            continue_flag: false,
        }
    }
    
    pub fn execute_program(&mut self, program: Program, _args: Vec<String>) -> Result<()> {
        // Спочатку обробляємо всі декларації
        for decl in &program.declarations {
            self.execute_declaration(decl.clone())?;
        }
        
        // Потім викликаємо функцію "головна" якщо вона є
        if let Some(Value::Function { params, body, closure }) = self.global_env.borrow().get("головна") {
            if !params.is_empty() {
                return Err(anyhow::anyhow!("Функція 'головна' не повинна мати параметрів"));
            }
            
            let prev_env = self.current_env.clone();
            self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));
            
            for stmt in body {
                self.execute_statement(stmt)?;
                if self.return_value.is_some() {
                    break;
                }
            }
            
            self.current_env = prev_env;
        } else {
            return Err(anyhow::anyhow!("Не знайдено функцію 'головна'"));
        }
        
        Ok(())
    }
    
    fn execute_declaration(&mut self, decl: Declaration) -> Result<()> {
        match decl {
            Declaration::Variable { name, value, .. } => {
                let val = if let Some(expr) = value {
                    self.evaluate_expression(expr)?
                } else {
                    Value::Null
                };
                self.current_env.borrow_mut().set(name, val);
            }
            
            Declaration::Function { name, params, body, .. } => {
                let func = Value::Function {
                    params,
                    body,
                    closure: self.current_env.clone(),
                };
                self.current_env.borrow_mut().set(name, func);
            }
            
            _ => {
                // TODO: Implement other declarations
            }
        }
        
        Ok(())
    }
    
    fn execute_statement(&mut self, stmt: Statement) -> Result<()> {
        match stmt {
            Statement::Expression(expr) => {
                self.evaluate_expression(expr)?;
            }
            
            Statement::Block(statements) => {
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                
                for stmt in statements {
                    self.execute_statement(stmt)?;
                    if self.return_value.is_some() || self.break_flag || self.continue_flag {
                        break;
                    }
                }
                
                self.current_env = prev_env;
            }
            
            Statement::Return(value) => {
                self.return_value = Some(if let Some(expr) = value {
                    self.evaluate_expression(expr)?
                } else {
                    Value::Null
                });
            }
            
            Statement::If { condition, then_branch, else_branch } => {
                let cond_value = self.evaluate_expression(condition)?;
                if cond_value.to_bool() {
                    self.execute_statement(*then_branch)?;
                } else if let Some(else_stmt) = else_branch {
                    self.execute_statement(*else_stmt)?;
                }
            }
            
            Statement::While { condition, body } => {
                while self.evaluate_expression(condition.clone())?.to_bool() {
                    self.execute_statement(*body.clone())?;
                    
                    if self.break_flag {
                        self.break_flag = false;
                        break;
                    }
                    if self.continue_flag {
                        self.continue_flag = false;
                        continue;
                    }
                    if self.return_value.is_some() {
                        break;
                    }
                }
            }
            
            Statement::For { variable, from, to, step, body } => {
                let from_val = match self.evaluate_expression(from)? {
                    Value::Integer(n) => n,
                    _ => return Err(anyhow::anyhow!("Початкове значення циклу має бути цілим числом")),
                };
                
                let to_val = match self.evaluate_expression(to)? {
                    Value::Integer(n) => n,
                    _ => return Err(anyhow::anyhow!("Кінцеве значення циклу має бути цілим числом")),
                };
                
                let step_val = if let Some(step_expr) = step {
                    match self.evaluate_expression(step_expr)? {
                        Value::Integer(n) => n,
                        _ => return Err(anyhow::anyhow!("Крок циклу має бути цілим числом")),
                    }
                } else {
                    1
                };
                
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                
                let mut i = from_val;
                while (step_val > 0 && i < to_val) || (step_val < 0 && i > to_val) {
                    self.current_env.borrow_mut().set(variable.clone(), Value::Integer(i));
                    
                    self.execute_statement(*body.clone())?;
                    
                    if self.break_flag {
                        self.break_flag = false;
                        break;
                    }
                    if self.continue_flag {
                        self.continue_flag = false;
                    }
                    if self.return_value.is_some() {
                        break;
                    }
                    
                    i += step_val;
                }
                
                self.current_env = prev_env;
            }
            
            Statement::Break => {
                self.break_flag = true;
            }
            
            Statement::Continue => {
                self.continue_flag = true;
            }
            
            Statement::Assignment { target, value, op } => {
                if let Expression::Identifier(name) = target {
                    let new_value = match op {
                        AssignmentOp::Assign => self.evaluate_expression(value)?,
                        AssignmentOp::AddAssign => {
                            let current = self.current_env.borrow().get(&name)
                                .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                            self.apply_binary_op(BinaryOp::Add, current, self.evaluate_expression(value)?)?
                        }
                        AssignmentOp::SubAssign => {
                            let current = self.current_env.borrow().get(&name)
                                .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                            self.apply_binary_op(BinaryOp::Sub, current, self.evaluate_expression(value)?)?
                        }
                        AssignmentOp::MulAssign => {
                            let current = self.current_env.borrow().get(&name)
                                .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                            self.apply_binary_op(BinaryOp::Mul, current, self.evaluate_expression(value)?)?
                        }
                        AssignmentOp::DivAssign => {
                            let current = self.current_env.borrow().get(&name)
                                .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                            self.apply_binary_op(BinaryOp::Div, current, self.evaluate_expression(value)?)?
                        }
                    };
                    
                    self.current_env.borrow_mut().update(&name, new_value)?;
                } else {
                    return Err(anyhow::anyhow!("Присвоєння можливе тільки до змінних"));
                }
            }
            
            Statement::Declaration(decl) => {
                self.execute_declaration(decl)?;
            }
        }
        
        Ok(())
    }
    
    fn evaluate_expression(&mut self, expr: Expression) -> Result<Value> {
        match expr {
            Expression::Literal(lit) => Ok(self.evaluate_literal(lit)),
            
            Expression::Identifier(name) => {
                self.current_env.borrow().get(&name)
                    .ok_or_else(|| anyhow::anyhow!("Невідома змінна або функція: {}", name))
            }
            
            Expression::Binary { left, op, right } => {
                let lhs = self.evaluate_expression(*left)?;
                let rhs = self.evaluate_expression(*right)?;
                self.apply_binary_op(op, lhs, rhs)
            }
            
            Expression::Unary { op, operand } => {
                let val = self.evaluate_expression(*operand)?;
                self.apply_unary_op(op, val)
            }
            
            Expression::Call { callee, args } => {
                if let Expression::Identifier(name) = *callee {
                    // Вбудовані функції
                    if name == "друк" {
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                print!(" ");
                            }
                            let val = self.evaluate_expression(arg.clone())?;
                            print!("{}", val.to_string());
                        }
                        println!();
                        Ok(Value::Null)
                    } else if name == "цілеврядок" {
                        if args.len() != 1 {
                            return Err(anyhow::anyhow!("цілеврядок очікує 1 аргумент"));
                        }
                        let val = self.evaluate_expression(args[0].clone())?;
                        match val {
                            Value::Integer(n) => Ok(Value::String(n.to_string())),
                            _ => Err(anyhow::anyhow!("цілеврядок очікує ціле число")),
                        }
                    } else if let Some(Value::Function { params, body, closure }) = 
                        self.current_env.borrow().get(&name) {
                        
                        // Створюємо нове середовище для функції
                        let prev_env = self.current_env.clone();
                        self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));
                        
                        // Прив'язуємо аргументи до параметрів
                        if args.len() != params.len() {
                            return Err(anyhow::anyhow!(
                                "Функція '{}' очікує {} аргументів, отримано {}",
                                name, params.len(), args.len()
                            ));
                        }
                        
                        for (param, arg_expr) in params.iter().zip(args.iter()) {
                            let arg_value = self.evaluate_expression(arg_expr.clone())?;
                            self.current_env.borrow_mut().set(param.name.clone(), arg_value);
                        }
                        
                        // Виконуємо тіло функції
                        let prev_return = self.return_value.take();
                        for stmt in body {
                            self.execute_statement(stmt)?;
                            if self.return_value.is_some() {
                                break;
                            }
                        }
                        
                        let result = self.return_value.take().unwrap_or(Value::Null);
                        self.return_value = prev_return;
                        self.current_env = prev_env;
                        
                        Ok(result)
                    } else {
                        Err(anyhow::anyhow!("Невідома функція: {}", name))
                    }
                } else {
                    Err(anyhow::anyhow!("Непрямі виклики функцій ще не підтримуються"))
                }
            }
            
            Expression::Array(elements) => {
                let mut values = Vec::new();
                for elem in elements {
                    values.push(self.evaluate_expression(elem)?);
                }
                Ok(Value::Array(values))
            }
            
            Expression::Index { object, index } => {
                let obj = self.evaluate_expression(*object)?;
                let idx = self.evaluate_expression(*index)?;
                
                match (obj, idx) {
                    (Value::Array(arr), Value::Integer(i)) => {
                        if i < 0 || i as usize >= arr.len() {
                            Err(anyhow::anyhow!("Індекс {} виходить за межі масиву", i))
                        } else {
                            Ok(arr[i as usize].clone())
                        }
                    }
                    _ => Err(anyhow::anyhow!("Індексація підтримується тільки для масивів")),
                }
            }
            
            Expression::MemberAccess { object, member } => {
                let obj = self.evaluate_expression(*object)?;
                
                match obj {
                    Value::Struct(fields) => {
                        fields.get(&member)
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("Поле '{}' не знайдено", member))
                    }
                    _ => Err(anyhow::anyhow!("Доступ до членів підтримується тільки для структур")),
                }
            }
            
            Expression::Struct { name: _, fields } => {
                let mut field_values = HashMap::new();
                for (field_name, field_expr) in fields {
                    field_values.insert(field_name, self.evaluate_expression(field_expr)?);
                }
                Ok(Value::Struct(field_values))
            }
            
            _ => Err(anyhow::anyhow!("Вираз {:?} ще не реалізований", expr)),
        }
    }
    
    fn evaluate_literal(&self, lit: Literal) -> Value {
        match lit {
            Literal::Integer(n) => Value::Integer(n),
            Literal::Float(f) => Value::Float(f),
            Literal::String(s) => Value::String(s),
            Literal::Char(c) => Value::Char(c),
            Literal::Bool(b) => Value::Bool(b),
            Literal::Null => Value::Null,
        }
    }
    
    fn apply_binary_op(&self, op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value> {
        match (op, lhs, rhs) {
            // Арифметичні операції
            (BinaryOp::Add, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (BinaryOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinaryOp::Add, Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
            
            (BinaryOp::Sub, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
            (BinaryOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            
            (BinaryOp::Mul, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
            (BinaryOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            
            (BinaryOp::Div, Value::Integer(a), Value::Integer(b)) => {
                if b == 0 {
                    Err(anyhow::anyhow!("Ділення на нуль"))
                } else {
                    Ok(Value::Integer(a / b))
                }
            }
            (BinaryOp::Div, Value::Float(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(anyhow::anyhow!("Ділення на нуль"))
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            
            (BinaryOp::Mod, Value::Integer(a), Value::Integer(b)) => {
                if b == 0 {
                    Err(anyhow::anyhow!("Ділення на нуль"))
                } else {
                    Ok(Value::Integer(a % b))
                }
            }
            
            (BinaryOp::Pow, Value::Integer(a), Value::Integer(b)) => {
                if b < 0 {
                    Ok(Value::Float((a as f64).powf(b as f64)))
                } else {
                    Ok(Value::Integer(a.pow(b as u32)))
                }
            }
            (BinaryOp::Pow, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(b))),
            
            // Порівняння
            (BinaryOp::Eq, a, b) => Ok(Value::Bool(self.values_equal(&a, &b))),
            (BinaryOp::Ne, a, b) => Ok(Value::Bool(!self.values_equal(&a, &b))),
            
            (BinaryOp::Lt, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a < b)),
            (BinaryOp::Lt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
            
            (BinaryOp::Le, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a <= b)),
            (BinaryOp::Le, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
            
            (BinaryOp::Gt, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a > b)),
            (BinaryOp::Gt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
            
            (BinaryOp::Ge, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a >= b)),
            (BinaryOp::Ge, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
            
            // Логічні операції
            (BinaryOp::And, a, b) => Ok(Value::Bool(a.to_bool() && b.to_bool())),
            (BinaryOp::Or, a, b) => Ok(Value::Bool(a.to_bool() || b.to_bool())),
            
            _ => Err(anyhow::anyhow!("Несумісні типи для операції {:?}", op)),
        }
    }
    
    fn apply_unary_op(&self, op: UnaryOp, val: Value) -> Result<Value> {
        match (op, val) {
            (UnaryOp::Neg, Value::Integer(n)) => Ok(Value::Integer(-n)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, v) => Ok(Value::Bool(!v.to_bool())),
            _ => Err(anyhow::anyhow!("Несумісний тип для унарної операції {:?}", op)),
        }
    }
    
    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            _ => false,
        }
    }
}

pub fn execute(program: Program, args: Vec<String>) -> Result<()> {
    let mut vm = VM::new();
    vm.execute_program(program, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tryzub_lexer::tokenize;
    use tryzub_parser::parse;
    
    #[test]
    fn test_arithmetic() {
        let source = r#"
функція головна() {
    змінна a = 10
    змінна b = 20
    друк(a + b)
    друк(a * b)
}
"#;
        
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        
        assert!(execute(program, vec![]).is_ok());
    }
}
