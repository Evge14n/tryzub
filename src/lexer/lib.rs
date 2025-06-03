use anyhow::Result;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Літерали
    ЦілеЧисло(i64),
    ДробовеЧисло(f64),
    Рядок(String),
    Символ(char),
    Логічне(bool),
    
    // Ідентифікатори
    Ідентифікатор(String),
    
    // Ключові слова
    Змінна,
    Стала,
    Функція,
    Повернути,
    Якщо,
    Інакше,
    Поки,
    Для,
    Від,
    До,
    Через,
    Переривати,
    Продовжити,
    Структура,
    Модуль,
    Імпорт,
    Експорт,
    Тип,
    Інтерфейс,
    Реалізує,
    Приватний,
    Публічний,
    Статичний,
    Асинхронний,
    Чекати,
    Спробувати,
    Зловити,
    Нарешті,
    Новий,
    Це,
    Супер,
    Нуль,
    Істина,
    Хиба,
    
    // Типи даних
    Цл8, Цл16, Цл32, Цл64,
    Чс8, Чс16, Чс32, Чс64,
    Дрб32, Дрб64,
    Лог,
    Сим,
    Тхт,
    
    // Оператори
    Плюс,
    Мінус,
    Помножити,
    Поділити,
    Залишок,
    Степінь,
    
    // Порівняння
    Дорівнює,
    НеДорівнює,
    Менше,
    Більше,
    МеншеАбоДорівнює,
    БільшеАбоДорівнює,
    
    // Логічні
    І,
    Або,
    Не,
    
    // Присвоєння
    Присвоїти,
    ПлюсПрисвоїти,
    МінусПрисвоїти,
    ПомножитиПрисвоїти,
    ПоділитиПрисвоїти,
    
    // Розділові знаки
    ЛіваДужка,
    ПраваДужка,
    ЛіваФігурна,
    ПраваФігурна,
    ЛіваКвадратна,
    ПраваКвадратна,
    Крапка,
    Кома,
    Крапка з Комою,
    Двокрапка,
    Стрілка,
    ПодвійнаСтрілка,
    
    // Спеціальні
    НовийРядок,
    КінецьФайлу,
}

#[derive(Error, Debug)]
pub enum LexerError {
    #[error("Невідомий символ '{0}' на рядку {1}, позиції {2}")]
    НевідомийСимвол(char, usize, usize),
    
    #[error("Незавершений рядок на рядку {0}")]
    НезавершенийРядок(usize),
    
    #[error("Неправильне число '{0}' на рядку {1}")]
    НеправильнеЧисло(String, usize),
    
    #[error("Незавершений коментар на рядку {0}")]
    НезавершенийКоментар(usize),
}

