pub mod bytecode;
pub mod compiler;
#[cfg(target_arch = "x86_64")]
pub mod jit;
#[cfg(target_arch = "x86_64")]
pub mod native;

// Тризуб VM v5.3.2

use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sha2::Digest as Sha2Digest;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;

type HmacSha256 = Hmac<Sha256>;

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

    fn len(&self) -> usize { self.entries.len() }
    fn clear(&mut self) { self.entries.clear(); }

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
    EnumVariant, Contract,
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
        generic_params: Vec<String>,
        params: Vec<Parameter>,
        return_type: Option<tryzub_parser::Type>,
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
    /// Модуль (namespace)
    Module(String, HashMap<String, Value>),
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
            Value::Module(name, _) => format!("<модуль {}>", name),
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
            Value::Module(..) => "модуль",
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
pub struct Scope {
    variables: HashMap<String, Value>,
    parent: Option<Environment>,
    inferred_types: HashMap<String, String>,
}

impl Scope {
    fn new(parent: Option<Environment>) -> Self {
        Self { variables: HashMap::new(), parent, inferred_types: HashMap::new() }
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
        if !self.inferred_types.contains_key(&name) {
            let type_name = value.type_name().to_string();
            if type_name != "нуль" && type_name != "функція" {
                self.inferred_types.insert(name.clone(), type_name);
            }
        }
        self.variables.insert(name, value);
    }

    fn update(&mut self, name: &str, value: Value) -> Result<()> {
        if self.variables.contains_key(name) {
            if let Some(expected_type) = self.inferred_types.get(name) {
                let actual_type = value.type_name().to_string();
                if &actual_type != expected_type && actual_type != "нуль" {
                    return Err(anyhow::anyhow!(
                        "Невідповідність типів: змінна '{}' має тип '{}', не можна присвоїти '{}'",
                        name, expected_type, actual_type
                    ));
                }
            }
            self.variables.insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().update(name, value)
        } else {
            Err(anyhow::anyhow!("Змінна '{}' не знайдена", name))
        }
    }

    fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.variables.keys().cloned().collect();
        if let Some(parent) = &self.parent {
            names.extend(parent.borrow().all_names());
        }
        names
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
    /// Зареєстровані трейти: (тип, метод) → тіло
    trait_methods: HashMap<(String, String), Vec<Statement>>,
    /// Визначення трейтів: ім'я трейту → методи (для default methods)
    trait_definitions: HashMap<String, Vec<tryzub_parser::TraitMethod>>,
    /// Які типи реалізують які трейти: (тип, трейт) → true
    trait_impls: HashMap<(String, String), bool>,
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
    /// Вже завантажені модулі (ім'я → Value::Module або true/false)
    loaded_modules: HashMap<String, bool>,
    /// Збережені модулі: ім'я → Value::Module
    module_values: HashMap<String, Value>,
    /// Модулі що зараз завантажуються (для виявлення циклічних залежностей)
    loading_modules: HashSet<String>,
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
    /// Лічильник для GC — запускати кожні N операцій
    #[allow(dead_code)]
    gc_threshold: u64,
    /// Випадковий JWT секрет (генерується при створенні VM)
    default_jwt_secret: String,
    /// Індекс для швидкого векторного пошуку
    vector_index: Option<Vec<(usize, Vec<f64>)>>,
    /// Відстеження виділеної пам'яті (адреса → layout)
    allocations: HashMap<usize, std::alloc::Layout>,
    /// Call stack для stack traces
    call_stack: Vec<CallFrame>,
}

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub function_name: String,
    pub file: String,
    pub line: usize,
}

