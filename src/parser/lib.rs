use anyhow::Result;
use thiserror::Error;
use tryzub_lexer::{Token, TokenKind, StringPart};
use std::fmt;

// ════════════════════════════════════════════════════════════════════
// AST — Абстрактне синтаксичне дерево мови Тризуб v2.0
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Declaration>,
}

// ── Декларації ──

#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Variable {
        name: String,
        ty: Option<Type>,
        value: Option<Expression>,
        is_mutable: bool,
    },
    Function {
        name: String,
        params: Vec<Parameter>,
        return_type: Option<Type>,
        body: Vec<Statement>,
        is_async: bool,
        visibility: Visibility,
        contract: Option<Contract>,
    },
    Struct {
        name: String,
        fields: Vec<Field>,
        methods: Vec<Declaration>, // Методи через реалізація
        visibility: Visibility,
    },
    /// Алгебраїчний тип (enum / sum type)
    Enum {
        name: String,
        generic_params: Vec<String>,
        variants: Vec<EnumVariant>,
        visibility: Visibility,
    },
    /// Трейт (типаж)
    Trait {
        name: String,
        generic_params: Vec<String>,
        methods: Vec<TraitMethod>,
        visibility: Visibility,
    },
    /// Реалізація трейта для типу: реалізація Трейт для Тип { ... }
    TraitImpl {
        trait_name: String,
        for_type: String,
        generic_params: Vec<String>,
        methods: Vec<Declaration>,
    },
    /// Реалізація методів для типу: реалізація Тип { ... }
    Impl {
        type_name: String,
        methods: Vec<Declaration>,
    },
    Module {
        name: String,
        declarations: Vec<Declaration>,
        visibility: Visibility,
    },
    Import {
        path: Vec<String>,
        items: Option<Vec<String>>, // використати модуль::{ елемент1, елемент2 }
        alias: Option<String>,
    },
    TypeAlias {
        name: String,
        generic_params: Vec<String>,
        ty: Type,
        visibility: Visibility,
    },
    Interface {
        name: String,
        methods: Vec<InterfaceMethod>,
        visibility: Visibility,
    },
    /// Оголошення ефекту
    Effect {
        name: String,
        operations: Vec<TraitMethod>,
    },
    /// Макрос
    Macro {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
    },
    /// Тестовий блок: тест "назва" { ... }
    Test {
        name: String,
        body: Vec<Statement>,
    },
    /// Фаз-тест: фаз "назва" вхід(...) { ... }
    FuzzTest {
        name: String,
        inputs: Vec<FuzzInput>,
        body: Vec<Statement>,
    },
    /// Бенчмарк: бенчмарк "назва" { ... }
    Benchmark {
        name: String,
        sizes: Vec<Expression>,
        body: Vec<Statement>,
    },
}

/// Вхідний параметр для фаз-тесту
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzInput {
    pub name: String,
    pub ty: Type,
    pub range: Option<(Expression, Expression)>,
}

/// Контракт функції (вимагає/гарантує)
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub preconditions: Vec<Expression>,    // вимагає { ... }
    pub postconditions: Vec<Expression>,   // гарантує { ... }
    pub result_name: Option<String>,       // гарантує(результат) { ... }
}

/// Ефекти функції [ввід_вивід, мережа]
#[derive(Debug, Clone, PartialEq)]
pub struct EffectAnnotation {
    pub effects: Vec<String>,
    pub is_pure: bool,  // чистий
}

/// Атрибут #[назва(аргументи)]
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<EnumField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumField {
    pub name: Option<String>, // Іменовані або позиційні
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub default_body: Option<Vec<Statement>>, // Метод за замовчуванням
    pub has_self: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceMethod {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
}

// ── Типи ──

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Цл8, Цл16, Цл32, Цл64,
    Чс8, Чс16, Чс32, Чс64,
    Дрб32, Дрб64,
    Лог,
    Сим,
    Тхт,
    Array(Box<Type>, usize),
    Slice(Box<Type>),
    Tuple(Vec<Type>),
    Reference(Box<Type>, bool), // bool = is_mutable
    Function(Vec<Type>, Option<Box<Type>>),
    Named(String),
    Generic(String, Vec<Type>), // Назва<Т1, Т2>
    Optional(Box<Type>),        // Опція<Т>
    Result(Box<Type>, Box<Type>), // Результат<Т, П>
    SelfType,                   // себе
}

// ── Інструкції (Statements) ──

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Expression(Expression),
    Block(Vec<Statement>),
    Return(Option<Expression>),
    If {
        condition: Expression,
        then_branch: Box<Statement>,
        else_branch: Option<Box<Statement>>,
    },
    While {
        condition: Expression,
        body: Box<Statement>,
    },
    For {
        variable: String,
        from: Expression,
        to: Expression,
        step: Option<Expression>,
        body: Box<Statement>,
    },
    /// Новий for...в (for-in) для ітерації по колекціях/діапазонах
    ForIn {
        pattern: Pattern,
        iterable: Expression,
        body: Box<Statement>,
    },
    Break,
    Continue,
    Assignment {
        target: Expression,
        value: Expression,
        op: AssignmentOp,
    },
    Declaration(Declaration),
    /// Деструктуризація: змінна { a, b, ..rest } = expr
    Destructure {
        pattern: Pattern,
        value: Expression,
        is_mutable: bool,
    },
    /// Try/catch/finally
    TryCatch {
        try_body: Box<Statement>,
        catch_param: Option<String>,
        catch_body: Option<Box<Statement>>,
        finally_body: Option<Box<Statement>>,
    },
    /// Перевірити (assert): перевірити вираз
    Assert(Expression),
    /// Блок з обробником ефектів: з_обробником Обробник { ... }
    WithHandler {
        handler: String,
        body: Box<Statement>,
    },
    /// Компчас блок: компчас { ... }
    CompTime(Vec<Statement>),
    /// Unsafe блок: небезпечний { ... }
    Unsafe(Vec<Statement>),
    /// Yield: віддати вираз
    Yield(Expression),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignmentOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
}

// ── Зразки (Patterns) для pattern matching та деструктуризації ──

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Wildcard: _
    Wildcard,
    /// Літерал: 42, "hello", істина
    Literal(Literal),
    /// Прив'язка змінної: х
    Binding(String),
    /// Варіант enum: Деякий(х)
    Variant {
        name: String,
        fields: Vec<Pattern>,
    },
    /// Деструктуризація структури: { поле1, поле2, ..решта }
    Struct {
        fields: Vec<(String, Option<Pattern>)>,
        rest: bool,
    },
    /// Деструктуризація масиву/кортежу: [a, b, ..rest]
    Array {
        elements: Vec<Pattern>,
        rest: Option<String>,
    },
    /// Кортеж: (a, b)
    Tuple(Vec<Pattern>),
    /// Guard умова: зразок якщо умова
    Guard {
        pattern: Box<Pattern>,
        condition: Box<Expression>,
    },
    /// OR-pattern: A | B
    Or(Vec<Pattern>),
}