pub struct Lexer {
    input: Vec<char>,
    current: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            current: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
        }
    }
    
    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        while !self.is_at_end() {
            self.skip_whitespace();
            if self.is_at_end() {
                break;
            }
            
            let start_column = self.column;
            let token = self.scan_token()?;
            
            if let Some(token) = token {
                self.tokens.push(token);
            }
        }
        
        self.tokens.push(Token {
            kind: TokenKind::КінецьФайлу,
            lexeme: String::new(),
            line: self.line,
            column: self.column,
        });
        
        Ok(self.tokens.clone())
    }
    
    fn scan_token(&mut self) -> Result<Option<Token>> {
        let start_column = self.column;
        let ch = self.advance();
        
        match ch {
            // Однолітерні токени
            '(' => Ok(Some(self.make_token(TokenKind::ЛіваДужка, start_column))),
            ')' => Ok(Some(self.make_token(TokenKind::ПраваДужка, start_column))),
            '{' => Ok(Some(self.make_token(TokenKind::ЛіваФігурна, start_column))),
            '}' => Ok(Some(self.make_token(TokenKind::ПраваФігурна, start_column))),
            '[' => Ok(Some(self.make_token(TokenKind::ЛіваКвадратна, start_column))),
            ']' => Ok(Some(self.make_token(TokenKind::ПраваКвадратна, start_column))),
            ',' => Ok(Some(self.make_token(TokenKind::Кома, start_column))),
            ';' => Ok(Some(self.make_token(TokenKind::КрапкаЗКомою, start_column))),
            ':' => Ok(Some(self.make_token(TokenKind::Двокрапка, start_column))),
            '.' => Ok(Some(self.make_token(TokenKind::Крапка, start_column))),
            
            // Оператори
            '+' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::ПлюсПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Плюс, start_column)))
                }
            }
            '-' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::МінусПрисвоїти, start_column)))
                } else if self.match_char('>') {
                    Ok(Some(self.make_token(TokenKind::Стрілка, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Мінус, start_column)))
                }
            }
            '*' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::ПомножитиПрисвоїти, start_column)))
                } else if self.match_char('*') {
                    Ok(Some(self.make_token(TokenKind::Степінь, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Помножити, start_column)))
                }
            }
            '/' => {
                if self.match_char('/') {
                    // Однорядковий коментар
                    while self.peek() != '\n' && !self.is_at_end() {
                        self.advance();
                    }
                    Ok(None)
                } else if self.match_char('*') {
                    // Багаторядковий коментар
                    self.skip_block_comment()?;
                    Ok(None)
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::ПоділитиПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Поділити, start_column)))
                }
            }
            '%' => Ok(Some(self.make_token(TokenKind::Залишок, start_column))),
            
            // Порівняння
            '=' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::Дорівнює, start_column)))
                } else if self.match_char('>') {
                    Ok(Some(self.make_token(TokenKind::ПодвійнаСтрілка, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Присвоїти, start_column)))
                }
            }
            '!' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::НеДорівнює, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Не, start_column)))
                }
            }
            '<' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::МеншеАбоДорівнює, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Менше, start_column)))
                }
            }
            '>' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::БільшеАбоДорівнює, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Більше, start_column)))
                }
            }
            '&' => {
                if self.match_char('&') {
                    Ok(Some(self.make_token(TokenKind::І, start_column)))
                } else {
                    Err(LexerError::НевідомийСимвол(ch, self.line, start_column).into())
                }
            }
            '|' => {
                if self.match_char('|') {
                    Ok(Some(self.make_token(TokenKind::Або, start_column)))
                } else {
                    Err(LexerError::НевідомийСимвол(ch, self.line, start_column).into())
                }
            }
            
            // Рядки
            '"' => self.scan_string(start_column),
            '\'' => self.scan_char(start_column),
            
            // Числа
            '0'..='9' => self.scan_number(start_column),
            
            // Ідентифікатори та ключові слова
            _ if ch.is_alphabetic() || ch == '_' => self.scan_identifier(start_column),
            
            _ => Err(LexerError::НевідомийСимвол(ch, self.line, start_column).into()),
        }
    }
    
    fn scan_string(&mut self, start_column: usize) -> Result<Option<Token>> {
        let mut value = String::new();
        
        while self.peek() != '"' && !self.is_at_end() {
            if self.peek() == '\n' {
                self.line += 1;
                self.column = 0;
            }
            if self.peek() == '\\' {
                self.advance();
                let escaped = match self.peek() {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    _ => self.peek(),
                };
                value.push(escaped);
                self.advance();
            } else {
                value.push(self.advance());
            }
        }
        
        if self.is_at_end() {
            return Err(LexerError::НезавершенийРядок(self.line).into());
        }
        
        self.advance(); // Закриваюча лапка
        
        Ok(Some(Token {
            kind: TokenKind::Рядок(value.clone()),
            lexeme: value,
            line: self.line,
            column: start_column,
        }))
    }
    
    fn scan_char(&mut self, start_column: usize) -> Result<Option<Token>> {
        let ch = if self.peek() == '\\' {
            self.advance();
            match self.peek() {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                _ => self.peek(),
            }
        } else {
            self.peek()
        };
        
        self.advance();
        
        if self.peek() != '\'' {
            return Err(LexerError::НевідомийСимвол(ch, self.line, start_column).into());
        }
        
        self.advance(); // Закриваюча лапка
        
        Ok(Some(Token {
            kind: TokenKind::Символ(ch),
            lexeme: ch.to_string(),
            line: self.line,
            column: start_column,
        }))
    }
    
    fn scan_number(&mut self, start_column: usize) -> Result<Option<Token>> {
        let mut value = String::new();
        value.push(self.previous());
        
        while self.peek().is_digit(10) {
            value.push(self.advance());
        }
        
        // Дробова частина
        if self.peek() == '.' && self.peek_next().is_digit(10) {
            value.push(self.advance()); // '.'
            while self.peek().is_digit(10) {
                value.push(self.advance());
            }
            
            let float_value = value.parse::<f64>()
                .map_err(|_| LexerError::НеправильнеЧисло(value.clone(), self.line))?;
            
            return Ok(Some(Token {
                kind: TokenKind::ДробовеЧисло(float_value),
                lexeme: value,
                line: self.line,
                column: start_column,
            }));
        }
        
        let int_value = value.parse::<i64>()
            .map_err(|_| LexerError::НеправильнеЧисло(value.clone(), self.line))?;
        
        Ok(Some(Token {
            kind: TokenKind::ЦілеЧисло(int_value),
            lexeme: value,
            line: self.line,
            column: start_column,
        }))
    }
    
    fn scan_identifier(&mut self, start_column: usize) -> Result<Option<Token>> {
        let mut value = String::new();
        value.push(self.previous());
        
        while self.peek().is_alphanumeric() || self.peek() == '_' || self.peek() == '\'' {
            value.push(self.advance());
        }
        
        let kind = match value.as_str() {
            "змінна" => TokenKind::Змінна,
            "стала" => TokenKind::Стала,
            "функція" => TokenKind::Функція,
            "повернути" => TokenKind::Повернути,
            "якщо" => TokenKind::Якщо,
            "інакше" => TokenKind::Інакше,
            "поки" => TokenKind::Поки,
            "для" => TokenKind::Для,
            "від" => TokenKind::Від,
            "до" => TokenKind::До,
            "через" => TokenKind::Через,
            "переривати" => TokenKind::Переривати,
            "продовжити" => TokenKind::Продовжити,
            "структура" => TokenKind::Структура,
            "модуль" => TokenKind::Модуль,
            "імпорт" => TokenKind::Імпорт,
            "експорт" => TokenKind::Експорт,
            "тип" => TokenKind::Тип,
            "інтерфейс" => TokenKind::Інтерфейс,
            "реалізує" => TokenKind::Реалізує,
            "приватний" => TokenKind::Приватний,
            "публічний" => TokenKind::Публічний,
            "статичний" => TokenKind::Статичний,
            "асинхронний" => TokenKind::Асинхронний,
            "чекати" => TokenKind::Чекати,
            "спробувати" => TokenKind::Спробувати,
            "зловити" => TokenKind::Зловити,
            "нарешті" => TokenKind::Нарешті,
            "новий" => TokenKind::Новий,
            "це" => TokenKind::Це,
            "супер" => TokenKind::Супер,
            "нуль" => TokenKind::Нуль,
            "істина" => TokenKind::Істина,
            "хиба" => TokenKind::Хиба,
            
            // Типи
            "цл8" => TokenKind::Цл8,
            "цл16" => TokenKind::Цл16,
            "цл32" => TokenKind::Цл32,
            "цл64" => TokenKind::Цл64,
            "чс8" => TokenKind::Чс8,
            "чс16" => TokenKind::Чс16,
            "чс32" => TokenKind::Чс32,
            "чс64" => TokenKind::Чс64,
            "дрб32" => TokenKind::Дрб32,
            "дрб64" => TokenKind::Дрб64,
            "лог" => TokenKind::Лог,
            "сим" => TokenKind::Сим,
            "тхт" => TokenKind::Тхт,
            
            _ => TokenKind::Ідентифікатор(value.clone()),
        };
        
        Ok(Some(Token {
            kind,
            lexeme: value,
            line: self.line,
            column: start_column,
        }))
    }
    
    fn skip_whitespace(&mut self) {
        while !self.is_at_end() {
            match self.peek() {
                ' ' | '\r' | '\t' => {
                    self.advance();
                }
                '\n' => {
                    self.line += 1;
                    self.column = 0;
                    self.advance();
                }
                _ => break,
            }
        }
    }
    
    fn skip_block_comment(&mut self) -> Result<()> {
        let start_line = self.line;
        let mut depth = 1;
        
        while depth > 0 && !self.is_at_end() {
            if self.peek() == '/' && self.peek_next() == '*' {
                self.advance();
                self.advance();
                depth += 1;
            } else if self.peek() == '*' && self.peek_next() == '/' {
                self.advance();
                self.advance();
                depth -= 1;
            } else {
                if self.peek() == '\n' {
                    self.line += 1;
                    self.column = 0;
                }
                self.advance();
            }
        }
        
        if depth > 0 {
            return Err(LexerError::НезавершенийКоментар(start_line).into());
        }
        
        Ok(())
    }
    
    fn make_token(&self, kind: TokenKind, column: usize) -> Token {
        Token {
            kind,
            lexeme: self.get_lexeme_for_kind(&kind),
            line: self.line,
            column,
        }
    }
    
    fn get_lexeme_for_kind(&self, kind: &TokenKind) -> String {
        match kind {
            TokenKind::ЛіваДужка => "(".to_string(),
            TokenKind::ПраваДужка => ")".to_string(),
            TokenKind::ЛіваФігурна => "{".to_string(),
            TokenKind::ПраваФігурна => "}".to_string(),
            TokenKind::ЛіваКвадратна => "[".to_string(),
            TokenKind::ПраваКвадратна => "]".to_string(),
            TokenKind::Крапка => ".".to_string(),
            TokenKind::Кома => ",".to_string(),
            TokenKind::КрапкаЗКомою => ";".to_string(),
            TokenKind::Двокрапка => ":".to_string(),
            TokenKind::Плюс => "+".to_string(),
            TokenKind::Мінус => "-".to_string(),
            TokenKind::Помножити => "*".to_string(),
            TokenKind::Поділити => "/".to_string(),
            TokenKind::Залишок => "%".to_string(),
            TokenKind::Степінь => "**".to_string(),
            TokenKind::Присвоїти => "=".to_string(),
            TokenKind::Дорівнює => "==".to_string(),
            TokenKind::НеДорівнює => "!=".to_string(),
            TokenKind::Менше => "<".to_string(),
            TokenKind::Більше => ">".to_string(),
            TokenKind::МеншеАбоДорівнює => "<=".to_string(),
            TokenKind::БільшеАбоДорівнює => ">=".to_string(),
            TokenKind::І => "&&".to_string(),
            TokenKind::Або => "||".to_string(),
            TokenKind::Не => "!".to_string(),
            TokenKind::Стрілка => "->".to_string(),
            TokenKind::ПодвійнаСтрілка => "=>".to_string(),
            _ => String::new(),
        }
    }
    
    fn advance(&mut self) -> char {
        let ch = self.current_char();
        self.current += 1;
        self.column += 1;
        ch
    }
    
    fn peek(&self) -> char {
        if self.is_at_end() {
            '\0'
        } else {
            self.input[self.current]
        }
    }
    
    fn peek_next(&self) -> char {
        if self.current + 1 >= self.input.len() {
            '\0'
        } else {
            self.input[self.current + 1]
        }
    }
    
    fn previous(&self) -> char {
        self.input[self.current - 1]
    }
    
    fn current_char(&self) -> char {
        self.input[self.current]
    }
    
    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() {
            return false;
        }
        if self.input[self.current] != expected {
            return false;
        }
        self.current += 1;
        self.column += 1;
        true
    }
    
    fn is_at_end(&self) -> bool {
        self.current >= self.input.len()
    }
}

pub fn tokenize(source: &str) -> Result<Vec<Token>> {
    let mut lexer = Lexer::new(source);
    lexer.tokenize()
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}:{}] {:?}: {}", self.line, self.column, self.kind, self.lexeme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let source = "змінна x = 10";
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens.len(), 5); // змінна, x, =, 10, EOF
    }
    
    #[test]
    fn test_string_literal() {
        let source = r#"змінна текст = "Привіт, світ!""#;
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens.len(), 5);
    }
    
    #[test]
    fn test_function() {
        let source = "функція додати(а: цл32, б: цл32) -> цл32 { повернути а + б }";
        let tokens = tokenize(source).unwrap();
        assert!(tokens.len() > 10);
    }
}
