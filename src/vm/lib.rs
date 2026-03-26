// Віртуальна машина мови Тризуб v3.9
// Автор: Мартинюк Євген

use anyhow::Result;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use serde_json;
use tryzub_parser::{
    Program, Declaration, Statement, Expression, Literal, BinaryOp, UnaryOp,
    Type, Parameter, AssignmentOp, Pattern, MatchArm, FormatPart, LambdaParam,
    EnumVariant, TraitMethod, Contract,
};

// ════════════════════════════════════════════════════════════════════
// Значення (Value) — всі можливі типи даних у VM
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Array(Vec<Value>),
    Tuple(Vec<Value>),
    Struct(String, HashMap<String, Value>), // (назва_типу, поля)
    /// Варіант алгебраїчного типу: Деякий(42), Помилка("ой")
    EnumVariant {
        type_name: String,
        variant: String,
        fields: Vec<Value>,
    },
    Function {
        name: Option<String>,
        params: Vec<Parameter>,
        body: Vec<Statement>,
        closure: Environment,
    },
    /// Лямбда-функція
    Lambda {
        params: Vec<LambdaParam>,
        body: LambdaBody,
        closure: Environment,
    },
    /// Вбудована функція (Rust callback)
    BuiltinFn(String),
    /// Частково застосована вбудована функція (каррінг для pipeline)
    CurriedBuiltin {
        name: String,
        saved_args: Vec<Value>,
    },
    /// Словник (HashMap)
    Dict(Vec<(Value, Value)>),
    /// Множина (Set)
    Set(Vec<Value>),
    /// Генератор (ліниві послідовності)
    Generator {
        params: Vec<Parameter>,
        body: Vec<Statement>,
        closure: Environment,
        /// Зібрані через віддати значення
        yielded_values: Vec<Value>,
        /// Поточний індекс для наступний()
        current_index: usize,
        /// Чи генератор вже виконувався
        executed: bool,
    },
    /// Діапазон
    Range {
        from: i64,
        to: i64,
        inclusive: bool,
    },
    Null,
}

#[derive(Debug, Clone)]
pub enum LambdaBody {
    Expr(Expression),
    Block(Vec<Statement>),
}

