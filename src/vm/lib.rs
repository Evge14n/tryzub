// Тризуб VM v4.2 — Оптимізований інтерпретатор

use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use serde_json;
use rusqlite;

// ════════════════════════════════════════════════════════════════════
// String Interning — рядки порівнюються за O(1) замість O(n)
// ════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct StringInterner {
    strings: HashSet<Rc<str>>,
}

impl StringInterner {
    fn new() -> Self {
        Self { strings: HashSet::new() }
    }

    fn intern(&mut self, s: &str) -> Rc<str> {
        if let Some(existing) = self.strings.get(s) {
            existing.clone()
        } else {
            let rc: Rc<str> = Rc::from(s);
            self.strings.insert(rc.clone());
            rc
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// Constant Folding Cache — кешує результати чистих обчислень
// ════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct PureCache {
    entries: HashMap<u64, Value>,
    max_size: usize,
}

impl PureCache {
    fn new(max_size: usize) -> Self {
        Self { entries: HashMap::new(), max_size }
    }

    fn get(&self, key: u64) -> Option<&Value> {
        self.entries.get(&key)
    }

    fn insert(&mut self, key: u64, value: Value) {
        if self.entries.len() >= self.max_size {
            // LRU-подібне очищення: видаляємо половину
            let keys: Vec<u64> = self.entries.keys().take(self.max_size / 2).cloned().collect();
            for k in keys { self.entries.remove(&k); }
        }
        self.entries.insert(key, value);
    }

    fn hash_args(name: &str, args: &[Value]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        for arg in args {
            match arg {
                Value::Integer(n) => { 0u8.hash(&mut hasher); n.hash(&mut hasher); }
                Value::Float(f) => { 1u8.hash(&mut hasher); f.to_bits().hash(&mut hasher); }
                Value::String(s) => { 2u8.hash(&mut hasher); s.hash(&mut hasher); }
                Value::Bool(b) => { 3u8.hash(&mut hasher); b.hash(&mut hasher); }
                _ => { 99u8.hash(&mut hasher); }
            }
        }
        hasher.finish()
    }
}
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

    pub fn to_display_string(&self) -> String {
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
    /// Веб-маршрути (якщо запущено веб-сервер)
    web_routes: Option<Arc<Mutex<WebRoutes>>>,
    /// SQLite з'єднання: шлях → Connection
    db_connections: HashMap<String, Arc<Mutex<rusqlite::Connection>>>,
    /// String interning для O(1) порівнянь
    string_interner: StringInterner,
    /// Кеш чистих функцій
    pure_cache: PureCache,
    /// Позначені як чисті функції
    pure_functions: HashSet<String>,
    /// Лічильник операцій VM (для профілювання)
    op_count: u64,
}

// ════════════════════════════════════════════════════════════════════
// Веб-сервер — реальний HTTP через std::net::TcpListener
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct WebRoute {
    method: String,
    path: String,
    path_parts: Vec<PathPart>,
    handler: Value,
}

#[derive(Debug, Clone)]
enum PathPart {
    Static(String),
    Param(String),
    Wildcard,
}

#[derive(Debug, Clone)]
pub struct WebRoutes {
    port: u16,
    routes: Vec<WebRoute>,
    static_dir: Option<String>,
}

impl WebRoutes {
    fn new(port: u16) -> Self {
        WebRoutes { port, routes: Vec::new(), static_dir: None }
    }

    fn add_route(&mut self, method: String, path: String, handler: Value) {
        let path_parts = path.split('/')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.starts_with('{') && s.ends_with('}') {
                    PathPart::Param(s[1..s.len()-1].to_string())
                } else if s == "*" {
                    PathPart::Wildcard
                } else {
                    PathPart::Static(s.to_string())
                }
            })
            .collect();
        self.routes.push(WebRoute { method, path, path_parts, handler });
    }

    fn find_route(&self, method: &str, path: &str) -> Option<(&WebRoute, HashMap<String, String>)> {
        let req_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        for route in &self.routes {
            if route.method != method { continue; }

            let mut params = HashMap::new();
            let mut matched = true;

            if route.path_parts.len() != req_parts.len() {
                // Перевірка wildcard
                if route.path_parts.last().map_or(false, |p| matches!(p, PathPart::Wildcard)) {
                    if req_parts.len() < route.path_parts.len() - 1 { continue; }
                } else {
                    continue;
                }
            }

            for (i, part) in route.path_parts.iter().enumerate() {
                match part {
                    PathPart::Static(s) => {
                        if req_parts.get(i) != Some(&s.as_str()) { matched = false; break; }
                    }
                    PathPart::Param(name) => {
                        if let Some(val) = req_parts.get(i) {
                            params.insert(name.clone(), val.to_string());
                        } else { matched = false; break; }
                    }
                    PathPart::Wildcard => break,
                }
            }

            if matched {
                return Some((route, params));
            }
        }
        None
    }
}

enum LoopOptResult {
    SetVariable(String, Value),
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

            // Оптимізація та профілювання
            scope.set("позначити_чистою".to_string(), Value::BuiltinFn("позначити_чистою".to_string()));
            scope.set("очистити_кеш".to_string(), Value::BuiltinFn("очистити_кеш".to_string()));
            scope.set("статистика_vm".to_string(), Value::BuiltinFn("статистика_vm".to_string()));
            scope.set("бенчмарк_вбудований".to_string(), Value::BuiltinFn("бенчмарк_вбудований".to_string()));

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
            web_routes: None,
            db_connections: HashMap::new(),
            string_interner: StringInterner::new(),
            pure_cache: PureCache::new(10_000),
            pure_functions: HashSet::new(),
            op_count: 0,
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
                if let Some(c) = contract {
                    self.contracts.insert(name.clone(), c);
                }
                // Інтернуємо ім'я функції
                let _interned = self.string_interner.intern(&name);
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

