use anyhow::Result;
use thiserror::Error;
use tryzub_lexer::{Token, TokenKind};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Declaration>,
}

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
    },
    Struct {
        name: String,
        fields: Vec<Field>,
        visibility: Visibility,
    },
    Module {
        name: String,
        declarations: Vec<Declaration>,
        visibility: Visibility,
    },
    Import {
        path: Vec<String>,
        alias: Option<String>,
    },
    TypeAlias {
        name: String,
        ty: Type,
        visibility: Visibility,
    },
    Interface {
        name: String,
        methods: Vec<InterfaceMethod>,
        visibility: Visibility,
    },
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
    Reference(Box<Type>, bool), // bool = is_mutable
    Function(Vec<Type>, Option<Box<Type>>),
    Named(String),
    Optional(Box<Type>),
    Result(Box<Type>, Box<Type>),
}

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
    Break,
    Continue,
    Assignment {
        target: Expression,
        value: Expression,
        op: AssignmentOp,
    },
    Declaration(Declaration),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal),
    Identifier(String),
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
    Array(Vec<Expression>),
    Struct {
        name: String,
        fields: Vec<(String, Expression)>,
    },
    Lambda {
        params: Vec<Parameter>,
        return_type: Option<Type>,
        body: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    Cast {
        expr: Box<Expression>,
        ty: Type,
    },
    Await(Box<Expression>),
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

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg, Not,
}

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
        } else if self.match_token(&TokenKind::Модуль) {
            self.module_declaration(visibility)
        } else if self.match_token(&TokenKind::Імпорт) {
            self.import_declaration()
        } else if self.match_token(&TokenKind::Тип) {
            self.type_alias_declaration(visibility)
        } else if self.match_token(&TokenKind::Інтерфейс) {
            self.interface_declaration(visibility)
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
        })
    }
    
    fn struct_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я структури")?;
        
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' після імені структури")?;
        
        let mut fields = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            let field_visibility = if self.match_token(&TokenKind::Публічний) {
                Visibility::Public
            } else {
                Visibility::Private
            };
            
            let field_name = self.consume_identifier("Очікувалось ім'я поля")?;
            self.consume(&TokenKind::Двокрапка, "Очікувалась ':' після імені поля")?;
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
        
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}' після полів структури")?;
        
        Ok(Declaration::Struct { name, fields, visibility })
    }
    
    fn module_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я модуля")?;
        
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' після імені модуля")?;
        
        let mut declarations = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            declarations.push(self.declaration()?);
        }
        
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}' після тіла модуля")?;
        
        Ok(Declaration::Module { name, declarations, visibility })
    }
    
    fn import_declaration(&mut self) -> Result<Declaration> {
        let mut path = vec![self.consume_identifier("Очікувався шлях імпорту")?];
        
        while self.match_token(&TokenKind::Крапка) {
            path.push(self.consume_identifier("Очікувалось ім'я після '.'")?);
        }
        
        let alias = if self.match_token(&TokenKind::Як) {
            Some(self.consume_identifier("Очікувався псевдонім")?)
        } else {
            None
        };
        
        Ok(Declaration::Import { path, alias })
    }
    
    fn type_alias_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я типу")?;
        self.consume(&TokenKind::Присвоїти, "Очікувалось '=' після імені типу")?;
        let ty = self.parse_type()?;
        
        Ok(Declaration::TypeAlias { name, ty, visibility })
    }
    
    fn interface_declaration(&mut self, visibility: Visibility) -> Result<Declaration> {
        let name = self.consume_identifier("Очікувалось ім'я інтерфейсу")?;
        
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{' після імені інтерфейсу")?;
        
        let mut methods = Vec::new();
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            self.consume(&TokenKind::Функція, "Очікувалась 'функція' в інтерфейсі")?;
            let method_name = self.consume_identifier("Очікувалось ім'я методу")?;
            
            self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після імені методу")?;
            
            let mut params = Vec::new();
            if !self.check(&TokenKind::ПраваДужка) {
                loop {
                    let param_name = self.consume_identifier("Очікувалось ім'я параметра")?;
                    self.consume(&TokenKind::Двокрапка, "Очікувалась ':' після імені параметра")?;
                    let param_type = self.parse_type()?;
                    
                    params.push(Parameter {
                        name: param_name,
                        ty: param_type,
                        default: None,
                    });
                    
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
            
            methods.push(InterfaceMethod {
                name: method_name,
                params,
                return_type,
            });
            
            if !self.match_token(&TokenKind::Кома) && !self.check(&TokenKind::ПраваФігурна) {
                return Err(ParseError::UnexpectedToken {
                    expected: "',' або '}'".to_string(),
                    found: format!("{:?}", self.peek().kind),
                    line: self.peek().line,
                }.into());
            }
        }
        
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}' після методів інтерфейсу")?;
        
        Ok(Declaration::Interface { name, methods, visibility })
    }
    
    fn statement(&mut self) -> Result<Statement> {
        if self.match_token(&TokenKind::Повернути) {
            let value = if self.check(&TokenKind::КрапкаЗКомою) {
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
        } else if self.check_declaration() {
            Ok(Statement::Declaration(self.declaration()?))
        } else {
            self.expression_statement()
        }
    }
    
    fn if_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після 'якщо'")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після умови")?;
        
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
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після умови")?;
        
        let body = Box::new(self.statement()?);
        
        Ok(Statement::While { condition, body })
    }
    
    fn for_statement(&mut self) -> Result<Statement> {
        self.consume(&TokenKind::ЛіваДужка, "Очікувалась '(' після 'для'")?;
        
        let variable = self.consume_identifier("Очікувалось ім'я змінної циклу")?;
        
        self.consume(&TokenKind::Від, "Очікувалось 'від'")?;
        let from = self.expression()?;
        
        self.consume(&TokenKind::До, "Очікувалось 'до'")?;
        let to = self.expression()?;
        
        let step = if self.match_token(&TokenKind::Через) {
            Some(self.expression()?)
        } else {
            None
        };
        
        self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після параметрів циклу")?;
        
        let body = Box::new(self.statement()?);
        
        Ok(Statement::For { variable, from, to, step, body })
    }
    
    fn block_statement(&mut self) -> Result<Statement> {
        let mut statements = Vec::new();
        
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            statements.push(self.statement()?);
        }
        
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}'")?;
        
        Ok(Statement::Block(statements))
    }
    
    fn expression_statement(&mut self) -> Result<Statement> {
        let expr = self.expression()?;
        
        // Перевіряємо чи це присвоєння
        if let Some(op) = self.match_assignment_op() {
            let value = self.expression()?;
            Ok(Statement::Assignment { target: expr, value, op })
        } else {
            Ok(Statement::Expression(expr))
        }
    }
    
    fn expression(&mut self) -> Result<Expression> {
        self.or_expression()
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
            expr = Expression::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        
        Ok(expr)
    }
    
    fn relational_expression(&mut self) -> Result<Expression> {
        let mut expr = self.additive_expression()?;
        
        while let Some(op) = self.match_relational_op() {
            let right = self.additive_expression()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        
        Ok(expr)
    }
    
    fn additive_expression(&mut self) -> Result<Expression> {
        let mut expr = self.multiplicative_expression()?;
        
        while let Some(op) = self.match_additive_op() {
            let right = self.multiplicative_expression()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        
        Ok(expr)
    }
    
    fn multiplicative_expression(&mut self) -> Result<Expression> {
        let mut expr = self.power_expression()?;
        
        while let Some(op) = self.match_multiplicative_op() {
            let right = self.power_expression()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
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
            Ok(Expression::Unary {
                op,
                operand: Box::new(operand),
            })
        } else {
            self.postfix_expression()
        }
    }
    
    fn postfix_expression(&mut self) -> Result<Expression> {
        let mut expr = self.primary_expression()?;
        
        loop {
            if self.match_token(&TokenKind::ЛіваДужка) {
                // Виклик функції
                let mut args = Vec::new();
                
                if !self.check(&TokenKind::ПраваДужка) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_token(&TokenKind::Кома) {
                            break;
                        }
                    }
                }
                
                self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після аргументів")?;
                
                expr = Expression::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.match_token(&TokenKind::ЛіваКвадратна) {
                // Індексація
                let index = self.expression()?;
                self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']' після індексу")?;
                
                expr = Expression::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.match_token(&TokenKind::Крапка) {
                // Доступ до члена
                let member = self.consume_identifier("Очікувалось ім'я члена")?;
                
                expr = Expression::MemberAccess {
                    object: Box::new(expr),
                    member,
                };
            } else {
                break;
            }
        }
        
        Ok(expr)
    }
    
    fn primary_expression(&mut self) -> Result<Expression> {
        // Літерали
        if let Some(literal) = self.match_literal() {
            return Ok(Expression::Literal(literal));
        }
        
        // Ідентифікатор
        if let TokenKind::Ідентифікатор(name) = &self.peek().kind {
            let name = name.clone();
            self.advance();
            
            // Перевіряємо чи це створення структури
            if self.check(&TokenKind::ЛіваФігурна) {
                return self.struct_expression(name);
            }
            
            return Ok(Expression::Identifier(name));
        }
        
        // Групування
        if self.match_token(&TokenKind::ЛіваДужка) {
            let expr = self.expression()?;
            self.consume(&TokenKind::ПраваДужка, "Очікувалась ')' після виразу")?;
            return Ok(expr);
        }
        
        // Масив
        if self.match_token(&TokenKind::ЛіваКвадратна) {
            return self.array_expression();
        }
        
        // Лямбда
        if self.check(&TokenKind::ПодвійнаСтрілка) {
            return self.lambda_expression();
        }
        
        // Await
        if self.match_token(&TokenKind::Чекати) {
            let expr = self.unary_expression()?;
            return Ok(Expression::Await(Box::new(expr)));
        }
        
        Err(ParseError::InvalidExpression(self.peek().line).into())
    }
    
    fn struct_expression(&mut self, name: String) -> Result<Expression> {
        self.consume(&TokenKind::ЛіваФігурна, "Очікувалась '{'")?;
        
        let mut fields = Vec::new();
        
        while !self.check(&TokenKind::ПраваФігурна) && !self.is_at_end() {
            let field_name = self.consume_identifier("Очікувалось ім'я поля")?;
            self.consume(&TokenKind::Двокрапка, "Очікувалась ':' після імені поля")?;
            let field_value = self.expression()?;
            
            fields.push((field_name, field_value));
            
            if !self.match_token(&TokenKind::Кома) {
                break;
            }
        }
        
        self.consume(&TokenKind::ПраваФігурна, "Очікувалась '}' після полів")?;
        
        Ok(Expression::Struct { name, fields })
    }
    
    fn array_expression(&mut self) -> Result<Expression> {
        let mut elements = Vec::new();
        
        if !self.check(&TokenKind::ПраваКвадратна) {
            loop {
                elements.push(self.expression()?);
                if !self.match_token(&TokenKind::Кома) {
                    break;
                }
            }
        }
        
        self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']' після елементів масиву")?;
        
        Ok(Expression::Array(elements))
    }
    
    fn lambda_expression(&mut self) -> Result<Expression> {
        // Спрощений синтаксис: |a, b| => a + b
        self.consume(&TokenKind::Або, "Очікувалась '|' перед параметрами лямбди")?;
        
        let mut params = Vec::new();
        
        if !self.check(&TokenKind::Або) {
            loop {
                let name = self.consume_identifier("Очікувалось ім'я параметра")?;
                let ty = if self.match_token(&TokenKind::Двокрапка) {
                    self.parse_type()?
                } else {
                    Type::Named("auto".to_string()) // Виведення типу
                };
                
                params.push(Parameter {
                    name,
                    ty,
                    default: None,
                });
                
                if !self.match_token(&TokenKind::Кома) {
                    break;
                }
            }
        }
        
        self.consume(&TokenKind::Або, "Очікувалась '|' після параметрів лямбди")?;
        
        let return_type = if self.match_token(&TokenKind::Стрілка) {
            Some(self.parse_type()?)
        } else {
            None
        };
        
        self.consume(&TokenKind::ПодвійнаСтрілка, "Очікувалась '=>' перед тілом лямбди")?;
        
        let body = Box::new(self.expression()?);
        
        Ok(Expression::Lambda { params, return_type, body })
    }
    
    fn parse_type(&mut self) -> Result<Type> {
        let base_type = match &self.peek().kind {
            TokenKind::Цл8 => { self.advance(); Type::Цл8 }
            TokenKind::Цл16 => { self.advance(); Type::Цл16 }
            TokenKind::Цл32 => { self.advance(); Type::Цл32 }
            TokenKind::Цл64 => { self.advance(); Type::Цл64 }
            TokenKind::Чс8 => { self.advance(); Type::Чс8 }
            TokenKind::Чс16 => { self.advance(); Type::Чс16 }
            TokenKind::Чс32 => { self.advance(); Type::Чс32 }
            TokenKind::Чс64 => { self.advance(); Type::Чс64 }
            TokenKind::Дрб32 => { self.advance(); Type::Дрб32 }
            TokenKind::Дрб64 => { self.advance(); Type::Дрб64 }
            TokenKind::Лог => { self.advance(); Type::Лог }
            TokenKind::Сим => { self.advance(); Type::Сим }
            TokenKind::Тхт => { self.advance(); Type::Тхт }
            TokenKind::Ідентифікатор(name) => {
                let name = name.clone();
                self.advance();
                Type::Named(name)
            }
            _ => return Err(ParseError::UnexpectedToken {
                expected: "тип".to_string(),
                found: format!("{:?}", self.peek().kind),
                line: self.peek().line,
            }.into()),
        };
        
        // Перевіряємо чи це масив
        if self.match_token(&TokenKind::ЛіваКвадратна) {
            if let TokenKind::ЦілеЧисло(size) = &self.peek().kind {
                let size = *size as usize;
                self.advance();
                self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']' після розміру масиву")?;
                return Ok(Type::Array(Box::new(base_type), size));
            } else {
                self.consume(&TokenKind::ПраваКвадратна, "Очікувалась ']' для слайсу")?;
                return Ok(Type::Slice(Box::new(base_type)));
            }
        }
        
        Ok(base_type)
    }
    
    fn match_literal(&mut self) -> Option<Literal> {
        match &self.peek().kind {
            TokenKind::ЦілеЧисло(n) => {
                let n = *n;
                self.advance();
                Some(Literal::Integer(n))
            }
            TokenKind::ДробовеЧисло(n) => {
                let n = *n;
                self.advance();
                Some(Literal::Float(n))
            }
            TokenKind::Рядок(s) => {
                let s = s.clone();
                self.advance();
                Some(Literal::String(s))
            }
            TokenKind::Символ(c) => {
                let c = *c;
                self.advance();
                Some(Literal::Char(c))
            }
            TokenKind::Істина => {
                self.advance();
                Some(Literal::Bool(true))
            }
            TokenKind::Хиба => {
                self.advance();
                Some(Literal::Bool(false))
            }
            TokenKind::Нуль => {
                self.advance();
                Some(Literal::Null)
            }
            _ => None,
        }
    }
    
    fn match_assignment_op(&mut self) -> Option<AssignmentOp> {
        match &self.peek().kind {
            TokenKind::Присвоїти => {
                self.advance();
                Some(AssignmentOp::Assign)
            }
            TokenKind::ПлюсПрисвоїти => {
                self.advance();
                Some(AssignmentOp::AddAssign)
            }
            TokenKind::МінусПрисвоїти => {
                self.advance();
                Some(AssignmentOp::SubAssign)
            }
            TokenKind::ПомножитиПрисвоїти => {
                self.advance();
                Some(AssignmentOp::MulAssign)
            }
            TokenKind::ПоділитиПрисвоїти => {
                self.advance();
                Some(AssignmentOp::DivAssign)
            }
            _ => None,
        }
    }
    
    fn match_equality_op(&mut self) -> Option<BinaryOp> {
        match &self.peek().kind {
            TokenKind::Дорівнює => {
                self.advance();
                Some(BinaryOp::Eq)
            }
            TokenKind::НеДорівнює => {
                self.advance();
                Some(BinaryOp::Ne)
            }
            _ => None,
        }
    }
    
    fn match_relational_op(&mut self) -> Option<BinaryOp> {
        match &self.peek().kind {
            TokenKind::Менше => {
                self.advance();
                Some(BinaryOp::Lt)
            }
            TokenKind::МеншеАбоДорівнює => {
                self.advance();
                Some(BinaryOp::Le)
            }
            TokenKind::Більше => {
                self.advance();
                Some(BinaryOp::Gt)
            }
            TokenKind::БільшеАбоДорівнює => {
                self.advance();
                Some(BinaryOp::Ge)
            }
            _ => None,
        }
    }
    
    fn match_additive_op(&mut self) -> Option<BinaryOp> {
        match &self.peek().kind {
            TokenKind::Плюс => {
                self.advance();
                Some(BinaryOp::Add)
            }
            TokenKind::Мінус => {
                self.advance();
                Some(BinaryOp::Sub)
            }
            _ => None,
        }
    }
    
    fn match_multiplicative_op(&mut self) -> Option<BinaryOp> {
        match &self.peek().kind {
            TokenKind::Помножити => {
                self.advance();
                Some(BinaryOp::Mul)
            }
            TokenKind::Поділити => {
                self.advance();
                Some(BinaryOp::Div)
            }
            TokenKind::Залишок => {
                self.advance();
                Some(BinaryOp::Mod)
            }
            _ => None,
        }
    }
    
    fn match_unary_op(&mut self) -> Option<UnaryOp> {
        match &self.peek().kind {
            TokenKind::Мінус => {
                self.advance();
                Some(UnaryOp::Neg)
            }
            TokenKind::Не => {
                self.advance();
                Some(UnaryOp::Not)
            }
            _ => None,
        }
    }
    
    fn check_declaration(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Змінна | TokenKind::Стала | TokenKind::Функція |
            TokenKind::Структура | TokenKind::Модуль | TokenKind::Імпорт |
            TokenKind::Тип | TokenKind::Інтерфейс | TokenKind::Публічний |
            TokenKind::Приватний | TokenKind::Асинхронний
        )
    }
    
    fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<Token> {
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
    
    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }
    
    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            false
        } else {
            std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
        }
    }
    
    fn advance(&mut self) -> Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }
    
    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::КінецьФайлу)
    }
    
    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }
    
    fn previous(&self) -> Token {
        self.tokens[self.current - 1].clone()
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Program> {
    let mut parser = Parser::new(tokens);
    parser.parse()
}

pub fn format_ast(_ast: Program) -> Result<String> {
    // TODO: Implement AST formatting
    Ok("// Відформатований код\n".to_string())
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for decl in &self.declarations {
            writeln!(f, "{:?}", decl)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tryzub_lexer::tokenize;

    #[test]
    fn test_parse_variable() {
        let source = "змінна x: цл32 = 10";
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_function() {
        let source = r#"
функція додати(а: цл32, б: цл32) -> цл32 {
    повернути а + б
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        assert_eq!(program.declarations.len(), 1);
    }

    #[test]
    fn test_parse_if_statement() {
        let source = r#"
функція тест() {
    якщо (x > 10) {
        друк("Більше")
    } інакше {
        друк("Менше")
    }
}
"#;
        let tokens = tokenize(source).unwrap();
        let _program = parse(tokens).unwrap();
    }
}