// ── Вирази ──

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal),
    Identifier(String),
    SelfRef,  // себе
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expression>,
    },
    Call {
        callee: Box<Expression>,
        args: Vec<Expression>,
    },
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    MemberAccess {
        object: Box<Expression>,
        member: String,
    },
    /// Виклик методу: об'єкт.метод(аргументи)
    MethodCall {
        object: Box<Expression>,
        method: String,
        args: Vec<Expression>,
    },
    Array(Vec<Expression>),
    Tuple(Vec<Expression>),
    Struct {
        name: String,
        fields: Vec<(String, Expression)>,
    },
    /// Лямбда-функція: |x, y| вираз
    Lambda {
        params: Vec<LambdaParam>,
        body: Box<Expression>,
    },
    /// Лямбда з блоком: |x| { інструкції }
    LambdaBlock {
        params: Vec<LambdaParam>,
        body: Vec<Statement>,
    },
    /// Вираз if: якщо умова { then } інакше { else }
    If {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    /// Зіставлення зразків (match)
    Match {
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
    },
    /// Pipeline: вираз |> функція
    Pipeline {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// Поширення помилки: вираз?
    ErrorPropagation(Box<Expression>),
    /// Форматований рядок: ф"...{вираз}..."
    FormatString(Vec<FormatPart>),
    /// Діапазон: від..до або від..=до
    Range {
        from: Box<Expression>,
        to: Box<Expression>,
        inclusive: bool,
    },
    /// Конструктор enum: Варіант(аргументи)
    EnumConstruct {
        variant: String,
        args: Vec<Expression>,
    },
    /// Приведення типів: вираз як тип
    Cast {
        expr: Box<Expression>,
        ty: Type,
    },
    /// Await: чекати вираз
    Await(Box<Expression>),
    /// Шлях: модуль::елемент
    Path {
        segments: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParam {
    pub name: String,
    pub ty: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormatPart {
    Text(String),
    Expr(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    // Побітові
    BitAnd, BitOr, BitXor,
    Shl, Shr,
    // Належність
    In, // x в масив
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg, Not, BitNot,
}

// ════════════════════════════════════════════════════════════════════
// Парсер
// ════════════════════════════════════════════════════════════════════

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Несподіваний токен: очікувався {expected}, отримано {found} на рядку {line}")]
    UnexpectedToken {
        expected: String,
        found: String,
        line: usize,
    },

    #[error("Несподіваний кінець файлу")]
    UnexpectedEof,

    #[error("Невалідний вираз на рядку {0}")]
    InvalidExpression(usize),

    #[error("Невалідне оголошення на рядку {0}")]
    InvalidDeclaration(usize),

    #[error("Невалідний зразок на рядку {0}")]
    InvalidPattern(usize),
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    pub fn parse(&mut self) -> Result<Program> {
        let mut declarations = Vec::new();

        while !self.is_at_end() {
            declarations.push(self.declaration()?);
        }

        Ok(Program { declarations })
    }

    // ── Декларації ──

    fn declaration(&mut self) -> Result<Declaration> {
        let visibility = if self.match_token(&TokenKind::Публічний) {
            Visibility::Public
        } else if self.match_token(&TokenKind::Приватний) {
            Visibility::Private
        } else {
            Visibility::Private
        };

        if self.match_token(&TokenKind::Змінна) || self.match_token(&TokenKind::Стала) {
            let is_mutable = self.previous().kind == TokenKind::Змінна;
            self.variable_declaration(is_mutable)
        } else if self.match_token(&TokenKind::Функція) {
            self.function_declaration(false, visibility)
        } else if self.match_token(&TokenKind::Асинхронний) {
            self.consume(&TokenKind::Функція, "Очікувалось 'функція' після 'асинхронний'")?;
            self.function_declaration(true, visibility)
        } else if self.match_token(&TokenKind::Структура) {
            self.struct_declaration(visibility)
        } else if self.match_token(&TokenKind::Тип) {
            self.type_or_enum_declaration(visibility)
        } else if self.match_token(&TokenKind::Трейт) {
            self.trait_declaration(visibility)
        } else if self.match_token(&TokenKind::Реалізація) {
            self.impl_declaration()
        } else if self.match_token(&TokenKind::Модуль) {
            self.module_declaration(visibility)
        } else if self.match_token(&TokenKind::Імпорт) {
            self.import_declaration()
        } else if self.match_token(&TokenKind::Інтерфейс) {
            self.interface_declaration(visibility)
        } else if self.match_token(&TokenKind::Ефект) {
            self.effect_declaration()
        } else if self.match_token(&TokenKind::Макрос) {
            self.macro_declaration()
        } else if self.match_token(&TokenKind::Тест) {
            self.test_declaration()
        } else if self.match_token(&TokenKind::Фаз) {
            self.fuzz_declaration()
        } else if self.match_token(&TokenKind::Бенчмарк) {
            self.benchmark_declaration()
        } else {
            Err(ParseError::InvalidDeclaration(self.peek().line).into())
        }
    }

    fn variable_declaration(&mut self, is_mutable: bool) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я змінної")?;

        let ty = if self.match_token(&TokenKind::Двокрапка) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.match_token(&TokenKind::Присвоїти) {
            Some(self.expression()?)
        } else {
            None
        };

        Ok(Declaration::Variable { name, ty, value, is_mutable })
    }

    fn function_declaration(&mut self, is_async: bool, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я функції")?;

        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після імені функції")?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::ПраваДужка) {
            loop {
                // Підтримка 'себе' як першого параметра
                if self.check(&TokenKind::Себе) {
                    self.advance();
                    params.push(Parameter {
                        name: "себе".to_string(),
                        ty: Type::SelfType,
                        default: None,
                    });
                } else {
                    let param_name = self.consume_identifier("Очікувалось ім'я параметра")?;
                    self.consume(&TokenKind::Двокрапка, "Очікувалась ':' після імені параметра")?;
                    let param_type = self.parse_type()?;

                    let default = if self.match_token(&TokenKind::Присвоїти) {
                        Some(self.expression()?)
                    } else {
                        None
                    };

                    params.push(Parameter {
                        name: param_name,
                        ty: param_type,
                        default,
                    });
                }

                if !self.match_token(&TokenKind::Кома) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після параметрів")?;

        let return_type = if self.match_token(&TokenKind::Стрілка) {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Парсимо контракти (вимагає/гарантує) перед тілом
        let mut contract: Option<Contract> = None;
        let mut preconditions = Vec::new();
        let mut postconditions = Vec::new();
        let mut result_name = None;

        // вимагає { умова1, умова2, ... }
        if self.match_token(&TokenKind::Вимагає) {
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                preconditions.push(self.expression()?);
                let _ = self.match_token(&TokenKind::Кома);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        }

        // гарантує або гарантує(результат) { умова1, ... }
        if self.match_token(&TokenKind::Гарантує) {
            if self.match_token(&TokenKind::ЛіваДужка) {
                result_name = Some(self.consume_identifier("Очікувалось ім'я результату")?);
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            }
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                postconditions.push(self.expression()?);
                let _ = self.match_token(&TokenKind::Кома);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        }

        if !preconditions.is_empty() || !postconditions.is_empty() {
            contract = Some(Contract { preconditions, postconditions, result_name });
        }

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' перед тілом функції")?;

        let mut body = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            body.push(self.statement()?);
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}' після тіла функції")?;

        Ok(Declaration::Function {
            name,
            params,
            return_type,
            body,
            is_async,
            visibility,
            contract,
        })
    }

    fn struct_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я структури")?;

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            let field_visibility = if self.match_token(&TokenKind::Публічний) {
                Visibility::Public
            } else {
                Visibility::Private
            };

            let field_name = self.consume_identifier("Очікувалось ім'я поля")?;
            self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
            let field_type = self.parse_type()?;

            fields.push(Field {
                name: field_name,
                ty: field_type,
                visibility: field_visibility,
            });

            if !self.match_token(&TokenKind::Кома) {
                break;
            }
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Declaration::Struct { name, fields, methods: Vec::new(), visibility })
    }

    /// тип Назва<Т> { Варіант1(поля), Варіант2 }
    fn type_or_enum_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я типу")?;

        // Перевіряємо generic параметри
        let generic_params = self.parse_generic_params()?;

        // Якщо далі '=', це type alias
        if self.match_token(&TokenKind::Присвоїти) {
            let ty = self.parse_type()?;
            return Ok(Declaration::TypeAlias { name, generic_params, ty, visibility });
        }

        // Інакше це enum з варіантами
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;

        let mut variants = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            let variant_name = self.consume_identifier("Очікувалось ім'я варіанту")?;

            let fields = if self.match_token(&TokenKind::ЛіваДужка) {
                let mut fields = Vec::new();
                if !self.check(&TokenKind::ПраваДужка) {
                    loop {
                        // Перевіряємо чи є ім'я поля (ім'я: тип) або тільки тип
                        let field = if self.check_identifier() && self.peek_next_kind() == Some(TokenKind::Двокрапка) {
                            let field_name = self.consume_identifier("Очікувалось ім'я поля")?;
                            self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                            let field_type = self.parse_type()?;
                            EnumField { name: Some(field_name), ty: field_type }
                        } else {
                            let field_type = self.parse_type()?;
                            EnumField { name: None, ty: field_type }
                        };
                        fields.push(field);

                        if !self.match_token(&TokenKind::Кома) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
                fields
            } else {
                Vec::new()
            };

            variants.push(EnumVariant { name: variant_name, fields });

            if !self.match_token(&TokenKind::Кома) {
                break;
            }
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Declaration::Enum { name, generic_params, variants, visibility })
    }

    /// трейт Назва<Т> { функція метод(себе) -> Тип ... }
    fn trait_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я трейту")?;
        let generic_params = self.parse_generic_params()?;

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            self.consume(&TokenKind::Функція, "Очікувалась 'функція' в трейті")?;
            let method_name = self.consume_identifier("Очікувалось ім'я методу")?;

            self.consume(&TokenKind::ЛіваДужка, "Очікувалась '('")?;

            let mut params = Vec::new();
            let mut has_self = false;
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    if self.check(&TokenKind::Себе) {
                        self.advance();
                        has_self = true;
                    } else {
                        let param_name = self.consume_identifier("Очікувалось ім'я параметра")?;
                        self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                        let param_type = self.parse_type()?;
                        params.push(Parameter { name: param_name, ty: param_type, default: None });
                    }

                    if !self.match_token(&TokenKind::Кома) {
                        break;
                    }
                }
            }

            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;

            let return_type = if self.match_token(&TokenKind::Стрілка) {
                Some(self.parse_type()?)
            } else {
                None
            };

            // Перевіряємо чи є тіло за замовчуванням
            let default_body = if self.check(&TokenKind::ЛіваФігурна) {
                self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
                let mut body = Vec::new();
                while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                    body.push(self.statement()?);
                }
                self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
                Some(body)
            } else {
                None
            };

            methods.push(TraitMethod {
                name: method_name,
                params,
                return_type,
                default_body,
                has_self,
            });
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Declaration::Trait { name, generic_params, methods, visibility })
    }

    /// реалізація Трейт для Тип { ... } або реалізація Тип { ... }
    fn impl_declaration(&mut self) -> Result<Declaration> {
        let first_name = self.consume_identifier("Очікувалось ім'я")?;
        let generic_params = self.parse_generic_params()?;

        if self.match_token(&TokenKind::Для) {
            // реалізація Трейт для Тип
            let for_type = self.consume_identifier("Очікувалось ім'я типу після 'для'")?;

            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let mut methods = Vec::new();
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                let vis = Visibility::Public;
                self.consume(&TokenKind::Функція, "Очікувалась 'функція'")?;
                methods.push(self.function_declaration(false, vis)?);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

            Ok(Declaration::TraitImpl {
                trait_name: first_name,
                for_type,
                generic_params,
                methods,
            })
        } else {
            // реалізація Тип { ... }
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let mut methods = Vec::new();
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                let vis = Visibility::Public;
                self.consume(&TokenKind::Функція, "Очікувалась 'функція'")?;
                methods.push(self.function_declaration(false, vis)?);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

            Ok(Declaration::Impl {
                type_name: first_name,
                methods,
            })
        }
    }

    fn module_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я модуля")?;

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut declarations = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            declarations.push(self.declaration()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Declaration::Module { name, declarations, visibility })
    }

    fn import_declaration(&mut self) -> Result<Declaration> {
        let mut path = vec![self.consume_identifier("Очікувався шлях імпорту")?];

        while self.match_token(&TokenKind::Крапка) || self.match_token(&TokenKind::ПодвійнаДвокрапка) {
            path.push(self.consume_identifier("Очікувалось ім'я після '::'")?);
        }

        // Перевіряємо { елемент1, елемент2 }
        let items = if self.match_token(&TokenKind::ЛіваФігурна) {
            let mut items = Vec::new();
            loop {
                items.push(self.consume_identifier("Очікувалось ім'я елемента")?);
                if !self.match_token(&TokenKind::Кома) {
                    break;
                }
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
            Some(items)
        } else {
            None
        };

        let alias = if self.match_token(&TokenKind::Як) {
            Some(self.consume_identifier("Очікувався псевдонім")?)
        } else {
            None
        };

        Ok(Declaration::Import { path, items, alias })
    }

    fn interface_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я інтерфейсу")?;

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            self.consume(&TokenKind::Функція, "Очікувалась 'функція'")?;
            let method_name = self.consume_identifier("Очікувалось ім'я методу")?;

            self.consume(&TokenKind::ЛіваДужка, "Очікувалась '('")?;
            let mut params = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    let param_name = self.consume_identifier("Очікувалось ім'я параметра")?;
                    self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                    let param_type = self.parse_type()?;
                    params.push(Parameter { name: param_name, ty: param_type, default: None });
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;

            let return_type = if self.match_token(&TokenKind::Стрілка) {
                Some(self.parse_type()?)
            } else {
                None
            };

            methods.push(InterfaceMethod { name: method_name, params, return_type });

            let _ = self.match_token(&TokenKind::Кома);
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Declaration::Interface { name, methods, visibility })
    }

    /// ефект Назва { функція операція(...) -> Тип }
    fn effect_declaration(&mut self) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я ефекту")?;
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;

        let mut operations = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            self.consume(&TokenKind::Функція, "Очікувалась 'функція'")?;
            let op_name = self.consume_identifier("Очікувалось ім'я операції")?;
            self.consume(&TokenKind::ЛіваДужка, "Очікувалась '('")?;
            let mut params = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    let pn = self.consume_identifier("Очікувалось ім'я параметра")?;
                    self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                    let pt = self.parse_type()?;
                    params.push(Parameter { name: pn, ty: pt, default: None });
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            let return_type = if self.match_token(&TokenKind::Стрілка) { Some(self.parse_type()?) } else { None };
            operations.push(TraitMethod { name: op_name, params, return_type, default_body: None, has_self: false });
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Declaration::Effect { name, operations })
    }

    /// макрос назва(параметри) { ... }
    fn macro_declaration(&mut self) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я макросу")?;
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '('")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::ПраваДужка) {
            loop {
                params.push(self.consume_identifier("Очікувалось ім'я параметра")?);
                if !self.match_token(&TokenKind::Кома) { break; }
            }
        }
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            body.push(self.statement()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Declaration::Macro { name, params, body })
    }

    /// тест "назва" { ... }
    fn test_declaration(&mut self) -> Result<Declaration> {
        let name = if let TokenKind::Рядок(s) = &self.peek().kind {
            let n = s.clone(); self.advance(); n
        } else {
            self.consume_identifier("Очікувалась назва тесту")?
        };
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            body.push(self.statement()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Declaration::Test { name, body })
    }

    /// фаз "назва" вхід(...) { ... }
    fn fuzz_declaration(&mut self) -> Result<Declaration> {
        let name = if let TokenKind::Рядок(s) = &self.peek().kind {
            let n = s.clone(); self.advance(); n
        } else {
            self.consume_identifier("Очікувалась назва фаз-тесту")?
        };

        // Парсимо вхідні параметри
        let mut inputs = Vec::new();
        // Пропускаємо "вхід" якщо є
        if self.check_identifier() {
            self.advance(); // skip "вхід"
        }
        if self.match_token(&TokenKind::ЛіваДужка) {
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    let inp_name = self.consume_identifier("Очікувалось ім'я")?;
                    self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                    let inp_type = self.parse_type()?;
                    let range = if self.match_token(&TokenKind::В) {
                        let from = self.expression()?;
                        self.consume(&TokenKind::Діапазон, "Очікувалось '..'")?;
                        let to = self.expression()?;
                        Some((from, to))
                    } else { None };
                    inputs.push(FuzzInput { name: inp_name, ty: inp_type, range });
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
        }

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            body.push(self.statement()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Declaration::FuzzTest { name, inputs, body })
    }

    /// бенчмарк "назва" розмір(...) { ... }
    fn benchmark_declaration(&mut self) -> Result<Declaration> {
        let name = if let TokenKind::Рядок(s) = &self.peek().kind {
            let n = s.clone(); self.advance(); n
        } else {
            self.consume_identifier("Очікувалась назва бенчмарку")?
        };

        let mut sizes = Vec::new();
        // Пропускаємо "розмір" якщо є
        if self.check_identifier() {
            self.advance();
        }
        if self.match_token(&TokenKind::ЛіваДужка) {
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    sizes.push(self.expression()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
        }

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            body.push(self.statement()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Declaration::Benchmark { name, sizes, body })
    }

    // ── Інструкції ──

    fn statement(&mut self) -> Result<Statement> {
        if self.match_token(&TokenKind::Повернути) {
            let value = if self.check(&TokenKind::ПраваФігурна) || self.check(&TokenKind::КрапкаЗКомою) {
                None
            } else {
                Some(self.expression()?)
            };
            Ok(Statement::Return(value))
        } else if self.match_token(&TokenKind::Якщо) {
            self.if_statement()
        } else if self.match_token(&TokenKind::Поки) {
            self.while_statement()
        } else if self.match_token(&TokenKind::Для) {
            self.for_statement()
        } else if self.match_token(&TokenKind::Переривати) {
            Ok(Statement::Break)
        } else if self.match_token(&TokenKind::Продовжити) {
            Ok(Statement::Continue)
        } else if self.match_token(&TokenKind::ЛіваФігурна) {
            self.block_statement()
        } else if self.match_token(&TokenKind::Спробувати) {
            self.try_catch_statement()
        } else if self.match_token(&TokenKind::Перевірити) {
            let expr = self.expression()?;
            Ok(Statement::Assert(expr))
        } else if self.match_token(&TokenKind::ЗОбробником) {
            let handler = self.consume_identifier("Очікувалось ім'я обробника")?;
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let body = Box::new(self.block_statement()?);
            Ok(Statement::WithHandler { handler, body })
        } else if self.match_token(&TokenKind::КомпЧас) {
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let mut stmts = Vec::new();
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                stmts.push(self.statement()?);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
            Ok(Statement::CompTime(stmts))
        } else if self.match_token(&TokenKind::Небезпечний) {
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let mut stmts = Vec::new();
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                stmts.push(self.statement()?);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
            Ok(Statement::Unsafe(stmts))
        } else if self.match_token(&TokenKind::Віддати) {
            let expr = self.expression()?;
            Ok(Statement::Yield(expr))
        } else if self.check_declaration() {
            Ok(Statement::Declaration(self.declaration()?))
        } else {
            self.expression_statement()
        }
    }

    fn if_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після 'якщо'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;

        let then_branch = Box::new(self.statement()?);
        let else_branch = if self.match_token(&TokenKind::Інакше) {
            Some(Box::new(self.statement()?))
        } else {
            None
        };

        Ok(Statement::If { condition, then_branch, else_branch })
    }

    fn while_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після 'поки'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
        let body = Box::new(self.statement()?);

        Ok(Statement::While { condition, body })
    }

    fn for_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після 'для'")?;

        let variable = self.consume_identifier("Очікувалось ім'я змінної циклу")?;

        // для (x в колекція) або для (x від a до b)
        if self.match_token(&TokenKind::В) {
            let iterable = self.expression()?;
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            let body = Box::new(self.statement()?);

            Ok(Statement::ForIn {
                pattern: Pattern::Binding(variable),
                iterable,
                body,
            })
        } else {
            self.consume(&TokenKind::Від, "Очікувалось 'від' або 'в'")?;
            let from = self.expression()?;
            self.consume(&TokenKind::До, "Очікувалось 'до'")?;
            let to = self.expression()?;

            let step = if self.match_token(&TokenKind::Через) {
                Some(self.expression()?)
            } else {
                None
            };

            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            let body = Box::new(self.statement()?);

            Ok(Statement::For { variable, from, to, step, body })
        }
    }

    fn block_statement(&mut self) -> Result<Statement> {
        let mut statements = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            statements.push(self.statement()?);
        }
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        Ok(Statement::Block(statements))
    }

    fn try_catch_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' після 'спробувати'")?;
        let try_body = Box::new(self.block_statement()?);

        let (catch_param, catch_body) = if self.match_token(&TokenKind::Зловити) {
            let param = self.consume_identifier("Очікувалось ім'я параметра помилки").ok();
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let body = Box::new(self.block_statement()?);
            (param, Some(body))
        } else {
            (None, None)
        };

        let finally_body = if self.match_token(&TokenKind::Нарешті) {
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            Some(Box::new(self.block_statement()?))
        } else {
            None
        };

        Ok(Statement::TryCatch { try_body, catch_param, catch_body, finally_body })
    }

    fn expression_statement(&mut self) -> Result<Statement> {
        let expr = self.expression()?;

        if let Some(op) = self.match_assignment_op() {
            let value = self.expression()?;
            Ok(Statement::Assignment { target: expr, value, op })
        } else {
            Ok(Statement::Expression(expr))
        }
    }

    // ── Вирази (з пріоритетом операторів) ──

    fn expression(&mut self) -> Result<Expression> {
        self.pipeline_expression()
    }

    /// Pipeline: вираз |> функція |> функція
    fn pipeline_expression(&mut self) -> Result<Expression> {
        let mut expr = self.or_expression()?;

        while self.match_token(&TokenKind::Конвеєр) {
            let right = self.or_expression()?;
            expr = Expression::Pipeline {
                left: Box::new(expr),
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn or_expression(&mut self) -> Result<Expression> {
        let mut expr = self.and_expression()?;
        while self.match_token(&TokenKind::Або) {
            let right = self.and_expression()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn and_expression(&mut self) -> Result<Expression> {
        let mut expr = self.equality_expression()?;
        while self.match_token(&TokenKind::І) {
            let right = self.equality_expression()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn equality_expression(&mut self) -> Result<Expression> {
        let mut expr = self.relational_expression()?;
        while let Some(op) = self.match_equality_op() {
            let right = self.relational_expression()?;
            expr = Expression::Binary { left: Box::new(expr), op, right: Box::new(right) };
        }
        Ok(expr)
    }

    fn relational_expression(&mut self) -> Result<Expression> {
        let mut expr = self.range_expression()?;
        while let Some(op) = self.match_relational_op() {
            let right = self.range_expression()?;
            expr = Expression::Binary { left: Box::new(expr), op, right: Box::new(right) };
        }
        Ok(expr)
    }

    /// Діапазони: a..b, a..=b
    fn range_expression(&mut self) -> Result<Expression> {
        let expr = self.bitwise_or_expression()?;

        if self.match_token(&TokenKind::ДіапазонВключ) {
            let to = self.bitwise_or_expression()?;
            return Ok(Expression::Range {
                from: Box::new(expr),
                to: Box::new(to),
                inclusive: true,
            });
        }

        if self.match_token(&TokenKind::Діапазон) {
            let to = self.bitwise_or_expression()?;
            return Ok(Expression::Range {
                from: Box::new(expr),
                to: Box::new(to),
                inclusive: false,
            });
        }

        Ok(expr)
    }

    /// Побітове OR: a | b (тільки коли не лямбда і не pipeline)
    fn bitwise_or_expression(&mut self) -> Result<Expression> {
        let mut expr = self.bitwise_xor_expression()?;
        // Note: | conflicts with lambda and pipeline, so we skip bitwise | here
        // Use explicit бітАбо() function instead
        Ok(expr)
    }

    /// Побітове XOR: a ^ b
    fn bitwise_xor_expression(&mut self) -> Result<Expression> {
        let mut expr = self.bitwise_and_expression()?;
        while self.match_token(&TokenKind::БітВиключне) {
            let right = self.bitwise_and_expression()?;
            expr = Expression::Binary { left: Box::new(expr), op: BinaryOp::BitXor, right: Box::new(right) };
        }
        Ok(expr)
    }

    /// Побітове AND: a & b (тільки коли не посилання)
    fn bitwise_and_expression(&mut self) -> Result<Expression> {
        let mut expr = self.shift_expression()?;
        // Note: & conflicts with references, skip bitwise & here
        Ok(expr)
    }

    /// Зсуви: a << b, a >> b
    fn shift_expression(&mut self) -> Result<Expression> {
        let mut expr = self.additive_expression()?;
        loop {
            if self.match_token(&TokenKind::ЗсувЛіво) {
                let right = self.additive_expression()?;
                expr = Expression::Binary { left: Box::new(expr), op: BinaryOp::Shl, right: Box::new(right) };
            } else if self.match_token(&TokenKind::ЗсувПраво) {
                let right = self.additive_expression()?;
                expr = Expression::Binary { left: Box::new(expr), op: BinaryOp::Shr, right: Box::new(right) };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn additive_expression(&mut self) -> Result<Expression> {
        let mut expr = self.multiplicative_expression()?;
        while let Some(op) = self.match_additive_op() {
            let right = self.multiplicative_expression()?;
            expr = Expression::Binary { left: Box::new(expr), op, right: Box::new(right) };
        }
        Ok(expr)
    }

    fn multiplicative_expression(&mut self) -> Result<Expression> {
        let mut expr = self.power_expression()?;
        while let Some(op) = self.match_multiplicative_op() {
            let right = self.power_expression()?;
            expr = Expression::Binary { left: Box::new(expr), op, right: Box::new(right) };
        }
        Ok(expr)
    }

    fn power_expression(&mut self) -> Result<Expression> {
        let mut expr = self.unary_expression()?;
        if self.match_token(&TokenKind::Степінь) {
            let right = self.power_expression()?; // Правоасоціативний
            expr = Expression::Binary {
                left: Box::new(expr),
                op: BinaryOp::Pow,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn unary_expression(&mut self) -> Result<Expression> {
        if let Some(op) = self.match_unary_op() {
            let operand = self.unary_expression()?;
            Ok(Expression::Unary { op, operand: Box::new(operand) })
        } else {
            self.postfix_expression()
        }
    }

    /// Постфіксні оператори: виклик, індексація, доступ до полів, ?
    fn postfix_expression(&mut self) -> Result<Expression> {
        let mut expr = self.primary()?;

        loop {
            if self.match_token(&TokenKind::ЛіваДужка) {
                // Виклик функції
                let mut args = Vec::new();
                if !self.check(&TokenKind::ПраваДужка) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_token(&TokenKind::Кома) { break; }
                    }
                }
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
                expr = Expression::Call { callee: Box::new(expr), args };
            } else if self.match_token(&TokenKind::ЛіваКвадратна) {
                // Індексація
                let index = self.expression()?;
                self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']'")?;
                expr = Expression::Index { object: Box::new(expr), index: Box::new(index) };
            } else if self.match_token(&TokenKind::Крапка) {
                // Доступ до поля або виклик методу
                let member = self.consume_identifier("Очікувалось ім'я поля або методу")?;
                if self.check(&TokenKind::ЛіваДужка) {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&TokenKind::ПраваДужка) {
                        loop {
                            args.push(self.expression()?);
                            if !self.match_token(&TokenKind::Кома) { break; }
                        }
                    }
                    self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
                    expr = Expression::MethodCall { object: Box::new(expr), method: member, args };
                } else {
                    expr = Expression::MemberAccess { object: Box::new(expr), member };
                }
            } else if self.match_token(&TokenKind::ЗнакПитання) {
                // Поширення помилки
                expr = Expression::ErrorPropagation(Box::new(expr));
            } else if self.match_token(&TokenKind::Як) {
                // Приведення типів
                let ty = self.parse_type()?;
                expr = Expression::Cast { expr: Box::new(expr), ty };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// Первинні вирази
    fn primary(&mut self) -> Result<Expression> {
        // Чекати (await)
        if self.match_token(&TokenKind::Чекати) {
            let expr = self.primary()?;
            return Ok(Expression::Await(Box::new(expr)));
        }

        // Літерали
        if let Some(lit) = self.match_literal() {
            return Ok(Expression::Literal(lit));
        }

        // Форматований рядок
        if self.check_format_string() {
            return self.parse_format_string();
        }

        // себе
        if self.match_token(&TokenKind::Себе) {
            return Ok(Expression::SelfRef);
        }

        // це
        if self.match_token(&TokenKind::Це) {
            return Ok(Expression::SelfRef);
        }

        // Лямбда: |параметри| вираз
        if self.check(&TokenKind::Вертикальна) {
            return self.parse_lambda();
        }

        // Зіставлення зразків: зіставити вираз { ... }
        if self.match_token(&TokenKind::Зіставити) {
            return self.parse_match_expression();
        }

        // Групування або кортеж: (вираз) або (a, b)
        if self.match_token(&TokenKind::ЛіваДужка) {
            let expr = self.expression()?;
            if self.match_token(&TokenKind::Кома) {
                // Кортеж
                let mut elements = vec![expr];
                loop {
                    elements.push(self.expression()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
                return Ok(Expression::Tuple(elements));
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            return Ok(expr);
        }

        // Масив: [елементи]
        if self.match_token(&TokenKind::ЛіваКвадратна) {
            let mut elements = Vec::new();
            if !self.check(&TokenKind::ПраваКвадратна) {
                loop {
                    elements.push(self.expression()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']'")?;
            return Ok(Expression::Array(elements));
        }

        // Ідентифікатор, конструктор структури/enum
        if self.check_identifier() {
            let name = self.consume_identifier("Очікувався ідентифікатор")?;

            // Конструктор структури: Назва { поле: значення }
            if self.check(&TokenKind::ЛіваФігурна) && self.is_struct_literal(&name) {
                self.advance();
                let mut fields = Vec::new();
                if !self.check(&TokenKind::ПраваФігурна) {
                    loop {
                        let field_name = self.consume_identifier("Очікувалось ім'я поля")?;
                        self.consume(&TokenKind::Двокрапка, "Очікувалась ':'")?;
                        let field_value = self.expression()?;
                        fields.push((field_name, field_value));
                        if !self.match_token(&TokenKind::Кома) { break; }
                    }
                }
                self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
                return Ok(Expression::Struct { name, fields });
            }

            // Шлях: модуль::елемент
            if self.check(&TokenKind::ПодвійнаДвокрапка) {
                let mut segments = vec![name];
                while self.match_token(&TokenKind::ПодвійнаДвокрапка) {
                    segments.push(self.consume_identifier("Очікувалось ім'я після '::'")?);
                }
                return Ok(Expression::Path { segments });
            }

            return Ok(Expression::Identifier(name));
        }

        Err(ParseError::InvalidExpression(self.peek().line).into())
    }

    /// Лямбда: |x, y| вираз  або  |x, y| { блок }
    fn parse_lambda(&mut self) -> Result<Expression> {
        self.consume(&TokenKind::Вертикальна, "Очікувалась '|'")?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::Вертикальна) {
            loop {
                let name = self.consume_identifier("Очікувалось ім'я параметра")?;
                let ty = if self.match_token(&TokenKind::Двокрапка) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(LambdaParam { name, ty });
                if !self.match_token(&TokenKind::Кома) { break; }
            }
        }

        self.consume(&TokenKind::Вертикальна, "Очікувалась '|'")?;

        if self.check(&TokenKind::ЛіваФігурна) {
            self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
            let mut body = Vec::new();
            while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                body.push(self.statement()?);
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
            Ok(Expression::LambdaBlock { params, body })
        } else {
            let body = self.expression()?;
            Ok(Expression::Lambda { params, body: Box::new(body) })
        }
    }

    /// зіставити вираз { зразок => вираз, ... }
    fn parse_match_expression(&mut self) -> Result<Expression> {
        let subject = self.expression()?;

        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' після зіставити")?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            let pattern = self.parse_pattern()?;
            self.consume(&TokenKind::ПодвійнаСтрілка, "Очікувалась '=>'")?;

            let body = if self.check(&TokenKind::ЛіваФігурна) {
                // Блок як тіло
                self.match_token(&TokenKind::ЛіваФігурна);
                let mut stmts = Vec::new();
                while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
                    stmts.push(self.statement()?);
                }
                self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
                // Останній вираз як результат
                if let Some(Statement::Expression(expr)) = stmts.last().cloned() {
                    expr
                } else {
                    Expression::Literal(Literal::Null)
                }
            } else {
                self.expression()?
            };

            arms.push(MatchArm { pattern, body });

            let _ = self.match_token(&TokenKind::Кома);
        }

        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;

        Ok(Expression::Match {
            subject: Box::new(subject),
            arms,
        })
    }

    /// Парсимо зразок (pattern)
    fn parse_pattern(&mut self) -> Result<Pattern> {
        let pattern = self.parse_single_pattern()?;

        // Guard: зразок якщо умова
        if self.match_token(&TokenKind::Якщо) {
            let condition = self.expression()?;
            return Ok(Pattern::Guard {
                pattern: Box::new(pattern),
                condition: Box::new(condition),
            });
        }

        // OR: A | B
        if self.check(&TokenKind::Вертикальна) {
            let mut patterns = vec![pattern];
            while self.match_token(&TokenKind::Вертикальна) {
                patterns.push(self.parse_single_pattern()?);
            }
            return Ok(Pattern::Or(patterns));
        }

        Ok(pattern)
    }

    fn parse_single_pattern(&mut self) -> Result<Pattern> {
        // Wildcard: _
        if self.match_token(&TokenKind::Підкреслення) {
            return Ok(Pattern::Wildcard);
        }

        // Літерали
        if let Some(lit) = self.match_literal() {
            return Ok(Pattern::Literal(lit));
        }

        // Кортеж: (a, b)
        if self.match_token(&TokenKind::ЛіваДужка) {
            let mut elements = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    elements.push(self.parse_pattern()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            return Ok(Pattern::Tuple(elements));
        }

        // Масив: [a, b, ..rest]
        if self.match_token(&TokenKind::ЛіваКвадратна) {
            let mut elements = Vec::new();
            let mut rest = None;
            if !self.check(&TokenKind::ПраваКвадратна) {
                loop {
                    if self.match_token(&TokenKind::Діапазон) {
                        rest = Some(self.consume_identifier("Очікувалось ім'я для решти")?);
                        break;
                    }
                    elements.push(self.parse_pattern()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']'")?;
            return Ok(Pattern::Array { elements, rest });
        }

        // Структура: { поле1, поле2, .. }
        if self.match_token(&TokenKind::ЛіваФігурна) {
            let mut fields = Vec::new();
            let mut rest = false;
            if !self.check(&TokenKind::ПраваФігурна) {
                loop {
                    if self.match_token(&TokenKind::Діапазон) {
                        rest = true;
                        break;
                    }
                    let name = self.consume_identifier("Очікувалось ім'я поля")?;
                    let sub_pattern = if self.match_token(&TokenKind::Двокрапка) {
                        Some(self.parse_pattern()?)
                    } else {
                        None
                    };
                    fields.push((name, sub_pattern));
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
            return Ok(Pattern::Struct { fields, rest });
        }

        // Ідентифікатор — або прив'язка або варіант enum
        if self.check_identifier() {
            let name = self.consume_identifier("Очікувалось ім'я")?;

            // Варіант з полями: Деякий(x)
            if self.match_token(&TokenKind::ЛіваДужка) {
                let mut fields = Vec::new();
                if !self.check(&TokenKind::ПраваДужка) {
                    loop {
                        fields.push(self.parse_pattern()?);
                        if !self.match_token(&TokenKind::Кома) { break; }
                    }
                }
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
                return Ok(Pattern::Variant { name, fields });
            }

            // Простий варіант без полів або прив'язка
            // Перша літера велика — варіант, маленька — прив'язка
            if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                return Ok(Pattern::Variant { name, fields: Vec::new() });
            }

            return Ok(Pattern::Binding(name));
        }

        Err(ParseError::InvalidPattern(self.peek().line).into())
    }

    fn parse_format_string(&mut self) -> Result<Expression> {
        if let TokenKind::ФормРядок(parts) = &self.peek().kind {
            let parts = parts.clone();
            self.advance();

            let format_parts: Vec<FormatPart> = parts.into_iter().map(|p| match p {
                StringPart::Text(s) => FormatPart::Text(s),
                StringPart::Expr(s) => {
                    // Парсимо вираз з рядка
                    let tokens = tryzub_lexer::tokenize(&s).unwrap_or_default();
                    let mut parser = Parser::new(tokens);
                    let expr = parser.expression().unwrap_or(Expression::Literal(Literal::Null));
                    FormatPart::Expr(expr)
                }
            }).collect();

            Ok(Expression::FormatString(format_parts))
        } else {
            Err(ParseError::InvalidExpression(self.peek().line).into())
        }
    }

    // ── Парсинг типів ──

    fn parse_type(&mut self) -> Result<Type> {
        // Себе
        if self.match_token(&TokenKind::Себе) {
            return Ok(Type::SelfType);
        }

        // Примітивні типи
        if self.match_token(&TokenKind::Цл8) { return Ok(Type::Цл8); }
        if self.match_token(&TokenKind::Цл16) { return Ok(Type::Цл16); }
        if self.match_token(&TokenKind::Цл32) { return Ok(Type::Цл32); }
        if self.match_token(&TokenKind::Цл64) { return Ok(Type::Цл64); }
        if self.match_token(&TokenKind::Чс8) { return Ok(Type::Чс8); }
        if self.match_token(&TokenKind::Чс16) { return Ok(Type::Чс16); }
        if self.match_token(&TokenKind::Чс32) { return Ok(Type::Чс32); }
        if self.match_token(&TokenKind::Чс64) { return Ok(Type::Чс64); }
        if self.match_token(&TokenKind::Дрб32) { return Ok(Type::Дрб32); }
        if self.match_token(&TokenKind::Дрб64) { return Ok(Type::Дрб64); }
        if self.match_token(&TokenKind::Лог) { return Ok(Type::Лог); }
        if self.match_token(&TokenKind::Сим) { return Ok(Type::Сим); }
        if self.match_token(&TokenKind::Тхт) { return Ok(Type::Тхт); }

        // Масив: [Тип]
        if self.match_token(&TokenKind::ЛіваКвадратна) {
            let elem_type = self.parse_type()?;
            self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']'")?;
            return Ok(Type::Slice(Box::new(elem_type)));
        }

        // Кортеж: (Тип1, Тип2)
        if self.match_token(&TokenKind::ЛіваДужка) {
            let mut types = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    types.push(self.parse_type()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            return Ok(Type::Tuple(types));
        }

        // Посилання: &Тип або &змін Тип
        if self.match_token(&TokenKind::Амперсанд) {
            let is_mut = self.match_token(&TokenKind::Змінна);
            let inner = self.parse_type()?;
            return Ok(Type::Reference(Box::new(inner), is_mut));
        }

        // Функціональний тип: функція(Т1, Т2) -> Р
        if self.match_token(&TokenKind::Функція) {
            self.consume(&TokenKind::ЛіваДужка, "Очікувалась '('")?;
            let mut param_types = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    param_types.push(self.parse_type()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
            }
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')'")?;
            let return_type = if self.match_token(&TokenKind::Стрілка) {
                Some(Box::new(self.parse_type()?))
            } else {
                None
            };
            return Ok(Type::Function(param_types, return_type));
        }

        // Іменований тип (можливо з generic параметрами)
        if self.check_identifier() {
            let name = self.consume_identifier("Очікувався тип")?;

            // Generic: Тип<Т1, Т2>
            if self.match_token(&TokenKind::Менше) {
                let mut type_params = Vec::new();
                loop {
                    type_params.push(self.parse_type()?);
                    if !self.match_token(&TokenKind::Кома) { break; }
                }
                self.consume(&TokenKind::Більше, "Очікувалась '>'")?;
                return Ok(Type::Generic(name, type_params));
            }

            return Ok(Type::Named(name));
        }

        Err(ParseError::InvalidExpression(self.peek().line).into())
    }

    fn parse_generic_params(&mut self) -> Result<Vec<String>> {
        let mut params = Vec::new();
        if self.match_token(&TokenKind::Менше) {
            loop {
                params.push(self.consume_identifier("Очікувалось ім'я generic параметра")?);
                if !self.match_token(&TokenKind::Кома) { break; }
            }
            self.consume(&TokenKind::Більше, "Очікувалась '>'")?;
        }
        Ok(params)
    }

    // ── Допоміжні методи ──

    fn match_literal(&mut self) -> Option<Literal> {
        let token = self.peek().clone();
        match &token.kind {
            TokenKind::ЦілеЧисло(n) => { let v = *n; self.advance(); Some(Literal::Integer(v)) }
            TokenKind::ДробовеЧисло(f) => { let v = *f; self.advance(); Some(Literal::Float(v)) }
            TokenKind::Рядок(s) => { let v = s.clone(); self.advance(); Some(Literal::String(v)) }
            TokenKind::Символ(c) => { let v = *c; self.advance(); Some(Literal::Char(v)) }
            TokenKind::Істина => { self.advance(); Some(Literal::Bool(true)) }
            TokenKind::Хиба => { self.advance(); Some(Literal::Bool(false)) }
            TokenKind::Нуль => { self.advance(); Some(Literal::Null) }
            _ => None,
        }
    }

    fn check_format_string(&self) -> bool {
        matches!(self.peek().kind, TokenKind::ФормРядок(_))
    }

    fn match_assignment_op(&mut self) -> Option<AssignmentOp> {
        if self.match_token(&TokenKind::Присвоїти) { Some(AssignmentOp::Assign) }
        else if self.match_token(&TokenKind::ПлюсПрисвоїти) { Some(AssignmentOp::AddAssign) }
        else if self.match_token(&TokenKind::МінусПрисвоїти) { Some(AssignmentOp::SubAssign) }
        else if self.match_token(&TokenKind::ПомножитиПрисвоїти) { Some(AssignmentOp::MulAssign) }
        else if self.match_token(&TokenKind::ПоділитиПрисвоїти) { Some(AssignmentOp::DivAssign) }
        else if self.match_token(&TokenKind::ЗалишокПрисвоїти) { Some(AssignmentOp::ModAssign) }
        else { None }
    }

    fn match_equality_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Дорівнює) { Some(BinaryOp::Eq) }
        else if self.match_token(&TokenKind::НеДорівнює) { Some(BinaryOp::Ne) }
        else { None }
    }

    fn match_relational_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Менше) { Some(BinaryOp::Lt) }
        else if self.match_token(&TokenKind::МеншеАбоДорівнює) { Some(BinaryOp::Le) }
        else if self.match_token(&TokenKind::Більше) { Some(BinaryOp::Gt) }
        else if self.match_token(&TokenKind::БільшеАбоДорівнює) { Some(BinaryOp::Ge) }
        else if self.match_token(&TokenKind::В) { Some(BinaryOp::In) }
        else { None }
    }

    fn match_additive_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Плюс) { Some(BinaryOp::Add) }
        else if self.match_token(&TokenKind::Мінус) { Some(BinaryOp::Sub) }
        else { None }
    }

    fn match_multiplicative_op(&mut self) -> Option<BinaryOp> {
        if self.match_token(&TokenKind::Помножити) { Some(BinaryOp::Mul) }
        else if self.match_token(&TokenKind::Поділити) { Some(BinaryOp::Div) }
        else if self.match_token(&TokenKind::Залишок) { Some(BinaryOp::Mod) }
        else { None }
    }

    fn match_unary_op(&mut self) -> Option<UnaryOp> {
        if self.match_token(&TokenKind::Мінус) { Some(UnaryOp::Neg) }
        else if self.match_token(&TokenKind::Не) { Some(UnaryOp::Not) }
        else if self.match_token(&TokenKind::БітНе) { Some(UnaryOp::BitNot) }
        else { None }
    }

    fn check_declaration(&self) -> bool {
        matches!(self.peek().kind,
            TokenKind::Змінна | TokenKind::Стала | TokenKind::Функція |
            TokenKind::Структура | TokenKind::Тип | TokenKind::Трейт |
            TokenKind::Реалізація | TokenKind::Модуль | TokenKind::Імпорт |
            TokenKind::Експорт | TokenKind::Інтерфейс |
            TokenKind::Публічний | TokenKind::Приватний | TokenKind::Асинхронний |
            TokenKind::Ефект | TokenKind::Макрос | TokenKind::Тест |
            TokenKind::Фаз | TokenKind::Бенчмарк | TokenKind::Чистий
        )
    }

    fn check_identifier(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ідентифікатор(_))
    }

    fn is_struct_literal(&self, name: &str) -> bool {
        // Вважаємо конструктором структури якщо ім'я з великої літери
        name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
    }

    fn peek_next_kind(&self) -> Option<TokenKind> {
        if self.current + 1 < self.tokens.len() {
            Some(self.tokens[self.current + 1].kind.clone())
        } else {
            None
        }
    }

    // ── Базові утиліти ──

    fn peek(&self) -> &Token {
        &self.tokens[self.current.min(self.tokens.len() - 1)]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        self.peek().kind == TokenKind::КінецьФайлу
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() { return false; }
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<&Token> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                expected: message.to_string(),
                found: format!("{:?}", self.peek().kind),
                line: self.peek().line,
            }.into())
        }
    }

    fn consume_identifier(&mut self, message: &str) -> Result<String> {
        if let TokenKind::Ідентифікатор(name) = &self.peek().kind {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            Err(ParseError::UnexpectedToken {
                expected: message.to_string(),
                found: format!("{:?}", self.peek().kind),
                line: self.peek().line,
            }.into())
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Program> {
    let mut parser = Parser::new(tokens);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tryzub_lexer::tokenize;

    #[test]
    fn test_parse_variable() {
        let tokens = tokenize("змінна x = 10").unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_function() {
        let source = "функція додати(а: цл32, б: цл32) -> цл32 { повернути а + б }";
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_enum() {
        let source = r#"
тип Опція<Т> {
    Деякий(Т),
    Нічого
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
        assert!(matches!(program.declarations[0], Declaration::Enum { .. }));
    }

    #[test]
    fn test_parse_trait() {
        let source = r#"
трейт Показуваний {
    функція показати(себе) -> тхт
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
        assert!(matches!(program.declarations[0], Declaration::Trait { .. }));
    }

    #[test]
    fn test_parse_match() {
        let source = r#"
функція головна() {
    змінна х = 5
    зіставити х {
        1 => друк("один"),
        2 => друк("два"),
        _ => друк("інше")
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_pipeline() {
        let source = r#"
функція головна() {
    змінна р = 5 |> подвоїти |> утроїти
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_for_in() {
        let source = r#"
функція головна() {
    для (х в 1..10) {
        друк(х)
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }
}