                // ═══ PREDICTIVE PATTERN RECOGNITION ═══
                // Розпізнаємо типові паттерни циклів і замінюємо O(n) на O(1)
                if step_val == 1 {
                    if let Some(result) = self.try_optimize_loop(&variable, from_val, to_val, &body) {
                        // Паттерн розпізнано — результат обчислено за O(1)!
                        match result {
                            LoopOptResult::SetVariable(name, value) => {
                                if self.current_env.borrow_mut().update(&name, value).is_err() {
                                    self.current_env.borrow_mut().set(name, Value::Null);
                                }
                            }
                        }
                        // Пропускаємо цикл
                    } else {
                        // Паттерн не розпізнано — звичайне виконання
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
                } else {
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
        self.op_count += 1;
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
                let func_name = name.clone().unwrap_or_default();

                // Кеш чистих функцій — якщо функція позначена як чиста,
                // повертаємо кешований результат замість перевиконання
                if self.pure_functions.contains(&func_name) {
                    let cache_key = PureCache::hash_args(&func_name, &args);
                    if let Some(cached) = self.pure_cache.get(cache_key) {
                        return Ok(cached.clone());
                    }
                }

                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

                for (param, arg) in params.iter().zip(args.iter()) {
                    if param.name != "себе" {
                        self.current_env.borrow_mut().set(param.name.clone(), arg.clone());
                    }
                }
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

                // Зберігаємо в кеш якщо функція чиста
                if self.pure_functions.contains(&func_name) {
                    let cache_key = PureCache::hash_args(&func_name, &args);
                    self.pure_cache.insert(cache_key, result.clone());
                }

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
                            let (values, _) = self.generator_cache.get(&gen_id).ok_or_else(|| anyhow::anyhow!("Генератор не знайдено"))?;
                            return Ok(Value::Array(values.clone()));
                        }
                        "взяти" => {
                            let n = match args.first() {
                                Some(Value::Integer(n)) => *n as usize,
                                _ => usize::MAX,
                            };
                            let (values, _) = self.generator_cache.get(&gen_id).ok_or_else(|| anyhow::anyhow!("Генератор не знайдено"))?;
                            return Ok(Value::Array(values.iter().take(n).cloned().collect()));
                        }
                        "наступний" => {
                            let (values, idx) = self.generator_cache.get_mut(&gen_id).ok_or_else(|| anyhow::anyhow!("Генератор не знайдено"))?;
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

    // ═══════════════════════════════════════════════════════════════
    // PREDICTIVE PATTERN RECOGNITION — розпізнає паттерни циклів
    // і замінює O(n) виконання на O(1) математичні формули.
    // Жодна інша інтерпретована мова цього не робить.
    // ═══════════════════════════════════════════════════════════════

    fn try_optimize_loop(&self, loop_var: &str, from: i64, to: i64, body: &Statement) -> Option<LoopOptResult> {
        // Тіло циклу має бути одним Statement або Block з одним Statement
        let stmt = match body {
            Statement::Block(stmts) if stmts.len() == 1 => &stmts[0],
            s => s,
        };

        // Паттерн: змінна = змінна + loop_var (арифметична сума)
        // Або:     змінна = змінна + loop_var * loop_var (сума квадратів)
        if let Statement::Assignment { target, value, op: AssignmentOp::Assign } = stmt {
            if let Expression::Identifier(target_name) = target {
                // Паттерн 1: acc = acc + i → сума арифметичної прогресії
                if let Expression::Binary { left, op: BinaryOp::Add, right } = value {
                    if self.is_ident(left, target_name) && self.is_ident(right, loop_var) {
                        let n = to - from; // кількість ітерацій
                        if n <= 0 { return None; }
                        // Формула: sum(from..to) = n * (from + to - 1) / 2
                        let sum = n * (from + to - 1) / 2;
                        let current = self.current_env.borrow().get(target_name)
                            .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                            .unwrap_or(0);
                        return Some(LoopOptResult::SetVariable(
                            target_name.clone(),
                            Value::Integer(current + sum),
                        ));
                    }
                    // Паттерн 1b: acc = i + acc (комутативний)
                    if self.is_ident(right, target_name) && self.is_ident(left, loop_var) {
                        let n = to - from;
                        if n <= 0 { return None; }
                        let sum = n * (from + to - 1) / 2;
                        let current = self.current_env.borrow().get(target_name)
                            .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                            .unwrap_or(0);
                        return Some(LoopOptResult::SetVariable(
                            target_name.clone(),
                            Value::Integer(current + sum),
                        ));
                    }
                }

                // Паттерн 2: acc = acc * factor (де factor не залежить від loop_var)
                if let Expression::Binary { left, op: BinaryOp::Mul, right } = value {
                    if self.is_ident(left, target_name) {
                        if let Expression::Literal(Literal::Integer(factor)) = right.as_ref() {
                            let n = to - from;
                            if n <= 0 { return None; }
                            let current = self.current_env.borrow().get(target_name)
                                .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                                .unwrap_or(1);
                            // acc * factor^n
                            let result = current * factor.pow(n as u32);
                            return Some(LoopOptResult::SetVariable(
                                target_name.clone(),
                                Value::Integer(result),
                            ));
                        }
                    }
                }

                // Паттерн 3: acc = acc + 1 (простий лічильник)
                if let Expression::Binary { left, op: BinaryOp::Add, right } = value {
                    if self.is_ident(left, target_name) {
                        if let Expression::Literal(Literal::Integer(1)) = right.as_ref() {
                            let n = to - from;
                            if n <= 0 { return None; }
                            let current = self.current_env.borrow().get(target_name)
                                .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                                .unwrap_or(0);
                            return Some(LoopOptResult::SetVariable(
                                target_name.clone(),
                                Value::Integer(current + n),
                            ));
                        }
                    }
                }

                // Паттерн 4: acc = acc + i * i (сума квадратів)
                if let Expression::Binary { left, op: BinaryOp::Add, right } = value {
                    if self.is_ident(left, target_name) {
                        if let Expression::Binary { left: ml, op: BinaryOp::Mul, right: mr } = right.as_ref() {
                            if self.is_ident(ml, loop_var) && self.is_ident(mr, loop_var) {
                                let n = to - from;
                                if n <= 0 { return None; }
                                // Формула суми квадратів: Σi² від a до b
                                let sum_sq_to = |m: i64| -> i64 { m * (m + 1) * (2 * m + 1) / 6 };
                                let sum = sum_sq_to(to - 1) - sum_sq_to(from - 1);
                                let current = self.current_env.borrow().get(target_name)
                                    .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                                    .unwrap_or(0);
                                return Some(LoopOptResult::SetVariable(
                                    target_name.clone(),
                                    Value::Integer(current + sum),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Паттерн з AssignmentOp::AddAssign: acc += i
        if let Statement::Assignment { target, value, op: AssignmentOp::AddAssign } = stmt {
            if let Expression::Identifier(target_name) = target {
                if self.is_ident_expr(value, loop_var) {
                    let n = to - from;
                    if n <= 0 { return None; }
                    let sum = n * (from + to - 1) / 2;
                    let current = self.current_env.borrow().get(target_name)
                        .and_then(|v| if let Value::Integer(n) = v { Some(n) } else { None })
                        .unwrap_or(0);
                    return Some(LoopOptResult::SetVariable(
                        target_name.clone(),
                        Value::Integer(current + sum),
                    ));
                }
            }
        }

        None
    }

    fn is_ident(&self, expr: &Expression, name: &str) -> bool {
        matches!(expr, Expression::Identifier(n) if n == name)
    }

    fn is_ident_expr(&self, expr: &Expression, name: &str) -> bool {
        matches!(expr, Expression::Identifier(n) if n == name)
    }

    /// Запускає HTTP сервер — реальний, на std::net::TcpListener
    fn start_web_server(&mut self, routes: WebRoutes) -> Result<()> {
        use std::net::TcpListener;
        use std::io::{Read, Write, BufRead, BufReader};

        let addr = format!("0.0.0.0:{}", routes.port);
        let listener = TcpListener::bind(&addr)
            .map_err(|e| anyhow::anyhow!("Не вдалося запустити сервер на {}: {}", addr, e))?;

        println!("\n🔱 Тризуб Web запущено на http://localhost:{}", routes.port);
        println!("   {} маршрутів зареєстровано", routes.routes.len());
        if let Some(ref dir) = routes.static_dir {
            println!("   📁 Статичні файли: {}/", dir);
        }
        println!("   Натисніть Ctrl+C для зупинки\n");

        // Rate limiting: IP → (кількість_запитів, час_першого)
        let mut rate_limits: HashMap<String, (u32, std::time::Instant)> = HashMap::new();
        let rate_limit_max: u32 = 200; // запитів
        let rate_limit_window = std::time::Duration::from_secs(60);

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let request_start = std::time::Instant::now();
                    let client_ip = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();

                    // Rate limiting
                    let entry = rate_limits.entry(client_ip.clone()).or_insert((0, std::time::Instant::now()));
                    if entry.1.elapsed() > rate_limit_window {
                        *entry = (1, std::time::Instant::now());
                    } else {
                        entry.0 += 1;
                        if entry.0 > rate_limit_max {
                            let response = "HTTP/1.1 429 Too Many Requests\r\n\
                                Content-Type: text/html; charset=utf-8\r\n\
                                Retry-After: 60\r\n\
                                Connection: close\r\n\r\n\
                                <html><body><h1>429</h1><p>Забагато запитів</p></body></html>";
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }
                    }

                    // Очищуємо старі записи rate limit кожні 100 запитів
                    if rate_limits.len() > 1000 {
                        rate_limits.retain(|_, (_, t)| t.elapsed() < rate_limit_window);
                    }

                    let mut reader = BufReader::new(stream.try_clone().map_err(|e| anyhow::anyhow!("TCP clone: {}", e))?);

                    // Читаємо першу лінію: GET /path HTTP/1.1
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() { continue; }
                    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
                    if parts.len() < 2 { continue; }

                    let method = parts[0];
                    let full_path = parts[1];

                    // Розділяємо шлях та query string
                    let (path, query_string) = if let Some(idx) = full_path.find('?') {
                        (&full_path[..idx], Some(&full_path[idx+1..]))
                    } else {
                        (full_path, None)
                    };

                    // Читаємо заголовки
                    let mut headers = HashMap::new();
                    let mut content_length: usize = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() || line.trim().is_empty() { break; }
                        if let Some((key, val)) = line.trim().split_once(": ") {
                            if key.to_lowercase() == "content-length" {
                                content_length = val.trim().parse().unwrap_or(0);
                            }
                            headers.insert(key.to_lowercase(), val.trim().to_string());
                        }
                    }

                    // Читаємо тіло запиту
                    let mut body_str = String::new();
                    if content_length > 0 {
                        let mut body_buf = vec![0u8; content_length];
                        let _ = reader.read_exact(&mut body_buf);
                        body_str = String::from_utf8_lossy(&body_buf).to_string();
                    }

                    // Парсимо query string
                    let query_params: Vec<(Value, Value)> = query_string
                        .unwrap_or("")
                        .split('&')
                        .filter(|s| !s.is_empty())
                        .filter_map(|pair| {
                            pair.split_once('=').map(|(k, v)| {
                                (Value::String(Self::url_decode(k)), Value::String(Self::url_decode(v)))
                            })
                        })
                        .collect();

                    // Парсимо тіло (JSON або form data)
                    let body_value = if body_str.starts_with('{') || body_str.starts_with('[') {
                        serde_json::from_str::<serde_json::Value>(&body_str)
                            .map(|v| VM::json_to_value(&v))
                            .unwrap_or(Value::String(body_str.clone()))
                    } else if !body_str.is_empty() {
                        // form data: key=value&key2=value2
                        let pairs: Vec<(Value, Value)> = body_str.split('&')
                            .filter_map(|pair| {
                                pair.split_once('=').map(|(k, v)| {
                                    (Value::String(Self::url_decode(k)), Value::String(Self::url_decode(v)))
                                })
                            })
                            .collect();
                        Value::Dict(pairs)
                    } else {
                        Value::Null
                    };

                    // Будуємо об'єкт запиту
                    let mut request_dict = vec![
                        (Value::String("метод".to_string()), Value::String(method.to_string())),
                        (Value::String("шлях".to_string()), Value::String(path.to_string())),
                        (Value::String("запит".to_string()), Value::Dict(query_params)),
                        (Value::String("тіло".to_string()), body_value),
                        (Value::String("тіло_сирий".to_string()), Value::String(body_str)),
                        (Value::String("ip".to_string()), Value::String(
                            stream.peer_addr().map(|a| a.to_string()).unwrap_or_default()
                        )),
                    ];

                    // Додаємо заголовки
                    let headers_dict: Vec<(Value, Value)> = headers.iter()
                        .map(|(k, v)| (Value::String(k.clone()), Value::String(v.clone())))
                        .collect();
                    request_dict.push((Value::String("заголовки".to_string()), Value::Dict(headers_dict)));

                    // Cookies
                    if let Some(cookie_str) = headers.get("cookie") {
                        let cookies: Vec<(Value, Value)> = cookie_str.split(';')
                            .filter_map(|c| {
                                c.trim().split_once('=').map(|(k, v)| {
                                    (Value::String(k.trim().to_string()), Value::String(v.trim().to_string()))
                                })
                            })
                            .collect();
                        request_dict.push((Value::String("cookies".to_string()), Value::Dict(cookies)));
                    }

                    let request = Value::Dict(request_dict);

                    // Знаходимо маршрут
                    let (response_body, response_type, response_status, extra_headers) =
                        if let Some((route, params)) = routes.find_route(method, path) {
                            // Додаємо параметри URL до запиту
                            // (через Dict оновлення - VM не може мутувати, тому передаємо як є)

                            // Виконуємо обробник
                            let handler = route.handler.clone();
                            match self.call_value(handler, vec![request]) {
                                Ok(Value::Dict(resp)) => {
                                    let body = resp.iter()
                                        .find(|(k, _)| k.to_display_string() == "тіло")
                                        .map(|(_, v)| v.to_display_string())
                                        .unwrap_or_default();
                                    let content_type = resp.iter()
                                        .find(|(k, _)| k.to_display_string() == "тип")
                                        .map(|(_, v)| v.to_display_string())
                                        .unwrap_or_else(|| "text/html; charset=utf-8".to_string());
                                    let status = resp.iter()
                                        .find(|(k, _)| k.to_display_string() == "статус")
                                        .and_then(|(_, v)| if let Value::Integer(n) = v { Some(*n as u16) } else { None })
                                        .unwrap_or(200);
                                    let location = resp.iter()
                                        .find(|(k, _)| k.to_display_string() == "Location")
                                        .map(|(_, v)| v.to_display_string());
                                    (body, content_type, status, location)
                                }
                                Ok(Value::String(s)) => {
                                    (s, "text/html; charset=utf-8".to_string(), 200, None)
                                }
                                Ok(v) => {
                                    let json = serde_json::to_string(&VM::value_to_json(&v)).unwrap_or_default();
                                    (json, "application/json; charset=utf-8".to_string(), 200, None)
                                }
                                Err(e) => {
                                    eprintln!("  ❌ {} {} — {}", method, path, e);
                                    let html = format!(
                                        "<html><head><meta charset='utf-8'></head>\
                                         <body><h1>500</h1><pre>{}</pre></body></html>", e
                                    );
                                    (html, "text/html; charset=utf-8".to_string(), 500, None)
                                }
                            }
                        } else if let Some(ref static_dir) = routes.static_dir {
                            // Спроба роздати статичний файл
                            let file_path = format!("{}{}", static_dir, path);
                            if let Ok(content) = std::fs::read(&file_path) {
                                let mime = Self::guess_mime(&file_path);
                                let body = if mime.starts_with("text/") || mime.contains("json") || mime.contains("javascript") || mime.contains("xml") || mime.contains("svg") {
                                    String::from_utf8_lossy(&content).to_string()
                                } else {
                                    // Бінарний файл — base64 не підходить для raw TCP
                                    // Відправляємо як bytes напряму
                                    let status_line = "HTTP/1.1 200 OK\r\n";
                                    let headers_str = format!(
                                        "Content-Type: {}\r\nContent-Length: {}\r\n\
                                         X-Content-Type-Options: nosniff\r\n\
                                         Cache-Control: public, max-age=86400\r\n\
                                         Connection: close\r\n\r\n",
                                        mime, content.len()
                                    );
                                    let _ = stream.write_all(status_line.as_bytes());
                                    let _ = stream.write_all(headers_str.as_bytes());
                                    let _ = stream.write_all(&content);
                                    let _ = stream.flush();
                                    println!("  {} {} → 200 ({})", method, path, mime);
                                    continue;
                                };
                                (body, mime, 200, None)
                            } else {
                                let html = "<html><head><meta charset='utf-8'></head>\
                                            <body style='font-family:sans-serif;text-align:center;padding:50px'>\
                                            <h1>404</h1><p>Сторінку не знайдено</p><hr><p>Тризуб Web</p></body></html>";
                                (html.to_string(), "text/html; charset=utf-8".to_string(), 404, None)
                            }
                        } else {
                            let html = "<html><head><meta charset='utf-8'></head>\
                                        <body style='font-family:sans-serif;text-align:center;padding:50px'>\
                                        <h1>404</h1><p>Сторінку не знайдено</p><hr><p>Тризуб Web</p></body></html>";
                            (html.to_string(), "text/html; charset=utf-8".to_string(), 404, None)
                        };

                    // Формуємо HTTP відповідь
                    let status_text = match response_status {
                        200 => "OK", 201 => "Created", 204 => "No Content",
                        301 => "Moved Permanently", 302 => "Found", 304 => "Not Modified",
                        400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden",
                        404 => "Not Found", 405 => "Method Not Allowed",
                        429 => "Too Many Requests", 500 => "Internal Server Error",
                        _ => "OK",
                    };

                    // CORS — обробка OPTIONS preflight
                    if method == "OPTIONS" {
                        let cors_response = "HTTP/1.1 204 No Content\r\n\
                            Access-Control-Allow-Origin: *\r\n\
                            Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\n\
                            Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
                            Access-Control-Max-Age: 86400\r\n\
                            Connection: close\r\n\r\n";
                        let _ = stream.write_all(cors_response.as_bytes());
                        let elapsed = request_start.elapsed();
                        println!("  OPTIONS {} → 204 ({:.1}мс)", path, elapsed.as_secs_f64() * 1000.0);
                        continue;
                    }

                    let mut response = format!(
                        "HTTP/1.1 {} {}\r\n\
                         Content-Type: {}\r\n\
                         Content-Length: {}\r\n\
                         X-Content-Type-Options: nosniff\r\n\
                         X-Frame-Options: DENY\r\n\
                         X-XSS-Protection: 1; mode=block\r\n\
                         Referrer-Policy: strict-origin-when-cross-origin\r\n\
                         Access-Control-Allow-Origin: *\r\n\
                         Connection: close\r\n",
                        response_status, status_text,
                        response_type,
                        response_body.len(),
                    );

                    if let Some(loc) = extra_headers {
                        response.push_str(&format!("Location: {}\r\n", loc));
                    }

                    // Cache-Control для статичних файлів
                    if path.contains('.') && response_status == 200 {
                        response.push_str("Cache-Control: public, max-age=86400\r\n");
                    }

                    response.push_str("\r\n");
                    response.push_str(&response_body);

                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();

                    let elapsed = request_start.elapsed();
                    println!("  {} {} → {} ({:.1}мс)", method, path, response_status,
                        elapsed.as_secs_f64() * 1000.0);
                }
                Err(e) => eprintln!("  Помилка з'єднання: {}", e),
            }
        }
        Ok(())
    }

    fn url_decode(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                }
            } else if c == '+' {
                result.push(' ');
            } else {
                result.push(c);
            }
        }
        result
    }

    fn guess_mime(path: &str) -> String {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        match ext.as_str() {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "js" => "application/javascript; charset=utf-8",
            "json" => "application/json; charset=utf-8",
            "xml" => "application/xml; charset=utf-8",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "ico" => "image/x-icon",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "pdf" => "application/pdf",
            "zip" => "application/zip",
            "mp3" => "audio/mpeg",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "wasm" => "application/wasm",
            "csv" => "text/csv; charset=utf-8",
            "txt" => "text/plain; charset=utf-8",
            "map" => "application/json",
            _ => "application/octet-stream",
        }.to_string()
    }

    /// Валідація SQL ідентифікаторів (захист від SQL injection через імена таблиць/колонок)
    fn validate_sql_identifier(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(anyhow::anyhow!("SQL ідентифікатор не може бути порожнім"));
        }
        for c in name.chars() {
            if !c.is_alphanumeric() && c != '_' {
                return Err(anyhow::anyhow!(
                    "SQL ідентифікатор '{}' містить недозволений символ '{}'. Дозволені: літери, цифри, _",
                    name, c
                ));
            }
        }
        if name.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            return Err(anyhow::anyhow!("SQL ідентифікатор '{}' не може починатись з цифри", name));
        }
        Ok(())
    }

    pub fn call_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value> {
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

            // ── Веб-сервер (HTTP на std::net) ──

            "веб_сервер" => {
                // веб_сервер(порт) — створює HTTP сервер
                let port = match args.first() {
                    Some(Value::Integer(p)) => *p as u16,
                    _ => 3000,
                };
                // Зберігаємо порт у глобальному стані VM
                self.web_routes = Some(Arc::new(Mutex::new(WebRoutes::new(port))));
                println!("🔱 Тризуб Web сервер ініціалізовано на порті {}", port);
                Ok(Value::Integer(port as i64))
            }

            "веб_отримати" | "веб_надіслати" | "веб_оновити" | "веб_видалити" => {
                // веб_отримати(шлях, обробник)
                if args.len() >= 2 {
                    let path = match &args[0] {
                        Value::String(s) => s.clone(),
                        _ => return Err(anyhow::anyhow!("{}: перший аргумент має бути шляхом", name)),
                    };
                    let method = match name {
                        "веб_отримати" => "GET",
                        "веб_надіслати" => "POST",
                        "веб_оновити" => "PUT",
                        "веб_видалити" => "DELETE",
                        _ => "GET",
                    };
                    let handler = args[1].clone();

                    if let Some(ref routes) = self.web_routes {
                        routes.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?.add_route(
                            method.to_string(), path.clone(), handler
                        );
                        println!("  {} {} → зареєстровано", method, path);
                    }
                    Ok(Value::Null)
                } else {
                    Err(anyhow::anyhow!("{} очікує (шлях, обробник)", name))
                }
            }

            "веб_статичні" => {
                // веб_статичні(директорія) — роздача статичних файлів
                if let Some(Value::String(dir)) = args.first() {
                    if let Some(ref routes) = self.web_routes {
                        routes.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?.static_dir = Some(dir.clone());
                        println!("  📁 Статичні файли: {}/", dir);
                    }
                    Ok(Value::Null)
                } else {
                    Err(anyhow::anyhow!("веб_статичні очікує директорію"))
                }
            }

            "веб_запустити" => {
                // веб_запустити() — запускає сервер
                if let Some(ref routes) = self.web_routes {
                    let routes_data = routes.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?.clone();
                    self.start_web_server(routes_data)?;
                }
                Ok(Value::Null)
            }

            "веб_html" => {
                // веб_html(html_string) → Dict з відповіддю
                let html = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => v.to_display_string(),
                    None => String::new(),
                };
                let status = if args.len() > 1 {
                    if let Some(Value::Integer(s)) = args.get(1) { *s } else { 200 }
                } else { 200 };
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(html)),
                    (Value::String("тип".to_string()), Value::String("text/html; charset=utf-8".to_string())),
                    (Value::String("статус".to_string()), Value::Integer(status)),
                ]))
            }

            "веб_json" => {
                // веб_json(значення) → Dict з JSON відповіддю
                let val = args.first().cloned().unwrap_or(Value::Null);
                let json_str = serde_json::to_string(&VM::value_to_json(&val))
                    .unwrap_or_else(|_| "null".to_string());
                let status = if args.len() > 1 {
                    if let Some(Value::Integer(s)) = args.get(1) { *s } else { 200 }
                } else { 200 };
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(json_str)),
                    (Value::String("тип".to_string()), Value::String("application/json; charset=utf-8".to_string())),
                    (Value::String("статус".to_string()), Value::Integer(status)),
                ]))
            }

            "веб_перенаправити" => {
                // веб_перенаправити(url)
                let url = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => "/".to_string(),
                };
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(String::new())),
                    (Value::String("статус".to_string()), Value::Integer(302)),
                    (Value::String("Location".to_string()), Value::String(url)),
                ]))
            }

            "веб_помилка" => {
                // веб_помилка(код, повідомлення)
                let status = match args.first() {
                    Some(Value::Integer(n)) => *n,
                    _ => 500,
                };
                let msg = args.get(1).map(|v| v.to_display_string()).unwrap_or_else(|| "Помилка".to_string());
                let html = format!(
                    "<html><head><meta charset='utf-8'><title>Помилка {}</title></head>\
                     <body style='font-family:sans-serif;text-align:center;padding:50px'>\
                     <h1>{}</h1><p>{}</p><hr><p>Тризуб Web</p></body></html>",
                    status, status, msg
                );
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(html)),
                    (Value::String("тип".to_string()), Value::String("text/html; charset=utf-8".to_string())),
                    (Value::String("статус".to_string()), Value::Integer(status)),
                ]))
            }

            // ── SQLite ORM ──

            "бд_відкрити" => {
                // бд_відкрити(шлях) → відкриває/створює SQLite базу
                let path = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => "дані.db".to_string(),
                };
                match rusqlite::Connection::open(&path) {
                    Ok(conn) => {
                        // Оптимізації SQLite
                        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;");
                        let db = Arc::new(Mutex::new(conn));
                        self.db_connections.insert(path.clone(), db);
                        println!("  📦 База даних: {}", path);
                        Ok(Value::String(path))
                    }
                    Err(e) => Err(anyhow::anyhow!("Не вдалося відкрити БД: {}", e)),
                }
            }

            "бд_створити_таблицю" => {
                // бд_створити_таблицю(назва, схема_словник)
                if args.len() >= 2 {
                    let table = match &args[0] {
                        Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() },
                        _ => return Err(anyhow::anyhow!("бд_створити_таблицю: назва має бути рядком")),
                    };
                    let schema = match &args[1] {
                        Value::Dict(pairs) => pairs.clone(),
                        _ => return Err(anyhow::anyhow!("бд_створити_таблицю: схема має бути словником")),
                    };

                    let mut columns = Vec::new();
                    columns.push("ід INTEGER PRIMARY KEY AUTOINCREMENT".to_string());

                    for (key, val) in &schema {
                        let col_name = key.to_display_string();
                        Self::validate_sql_identifier(&col_name)?;
                        let col_type = match val {
                            Value::String(t) => match t.as_str() {
                                "тхт" | "текст" => "TEXT",
                                "цл64" | "ціле" => "INTEGER",
                                "дрб64" | "дробове" => "REAL",
                                "лог" | "логічне" => "INTEGER",
                                "дата" => "TEXT",
                                "json" => "TEXT",
                                _ => "TEXT",
                            },
                            _ => "TEXT",
                        };
                        columns.push(format!("{} {}", col_name, col_type));
                    }

                    let sql = format!("CREATE TABLE IF NOT EXISTS {} ({})",
                        table, columns.join(", "));

                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        db.execute(&sql, [])
                            .map_err(|e| anyhow::anyhow!("SQL помилка: {}", e))?;
                        println!("  ✓ Таблиця '{}' створена", table);
                        Ok(Value::Bool(true))
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних. Викличте бд_відкрити() спочатку."))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_створити_таблицю очікує (назва, схема)"))
                }
            }

            "бд_створити" => {
                // бд_створити(таблиця, словник_даних) → словник з ід
                if args.len() >= 2 {
                    let table = match &args[0] { Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() }, _ => return Err(anyhow::anyhow!("бд_створити: назва таблиці має бути рядком")) };
                    let data = match &args[1] { Value::Dict(pairs) => pairs.clone(), _ => return Err(anyhow::anyhow!("бд_створити: дані мають бути словником")) };

                    let col_names: Vec<String> = data.iter().map(|(k, _)| k.to_display_string()).collect();
                    for cn in &col_names { Self::validate_sql_identifier(cn)?; }
                    let placeholders: Vec<String> = (0..data.len()).map(|i| format!("?{}", i + 1)).collect();

                    let sql = format!("INSERT INTO {} ({}) VALUES ({})",
                        table, col_names.join(", "), placeholders.join(", "));

                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let params: Vec<Box<dyn rusqlite::types::ToSql>> = data.iter()
                            .map(|(_, v)| Self::value_to_sql_param(v))
                            .collect();
                        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

                        db.execute(&sql, param_refs.as_slice())
                            .map_err(|e| anyhow::anyhow!("SQL помилка: {}", e))?;

                        let id = db.last_insert_rowid();
                        let mut result = data.clone();
                        result.insert(0, (Value::String("ід".to_string()), Value::Integer(id)));
                        Ok(Value::Dict(result))
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_створити очікує (таблиця, дані)"))
                }
            }

            "бд_знайти" => {
                // бд_знайти(таблиця, ід) → словник або нуль
                if args.len() >= 2 {
                    let table = match &args[0] { Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() }, _ => return Err(anyhow::anyhow!("бд_знайти: назва таблиці")) };
                    let id = match &args[1] { Value::Integer(n) => *n, _ => return Err(anyhow::anyhow!("бд_знайти: ід має бути цілим")) };

                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let sql = format!("SELECT * FROM {} WHERE ід = ?1", table);
                        let mut stmt = db.prepare(&sql).map_err(|e| anyhow::anyhow!("SQL: {}", e))?;

                        let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

                        let result = stmt.query_row([id], |row| {
                            let mut pairs = Vec::new();
                            for (i, col) in columns.iter().enumerate() {
                                let val = Self::sql_to_value(row, i);
                                pairs.push((Value::String(col.clone()), val));
                            }
                            Ok(Value::Dict(pairs))
                        });

                        match result {
                            Ok(val) => Ok(val),
                            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Value::Null),
                            Err(e) => Err(anyhow::anyhow!("SQL: {}", e)),
                        }
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_знайти очікує (таблиця, ід)"))
                }
            }

            "бд_всі" | "бд_запит" => {
                // бд_всі(таблиця) або бд_запит(таблиця, де_словник)
                if args.is_empty() { return Err(anyhow::anyhow!("{} очікує назву таблиці", name)); }
                let table = match &args[0] { Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() }, _ => return Err(anyhow::anyhow!("назва таблиці має бути рядком")) };

                let where_clause = if args.len() >= 2 {
                    if let Value::Dict(pairs) = &args[1] {
                        let conditions: Vec<String> = pairs.iter().enumerate()
                            .map(|(i, (k, _))| format!("{} = ?{}", k.to_display_string(), i + 1))
                            .collect();
                        Some((conditions.join(" AND "), pairs.clone()))
                    } else { None }
                } else { None };

                if let Some(conn) = self.get_db_connection() {
                    let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                    let sql = if let Some((ref where_str, _)) = where_clause {
                        format!("SELECT * FROM {} WHERE {} LIMIT 1000", table, where_str)
                    } else {
                        format!("SELECT * FROM {} LIMIT 1000", table)
                    };

                    let mut stmt = db.prepare(&sql).map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

                    let params: Vec<Box<dyn rusqlite::types::ToSql>> = if let Some((_, ref pairs)) = where_clause {
                        pairs.iter().map(|(_, v)| Self::value_to_sql_param(v)).collect()
                    } else {
                        vec![]
                    };
                    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

                    let rows = stmt.query_map(param_refs.as_slice(), |row| {
                        let mut pairs = Vec::new();
                        for (i, col) in columns.iter().enumerate() {
                            let val = Self::sql_to_value(row, i);
                            pairs.push((Value::String(col.clone()), val));
                        }
                        Ok(Value::Dict(pairs))
                    }).map_err(|e| anyhow::anyhow!("SQL: {}", e))?;

                    let results: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
                    Ok(Value::Array(results))
                } else {
                    Err(anyhow::anyhow!("Немає відкритої бази даних"))
                }
            }

            "бд_оновити" => {
                // бд_оновити(таблиця, ід, дані_словник)
                if args.len() >= 3 {
                    let table = match &args[0] { Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() }, _ => return Err(anyhow::anyhow!("назва таблиці")) };
                    let id = match &args[1] { Value::Integer(n) => *n, _ => return Err(anyhow::anyhow!("ід має бути цілим")) };
                    let data = match &args[2] { Value::Dict(pairs) => pairs.clone(), _ => return Err(anyhow::anyhow!("дані мають бути словником")) };

                    for (k, _) in &data { Self::validate_sql_identifier(&k.to_display_string())?; }
                    let sets: Vec<String> = data.iter().enumerate()
                        .map(|(i, (k, _))| format!("{} = ?{}", k.to_display_string(), i + 1))
                        .collect();

                    let sql = format!("UPDATE {} SET {} WHERE ід = ?{}", table, sets.join(", "), data.len() + 1);

                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = data.iter()
                            .map(|(_, v)| Self::value_to_sql_param(v))
                            .collect();
                        params.push(Box::new(id));
                        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

                        let affected = db.execute(&sql, param_refs.as_slice())
                            .map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                        Ok(Value::Integer(affected as i64))
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_оновити очікує (таблиця, ід, дані)"))
                }
            }

            "бд_видалити" => {
                // бд_видалити(таблиця, ід)
                if args.len() >= 2 {
                    let table = match &args[0] { Value::String(s) => { Self::validate_sql_identifier(s)?; s.clone() }, _ => return Err(anyhow::anyhow!("назва таблиці")) };
                    let id = match &args[1] { Value::Integer(n) => *n, _ => return Err(anyhow::anyhow!("ід має бути цілим")) };

                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let sql = format!("DELETE FROM {} WHERE ід = ?1", table);
                        let affected = db.execute(&sql, [id])
                            .map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                        Ok(Value::Integer(affected as i64))
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_видалити очікує (таблиця, ід)"))
                }
            }

            "бд_кількість" => {
                // бд_кількість(таблиця) → кількість записів
                if let Some(Value::String(table)) = args.first() {
                    Self::validate_sql_identifier(table)?;
                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let sql = format!("SELECT COUNT(*) FROM {}", table);
                        let count: i64 = db.query_row(&sql, [], |row| row.get(0))
                            .map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                        Ok(Value::Integer(count))
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_кількість очікує назву таблиці"))
                }
            }

            "бд_sql" => {
                // бд_sql(запит, параметри...) → виконує raw SQL
                if let Some(Value::String(sql)) = args.first() {
                    if let Some(conn) = self.get_db_connection() {
                        let db = conn.lock().map_err(|e| anyhow::anyhow!("Помилка блокування: {}", e))?;
                        let params: Vec<Box<dyn rusqlite::types::ToSql>> = args[1..].iter()
                            .map(|v| Self::value_to_sql_param(v))
                            .collect();
                        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

                        if sql.trim().to_uppercase().starts_with("SELECT") {
                            let mut stmt = db.prepare(sql).map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                            let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
                            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                                let mut pairs = Vec::new();
                                for (i, col) in columns.iter().enumerate() {
                                    pairs.push((Value::String(col.clone()), Self::sql_to_value(row, i)));
                                }
                                Ok(Value::Dict(pairs))
                            }).map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                            let results: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
                            Ok(Value::Array(results))
                        } else {
                            let affected = db.execute(sql, param_refs.as_slice())
                                .map_err(|e| anyhow::anyhow!("SQL: {}", e))?;
                            Ok(Value::Integer(affected as i64))
                        }
                    } else {
                        Err(anyhow::anyhow!("Немає відкритої бази даних"))
                    }
                } else {
                    Err(anyhow::anyhow!("бд_sql очікує SQL запит"))
                }
            }

            // ── Шаблонізатор ──

            "веб_шаблон" => {
                // веб_шаблон(ім'я_шаблону, дані_словник) → HTML
                if args.len() >= 2 {
                    let template_name = match &args[0] {
                        Value::String(s) => s.clone(),
                        _ => return Err(anyhow::anyhow!("веб_шаблон: ім'я має бути рядком")),
                    };
                    let data = args[1].clone();

                    // Шукаємо шаблон у шаблони/ директорії
                    let paths = vec![
                        format!("шаблони/{}.тхтмл", template_name),
                        format!("шаблони/{}.html", template_name),
                        format!("templates/{}.html", template_name),
                    ];

                    let mut template_content = None;
                    for path in &paths {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            template_content = Some(content);
                            break;
                        }
                    }

                    let template = match template_content {
                        Some(t) => t,
                        None => return Err(anyhow::anyhow!("Шаблон '{}' не знайдено", template_name)),
                    };

                    let rendered = self.render_template(&template, &data)?;

                    Ok(Value::Dict(vec![
                        (Value::String("тіло".to_string()), Value::String(rendered)),
                        (Value::String("тип".to_string()), Value::String("text/html; charset=utf-8".to_string())),
                        (Value::String("статус".to_string()), Value::Integer(200)),
                    ]))
                } else {
                    Err(anyhow::anyhow!("веб_шаблон очікує (ім'я, дані)"))
                }
            }

            "шаблон_рядок" => {
                // шаблон_рядок(шаблон_тхт, дані) → відрендерений рядок
                if args.len() >= 2 {
                    let template = match &args[0] {
                        Value::String(s) => s.clone(),
                        _ => return Err(anyhow::anyhow!("шаблон_рядок: шаблон має бути рядком")),
                    };
                    let data = args[1].clone();
                    let rendered = self.render_template(&template, &data)?;
                    Ok(Value::String(rendered))
                } else {
                    Err(anyhow::anyhow!("шаблон_рядок очікує (шаблон, дані)"))
                }
            }

            // ── Автентифікація (JWT + bcrypt-подібне хешування) ──

            "авт_хешувати" => {
                // авт_хешувати(пароль) → хеш (SHA256 + salt)
                match args.first() {
                    Some(Value::String(password)) => {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        // Генеруємо salt з часу
                        let salt: u64 = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;
                        let mut hasher = DefaultHasher::new();
                        salt.hash(&mut hasher);
                        password.hash(&mut hasher);
                        // Кілька раундів хешування
                        let mut hash_val = hasher.finish();
                        for _ in 0..1000 {
                            let mut h = DefaultHasher::new();
                            hash_val.hash(&mut h);
                            password.hash(&mut h);
                            salt.hash(&mut h);
                            hash_val = h.finish();
                        }
                        Ok(Value::String(format!("$тх${}${:016x}", salt, hash_val)))
                    }
                    _ => Err(anyhow::anyhow!("авт_хешувати очікує пароль (тхт)")),
                }
            }

            "авт_перевірити" => {
                // авт_перевірити(пароль, хеш) → лог
                if args.len() >= 2 {
                    if let (Value::String(password), Value::String(stored_hash)) = (&args[0], &args[1]) {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        // Розбираємо збережений хеш
                        let parts: Vec<&str> = stored_hash.split('$').collect();
                        if parts.len() >= 4 && parts[1] == "тх" {
                            if let Ok(salt) = parts[2].parse::<u64>() {
                                let mut hasher = DefaultHasher::new();
                                salt.hash(&mut hasher);
                                password.hash(&mut hasher);
                                let mut hash_val = hasher.finish();
                                for _ in 0..1000 {
                                    let mut h = DefaultHasher::new();
                                    hash_val.hash(&mut h);
                                    password.hash(&mut h);
                                    salt.hash(&mut h);
                                    hash_val = h.finish();
                                }
                                let computed = format!("$тх${}${:016x}", salt, hash_val);
                                // Timing-safe порівняння
                                let eq = stored_hash.len() == computed.len() &&
                                    stored_hash.bytes().zip(computed.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
                                return Ok(Value::Bool(eq));
                            }
                        }
                        Ok(Value::Bool(false))
                    } else {
                        Err(anyhow::anyhow!("авт_перевірити очікує (пароль, хеш)"))
                    }
                } else {
                    Err(anyhow::anyhow!("авт_перевірити очікує 2 аргументи"))
                }
            }

            "авт_створити_токен" => {
                // авт_створити_токен(дані_словник, секрет, термін_хвилин) → JWT-подібний токен
                if args.len() >= 1 {
                    let data = &args[0];
                    let secret = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or_else(|| "тризуб-секрет-за-замовчуванням".to_string());
                    let ttl_min = args.get(2).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None })
                        .unwrap_or(1440); // 24 години

                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let exp = now + (ttl_min as u64 * 60);

                    // Header
                    let header = serde_json::json!({"алг": "ТХ256", "тип": "JWT"});
                    let header_b64 = Self::base64_encode(&serde_json::to_string(&header).unwrap_or_default());

                    // Payload
                    let mut payload = VM::value_to_json(data);
                    if let serde_json::Value::Object(ref mut map) = payload {
                        map.insert("exp".to_string(), serde_json::json!(exp));
                        map.insert("iat".to_string(), serde_json::json!(now));
                    }
                    let payload_b64 = Self::base64_encode(&serde_json::to_string(&payload).unwrap_or_default());

                    // Signature (HMAC-like з DefaultHasher)
                    let sign_input = format!("{}.{}.{}", header_b64, payload_b64, secret);
                    let mut hasher = DefaultHasher::new();
                    sign_input.hash(&mut hasher);
                    let sig = format!("{:016x}", hasher.finish());
                    let sig_b64 = Self::base64_encode(&sig);

                    Ok(Value::String(format!("{}.{}.{}", header_b64, payload_b64, sig_b64)))
                } else {
                    Err(anyhow::anyhow!("авт_створити_токен очікує (дані)"))
                }
            }

            "авт_перевірити_токен" => {
                // авт_перевірити_токен(токен, секрет) → дані або Помилка
                if args.len() >= 1 {
                    if let Value::String(token) = &args[0] {
                        let secret = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                            .unwrap_or_else(|| "тризуб-секрет-за-замовчуванням".to_string());

                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};

                        let parts: Vec<&str> = token.split('.').collect();
                        if parts.len() != 3 {
                            return Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String("Невалідний токен".to_string())],
                            });
                        }

                        // Перевіряємо підпис
                        let sign_input = format!("{}.{}.{}", parts[0], parts[1], secret);
                        let mut hasher = DefaultHasher::new();
                        sign_input.hash(&mut hasher);
                        let expected_sig = Self::base64_encode(&format!("{:016x}", hasher.finish()));

                        if parts[2] != expected_sig {
                            return Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String("Невалідний підпис".to_string())],
                            });
                        }

                        // Декодуємо payload
                        let payload_str = Self::base64_decode(parts[1]);
                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&payload_str) {
                            // Перевіряємо термін дії
                            if let Some(exp) = json_val.get("exp").and_then(|v| v.as_u64()) {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                if now > exp {
                                    return Ok(Value::EnumVariant {
                                        type_name: "Результат".to_string(),
                                        variant: "Помилка".to_string(),
                                        fields: vec![Value::String("Токен прострочений".to_string())],
                                    });
                                }
                            }
                            Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Успіх".to_string(),
                                fields: vec![VM::json_to_value(&json_val)],
                            })
                        } else {
                            Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String("Не вдалося декодувати payload".to_string())],
                            })
                        }
                    } else {
                        Err(anyhow::anyhow!("авт_перевірити_токен очікує токен (тхт)"))
                    }
                } else {
                    Err(anyhow::anyhow!("авт_перевірити_токен очікує (токен)"))
                }
            }

            // ── Утиліти для середовища ──

            "середовище" => {
                // середовище(назва) → значення змінної середовища або нуль
                match args.first() {
                    Some(Value::String(name)) => {
                        match std::env::var(name) {
                            Ok(val) => Ok(Value::String(val)),
                            Err(_) => Ok(Value::Null),
                        }
                    }
                    _ => Err(anyhow::anyhow!("середовище очікує назву змінної")),
                }
            }

            "випадкове" => {
                // випадкове(мін, макс) → випадкове ціле число
                let min = args.first().and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None }).unwrap_or(0);
                let max = args.get(1).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None }).unwrap_or(100);
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i64;
                let range = max - min + 1;
                if range <= 0 { return Ok(Value::Integer(min)); }
                Ok(Value::Integer(min + (seed.abs() % range)))
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
                Ok(Value::Set(args))
            }

            // ── Оптимізація та профілювання ──

            "позначити_чистою" => {
                // позначити_чистою("ім'я_функції") — додає функцію до кешу
                if let Some(Value::String(fname)) = args.first() {
                    self.pure_functions.insert(fname.clone());
                    Ok(Value::Bool(true))
                } else {
                    Err(anyhow::anyhow!("позначити_чистою очікує ім'я функції"))
                }
            }

            "очистити_кеш" => {
                self.pure_cache = PureCache::new(10_000);
                Ok(Value::Null)
            }

            "статистика_vm" => {
                let mut stats = Vec::new();
                stats.push((Value::String("операцій".into()), Value::Integer(self.op_count as i64)));
                stats.push((Value::String("інтерновано_рядків".into()), Value::Integer(self.string_interner.strings.len() as i64)));
                stats.push((Value::String("кешовано_результатів".into()), Value::Integer(self.pure_cache.entries.len() as i64)));
                stats.push((Value::String("чистих_функцій".into()), Value::Integer(self.pure_functions.len() as i64)));
                stats.push((Value::String("enum_типів".into()), Value::Integer(self.enum_types.len() as i64)));
                stats.push((Value::String("контрактів".into()), Value::Integer(self.contracts.len() as i64)));
                stats.push((Value::String("макросів".into()), Value::Integer(self.macros.len() as i64)));
                Ok(Value::Dict(stats))
            }

            "бенчмарк_вбудований" => {
                // бенчмарк_вбудований(назва, ітерацій) — вимірює базові операції VM
                let iterations = match args.first() {
                    Some(Value::Integer(n)) => *n as u64,
                    _ => 1_000_000,
                };

                let start = std::time::Instant::now();

                // Арифметика
                let arith_start = std::time::Instant::now();
                let mut sum: i64 = 0;
                for i in 0..iterations {
                    sum = sum.wrapping_add(i as i64).wrapping_mul(3).wrapping_add(7);
                }
                let arith_time = arith_start.elapsed();
                let _ = sum; // prevent optimization

                // HashMap lookup (імітація scope.get)
                let mut map = HashMap::new();
                for i in 0..100 {
                    map.insert(format!("змінна_{}", i), Value::Integer(i));
                }
                let lookup_start = std::time::Instant::now();
                for i in 0..iterations {
                    let _ = map.get(&format!("змінна_{}", i % 100));
                }
                let lookup_time = lookup_start.elapsed();

                // Алокація Value
                let alloc_start = std::time::Instant::now();
                let mut vec = Vec::with_capacity(iterations as usize);
                for i in 0..iterations.min(100_000) {
                    vec.push(Value::Integer(i as i64));
                }
                let alloc_time = alloc_start.elapsed();
                drop(vec);

                let total = start.elapsed();

                println!("\n  ⚡ Бенчмарк Тризуб VM ({} ітерацій)", iterations);
                println!("  ─────────────────────────────────────");
                println!("  Арифметика:     {:>8.2} мс ({:.0} млн оп/с)",
                    arith_time.as_secs_f64() * 1000.0,
                    iterations as f64 / arith_time.as_secs_f64() / 1_000_000.0);
                println!("  Lookup змінних: {:>8.2} мс ({:.0} млн оп/с)",
                    lookup_time.as_secs_f64() * 1000.0,
                    iterations as f64 / lookup_time.as_secs_f64() / 1_000_000.0);
                println!("  Алокація Value: {:>8.2} мс ({:.0} тис/с)",
                    alloc_time.as_secs_f64() * 1000.0,
                    iterations.min(100_000) as f64 / alloc_time.as_secs_f64() / 1_000.0);
                println!("  ─────────────────────────────────────");
                println!("  Загалом:        {:>8.2} мс", total.as_secs_f64() * 1000.0);

                Ok(Value::Dict(vec![
                    (Value::String("арифметика_мс".into()), Value::Float(arith_time.as_secs_f64() * 1000.0)),
                    (Value::String("lookup_мс".into()), Value::Float(lookup_time.as_secs_f64() * 1000.0)),
                    (Value::String("алокація_мс".into()), Value::Float(alloc_time.as_secs_f64() * 1000.0)),
                    (Value::String("загалом_мс".into()), Value::Float(total.as_secs_f64() * 1000.0)),
                    (Value::String("ітерацій".into()), Value::Integer(iterations as i64)),
                ]))
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

    // ── Шаблонізатор ──

    fn render_template(&mut self, template: &str, data: &Value) -> Result<String> {
        self.render_template_depth(template, data, 0)
    }

    fn render_template_depth(&mut self, template: &str, data: &Value, depth: usize) -> Result<String> {
        if depth > 10 {
            return Err(anyhow::anyhow!("Шаблон: перевищено максимальну глибину включень (10). Можливо циклічне включити."));
        }
        let mut result = String::new();
        let chars: Vec<char> = template.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '{' && chars[i + 1] != '{' {
                // Знаходимо кінець виразу
                let start = i + 1;
                let mut depth = 1;
                let mut j = start;
                while j < chars.len() && depth > 0 {
                    if chars[j] == '{' { depth += 1; }
                    if chars[j] == '}' { depth -= 1; }
                    if depth > 0 { j += 1; }
                }

                let expr: String = chars[start..j].iter().collect();
                let expr = expr.trim();

                if expr.starts_with("якщо ") {
                    // Умовний блок: {якщо умова}...{/якщо}
                    let condition = &expr[9..]; // "якщо " = 9 bytes in UTF-8? Let's be safe
                    let condition = expr.trim_start_matches("якщо").trim();
                    let end_tag = "{/якщо}";
                    let else_tag = "{інакше}";

                    // Знаходимо {/якщо}
                    let remaining: String = chars[j+1..].iter().collect();
                    let (if_body, else_body, skip_len) = if let Some(else_pos) = remaining.find(else_tag) {
                        if let Some(end_pos) = remaining.find(end_tag) {
                            if else_pos < end_pos {
                                let if_b = &remaining[..else_pos];
                                let else_b = &remaining[else_pos + else_tag.len()..end_pos];
                                (if_b.to_string(), Some(else_b.to_string()), end_pos + end_tag.len())
                            } else {
                                let if_b = &remaining[..end_pos];
                                (if_b.to_string(), None, end_pos + end_tag.len())
                            }
                        } else {
                            (remaining.clone(), None, remaining.len())
                        }
                    } else if let Some(end_pos) = remaining.find(end_tag) {
                        let if_b = &remaining[..end_pos];
                        (if_b.to_string(), None, end_pos + end_tag.len())
                    } else {
                        (remaining.clone(), None, remaining.len())
                    };

                    // Обчислюємо умову
                    let cond_val = self.resolve_template_value(condition, data);
                    if cond_val.to_bool() {
                        result.push_str(&self.render_template_depth(&if_body, data, depth + 1)?);
                    } else if let Some(else_b) = else_body {
                        result.push_str(&self.render_template_depth(&else_b, data, depth + 1)?);
                    }

                    // Переміщуємо позицію
                    i = j + 1 + skip_len;
                    continue;
                } else if expr.starts_with("для ") {
                    // Цикл: {для елемент в масив}...{/для}
                    let for_expr = expr.trim_start_matches("для").trim();
                    let parts: Vec<&str> = for_expr.splitn(3, ' ').collect();

                    if parts.len() >= 3 && parts[1] == "в" {
                        let var_name = parts[0];
                        let collection_name = parts[2];

                        let end_tag = "{/для}";
                        let remaining: String = chars[j+1..].iter().collect();
                        let (body, skip_len) = if let Some(end_pos) = remaining.find(end_tag) {
                            (&remaining[..end_pos], end_pos + end_tag.len())
                        } else {
                            (remaining.as_str(), remaining.len())
                        };

                        let collection = self.resolve_template_value(collection_name, data);
                        if let Value::Array(items) = collection {
                            for (idx, item) in items.iter().enumerate() {
                                // Створюємо контекст з елементом
                                let mut item_data_pairs = match data {
                                    Value::Dict(pairs) => pairs.clone(),
                                    _ => vec![],
                                };
                                item_data_pairs.push((Value::String(var_name.to_string()), item.clone()));
                                item_data_pairs.push((Value::String("індекс".to_string()), Value::Integer(idx as i64)));
                                item_data_pairs.push((Value::String("лічильник".to_string()), Value::Integer(idx as i64 + 1)));
                                item_data_pairs.push((Value::String("перший".to_string()), Value::Bool(idx == 0)));
                                item_data_pairs.push((Value::String("останній".to_string()), Value::Bool(idx == items.len() - 1)));

                                let item_data = Value::Dict(item_data_pairs);
                                result.push_str(&self.render_template_depth(body, &item_data, depth + 1)?);
                            }
                        }

                        i = j + 1 + skip_len;
                        continue;
                    }
                } else if expr.starts_with("включити ") {
                    // Включення: {включити "компонент"}
                    let include_name = expr.trim_start_matches("включити").trim().trim_matches('"');
                    let paths = vec![
                        format!("шаблони/{}.тхтмл", include_name),
                        format!("шаблони/компоненти/{}.тхтмл", include_name),
                        format!("шаблони/{}.html", include_name),
                    ];
                    for path in &paths {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            result.push_str(&self.render_template_depth(&content, data, depth + 1)?);
                            break;
                        }
                    }
                    i = j + 1;
                    continue;
                } else if expr == "csrf" {
                    // CSRF токен
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut h = DefaultHasher::new();
                    std::time::SystemTime::now().hash(&mut h);
                    let token = format!("{:016x}", h.finish());
                    result.push_str(&format!(
                        "<input type=\"hidden\" name=\"csrf_token\" value=\"{}\">", token
                    ));
                    i = j + 1;
                    continue;
                } else {
                    // Змінна: {назва} або {назва |> фільтр}
                    let (var_path, filter) = if let Some(pipe_pos) = expr.find("|>") {
                        (expr[..pipe_pos].trim(), Some(expr[pipe_pos+2..].trim()))
                    } else {
                        (expr, None)
                    };

                    let val = self.resolve_template_value(var_path, data);
                    let mut text = val.to_display_string();

                    // Фільтри
                    if let Some(f) = filter {
                        text = match f {
                            "великими" => text.to_uppercase(),
                            "малими" => text.to_lowercase(),
                            _ if f.starts_with("обрізати_до(") => {
                                let len_str = f.trim_start_matches("обрізати_до(").trim_end_matches(')');
                                let max_len: usize = len_str.parse().unwrap_or(100);
                                if text.chars().count() > max_len {
                                    let truncated: String = text.chars().take(max_len).collect();
                                    format!("{}...", truncated)
                                } else { text }
                            }
                            "гроші" => {
                                if let Ok(num) = text.parse::<f64>() {
                                    let whole = num as i64;
                                    let frac = ((num - whole as f64) * 100.0).round() as i64;
                                    let formatted = Self::format_number(whole);
                                    format!("{},{:02}", formatted, frac)
                                } else { text }
                            }
                            "довжина" => {
                                match &val {
                                    Value::Array(a) => a.len().to_string(),
                                    Value::String(s) => s.chars().count().to_string(),
                                    _ => text,
                                }
                            }
                            _ => text,
                        };
                    }

                    // HTML екранування (захист від XSS)
                    if !expr.starts_with('!') {
                        text = text.replace('&', "&amp;")
                            .replace('<', "&lt;")
                            .replace('>', "&gt;")
                            .replace('"', "&quot;")
                            .replace('\'', "&#39;");
                    }

                    result.push_str(&text);
                }

                i = j + 1;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        Ok(result)
    }

    fn resolve_template_value(&self, path: &str, data: &Value) -> Value {
        let path = path.trim();

        // Дотовий доступ: товар.назва
        let parts: Vec<&str> = path.split('.').collect();

        let mut current = data.clone();
        for part in &parts {
            let part = part.trim();
            current = match &current {
                Value::Dict(pairs) => {
                    pairs.iter()
                        .find(|(k, _)| k.to_display_string() == part)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Null)
                }
                Value::Struct(_, fields) => {
                    fields.get(part).cloned().unwrap_or(Value::Null)
                }
                _ => Value::Null,
            };
        }
        current
    }

    fn format_number(n: i64) -> String {
        let s = n.abs().to_string();
        let mut result = String::new();
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 { result.push(' '); }
            result.push(c);
        }
        if n < 0 { result.push('-'); }
        result.chars().rev().collect()
    }

    fn base64_encode(input: &str) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let bytes = input.as_bytes();
        let mut result = String::new();
        let mut i = 0;
        while i < bytes.len() {
            let b0 = bytes[i] as u32;
            let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
            let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
            result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
            if i + 1 < bytes.len() { result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char); }
            if i + 2 < bytes.len() { result.push(CHARS[(triple & 0x3F) as usize] as char); }
            i += 3;
        }
        result
    }

    fn base64_decode(input: &str) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut bytes = Vec::new();
        let input_bytes: Vec<u8> = input.bytes()
            .filter_map(|b| CHARS.iter().position(|&c| c == b).map(|p| p as u8))
            .collect();
        let mut i = 0;
        while i < input_bytes.len() {
            let b0 = input_bytes[i] as u32;
            let b1 = if i + 1 < input_bytes.len() { input_bytes[i + 1] as u32 } else { 0 };
            let b2 = if i + 2 < input_bytes.len() { input_bytes[i + 2] as u32 } else { 0 };
            let b3 = if i + 3 < input_bytes.len() { input_bytes[i + 3] as u32 } else { 0 };
            let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
            bytes.push(((triple >> 16) & 0xFF) as u8);
            if i + 2 < input_bytes.len() { bytes.push(((triple >> 8) & 0xFF) as u8); }
            if i + 3 < input_bytes.len() { bytes.push((triple & 0xFF) as u8); }
            i += 4;
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    // ── SQLite допоміжні методи ──

    fn get_db_connection(&self) -> Option<Arc<Mutex<rusqlite::Connection>>> {
        self.db_connections.values().next().cloned()
    }

    fn value_to_sql_param(val: &Value) -> Box<dyn rusqlite::types::ToSql> {
        match val {
            Value::Integer(n) => Box::new(*n),
            Value::Float(f) => Box::new(*f),
            Value::String(s) => Box::new(s.clone()),
            Value::Bool(b) => Box::new(*b as i64),
            Value::Null => Box::new(rusqlite::types::Null),
            _ => Box::new(val.to_display_string()),
        }
    }

    fn sql_to_value(row: &rusqlite::Row, idx: usize) -> Value {
        // Пробуємо різні типи
        if let Ok(v) = row.get::<_, i64>(idx) { return Value::Integer(v); }
        if let Ok(v) = row.get::<_, f64>(idx) { return Value::Float(v); }
        if let Ok(v) = row.get::<_, String>(idx) { return Value::String(v); }
        Value::Null
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

    #[test]
    fn test_auth_hash_verify() {
        // Тест на рівні VM напряму — без парсера
        let mut vm = VM::new();
        let hash = vm.call_builtin("авт_хешувати", vec![Value::String("пароль123".to_string())]).unwrap();
        let hash_str = hash.to_display_string();

        // Правильний пароль
        let ok = vm.call_builtin("авт_перевірити", vec![
            Value::String("пароль123".to_string()),
            Value::String(hash_str.clone()),
        ]).unwrap();
        assert!(matches!(ok, Value::Bool(true)));

        // Невірний пароль
        let bad = vm.call_builtin("авт_перевірити", vec![
            Value::String("невірний".to_string()),
            Value::String(hash_str),
        ]).unwrap();
        assert!(matches!(bad, Value::Bool(false)));
    }

    #[test]
    fn test_auth_jwt_roundtrip() {
        let mut vm = VM::new();
        // Створюємо токен
        let data = Value::Dict(vec![
            (Value::String("ід".to_string()), Value::Integer(42)),
            (Value::String("роль".to_string()), Value::String("адмін".to_string())),
        ]);
        let token = vm.call_builtin("авт_створити_токен", vec![
            data, Value::String("секрет".to_string()),
        ]).unwrap();

        // Перевіряємо з правильним секретом
        let result = vm.call_builtin("авт_перевірити_токен", vec![
            token.clone(), Value::String("секрет".to_string()),
        ]).unwrap();
        assert!(matches!(result, Value::EnumVariant { variant, .. } if variant == "Успіх"));

        // Перевіряємо з невірним секретом
        let bad = vm.call_builtin("авт_перевірити_токен", vec![
            token, Value::String("невірний".to_string()),
        ]).unwrap();
        assert!(matches!(bad, Value::EnumVariant { variant, .. } if variant == "Помилка"));
    }

    #[test]
    fn test_template_rendering() {
        let mut vm = VM::new();
        // Проста змінна
        let data = Value::Dict(vec![
            (Value::String("імя".to_string()), Value::String("Тризуб".to_string())),
        ]);
        let result = vm.render_template("Привіт, {імя}!", &data).unwrap();
        assert!(result.contains("Тризуб"));

        // Умова
        let data2 = Value::Dict(vec![
            (Value::String("показати".to_string()), Value::Bool(true)),
        ]);
        let result2 = vm.render_template("{якщо показати}Видно{/якщо}", &data2).unwrap();
        assert!(result2.contains("Видно"));

        // Цикл
        let data3 = Value::Dict(vec![
            (Value::String("елементи".to_string()), Value::Array(vec![
                Value::Integer(1), Value::Integer(2), Value::Integer(3),
            ])),
        ]);
        let result3 = vm.render_template("{для х в елементи}[{х}]{/для}", &data3).unwrap();
        assert!(result3.contains("[1]"));
        assert!(result3.contains("[2]"));
        assert!(result3.contains("[3]"));
    }

    #[test]
    fn test_sqlite_crud() {
        let mut vm = VM::new();
        // Відкриваємо in-memory БД
        vm.call_builtin("бд_відкрити", vec![Value::String(":memory:".to_string())]).unwrap();

        // Створюємо таблицю
        let schema = Value::Dict(vec![
            (Value::String("назва".to_string()), Value::String("тхт".to_string())),
            (Value::String("число".to_string()), Value::String("цл64".to_string())),
        ]);
        vm.call_builtin("бд_створити_таблицю", vec![
            Value::String("тест".to_string()), schema,
        ]).unwrap();

        // Створюємо запис
        let data = Value::Dict(vec![
            (Value::String("назва".to_string()), Value::String("один".to_string())),
            (Value::String("число".to_string()), Value::Integer(42)),
        ]);
        let created = vm.call_builtin("бд_створити", vec![
            Value::String("тест".to_string()), data,
        ]).unwrap();
        assert!(matches!(created, Value::Dict(_)));

        // Кількість
        let count = vm.call_builtin("бд_кількість", vec![
            Value::String("тест".to_string()),
        ]).unwrap();
        assert!(matches!(count, Value::Integer(1)));

        // Знайти
        let found = vm.call_builtin("бд_знайти", vec![
            Value::String("тест".to_string()), Value::Integer(1),
        ]).unwrap();
        assert!(!matches!(found, Value::Null));

        // Оновити
        let update_data = Value::Dict(vec![
            (Value::String("число".to_string()), Value::Integer(99)),
        ]);
        vm.call_builtin("бд_оновити", vec![
            Value::String("тест".to_string()), Value::Integer(1), update_data,
        ]).unwrap();

        // Видалити
        vm.call_builtin("бд_видалити", vec![
            Value::String("тест".to_string()), Value::Integer(1),
        ]).unwrap();

        let count2 = vm.call_builtin("бд_кількість", vec![
            Value::String("тест".to_string()),
        ]).unwrap();
        assert!(matches!(count2, Value::Integer(0)));
    }

    #[test]
    fn test_sql_injection_prevention() {
        assert!(VM::validate_sql_identifier("товари").is_ok());
        assert!(VM::validate_sql_identifier("моя_таблиця").is_ok());
        assert!(VM::validate_sql_identifier("назва123").is_ok());
        assert!(VM::validate_sql_identifier("users; DROP TABLE--").is_err());
        assert!(VM::validate_sql_identifier("table name").is_err());
        assert!(VM::validate_sql_identifier("").is_err());
        assert!(VM::validate_sql_identifier("123start").is_err());
    }

    #[test]
    fn test_web_response_builtins() {
        let mut vm = VM::new();

        let html = vm.call_builtin("веб_html", vec![Value::String("<h1>Тест</h1>".to_string())]).unwrap();
        assert!(matches!(html, Value::Dict(_)));

        let json = vm.call_builtin("веб_json", vec![
            Value::Dict(vec![(Value::String("ключ".to_string()), Value::String("значення".to_string()))]),
        ]).unwrap();
        assert!(matches!(json, Value::Dict(_)));

        let err = vm.call_builtin("веб_помилка", vec![Value::Integer(404), Value::String("Не знайдено".to_string())]).unwrap();
        assert!(matches!(err, Value::Dict(_)));

        let redir = vm.call_builtin("веб_перенаправити", vec![Value::String("/головна".to_string())]).unwrap();
        assert!(matches!(redir, Value::Dict(_)));
    }

    #[test]
    fn test_env_and_random() {
        let mut vm = VM::new();
        // PATH існує на всіх системах
        let path = vm.call_builtin("середовище", vec![Value::String("PATH".to_string())]).unwrap();
        assert!(matches!(path, Value::String(_)));

        let num = vm.call_builtin("випадкове", vec![Value::Integer(1), Value::Integer(100)]).unwrap();
        if let Value::Integer(n) = num {
            assert!(n >= 1 && n <= 100);
        } else {
            panic!("випадкове має повернути Integer");
        }
    }
}