impl Value {
    fn to_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Integer(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Null => false,
            Value::Array(arr) => !arr.is_empty(),
            Value::EnumVariant { variant, .. } => variant != "Нічого",
            _ => true,
        }
    }

    fn to_display_string(&self) -> String {
        match self {
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => {
                if *f == f.floor() && f.is_finite() {
                    format!("{:.1}", f)
                } else {
                    f.to_string()
                }
            }
            Value::String(s) => s.clone(),
            Value::Char(c) => c.to_string(),
            Value::Bool(b) => if *b { "істина" } else { "хиба" }.to_string(),
            Value::Array(arr) => {
                let elements: Vec<String> = arr.iter().map(|v| v.to_display_string()).collect();
                format!("[{}]", elements.join(", "))
            }
            Value::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(|v| v.to_display_string()).collect();
                format!("({})", parts.join(", "))
            }
            Value::Struct(name, fields) => {
                let parts: Vec<String> = fields.iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_display_string()))
                    .collect();
                format!("{} {{ {} }}", name, parts.join(", "))
            }
            Value::EnumVariant { variant, fields, .. } => {
                if fields.is_empty() {
                    variant.clone()
                } else {
                    let parts: Vec<String> = fields.iter().map(|v| v.to_display_string()).collect();
                    format!("{}({})", variant, parts.join(", "))
                }
            }
            Value::Range { from, to, inclusive } => {
                if *inclusive {
                    format!("{}..={}", from, to)
                } else {
                    format!("{}..{}", from, to)
                }
            }
            Value::Dict(pairs) => {
                let parts: Vec<String> = pairs.iter()
                    .map(|(k, v)| format!("{} -> {}", k.to_display_string(), v.to_display_string()))
                    .collect();
                format!("#{{{}}}", parts.join(", "))
            }
            Value::Set(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.to_display_string()).collect();
                format!("%{{{}}}", parts.join(", "))
            }
            Value::Null => "нуль".to_string(),
            Value::Function { name, .. } => format!("<функція {}>", name.as_deref().unwrap_or("анонімна")),
            Value::Lambda { .. } => "<лямбда>".to_string(),
            Value::BuiltinFn(name) => format!("<вбудована {}>", name),
            Value::CurriedBuiltin { name, .. } => format!("<каррінг {}>", name),
            Value::Generator { .. } => "<генератор>".to_string(),
        }
    }

    fn type_name(&self) -> &str {
        match self {
            Value::Integer(_) => "цл64",
            Value::Float(_) => "дрб64",
            Value::String(_) => "тхт",
            Value::Char(_) => "сим",
            Value::Bool(_) => "лог",
            Value::Array(_) => "масив",
            Value::Tuple(_) => "кортеж",
            Value::Dict(_) => "словник",
            Value::Set(_) => "множина",
            Value::Struct(name, _) => name,
            Value::EnumVariant { type_name, .. } => type_name,
            Value::Null => "нуль",
            _ => "функція",
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// Середовище виконання (Scope)
// ════════════════════════════════════════════════════════════════════

type Environment = Rc<RefCell<Scope>>;

#[derive(Debug, Clone)]
struct Scope {
    variables: HashMap<String, Value>,
    parent: Option<Environment>,
}

impl Scope {
    fn new(parent: Option<Environment>) -> Self {
        Self { variables: HashMap::new(), parent }
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

// ════════════════════════════════════════════════════════════════════
// Віртуальна машина
// ════════════════════════════════════════════════════════════════════

pub struct VM {
    global_env: Environment,
    current_env: Environment,
    return_value: Option<Value>,
    break_flag: bool,
    continue_flag: bool,
    /// Зареєстровані типи enum
    enum_types: HashMap<String, Vec<EnumVariant>>,
    /// Зареєстровані трейти
    trait_methods: HashMap<(String, String), Vec<Statement>>,
    /// Контракти функцій
    contracts: HashMap<String, Contract>,
    /// Стек обробників ефектів: ім'я_обробника → Environment з обробником
    effect_handlers: Vec<(String, Environment)>,
    /// Зареєстровані ефекти: ім'я_ефекту → операції
    registered_effects: HashMap<String, Vec<String>>,
    /// Yielded values від генераторів
    yielded_values: Vec<Value>,
    /// Черга async завдань
    async_queue: Vec<(Vec<Statement>, Environment)>,
    /// Зареєстровані макроси: ім'я → (параметри, тіло)
    macros: HashMap<String, (Vec<String>, Vec<Statement>)>,
    /// Шляхи для пошуку stdlib модулів
    stdlib_paths: Vec<String>,
    /// Вже завантажені модулі
    loaded_modules: HashMap<String, bool>,
    /// Кеш виконаних генераторів: id → (yielded_values, current_index)
    generator_cache: HashMap<usize, (Vec<Value>, usize)>,
    /// Лічильник ID для генераторів
    generator_id_counter: usize,
}

impl VM {
    pub fn new() -> Self {
        let global_scope = Rc::new(RefCell::new(Scope::new(None)));

        // Додаємо вбудовані функції
        {
            let mut scope = global_scope.borrow_mut();
            scope.set("друк".to_string(), Value::BuiltinFn("друк".to_string()));
            scope.set("цілеврядок".to_string(), Value::BuiltinFn("цілеврядок".to_string()));
            scope.set("довжина".to_string(), Value::BuiltinFn("довжина".to_string()));
            scope.set("тип_значення".to_string(), Value::BuiltinFn("тип_значення".to_string()));
            scope.set("діапазон".to_string(), Value::BuiltinFn("діапазон".to_string()));
            scope.set("фільтрувати".to_string(), Value::BuiltinFn("фільтрувати".to_string()));
            scope.set("перетворити".to_string(), Value::BuiltinFn("перетворити".to_string()));
            scope.set("згорнути".to_string(), Value::BuiltinFn("згорнути".to_string()));
            scope.set("сортувати".to_string(), Value::BuiltinFn("сортувати".to_string()));
            scope.set("обернути".to_string(), Value::BuiltinFn("обернути".to_string()));
            scope.set("додати".to_string(), Value::BuiltinFn("додати".to_string()));
            scope.set("паніка".to_string(), Value::BuiltinFn("паніка".to_string()));
            scope.set("словник".to_string(), Value::BuiltinFn("словник".to_string()));
            scope.set("множина".to_string(), Value::BuiltinFn("множина".to_string()));
            scope.set("виконати_ефект".to_string(), Value::BuiltinFn("виконати_ефект".to_string()));

            // Ввід/вивід
            scope.set("ввід".to_string(), Value::BuiltinFn("ввід".to_string()));

            // Генератори
            scope.set("генератор".to_string(), Value::BuiltinFn("генератор".to_string()));

            // Час
            scope.set("час_зараз".to_string(), Value::BuiltinFn("час_зараз".to_string()));
            scope.set("час_затримка".to_string(), Value::BuiltinFn("час_затримка".to_string()));

            // JSON
            scope.set("json_розібрати".to_string(), Value::BuiltinFn("json_розібрати".to_string()));
            scope.set("json_в_рядок".to_string(), Value::BuiltinFn("json_в_рядок".to_string()));
            scope.set("json_в_рядок_красиво".to_string(), Value::BuiltinFn("json_в_рядок_красиво".to_string()));

            // Математика (нативна)
            scope.set("корінь".to_string(), Value::BuiltinFn("корінь".to_string()));
            scope.set("синус".to_string(), Value::BuiltinFn("синус".to_string()));
            scope.set("косинус".to_string(), Value::BuiltinFn("косинус".to_string()));
            scope.set("степінь_ф".to_string(), Value::BuiltinFn("степінь_ф".to_string()));
            scope.set("логарифм".to_string(), Value::BuiltinFn("логарифм".to_string()));
            scope.set("ПІ".to_string(), Value::Float(std::f64::consts::PI));
            scope.set("Е".to_string(), Value::Float(std::f64::consts::E));
            scope.set("ціле_з_рядка".to_string(), Value::BuiltinFn("ціле_з_рядка".to_string()));
            scope.set("дробове_з_рядка".to_string(), Value::BuiltinFn("дробове_з_рядка".to_string()));

            // Файловий I/O
            scope.set("файл_прочитати".to_string(), Value::BuiltinFn("файл_прочитати".to_string()));
            scope.set("файл_записати".to_string(), Value::BuiltinFn("файл_записати".to_string()));
            scope.set("файл_існує".to_string(), Value::BuiltinFn("файл_існує".to_string()));
            scope.set("файл_рядки".to_string(), Value::BuiltinFn("файл_рядки".to_string()));
            scope.set("файл_додати".to_string(), Value::BuiltinFn("файл_додати".to_string()));
            scope.set("мін".to_string(), Value::BuiltinFn("мін".to_string()));
            scope.set("макс".to_string(), Value::BuiltinFn("макс".to_string()));
            scope.set("абс".to_string(), Value::BuiltinFn("абс".to_string()));

            // Вбудовані конструктори Опція/Результат
            scope.set("Деякий".to_string(), Value::BuiltinFn("Деякий".to_string()));
            scope.set("Нічого".to_string(), Value::EnumVariant {
                type_name: "Опція".to_string(),
                variant: "Нічого".to_string(),
                fields: vec![],
            });
            scope.set("Успіх".to_string(), Value::BuiltinFn("Успіх".to_string()));
            scope.set("Помилка".to_string(), Value::BuiltinFn("Помилка".to_string()));
        }

        Self {
            global_env: global_scope.clone(),
            current_env: global_scope,
            return_value: None,
            break_flag: false,
            continue_flag: false,
            enum_types: HashMap::new(),
            trait_methods: HashMap::new(),
            contracts: HashMap::new(),
            yielded_values: Vec::new(),
            async_queue: Vec::new(),
            macros: HashMap::new(),
            effect_handlers: Vec::new(),
            registered_effects: HashMap::new(),
            stdlib_paths: vec![
                "stdlib".to_string(),
                "../stdlib".to_string(),
            ],
            loaded_modules: HashMap::new(),
            generator_cache: HashMap::new(),
            generator_id_counter: 0,
        }
    }

    pub fn execute_program(&mut self, program: Program, _args: Vec<String>) -> Result<()> {
        for decl in &program.declarations {
            self.execute_declaration(decl.clone())?;
        }

        let main_fn = self.global_env.borrow().get("головна");
        if let Some(Value::Function { params, body, closure, .. }) = main_fn {
            if params.iter().any(|p| p.name != "себе") && !params.is_empty() {
                return Err(anyhow::anyhow!("Функція 'головна' не повинна мати параметрів"));
            }

            let prev_env = self.current_env.clone();
            self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

            for stmt in body {
                self.execute_statement(stmt)?;
                if self.return_value.is_some() { break; }
            }

            self.return_value = None;
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
            Declaration::Function { name, params, body, contract, .. } => {
                let func = Value::Function {
                    name: Some(name.clone()),
                    params,
                    body,
                    closure: self.current_env.clone(),
                };
                // Зберігаємо контракт для перевірки при виклику
                if let Some(c) = contract {
                    self.contracts.insert(name.clone(), c);
                }
                self.current_env.borrow_mut().set(name, func);
            }
            Declaration::Enum { name, variants, .. } => {
                // Реєструємо варіанти як конструктори
                for variant in &variants {
                    let variant_name = variant.name.clone();
                    let type_name = name.clone();
                    if variant.fields.is_empty() {
                        self.current_env.borrow_mut().set(variant_name.clone(), Value::EnumVariant {
                            type_name: type_name.clone(),
                            variant: variant_name,
                            fields: vec![],
                        });
                    } else {
                        // Функція-конструктор
                        self.current_env.borrow_mut().set(variant_name.clone(),
                            Value::BuiltinFn(format!("{}::{}", type_name, variant_name)));
                    }
                }
                self.enum_types.insert(name, variants);
            }
            Declaration::Trait { .. } => {
                // Трейти зберігаються для перевірки типів
            }
            Declaration::TraitImpl { for_type, methods, .. } |
            Declaration::Impl { type_name: for_type, methods } => {
                for method in methods {
                    if let Declaration::Function { name, params, body, .. } = method {
                        let func = Value::Function {
                            name: Some(name.clone()),
                            params,
                            body: body.clone(),
                            closure: self.current_env.clone(),
                        };
                        // Зберігаємо як тип::метод
                        self.current_env.borrow_mut().set(
                            format!("{}::{}", for_type, name), func.clone()
                        );
                        self.trait_methods.insert(
                            (for_type.clone(), name), body
                        );
                    }
                }
            }
            Declaration::Struct { name, fields, .. } => {
                // Зберігаємо інформацію про структуру для конструктора
                let field_names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                self.current_env.borrow_mut().set(
                    format!("__struct_fields_{}", name),
                    Value::Array(field_names.into_iter().map(Value::String).collect())
                );
            }
            Declaration::Effect { name, operations, .. } => {
                let op_names: Vec<String> = operations.iter().map(|o| o.name.clone()).collect();
                self.registered_effects.insert(name, op_names);
            }
            Declaration::Macro { name, params, body } => {
                let builtin_name = format!("__macro_{}", name);
                self.macros.insert(name.clone(), (params, body));
                self.current_env.borrow_mut().set(name, Value::BuiltinFn(builtin_name));
            }
            Declaration::FuzzTest { name, inputs, body } => {
                // Фаз-тести запускаються через `тризуб тестувати`
                // Генеруємо випадкові входи та виконуємо тіло
            }
            Declaration::Benchmark { name, sizes, body } => {
                // Бенчмарки запускаються через `тризуб тестувати`
            }
            Declaration::Import { path, .. } => {
                // Реальний імпорт — шукаємо та завантажуємо модуль
                let module_name = path.last().cloned().unwrap_or_default();
                if !self.loaded_modules.contains_key(&module_name) {
                    self.load_module(&module_name)?;
                }
            }
            Declaration::Test { name, body } => {
                // Тести не виконуються при звичайному запуску —
                // тільки через `тризуб тестувати`
            }
            _ => {
                // TypeAlias, Interface, FuzzTest, Benchmark, Macro — парсяться але не виконуються
            }
        }
        Ok(())
    }

    fn execute_statement(&mut self, stmt: Statement) -> Result<()> {
        match stmt {
            Statement::Expression(expr) => { self.evaluate_expression(expr)?; }
            Statement::Block(statements) => {
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                for stmt in statements {
                    self.execute_statement(stmt)?;
                    if self.return_value.is_some() || self.break_flag || self.continue_flag { break; }
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
                    if self.break_flag { self.break_flag = false; break; }
                    if self.continue_flag { self.continue_flag = false; continue; }
                    if self.return_value.is_some() { break; }
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
                } else { 1 };

                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));

                let mut i = from_val;
                while (step_val > 0 && i < to_val) || (step_val < 0 && i > to_val) {
                    self.current_env.borrow_mut().set(variable.clone(), Value::Integer(i));
                    self.execute_statement(*body.clone())?;
                    if self.break_flag { self.break_flag = false; break; }
                    if self.continue_flag { self.continue_flag = false; }
                    if self.return_value.is_some() { break; }
                    i += step_val;
                }
                self.current_env = prev_env;
            }
            Statement::ForIn { pattern, iterable, body } => {
                let iter_val = self.evaluate_expression(iterable)?;
                let items = match iter_val {
                    Value::Array(arr) => arr,
                    Value::Range { from, to, inclusive } => {
                        let end = if inclusive { to + 1 } else { to };
                        (from..end).map(Value::Integer).collect()
                    }
                    Value::Dict(pairs) => {
                        // Ітерація по словнику — кожен елемент це кортеж (ключ, значення)
                        pairs.into_iter().map(|(k, v)| Value::Tuple(vec![k, v])).collect()
                    }
                    Value::Set(items) => items,
                    Value::String(s) => {
                        s.chars().map(Value::Char).collect()
                    }
                    Value::Generator { body, closure, .. } => {
                        // Виконуємо генератор та ітеруємо по yielded значеннях
                        self.execute_generator(body, closure)?
                    }
                    _ => return Err(anyhow::anyhow!("Неможливо ітерувати по {}", iter_val.type_name())),
                };

                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));

                for item in items {
                    self.bind_pattern(&pattern, &item)?;
                    self.execute_statement(*body.clone())?;
                    if self.break_flag { self.break_flag = false; break; }
                    if self.continue_flag { self.continue_flag = false; }
                    if self.return_value.is_some() { break; }
                }
                self.current_env = prev_env;
            }
            Statement::Break => { self.break_flag = true; }
            Statement::Continue => { self.continue_flag = true; }
            Statement::Assignment { target, value, op } => {
                self.execute_assignment(target, value, op)?;
            }
            Statement::Declaration(decl) => {
                self.execute_declaration(decl)?;
            }
            Statement::TryCatch { try_body, catch_param, catch_body, finally_body } => {
                let result = self.execute_statement(*try_body);
                if let Err(err) = result {
                    if let (Some(param), Some(body)) = (catch_param, catch_body) {
                        let prev_env = self.current_env.clone();
                        self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                        self.current_env.borrow_mut().set(param, Value::String(err.to_string()));
                        self.execute_statement(*body)?;
                        self.current_env = prev_env;
                    }
                }
                if let Some(finally) = finally_body {
                    self.execute_statement(*finally)?;
                }
            }
            Statement::Destructure { pattern, value, .. } => {
                let val = self.evaluate_expression(value)?;
                self.bind_pattern(&pattern, &val)?;
            }
            Statement::Assert(expr) => {
                let val = self.evaluate_expression(expr.clone())?;
                if !val.to_bool() {
                    return Err(anyhow::anyhow!("Перевірка не пройшла: {:?}", expr));
                }
            }
            Statement::WithHandler { handler, body } => {
                // Пушимо обробник на стек ефектів
                let handler_env = self.current_env.clone();
                self.effect_handlers.push((handler.clone(), handler_env));

                // Виконуємо тіло — всі виклики функцій з ефектами
                // будуть бачити цей обробник
                let result = self.execute_statement(*body);

                // Попаємо обробник зі стеку
                self.effect_handlers.pop();

                // Прокидуємо помилку якщо є
                result?;
            }
            Statement::CompTime(stmts) => {
                // Компчас — виконуємо на етапі "компіляції" (в VM — просто виконуємо)
                for stmt in stmts {
                    self.execute_statement(stmt)?;
                    if self.return_value.is_some() { break; }
                }
            }
            Statement::Unsafe(stmts) => {
                for stmt in stmts {
                    self.execute_statement(stmt)?;
                    if self.return_value.is_some() { break; }
                }
            }
            Statement::Yield(expr) => {
                let val = self.evaluate_expression(expr)?;
                self.yielded_values.push(val);
            }
        }
        Ok(())
    }

    fn execute_assignment(&mut self, target: Expression, value: Expression, op: AssignmentOp) -> Result<()> {
        match target {
            Expression::Identifier(name) => {
                let new_value = match op {
                    AssignmentOp::Assign => self.evaluate_expression(value)?,
                    _ => {
                        let current = self.current_env.borrow().get(&name)
                            .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", name))?;
                        let rhs = self.evaluate_expression(value)?;
                        let bin_op = match op {
                            AssignmentOp::AddAssign => BinaryOp::Add,
                            AssignmentOp::SubAssign => BinaryOp::Sub,
                            AssignmentOp::MulAssign => BinaryOp::Mul,
                            AssignmentOp::DivAssign => BinaryOp::Div,
                            AssignmentOp::ModAssign => BinaryOp::Mod,
                            _ => unreachable!(),
                        };
                        self.apply_binary_op(bin_op, current, rhs)?
                    }
                };
                self.current_env.borrow_mut().update(&name, new_value)?;
            }
            Expression::MemberAccess { object, member } => {
                if let Expression::Identifier(obj_name) = *object {
                    let new_value = self.evaluate_expression(value)?;
                    let obj = self.current_env.borrow().get(&obj_name)
                        .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", obj_name))?;
                    if let Value::Struct(type_name, mut fields) = obj {
                        fields.insert(member, new_value);
                        let updated = Value::Struct(type_name, fields);
                        self.current_env.borrow_mut().update(&obj_name, updated)?;
                    }
                }
            }
            _ => return Err(anyhow::anyhow!("Присвоєння можливе тільки до змінних")),
        }
        Ok(())
    }

    // ── Обчислення виразів ──

    fn evaluate_expression(&mut self, expr: Expression) -> Result<Value> {
        match expr {
            Expression::Literal(lit) => Ok(self.evaluate_literal(lit)),
            Expression::Identifier(name) => {
                self.current_env.borrow().get(&name)
                    .ok_or_else(|| anyhow::anyhow!("Невідома змінна або функція: {}", name))
            }
            Expression::SelfRef => {
                self.current_env.borrow().get("себе")
                    .ok_or_else(|| anyhow::anyhow!("'себе' доступне тільки в методах"))
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
                let func = self.evaluate_expression(*callee)?;
                let mut arg_values = Vec::new();
                for arg in args {
                    arg_values.push(self.evaluate_expression(arg)?);
                }
                self.call_value(func, arg_values)
            }
            Expression::MethodCall { object, method, args } => {
                let obj = self.evaluate_expression(*object)?;
                let mut arg_values = Vec::new();
                for arg in args {
                    arg_values.push(self.evaluate_expression(arg)?);
                }
                self.call_method(obj, &method, arg_values)
            }
            Expression::Index { object, index } => {
                let obj = self.evaluate_expression(*object)?;
                let idx = self.evaluate_expression(*index)?;
                match (obj, idx) {
                    (Value::Array(arr), Value::Integer(i)) => {
                        let idx = if i < 0 { arr.len() as i64 + i } else { i } as usize;
                        arr.get(idx).cloned().ok_or_else(|| anyhow::anyhow!("Індекс {} поза межами", i))
                    }
                    (Value::String(s), Value::Integer(i)) => {
                        let idx = if i < 0 { s.len() as i64 + i } else { i } as usize;
                        s.chars().nth(idx).map(Value::Char)
                            .ok_or_else(|| anyhow::anyhow!("Індекс {} поза межами", i))
                    }
                    _ => Err(anyhow::anyhow!("Індексація підтримується тільки для масивів та рядків")),
                }
            }
            Expression::MemberAccess { object, member } => {
                let obj = self.evaluate_expression(*object)?;
                match &obj {
                    Value::Struct(_, fields) => {
                        fields.get(&member).cloned()
                            .ok_or_else(|| anyhow::anyhow!("Поле '{}' не знайдено", member))
                    }
                    Value::Array(arr) if member == "довжина" => Ok(Value::Integer(arr.len() as i64)),
                    Value::String(s) if member == "довжина" => Ok(Value::Integer(s.len() as i64)),
                    _ => Err(anyhow::anyhow!("Доступ до поля '{}' неможливий для {}", member, obj.type_name())),
                }
            }
            Expression::Array(elements) => {
                let mut values = Vec::new();
                for elem in elements {
                    values.push(self.evaluate_expression(elem)?);
                }
                Ok(Value::Array(values))
            }
            Expression::Tuple(elements) => {
                let mut values = Vec::new();
                for elem in elements {
                    values.push(self.evaluate_expression(elem)?);
                }
                Ok(Value::Tuple(values))
            }
            Expression::Struct { name, fields } => {
                let mut field_values = HashMap::new();
                for (field_name, field_expr) in fields {
                    field_values.insert(field_name, self.evaluate_expression(field_expr)?);
                }
                Ok(Value::Struct(name, field_values))
            }
            Expression::Lambda { params, body } => {
                Ok(Value::Lambda {
                    params,
                    body: LambdaBody::Expr(*body),
                    closure: self.current_env.clone(),
                })
            }
            Expression::LambdaBlock { params, body } => {
                Ok(Value::Lambda {
                    params,
                    body: LambdaBody::Block(body),
                    closure: self.current_env.clone(),
                })
            }
            Expression::Match { subject, arms } => {
                let value = self.evaluate_expression(*subject)?;
                self.evaluate_match(value, arms)
            }
            Expression::Pipeline { left, right } => {
                let arg = self.evaluate_expression(*left)?;
                let func = self.evaluate_expression(*right)?;
                self.call_value(func, vec![arg])
            }
            Expression::ErrorPropagation(expr) => {
                let value = self.evaluate_expression(*expr)?;
                match &value {
                    Value::EnumVariant { variant, fields, .. } if variant == "Успіх" => {
                        Ok(fields.first().cloned().unwrap_or(Value::Null))
                    }
                    Value::EnumVariant { variant, .. } if variant == "Помилка" => {
                        // Поширюємо помилку назовні
                        self.return_value = Some(value.clone());
                        Ok(value)
                    }
                    Value::EnumVariant { variant, fields, .. } if variant == "Деякий" => {
                        Ok(fields.first().cloned().unwrap_or(Value::Null))
                    }
                    Value::EnumVariant { variant, .. } if variant == "Нічого" => {
                        self.return_value = Some(value.clone());
                        Ok(value)
                    }
                    _ => Ok(value),
                }
            }
            Expression::FormatString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        FormatPart::Text(text) => result.push_str(&text),
                        FormatPart::Expr(expr) => {
                            let val = self.evaluate_expression(expr)?;
                            result.push_str(&val.to_display_string());
                        }
                    }
                }
                Ok(Value::String(result))
            }
            Expression::Range { from, to, inclusive } => {
                let from_val = match self.evaluate_expression(*from)? {
                    Value::Integer(n) => n,
                    _ => return Err(anyhow::anyhow!("Діапазон підтримує тільки цілі числа")),
                };
                let to_val = match self.evaluate_expression(*to)? {
                    Value::Integer(n) => n,
                    _ => return Err(anyhow::anyhow!("Діапазон підтримує тільки цілі числа")),
                };
                Ok(Value::Range { from: from_val, to: to_val, inclusive })
            }
            Expression::EnumConstruct { variant, args } => {
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.evaluate_expression(arg)?);
                }
                Ok(Value::EnumVariant {
                    type_name: String::new(),
                    variant,
                    fields: values,
                })
            }
            Expression::Path { segments } => {
                let full_name = segments.join("::");
                self.current_env.borrow().get(&full_name)
                    .ok_or_else(|| anyhow::anyhow!("Невідомий шлях: {}", full_name))
            }
            Expression::Cast { expr, ty } => {
                let val = self.evaluate_expression(*expr)?;
                // Реальна конвертація типів
                match (&val, &ty) {
                    (Value::Integer(n), Type::Дрб64) => Ok(Value::Float(*n as f64)),
                    (Value::Integer(n), Type::Дрб32) => Ok(Value::Float(*n as f64)),
                    (Value::Integer(n), Type::Тхт) => Ok(Value::String(n.to_string())),
                    (Value::Integer(n), Type::Лог) => Ok(Value::Bool(*n != 0)),
                    (Value::Integer(n), Type::Сим) => Ok(Value::Char(char::from_u32(*n as u32).unwrap_or('?'))),
                    (Value::Float(f), Type::Цл64) => Ok(Value::Integer(*f as i64)),
                    (Value::Float(f), Type::Цл32) => Ok(Value::Integer(*f as i64)),
                    (Value::Float(f), Type::Тхт) => Ok(Value::String(f.to_string())),
                    (Value::Bool(b), Type::Цл64) => Ok(Value::Integer(if *b { 1 } else { 0 })),
                    (Value::Bool(b), Type::Тхт) => Ok(Value::String(if *b { "істина" } else { "хиба" }.to_string())),
                    (Value::String(s), Type::Цл64) => Ok(Value::Integer(s.parse::<i64>().unwrap_or(0))),
                    (Value::String(s), Type::Дрб64) => Ok(Value::Float(s.parse::<f64>().unwrap_or(0.0))),
                    (Value::Char(c), Type::Цл64) => Ok(Value::Integer(*c as i64)),
                    _ => Ok(val), // Якщо конвертація невідома — повертаємо як є
                }
            }
            Expression::Await(expr) => {
                // Async/await: event loop з чергою завдань
                // 1. Обчислюємо вираз
                let val = self.evaluate_expression(*expr)?;
                // 2. Якщо результат — функція/лямбда (Future), плануємо та виконуємо
                match val {
                    Value::Function { .. } | Value::Lambda { .. } => {
                        // Додаємо в чергу та одразу виконуємо (cooperative scheduling)
                        let result = self.call_value(val, vec![])?;
                        // Після кожного await — обробляємо решту черги
                        self.drain_async_queue()?;
                        Ok(result)
                    }
                    // Якщо це вже обчислене значення — просто повертаємо
                    _ => Ok(val),
                }
            }
            Expression::If { condition, then_expr, else_expr } => {
                let cond = self.evaluate_expression(*condition)?;
                if cond.to_bool() {
                    self.evaluate_expression(*then_expr)
                } else {
                    self.evaluate_expression(*else_expr)
                }
            }
        }
    }

    // ── Виклик значень ──

    fn call_value(&mut self, func: Value, args: Vec<Value>) -> Result<Value> {
        match func {
            Value::Function { params, body, closure, name } => {
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

                for (param, arg) in params.iter().zip(args.iter()) {
                    if param.name != "себе" {
                        self.current_env.borrow_mut().set(param.name.clone(), arg.clone());
                    }
                }

                // Перевірка передумов (контракти: вимагає)
                let func_name = name.clone().unwrap_or_default();
                if let Some(contract) = self.contracts.get(&func_name).cloned() {
                    for pre in &contract.preconditions {
                        let val = self.evaluate_expression(pre.clone())?;
                        if !val.to_bool() {
                            return Err(anyhow::anyhow!(
                                "Контракт порушено: передумова не виконана у функції '{}'", func_name
                            ));
                        }
                    }
                }

                let prev_return = self.return_value.take();
                let mut last_expr_value = Value::Null;

                for (i, stmt) in body.iter().enumerate() {
                    if i == body.len() - 1 {
                        if let Statement::Expression(expr) = stmt {
                            last_expr_value = self.evaluate_expression(expr.clone())?;
                            break;
                        }
                    }
                    self.execute_statement(stmt.clone())?;
                    if self.return_value.is_some() { break; }
                }

                let result = self.return_value.take().unwrap_or(last_expr_value);

                // Перевірка постумов (контракти: гарантує)
                if let Some(contract) = self.contracts.get(&func_name).cloned() {
                    if !contract.postconditions.is_empty() {
                        // Зберігаємо результат як змінну для перевірки
                        if let Some(ref rn) = contract.result_name {
                            self.current_env.borrow_mut().set(rn.clone(), result.clone());
                        }
                        for post in &contract.postconditions {
                            let val = self.evaluate_expression(post.clone())?;
                            if !val.to_bool() {
                                return Err(anyhow::anyhow!(
                                    "Контракт порушено: постумова не виконана у функції '{}'", func_name
                                ));
                            }
                        }
                    }
                }

                self.return_value = prev_return;
                self.current_env = prev_env;
                Ok(result)
            }
            Value::Lambda { params, body, closure } => {
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

                for (param, arg) in params.iter().zip(args.iter()) {
                    self.current_env.borrow_mut().set(param.name.clone(), arg.clone());
                }

                let result = match body {
                    LambdaBody::Expr(expr) => self.evaluate_expression(expr)?,
                    LambdaBody::Block(stmts) => {
                        let prev_return = self.return_value.take();
                        for stmt in stmts {
                            self.execute_statement(stmt)?;
                            if self.return_value.is_some() { break; }
                        }
                        let r = self.return_value.take().unwrap_or(Value::Null);
                        self.return_value = prev_return;
                        r
                    }
                };

                self.current_env = prev_env;
                Ok(result)
            }
            Value::BuiltinFn(name) => self.call_builtin(&name, args),
            Value::CurriedBuiltin { name, saved_args } => {
                // Pipeline каррінг: масив |> фільтрувати(предикат)
                // CurriedBuiltin має збережений предикат, args[0] = масив
                let mut all_args = args;
                all_args.extend(saved_args);
                self.call_builtin(&name, all_args)
            }
            _ => Err(anyhow::anyhow!("Неможливо викликати {:?}", func.type_name())),
        }
    }

    fn call_method(&mut self, obj: Value, method: &str, args: Vec<Value>) -> Result<Value> {
        // ── Методи масивів ──
        if let Value::Array(ref arr) = obj {
            match method {
                "довжина" => return Ok(Value::Integer(arr.len() as i64)),
                "додати" => {
                    let mut new_arr = arr.clone();
                    for arg in &args { new_arr.push(arg.clone()); }
                    return Ok(Value::Array(new_arr));
                }
                "перший" => return Ok(arr.first().cloned().unwrap_or(Value::Null)),
                "останній" => return Ok(arr.last().cloned().unwrap_or(Value::Null)),
                "пусто" => return Ok(Value::Bool(arr.is_empty())),
                "містить" => {
                    if let Some(val) = args.first() {
                        return Ok(Value::Bool(arr.iter().any(|v| self.values_equal(v, val))));
                    }
                    return Ok(Value::Bool(false));
                }
                "обернути" => {
                    let mut rev = arr.clone();
                    rev.reverse();
                    return Ok(Value::Array(rev));
                }
                "сортувати" => {
                    let mut sorted = arr.clone();
                    sorted.sort_by(|a, b| match (a, b) {
                        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
                        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                        (Value::String(x), Value::String(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    });
                    return Ok(Value::Array(sorted));
                }
                "фільтрувати" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for item in arr {
                            let cond = self.call_value(func.clone(), vec![item.clone()])?;
                            if cond.to_bool() { result.push(item.clone()); }
                        }
                        return Ok(Value::Array(result));
                    }
                    return Err(anyhow::anyhow!(".фільтрувати() потребує предикат"));
                }
                "перетворити" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for item in arr {
                            result.push(self.call_value(func.clone(), vec![item.clone()])?);
                        }
                        return Ok(Value::Array(result));
                    }
                    return Err(anyhow::anyhow!(".перетворити() потребує функцію"));
                }
                "згорнути" => {
                    if args.len() == 2 {
                        let mut acc = args[0].clone();
                        let func = args[1].clone();
                        for item in arr {
                            acc = self.call_value(func.clone(), vec![acc, item.clone()])?;
                        }
                        return Ok(acc);
                    }
                    return Err(anyhow::anyhow!(".згорнути() потребує початок та функцію"));
                }
                "з_єднати" => {
                    let sep = match args.first() {
                        Some(Value::String(s)) => s.as_str(),
                        _ => ", ",
                    };
                    let parts: Vec<String> = arr.iter().map(|v| v.to_display_string()).collect();
                    return Ok(Value::String(parts.join(sep)));
                }
                _ => {}
            }
        }

        // ── Методи рядків ──
        if let Value::String(ref s) = obj {
            match method {
                "довжина" => return Ok(Value::Integer(s.chars().count() as i64)),
                "пусто" => return Ok(Value::Bool(s.is_empty())),
                "містить" => {
                    if let Some(Value::String(sub)) = args.first() {
                        return Ok(Value::Bool(s.contains(sub.as_str())));
                    }
                    return Ok(Value::Bool(false));
                }
                "починається_з" => {
                    if let Some(Value::String(pre)) = args.first() {
                        return Ok(Value::Bool(s.starts_with(pre.as_str())));
                    }
                    return Ok(Value::Bool(false));
                }
                "закінчується_на" => {
                    if let Some(Value::String(suf)) = args.first() {
                        return Ok(Value::Bool(s.ends_with(suf.as_str())));
                    }
                    return Ok(Value::Bool(false));
                }
                "великими" => return Ok(Value::String(s.to_uppercase())),
                "малими" => return Ok(Value::String(s.to_lowercase())),
                "обрізати" => return Ok(Value::String(s.trim().to_string())),
                "розділити" => {
                    let sep = match args.first() {
                        Some(Value::String(sep)) => sep.clone(),
                        _ => " ".to_string(),
                    };
                    let parts: Vec<Value> = s.split(&sep).map(|p| Value::String(p.to_string())).collect();
                    return Ok(Value::Array(parts));
                }
                "замінити" => {
                    if args.len() == 2 {
                        if let (Value::String(from), Value::String(to)) = (&args[0], &args[1]) {
                            return Ok(Value::String(s.replace(from.as_str(), to.as_str())));
                        }
                    }
                    return Err(anyhow::anyhow!(".замінити() потребує 2 рядки"));
                }
                "підрядок" => {
                    if args.len() == 2 {
                        if let (Value::Integer(from), Value::Integer(to)) = (&args[0], &args[1]) {
                            let from = *from as usize;
                            let to = (*to as usize).min(s.chars().count());
                            let sub: String = s.chars().skip(from).take(to - from).collect();
                            return Ok(Value::String(sub));
                        }
                    }
                    return Err(anyhow::anyhow!(".підрядок() потребує 2 числа"));
                }
                _ => {}
            }
        }

        // ── Методи словників ──
        if let Value::Dict(ref pairs) = obj {
            match method {
                "довжина" => return Ok(Value::Integer(pairs.len() as i64)),
                "ключі" => return Ok(Value::Array(pairs.iter().map(|(k, _)| k.clone()).collect())),
                "значення" => return Ok(Value::Array(pairs.iter().map(|(_, v)| v.clone()).collect())),
                "містить_ключ" => {
                    if let Some(key) = args.first() {
                        return Ok(Value::Bool(pairs.iter().any(|(k, _)| self.values_equal(k, key))));
                    }
                    return Ok(Value::Bool(false));
                }
                "отримати" => {
                    if let Some(key) = args.first() {
                        for (k, v) in pairs {
                            if self.values_equal(k, key) { return Ok(v.clone()); }
                        }
                        return Ok(Value::Null);
                    }
                    return Ok(Value::Null);
                }
                "додати" => {
                    if args.len() == 2 {
                        let mut new_pairs = pairs.clone();
                        // Видаляємо якщо ключ вже є
                        new_pairs.retain(|(k, _)| !self.values_equal(k, &args[0]));
                        new_pairs.push((args[0].clone(), args[1].clone()));
                        return Ok(Value::Dict(new_pairs));
                    }
                    return Err(anyhow::anyhow!("словник.додати потребує ключ та значення"));
                }
                "видалити" => {
                    if let Some(key) = args.first() {
                        let new_pairs: Vec<(Value, Value)> = pairs.iter()
                            .filter(|(k, _)| !self.values_equal(k, key))
                            .cloned().collect();
                        return Ok(Value::Dict(new_pairs));
                    }
                    return Err(anyhow::anyhow!("словник.видалити потребує ключ"));
                }
                _ => {}
            }
        }

        // ── Методи множин ──
        if let Value::Set(ref items) = obj {
            match method {
                "довжина" => return Ok(Value::Integer(items.len() as i64)),
                "містить" => {
                    if let Some(val) = args.first() {
                        return Ok(Value::Bool(items.iter().any(|v| self.values_equal(v, val))));
                    }
                    return Ok(Value::Bool(false));
                }
                "додати" => {
                    if let Some(val) = args.first() {
                        let mut new_items = items.clone();
                        if !new_items.iter().any(|v| self.values_equal(v, val)) {
                            new_items.push(val.clone());
                        }
                        return Ok(Value::Set(new_items));
                    }
                    return Err(anyhow::anyhow!("множина.додати потребує елемент"));
                }
                "перетин" => {
                    if let Some(Value::Set(other)) = args.first() {
                        let result: Vec<Value> = items.iter()
                            .filter(|v| other.iter().any(|o| self.values_equal(v, o)))
                            .cloned().collect();
                        return Ok(Value::Set(result));
                    }
                    return Err(anyhow::anyhow!("множина.перетин потребує множину"));
                }
                "об_єднання" | "об'єднання" => {
                    if let Some(Value::Set(other)) = args.first() {
                        let mut result = items.clone();
                        for o in other {
                            if !result.iter().any(|v| self.values_equal(v, o)) {
                                result.push(o.clone());
                            }
                        }
                        return Ok(Value::Set(result));
                    }
                    return Err(anyhow::anyhow!("множина.об'єднання потребує множину"));
                }
                "різниця" => {
                    if let Some(Value::Set(other)) = args.first() {
                        let result: Vec<Value> = items.iter()
                            .filter(|v| !other.iter().any(|o| self.values_equal(v, o)))
                            .cloned().collect();
                        return Ok(Value::Set(result));
                    }
                    return Err(anyhow::anyhow!("множина.різниця потребує множину"));
                }
                _ => {}
            }
        }

        // ── Методи генераторів ──
        if let Value::Generator { ref body, ref closure, current_index, .. } = obj {
            // ID генератора = current_index (встановлюється при створенні)
            let gen_id = current_index;
            match method {
                "в_масив" | "наступний" | "взяти" => {
                    // Виконуємо тіло ТІЛЬКИ ОДИН РАЗ, кешуємо результат
                    if !self.generator_cache.contains_key(&gen_id) {
                        let collected = self.execute_generator(body.clone(), closure.clone())?;
                        self.generator_cache.insert(gen_id, (collected, 0));
                    }

                    match method {
                        "в_масив" => {
                            let (values, _) = self.generator_cache.get(&gen_id).unwrap();
                            return Ok(Value::Array(values.clone()));
                        }
                        "взяти" => {
                            let n = match args.first() {
                                Some(Value::Integer(n)) => *n as usize,
                                _ => usize::MAX,
                            };
                            let (values, _) = self.generator_cache.get(&gen_id).unwrap();
                            return Ok(Value::Array(values.iter().take(n).cloned().collect()));
                        }
                        "наступний" => {
                            let (values, idx) = self.generator_cache.get_mut(&gen_id).unwrap();
                            if *idx < values.len() {
                                let val = values[*idx].clone();
                                *idx += 1;
                                return Ok(Value::EnumVariant {
                                    type_name: "Опція".to_string(),
                                    variant: "Деякий".to_string(),
                                    fields: vec![val],
                                });
                            }
                            return Ok(Value::EnumVariant {
                                type_name: "Опція".to_string(),
                                variant: "Нічого".to_string(),
                                fields: vec![],
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // ── Методи цілих чисел ──
        if let Value::Integer(n) = &obj {
            match method {
                "парне" => return Ok(Value::Bool(n % 2 == 0)),
                "непарне" => return Ok(Value::Bool(n % 2 != 0)),
                "абс" => return Ok(Value::Integer(n.abs())),
                "в_текст" => return Ok(Value::String(n.to_string())),
                "в_дробове" => return Ok(Value::Float(*n as f64)),
                "степінь" => {
                    if let Some(Value::Integer(exp)) = args.first() {
                        return Ok(Value::Integer(n.pow(*exp as u32)));
                    }
                    return Err(anyhow::anyhow!(".степінь() потребує ціле число"));
                }
                "мін" => {
                    if let Some(Value::Integer(other)) = args.first() {
                        return Ok(Value::Integer(*n.min(other)));
                    }
                    return Err(anyhow::anyhow!(".мін() потребує число"));
                }
                "макс" => {
                    if let Some(Value::Integer(other)) = args.first() {
                        return Ok(Value::Integer(*n.max(other)));
                    }
                    return Err(anyhow::anyhow!(".макс() потребує число"));
                }
                _ => {}
            }
        }

        // ── Методи дробових чисел ──
        if let Value::Float(f) = &obj {
            match method {
                "абс" => return Ok(Value::Float(f.abs())),
                "округлити" => return Ok(Value::Integer(f.round() as i64)),
                "підлога" => return Ok(Value::Integer(f.floor() as i64)),
                "стеля" => return Ok(Value::Integer(f.ceil() as i64)),
                "в_текст" => return Ok(Value::String(f.to_string())),
                "в_ціле" => return Ok(Value::Integer(*f as i64)),
                "нескінченний" => return Ok(Value::Bool(f.is_infinite())),
                "число" => return Ok(Value::Bool(f.is_finite() && !f.is_nan())),
                "корінь" => return Ok(Value::Float(f.sqrt())),
                _ => {}
            }
        }

        // ── Зареєстровані методи (трейти/реалізації) ──
        let type_name = match &obj {
            Value::Struct(name, _) => name.clone(),
            Value::EnumVariant { type_name, .. } => type_name.clone(),
            _ => obj.type_name().to_string(),
        };

        let method_key = format!("{}::{}", type_name, method);
        let maybe_func = self.current_env.borrow().get(&method_key);
        if let Some(func) = maybe_func {
            let mut all_args = vec![obj];
            all_args.extend(args);
            return self.call_value(func, all_args);
        }

        Err(anyhow::anyhow!("Метод '{}' не знайдено для типу {}", method, type_name))
    }

    fn call_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value> {
        match name {
            // ── Базові ──
            "друк" => {
                let parts: Vec<String> = args.iter().map(|v| v.to_display_string()).collect();
                println!("{}", parts.join(" "));
                Ok(Value::Null)
            }
            "цілеврядок" => {
                match args.first() {
                    Some(v) => Ok(Value::String(v.to_display_string())),
                    None => Err(anyhow::anyhow!("цілеврядок очікує 1 аргумент")),
                }
            }
            "довжина" => {
                match args.first() {
                    Some(Value::Array(arr)) => Ok(Value::Integer(arr.len() as i64)),
                    Some(Value::String(s)) => Ok(Value::Integer(s.chars().count() as i64)),
                    _ => Err(anyhow::anyhow!("довжина підтримує масиви та рядки")),
                }
            }
            "тип_значення" => {
                match args.first() {
                    Some(v) => Ok(Value::String(v.type_name().to_string())),
                    None => Err(anyhow::anyhow!("тип_значення очікує 1 аргумент")),
                }
            }
            "паніка" => {
                let msg = args.first().map(|v| v.to_display_string()).unwrap_or_default();
                Err(anyhow::anyhow!("Паніка: {}", msg))
            }

            // ── Опція/Результат конструктори ──
            "Деякий" => Ok(Value::EnumVariant {
                type_name: "Опція".to_string(), variant: "Деякий".to_string(), fields: args,
            }),
            "Успіх" => Ok(Value::EnumVariant {
                type_name: "Результат".to_string(), variant: "Успіх".to_string(), fields: args,
            }),
            "Помилка" => Ok(Value::EnumVariant {
                type_name: "Результат".to_string(), variant: "Помилка".to_string(), fields: args,
            }),

            // ── Колекції: повна реалізація з каррінгом для pipeline ──

            "фільтрувати" => {
                if args.len() == 2 {
                    let arr = match &args[0] { Value::Array(a) => a.clone(), _ => return Err(anyhow::anyhow!("фільтрувати: перший аргумент має бути масивом")) };
                    let func = args[1].clone();
                    let mut result = Vec::new();
                    for item in arr {
                        let cond = self.call_value(func.clone(), vec![item.clone()])?;
                        if cond.to_bool() { result.push(item); }
                    }
                    Ok(Value::Array(result))
                } else if args.len() == 1 {
                    Ok(self.curry_builtin("фільтрувати", args))
                } else { Err(anyhow::anyhow!("фільтрувати очікує 1-2 аргументи")) }
            }
            "перетворити" => {
                if args.len() == 2 {
                    let arr = match &args[0] { Value::Array(a) => a.clone(), _ => return Err(anyhow::anyhow!("перетворити: перший аргумент має бути масивом")) };
                    let func = args[1].clone();
                    let mut result = Vec::new();
                    for item in arr {
                        result.push(self.call_value(func.clone(), vec![item])?);
                    }
                    Ok(Value::Array(result))
                } else if args.len() == 1 {
                    Ok(self.curry_builtin("перетворити", args))
                } else { Err(anyhow::anyhow!("перетворити очікує 1-2 аргументи")) }
            }
            "згорнути" => {
                if args.len() == 3 {
                    let arr = match &args[0] { Value::Array(a) => a.clone(), _ => return Err(anyhow::anyhow!("згорнути: перший аргумент має бути масивом")) };
                    let mut acc = args[1].clone();
                    let func = args[2].clone();
                    for item in arr { acc = self.call_value(func.clone(), vec![acc, item])?; }
                    Ok(acc)
                } else if args.len() == 2 {
                    Ok(self.curry_builtin("згорнути", args))
                } else { Err(anyhow::anyhow!("згорнути очікує 2-3 аргументи")) }
            }
            "сортувати" => {
                match args.first() {
                    Some(Value::Array(arr)) => {
                        let mut sorted = arr.clone();
                        sorted.sort_by(|a, b| {
                            match (a, b) {
                                (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
                                (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                                (Value::String(x), Value::String(y)) => x.cmp(y),
                                _ => std::cmp::Ordering::Equal,
                            }
                        });
                        Ok(Value::Array(sorted))
                    }
                    _ => Err(anyhow::anyhow!("сортувати очікує масив")),
                }
            }
            "обернути" => {
                match args.first() {
                    Some(Value::Array(arr)) => {
                        let mut rev = arr.clone();
                        rev.reverse();
                        Ok(Value::Array(rev))
                    }
                    Some(Value::String(s)) => {
                        Ok(Value::String(s.chars().rev().collect()))
                    }
                    _ => Err(anyhow::anyhow!("обернути очікує масив або рядок")),
                }
            }
            "додати" => {
                // додати(масив, елемент)
                if args.len() == 2 {
                    if let Value::Array(mut arr) = args[0].clone() {
                        arr.push(args[1].clone());
                        Ok(Value::Array(arr))
                    } else {
                        Err(anyhow::anyhow!("додати очікує масив як перший аргумент"))
                    }
                } else { Err(anyhow::anyhow!("додати очікує 2 аргументи")) }
            }
            "діапазон" => {
                // діапазон(від, до) — створює масив
                if args.len() == 2 {
                    match (&args[0], &args[1]) {
                        (Value::Integer(from), Value::Integer(to)) => {
                            Ok(Value::Array((*from..*to).map(Value::Integer).collect()))
                        }
                        _ => Err(anyhow::anyhow!("діапазон очікує два цілі числа")),
                    }
                } else { Err(anyhow::anyhow!("діапазон очікує 2 аргументи")) }
            }

            "виконати_ефект" => {
                // виконати_ефект("ім'я_ефекту", "операція", аргументи...)
                if args.len() >= 2 {
                    let effect_name = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Перший аргумент має бути ім'ям ефекту")) };
                    let operation = match &args[1] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Другий аргумент має бути ім'ям операції")) };
                    let effect_args = args[2..].to_vec();
                    self.perform_effect(&effect_name, &operation, effect_args)
                } else {
                    Err(anyhow::anyhow!("виконати_ефект потребує мінімум 2 аргументи"))
                }
            }
            "мін" => {
                if args.len() == 2 {
                    match (&args[0], &args[1]) {
                        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(*a.min(b))),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.min(*b))),
                        _ => Err(anyhow::anyhow!("мін очікує два числа")),
                    }
                } else { Err(anyhow::anyhow!("мін очікує 2 аргументи")) }
            }
            "макс" => {
                if args.len() == 2 {
                    match (&args[0], &args[1]) {
                        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(*a.max(b))),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.max(*b))),
                        _ => Err(anyhow::anyhow!("макс очікує два числа")),
                    }
                } else { Err(anyhow::anyhow!("макс очікує 2 аргументи")) }
            }
            "абс" => {
                match args.first() {
                    Some(Value::Integer(n)) => Ok(Value::Integer(n.abs())),
                    Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
                    _ => Err(anyhow::anyhow!("абс очікує число")),
                }
            }
            // ── Генератори ──
            "генератор" => {
                // генератор(функція) — створює генератор з функції що містить віддати
                match args.first() {
                    Some(Value::Function { body, closure, .. }) => {
                        self.generator_id_counter += 1;
                        Ok(Value::Generator {
                            params: vec![],
                            body: body.clone(),
                            closure: closure.clone(),
                            yielded_values: vec![],
                            current_index: self.generator_id_counter,
                            executed: false,
                        })
                    }
                    Some(Value::Lambda { body: LambdaBody::Block(stmts), closure, .. }) => {
                        self.generator_id_counter += 1;
                        Ok(Value::Generator {
                            params: vec![],
                            body: stmts.clone(),
                            closure: closure.clone(),
                            yielded_values: vec![],
                            current_index: self.generator_id_counter,
                            executed: false,
                        })
                    }
                    _ => Err(anyhow::anyhow!("генератор очікує функцію")),
                }
            }

            // ── Ввід ──
            "ввід" => {
                use std::io::{self, Write, BufRead};
                // Якщо є аргумент — друкуємо як підказку
                if let Some(Value::String(prompt)) = args.first() {
                    print!("{}", prompt);
                    io::stdout().flush().ok();
                }
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line).ok();
                Ok(Value::String(line.trim_end_matches('\n').trim_end_matches('\r').to_string()))
            }

            // ── Час ──
            "час_зараз" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                Ok(Value::Float(now.as_secs_f64() * 1000.0)) // мілісекунди
            }
            "час_затримка" => {
                match args.first() {
                    Some(Value::Integer(ms)) => {
                        std::thread::sleep(std::time::Duration::from_millis(*ms as u64));
                        Ok(Value::Null)
                    }
                    _ => Err(anyhow::anyhow!("час_затримка очікує мілісекунди")),
                }
            }

            // ── JSON ──
            "json_розібрати" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        match serde_json::from_str::<serde_json::Value>(s) {
                            Ok(json) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Успіх".to_string(),
                                fields: vec![VM::json_to_value(&json)],
                            }),
                            Err(e) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String(e.to_string())],
                            }),
                        }
                    }
                    _ => Err(anyhow::anyhow!("json_розібрати очікує рядок")),
                }
            }
            "json_в_рядок" => {
                match args.first() {
                    Some(val) => {
                        let json = VM::value_to_json(val);
                        Ok(Value::String(json.to_string()))
                    }
                    None => Err(anyhow::anyhow!("json_в_рядок очікує значення")),
                }
            }
            "json_в_рядок_красиво" => {
                match args.first() {
                    Some(val) => {
                        let json = VM::value_to_json(val);
                        Ok(Value::String(serde_json::to_string_pretty(&json).unwrap_or_default()))
                    }
                    None => Err(anyhow::anyhow!("json_в_рядок_красиво очікує значення")),
                }
            }

            // ── Математика (нативна) ──
            "корінь" => {
                match args.first() {
                    Some(Value::Float(f)) => Ok(Value::Float(f.sqrt())),
                    Some(Value::Integer(n)) => Ok(Value::Float((*n as f64).sqrt())),
                    _ => Err(anyhow::anyhow!("корінь очікує число")),
                }
            }
            "синус" => {
                match args.first() {
                    Some(Value::Float(f)) => Ok(Value::Float(f.sin())),
                    Some(Value::Integer(n)) => Ok(Value::Float((*n as f64).sin())),
                    _ => Err(anyhow::anyhow!("синус очікує число")),
                }
            }
            "косинус" => {
                match args.first() {
                    Some(Value::Float(f)) => Ok(Value::Float(f.cos())),
                    Some(Value::Integer(n)) => Ok(Value::Float((*n as f64).cos())),
                    _ => Err(anyhow::anyhow!("косинус очікує число")),
                }
            }
            "степінь_ф" => {
                if args.len() == 2 {
                    let base = match &args[0] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => return Err(anyhow::anyhow!("очікується число")) };
                    let exp = match &args[1] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => return Err(anyhow::anyhow!("очікується число")) };
                    Ok(Value::Float(base.powf(exp)))
                } else { Err(anyhow::anyhow!("степінь_ф очікує 2 аргументи")) }
            }
            "логарифм" => {
                match args.first() {
                    Some(Value::Float(f)) => Ok(Value::Float(f.ln())),
                    Some(Value::Integer(n)) => Ok(Value::Float((*n as f64).ln())),
                    _ => Err(anyhow::anyhow!("логарифм очікує число")),
                }
            }
            "ціле_з_рядка" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        match s.trim().parse::<i64>() {
                            Ok(n) => Ok(Value::EnumVariant { type_name: "Результат".to_string(), variant: "Успіх".to_string(), fields: vec![Value::Integer(n)] }),
                            Err(e) => Ok(Value::EnumVariant { type_name: "Результат".to_string(), variant: "Помилка".to_string(), fields: vec![Value::String(e.to_string())] }),
                        }
                    }
                    _ => Err(anyhow::anyhow!("ціле_з_рядка очікує рядок")),
                }
            }
            "дробове_з_рядка" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        match s.trim().parse::<f64>() {
                            Ok(f) => Ok(Value::EnumVariant { type_name: "Результат".to_string(), variant: "Успіх".to_string(), fields: vec![Value::Float(f)] }),
                            Err(e) => Ok(Value::EnumVariant { type_name: "Результат".to_string(), variant: "Помилка".to_string(), fields: vec![Value::String(e.to_string())] }),
                        }
                    }
                    _ => Err(anyhow::anyhow!("дробове_з_рядка очікує рядок")),
                }
            }

            // ── Файловий I/O ──
            "файл_прочитати" => {
                match args.first() {
                    Some(Value::String(path)) => {
                        match std::fs::read_to_string(path) {
                            Ok(content) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Успіх".to_string(),
                                fields: vec![Value::String(content)],
                            }),
                            Err(e) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String(e.to_string())],
                            }),
                        }
                    }
                    _ => Err(anyhow::anyhow!("файл_прочитати очікує шлях (тхт)")),
                }
            }
            "файл_записати" => {
                if args.len() == 2 {
                    if let (Value::String(path), Value::String(content)) = (&args[0], &args[1]) {
                        match std::fs::write(path, content) {
                            Ok(()) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Успіх".to_string(),
                                fields: vec![Value::Null],
                            }),
                            Err(e) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String(e.to_string())],
                            }),
                        }
                    } else { Err(anyhow::anyhow!("файл_записати очікує (шлях, зміст)")) }
                } else { Err(anyhow::anyhow!("файл_записати очікує 2 аргументи")) }
            }
            "файл_існує" => {
                match args.first() {
                    Some(Value::String(path)) => Ok(Value::Bool(std::path::Path::new(path).exists())),
                    _ => Err(anyhow::anyhow!("файл_існує очікує шлях")),
                }
            }
            "файл_рядки" => {
                match args.first() {
                    Some(Value::String(path)) => {
                        match std::fs::read_to_string(path) {
                            Ok(content) => {
                                let lines: Vec<Value> = content.lines().map(|l| Value::String(l.to_string())).collect();
                                Ok(Value::EnumVariant {
                                    type_name: "Результат".to_string(),
                                    variant: "Успіх".to_string(),
                                    fields: vec![Value::Array(lines)],
                                })
                            }
                            Err(e) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String(e.to_string())],
                            }),
                        }
                    }
                    _ => Err(anyhow::anyhow!("файл_рядки очікує шлях")),
                }
            }
            "файл_додати" => {
                if args.len() == 2 {
                    if let (Value::String(path), Value::String(content)) = (&args[0], &args[1]) {
                        use std::io::Write;
                        match std::fs::OpenOptions::new().append(true).create(true).open(path) {
                            Ok(mut file) => {
                                let _ = file.write_all(content.as_bytes());
                                Ok(Value::EnumVariant {
                                    type_name: "Результат".to_string(),
                                    variant: "Успіх".to_string(),
                                    fields: vec![Value::Null],
                                })
                            }
                            Err(e) => Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String(e.to_string())],
                            }),
                        }
                    } else { Err(anyhow::anyhow!("файл_додати очікує (шлях, зміст)")) }
                } else { Err(anyhow::anyhow!("файл_додати очікує 2 аргументи")) }
            }

            "словник" => {
                let mut pairs = Vec::new();
                let mut i = 0;
                while i + 1 < args.len() {
                    pairs.push((args[i].clone(), args[i + 1].clone()));
                    i += 2;
                }
                Ok(Value::Dict(pairs))
            }
            "множина" => {
                // множина(елемент1, елемент2, ...)
                Ok(Value::Set(args))
            }

            // ── Макроси ──
            _ if name.starts_with("__macro_") => {
                let macro_name = &name[8..]; // skip "__macro_"
                if let Some((params, body)) = self.macros.get(macro_name).cloned() {
                    let prev_env = self.current_env.clone();
                    self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));

                    // Прив'язуємо аргументи
                    for (param, arg) in params.iter().zip(args.iter()) {
                        self.current_env.borrow_mut().set(param.clone(), arg.clone());
                    }

                    // Виконуємо тіло макросу
                    let prev_return = self.return_value.take();
                    let mut last_val = Value::Null;
                    for (i, stmt) in body.iter().enumerate() {
                        if i == body.len() - 1 {
                            if let Statement::Expression(expr) = stmt {
                                last_val = self.evaluate_expression(expr.clone())?;
                                break;
                            }
                        }
                        self.execute_statement(stmt.clone())?;
                        if self.return_value.is_some() { break; }
                    }
                    let result = self.return_value.take().unwrap_or(last_val);
                    self.return_value = prev_return;
                    self.current_env = prev_env;
                    return Ok(result);
                }
                Err(anyhow::anyhow!("Макрос '{}' не знайдено", macro_name))
            }

            // ── Enum конструктор ──
            _ if name.contains("::") => {
                let parts: Vec<&str> = name.split("::").collect();
                if parts.len() == 2 {
                    Ok(Value::EnumVariant {
                        type_name: parts[0].to_string(),
                        variant: parts[1].to_string(),
                        fields: args,
                    })
                } else {
                    Err(anyhow::anyhow!("Невідома функція: {}", name))
                }
            }
            _ => Err(anyhow::anyhow!("Невідома вбудована функція: {}", name))
        }
    }

    /// Каррінг: зберігає аргументи та повертає CurriedBuiltin
    fn curry_builtin(&self, name: &str, args: Vec<Value>) -> Value {
        Value::CurriedBuiltin {
            name: name.to_string(),
            saved_args: args,
        }
    }

    // ── Завантаження модулів ──

    /// Виконує тіло генератора та збирає yielded значення
    fn execute_generator(&mut self, body: Vec<Statement>, closure: Environment) -> Result<Vec<Value>> {
        let prev_yielded = std::mem::take(&mut self.yielded_values);
        let prev_env = self.current_env.clone();
        self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

        for stmt in body {
            self.execute_statement(stmt).ok(); // Ігноруємо помилки break/return
            if self.return_value.is_some() { break; }
        }
        self.return_value = None;

        self.current_env = prev_env;
        let collected = std::mem::replace(&mut self.yielded_values, prev_yielded);
        Ok(collected)
    }

    /// Додає шлях для пошуку модулів (відносно поточного файлу)
    pub fn add_module_path(&mut self, path: String) {
        if !self.stdlib_paths.contains(&path) {
            self.stdlib_paths.insert(0, path);
        }
    }

    fn load_module(&mut self, name: &str) -> Result<()> {
        let filenames = vec![
            format!("{}.тризуб", name),
            format!("{}.tryzub", name),
        ];

        // Шукаємо у: 1) робоча директорія, 2) stdlib/, 3) ../stdlib/
        let mut search_paths = self.stdlib_paths.clone();
        search_paths.insert(0, ".".to_string());

        // Також шукаємо у вкладених директоріях (ядро/модуль)
        let sub_filenames: Vec<String> = vec![
            format!("{}/{}.тризуб", name, name),
            format!("ядро/{}.тризуб", name),
        ];

        for base_path in &search_paths {
            for filename in filenames.iter().chain(sub_filenames.iter()) {
                let path = format!("{}/{}", base_path, filename);
                if let Ok(source) = std::fs::read_to_string(&path) {
                    let tokens = tryzub_lexer::tokenize(&source)?;
                    let program = tryzub_parser::parse(tokens)?;

                    for decl in program.declarations {
                        self.execute_declaration(decl)?;
                    }

                    self.loaded_modules.insert(name.to_string(), true);
                    return Ok(());
                }
            }
        }

        // Модуль не знайдено — не помилка, просто попередження
        self.loaded_modules.insert(name.to_string(), false);
        Ok(())
    }

    /// Перевіряє чи є активний обробник для даного ефекту
    fn find_effect_handler(&self, effect_name: &str) -> Option<&(String, Environment)> {
        self.effect_handlers.iter().rev().find(|(name, _)| name == effect_name)
    }

    /// Виконати ефект — шукає обробник у стеку та викликає його
    fn perform_effect(&mut self, effect_name: &str, operation: &str, args: Vec<Value>) -> Result<Value> {
        // Шукаємо обробник
        if let Some((handler_name, handler_env)) = self.find_effect_handler(effect_name).cloned() {
            // Шукаємо функцію обробника в його середовищі
            let handler_fn_name = format!("{}_{}", handler_name, operation);
            if let Some(func) = handler_env.borrow().get(&handler_fn_name) {
                return self.call_value(func, args);
            }
            // Якщо конкретного обробника немає — логуємо та виконуємо за замовчуванням
            if let Some(func) = handler_env.borrow().get(&format!("{}_за_замовчуванням", handler_name)) {
                return self.call_value(func, vec![Value::String(operation.to_string())]);
            }
        }

        // Немає обробника — помилка
        Err(anyhow::anyhow!("Ефект '{}::{}' не оброблено — немає активного обробника", effect_name, operation))
    }

    /// Виконує async завдання з черги
    fn drain_async_queue(&mut self) -> Result<()> {
        while let Some((stmts, env)) = self.async_queue.pop() {
            let prev_env = self.current_env.clone();
            self.current_env = env;
            for stmt in stmts {
                self.execute_statement(stmt)?;
            }
            self.current_env = prev_env;
        }
        Ok(())
    }

    // ── Pattern Matching ──

    fn evaluate_match(&mut self, value: Value, arms: Vec<MatchArm>) -> Result<Value> {
        for arm in arms {
            if self.pattern_matches(&arm.pattern, &value)? {
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                self.bind_pattern(&arm.pattern, &value)?;
                let result = self.evaluate_expression(arm.body)?;
                self.current_env = prev_env;
                return Ok(result);
            }
        }
        Err(anyhow::anyhow!("Жоден зразок не збігся"))
    }

    fn pattern_matches(&mut self, pattern: &Pattern, value: &Value) -> Result<bool> {
        match pattern {
            Pattern::Wildcard => Ok(true),
            Pattern::Literal(lit) => {
                let lit_val = self.evaluate_literal(lit.clone());
                Ok(self.values_equal(&lit_val, value))
            }
            Pattern::Binding(_) => Ok(true),
            Pattern::Variant { name, fields } => {
                match value {
                    Value::EnumVariant { variant, fields: val_fields, .. } => {
                        if name != variant { return Ok(false); }
                        if fields.len() != val_fields.len() { return Ok(false); }
                        for (pat, val) in fields.iter().zip(val_fields.iter()) {
                            if !self.pattern_matches(pat, val)? { return Ok(false); }
                        }
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            Pattern::Tuple(patterns) => {
                if let Value::Tuple(values) = value {
                    if patterns.len() != values.len() { return Ok(false); }
                    for (p, v) in patterns.iter().zip(values.iter()) {
                        if !self.pattern_matches(p, v)? { return Ok(false); }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Pattern::Array { elements, .. } => {
                if let Value::Array(arr) = value {
                    if elements.len() > arr.len() { return Ok(false); }
                    for (p, v) in elements.iter().zip(arr.iter()) {
                        if !self.pattern_matches(p, v)? { return Ok(false); }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Pattern::Guard { pattern, condition } => {
                if !self.pattern_matches(pattern, value)? { return Ok(false); }
                // Тимчасово прив'язуємо значення для перевірки умови
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(self.current_env.clone()))));
                self.bind_pattern(pattern, value)?;
                let cond = self.evaluate_expression(*condition.clone())?;
                self.current_env = prev_env;
                Ok(cond.to_bool())
            }
            Pattern::Or(patterns) => {
                for p in patterns {
                    if self.pattern_matches(p, value)? { return Ok(true); }
                }
                Ok(false)
            }
            Pattern::Struct { fields, .. } => {
                if let Value::Struct(_, val_fields) = value {
                    for (name, sub_pat) in fields {
                        if let Some(val) = val_fields.get(name) {
                            if let Some(p) = sub_pat {
                                if !self.pattern_matches(p, val)? { return Ok(false); }
                            }
                        } else {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &Value) -> Result<()> {
        match pattern {
            Pattern::Wildcard => {}
            Pattern::Literal(_) => {}
            Pattern::Binding(name) => {
                self.current_env.borrow_mut().set(name.clone(), value.clone());
            }
            Pattern::Variant { fields, .. } => {
                if let Value::EnumVariant { fields: val_fields, .. } = value {
                    for (pat, val) in fields.iter().zip(val_fields.iter()) {
                        self.bind_pattern(pat, val)?;
                    }
                }
            }
            Pattern::Tuple(patterns) => {
                if let Value::Tuple(values) = value {
                    for (p, v) in patterns.iter().zip(values.iter()) {
                        self.bind_pattern(p, v)?;
                    }
                }
            }
            Pattern::Array { elements, rest } => {
                if let Value::Array(arr) = value {
                    for (i, p) in elements.iter().enumerate() {
                        if let Some(v) = arr.get(i) {
                            self.bind_pattern(p, v)?;
                        }
                    }
                    if let Some(rest_name) = rest {
                        let rest_vals = arr[elements.len()..].to_vec();
                        self.current_env.borrow_mut().set(rest_name.clone(), Value::Array(rest_vals));
                    }
                }
            }
            Pattern::Struct { fields, .. } => {
                if let Value::Struct(_, val_fields) = value {
                    for (name, sub_pat) in fields {
                        if let Some(val) = val_fields.get(name) {
                            if let Some(p) = sub_pat {
                                self.bind_pattern(p, val)?;
                            } else {
                                self.current_env.borrow_mut().set(name.clone(), val.clone());
                            }
                        }
                    }
                }
            }
            Pattern::Guard { pattern, .. } => {
                self.bind_pattern(pattern, value)?;
            }
            Pattern::Or(patterns) => {
                for p in patterns {
                    if self.pattern_matches(p, value)? {
                        self.bind_pattern(p, value)?;
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    // ── Допоміжні методи ──

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
        match (op, &lhs, &rhs) {
            // Арифметика цілих
            (BinaryOp::Add, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (BinaryOp::Sub, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
            (BinaryOp::Mul, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
            (BinaryOp::Div, Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 { Err(anyhow::anyhow!("Ділення на нуль")) }
                else { Ok(Value::Integer(a / b)) }
            }
            (BinaryOp::Mod, Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 { Err(anyhow::anyhow!("Ділення на нуль")) }
                else { Ok(Value::Integer(a % b)) }
            }
            (BinaryOp::Pow, Value::Integer(a), Value::Integer(b)) => {
                if *b < 0 { Ok(Value::Float((*a as f64).powf(*b as f64))) }
                else { Ok(Value::Integer(a.pow(*b as u32))) }
            }

            // Арифметика дробових
            (BinaryOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinaryOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (BinaryOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (BinaryOp::Div, Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 { Err(anyhow::anyhow!("Ділення на нуль")) }
                else { Ok(Value::Float(a / b)) }
            }
            (BinaryOp::Pow, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),

            // Змішані числа
            (BinaryOp::Add, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (BinaryOp::Add, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
            (BinaryOp::Sub, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (BinaryOp::Sub, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a - *b as f64)),
            (BinaryOp::Mul, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (BinaryOp::Mul, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a * *b as f64)),
            (BinaryOp::Div, Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (BinaryOp::Div, Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a / *b as f64)),

            // Конкатенація рядків
            (BinaryOp::Add, Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            (BinaryOp::Add, Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b.to_display_string()))),
            (BinaryOp::Add, a, Value::String(b)) => Ok(Value::String(format!("{}{}", a.to_display_string(), b))),

            // Порівняння
            (BinaryOp::Eq, a, b) => Ok(Value::Bool(self.values_equal(a, b))),
            (BinaryOp::Ne, a, b) => Ok(Value::Bool(!self.values_equal(a, b))),

            (BinaryOp::Lt, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a < b)),
            (BinaryOp::Lt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
            (BinaryOp::Le, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a <= b)),
            (BinaryOp::Le, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
            (BinaryOp::Gt, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a > b)),
            (BinaryOp::Gt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
            (BinaryOp::Ge, Value::Integer(a), Value::Integer(b)) => Ok(Value::Bool(a >= b)),
            (BinaryOp::Ge, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),

            // Логічні
            (BinaryOp::And, a, b) => Ok(Value::Bool(a.to_bool() && b.to_bool())),
            (BinaryOp::Or, a, b) => Ok(Value::Bool(a.to_bool() || b.to_bool())),

            // Належність (оператор "в")
            (BinaryOp::In, _, Value::Array(arr)) => {
                Ok(Value::Bool(arr.iter().any(|v| self.values_equal(&lhs, v))))
            }
            (BinaryOp::In, _, Value::Set(items)) => {
                Ok(Value::Bool(items.iter().any(|v| self.values_equal(&lhs, v))))
            }
            (BinaryOp::In, _, Value::Dict(pairs)) => {
                Ok(Value::Bool(pairs.iter().any(|(k, _)| self.values_equal(&lhs, k))))
            }
            (BinaryOp::In, Value::Integer(val), Value::Range { from, to, inclusive }) => {
                if *inclusive {
                    Ok(Value::Bool(*val >= *from && *val <= *to))
                } else {
                    Ok(Value::Bool(*val >= *from && *val < *to))
                }
            }
            (BinaryOp::In, Value::Char(_), Value::String(s)) => {
                if let Value::Char(c) = &lhs {
                    Ok(Value::Bool(s.contains(*c)))
                } else { Ok(Value::Bool(false)) }
            }
            (BinaryOp::In, Value::String(sub), Value::String(s)) => {
                Ok(Value::Bool(s.contains(sub.as_str())))
            }

            // Побітові
            (BinaryOp::BitAnd, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a & b)),
            (BinaryOp::BitOr, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a | b)),
            (BinaryOp::BitXor, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a ^ b)),
            (BinaryOp::Shl, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a << b)),
            (BinaryOp::Shr, Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a >> b)),

            _ => Err(anyhow::anyhow!("Несумісні типи для операції {:?}: {} та {}",
                op, lhs.type_name(), rhs.type_name())),
        }
    }

    fn apply_unary_op(&self, op: UnaryOp, val: Value) -> Result<Value> {
        match (op, &val) {
            (UnaryOp::Neg, Value::Integer(n)) => Ok(Value::Integer(-n)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, _) => Ok(Value::Bool(!val.to_bool())),
            (UnaryOp::BitNot, Value::Integer(n)) => Ok(Value::Integer(!n)),
            _ => Err(anyhow::anyhow!("Несумісний тип для унарної операції {:?}: {}", op, val.type_name())),
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
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() &&
                    a.iter().zip(b.iter()).all(|(x, y)| self.values_equal(x, y))
            }
            (Value::Tuple(a), Value::Tuple(b)) => {
                a.len() == b.len() &&
                    a.iter().zip(b.iter()).all(|(x, y)| self.values_equal(x, y))
            }
            (Value::Dict(a), Value::Dict(b)) => {
                a.len() == b.len() &&
                    a.iter().all(|(k1, v1)| b.iter().any(|(k2, v2)| self.values_equal(k1, k2) && self.values_equal(v1, v2)))
            }
            (Value::Set(a), Value::Set(b)) => {
                a.len() == b.len() &&
                    a.iter().all(|x| b.iter().any(|y| self.values_equal(x, y)))
            }
            (Value::EnumVariant { variant: v1, fields: f1, .. },
             Value::EnumVariant { variant: v2, fields: f2, .. }) => {
                v1 == v2 && f1.len() == f2.len() &&
                    f1.iter().zip(f2.iter()).all(|(a, b)| self.values_equal(a, b))
            }
            _ => false,
        }
    }

    // ── JSON конвертація ──

    fn json_to_value(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| VM::json_to_value(v)).collect())
            }
            serde_json::Value::Object(map) => {
                let pairs: Vec<(Value, Value)> = map.iter()
                    .map(|(k, v)| (Value::String(k.clone()), VM::json_to_value(v)))
                    .collect();
                Value::Dict(pairs)
            }
        }
    }

    fn value_to_json(val: &Value) -> serde_json::Value {
        match val {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Integer(n) => serde_json::json!(*n),
            Value::Float(f) => serde_json::json!(*f),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| VM::value_to_json(v)).collect())
            }
            Value::Dict(pairs) => {
                let mut map = serde_json::Map::new();
                for (k, v) in pairs {
                    let key = k.to_display_string();
                    map.insert(key, VM::value_to_json(v));
                }
                serde_json::Value::Object(map)
            }
            Value::Struct(_, fields) => {
                let mut map = serde_json::Map::new();
                for (k, v) in fields {
                    map.insert(k.clone(), VM::value_to_json(v));
                }
                serde_json::Value::Object(map)
            }
            _ => serde_json::Value::String(val.to_display_string()),
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
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert!(execute(program, vec![]).is_ok());
    }

    #[test]
    fn test_match_expression() {
        let source = r#"
функція головна() {
    змінна х = 2
    зіставити х {
        1 => друк("один"),
        2 => друк("два"),
        _ => друк("інше")
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert!(execute(program, vec![]).is_ok());
    }

    #[test]
    fn test_enum_and_match() {
        let source = r#"
тип Колір {
    Червоний,
    Зелений,
    Синій
}

функція головна() {
    змінна к = Червоний
    зіставити к {
        Червоний => друк("червоний"),
        Зелений => друк("зелений"),
        Синій => друк("синій")
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert!(execute(program, vec![]).is_ok());
    }

    #[test]
    fn test_option_type() {
        let source = r#"
функція головна() {
    змінна значення = Деякий(42)
    зіставити значення {
        Деякий(н) => друк(н),
        Нічого => друк("пусто")
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert!(execute(program, vec![]).is_ok());
    }

    #[test]
    fn test_for_in_range() {
        let source = r#"
функція головна() {
    для (і в 1..5) {
        друк(і)
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert!(execute(program, vec![]).is_ok());
    }
}