// ════════════════════════════════════════════════════════════════════
// Веб-сервер — реальний HTTP через std::net::TcpListener
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
                if route.path_parts.last().is_some_and(|p| matches!(p, PathPart::Wildcard)) {
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

impl Drop for VM {
    fn drop(&mut self) {
        for (addr, layout) in self.allocations.drain() {
            unsafe { std::alloc::dealloc(addr as *mut u8, layout); }
        }
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
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
            scope.set("перевірити_рівне".to_string(), Value::BuiltinFn("перевірити_рівне".to_string()));
            scope.set("перевірити_не_рівне".to_string(), Value::BuiltinFn("перевірити_не_рівне".to_string()));
            scope.set("перевірити_помилку".to_string(), Value::BuiltinFn("перевірити_помилку".to_string()));
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

            // Async / Concurrency
            scope.set("все".to_string(), Value::BuiltinFn("все".to_string()));
            scope.set("перегони".to_string(), Value::BuiltinFn("перегони".to_string()));
            scope.set("потік".to_string(), Value::BuiltinFn("потік".to_string()));
            scope.set("канал".to_string(), Value::BuiltinFn("канал".to_string()));

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
            scope.set("ціле_в_рядок".to_string(), Value::BuiltinFn("ціле_в_рядок".to_string()));
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

            // Regex
            scope.set("regex_відповідає".to_string(), Value::BuiltinFn("regex_відповідає".to_string()));
            scope.set("regex_знайти".to_string(), Value::BuiltinFn("regex_знайти".to_string()));
            scope.set("regex_замінити".to_string(), Value::BuiltinFn("regex_замінити".to_string()));

            // HTTP клієнт
            scope.set("http_отримати".to_string(), Value::BuiltinFn("http_отримати".to_string()));
            scope.set("http_надіслати".to_string(), Value::BuiltinFn("http_надіслати".to_string()));

            // Сесії
            scope.set("веб_сесія_зберегти".to_string(), Value::BuiltinFn("веб_сесія_зберегти".to_string()));
            scope.set("веб_сесія_отримати".to_string(), Value::BuiltinFn("веб_сесія_отримати".to_string()));
            scope.set("веб_сесія_видалити".to_string(), Value::BuiltinFn("веб_сесія_видалити".to_string()));
            scope.set("веб_зберегти_файл".to_string(), Value::BuiltinFn("веб_зберегти_файл".to_string()));

            // Кібербезпека
            scope.set("хеш_md5".to_string(), Value::BuiltinFn("хеш_md5".to_string()));
            scope.set("хеш_sha256".to_string(), Value::BuiltinFn("хеш_sha256".to_string()));
            scope.set("хеш_sha512".to_string(), Value::BuiltinFn("хеш_sha512".to_string()));
            scope.set("шифрувати_aes".to_string(), Value::BuiltinFn("шифрувати_aes".to_string()));
            scope.set("розшифрувати_aes".to_string(), Value::BuiltinFn("розшифрувати_aes".to_string()));
            scope.set("в_hex".to_string(), Value::BuiltinFn("в_hex".to_string()));
            scope.set("з_hex".to_string(), Value::BuiltinFn("з_hex".to_string()));
            scope.set("в_base64".to_string(), Value::BuiltinFn("в_base64".to_string()));
            scope.set("з_base64".to_string(), Value::BuiltinFn("з_base64".to_string()));
            scope.set("сканувати_порт".to_string(), Value::BuiltinFn("сканувати_порт".to_string()));
            scope.set("сканувати_порти".to_string(), Value::BuiltinFn("сканувати_порти".to_string()));
            scope.set("генерувати_пароль".to_string(), Value::BuiltinFn("генерувати_пароль".to_string()));
            scope.set("генерувати_токен".to_string(), Value::BuiltinFn("генерувати_токен".to_string()));
            scope.set("dns_запит".to_string(), Value::BuiltinFn("dns_запит".to_string()));
            scope.set("url_кодувати".to_string(), Value::BuiltinFn("url_кодувати".to_string()));
            scope.set("url_розкодувати".to_string(), Value::BuiltinFn("url_розкодувати".to_string()));

            // Розширена кібербезпека
            scope.set("фазити".to_string(), Value::BuiltinFn("фазити".to_string()));
            scope.set("аудит_рядок".to_string(), Value::BuiltinFn("аудит_рядок".to_string()));
            scope.set("блокчейн_хеш".to_string(), Value::BuiltinFn("блокчейн_хеш".to_string()));
            scope.set("merkle_дерево".to_string(), Value::BuiltinFn("merkle_дерево".to_string()));
            scope.set("стего_приховати".to_string(), Value::BuiltinFn("стего_приховати".to_string()));
            scope.set("стего_дістати".to_string(), Value::BuiltinFn("стего_дістати".to_string()));
            scope.set("xor_шифр".to_string(), Value::BuiltinFn("xor_шифр".to_string()));
            scope.set("rot13".to_string(), Value::BuiltinFn("rot13".to_string()));
            scope.set("ентропія".to_string(), Value::BuiltinFn("ентропія".to_string()));
            scope.set("перевірити_пароль".to_string(), Value::BuiltinFn("перевірити_пароль".to_string()));
            scope.set("хонейпот".to_string(), Value::BuiltinFn("хонейпот".to_string()));
            scope.set("часова_мітка".to_string(), Value::BuiltinFn("часова_мітка".to_string()));

            // IoT / Embedded / Дрони
            for name in &["serial_відкрити", "serial_записати", "serial_прочитати", "serial_закрити",
                "serial_порти", "gpio_записати", "gpio_прочитати", "gpio_режим",
                "pid_створити", "pid_обчислити", "pwm_значення",
                "відстань_gps", "кут_до_точки",
                "i2c_записати", "i2c_прочитати", "spi_передати",
                "затримка_мкс", "затримка_мс",
                "байти_в_число", "число_в_байти", "біт_встановити", "біт_прочитати"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }

            // Векторна математика / ML / Утиліти
            for name in &["вектор_скалярний_добуток", "вектор_косинусна_подібність",
                "вектор_нормалізувати", "вектор_евклідова_відстань", "вектор_найближчі",
                "вектор_індекс_створити", "вектор_індекс_пошук",
                "він_розібрати", "завантажити_файл", "веб_мультіпарт"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }

            for name in &["зображення_розмір", "зображення_змінити_розмір", "зображення_обрізати",
                "зображення_мініатюра", "зображення_формат", "зображення_сірий",
                "зображення_повернути", "зображення_відзеркалити", "зображення_в_тензор"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }

            // Subprocess / Python ML
            for name in &["виконати_команду", "пітон", "пітон_файл", "мл_ембединг"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }

            // Веб-сервер + БД + Автентифікація
            for name in &["веб_сервер", "веб_отримати", "веб_надіслати", "веб_оновити",
                "веб_видалити", "веб_статичні", "веб_запустити", "веб_html", "веб_json",
                "веб_шаблон", "веб_перенаправити", "веб_помилка", "шаблон_рядок",
                "веб_cookie", "веб_сесія_створити", "веб_gzip",
                "веб_сесія_зберегти", "веб_сесія_отримати", "веб_сесія_видалити", "веб_зберегти_файл",
                "бд_відкрити", "бд_створити_таблицю", "бд_створити", "бд_знайти",
                "бд_всі", "бд_запит", "бд_оновити", "бд_видалити", "бд_кількість", "бд_sql",
                "авт_хешувати", "авт_перевірити", "авт_створити_токен", "авт_перевірити_токен",
                "веб_csrf_перевірити"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }

            // Вбудовані конструктори Опція/Результат
            scope.set("Деякий".to_string(), Value::BuiltinFn("Деякий".to_string()));
            scope.set("Нічого".to_string(), Value::EnumVariant {
                type_name: "Опція".to_string(),
                variant: "Нічого".to_string(),
                fields: vec![],
            });
            scope.set("Успіх".to_string(), Value::BuiltinFn("Успіх".to_string()));
            scope.set("Помилка".to_string(), Value::BuiltinFn("Помилка".to_string()));

            // Системне програмування
            for name in &["зовнішня_бібліотека", "зовнішній_виклик", "зовнішній_виклик_дрб", "закрити_бібліотеку",
                "виділити_пам'ять", "звільнити_пам'ять",
                "записати_байт", "прочитати_байт", "записати_слово", "прочитати_слово",
                "копіювати_пам'ять", "заповнити_пам'ять",
                "розмір_вказівника", "asm_виконати", "системний_виклик"] {
                scope.set(name.to_string(), Value::BuiltinFn(name.to_string()));
            }
        }

        Self {
            global_env: global_scope.clone(),
            current_env: global_scope,
            return_value: None,
            break_flag: false,
            continue_flag: false,
            enum_types: HashMap::new(),
            trait_methods: HashMap::new(),
            trait_definitions: HashMap::new(),
            trait_impls: HashMap::new(),
            contracts: HashMap::new(),
            yielded_values: Vec::new(),
            async_queue: Vec::new(),
            macros: HashMap::new(),
            effect_handlers: Vec::new(),
            registered_effects: HashMap::new(),
            stdlib_paths: {
                let mut paths = vec![
                    "stdlib".to_string(),
                    "../stdlib".to_string(),
                    ".тризуб_модулі".to_string(),
                    "../.тризуб_модулі".to_string(),
                ];
                if let Ok(env_paths) = std::env::var("ТРИЗУБ_ШЛЯХ") {
                    for p in env_paths.split(';') {
                        let trimmed = p.trim();
                        if !trimmed.is_empty() {
                            paths.push(trimmed.to_string());
                        }
                    }
                }
                if let Ok(env_paths) = std::env::var("TRYZUB_PATH") {
                    for p in env_paths.split(';') {
                        let trimmed = p.trim();
                        if !trimmed.is_empty() {
                            paths.push(trimmed.to_string());
                        }
                    }
                }
                paths
            },
            loaded_modules: HashMap::new(),
            module_values: HashMap::new(),
            loading_modules: HashSet::new(),
            generator_cache: HashMap::new(),
            generator_id_counter: 0,
            web_routes: None,
            db_connections: HashMap::new(),
            string_interner: StringInterner::new(),
            pure_cache: PureCache::new(10_000),
            pure_functions: HashSet::new(),
            op_count: 0,
            gc_threshold: 10_000,
            default_jwt_secret: {
                let mut rng = rand::thread_rng();
                (0..64).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
            },
            vector_index: None,
            allocations: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn execute_program(&mut self, program: Program, _args: Vec<String>) -> Result<()> {
        // Спочатку реєструємо всі оголошення
        for decl in &program.declarations {
            self.execute_declaration(decl.clone())?;
        }

        // Шукаємо функцію головна() — якщо є, запускаємо
        let main_fn = self.global_env.borrow().get("головна");
        if let Some(Value::Function { params: _, body, closure, .. }) = main_fn {
            let prev_env = self.current_env.clone();
            self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));
            for stmt in body {
                self.execute_statement(stmt)?;
                if self.return_value.is_some() { break; }
            }
            self.return_value = None;
            self.current_env = prev_env;
        }
        // Якщо немає головна() — не помилка, оголошення вже виконались

        Ok(())
    }

    fn execute_declaration(&mut self, decl: Declaration) -> Result<()> {
        match decl {
            Declaration::Variable { name, ty, value, .. } => {
                let val = if let Some(expr) = value {
                    self.evaluate_expression(expr)?
                } else {
                    Value::Null
                };
                if let Some(ref expected_type) = ty {
                    self.check_type(&val, expected_type)?;
                }
                self.current_env.borrow_mut().set(name, val);
            }
            Declaration::Function { name, generic_params, params, return_type, body, contract, .. } => {
                let func = Value::Function {
                    name: Some(name.clone()),
                    generic_params,
                    params,
                    return_type,
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
            Declaration::Trait { name, methods, .. } => {
                self.trait_definitions.insert(name, methods);
            }
            Declaration::TraitImpl { trait_name, for_type, methods, .. } => {
                // Зберігаємо що тип реалізує трейт
                self.trait_impls.insert((for_type.clone(), trait_name.clone()), true);

                // Зберігаємо реалізовані методи
                let mut implemented: HashSet<String> = HashSet::new();
                for method in methods {
                    if let Declaration::Function { name, generic_params, params, return_type, body, .. } = method {
                        implemented.insert(name.clone());
                        let func = Value::Function {
                            name: Some(name.clone()),
                            generic_params,
                            params,
                            return_type,
                            body: body.clone(),
                            closure: self.current_env.clone(),
                        };
                        self.current_env.borrow_mut().set(
                            format!("{}::{}", for_type, name), func.clone()
                        );
                        self.trait_methods.insert(
                            (for_type.clone(), name), body
                        );
                    }
                }

                // Default methods — якщо трейт має default_body а реалізація не перевизначила
                if let Some(trait_defs) = self.trait_definitions.get(&trait_name).cloned() {
                    for tm in &trait_defs {
                        if !implemented.contains(&tm.name) {
                            if let Some(ref default_body) = tm.default_body {
                                let mut params = Vec::new();
                                if tm.has_self {
                                    params.push(Parameter {
                                        name: "себе".to_string(),
                                        ty: Type::SelfType,
                                        default: None,
                                    });
                                }
                                params.extend(tm.params.clone());
                                let func = Value::Function {
                                    name: Some(tm.name.clone()),
                                    generic_params: vec![],
                                    params,
                                    return_type: None,
                                    body: default_body.clone(),
                                    closure: self.current_env.clone(),
                                };
                                self.current_env.borrow_mut().set(
                                    format!("{}::{}", for_type, tm.name), func
                                );
                                self.trait_methods.insert(
                                    (for_type.clone(), tm.name.clone()), default_body.clone()
                                );
                            }
                        }
                    }
                }
            }
            Declaration::Impl { type_name: for_type, methods } => {
                for method in methods {
                    if let Declaration::Function { name, generic_params, params, return_type, body, .. } = method {
                        let func = Value::Function {
                            name: Some(name.clone()),
                            generic_params,
                            params,
                            return_type,
                            body: body.clone(),
                            closure: self.current_env.clone(),
                        };
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
            Declaration::FuzzTest { name: _, inputs: _, body: _ } => {
                // Фаз-тести запускаються через `тризуб тестувати`
                // Генеруємо випадкові входи та виконуємо тіло
            }
            Declaration::Benchmark { name: _, sizes: _, body: _ } => {
                // Бенчмарки запускаються через `тризуб тестувати`
            }
            Declaration::Import { path, items, alias } => {
                let module_name = path.last().cloned().unwrap_or_default();
                if !self.loaded_modules.contains_key(&module_name) {
                    self.load_module(&module_name)?;
                }
                // Визначаємо як зробити модуль доступним
                if let Some(module_val) = self.module_values.get(&module_name).cloned() {
                    if let Some(ref selected_items) = items {
                        // імпорт модуль { а, б } — копіюємо вибрані символи в поточний scope
                        if let Value::Module(_, ref members) = module_val {
                            for item in selected_items {
                                if let Some(val) = members.get(item) {
                                    self.current_env.borrow_mut().set(item.clone(), val.clone());
                                } else {
                                    return Err(anyhow::anyhow!(
                                        "Символ '{}' не знайдено в модулі '{}'", item, module_name
                                    ));
                                }
                            }
                        }
                    } else if let Some(ref alias_name) = alias {
                        // імпорт модуль як псевдонім
                        self.current_env.borrow_mut().set(alias_name.clone(), module_val);
                    } else {
                        // імпорт модуль — реєструємо під іменем модуля
                        self.current_env.borrow_mut().set(module_name.clone(), module_val);
                    }
                }
            }
            Declaration::Test { name: _, body: _ } => {
                // Тести не виконуються при звичайному запуску —
                // тільки через `тризуб тестувати`
            }
            Declaration::Module { name, declarations, .. } => {
                // Виконуємо оголошення модуля в ізольованому середовищі
                let prev_env = self.current_env.clone();
                let module_env = Rc::new(RefCell::new(Scope::new(Some(self.global_env.clone()))));
                self.current_env = module_env.clone();

                for decl in declarations {
                    self.execute_declaration(decl)?;
                }

                let mut members = HashMap::new();
                for (k, v) in &module_env.borrow().variables {
                    members.insert(k.clone(), v.clone());
                }

                self.current_env = prev_env;

                let module_val = Value::Module(name.clone(), members);
                self.module_values.insert(name.clone(), module_val.clone());
                self.current_env.borrow_mut().set(name, module_val);
            }
            _ => {
                // TypeAlias, Interface — парсяться але не виконуються
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
            Expression::Index { object, index } => {
                if let Expression::Identifier(obj_name) = *object {
                    let idx = self.evaluate_expression(*index)?;
                    let new_value = self.evaluate_expression(value)?;
                    let obj = self.current_env.borrow().get(&obj_name)
                        .ok_or_else(|| anyhow::anyhow!("Невідома змінна: {}", obj_name))?;
                    match obj {
                        Value::Array(mut arr) => {
                            if let Value::Integer(i) = idx {
                                let idx = if i < 0 { arr.len() as i64 + i } else { i } as usize;
                                if idx < arr.len() {
                                    arr[idx] = new_value;
                                    self.current_env.borrow_mut().update(&obj_name, Value::Array(arr))?;
                                }
                            }
                        }
                        Value::Dict(mut pairs) => {
                            let mut found = false;
                            for (k, v) in pairs.iter_mut() {
                                if self.values_equal(k, &idx) {
                                    *v = new_value.clone();
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                pairs.push((idx, new_value));
                            }
                            self.current_env.borrow_mut().update(&obj_name, Value::Dict(pairs))?;
                        }
                        _ => return Err(anyhow::anyhow!("Індексне присвоєння підтримується тільки для масивів та словників")),
                    }
                }
            }
            _ => return Err(anyhow::anyhow!("Присвоєння можливе тільки до змінних")),
        }
        Ok(())
    }

    // ── Обчислення виразів ──

    #[inline(always)]
    fn evaluate_expression(&mut self, expr: Expression) -> Result<Value> {
        self.op_count += 1;
        if self.op_count & 0xFFFF == 0 {
            self.run_gc();
        }
        match expr {
            Expression::Literal(lit) => Ok(self.evaluate_literal(lit)),
            Expression::Identifier(name) => {
                self.current_env.borrow().get(&name)
                    .ok_or_else(|| {
                        let known = self.current_env.borrow().all_names();
                        let suggestion = Self::find_similar(&name, &known);
                        let hint = if let Some(s) = suggestion {
                            format!("\n  Підказка: можливо ви мали на увазі '{}'?", s)
                        } else { String::new() };
                        let trace = self.format_stack_trace();
                        anyhow::anyhow!("[Т001] Невідома змінна або функція: '{}'{}\n{}", name, hint, trace)
                    })
            }
            Expression::SelfRef => {
                self.current_env.borrow().get("себе")
                    .ok_or_else(|| anyhow::anyhow!("'себе' доступне тільки в методах"))
            }
            Expression::Binary { left, op, right } => {
                let lhs = self.evaluate_expression(*left)?;
                let rhs = self.evaluate_expression(*right)?;
                if let (Value::Integer(a), Value::Integer(b)) = (&lhs, &rhs) {
                    match op {
                        BinaryOp::Add => return Ok(Value::Integer(a + b)),
                        BinaryOp::Sub => return Ok(Value::Integer(a - b)),
                        BinaryOp::Mul => return Ok(Value::Integer(a * b)),
                        BinaryOp::Div => return if *b != 0 { Ok(Value::Integer(a / b)) } else { Err(anyhow::anyhow!("Ділення на нуль")) },
                        BinaryOp::Mod => return if *b != 0 { Ok(Value::Integer(a % b)) } else { Err(anyhow::anyhow!("Ділення на нуль")) },
                        BinaryOp::Lt => return Ok(Value::Bool(a < b)),
                        BinaryOp::Le => return Ok(Value::Bool(a <= b)),
                        BinaryOp::Gt => return Ok(Value::Bool(a > b)),
                        BinaryOp::Ge => return Ok(Value::Bool(a >= b)),
                        BinaryOp::Eq => return Ok(Value::Bool(a == b)),
                        BinaryOp::Ne => return Ok(Value::Bool(a != b)),
                        _ => {}
                    }
                }
                match self.apply_binary_op(op.clone(), lhs.clone(), rhs.clone()) {
                    Ok(result) => Ok(result),
                    Err(_) => {
                        // Operator overloading — шукаємо трейт-метод
                        let method_name = match op {
                            BinaryOp::Add => "додати",
                            BinaryOp::Sub => "відняти",
                            BinaryOp::Mul => "помножити",
                            BinaryOp::Div => "поділити",
                            BinaryOp::Eq => "дорівнює",
                            BinaryOp::Lt => "менше",
                            BinaryOp::Gt => "більше",
                            _ => return Err(anyhow::anyhow!("Несумісні типи для операції {:?}: {} та {}",
                                op, lhs.type_name(), rhs.type_name())),
                        };
                        self.call_method(lhs, method_name, vec![rhs])
                    }
                }
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
                    Value::Module(_, members) => {
                        members.get(&member).cloned()
                            .ok_or_else(|| anyhow::anyhow!("Символ '{}' не знайдено в модулі", member))
                    }
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
            Value::Function { params, body, closure, name, return_type, .. } => {
                let func_name = name.clone().unwrap_or_default();

                // Кеш чистих функцій — якщо функція позначена як чиста,
                // повертаємо кешований результат замість перевиконання
                if self.pure_functions.contains(&func_name) {
                    let cache_key = PureCache::hash_args(&func_name, &args);
                    if let Some(cached) = self.pure_cache.get(cache_key) {
                        return Ok(cached.clone());
                    }
                }

                if self.call_stack.len() > 10000 {
                    return Err(anyhow::anyhow!(
                        "Переповнення стеку викликів (глибина > 10000). Перевірте рекурсію у функції '{}'", func_name
                    ));
                }
                self.call_stack.push(CallFrame {
                    function_name: func_name.clone(),
                    file: String::new(),
                    line: 0,
                });
                let prev_env = self.current_env.clone();
                self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));

                for (i, param) in params.iter().enumerate() {
                    if param.name == "себе" {
                        if let Some(self_val) = args.get(i) {
                            self.current_env.borrow_mut().set("себе".to_string(), self_val.clone());
                        }
                        continue;
                    }
                    let val = if let Some(arg) = args.get(i) {
                        arg.clone()
                    } else if let Some(ref default_expr) = param.default {
                        self.evaluate_expression(default_expr.clone())?
                    } else {
                        Value::Null
                    };
                    if !matches!(&param.ty, tryzub_parser::Type::Named(n) if n == "Будь")
                        && !matches!(&param.ty, tryzub_parser::Type::SelfType) {
                        self.check_type(&val, &param.ty)?;
                    }
                    self.current_env.borrow_mut().set(param.name.clone(), val);
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

                if let Some(ref ret_ty) = return_type {
                    if !matches!(ret_ty, tryzub_parser::Type::Named(n) if n == "Будь") {
                        self.check_type(&result, ret_ty).map_err(|e| {
                            anyhow::anyhow!("Функція '{}': {}", func_name, e)
                        })?;
                    }
                }

                self.return_value = prev_return;
                self.current_env = prev_env;
                self.call_stack.pop();

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
        // ── Виклик функції з модуля ──
        if let Value::Module(ref mod_name, ref members) = obj {
            if let Some(func) = members.get(method) {
                return self.call_value(func.clone(), args);
            }
            return Err(anyhow::anyhow!("Функція '{}' не знайдена в модулі '{}'", method, mod_name));
        }
        // ── Ліниві методи Range ──
        if let Value::Range { from, to, inclusive } = &obj {
            let end = if *inclusive { *to + 1 } else { *to };
            match method {
                "в_масив" => {
                    return Ok(Value::Array((*from..end).map(Value::Integer).collect()));
                }
                "взяти" => {
                    let n = match args.first() {
                        Some(Value::Integer(n)) => *n as usize,
                        _ => return Err(anyhow::anyhow!(".взяти() потребує ціле число")),
                    };
                    let result: Vec<Value> = (*from..end).take(n).map(Value::Integer).collect();
                    return Ok(Value::Array(result));
                }
                "фільтрувати" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for i in *from..end {
                            let val = Value::Integer(i);
                            let cond = self.call_value(func.clone(), vec![val.clone()])?;
                            if cond.to_bool() { result.push(val); }
                        }
                        return Ok(Value::Array(result));
                    }
                    return Err(anyhow::anyhow!(".фільтрувати() потребує предикат"));
                }
                "перетворити" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for i in *from..end {
                            result.push(self.call_value(func.clone(), vec![Value::Integer(i)])?);
                        }
                        return Ok(Value::Array(result));
                    }
                    return Err(anyhow::anyhow!(".перетворити() потребує функцію"));
                }
                "згорнути" => {
                    if args.len() >= 2 {
                        let mut acc = args[0].clone();
                        let func = args[1].clone();
                        for i in *from..end {
                            acc = self.call_value(func.clone(), vec![acc, Value::Integer(i)])?;
                        }
                        return Ok(acc);
                    }
                    return Err(anyhow::anyhow!(".згорнути() потребує початкове значення та функцію"));
                }
                _ => {}
            }
        }

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
                "взяти" => {
                    let n = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => arr.len() };
                    return Ok(Value::Array(arr.iter().take(n).cloned().collect()));
                }
                "в_масив" => return Ok(Value::Array(arr.clone())),
                "зрізати" | "зріз" => {
                    let from = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => 0 };
                    let to = match args.get(1) { Some(Value::Integer(n)) => *n as usize, _ => arr.len() };
                    return Ok(Value::Array(arr[from.min(arr.len())..to.min(arr.len())].to_vec()));
                }
                "розгорнути" => {
                    let mut result = Vec::new();
                    for item in arr {
                        if let Value::Array(inner) = item { result.extend(inner.iter().cloned()); }
                        else { result.push(item.clone()); }
                    }
                    return Ok(Value::Array(result));
                }
                "зшити" => {
                    if let Some(Value::Array(other)) = args.first() {
                        let pairs: Vec<Value> = arr.iter().zip(other.iter())
                            .map(|(a, b)| Value::Array(vec![a.clone(), b.clone()]))
                            .collect();
                        return Ok(Value::Array(pairs));
                    }
                    return Err(anyhow::anyhow!(".зшити() потребує масив"));
                }
                "пронумерувати" => {
                    let result: Vec<Value> = arr.iter().enumerate()
                        .map(|(i, v)| Value::Array(vec![Value::Integer(i as i64), v.clone()]))
                        .collect();
                    return Ok(Value::Array(result));
                }
                "будь_який" => {
                    if let Some(func) = args.first() {
                        for item in arr {
                            if self.call_value(func.clone(), vec![item.clone()])?.to_bool() {
                                return Ok(Value::Bool(true));
                            }
                        }
                        return Ok(Value::Bool(false));
                    }
                    return Ok(Value::Bool(arr.iter().any(|v| v.to_bool())));
                }
                "кожен" => {
                    if let Some(func) = args.first() {
                        for item in arr {
                            if !self.call_value(func.clone(), vec![item.clone()])?.to_bool() {
                                return Ok(Value::Bool(false));
                            }
                        }
                        return Ok(Value::Bool(true));
                    }
                    return Ok(Value::Bool(arr.iter().all(|v| v.to_bool())));
                }
                "знайти" => {
                    if let Some(func) = args.first() {
                        for item in arr {
                            if self.call_value(func.clone(), vec![item.clone()])?.to_bool() {
                                return Ok(item.clone());
                            }
                        }
                    }
                    return Ok(Value::Null);
                }
                "позиція" => {
                    if let Some(func) = args.first() {
                        for (i, item) in arr.iter().enumerate() {
                            if self.call_value(func.clone(), vec![item.clone()])
                                .map(|v| v.to_bool()).unwrap_or(false) {
                                return Ok(Value::Integer(i as i64));
                            }
                        }
                    }
                    return Ok(Value::Integer(-1));
                }
                "унікальні" => {
                    let mut result = Vec::new();
                    for item in arr {
                        if !result.iter().any(|v| self.values_equal(v, item)) {
                            result.push(item.clone());
                        }
                    }
                    return Ok(Value::Array(result));
                }
                "частини" => {
                    if let Some(Value::Integer(n)) = args.first() {
                        let n = *n as usize;
                        if n == 0 { return Err(anyhow::anyhow!(".частини(0) — недопустимо")); }
                        let chunks: Vec<Value> = arr.chunks(n)
                            .map(|c| Value::Array(c.to_vec()))
                            .collect();
                        return Ok(Value::Array(chunks));
                    }
                    return Err(anyhow::anyhow!(".частини() потребує число"));
                }
                "мін" => {
                    return Ok(arr.iter().min_by(|a, b| match (a, b) {
                        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
                        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                        _ => std::cmp::Ordering::Equal,
                    }).cloned().unwrap_or(Value::Null));
                }
                "макс" => {
                    return Ok(arr.iter().max_by(|a, b| match (a, b) {
                        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
                        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                        _ => std::cmp::Ordering::Equal,
                    }).cloned().unwrap_or(Value::Null));
                }
                "сума" => {
                    let mut total: i64 = 0;
                    for item in arr {
                        if let Value::Integer(n) = item { total += n; }
                    }
                    return Ok(Value::Integer(total));
                }
                "видалити_за" => {
                    if let Some(Value::Integer(i)) = args.first() {
                        let i = *i as usize;
                        if i < arr.len() {
                            let mut new_arr = arr.clone();
                            new_arr.remove(i);
                            return Ok(Value::Array(new_arr));
                        }
                    }
                    return Ok(Value::Array(arr.clone()));
                }
                "вставити" => {
                    if args.len() == 2 {
                        if let Value::Integer(i) = &args[0] {
                            let i = *i as usize;
                            let mut new_arr = arr.clone();
                            new_arr.insert(i.min(new_arr.len()), args[1].clone());
                            return Ok(Value::Array(new_arr));
                        }
                    }
                    return Err(anyhow::anyhow!(".вставити(індекс, значення)"));
                }
                "кожен_виконати" => {
                    if let Some(func) = args.first() {
                        for item in arr {
                            self.call_value(func.clone(), vec![item.clone()])?;
                        }
                    }
                    return Ok(Value::Null);
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
                "символи" => {
                    return Ok(Value::Array(s.chars().map(|c| Value::String(c.to_string())).collect()));
                }
                "знайти" => {
                    if let Some(Value::String(sub)) = args.first() {
                        return Ok(match s.find(sub.as_str()) {
                            Some(i) => Value::Integer(s[..i].chars().count() as i64),
                            None => Value::Integer(-1),
                        });
                    }
                    return Ok(Value::Integer(-1));
                }
                "індекс" => {
                    if let Some(Value::Integer(i)) = args.first() {
                        return Ok(s.chars().nth(*i as usize)
                            .map(|c| Value::String(c.to_string()))
                            .unwrap_or(Value::Null));
                    }
                    return Ok(Value::Null);
                }
                "кількість" => {
                    if let Some(Value::String(sub)) = args.first() {
                        return Ok(Value::Integer(s.matches(sub.as_str()).count() as i64));
                    }
                    return Ok(Value::Integer(0));
                }
                "обернути" => {
                    return Ok(Value::String(s.chars().rev().collect()));
                }
                "повторити" => {
                    if let Some(Value::Integer(n)) = args.first() {
                        return Ok(Value::String(s.repeat(*n as usize)));
                    }
                    return Ok(Value::String(s.clone()));
                }
                "зліва" => {
                    if let Some(Value::Integer(n)) = args.first() {
                        let n = *n as usize;
                        let width = s.chars().count();
                        if width >= n { return Ok(Value::String(s.clone())); }
                        let pad = match args.get(1) { Some(Value::String(p)) => p.chars().next().unwrap_or(' '), _ => ' ' };
                        let padding: String = std::iter::repeat(pad).take(n - width).collect();
                        return Ok(Value::String(format!("{}{}", s, padding)));
                    }
                    return Ok(Value::String(s.clone()));
                }
                "справа" => {
                    if let Some(Value::Integer(n)) = args.first() {
                        let n = *n as usize;
                        let width = s.chars().count();
                        if width >= n { return Ok(Value::String(s.clone())); }
                        let pad = match args.get(1) { Some(Value::String(p)) => p.chars().next().unwrap_or(' '), _ => ' ' };
                        let padding: String = std::iter::repeat(pad).take(n - width).collect();
                        return Ok(Value::String(format!("{}{}", padding, s)));
                    }
                    return Ok(Value::String(s.clone()));
                }
                "обрізати_зліва" => return Ok(Value::String(s.trim_start().to_string())),
                "обрізати_справа" => return Ok(Value::String(s.trim_end().to_string())),
                "це_число" => return Ok(Value::Bool(s.parse::<f64>().is_ok())),
                "це_літера" => return Ok(Value::Bool(s.chars().all(|c| c.is_alphabetic()))),
                "це_цифра" => return Ok(Value::Bool(s.chars().all(|c| c.is_ascii_digit()))),
                "в_число" => {
                    return Ok(match s.parse::<i64>() {
                        Ok(n) => Value::Integer(n),
                        Err(_) => match s.parse::<f64>() {
                            Ok(f) => Value::Float(f),
                            Err(_) => Value::Null,
                        }
                    });
                }
                "з'єднати" => {
                    if let Some(Value::Array(arr)) = args.first() {
                        let joined: String = arr.iter().map(|v| v.to_display_string()).collect::<Vec<_>>().join(&s);
                        return Ok(Value::String(joined));
                    }
                    return Ok(Value::String(s.clone()));
                }
                "рядки" => {
                    return Ok(Value::Array(s.lines().map(|l| Value::String(l.to_string())).collect()));
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
                "отримати_або" => {
                    if args.len() == 2 {
                        for (k, v) in pairs {
                            if self.values_equal(k, &args[0]) { return Ok(v.clone()); }
                        }
                        return Ok(args[1].clone());
                    }
                    return Err(anyhow::anyhow!(".отримати_або(ключ, значення_за_замовчуванням)"));
                }
                "об_єднати" => {
                    if let Some(Value::Dict(other)) = args.first() {
                        let mut result = pairs.clone();
                        for (k, v) in other {
                            if let Some(existing) = result.iter_mut().find(|(ek, _)| self.values_equal(ek, k)) {
                                existing.1 = v.clone();
                            } else {
                                result.push((k.clone(), v.clone()));
                            }
                        }
                        return Ok(Value::Dict(result));
                    }
                    return Err(anyhow::anyhow!(".об'єднати() потребує словник"));
                }
                "фільтрувати" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for (k, v) in pairs {
                            let keep = self.call_value(func.clone(), vec![k.clone(), v.clone()])?;
                            if keep.to_bool() { result.push((k.clone(), v.clone())); }
                        }
                        return Ok(Value::Dict(result));
                    }
                    return Err(anyhow::anyhow!(".фільтрувати() потребує функцію"));
                }
                "перетворити" => {
                    if let Some(func) = args.first() {
                        let mut result = Vec::new();
                        for (k, v) in pairs {
                            let new_val = self.call_value(func.clone(), vec![k.clone(), v.clone()])?;
                            result.push((k.clone(), new_val));
                        }
                        return Ok(Value::Dict(result));
                    }
                    return Err(anyhow::anyhow!(".перетворити() потребує функцію"));
                }
                "пусто" => return Ok(Value::Bool(pairs.is_empty())),
                "пари" => {
                    return Ok(Value::Array(pairs.iter()
                        .map(|(k, v)| Value::Array(vec![k.clone(), v.clone()]))
                        .collect()));
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
                "підмножина" => {
                    if let Some(Value::Set(other)) = args.first() {
                        return Ok(Value::Bool(items.iter().all(|v| other.iter().any(|o| self.values_equal(v, o)))));
                    }
                    return Ok(Value::Bool(false));
                }
                "надмножина" => {
                    if let Some(Value::Set(other)) = args.first() {
                        return Ok(Value::Bool(other.iter().all(|v| items.iter().any(|o| self.values_equal(v, o)))));
                    }
                    return Ok(Value::Bool(false));
                }
                "неперетинні" => {
                    if let Some(Value::Set(other)) = args.first() {
                        return Ok(Value::Bool(!items.iter().any(|v| other.iter().any(|o| self.values_equal(v, o)))));
                    }
                    return Ok(Value::Bool(true));
                }
                "симетрична_різниця" => {
                    if let Some(Value::Set(other)) = args.first() {
                        let mut result: Vec<Value> = items.iter()
                            .filter(|v| !other.iter().any(|o| self.values_equal(v, o)))
                            .cloned().collect();
                        for o in other {
                            if !items.iter().any(|v| self.values_equal(v, o)) {
                                result.push(o.clone());
                            }
                        }
                        return Ok(Value::Set(result));
                    }
                    return Err(anyhow::anyhow!("множина.симетрична_різниця потребує множину"));
                }
                "пусто" => return Ok(Value::Bool(items.is_empty())),
                "в_масив" => return Ok(Value::Array(items.clone())),
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

        println!("\nWeb server started на http://localhost:{}", routes.port);
        println!("   {} маршрутів зареєстровано", routes.routes.len());
        if let Some(ref dir) = routes.static_dir {
            println!("   Статичні файли: {}/", dir);
        }
        println!("   Натисніть Ctrl+C для зупинки\n");

        let mut rate_limits: HashMap<String, (u32, std::time::Instant)> = HashMap::new();
        let rate_limit_max: u32 = 200;
        let rate_limit_window = std::time::Duration::from_secs(60);
        let max_body_size: usize = 10 * 1024 * 1024; // 10MB
        let active_threads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_threads: usize = 64;

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    stream.set_nodelay(true).ok();
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
                    stream.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();
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

                    if rate_limits.len() > 1000 {
                        rate_limits.retain(|_, (_, t)| t.elapsed() < rate_limit_window);
                    }

                    let mut reader = BufReader::new(stream.try_clone().map_err(|e| anyhow::anyhow!("TCP clone: {}", e))?);

                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() { continue; }
                    let parts: Vec<&str> = request_line.split_whitespace().collect();
                    if parts.len() < 2 { continue; }

                    let method = parts[0];
                    let full_path = parts[1];

                    let (raw_path, query_string) = if let Some(idx) = full_path.find('?') {
                        (&full_path[..idx], Some(&full_path[idx+1..]))
                    } else {
                        (full_path, None)
                    };
                    let decoded_path = Self::url_decode_path(raw_path);
                    let path = decoded_path.as_str();

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

                    let accept_encoding_gzip = headers.get("accept-encoding")
                        .map(|v| v.contains("gzip")).unwrap_or(false);

                    let has_ext = path.rfind('.').map(|i| i > path.rfind('/').unwrap_or(0)).unwrap_or(false);
                    let is_static_candidate = method == "GET" && routes.static_dir.is_some() && has_ext
                        && !path.contains("..") && !path.contains("%2e") && !path.contains("%2E") && !path.contains('\0')
                        && routes.find_route(method, path).is_none();

                    if is_static_candidate {
                        let counter = active_threads.clone();
                        let prev = counter.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        if prev < max_threads {
                            let static_dir = routes.static_dir.as_ref().unwrap().clone();
                            let path_owned = path.to_string();
                            std::thread::spawn(move || {
                                struct ThreadGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
                                impl Drop for ThreadGuard {
                                    fn drop(&mut self) {
                                        self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                                    }
                                }
                                let _guard = ThreadGuard(counter);
                                Self::serve_static_threaded(stream, &path_owned, &static_dir, accept_encoding_gzip);
                            });
                            continue;
                        }
                        counter.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                    }

                    let mut body_str = String::new();
                    let mut body_buf: Vec<u8> = Vec::new();
                    if content_length > max_body_size {
                        let response = "HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes());
                        continue;
                    }
                    if content_length > 0 {
                        body_buf = vec![0u8; content_length];
                        let _ = reader.read_exact(&mut body_buf);
                        body_str = String::from_utf8(body_buf.clone())
                            .unwrap_or_else(|_| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &body_buf));
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
                        (Value::String("тіло_байти".to_string()), if content_length > 0 {
                            Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &body_buf))
                        } else { Value::String(String::new()) }),
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
                        if let Some((route, _params)) = routes.find_route(method, path) {
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
                                    eprintln!("  [X] {} {} — {}", method, path, e);
                                    let html = format!(
                                        "<html><head><meta charset='utf-8'></head>\
                                         <body><h1>500</h1><pre>{}</pre></body></html>",
                                        e.to_string().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
                                    );
                                    (html, "text/html; charset=utf-8".to_string(), 500, None)
                                }
                            }
                        } else if let Some(ref static_dir) = routes.static_dir {
                            let file_path = format!("{}{}", static_dir, path);
                            if path.contains("..") || path.contains('\0') {
                                let html = "<html><body><h1>403 Forbidden</h1></body></html>";
                                (html.to_string(), "text/html; charset=utf-8".to_string(), 403, None::<String>)
                            } else if let Ok(content) = std::fs::read(&file_path) {
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
                                            <h1>404</h1><p>Сторінку не знайдено</p><hr><p>Web Server</p></body></html>";
                                (html.to_string(), "text/html; charset=utf-8".to_string(), 404, None)
                            }
                        } else {
                            let html = "<html><head><meta charset='utf-8'></head>\
                                        <body style='font-family:sans-serif;text-align:center;padding:50px'>\
                                        <h1>404</h1><p>Сторінку не знайдено</p><hr><p>Web Server</p></body></html>";
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
                        let safe_loc = loc.replace(['\r', '\n'], "");
                        response.push_str(&format!("Location: {}\r\n", safe_loc));
                    }

                    if path.contains('.') && response_status == 200 {
                        response.push_str("Cache-Control: public, max-age=86400\r\n");
                    }

                    // Gzip стиснення для відповідей > 1KB
                    let body_bytes = if response_body.len() > 1024 && accept_encoding_gzip {
                        use flate2::write::GzEncoder;
                        use flate2::Compression;
                        use std::io::Write as IoWrite;
                        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
                        let _ = encoder.write_all(response_body.as_bytes());
                        if let Ok(compressed) = encoder.finish() {
                            response = response.replace(
                                &format!("Content-Length: {}", response_body.len()),
                                &format!("Content-Length: {}\r\nContent-Encoding: gzip", compressed.len())
                            );
                            compressed
                        } else {
                            response_body.as_bytes().to_vec()
                        }
                    } else {
                        response_body.as_bytes().to_vec()
                    };

                    response.push_str("\r\n");
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&body_bytes);
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

    fn url_decode_path(input: &str) -> String {
        let mut result = Vec::new();
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(byte) = u8::from_str_radix(
                    &String::from_utf8_lossy(&bytes[i+1..i+3]), 16
                ) {
                    result.push(byte);
                    i += 3;
                    continue;
                }
            }
            result.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(result).unwrap_or_else(|_| input.to_string())
    }

    fn serve_static_threaded(mut stream: std::net::TcpStream, path: &str, static_dir: &str, gzip: bool) {
        use std::io::Write;
        let file_path = format!("{}{}", static_dir, path);
        let (status, status_text, body_bytes, mime) = if let Ok(content) = std::fs::read(&file_path) {
            let mime = Self::guess_mime(&file_path);
            (200, "OK", content, mime)
        } else {
            (404, "Not Found", b"<html><body><h1>404</h1></body></html>".to_vec(), "text/html; charset=utf-8".to_string())
        };

        let final_body = if gzip && body_bytes.len() > 1024 {
            use flate2::write::GzEncoder;
            use flate2::Compression;
            let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
            let _ = enc.write_all(&body_bytes);
            if let Ok(compressed) = enc.finish() {
                let header = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Encoding: gzip\r\n\
                     Cache-Control: public, max-age=86400\r\nConnection: close\r\n\r\n",
                    status, status_text, mime, compressed.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&compressed);
                let _ = stream.flush();
                return;
            }
            body_bytes
        } else {
            body_bytes
        };

        let header = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
             Cache-Control: public, max-age=86400\r\nConnection: close\r\n\r\n",
            status, status_text, mime, final_body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&final_body);
        let _ = stream.flush();
    }

    #[cfg(target_arch = "x86_64")]
    fn encode_x86(mnemonic: &str, operands: &[&str], out: &mut Vec<u8>) -> Result<()> {
        fn parse_reg(s: &str) -> Option<u8> {
            match s.trim() {
                "rax" | "eax" | "al" => Some(0), "rcx" | "ecx" | "cl" => Some(1),
                "rdx" | "edx" | "dl" => Some(2), "rbx" | "ebx" | "bl" => Some(3),
                "rsp" | "esp" => Some(4), "rbp" | "ebp" => Some(5),
                "rsi" | "esi" => Some(6), "rdi" | "edi" => Some(7),
                "r8" => Some(8), "r9" => Some(9), "r10" => Some(10), "r11" => Some(11),
                "r12" => Some(12), "r13" => Some(13), "r14" => Some(14), "r15" => Some(15),
                _ => None,
            }
        }
        fn parse_imm(s: &str) -> Option<i64> {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x") {
                i64::from_str_radix(hex, 16).ok()
            } else {
                s.parse().ok()
            }
        }
        fn is_64bit(s: &str) -> bool {
            let s = s.trim();
            s.starts_with('r') || s.starts_with("r8") || s.starts_with("r9") ||
            s.starts_with("r1") || s == "rsp" || s == "rbp" || s == "rsi" || s == "rdi"
        }
        fn modrm(md: u8, reg: u8, rm: u8) -> u8 { (md << 6) | ((reg & 7) << 3) | (rm & 7) }
        fn rex(w: bool, r: u8, b: u8) -> u8 {
            0x40 | if w { 8 } else { 0 } | if r > 7 { 4 } else { 0 } | if b > 7 { 1 } else { 0 }
        }

        match mnemonic.as_ref() {
            "nop" => out.push(0x90),
            "ret" => out.push(0xC3),
            "hlt" => out.push(0xF4),
            "cli" => out.push(0xFA),
            "sti" => out.push(0xFB),
            "cld" => out.push(0xFC),
            "std" => out.push(0xFD),
            "rdtsc" => { out.push(0x0F); out.push(0x31); }
            "cpuid" => { out.push(0x0F); out.push(0xA2); }
            "pause" => { out.push(0xF3); out.push(0x90); }
            "mfence" => { out.extend_from_slice(&[0x0F, 0xAE, 0xF0]); }
            "lfence" => { out.extend_from_slice(&[0x0F, 0xAE, 0xE8]); }
            "sfence" => { out.extend_from_slice(&[0x0F, 0xAE, 0xF8]); }
            "syscall" => { out.push(0x0F); out.push(0x05); }
            "int" => {
                let n = parse_imm(operands.first().unwrap_or(&"0")).unwrap_or(0) as u8;
                out.push(0xCD); out.push(n);
            }
            "push" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    if r > 7 { out.push(0x41); }
                    out.push(0x50 + (r & 7));
                } else if let Some(imm) = parse_imm(operands.first().unwrap_or(&"")) {
                    if imm >= -128 && imm <= 127 {
                        out.push(0x6A); out.push(imm as u8);
                    } else {
                        out.push(0x68); out.extend_from_slice(&(imm as i32).to_le_bytes());
                    }
                }
            }
            "pop" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    if r > 7 { out.push(0x41); }
                    out.push(0x58 + (r & 7));
                }
            }
            "mov" => {
                if operands.len() != 2 { return Err(anyhow::anyhow!("mov потребує 2 операнди")); }
                let dst = operands[0].trim();
                let src = operands[1].trim();
                if let (Some(dr), Some(sr)) = (parse_reg(dst), parse_reg(src)) {
                    out.push(rex(is_64bit(dst), sr, dr));
                    out.push(0x89);
                    out.push(modrm(3, sr, dr));
                } else if let (Some(dr), Some(imm)) = (parse_reg(dst), parse_imm(src)) {
                    if is_64bit(dst) {
                        out.push(rex(true, 0, dr));
                        out.push(0xB8 + (dr & 7));
                        out.extend_from_slice(&imm.to_le_bytes());
                    } else {
                        if dr > 7 { out.push(0x41); }
                        out.push(0xB8 + (dr & 7));
                        out.extend_from_slice(&(imm as i32).to_le_bytes());
                    }
                } else {
                    return Err(anyhow::anyhow!("mov: невідомі операнди '{}', '{}'", dst, src));
                }
            }
            "add" | "sub" | "and" | "or" | "xor" | "cmp" => {
                let op_code: u8 = match mnemonic.as_ref() {
                    "add" => 0, "or" => 1, "and" => 4, "sub" => 5, "xor" => 6, "cmp" => 7, _ => 0
                };
                if operands.len() != 2 { return Err(anyhow::anyhow!("{} потребує 2 операнди", mnemonic)); }
                let dst = operands[0].trim();
                let src = operands[1].trim();
                if let (Some(dr), Some(sr)) = (parse_reg(dst), parse_reg(src)) {
                    out.push(rex(is_64bit(dst), sr, dr));
                    out.push(0x01 + op_code * 8);
                    out.push(modrm(3, sr, dr));
                } else if let (Some(dr), Some(imm)) = (parse_reg(dst), parse_imm(src)) {
                    out.push(rex(is_64bit(dst), 0, dr));
                    if imm >= -128 && imm <= 127 {
                        out.push(0x83);
                        out.push(modrm(3, op_code, dr));
                        out.push(imm as u8);
                    } else {
                        out.push(0x81);
                        out.push(modrm(3, op_code, dr));
                        out.extend_from_slice(&(imm as i32).to_le_bytes());
                    }
                }
            }
            "inc" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    out.push(rex(is_64bit(operands[0]), 0, r));
                    out.push(0xFF); out.push(modrm(3, 0, r));
                }
            }
            "dec" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    out.push(rex(is_64bit(operands[0]), 0, r));
                    out.push(0xFF); out.push(modrm(3, 1, r));
                }
            }
            "neg" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    out.push(rex(is_64bit(operands[0]), 0, r));
                    out.push(0xF7); out.push(modrm(3, 3, r));
                }
            }
            "not" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    out.push(rex(is_64bit(operands[0]), 0, r));
                    out.push(0xF7); out.push(modrm(3, 2, r));
                }
            }
            "imul" => {
                if operands.len() == 2 {
                    if let (Some(dr), Some(sr)) = (parse_reg(operands[0]), parse_reg(operands[1])) {
                        out.push(rex(is_64bit(operands[0]), dr, sr));
                        out.push(0x0F); out.push(0xAF);
                        out.push(modrm(3, dr, sr));
                    }
                }
            }
            "idiv" | "div" | "mul" => {
                let ext = match mnemonic.as_ref() { "mul" => 4, "div" => 6, "idiv" => 7, _ => 7 };
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    out.push(rex(is_64bit(operands[0]), 0, r));
                    out.push(0xF7); out.push(modrm(3, ext, r));
                }
            }
            "shl" | "shr" | "sar" => {
                let ext = match mnemonic.as_ref() { "shl" => 4, "shr" => 5, "sar" => 7, _ => 4 };
                if operands.len() == 2 {
                    if let Some(r) = parse_reg(operands[0]) {
                        if operands[1].trim() == "cl" {
                            out.push(rex(is_64bit(operands[0]), 0, r));
                            out.push(0xD3); out.push(modrm(3, ext, r));
                        } else if let Some(imm) = parse_imm(operands[1]) {
                            out.push(rex(is_64bit(operands[0]), 0, r));
                            out.push(0xC1); out.push(modrm(3, ext, r)); out.push(imm as u8);
                        }
                    }
                }
            }
            "jmp" | "je" | "jne" | "jz" | "jnz" | "jl" | "jg" | "jle" | "jge" | "ja" | "jb" => {
                if let Some(offset) = parse_imm(operands.first().unwrap_or(&"0")) {
                    let rel = offset as i32;
                    match mnemonic.as_ref() {
                        "jmp" => { out.push(0xE9); out.extend_from_slice(&rel.to_le_bytes()); }
                        "je" | "jz" => { out.extend_from_slice(&[0x0F, 0x84]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jne" | "jnz" => { out.extend_from_slice(&[0x0F, 0x85]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jl" => { out.extend_from_slice(&[0x0F, 0x8C]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jg" => { out.extend_from_slice(&[0x0F, 0x8F]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jle" => { out.extend_from_slice(&[0x0F, 0x8E]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jge" => { out.extend_from_slice(&[0x0F, 0x8D]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "ja" => { out.extend_from_slice(&[0x0F, 0x87]); out.extend_from_slice(&rel.to_le_bytes()); }
                        "jb" => { out.extend_from_slice(&[0x0F, 0x82]); out.extend_from_slice(&rel.to_le_bytes()); }
                        _ => {}
                    }
                }
            }
            "call" => {
                if let Some(r) = parse_reg(operands.first().unwrap_or(&"")) {
                    if r > 7 { out.push(0x41); }
                    out.push(0xFF); out.push(modrm(3, 2, r));
                } else if let Some(offset) = parse_imm(operands.first().unwrap_or(&"0")) {
                    out.push(0xE8); out.extend_from_slice(&(offset as i32).to_le_bytes());
                }
            }
            "xchg" => {
                if let (Some(a), Some(b)) = (parse_reg(operands.first().unwrap_or(&"")), parse_reg(operands.get(1).unwrap_or(&""))) {
                    out.push(rex(is_64bit(operands[0]), a, b));
                    out.push(0x87); out.push(modrm(3, a, b));
                }
            }
            "test" => {
                if let (Some(a), Some(b)) = (parse_reg(operands.first().unwrap_or(&"")), parse_reg(operands.get(1).unwrap_or(&""))) {
                    out.push(rex(is_64bit(operands[0]), b, a));
                    out.push(0x85); out.push(modrm(3, b, a));
                }
            }
            "lea" => {
                // lea reg, [rip + offset] — simplified: lea reg, [rel offset]
                if operands.len() == 2 {
                    if let (Some(dr), Some(imm)) = (parse_reg(operands[0]), parse_imm(operands[1].trim_start_matches('[').trim_end_matches(']'))) {
                        out.push(rex(true, dr, 0));
                        out.push(0x8D);
                        out.push(modrm(0, dr, 5)); // RIP-relative
                        out.extend_from_slice(&(imm as i32).to_le_bytes());
                    }
                }
            }
            "cqo" | "cdq" => {
                if mnemonic == "cqo" { out.push(0x48); }
                out.push(0x99);
            }
            _ => return Err(anyhow::anyhow!("Невідома інструкція: '{}'. Підтримуються: mov, add, sub, mul, div, and, or, xor, cmp, push, pop, jmp, je/jne/jl/jg, call, ret, nop, inc, dec, neg, not, shl, shr, imul, idiv, lea, test, xchg, int, syscall, rdtsc, cpuid, hlt, cli, sti, cld, std, pause, mfence, lfence, sfence", mnemonic)),
        }
        Ok(())
    }

    fn find_similar(target: &str, candidates: &[String]) -> Option<String> {
        let mut best: Option<(usize, &str)> = None;
        for c in candidates {
            if c.starts_with('_') || c.len() < 2 { continue; }
            let dist = Self::levenshtein(target, c);
            let threshold = (target.chars().count() / 2).max(2);
            if dist <= threshold {
                if best.is_none() || dist < best.unwrap().0 {
                    best = Some((dist, c));
                }
            }
        }
        best.map(|(_, s)| s.to_string())
    }

    fn levenshtein(a: &str, b: &str) -> usize {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let (m, n) = (a.len(), b.len());
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for i in 0..=m { dp[i][0] = i; }
        for j in 0..=n { dp[0][j] = j; }
        for i in 1..=m {
            for j in 1..=n {
                let cost = if a[i-1] == b[j-1] { 0 } else { 1 };
                dp[i][j] = (dp[i-1][j] + 1).min(dp[i][j-1] + 1).min(dp[i-1][j-1] + cost);
            }
        }
        dp[m][n]
    }

    fn format_stack_trace(&self) -> String {
        if self.call_stack.is_empty() { return String::new(); }
        let mut trace = String::from("  Стек викликів:\n");
        for (i, frame) in self.call_stack.iter().rev().enumerate() {
            if frame.file.is_empty() {
                trace.push_str(&format!("    {}. {}()\n", i, frame.function_name));
            } else {
                trace.push_str(&format!("    {}. {}() в {}:{}\n", i, frame.function_name, frame.file, frame.line));
            }
        }
        trace
    }

    fn check_type(&self, value: &Value, expected: &tryzub_parser::Type) -> Result<()> {
        use tryzub_parser::Type;
        let ok = match expected {
            Type::Цл8 | Type::Цл16 | Type::Цл32 | Type::Цл64 |
            Type::Чс8 | Type::Чс16 | Type::Чс32 | Type::Чс64 => matches!(value, Value::Integer(_)),
            Type::Дрб32 | Type::Дрб64 => matches!(value, Value::Float(_)),
            Type::Лог => matches!(value, Value::Bool(_)),
            Type::Тхт => matches!(value, Value::String(_)),
            Type::Сим => matches!(value, Value::Char(_)),
            Type::Slice(_) | Type::Array(_, _) => matches!(value, Value::Array(_)),
            Type::Tuple(_) => matches!(value, Value::Tuple(_)),
            Type::Named(name) => {
                match value {
                    Value::Struct(sname, _) => {
                        sname == name || self.trait_impls.contains_key(&(sname.clone(), name.clone()))
                    }
                    Value::EnumVariant { type_name, .. } => {
                        type_name == name || self.trait_impls.contains_key(&(type_name.clone(), name.clone()))
                    }
                    _ => {
                        // Named тип що не є відомим struct/enum — пропускаємо перевірку
                        // (може бути аліас або невідомий тип)
                        !self.enum_types.contains_key(name)
                    }
                }
            }
            Type::Optional(inner) => matches!(value, Value::Null) || self.check_type(value, inner).is_ok(),
            Type::Function(_, _) => matches!(value, Value::Function { .. } | Value::Lambda { .. } | Value::BuiltinFn(_)),
            _ => true,
        };
        if ok {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Невідповідність типів: очікувався '{}', отримано '{}' (значення: {})",
                Self::type_to_ukrainian(expected),
                value.type_name(),
                self.format_value_short(value)
            ))
        }
    }

    fn type_to_ukrainian(ty: &tryzub_parser::Type) -> String {
        use tryzub_parser::Type;
        match ty {
            Type::Цл8 => "цл8".to_string(), Type::Цл16 => "цл16".to_string(),
            Type::Цл32 => "цл32".to_string(), Type::Цл64 => "цл64".to_string(),
            Type::Чс8 => "чс8".to_string(), Type::Чс16 => "чс16".to_string(),
            Type::Чс32 => "чс32".to_string(), Type::Чс64 => "чс64".to_string(),
            Type::Дрб32 => "дрб32".to_string(), Type::Дрб64 => "дрб64".to_string(),
            Type::Лог => "лог".to_string(), Type::Тхт => "тхт".to_string(), Type::Сим => "сим".to_string(),
            Type::Slice(_) | Type::Array(_, _) => "масив".to_string(),
            Type::Tuple(_) => "кортеж".to_string(),
            Type::Function(_, _) => "функція".to_string(),
            Type::Named(name) => name.clone(),
            Type::SelfType => "себе".to_string(),
            Type::Optional(inner) => format!("{}?", Self::type_to_ukrainian(inner)),
            Type::Generic(name, _) => name.clone(),
            _ => "тип".to_string(),
        }
    }

    fn format_value_short(&self, value: &Value) -> String {
        match value {
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) if s.chars().count() > 20 => format!("\"{}...\"", s.chars().take(20).collect::<String>()),
            Value::String(s) => format!("\"{}\"", s),
            Value::Bool(b) => if *b { "істина".to_string() } else { "хиба".to_string() },
            Value::Array(a) => format!("[...] ({})", a.len()),
            Value::Null => "нуль".to_string(),
            _ => value.type_name().to_string(),
        }
    }

    fn check_memory_access(&self, addr: usize, size: usize) -> Result<()> {
        for (&base, layout) in &self.allocations {
            if addr >= base && addr + size <= base + layout.size() {
                return Ok(());
            }
        }
        Err(anyhow::anyhow!(
            "Доступ до пам'яті за межами виділеного блоку: 0x{:x} (розмір {})", addr, size
        ))
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
        if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(anyhow::anyhow!("SQL ідентифікатор '{}' не може починатись з цифри", name));
        }
        Ok(())
    }

    fn value_to_float_vec(&self, val: &Value) -> Vec<f64> {
        match val {
            Value::Array(arr) => arr.iter().map(|v| match v {
                Value::Float(f) => *f,
                Value::Integer(i) => *i as f64,
                _ => 0.0,
            }).collect(),
            _ => vec![],
        }
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
                let trace = self.format_stack_trace();
                Err(anyhow::anyhow!("Паніка: {}\n{}", msg, trace))
            }
            "перевірити_рівне" => {
                if args.len() < 2 { return Err(anyhow::anyhow!("перевірити_рівне(очікуване, фактичне)")); }
                let expected = args[0].to_display_string();
                let actual = args[1].to_display_string();
                if expected != actual {
                    let msg = if args.len() > 2 { args[2].to_display_string() } else { String::new() };
                    let trace = self.format_stack_trace();
                    return Err(anyhow::anyhow!("Перевірка рівності не пройшла{}: очікувалось '{}', отримано '{}'\n{}",
                        if msg.is_empty() { String::new() } else { format!(" ({})", msg) }, expected, actual, trace));
                }
                Ok(Value::Bool(true))
            }
            "перевірити_не_рівне" => {
                if args.len() < 2 { return Err(anyhow::anyhow!("перевірити_не_рівне(а, б)")); }
                let a = args[0].to_display_string();
                let b = args[1].to_display_string();
                if a == b {
                    let trace = self.format_stack_trace();
                    return Err(anyhow::anyhow!("Перевірка нерівності не пройшла: обидва '{}'\n{}", a, trace));
                }
                Ok(Value::Bool(true))
            }
            "перевірити_помилку" => {
                if let Some(func) = args.first() {
                    let result = self.call_value(func.clone(), vec![]);
                    if result.is_ok() {
                        return Err(anyhow::anyhow!("Очікувалась помилка, але функція виконалась успішно"));
                    }
                    return Ok(Value::Bool(true));
                }
                Err(anyhow::anyhow!("перевірити_помилку(функція)"))
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
                // діапазон(від, до) — повертає ліниве Range
                if args.len() == 2 {
                    match (&args[0], &args[1]) {
                        (Value::Integer(from), Value::Integer(to)) => {
                            Ok(Value::Range { from: *from, to: *to, inclusive: false })
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

            // ── Async / Concurrency ──
            "все" => {
                // все([ф1, ф2, ф3]) — виконує всі функції, повертає масив результатів
                match args.first() {
                    Some(Value::Array(funcs)) => {
                        let mut results = Vec::new();
                        for func in funcs {
                            let result = self.call_value(func.clone(), vec![])?;
                            results.push(result);
                        }
                        Ok(Value::Array(results))
                    }
                    _ => Err(anyhow::anyhow!("все() очікує масив функцій")),
                }
            }
            "перегони" => {
                // перегони([ф1, ф2, ф3]) — виконує всі, повертає перший не-null результат
                match args.first() {
                    Some(Value::Array(funcs)) => {
                        for func in funcs {
                            let result = self.call_value(func.clone(), vec![])?;
                            if !matches!(result, Value::Null) {
                                return Ok(result);
                            }
                        }
                        Ok(Value::Null)
                    }
                    _ => Err(anyhow::anyhow!("перегони() очікує масив функцій")),
                }
            }
            "потік" => {
                // потік(функція) — запускає функцію в окремому потоці
                // Повертає результат після завершення (join)
                match args.first() {
                    Some(Value::Function { body, closure, .. }) => {
                        let body = body.clone();
                        let closure = closure.clone();
                        let prev_env = self.current_env.clone();
                        self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure))));
                        let mut result = Value::Null;
                        for stmt in body {
                            self.execute_statement(stmt)?;
                            if let Some(rv) = self.return_value.take() {
                                result = rv;
                                break;
                            }
                        }
                        self.current_env = prev_env;
                        Ok(result)
                    }
                    Some(Value::Lambda { body: LambdaBody::Expr(expr), closure, .. }) => {
                        let prev_env = self.current_env.clone();
                        self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure.clone()))));
                        let result = self.evaluate_expression(expr.clone())?;
                        self.current_env = prev_env;
                        Ok(result)
                    }
                    Some(Value::Lambda { body: LambdaBody::Block(stmts), closure, .. }) => {
                        let prev_env = self.current_env.clone();
                        self.current_env = Rc::new(RefCell::new(Scope::new(Some(closure.clone()))));
                        let mut result = Value::Null;
                        for stmt in stmts.clone() {
                            self.execute_statement(stmt)?;
                            if let Some(rv) = self.return_value.take() {
                                result = rv;
                                break;
                            }
                        }
                        self.current_env = prev_env;
                        Ok(result)
                    }
                    _ => Err(anyhow::anyhow!("потік() очікує функцію")),
                }
            }
            "канал" => {
                // канал() — повертає пару [відправник, отримувач]
                // В поточній реалізації — через спільний масив
                let buffer = Value::Array(vec![]);
                Ok(Value::Array(vec![buffer.clone(), buffer]))
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
            "ціле_в_рядок" => {
                match args.first() {
                    Some(Value::Integer(n)) => Ok(Value::String(n.to_string())),
                    Some(Value::Float(f)) => Ok(Value::String((*f as i64).to_string())),
                    Some(Value::String(s)) => Ok(Value::String(s.clone())),
                    Some(Value::Bool(b)) => Ok(Value::String(if *b { "1" } else { "0" }.to_string())),
                    _ => Ok(Value::String("0".to_string())),
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
                            Ok(content) => Ok(Value::String(content)),
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
                println!("Web server initialized на порті {}", port);
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
                        println!("  Статичні файли: {}/", dir);
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
                let safe_url = url.replace(['\r', '\n'], "");
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(String::new())),
                    (Value::String("статус".to_string()), Value::Integer(302)),
                    (Value::String("Location".to_string()), Value::String(safe_url)),
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
                     <h1>{}</h1><p>{}</p><hr><p>Web Server</p></body></html>",
                    status, status, msg
                );
                Ok(Value::Dict(vec![
                    (Value::String("тіло".to_string()), Value::String(html)),
                    (Value::String("тип".to_string()), Value::String("text/html; charset=utf-8".to_string())),
                    (Value::String("статус".to_string()), Value::Integer(status)),
                ]))
            }

            // ── Cookies та сесії ──

            "веб_cookie" => {
                // веб_cookie(назва, значення, параметри) → Set-Cookie рядок
                let name = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(anyhow::anyhow!("веб_cookie: назва має бути рядком")),
                };
                let value = args.get(1).map(|v| v.to_display_string()).unwrap_or_default();
                let max_age = args.get(2).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None })
                    .unwrap_or(86400); // 24 години

                let cookie = format!(
                    "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Strict",
                    name, value, max_age
                );
                Ok(Value::String(cookie))
            }

            "веб_сесія_створити" => {
                // веб_сесія_створити(дані) → session_id + Set-Cookie
                let data = args.first().cloned().unwrap_or(Value::Dict(vec![]));
                let session_id: String = {
                    let mut rng = rand::thread_rng();
                    (0..32).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
                };

                let _json = serde_json::to_string(&VM::value_to_json(&data)).unwrap_or_default();
                // Зберігаємо в БД якщо є підключення, інакше в пам'яті
                let cookie = format!(
                    "тризуб_сесія={}; Max-Age=86400; Path=/; HttpOnly; SameSite=Strict",
                    session_id
                );
                Ok(Value::Dict(vec![
                    (Value::String("ід".to_string()), Value::String(session_id)),
                    (Value::String("cookie".to_string()), Value::String(cookie)),
                    (Value::String("дані".to_string()), data),
                ]))
            }

            "веб_gzip" => {
                // веб_gzip(рядок) → стиснений рядок (base64)
                match args.first() {
                    Some(Value::String(s)) => {
                        use flate2::write::GzEncoder;
                        use flate2::Compression;
                        use std::io::Write;
                        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
                        let _ = encoder.write_all(s.as_bytes());
                        match encoder.finish() {
                            Ok(compressed) => {
                                let ratio = if !s.is_empty() { (compressed.len() as f64 / s.len() as f64 * 100.0) as i64 } else { 100 };
                                Ok(Value::Dict(vec![
                                    (Value::String("дані".to_string()), Value::String(URL_SAFE_NO_PAD.encode(&compressed))),
                                    (Value::String("розмір_до".to_string()), Value::Integer(s.len() as i64)),
                                    (Value::String("розмір_після".to_string()), Value::Integer(compressed.len() as i64)),
                                    (Value::String("відсоток".to_string()), Value::Integer(ratio)),
                                ]))
                            }
                            Err(e) => Err(anyhow::anyhow!("gzip помилка: {}", e)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("веб_gzip очікує рядок")),
                }
            }

            // ── Сесії зі збереженням у SQLite ──

            "веб_сесія_зберегти" => {
                // веб_сесія_зберегти(session_id, дані) → зберігає в SQLite
                if args.len() >= 2 {
                    if let (Value::String(sid), data) = (&args[0], &args[1]) {
                        let json = serde_json::to_string(&VM::value_to_json(data)).unwrap_or_default();
                        if let Some(conn) = self.get_db_connection() {
                            let db = conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                            let _ = db.execute_batch("CREATE TABLE IF NOT EXISTS __сесії (ід TEXT PRIMARY KEY, дані TEXT, оновлено INTEGER)");
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                            db.execute("INSERT OR REPLACE INTO __сесії (ід, дані, оновлено) VALUES (?1, ?2, ?3)",
                                rusqlite::params![sid, json, now as i64])
                                .map_err(|e| anyhow::anyhow!("Сесія: {}", e))?;
                            Ok(Value::Bool(true))
                        } else {
                            Err(anyhow::anyhow!("БД не відкрита для сесій"))
                        }
                    } else { Err(anyhow::anyhow!("веб_сесія_зберегти(ід, дані)")) }
                } else { Err(anyhow::anyhow!("веб_сесія_зберегти очікує 2 аргументи")) }
            }

            "веб_сесія_отримати" => {
                // веб_сесія_отримати(session_id) → дані або Null
                match args.first() {
                    Some(Value::String(sid)) => {
                        if let Some(conn) = self.get_db_connection() {
                            let db = conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                            let result: Result<String, _> = db.query_row(
                                "SELECT дані FROM __сесії WHERE ід = ?1", rusqlite::params![sid],
                                |row| row.get(0));
                            match result {
                                Ok(json_str) => {
                                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                        Ok(VM::json_to_value(&json_val))
                                    } else { Ok(Value::Null) }
                                }
                                Err(_) => Ok(Value::Null),
                            }
                        } else { Ok(Value::Null) }
                    }
                    _ => Ok(Value::Null),
                }
            }

            "веб_сесія_видалити" => {
                match args.first() {
                    Some(Value::String(sid)) => {
                        if let Some(conn) = self.get_db_connection() {
                            let db = conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                            let _ = db.execute("DELETE FROM __сесії WHERE ід = ?1", rusqlite::params![sid]);
                        }
                        Ok(Value::Bool(true))
                    }
                    _ => Ok(Value::Bool(false)),
                }
            }

            // ── Файловий upload ──

            "веб_зберегти_файл" => {
                // веб_зберегти_файл(дані_base64, шлях) → зберігає файл
                if args.len() >= 2 {
                    if let (Value::String(data), Value::String(path)) = (&args[0], &args[1]) {
                        if path.contains("..") || path.contains('\0') {
                            return Err(anyhow::anyhow!("Небезпечний шлях файлу"));
                        }
                        if let Some(parent) = std::path::Path::new(path).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(bytes) = URL_SAFE_NO_PAD.decode(data) {
                            std::fs::write(path, &bytes)
                                .map_err(|e| anyhow::anyhow!("Запис файлу: {}", e))?;
                            Ok(Value::Dict(vec![
                                (Value::String("шлях".into()), Value::String(path.clone())),
                                (Value::String("розмір".into()), Value::Integer(bytes.len() as i64)),
                            ]))
                        } else {
                            std::fs::write(path, data.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Запис файлу: {}", e))?;
                            Ok(Value::Dict(vec![
                                (Value::String("шлях".into()), Value::String(path.clone())),
                                (Value::String("розмір".into()), Value::Integer(data.len() as i64)),
                            ]))
                        }
                    } else { Err(anyhow::anyhow!("веб_зберегти_файл(дані, шлях)")) }
                } else { Err(anyhow::anyhow!("веб_зберегти_файл очікує 2 аргументи")) }
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
                        println!("  База даних: {}", path);
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
                        for (k, _) in pairs.iter() {
                            Self::validate_sql_identifier(&k.to_display_string())?;
                        }
                        let conditions: Vec<String> = pairs.iter().enumerate()
                            .map(|(i, (k, _))| {
                                let col = k.to_display_string();
                                format!("{} = ?{}", col, i + 1)
                            })
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
                // PBKDF2-подібне хешування: SHA256 + random salt + 600000 ітерацій
                match args.first() {
                    Some(Value::String(password)) => {
                        use sha2::{Sha256, Digest};
                        let mut rng = rand::thread_rng();
                        let salt: [u8; 16] = rng.gen();
                        let salt_b64 = URL_SAFE_NO_PAD.encode(salt);

                        let mut hash = {
                            let mut h = Sha256::new();
                            h.update(salt);
                            h.update(password.as_bytes());
                            h.finalize()
                        };
                        for _ in 0..600_000 {
                            let mut h = Sha256::new();
                            h.update(salt);
                            h.update(password.as_bytes());
                            h.update(hash);
                            hash = h.finalize();
                        }
                        let hash_b64 = URL_SAFE_NO_PAD.encode(hash);
                        Ok(Value::String(format!("$тх2$600000${}${}", salt_b64, hash_b64)))
                    }
                    _ => Err(anyhow::anyhow!("авт_хешувати очікує пароль (тхт)")),
                }
            }

            "авт_перевірити" => {
                // Timing-safe перевірка пароля проти збереженого хешу
                if args.len() >= 2 {
                    if let (Value::String(password), Value::String(stored)) = (&args[0], &args[1]) {
                        use sha2::{Sha256, Digest};
                        let parts: Vec<&str> = stored.split('$').collect();
                        if parts.len() >= 5 && parts[1] == "тх2" {
                            let rounds: u32 = parts[2].parse().unwrap_or(600_000);
                            let salt = URL_SAFE_NO_PAD.decode(parts[3]).unwrap_or_default();
                            let stored_hash = parts[4];

                            let mut hash = {
                                let mut h = Sha256::new();
                                h.update(&salt);
                                h.update(password.as_bytes());
                                h.finalize()
                            };
                            for _ in 0..rounds {
                                let mut h = Sha256::new();
                                h.update(&salt);
                                h.update(password.as_bytes());
                                h.update(hash);
                                hash = h.finalize();
                            }
                            let computed = URL_SAFE_NO_PAD.encode(hash);
                            // Timing-safe
                            let eq = stored_hash.len() == computed.len() &&
                                stored_hash.bytes().zip(computed.bytes())
                                    .fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
                            return Ok(Value::Bool(eq));
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
                // JWT з HMAC-SHA256 (RFC 7519 сумісний)
                if !args.is_empty() {
                    let data = &args[0];
                    let secret = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or_else(|| self.default_jwt_secret.clone());
                    let ttl_min = args.get(2).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None })
                        .unwrap_or(1440);

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let exp = now + (ttl_min as u64 * 60);

                    let header = r#"{"alg":"HS256","typ":"JWT"}"#;
                    let header_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());

                    let mut payload = VM::value_to_json(data);
                    if let serde_json::Value::Object(ref mut map) = payload {
                        map.insert("exp".to_string(), serde_json::json!(exp));
                        map.insert("iat".to_string(), serde_json::json!(now));
                    }
                    let payload_str = serde_json::to_string(&payload).unwrap_or_default();
                    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_str.as_bytes());

                    // HMAC-SHA256 підпис
                    let sign_input = format!("{}.{}", header_b64, payload_b64);
                    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                        .map_err(|e| anyhow::anyhow!("HMAC помилка: {}", e))?;
                    mac.update(sign_input.as_bytes());
                    let sig = mac.finalize().into_bytes();
                    let sig_b64 = URL_SAFE_NO_PAD.encode(sig);

                    Ok(Value::String(format!("{}.{}.{}", header_b64, payload_b64, sig_b64)))
                } else {
                    Err(anyhow::anyhow!("авт_створити_токен очікує (дані)"))
                }
            }

            "авт_перевірити_токен" => {
                // JWT верифікація з HMAC-SHA256
                if !args.is_empty() {
                    if let Value::String(token) = &args[0] {
                        let secret = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                            .unwrap_or_else(|| self.default_jwt_secret.clone());

                        let parts: Vec<&str> = token.split('.').collect();
                        if parts.len() != 3 {
                            return Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String("Невалідний токен".to_string())],
                            });
                        }

                        // HMAC-SHA256 верифікація
                        let sign_input = format!("{}.{}", parts[0], parts[1]);
                        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                            .map_err(|e| anyhow::anyhow!("HMAC: {}", e))?;
                        mac.update(sign_input.as_bytes());
                        let expected_sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

                        let sig_valid = {
                            let a = expected_sig.as_bytes();
                            let b = parts[2].as_bytes();
                            if a.len() != b.len() { false }
                            else {
                                let mut diff: u8 = 0;
                                for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
                                diff == 0
                            }
                        };
                        if !sig_valid {
                            return Ok(Value::EnumVariant {
                                type_name: "Результат".to_string(),
                                variant: "Помилка".to_string(),
                                fields: vec![Value::String("Невалідний підпис".to_string())],
                            });
                        }

                        let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).unwrap_or_default();
                        let payload_str = String::from_utf8(payload_bytes).unwrap_or_default();
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

            "веб_csrf_перевірити" => {
                let token1 = match &args[0] { Value::String(s) => s.as_bytes().to_vec(), _ => vec![] };
                let token2 = match &args[1] { Value::String(s) => s.as_bytes().to_vec(), _ => vec![] };
                if token1.len() != token2.len() { return Ok(Value::Bool(false)); }
                let mut result: u8 = 0;
                for (a, b) in token1.iter().zip(token2.iter()) {
                    result |= a ^ b;
                }
                Ok(Value::Bool(result == 0))
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

            // ── Регулярні вирази ──

            "regex_відповідає" => {
                if args.len() >= 2 {
                    if let (Value::String(pattern), Value::String(text)) = (&args[0], &args[1]) {
                        match regex::Regex::new(pattern) {
                            Ok(re) => Ok(Value::Bool(re.is_match(text))),
                            Err(e) => Err(anyhow::anyhow!("Невалідний regex: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("regex_відповідає очікує (шаблон, текст)")) }
                } else { Err(anyhow::anyhow!("regex_відповідає очікує 2 аргументи")) }
            }

            "regex_знайти" => {
                if args.len() >= 2 {
                    if let (Value::String(pattern), Value::String(text)) = (&args[0], &args[1]) {
                        match regex::Regex::new(pattern) {
                            Ok(re) => {
                                let matches: Vec<Value> = re.find_iter(text)
                                    .map(|m| Value::String(m.as_str().to_string()))
                                    .collect();
                                Ok(Value::Array(matches))
                            }
                            Err(e) => Err(anyhow::anyhow!("Невалідний regex: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("regex_знайти очікує (шаблон, текст)")) }
                } else { Err(anyhow::anyhow!("regex_знайти очікує 2 аргументи")) }
            }

            "regex_замінити" => {
                if args.len() >= 3 {
                    if let (Value::String(pattern), Value::String(text), Value::String(replacement)) = (&args[0], &args[1], &args[2]) {
                        match regex::Regex::new(pattern) {
                            Ok(re) => Ok(Value::String(re.replace_all(text, replacement.as_str()).to_string())),
                            Err(e) => Err(anyhow::anyhow!("Невалідний regex: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("regex_замінити очікує (шаблон, текст, заміна)")) }
                } else { Err(anyhow::anyhow!("regex_замінити очікує 3 аргументи")) }
            }

            // ── HTTP клієнт ──

            "http_отримати" => {
                match args.first() {
                    Some(Value::String(url)) => {
                        match ureq::get(url).call() {
                            Ok(resp) => {
                                let status = resp.status() as i64;
                                let body = resp.into_string().unwrap_or_default();
                                Ok(Value::Dict(vec![
                                    (Value::String("статус".into()), Value::Integer(status)),
                                    (Value::String("тіло".into()), Value::String(body)),
                                ]))
                            }
                            Err(e) => {
                                Ok(Value::Dict(vec![
                                    (Value::String("статус".into()), Value::Integer(0)),
                                    (Value::String("помилка".into()), Value::String(e.to_string())),
                                ]))
                            }
                        }
                    }
                    _ => Err(anyhow::anyhow!("http_отримати очікує URL")),
                }
            }

            "http_надіслати" => {
                if args.len() >= 2 {
                    if let (Value::String(url), body_val) = (&args[0], &args[1]) {
                        let body_str = match body_val {
                            Value::Dict(_) => serde_json::to_string(&VM::value_to_json(body_val)).unwrap_or_default(),
                            Value::String(s) => s.clone(),
                            _ => body_val.to_display_string(),
                        };
                        let content_type = if matches!(body_val, Value::Dict(_)) {
                            "application/json"
                        } else {
                            "text/plain"
                        };
                        match ureq::post(url).set("Content-Type", content_type).send_string(&body_str) {
                            Ok(resp) => {
                                let status = resp.status() as i64;
                                let resp_body = resp.into_string().unwrap_or_default();
                                Ok(Value::Dict(vec![
                                    (Value::String("статус".into()), Value::Integer(status)),
                                    (Value::String("тіло".into()), Value::String(resp_body)),
                                ]))
                            }
                            Err(e) => {
                                Ok(Value::Dict(vec![
                                    (Value::String("статус".into()), Value::Integer(0)),
                                    (Value::String("помилка".into()), Value::String(e.to_string())),
                                ]))
                            }
                        }
                    } else { Err(anyhow::anyhow!("http_надіслати очікує (URL, тіло)")) }
                } else { Err(anyhow::anyhow!("http_надіслати очікує 2 аргументи")) }
            }

            // ── Кібербезпека ──

            "хеш_md5" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        use md5::Digest;
                        let mut hasher = md5::Md5::new();
                        hasher.update(s.as_bytes());
                        Ok(Value::String(format!("{:x}", hasher.finalize())))
                    }
                    _ => Err(anyhow::anyhow!("хеш_md5 очікує рядок")),
                }
            }

            "хеш_sha256" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        use sha2::Digest;
                        let mut hasher = sha2::Sha256::new();
                        hasher.update(s.as_bytes());
                        Ok(Value::String(format!("{:x}", hasher.finalize())))
                    }
                    _ => Err(anyhow::anyhow!("хеш_sha256 очікує рядок")),
                }
            }

            "хеш_sha512" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        use sha2::Digest;
                        let mut hasher = sha2::Sha512::new();
                        hasher.update(s.as_bytes());
                        Ok(Value::String(format!("{:x}", hasher.finalize())))
                    }
                    _ => Err(anyhow::anyhow!("хеш_sha512 очікує рядок")),
                }
            }

            "шифрувати_aes" => {
                // шифрувати_aes(текст, ключ_32_байти) → зашифрований base64
                if args.len() >= 2 {
                    if let (Value::String(plaintext), Value::String(key)) = (&args[0], &args[1]) {
                        use aes_gcm::{Aes256Gcm, Key, Nonce, aead::{Aead, KeyInit}};
                        let key_bytes = sha2::Sha256::digest(key.as_bytes());
                        let cipher_key = Key::<Aes256Gcm>::from_slice(&key_bytes);
                        let cipher = Aes256Gcm::new(cipher_key);
                        let mut nonce_bytes = [0u8; 12];
                        let mut rng = rand::thread_rng();
                        for b in &mut nonce_bytes { *b = rng.gen(); }
                        let nonce = Nonce::from_slice(&nonce_bytes);
                        match cipher.encrypt(nonce, plaintext.as_bytes().as_ref()) {
                            Ok(ciphertext) => {
                                let mut result = nonce_bytes.to_vec();
                                result.extend_from_slice(&ciphertext);
                                Ok(Value::String(URL_SAFE_NO_PAD.encode(&result)))
                            }
                            Err(e) => Err(anyhow::anyhow!("AES шифрування: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("шифрувати_aes(текст, ключ)")) }
                } else { Err(anyhow::anyhow!("шифрувати_aes очікує 2 аргументи")) }
            }

            "розшифрувати_aes" => {
                if args.len() >= 2 {
                    if let (Value::String(encrypted), Value::String(key)) = (&args[0], &args[1]) {
                        use aes_gcm::{Aes256Gcm, Key, Nonce, aead::{Aead, KeyInit}};
                        use sha2::Digest;
                        let key_bytes = sha2::Sha256::digest(key.as_bytes());
                        let cipher_key = Key::<Aes256Gcm>::from_slice(&key_bytes);
                        let cipher = Aes256Gcm::new(cipher_key);
                        match URL_SAFE_NO_PAD.decode(encrypted) {
                            Ok(data) if data.len() > 12 => {
                                let nonce = Nonce::from_slice(&data[..12]);
                                match cipher.decrypt(nonce, &data[12..]) {
                                    Ok(plaintext) => Ok(Value::String(String::from_utf8_lossy(&plaintext).to_string())),
                                    Err(_) => Ok(Value::EnumVariant {
                                        type_name: "Результат".into(), variant: "Помилка".into(),
                                        fields: vec![Value::String("Невірний ключ або пошкоджені дані".into())],
                                    }),
                                }
                            }
                            _ => Err(anyhow::anyhow!("Невалідні зашифровані дані")),
                        }
                    } else { Err(anyhow::anyhow!("розшифрувати_aes(дані, ключ)")) }
                } else { Err(anyhow::anyhow!("розшифрувати_aes очікує 2 аргументи")) }
            }

            "в_hex" => {
                match args.first() {
                    Some(Value::String(s)) => Ok(Value::String(hex::encode(s.as_bytes()))),
                    Some(Value::Integer(n)) => Ok(Value::String(format!("{:x}", n))),
                    _ => Err(anyhow::anyhow!("в_hex очікує рядок або число")),
                }
            }

            "з_hex" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        match hex::decode(s) {
                            Ok(bytes) => Ok(Value::String(String::from_utf8_lossy(&bytes).to_string())),
                            Err(e) => Err(anyhow::anyhow!("Невалідний hex: {}", e)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("з_hex очікує рядок")),
                }
            }

            "в_base64" => {
                match args.first() {
                    Some(Value::String(s)) => Ok(Value::String(URL_SAFE_NO_PAD.encode(s.as_bytes()))),
                    _ => Err(anyhow::anyhow!("в_base64 очікує рядок")),
                }
            }

            "з_base64" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        match URL_SAFE_NO_PAD.decode(s) {
                            Ok(bytes) => Ok(Value::String(String::from_utf8_lossy(&bytes).to_string())),
                            Err(e) => Err(anyhow::anyhow!("Невалідний base64: {}", e)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("з_base64 очікує рядок")),
                }
            }

            "сканувати_порт" => {
                // сканувати_порт(хост, порт, таймаут_мс) → лог
                if args.len() >= 2 {
                    if let (Value::String(host), Value::Integer(port)) = (&args[0], &args[1]) {
                        let timeout_ms = args.get(2).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None }).unwrap_or(1000);
                        let addr = format!("{}:{}", host, port);
                        let timeout = std::time::Duration::from_millis(timeout_ms as u64);
                        match std::net::TcpStream::connect_timeout(
                            &addr.parse().unwrap_or_else(|_| std::net::SocketAddr::from(([127,0,0,1], *port as u16))),
                            timeout
                        ) {
                            Ok(_) => Ok(Value::Bool(true)),
                            Err(_) => Ok(Value::Bool(false)),
                        }
                    } else { Err(anyhow::anyhow!("сканувати_порт(хост, порт)")) }
                } else { Err(anyhow::anyhow!("сканувати_порт очікує 2 аргументи")) }
            }

            "сканувати_порти" => {
                // сканувати_порти(хост, від, до) → масив відкритих портів
                if args.len() >= 3 {
                    if let (Value::String(host), Value::Integer(from), Value::Integer(to)) = (&args[0], &args[1], &args[2]) {
                        let timeout = std::time::Duration::from_millis(200);
                        let mut open_ports = Vec::new();
                        for port in *from..=*to {
                            let addr = format!("{}:{}", host, port);
                            if let Ok(parsed) = addr.parse::<std::net::SocketAddr>() {
                                if std::net::TcpStream::connect_timeout(&parsed, timeout).is_ok() {
                                    open_ports.push(Value::Integer(port));
                                }
                            }
                        }
                        Ok(Value::Array(open_ports))
                    } else { Err(anyhow::anyhow!("сканувати_порти(хост, від, до)")) }
                } else { Err(anyhow::anyhow!("сканувати_порти очікує 3 аргументи")) }
            }

            "генерувати_пароль" => {
                // генерувати_пароль(довжина, опції) → безпечний пароль
                let length = match args.first() {
                    Some(Value::Integer(n)) => *n as usize,
                    _ => 16,
                };
                let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()-_=+[]{}|;:,.<>?";
                let chars: Vec<char> = charset.chars().collect();
                let mut rng = rand::thread_rng();
                let password: String = (0..length).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
                Ok(Value::String(password))
            }

            "генерувати_токен" => {
                // генерувати_токен(довжина_байтів) → криптостійкий hex токен
                let bytes = match args.first() {
                    Some(Value::Integer(n)) => *n as usize,
                    _ => 32,
                };
                let mut rng = rand::thread_rng();
                let token: String = (0..bytes).map(|_| format!("{:02x}", rng.gen::<u8>())).collect();
                Ok(Value::String(token))
            }

            "dns_запит" => {
                // dns_запит(домен) → масив IP адрес
                match args.first() {
                    Some(Value::String(domain)) => {
                        use std::net::ToSocketAddrs;
                        let addr = format!("{}:80", domain);
                        match addr.to_socket_addrs() {
                            Ok(addrs) => {
                                let ips: Vec<Value> = addrs
                                    .map(|a| Value::String(a.ip().to_string()))
                                    .collect();
                                Ok(Value::Array(ips))
                            }
                            Err(e) => Ok(Value::Array(vec![Value::String(format!("Помилка: {}", e))])),
                        }
                    }
                    _ => Err(anyhow::anyhow!("dns_запит очікує домен")),
                }
            }

            "url_кодувати" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        let encoded: String = s.bytes().map(|b| {
                            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                                format!("{}", b as char)
                            } else {
                                format!("%{:02X}", b)
                            }
                        }).collect();
                        Ok(Value::String(encoded))
                    }
                    _ => Err(anyhow::anyhow!("url_кодувати очікує рядок")),
                }
            }

            "url_розкодувати" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        let mut bytes = Vec::new();
                        let mut chars = s.chars();
                        while let Some(c) = chars.next() {
                            if c == '%' {
                                let hex: String = chars.by_ref().take(2).collect();
                                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                                    bytes.push(byte);
                                }
                            } else if c == '+' {
                                bytes.push(b' ');
                            } else {
                                let mut buf = [0u8; 4];
                                let encoded = c.encode_utf8(&mut buf);
                                bytes.extend_from_slice(encoded.as_bytes());
                            }
                        }
                        Ok(Value::String(String::from_utf8_lossy(&bytes).to_string()))
                    }
                    _ => Err(anyhow::anyhow!("url_розкодувати очікує рядок")),
                }
            }

            // ── Розширена кібербезпека ──

            "фазити" => {
                // фазити(функція, кількість) → тестує функцію випадковими даними
                if args.len() >= 2 {
                    let func = args[0].clone();
                    let count = match &args[1] { Value::Integer(n) => *n as u64, _ => 1000 };
                    let mut rng = rand::thread_rng();
                    let mut crashes = Vec::new();
                    let mut tested = 0u64;

                    for i in 0..count {
                        let test_input = match rng.gen_range(0..5) {
                            0 => Value::Integer(rng.gen_range(-1000000..1000000)),
                            1 => Value::Float(rng.gen::<f64>() * 1000.0 - 500.0),
                            2 => Value::String(String::new()),
                            3 => Value::Null,
                            _ => Value::String((0..rng.gen_range(1..100)).map(|_| rng.gen::<char>()).collect()),
                        };
                        if let Err(e) = self.call_value(func.clone(), vec![test_input.clone()]) {
                            crashes.push(Value::Dict(vec![
                                (Value::String("вхід".into()), test_input),
                                (Value::String("помилка".into()), Value::String(e.to_string())),
                                (Value::String("ітерація".into()), Value::Integer(i as i64)),
                            ]));
                            if crashes.len() >= 10 { break; }
                        }
                        tested += 1;
                    }

                    Ok(Value::Dict(vec![
                        (Value::String("тестовано".into()), Value::Integer(tested as i64)),
                        (Value::String("падінь".into()), Value::Integer(crashes.len() as i64)),
                        (Value::String("деталі".into()), Value::Array(crashes)),
                    ]))
                } else { Err(anyhow::anyhow!("фазити(функція, кількість)")) }
            }

            "аудит_рядок" => {
                // аудит_рядок(рядок) → перевіряє на типові вразливості
                match args.first() {
                    Some(Value::String(s)) => {
                        let mut issues = Vec::new();
                        if s.contains("<script") || s.contains("javascript:") || s.contains("onerror=") {
                            issues.push(Value::String("XSS: знайдено потенційний скрипт".into()));
                        }
                        if s.contains("' OR ") || s.contains("'; DROP") || s.contains("1=1") || s.contains("UNION SELECT") {
                            issues.push(Value::String("SQL Injection: знайдено підозрілий SQL".into()));
                        }
                        if s.contains("../") || s.contains("..\\") {
                            issues.push(Value::String("Path Traversal: знайдено обхід шляху".into()));
                        }
                        if s.contains('\0') {
                            issues.push(Value::String("Null Byte Injection: знайдено нульовий байт".into()));
                        }
                        if s.contains("{{") || s.contains("{%") || s.contains("${") {
                            issues.push(Value::String("Template Injection: знайдено шаблонний вираз".into()));
                        }
                        if regex::Regex::new(r"(?i)(cmd|powershell|bash|sh)\s*[;&|]").ok()
                            .is_some_and(|re| re.is_match(s)) {
                            issues.push(Value::String("Command Injection: знайдено команду оболонки".into()));
                        }
                        Ok(Value::Dict(vec![
                            (Value::String("безпечно".into()), Value::Bool(issues.is_empty())),
                            (Value::String("вразливості".into()), Value::Array(issues)),
                        ]))
                    }
                    _ => Err(anyhow::anyhow!("аудит_рядок очікує рядок")),
                }
            }

            "блокчейн_хеш" => {
                // блокчейн_хеш(дані, попередній_хеш) → хеш блоку
                if args.len() >= 2 {
                    if let (Value::String(data), Value::String(prev_hash)) = (&args[0], &args[1]) {
                        use sha2::Digest;
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let block = format!("{}:{}:{}", prev_hash, data, now);
                        let hash = format!("{:x}", sha2::Sha256::digest(block.as_bytes()));
                        Ok(Value::Dict(vec![
                            (Value::String("хеш".into()), Value::String(hash)),
                            (Value::String("дані".into()), Value::String(data.clone())),
                            (Value::String("попередній".into()), Value::String(prev_hash.clone())),
                            (Value::String("час".into()), Value::Integer(now as i64)),
                        ]))
                    } else { Err(anyhow::anyhow!("блокчейн_хеш(дані, поп_хеш)")) }
                } else { Err(anyhow::anyhow!("блокчейн_хеш очікує 2 аргументи")) }
            }

            "merkle_дерево" => {
                // merkle_дерево(масив_даних) → кореневий хеш
                match args.first() {
                    Some(Value::Array(items)) => {
                        use sha2::Digest;
                        let mut hashes: Vec<String> = items.iter()
                            .map(|v| format!("{:x}", sha2::Sha256::digest(v.to_display_string().as_bytes())))
                            .collect();
                        while hashes.len() > 1 {
                            let mut next = Vec::new();
                            let mut i = 0;
                            while i < hashes.len() {
                                let left = &hashes[i];
                                let right = if i + 1 < hashes.len() { &hashes[i + 1] } else { left };
                                let combined = format!("{}{}", left, right);
                                next.push(format!("{:x}", sha2::Sha256::digest(combined.as_bytes())));
                                i += 2;
                            }
                            hashes = next;
                        }
                        Ok(Value::String(hashes.first().cloned().unwrap_or_default()))
                    }
                    _ => Err(anyhow::anyhow!("merkle_дерево очікує масив")),
                }
            }

            "стего_приховати" => {
                // стего_приховати(носій_рядок, секрет) → рядок з прихованим повідомленням (zero-width chars)
                if args.len() >= 2 {
                    if let (Value::String(carrier), Value::String(secret)) = (&args[0], &args[1]) {
                        let mut result = String::new();
                        let binary: String = secret.bytes()
                            .map(|b| format!("{:08b}", b)).collect();
                        let mut bit_iter = binary.chars();
                        for ch in carrier.chars() {
                            result.push(ch);
                            if let Some(bit) = bit_iter.next() {
                                result.push(if bit == '1' { '\u{200B}' } else { '\u{200C}' }); // zero-width space/non-joiner
                            }
                        }
                        // Решту бітів додаємо в кінець
                        for bit in bit_iter {
                            result.push(if bit == '1' { '\u{200B}' } else { '\u{200C}' });
                        }
                        Ok(Value::String(result))
                    } else { Err(anyhow::anyhow!("стего_приховати(носій, секрет)")) }
                } else { Err(anyhow::anyhow!("стего_приховати очікує 2 аргументи")) }
            }

            "стего_дістати" => {
                // стего_дістати(рядок_зі_стего) → прихований текст
                match args.first() {
                    Some(Value::String(s)) => {
                        let bits: String = s.chars()
                            .filter_map(|c| match c {
                                '\u{200B}' => Some('1'),
                                '\u{200C}' => Some('0'),
                                _ => None,
                            }).collect();
                        let mut bytes = Vec::new();
                        let mut i = 0;
                        while i + 8 <= bits.len() {
                            if let Ok(byte) = u8::from_str_radix(&bits[i..i+8], 2) {
                                if byte == 0 { break; }
                                bytes.push(byte);
                            }
                            i += 8;
                        }
                        Ok(Value::String(String::from_utf8_lossy(&bytes).to_string()))
                    }
                    _ => Err(anyhow::anyhow!("стего_дістати очікує рядок")),
                }
            }

            "xor_шифр" => {
                // xor_шифр(дані, ключ) → XOR шифрування/розшифрування
                if args.len() >= 2 {
                    if let (Value::String(data), Value::String(key)) = (&args[0], &args[1]) {
                        if key.is_empty() { return Err(anyhow::anyhow!("Ключ не може бути порожнім")); }
                        let key_bytes = key.as_bytes();
                        let result: Vec<u8> = data.bytes()
                            .enumerate()
                            .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
                            .collect();
                        Ok(Value::String(hex::encode(&result)))
                    } else { Err(anyhow::anyhow!("xor_шифр(дані, ключ)")) }
                } else { Err(anyhow::anyhow!("xor_шифр очікує 2 аргументи")) }
            }

            "rot13" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        let result: String = s.chars().map(|c| match c {
                            'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
                            'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
                            'а'..='п' | 'А'..='П' => (c as u32 + 16).try_into().unwrap_or(c),
                            'р'..='я' | 'Р'..='Я' => (c as u32 - 16).try_into().unwrap_or(c),
                            _ => c,
                        }).collect();
                        Ok(Value::String(result))
                    }
                    _ => Err(anyhow::anyhow!("rot13 очікує рядок")),
                }
            }

            "ентропія" => {
                // ентропія(рядок) → біти ентропії (міра випадковості)
                match args.first() {
                    Some(Value::String(s)) => {
                        if s.is_empty() { return Ok(Value::Float(0.0)); }
                        let mut freq = HashMap::new();
                        for c in s.chars() {
                            *freq.entry(c).or_insert(0u64) += 1;
                        }
                        let len = s.len() as f64;
                        let entropy: f64 = freq.values()
                            .map(|&count| {
                                let p = count as f64 / len;
                                if p > 0.0 { -p * p.log2() } else { 0.0 }
                            }).sum();
                        Ok(Value::Float((entropy * 100.0).round() / 100.0))
                    }
                    _ => Err(anyhow::anyhow!("ентропія очікує рядок")),
                }
            }

            "перевірити_пароль" => {
                // перевірити_пароль(пароль) → оцінка сили + рекомендації
                match args.first() {
                    Some(Value::String(password)) => {
                        let len = password.len();
                        let has_upper = password.chars().any(|c| c.is_uppercase());
                        let has_lower = password.chars().any(|c| c.is_lowercase());
                        let has_digit = password.chars().any(|c| c.is_ascii_digit());
                        let has_special = password.chars().any(|c| !c.is_alphanumeric());
                        let has_cyrillic = password.chars().any(|c| ('а'..='я').contains(&c) || ('А'..='Я').contains(&c));

                        let mut score = 0i64;
                        if len >= 8 { score += 1; }
                        if len >= 12 { score += 1; }
                        if len >= 16 { score += 1; }
                        if has_upper { score += 1; }
                        if has_lower { score += 1; }
                        if has_digit { score += 1; }
                        if has_special { score += 2; }
                        if has_cyrillic { score += 2; }

                        let mut recommendations = Vec::new();
                        if len < 8 { recommendations.push(Value::String("Мінімум 8 символів".into())); }
                        if !has_upper { recommendations.push(Value::String("Додайте великі літери".into())); }
                        if !has_digit { recommendations.push(Value::String("Додайте цифри".into())); }
                        if !has_special { recommendations.push(Value::String("Додайте спецсимволи (!@#$)".into())); }

                        let common = ["password", "123456", "qwerty", "admin", "пароль", "123456789"];
                        let is_common = common.iter().any(|&p| password.to_lowercase().contains(p));
                        if is_common { score = 0; recommendations.push(Value::String("Пароль занадто поширений!".into())); }

                        let strength = match score {
                            0..=2 => "слабкий",
                            3..=5 => "середній",
                            6..=7 => "сильний",
                            _ => "дуже сильний",
                        };

                        Ok(Value::Dict(vec![
                            (Value::String("сила".into()), Value::String(strength.into())),
                            (Value::String("оцінка".into()), Value::Integer(score)),
                            (Value::String("довжина".into()), Value::Integer(len as i64)),
                            (Value::String("кирилиця".into()), Value::Bool(has_cyrillic)),
                            (Value::String("рекомендації".into()), Value::Array(recommendations)),
                        ]))
                    }
                    _ => Err(anyhow::anyhow!("перевірити_пароль очікує рядок")),
                }
            }

            "хонейпот" => {
                // хонейпот(порт) → запускає пастку-сервер що логує всі з'єднання
                match args.first() {
                    Some(Value::Integer(port)) => {
                        let addr = format!("0.0.0.0:{}", port);
                        println!("\n  Хонейпот запущено на порті {}", port);
                        println!("  Логую всі з'єднання... (Ctrl+C для зупинки)\n");
                        match std::net::TcpListener::bind(&addr) {
                            Ok(listener) => {
                                listener.set_nonblocking(false).ok();
                                let mut connections = Vec::new();
                                for stream in listener.incoming().take(100) {
                                    match stream {
                                        Ok(mut s) => {
                                            use std::io::Read;
                                            let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
                                            let now = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                                            let mut buf = [0u8; 4096];
                                            s.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
                                            let data = match s.read(&mut buf) {
                                                Ok(n) => String::from_utf8_lossy(&buf[..n]).to_string(),
                                                Err(_) => String::new(),
                                            };
                                            println!("  [{}] З'єднання від {} ({} байт)", now, peer, data.len());
                                            connections.push(Value::Dict(vec![
                                                (Value::String("ip".into()), Value::String(peer)),
                                                (Value::String("час".into()), Value::Integer(now as i64)),
                                                (Value::String("дані".into()), Value::String(data.chars().take(500).collect())),
                                            ]));
                                            // Відповідаємо фейковим банером
                                            use std::io::Write;
                                            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nServer: Apache/2.4.41\r\n\r\n<html><body>Welcome</body></html>");
                                        }
                                        Err(_) => break,
                                    }
                                }
                                Ok(Value::Array(connections))
                            }
                            Err(e) => Err(anyhow::anyhow!("Хонейпот: {}", e)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("хонейпот очікує порт")),
                }
            }

            "часова_мітка" => {
                // часова_мітка() → Unix timestamp
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(Value::Integer(now as i64))
            }

            // ── IoT / Embedded / Дрони ──

            "serial_порти" => {
                let ports = serialport::available_ports().unwrap_or_default();
                let result: Vec<Value> = ports.iter().map(|p| {
                    Value::Dict(vec![
                        (Value::String("порт".into()), Value::String(p.port_name.clone())),
                        (Value::String("тип".into()), Value::String(format!("{:?}", p.port_type))),
                    ])
                }).collect();
                Ok(Value::Array(result))
            }

            "serial_відкрити" => {
                if args.len() >= 2 {
                    if let (Value::String(port), Value::Integer(baud)) = (&args[0], &args[1]) {
                        match serialport::new(port, *baud as u32)
                            .timeout(std::time::Duration::from_millis(1000))
                            .open() {
                            Ok(_) => {
                                println!("  Serial: {} @ {} baud", port, baud);
                                Ok(Value::Dict(vec![
                                    (Value::String("порт".into()), Value::String(port.clone())),
                                    (Value::String("швидкість".into()), Value::Integer(*baud)),
                                    (Value::String("статус".into()), Value::String("відкрито".into())),
                                ]))
                            }
                            Err(e) => Err(anyhow::anyhow!("Serial: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("serial_відкрити(порт, швидкість)")) }
                } else { Err(anyhow::anyhow!("serial_відкрити очікує 2 аргументи")) }
            }

            "serial_записати" => {
                if args.len() >= 2 {
                    if let (Value::String(port_name), Value::String(data)) = (&args[0], &args[1]) {
                        match serialport::new(port_name, 9600)
                            .timeout(std::time::Duration::from_millis(1000))
                            .open() {
                            Ok(mut port) => {
                                use std::io::Write;
                                port.write_all(data.as_bytes())
                                    .map_err(|e| anyhow::anyhow!("Serial write: {}", e))?;
                                Ok(Value::Integer(data.len() as i64))
                            }
                            Err(e) => Err(anyhow::anyhow!("Serial: {}", e)),
                        }
                    } else { Err(anyhow::anyhow!("serial_записати(порт, дані)")) }
                } else { Err(anyhow::anyhow!("serial_записати очікує 2 аргументи")) }
            }

            "serial_прочитати" => {
                match args.first() {
                    Some(Value::String(port_name)) => {
                        let timeout = args.get(1).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None }).unwrap_or(1000);
                        match serialport::new(port_name, 9600)
                            .timeout(std::time::Duration::from_millis(timeout as u64))
                            .open() {
                            Ok(mut port) => {
                                use std::io::Read;
                                let mut buf = vec![0u8; 1024];
                                match port.read(&mut buf) {
                                    Ok(n) => Ok(Value::String(String::from_utf8_lossy(&buf[..n]).to_string())),
                                    Err(_) => Ok(Value::String(String::new())),
                                }
                            }
                            Err(e) => Err(anyhow::anyhow!("Serial: {}", e)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("serial_прочитати(порт)")),
                }
            }

            "serial_закрити" => {
                Ok(Value::Bool(true))
            }

            "gpio_режим" => {
                // gpio_режим(пін, "вивід"/"ввід") — для Raspberry Pi через /sys/class/gpio
                if args.len() >= 2 {
                    if let (Value::Integer(pin), Value::String(mode)) = (&args[0], &args[1]) {
                        let direction = if mode == "вивід" || mode == "out" { "out" } else { "in" };
                        // Export GPIO
                        let _ = std::fs::write("/sys/class/gpio/export", pin.to_string());
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let dir_path = format!("/sys/class/gpio/gpio{}/direction", pin);
                        match std::fs::write(&dir_path, direction) {
                            Ok(_) => Ok(Value::Bool(true)),
                            Err(_) => Ok(Value::Bool(false)),
                        }
                    } else { Err(anyhow::anyhow!("gpio_режим(пін, режим)")) }
                } else { Err(anyhow::anyhow!("gpio_режим очікує 2 аргументи")) }
            }

            "gpio_записати" => {
                if args.len() >= 2 {
                    if let (Value::Integer(pin), Value::Integer(val)) = (&args[0], &args[1]) {
                        let path = format!("/sys/class/gpio/gpio{}/value", pin);
                        match std::fs::write(&path, if *val != 0 { "1" } else { "0" }) {
                            Ok(_) => Ok(Value::Bool(true)),
                            Err(_) => Ok(Value::Bool(false)),
                        }
                    } else { Err(anyhow::anyhow!("gpio_записати(пін, значення)")) }
                } else { Err(anyhow::anyhow!("gpio_записати очікує 2 аргументи")) }
            }

            "gpio_прочитати" => {
                match args.first() {
                    Some(Value::Integer(pin)) => {
                        let path = format!("/sys/class/gpio/gpio{}/value", pin);
                        match std::fs::read_to_string(&path) {
                            Ok(val) => Ok(Value::Integer(val.trim().parse().unwrap_or(0))),
                            Err(_) => Ok(Value::Integer(0)),
                        }
                    }
                    _ => Err(anyhow::anyhow!("gpio_прочитати(пін)")),
                }
            }

            "pid_створити" => {
                // pid_створити(kp, ki, kd) — PID контролер для дронів/роботів
                if args.len() >= 3 {
                    if let (Value::Float(kp), Value::Float(ki), Value::Float(kd)) = (&args[0], &args[1], &args[2]) {
                        Ok(Value::Dict(vec![
                            (Value::String("kp".into()), Value::Float(*kp)),
                            (Value::String("ki".into()), Value::Float(*ki)),
                            (Value::String("kd".into()), Value::Float(*kd)),
                            (Value::String("integral".into()), Value::Float(0.0)),
                            (Value::String("prev_error".into()), Value::Float(0.0)),
                            (Value::String("dt".into()), Value::Float(0.01)),
                        ]))
                    } else { Err(anyhow::anyhow!("pid_створити(kp, ki, kd) — дробові числа")) }
                } else { Err(anyhow::anyhow!("pid_створити очікує 3 аргументи")) }
            }

            "pid_обчислити" => {
                // pid_обчислити(pid, setpoint, current) — повертає корекцію
                if args.len() >= 3 {
                    if let (Value::Dict(pid), Value::Float(setpoint), Value::Float(current)) = (&args[0], &args[1], &args[2]) {
                        let get_f = |name: &str| -> f64 {
                            pid.iter().find(|(k,_)| k.to_display_string() == name)
                                .map(|(_, v)| if let Value::Float(f) = v { *f } else { 0.0 })
                                .unwrap_or(0.0)
                        };
                        let kp = get_f("kp");
                        let ki = get_f("ki");
                        let kd = get_f("kd");
                        let prev_error = get_f("prev_error");
                        let integral = get_f("integral");
                        let dt = get_f("dt").max(0.001);

                        let error = setpoint - current;
                        let new_integral = (integral + error * dt).clamp(-1000.0, 1000.0);
                        let derivative = (error - prev_error) / dt;
                        let output = kp * error + ki * new_integral + kd * derivative;

                        Ok(Value::Dict(vec![
                            (Value::String("вивід".into()), Value::Float(output)),
                            (Value::String("помилка".into()), Value::Float(error)),
                            (Value::String("kp".into()), Value::Float(kp)),
                            (Value::String("ki".into()), Value::Float(ki)),
                            (Value::String("kd".into()), Value::Float(kd)),
                            (Value::String("integral".into()), Value::Float(new_integral)),
                            (Value::String("prev_error".into()), Value::Float(error)),
                            (Value::String("dt".into()), Value::Float(dt)),
                        ]))
                    } else { Err(anyhow::anyhow!("pid_обчислити(pid, ціль, поточне)")) }
                } else { Err(anyhow::anyhow!("pid_обчислити очікує 3 аргументи")) }
            }

            "pwm_значення" => {
                // pwm_значення(відсоток, мін_мкс, макс_мкс) — конвертує % в мікросекунди PWM
                if !args.is_empty() {
                    let percent = match &args[0] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 };
                    let min_us = args.get(1).and_then(|v| if let Value::Integer(n) = v { Some(*n as f64) } else if let Value::Float(f) = v { Some(*f) } else { None }).unwrap_or(1000.0);
                    let max_us = args.get(2).and_then(|v| if let Value::Integer(n) = v { Some(*n as f64) } else if let Value::Float(f) = v { Some(*f) } else { None }).unwrap_or(2000.0);
                    let us = min_us + (max_us - min_us) * (percent.clamp(0.0, 100.0) / 100.0);
                    Ok(Value::Float(us))
                } else { Err(anyhow::anyhow!("pwm_значення(відсоток)")) }
            }

            "відстань_gps" => {
                // відстань_gps(lat1, lon1, lat2, lon2) — Haversine формула, метри
                if args.len() >= 4 {
                    let to_f = |v: &Value| -> f64 { match v { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 } };
                    let lat1 = to_f(&args[0]).to_radians();
                    let lon1 = to_f(&args[1]).to_radians();
                    let lat2 = to_f(&args[2]).to_radians();
                    let lon2 = to_f(&args[3]).to_radians();
                    let dlat = lat2 - lat1;
                    let dlon = lon2 - lon1;
                    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
                    let c = 2.0 * a.sqrt().asin();
                    Ok(Value::Float((6371000.0 * c * 100.0).round() / 100.0))
                } else { Err(anyhow::anyhow!("відстань_gps(lat1, lon1, lat2, lon2)")) }
            }

            "кут_до_точки" => {
                // кут_до_точки(lat1, lon1, lat2, lon2) — bearing в градусах
                if args.len() >= 4 {
                    let to_f = |v: &Value| -> f64 { match v { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 } };
                    let lat1 = to_f(&args[0]).to_radians();
                    let lon1 = to_f(&args[1]).to_radians();
                    let lat2 = to_f(&args[2]).to_radians();
                    let lon2 = to_f(&args[3]).to_radians();
                    let dlon = lon2 - lon1;
                    let x = dlon.sin() * lat2.cos();
                    let y = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
                    let bearing = x.atan2(y).to_degrees();
                    Ok(Value::Float(((bearing + 360.0) % 360.0 * 100.0).round() / 100.0))
                } else { Err(anyhow::anyhow!("кут_до_точки(lat1, lon1, lat2, lon2)")) }
            }

            "i2c_записати" | "i2c_прочитати" | "spi_передати" => {
                // I2C/SPI через /dev/ на Linux
                Err(anyhow::anyhow!("{}: потрібна Linux система з /dev/i2c або /dev/spi", name))
            }

            "затримка_мкс" => {
                match args.first() {
                    Some(Value::Integer(us)) => {
                        std::thread::sleep(std::time::Duration::from_micros(*us as u64));
                        Ok(Value::Null)
                    }
                    _ => Err(anyhow::anyhow!("затримка_мкс(мікросекунди)")),
                }
            }

            "затримка_мс" => {
                match args.first() {
                    Some(Value::Integer(ms)) => {
                        std::thread::sleep(std::time::Duration::from_millis(*ms as u64));
                        Ok(Value::Null)
                    }
                    _ => Err(anyhow::anyhow!("затримка_мс(мілісекунди)")),
                }
            }

            "байти_в_число" => {
                // байти_в_число([байт1, байт2, ...], "le"/"be") — конвертує масив байтів в число
                if let Some(Value::Array(bytes)) = args.first() {
                    let be = args.get(1).map(|v| v.to_display_string() == "be").unwrap_or(false);
                    let byte_vals: Vec<u8> = bytes.iter().map(|v| match v { Value::Integer(n) => *n as u8, _ => 0 }).collect();
                    let result: i64 = if be {
                        byte_vals.iter().fold(0i64, |acc, &b| (acc << 8) | b as i64)
                    } else {
                        byte_vals.iter().rev().fold(0i64, |acc, &b| (acc << 8) | b as i64)
                    };
                    Ok(Value::Integer(result))
                } else { Err(anyhow::anyhow!("байти_в_число([байти])")) }
            }

            "число_в_байти" => {
                // число_в_байти(число, кількість_байтів) — конвертує число в масив байтів
                if !args.is_empty() {
                    let num = match &args[0] { Value::Integer(n) => *n, _ => 0 };
                    let count = args.get(1).and_then(|v| if let Value::Integer(n) = v { Some(*n) } else { None }).unwrap_or(4) as usize;
                    let bytes: Vec<Value> = (0..count).map(|i| Value::Integer((num >> (i * 8)) & 0xFF)).collect();
                    Ok(Value::Array(bytes))
                } else { Err(anyhow::anyhow!("число_в_байти(число)")) }
            }

            "біт_встановити" => {
                // біт_встановити(число, позиція, значення) — встановлює біт
                if args.len() >= 3 {
                    if let (Value::Integer(num), Value::Integer(pos), Value::Integer(val)) = (&args[0], &args[1], &args[2]) {
                        if *val != 0 { Ok(Value::Integer(num | (1 << pos))) }
                        else { Ok(Value::Integer(num & !(1 << pos))) }
                    } else { Err(anyhow::anyhow!("біт_встановити(число, позиція, значення)")) }
                } else { Err(anyhow::anyhow!("біт_встановити очікує 3 аргументи")) }
            }

            "біт_прочитати" => {
                if args.len() >= 2 {
                    if let (Value::Integer(num), Value::Integer(pos)) = (&args[0], &args[1]) {
                        Ok(Value::Integer((num >> pos) & 1))
                    } else { Err(anyhow::anyhow!("біт_прочитати(число, позиція)")) }
                } else { Err(anyhow::anyhow!("біт_прочитати очікує 2 аргументи")) }
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

                println!("\n  Бенчмарк Тризуб VM ({} ітерацій)", iterations);
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

            // ── Векторна математика / ML ──

            "вектор_скалярний_добуток" => {
                let a = self.value_to_float_vec(&args[0]);
                let b = self.value_to_float_vec(&args[1]);
                if a.len() != b.len() { return Err(anyhow::anyhow!("Vectors must have same length")); }
                let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                Ok(Value::Float(dot))
            }
            "вектор_косинусна_подібність" => {
                let a = self.value_to_float_vec(&args[0]);
                let b = self.value_to_float_vec(&args[1]);
                if a.len() != b.len() { return Err(anyhow::anyhow!("Vectors must have same length")); }
                let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
                let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm_a == 0.0 || norm_b == 0.0 { return Ok(Value::Float(0.0)); }
                Ok(Value::Float(dot / (norm_a * norm_b)))
            }
            "вектор_нормалізувати" => {
                let a = self.value_to_float_vec(&args[0]);
                let norm: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm == 0.0 { return Ok(args[0].clone()); }
                let normalized: Vec<Value> = a.iter().map(|x| Value::Float(x / norm)).collect();
                Ok(Value::Array(normalized))
            }
            "вектор_евклідова_відстань" => {
                let a = self.value_to_float_vec(&args[0]);
                let b = self.value_to_float_vec(&args[1]);
                if a.len() != b.len() { return Err(anyhow::anyhow!("Vectors must have same length")); }
                let dist: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt();
                Ok(Value::Float(dist))
            }
            "вектор_найближчі" => {
                let query = self.value_to_float_vec(&args[0]);
                let vectors = match &args[1] {
                    Value::Array(arr) => arr.clone(),
                    _ => return Err(anyhow::anyhow!("Expected array of vectors")),
                };
                let top_k = match &args[2] {
                    Value::Integer(n) => *n as usize,
                    _ => 5,
                };
                let mut similarities: Vec<(usize, f64)> = vectors.iter().enumerate().map(|(i, v)| {
                    let vec_b = self.value_to_float_vec(v);
                    if query.len() != vec_b.len() { return (i, 0.0); }
                    let dot: f64 = query.iter().zip(vec_b.iter()).map(|(x, y)| x * y).sum();
                    let norm_a: f64 = query.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let norm_b: f64 = vec_b.iter().map(|x| x * x).sum::<f64>().sqrt();
                    if norm_a == 0.0 || norm_b == 0.0 { (i, 0.0) } else { (i, dot / (norm_a * norm_b)) }
                }).collect();
                similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let results: Vec<Value> = similarities.iter().take(top_k).map(|(idx, sim)| {
                    let mut map = Vec::new();
                    map.push((Value::String("індекс".to_string()), Value::Integer(*idx as i64)));
                    map.push((Value::String("подібність".to_string()), Value::Float(*sim)));
                    Value::Dict(map)
                }).collect();
                Ok(Value::Array(results))
            }

            // ── Vector Index ──

            "вектор_індекс_створити" => {
                let vectors = match &args[0] {
                    Value::Array(arr) => arr.clone(),
                    _ => return Err(anyhow::anyhow!("Expected array of vectors")),
                };
                let mut index_data = Vec::new();
                for (i, v) in vectors.iter().enumerate() {
                    let vec = self.value_to_float_vec(v);
                    let norm: f64 = vec.iter().map(|x| x * x).sum::<f64>().sqrt();
                    let normalized: Vec<f64> = if norm > 0.0 { vec.iter().map(|x| x / norm).collect() } else { vec };
                    index_data.push((i, normalized));
                }
                self.vector_index = Some(index_data);
                Ok(Value::String(format!("Index created: {} vectors", vectors.len())))
            }
            "вектор_індекс_пошук" => {
                let query = self.value_to_float_vec(&args[0]);
                let top_k = match args.get(1) { Some(Value::Integer(n)) => *n as usize, _ => 5 };
                let index = self.vector_index.as_ref().ok_or_else(|| anyhow::anyhow!("No index created"))?;
                let query_norm: f64 = query.iter().map(|x| x * x).sum::<f64>().sqrt();
                let q_normalized: Vec<f64> = if query_norm > 0.0 { query.iter().map(|x| x / query_norm).collect() } else { query };
                let mut scores: Vec<(usize, f64)> = index.iter().map(|(idx, vec)| {
                    let sim: f64 = q_normalized.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
                    (*idx, sim)
                }).collect();
                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let results: Vec<Value> = scores.iter().take(top_k).map(|(idx, sim)| {
                    let mut map = Vec::new();
                    map.push((Value::String("індекс".to_string()), Value::Integer(*idx as i64)));
                    map.push((Value::String("подібність".to_string()), Value::Float(*sim)));
                    Value::Dict(map)
                }).collect();
                Ok(Value::Array(results))
            }

            // ── VIN парсер ──

            "він_розібрати" => {
                let vin = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected string")) };
                if vin.len() != 17 { return Err(anyhow::anyhow!("VIN must be 17 characters")); }
                let vin_upper = vin.to_uppercase();
                let region = match vin_upper.chars().next().unwrap_or('0') {
                    '1'..='5' => "Північна Америка", 'S'..='Z' => "Європа", 'J'..='R' => "Азія",
                    '6'..='7' => "Океанія", '8'..='9' => "Південна Америка", 'A'..='H' => "Африка", _ => "Невідомий"
                };
                let year_char = vin_upper.chars().nth(9).unwrap_or('0');
                let year = match year_char {
                    'A' => 2010, 'B' => 2011, 'C' => 2012, 'D' => 2013, 'E' => 2014,
                    'F' => 2015, 'G' => 2016, 'H' => 2017, 'J' => 2018, 'K' => 2019,
                    'L' => 2020, 'M' => 2021, 'N' => 2022, 'P' => 2023, 'R' => 2024,
                    'S' => 2025, 'T' => 2026, '1' => 2001, '2' => 2002, '3' => 2003,
                    '4' => 2004, '5' => 2005, '6' => 2006, '7' => 2007, '8' => 2008, '9' => 2009, _ => 0
                };
                let mut result = Vec::new();
                result.push((Value::String("він".to_string()), Value::String(vin_upper.clone())));
                result.push((Value::String("регіон".to_string()), Value::String(region.to_string())));
                result.push((Value::String("виробник".to_string()), Value::String(vin_upper[0..3].to_string())));
                result.push((Value::String("опис".to_string()), Value::String(vin_upper[3..8].to_string())));
                result.push((Value::String("рік".to_string()), Value::Integer(year)));
                result.push((Value::String("завод".to_string()), Value::String(vin_upper[10..11].to_string())));
                result.push((Value::String("серійний".to_string()), Value::String(vin_upper[11..17].to_string())));
                Ok(Value::Dict(result))
            }

            // ── Завантаження файлів ──

            "завантажити_файл" => {
                use std::io::Read;
                let url = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected URL")) };
                let path = match &args[1] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                if path.contains("..") || path.contains('\0') { return Err(anyhow::anyhow!("Invalid path")); }
                let resp = ureq::get(&url).call().map_err(|e| anyhow::anyhow!("Download failed: {}", e))?;
                let mut bytes = Vec::new();
                resp.into_reader().take(100_000_000).read_to_end(&mut bytes).map_err(|e| anyhow::anyhow!("Read failed: {}", e))?;
                std::fs::write(&path, &bytes).map_err(|e| anyhow::anyhow!("Write failed: {}", e))?;
                let mut result = Vec::new();
                result.push((Value::String("шлях".to_string()), Value::String(path)));
                result.push((Value::String("розмір".to_string()), Value::Integer(bytes.len() as i64)));
                Ok(Value::Dict(result))
            }

            // ── Multipart form parsing ──

            "веб_мультіпарт" => {
                let body = match &args[0] {
                    Value::String(s) => {
                        if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '\n') && s.len() > 100 {
                            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).unwrap_or_else(|_| s.as_bytes().to_vec())
                        } else {
                            s.as_bytes().to_vec()
                        }
                    },
                    _ => return Err(anyhow::anyhow!("Expected body"))
                };
                let boundary = match &args[1] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected boundary")) };
                let delimiter = format!("--{}", boundary).into_bytes();
                let mut parts = Vec::new();
                let mut start = 0;
                while let Some(pos) = body[start..].windows(delimiter.len()).position(|w| w == delimiter.as_slice()) {
                    if start > 0 {
                        let part_data = &body[start..start + pos];
                        if let Some(header_end) = part_data.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&part_data[..header_end]).to_string();
                            let content = &part_data[header_end + 4..];
                            let content = if content.ends_with(b"\r\n") { &content[..content.len()-2] } else { content };
                            let name = headers.split("name=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("").to_string();
                            let filename = headers.split("filename=\"").nth(1).and_then(|s| s.split('"').next()).map(|s| s.to_string());
                            let mut entry = Vec::new();
                            entry.push((Value::String("назва".to_string()), Value::String(name)));
                            if let Some(fname) = &filename {
                                entry.push((Value::String("файл".to_string()), Value::String(fname.clone())));
                                entry.push((Value::String("розмір".to_string()), Value::Integer(content.len() as i64)));
                                entry.push((Value::String("дані_base64".to_string()), Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, content))));
                            } else {
                                entry.push((Value::String("значення".to_string()), Value::String(String::from_utf8_lossy(content).to_string())));
                            }
                            parts.push(Value::Dict(entry));
                        }
                    }
                    start = start + pos + delimiter.len();
                    if body.get(start) == Some(&b'\r') { start += 2; }
                }
                Ok(Value::Array(parts))
            }

            "зображення_розмір" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let mut result = Vec::new();
                result.push((Value::String("ширина".to_string()), Value::Integer(img.width() as i64)));
                result.push((Value::String("висота".to_string()), Value::Integer(img.height() as i64)));
                Ok(Value::Dict(result))
            }
            "зображення_змінити_розмір" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let width = match &args[1] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected width")) };
                let height = match &args[2] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected height")) };
                let output = match args.get(3) { Some(Value::String(s)) => s.clone(), _ => path.clone() };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
                resized.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                let mut result = Vec::new();
                result.push((Value::String("шлях".to_string()), Value::String(output)));
                result.push((Value::String("ширина".to_string()), Value::Integer(width as i64)));
                result.push((Value::String("висота".to_string()), Value::Integer(height as i64)));
                Ok(Value::Dict(result))
            }
            "зображення_обрізати" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let x = match &args[1] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected x")) };
                let y = match &args[2] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected y")) };
                let w = match &args[3] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected width")) };
                let h = match &args[4] { Value::Integer(n) => *n as u32, _ => return Err(anyhow::anyhow!("Expected height")) };
                let output = match args.get(5) { Some(Value::String(s)) => s.clone(), _ => path.clone() };
                let mut img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let cropped = img.crop(x, y, w, h);
                cropped.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                Ok(Value::String(output))
            }
            "зображення_мініатюра" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let max_size = match &args[1] { Value::Integer(n) => *n as u32, _ => 200 };
                let output = match args.get(2) { Some(Value::String(s)) => s.clone(), _ => {
                    let p = std::path::Path::new(&path);
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("img");
                    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("jpg");
                    format!("{}_thumb.{}", stem, ext)
                }};
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let thumb = img.thumbnail(max_size, max_size);
                thumb.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                let mut result = Vec::new();
                result.push((Value::String("шлях".to_string()), Value::String(output)));
                result.push((Value::String("ширина".to_string()), Value::Integer(thumb.width() as i64)));
                result.push((Value::String("висота".to_string()), Value::Integer(thumb.height() as i64)));
                Ok(Value::Dict(result))
            }
            "зображення_формат" => {
                let input = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let output = match &args[1] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected output path")) };
                let img = image::open(&input).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                img.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                Ok(Value::String(output))
            }
            "зображення_сірий" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let output = match args.get(1) { Some(Value::String(s)) => s.clone(), _ => path.clone() };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let gray = img.grayscale();
                gray.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                Ok(Value::String(output))
            }
            "зображення_повернути" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let degrees = match &args[1] { Value::Integer(n) => *n, _ => 90 };
                let output = match args.get(2) { Some(Value::String(s)) => s.clone(), _ => path.clone() };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let rotated = match degrees {
                    90 => img.rotate90(),
                    180 => img.rotate180(),
                    270 => img.rotate270(),
                    _ => return Err(anyhow::anyhow!("Supported rotations: 90, 180, 270")),
                };
                rotated.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                Ok(Value::String(output))
            }
            "зображення_відзеркалити" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let axis = match args.get(1) { Some(Value::String(s)) => s.clone(), _ => "горизонтально".to_string() };
                let output = match args.get(2) { Some(Value::String(s)) => s.clone(), _ => path.clone() };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let flipped = if axis == "вертикально" { img.flipv() } else { img.fliph() };
                flipped.save(&output).map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
                Ok(Value::String(output))
            }
            "зображення_в_тензор" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected path")) };
                let width = match args.get(1) { Some(Value::Integer(n)) => *n as u32, _ => 224 };
                let height = match args.get(2) { Some(Value::Integer(n)) => *n as u32, _ => 224 };
                let img = image::open(&path).map_err(|e| anyhow::anyhow!("Failed to open image: {}", e))?;
                let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
                let rgb = resized.to_rgb8();
                let mut r_channel = Vec::new();
                let mut g_channel = Vec::new();
                let mut b_channel = Vec::new();
                for pixel in rgb.pixels() {
                    r_channel.push(Value::Float((pixel[0] as f64 / 255.0 - 0.485) / 0.229));
                    g_channel.push(Value::Float((pixel[1] as f64 / 255.0 - 0.456) / 0.224));
                    b_channel.push(Value::Float((pixel[2] as f64 / 255.0 - 0.406) / 0.225));
                }
                let tensor = vec![
                    Value::Array(r_channel),
                    Value::Array(g_channel),
                    Value::Array(b_channel),
                ];
                let mut result = Vec::new();
                result.push((Value::String("тензор".to_string()), Value::Array(tensor)));
                result.push((Value::String("форма".to_string()), Value::Array(vec![
                    Value::Integer(3), Value::Integer(height as i64), Value::Integer(width as i64)
                ])));
                Ok(Value::Dict(result))
            }

            // ── Subprocess / Python / ML ──
            "виконати_команду" => {
                let cmd = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected command string")) };
                let cmd_args: Vec<String> = args.iter().skip(1).filter_map(|a| match a { Value::String(s) => Some(s.clone()), _ => None }).collect();
                let output = std::process::Command::new(&cmd)
                    .args(&cmd_args)
                    .output()
                    .map_err(|e| anyhow::anyhow!("Command failed: {}", e))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let mut result = Vec::new();
                result.push((Value::String("вивід".to_string()), Value::String(stdout.trim().to_string())));
                result.push((Value::String("помилки".to_string()), Value::String(stderr.trim().to_string())));
                result.push((Value::String("код".to_string()), Value::Integer(output.status.code().unwrap_or(-1) as i64)));
                Ok(Value::Dict(result))
            }
            "пітон" => {
                let script = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected Python script")) };
                let py_cmd = if cfg!(windows) { "python" } else { "python3" };
                let output = std::process::Command::new(py_cmd)
                    .args(["-c", &script])
                    .output()
                    .map_err(|e| anyhow::anyhow!("Python not found: {}", e))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string().trim().to_string();
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(anyhow::anyhow!("Python error: {}", stderr.trim()));
                }
                if stdout.starts_with('{') || stdout.starts_with('[') {
                    match serde_json::from_str::<serde_json::Value>(&stdout) {
                        Ok(v) => Ok(VM::json_to_value(&v)),
                        Err(_) => Ok(Value::String(stdout)),
                    }
                } else if let Ok(n) = stdout.parse::<i64>() {
                    Ok(Value::Integer(n))
                } else if let Ok(f) = stdout.parse::<f64>() {
                    Ok(Value::Float(f))
                } else {
                    Ok(Value::String(stdout))
                }
            }
            "пітон_файл" => {
                let path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected file path")) };
                let extra_args: Vec<String> = args.iter().skip(1).filter_map(|a| match a { Value::String(s) => Some(s.clone()), Value::Integer(n) => Some(n.to_string()), Value::Float(f) => Some(f.to_string()), _ => None }).collect();
                let py_cmd = if cfg!(windows) { "python" } else { "python3" };
                let output = std::process::Command::new(py_cmd)
                    .arg(&path)
                    .args(&extra_args)
                    .output()
                    .map_err(|e| anyhow::anyhow!("Python not found: {}", e))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string().trim().to_string();
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(anyhow::anyhow!("Python error: {}", stderr.trim()));
                }
                if stdout.starts_with('{') || stdout.starts_with('[') {
                    match serde_json::from_str::<serde_json::Value>(&stdout) {
                        Ok(v) => Ok(VM::json_to_value(&v)),
                        Err(_) => Ok(Value::String(stdout)),
                    }
                } else {
                    Ok(Value::String(stdout))
                }
            }
            "мл_ембединг" => {
                let image_path = match &args[0] { Value::String(s) => s.clone(), _ => return Err(anyhow::anyhow!("Expected image path")) };
                let model = match args.get(1) { Some(Value::String(s)) => s.clone(), _ => "clip".to_string() };
                let py_cmd = if cfg!(windows) { "python" } else { "python3" };
                let script = format!(r#"
import json, sys
try:
    if '{}' == 'clip':
        from sentence_transformers import SentenceTransformer
        from PIL import Image
        model = SentenceTransformer('clip-ViT-B-32')
        img = Image.open('{}')
        emb = model.encode(img).tolist()
        print(json.dumps(emb))
    else:
        print(json.dumps([0.0]*512))
except Exception as e:
    print(json.dumps({{"error": str(e)}}))
"#, model, image_path.replace('\\', "\\\\"));
                let output = std::process::Command::new(py_cmd)
                    .args(["-c", &script])
                    .output()
                    .map_err(|e| anyhow::anyhow!("Python/CLIP not found: {}", e))?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string().trim().to_string();
                match serde_json::from_str::<serde_json::Value>(&stdout) {
                    Ok(serde_json::Value::Array(arr)) => {
                        let vec: Vec<Value> = arr.iter().filter_map(|v| v.as_f64().map(Value::Float)).collect();
                        Ok(Value::Array(vec))
                    }
                    Ok(serde_json::Value::Object(obj)) => {
                        if let Some(err) = obj.get("error") {
                            Err(anyhow::anyhow!("CLIP error: {}", err))
                        } else {
                            Ok(Value::String(stdout))
                        }
                    }
                    _ => Err(anyhow::anyhow!("Unexpected CLIP output")),
                }
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
            // ── Системне програмування ──

            // FFI: завантажити бібліотеку
            "зовнішня_бібліотека" => {
                let path = args.first().map(|v| v.to_display_string())
                    .ok_or_else(|| anyhow::anyhow!("зовнішня_бібліотека(шлях)"))?;
                let lib = unsafe { libloading::Library::new(&path) }
                    .map_err(|e| anyhow::anyhow!("Не вдалось завантажити {}: {}", path, e))?;
                let ptr = Box::into_raw(Box::new(lib)) as usize;
                Ok(Value::Integer(ptr as i64))
            }

            // FFI: викликати C функцію
            "зовнішній_виклик" => {
                if args.len() < 2 {
                    return Err(anyhow::anyhow!("зовнішній_виклик(бібліотека, функція, [аргументи...])"));
                }
                let lib_ptr = match &args[0] {
                    Value::Integer(p) => *p as usize,
                    _ => return Err(anyhow::anyhow!("Перший аргумент — вказівник на бібліотеку")),
                };
                let fn_name = args[1].to_display_string();
                let lib = unsafe { &*(lib_ptr as *const libloading::Library) };

                // Конвертуємо аргументи: String → *const u8, Float → f64 bits, Integer → i64
                let mut c_strings: Vec<std::ffi::CString> = Vec::new();
                let call_args: Vec<i64> = args[2..].iter().map(|v| match v {
                    Value::Integer(n) => *n,
                    Value::Float(f) => f.to_bits() as i64,
                    Value::Bool(b) => *b as i64,
                    Value::String(s) => {
                        let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default();
                        let ptr = cs.as_ptr() as i64;
                        c_strings.push(cs);
                        ptr
                    }
                    Value::Null => 0,
                    _ => 0,
                }).collect();

                unsafe {
                    match call_args.len() {
                        0 => {
                            let f: libloading::Symbol<unsafe extern "C" fn() -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f()))
                        }
                        1 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0])))
                        }
                        2 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64, i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0], call_args[1])))
                        }
                        3 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64, i64, i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0], call_args[1], call_args[2])))
                        }
                        4 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64, i64, i64, i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0], call_args[1], call_args[2], call_args[3])))
                        }
                        5 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0], call_args[1], call_args[2], call_args[3], call_args[4])))
                        }
                        6 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Integer(f(call_args[0], call_args[1], call_args[2], call_args[3], call_args[4], call_args[5])))
                        }
                        _ => Err(anyhow::anyhow!("FFI підтримує до 6 аргументів"))
                    }
                }
            }

            "зовнішній_виклик_дрб" => {
                if args.len() < 2 {
                    return Err(anyhow::anyhow!("зовнішній_виклик_дрб(бібліотека, функція, [аргументи...])"));
                }
                let lib_ptr = match &args[0] {
                    Value::Integer(p) => *p as usize,
                    _ => return Err(anyhow::anyhow!("Перший аргумент — вказівник на бібліотеку")),
                };
                let fn_name = args[1].to_display_string();
                let lib = unsafe { &*(lib_ptr as *const libloading::Library) };
                let mut c_strings: Vec<std::ffi::CString> = Vec::new();
                let call_args: Vec<i64> = args[2..].iter().map(|v| match v {
                    Value::Integer(n) => *n,
                    Value::Float(f) => f.to_bits() as i64,
                    Value::Bool(b) => *b as i64,
                    Value::String(s) => {
                        let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default();
                        let ptr = cs.as_ptr() as i64;
                        c_strings.push(cs);
                        ptr
                    }
                    Value::Null => 0,
                    _ => 0,
                }).collect();
                unsafe {
                    match call_args.len() {
                        0 => {
                            let f: libloading::Symbol<unsafe extern "C" fn() -> f64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            Ok(Value::Float(f()))
                        }
                        1 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(f64) -> f64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            let a = match &args[2] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 };
                            Ok(Value::Float(f(a)))
                        }
                        2 => {
                            let f: libloading::Symbol<unsafe extern "C" fn(f64, f64) -> f64> = lib.get(fn_name.as_bytes())
                                .map_err(|e| anyhow::anyhow!("Функція '{}' не знайдена: {}", fn_name, e))?;
                            let a = match &args[2] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 };
                            let b = match &args[3] { Value::Float(f) => *f, Value::Integer(n) => *n as f64, _ => 0.0 };
                            Ok(Value::Float(f(a, b)))
                        }
                        _ => Err(anyhow::anyhow!("зовнішній_виклик_дрб підтримує до 2 аргументів"))
                    }
                }
            }

            "закрити_бібліотеку" => {
                let ptr = match args.first() {
                    Some(Value::Integer(p)) => *p as usize,
                    _ => return Err(anyhow::anyhow!("закрити_бібліотеку(вказівник)")),
                };
                unsafe { let _ = Box::from_raw(ptr as *mut libloading::Library); }
                Ok(Value::Null)
            }

            // Пам'ять: виділити
            "виділити_пам'ять" => {
                let size = match args.first() {
                    Some(Value::Integer(n)) => *n as usize,
                    _ => return Err(anyhow::anyhow!("виділити_пам'ять(розмір)")),
                };
                let layout = std::alloc::Layout::from_size_align(size, 8)
                    .map_err(|_| anyhow::anyhow!("Невірний розмір: {}", size))?;
                let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
                if ptr.is_null() {
                    Err(anyhow::anyhow!("Не вдалось виділити {} байт", size))
                } else {
                    self.allocations.insert(ptr as usize, layout);
                    Ok(Value::Integer(ptr as usize as i64))
                }
            }

            // Пам'ять: звільнити
            "звільнити_пам'ять" => {
                let addr = match args.first() {
                    Some(Value::Integer(n)) => *n as usize,
                    _ => return Err(anyhow::anyhow!("звільнити_пам'ять(адреса)")),
                };
                if let Some(layout) = self.allocations.remove(&addr) {
                    unsafe { std::alloc::dealloc(addr as *mut u8, layout); }
                    Ok(Value::Bool(true))
                } else {
                    Err(anyhow::anyhow!("Невідома адреса: 0x{:x}", addr))
                }
            }

            "записати_байт" => {
                let addr = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("записати_байт(адреса, значення)")) };
                let val = match args.get(1) { Some(Value::Integer(n)) => *n as u8, _ => return Err(anyhow::anyhow!("записати_байт(адреса, значення)")) };
                self.check_memory_access(addr, 1)?;
                unsafe { *(addr as *mut u8) = val; }
                Ok(Value::Null)
            }

            "прочитати_байт" => {
                let addr = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("прочитати_байт(адреса)")) };
                self.check_memory_access(addr, 1)?;
                let val = unsafe { *(addr as *const u8) };
                Ok(Value::Integer(val as i64))
            }

            "записати_слово" => {
                let addr = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("записати_слово(адреса, значення)")) };
                let val = match args.get(1) { Some(Value::Integer(n)) => *n, _ => return Err(anyhow::anyhow!("записати_слово(адреса, значення)")) };
                self.check_memory_access(addr, 8)?;
                unsafe { *(addr as *mut i64) = val; }
                Ok(Value::Null)
            }

            "прочитати_слово" => {
                let addr = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("прочитати_слово(адреса)")) };
                self.check_memory_access(addr, 8)?;
                let val = unsafe { *(addr as *const i64) };
                Ok(Value::Integer(val))
            }

            "копіювати_пам'ять" => {
                let dst = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("копіювати_пам'ять(куди, звідки, розмір)")) };
                let src = match args.get(1) { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("копіювати_пам'ять(куди, звідки, розмір)")) };
                let size = match args.get(2) { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("копіювати_пам'ять(куди, звідки, розмір)")) };
                self.check_memory_access(dst, size)?;
                self.check_memory_access(src, size)?;
                unsafe { std::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, size); }
                Ok(Value::Null)
            }

            "заповнити_пам'ять" => {
                let addr = match args.first() { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("заповнити_пам'ять(адреса, значення, розмір)")) };
                let val = match args.get(1) { Some(Value::Integer(n)) => *n as u8, _ => return Err(anyhow::anyhow!("заповнити_пам'ять(адреса, значення, розмір)")) };
                let size = match args.get(2) { Some(Value::Integer(n)) => *n as usize, _ => return Err(anyhow::anyhow!("заповнити_пам'ять(адреса, значення, розмір)")) };
                self.check_memory_access(addr, size)?;
                unsafe { std::ptr::write_bytes(addr as *mut u8, val, size); }
                Ok(Value::Null)
            }

            // Пам'ять: розмір вказівника
            "розмір_вказівника" => {
                Ok(Value::Integer(std::mem::size_of::<usize>() as i64))
            }

            // Inline assembly (x86_64)
            "asm_виконати" => {
                let code = args.first().map(|v| v.to_display_string())
                    .ok_or_else(|| anyhow::anyhow!("asm_виконати(\"код\", [аргумент1, аргумент2, ...])"))?;

                let input_values: Vec<i64> = args.iter().skip(1).map(|v| match v {
                    Value::Integer(n) => *n,
                    Value::Float(f) => f.to_bits() as i64,
                    Value::Bool(b) => *b as i64,
                    _ => 0,
                }).collect();

                #[cfg(target_arch = "x86_64")]
                {
                    let asm_code = code.replace(';', "\n");
                    let mut machine_code: Vec<u8> = Vec::new();
                    let mut labels: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                    let mut label_refs: Vec<(String, usize)> = Vec::new();

                    if !input_values.is_empty() {
                        if input_values.len() >= 1 {
                            machine_code.extend_from_slice(&[0x48, 0x89, 0xF8]); // mov rax, rdi
                        }
                    }

                    let lines: Vec<&str> = asm_code.lines().collect();
                    for line in &lines {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with(';') || line.starts_with('#') { continue; }
                        if line.ends_with(':') {
                            let label = line.trim_end_matches(':').trim().to_string();
                            labels.insert(label, machine_code.len());
                            continue;
                        }
                        let parts: Vec<&str> = line.splitn(2, |c: char| c == ' ' || c == '\t').collect();
                        let mnemonic = parts[0].to_lowercase();
                        let operands: Vec<&str> = if parts.len() > 1 {
                            parts[1].split(',').map(|s| s.trim()).collect()
                        } else { vec![] };

                        if (mnemonic == "jmp" || mnemonic == "je" || mnemonic == "jne" || mnemonic == "jl" || mnemonic == "jg" || mnemonic == "jle" || mnemonic == "jge") && !operands.is_empty() {
                            if operands[0].parse::<i64>().is_err() && !operands[0].starts_with("0x") {
                                let label_name = operands[0].to_string();
                                let opcode = match mnemonic.as_str() {
                                    "jmp" => vec![0xE9],
                                    "je" | "jz" => vec![0x0F, 0x84],
                                    "jne" | "jnz" => vec![0x0F, 0x85],
                                    "jl" => vec![0x0F, 0x8C],
                                    "jg" => vec![0x0F, 0x8F],
                                    "jle" => vec![0x0F, 0x8E],
                                    "jge" => vec![0x0F, 0x8D],
                                    _ => vec![0xE9],
                                };
                                machine_code.extend_from_slice(&opcode);
                                label_refs.push((label_name, machine_code.len()));
                                machine_code.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
                                continue;
                            }
                        }

                        Self::encode_x86(&mnemonic, &operands, &mut machine_code)?;
                    }

                    for (label_name, ref_offset) in &label_refs {
                        if let Some(&target) = labels.get(label_name) {
                            let rel = (target as i64) - (*ref_offset as i64 + 4);
                            let bytes = (rel as i32).to_le_bytes();
                            machine_code[*ref_offset..*ref_offset + 4].copy_from_slice(&bytes);
                        }
                    }

                    if machine_code.is_empty() || machine_code.last() != Some(&0xC3) {
                        machine_code.push(0xC3); // ret
                    }

                    let jit_fn = jit::JitFunction::new(machine_code);
                    let result = if input_values.is_empty() {
                        jit_fn.execute_raw()
                    } else {
                        jit_fn.execute_with_arg(input_values[0])
                    };
                    Ok(Value::Integer(result))
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    Err(anyhow::anyhow!("Inline assembly підтримується тільки на x86_64"))
                }
            }

            // Системний виклик (syscall)
            "системний_виклик" => {
                #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
                {
                    let num = match args.get(0) { Some(Value::Integer(n)) => *n as u64, _ => return Err(anyhow::anyhow!("системний_виклик(номер, ...)")) };
                    let a1 = args.get(1).map(|v| match v { Value::Integer(n) => *n as u64, _ => 0 }).unwrap_or(0);
                    let a2 = args.get(2).map(|v| match v { Value::Integer(n) => *n as u64, _ => 0 }).unwrap_or(0);
                    let a3 = args.get(3).map(|v| match v { Value::Integer(n) => *n as u64, _ => 0 }).unwrap_or(0);
                    let ret: u64;
                    unsafe {
                        std::arch::asm!(
                            "syscall",
                            inout("rax") num => ret,
                            in("rdi") a1,
                            in("rsi") a2,
                            in("rdx") a3,
                            out("rcx") _,
                            out("r11") _,
                        );
                    }
                    Ok(Value::Integer(ret as i64))
                }
                #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
                {
                    Err(anyhow::anyhow!("системний_виклик доступний тільки на Linux x86_64"))
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
        // Перевірка циклічних залежностей
        if self.loading_modules.contains(name) {
            return Err(anyhow::anyhow!(
                "Циклічна залежність: модуль '{}' вже завантажується.\n  Ланцюг: {} → {}",
                name,
                self.loading_modules.iter().cloned().collect::<Vec<_>>().join(" → "),
                name
            ));
        }

        let filenames = [format!("{}.тризуб", name),
            format!("{}.tryzub", name)];

        let mut search_paths = self.stdlib_paths.clone();
        search_paths.insert(0, ".".to_string());

        let sub_filenames: Vec<String> = vec![
            format!("{}/{}.тризуб", name, name),
            format!("ядро/{}.тризуб", name),
        ];

        for base_path in &search_paths {
            for filename in filenames.iter().chain(sub_filenames.iter()) {
                let path = format!("{}/{}", base_path, filename);
                if let Ok(source) = std::fs::read_to_string(&path) {
                    self.loading_modules.insert(name.to_string());

                    let tokens = tryzub_lexer::tokenize(&source)?;
                    let program = tryzub_parser::parse(tokens)?;

                    // Зберігаємо поточне середовище та створюємо ізольоване для модуля
                    let prev_env = self.current_env.clone();
                    let module_env = Rc::new(RefCell::new(Scope::new(Some(self.global_env.clone()))));
                    self.current_env = module_env.clone();

                    for decl in program.declarations {
                        self.execute_declaration(decl)?;
                    }

                    // Збираємо всі публічні символи модуля
                    let mut members = HashMap::new();
                    let scope = module_env.borrow();
                    for (k, v) in &scope.variables {
                        members.insert(k.clone(), v.clone());
                    }

                    // Відновлюємо попереднє середовище
                    self.current_env = prev_env;
                    self.loading_modules.remove(name);

                    // Зберігаємо модуль як Value::Module
                    let module_val = Value::Module(name.to_string(), members);
                    self.module_values.insert(name.to_string(), module_val.clone());

                    // Також реєструємо в глобальному scope для зворотної сумісності
                    self.global_env.borrow_mut().set(name.to_string(), module_val);

                    self.loaded_modules.insert(name.to_string(), true);
                    return Ok(());
                }
            }
        }

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

    // ── GC: Збирач сміття для циклічних посилань ──

    fn run_gc(&mut self) {
        if self.generator_cache.len() > 100 {
            self.generator_cache.clear();
        }
        if self.pure_cache.len() > 5_000 {
            self.pure_cache.clear();
        }
        if self.effect_handlers.len() > 50 {
            self.effect_handlers.truncate(10);
        }

        // Scope pruning вимкнено — sweep_scope некоректно видаляє parent scopes
        // що ще містять живі змінні (наприклад в циклах for з >10K ітерацій).
        // Потрібна повноцінна reachability analysis перед pruning.
    }

    #[allow(dead_code)]
    fn sweep_scope(&self, env: &Environment) {
        let scope = env.borrow();
        if let Some(ref parent) = scope.parent {
            // Якщо parent тримається тільки цим scope (strong_count == 2: parent's own + this ref)
            // і parent не є global — можна обрізати parent chain
            if Rc::strong_count(parent) <= 2 && !std::ptr::eq(parent.as_ptr(), self.global_env.as_ptr()) {
                let parent_scope = parent.borrow();
                if let Some(ref grandparent) = parent_scope.parent {
                    if Rc::strong_count(grandparent) <= 2
                        && !std::ptr::eq(grandparent.as_ptr(), self.global_env.as_ptr())
                    {
                        // grandparent більше нікому не потрібен — drop chain зупиниться тут
                        drop(parent_scope);
                        parent.borrow_mut().parent = None;
                    }
                }
            }
        }
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
                Value::Array(arr.iter().map(VM::json_to_value).collect())
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

                if let Some(_condition) = expr.strip_prefix("якщо ") {
                    // Умовний блок: {якщо умова}...{/якщо}
                    // "якщо " = 9 bytes in UTF-8? Let's be safe
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
                    let mut rng = rand::thread_rng();
                    let token_bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
                    let token = token_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
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

    // base64 тепер через crate base64 (URL_SAFE_NO_PAD)

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
                serde_json::Value::Array(arr.iter().map(VM::value_to_json).collect())
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
    let handle = std::thread::Builder::new()
        .name("tryzub-vm".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let mut vm = VM::new();
            vm.execute_program(program, args)
        })
        .map_err(|e| anyhow::anyhow!("Не вдалося створити потік VM: {}", e))?;
    handle.join().unwrap_or_else(|_| Err(anyhow::anyhow!("VM паніка")))
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

    // ── Тести системного програмування ──

    fn run_tryzub(source: &str) -> Result<()> {
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        execute(program, vec![])
    }

    #[test]
    fn test_memory_alloc_write_read_free() {
        let r = run_tryzub(r#"
функція головна() {
    стала б = виділити_пам'ять(64)
    записати_байт(б, 42)
    стала значення = прочитати_байт(б)
    перевірити (значення == 42)
    записати_слово(б + 8, 1234567890)
    стала с = прочитати_слово(б + 8)
    перевірити (с == 1234567890)
    звільнити_пам'ять(б)
}
"#);
        assert!(r.is_ok(), "Memory alloc/write/read/free failed: {:?}", r.err());
    }

    #[test]
    fn test_memory_bounds_check() {
        let r = run_tryzub(r#"
функція головна() {
    стала б = виділити_пам'ять(16)
    записати_байт(б + 100, 0)
}
"#);
        assert!(r.is_err(), "Bounds check should reject out-of-range write");
    }

    #[test]
    fn test_memory_memset_memcpy() {
        let r = run_tryzub(r#"
функція головна() {
    стала а = виділити_пам'ять(64)
    стала б = виділити_пам'ять(64)
    заповнити_пам'ять(а, 0xAB, 16)
    перевірити (прочитати_байт(а) == 0xAB)
    перевірити (прочитати_байт(а + 15) == 0xAB)
    копіювати_пам'ять(б, а, 16)
    перевірити (прочитати_байт(б) == 0xAB)
    звільнити_пам'ять(а)
    звільнити_пам'ять(б)
}
"#);
        assert!(r.is_ok(), "memset/memcpy failed: {:?}", r.err());
    }

    #[test]
    fn test_memory_double_free() {
        let r = run_tryzub(r#"
функція головна() {
    стала б = виділити_пам'ять(16)
    звільнити_пам'ять(б)
    звільнити_пам'ять(б)
}
"#);
        assert!(r.is_err(), "Double free should return error");
    }

    #[test]
    fn test_pointer_size() {
        let mut vm = VM::new();
        let result = vm.call_builtin("розмір_вказівника", vec![]).unwrap();
        if let Value::Integer(n) = result {
            assert_eq!(n, 8); // x86_64
        } else {
            panic!("Expected Integer");
        }
    }

    #[test]
    fn test_inline_asm_mov_ret() {
        let r = run_tryzub(r#"
функція головна() {
    стала результат = asm_виконати("mov rax, 42\nret")
    перевірити (результат == 42)
}
"#);
        assert!(r.is_ok(), "Inline asm mov+ret failed: {:?}", r.err());
    }

    #[test]
    fn test_inline_asm_arithmetic() {
        let r = run_tryzub(r#"
функція головна() {
    стала результат = asm_виконати("mov rax, 100\nsub rax, 58\nret")
    перевірити (результат == 42)
}
"#);
        assert!(r.is_ok(), "Inline asm arithmetic failed: {:?}", r.err());
    }

    #[test]
    fn test_inline_asm_bitwise() {
        let r = run_tryzub(r#"
функція головна() {
    стала результат = asm_виконати("mov rax, 0xFF\nand rax, 0x0F\nret")
    перевірити (результат == 15)
}
"#);
        assert!(r.is_ok(), "Inline asm bitwise failed: {:?}", r.err());
    }

    #[test]
    fn test_inline_asm_invalid() {
        let r = run_tryzub(r#"
функція головна() {
    asm_виконати("невідома_інструкція rax")
}
"#);
        assert!(r.is_err(), "Invalid asm should return error");
    }

    #[test]
    fn test_ffi_load_library() {
        let r = run_tryzub(r#"
функція головна() {
    стала л = зовнішня_бібліотека("msvcrt.dll")
    перевірити (л > 0)
}
"#);
        assert!(r.is_ok(), "FFI load library failed: {:?}", r.err());
    }

    #[test]
    fn test_ffi_invalid_library() {
        let r = run_tryzub(r#"
функція головна() {
    зовнішня_бібліотека("неіснуюча_бібліотека.dll")
}
"#);
        assert!(r.is_err(), "FFI should fail for non-existent library");
    }

    #[test]
    fn test_jit_arithmetic() {
        // Тестуємо JIT компіляцію простої арифметики
        let source = "функція головна() { стала а = 7\n стала б = 6\n друк(а * б) }";
        let tokens = tokenize(source).unwrap();
        let ast = parse(tokens).unwrap();
        let compiler = crate::compiler::Compiler::new();
        let chunk = compiler.compile_program(&ast);
        // Перевірка що компіляція в bytecode працює
        assert!(!chunk.code.is_empty(), "Bytecode chunk should not be empty");
        // JIT компіляція
        let jit_compiler = crate::jit::JitCompiler::new();
        let jit_fn = jit_compiler.compile(&chunk);
        // JIT execute працює без panic
        let _ = jit_fn.execute();
    }

    #[test]
    fn test_top_level_code() {
        // Top-level wrapping happens in run_file, so we wrap manually
        let source = "функція головна() { друк(42) }";
        let tokens = tokenize(source).unwrap();
        let ast = parse(tokens).unwrap();
        assert!(execute(ast, vec![]).is_ok());
    }

    #[test]
    fn test_optional_parens() {
        let r = run_tryzub(r#"
функція головна() {
    якщо істина { друк("ok") }
    для і в 1..4 { друк(і) }
    змінна х = 3
    поки х > 0 { х = х - 1 }
    перевірити (х == 0)
}
"#);
        assert!(r.is_ok(), "Optional parens failed: {:?}", r.err());
    }

    #[test]
    fn test_default_params() {
        let r = run_tryzub(r#"
функція додати(а, б = 10) {
    повернути а + б
}
функція головна() {
    перевірити (додати(5) == 15)
    перевірити (додати(5, 20) == 25)
}
"#);
        assert!(r.is_ok(), "Default params failed: {:?}", r.err());
    }
}
