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
    ФормРядок(Vec<StringPart>), // ф"...{вираз}..."
    Символ(char),
    Логічне(bool),

    // Ідентифікатори
    Ідентифікатор(String),

    // ── Ключові слова: оголошення ──
    Змінна,
    Стала,
    Функція,
    Повернути,
    Структура,
    Модуль,
    Імпорт,
    Експорт,

    // ── Ключові слова: типи та трейти ──
    Тип,           // алгебраїчний тип (enum)
    Трейт,         // trait
    Реалізація,    // impl ... для ...
    Інтерфейс,     // interface (deprecated, use трейт)
    Реалізує,      // implements (deprecated)

    // ── Ключові слова: керування потоком ──
    Якщо,
    Інакше,
    Зіставити,     // match
    Поки,
    Для,
    В,             // in (для ітерації)
    Від,
    До,
    Через,
    Переривати,
    Продовжити,

    // ── Ключові слова: модифікатори ──
    Приватний,
    Публічний,
    Статичний,
    Асинхронний,
    Чекати,

    // ── Ключові слова: обробка помилок ──
    Спробувати,
    Зловити,
    Нарешті,

    // ── Ключові слова: OOP та посилання ──
    Новий,
    Це,
    Себе,          // self
    Супер,
    Для_Кого,      // "для" в контексті "реалізація X для Y"

    // ── Ключові слова: значення ──
    Нуль,
    Істина,
    Хиба,

    // ── Ключові слова: concurrency ──
    Потік,         // spawn thread
    Канал,         // channel

    // ── Ключові слова: системне програмування ──
    Небезпечний,   // unsafe блок
    Асемблер,      // asm! inline assembly
    Зовнішній,     // extern "C"
    Розмір,        // sizeof
    Зміщення,      // offsetof
    Вирівняний,    // align
    Упакований,    // packed struct
    Мінливий,      // volatile
    Статичний_Змін, // static mut
    Переривання,   // interrupt handler

    // ── Ключові слова: час компіляції ──
    КомпЧас,       // comptime (як Zig)
    Вбудований,    // inline
    НеВбудований,  // noinline
    Гарячий,       // hot path optimization hint
    Холодний,      // cold path optimization hint
    Макрос,        // macro definition

    // ── Ключові слова: система ефектів ──
    Чистий,        // pure function (no side effects)
    Ефект,         // effect declaration
    ЗОбробником,   // with handler block

    // ── Ключові слова: контракти ──
    Вимагає,       // requires (precondition)
    Гарантує,      // ensures (postcondition)
    Старе,         // old() — value before function call
    Інваріант,     // invariant (loop/type invariant)

    // ── Ключові слова: персистентність та пам'ять ──
    Персистентний, // persistent variable (survives reboot)
    Летючий,       // volatile memory (lost on reboot)
    Стійкий,       // persistent memory region
    Спільний,      // shared memory between processes

    // ── Ключові слова: capability security ──
    Можливість,    // capability token
    Дозвіл,        // permission
    Пісочниця,     // sandbox

    // ── Ключові слова: UI ──
    Вікно,         // window declaration
    Стовпець,      // column layout
    Рядок_UI,      // row layout (renamed to avoid conflict)
    Сітка,         // grid layout
    Кнопка,        // button
    ГарячіКлавіші, // hotkeys

    // ── Ключові слова: відлагодження ──
    Відлагодити,   // debug block
    Зупинка,       // breakpoint
    Назад,         // step back (time travel)
    Розгалужити,   // branch (what-if debugging)
    ЗнайтиМомент,  // find moment when condition

    // ── Ключові слова: тестування ──
    Тест,          // test block
    Фаз,           // fuzz test
    Бенчмарк,      // benchmark
    Перевірити,    // assert
    Виміряти,      // measure block

    // ── Ключові слова: інше ──
    Як,            // as (для import alias та type cast)
    Де,            // where (для generic constraints)

    // ── Типи даних ──
    Цл8, Цл16, Цл32, Цл64,
    Чс8, Чс16, Чс32, Чс64,
    Дрб32, Дрб64,
    Лог,
    Сим,
    Тхт,
    // Системні типи
    ЧсРозм,       // usize
    ЦлРозм,       // isize
    Вказівник,     // raw pointer *mut T / *const T
    Пусто,         // void / () / never type

    // ── Оператори: арифметичні ──
    Плюс,          // +
    Мінус,         // -
    Помножити,     // *
    Поділити,      // /
    Залишок,       // %
    Степінь,       // **

    // ── Оператори: порівняння ──
    Дорівнює,      // ==
    НеДорівнює,    // !=
    Менше,         // <
    Більше,        // >
    МеншеАбоДорівнює,  // <=
    БільшеАбоДорівнює, // >=

    // ── Оператори: логічні ──
    І,             // &&
    Або,           // ||
    Не,            // !

    // ── Оператори: присвоєння ──
    Присвоїти,     // =
    ПлюсПрисвоїти, // +=
    МінусПрисвоїти,// -=
    ПомножитиПрисвоїти, // *=
    ПоділитиПрисвоїти,  // /=
    ЗалишокПрисвоїти,   // %=

    // ── Оператори: побітові ──
    БітІ,          // & (bitwise AND) — в контексті виразів
    БітАбо,        // | (bitwise OR) — в контексті виразів
    БітВиключне,   // ^ (bitwise XOR)
    БітНе,         // ~ (bitwise NOT)
    ЗсувЛіво,      // << (left shift)
    ЗсувПраво,     // >> (right shift)
    БітІПрисвоїти, // &=
    БітАбоПрисвоїти, // |=
    БітВиключнеПрисвоїти, // ^=
    ЗсувЛівоПрисвоїти,    // <<=
    ЗсувПравоПрисвоїти,   // >>=

    // ── Оператори: спеціальні ──
    Конвеєр,       // |>  (pipeline)
    Діапазон,      // ..  (range exclusive)
    ДіапазонВключ, // ..= (range inclusive)
    ЗнакПитання,   // ?   (error propagation)
    Зірочка,       // * (dereference pointer)
    Решітка,       // # (для атрибутів #[...])
    Собака,        // @ (для анотацій)

    // ── Розділові знаки ──
    ЛіваДужка,     // (
    ПраваДужка,    // )
    ЛіваФігурна,   // {
    ПраваФігурна,  // }
    ЛіваКвадратна, // [
    ПраваКвадратна,// ]
    Крапка,        // .
    Кома,          // ,
    КрапкаЗКомою,  // ;
    Двокрапка,     // :
    ПодвійнаДвокрапка, // ::
    Стрілка,       // ->
    ПодвійнаСтрілка,   // =>
    Амперсанд,     // & (для посилань)
    Вертикальна,   // | (для лямбд та pattern matching)
    Підкреслення,  // _ (wildcard)
    ДвіКрапки,     // ..  (struct rest pattern)

    // ── Спеціальні ──
    НовийРядок,
    КінецьФайлу,
}

/// Частина форматованого рядка ф"..."
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Text(String),
    Expr(String),
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

    #[error("Незавершена інтерполяція рядка на рядку {0}")]
    НезавершенаІнтерполяція(usize),
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
            '?' => Ok(Some(self.make_token(TokenKind::ЗнакПитання, start_column))),
            '#' => Ok(Some(self.make_token(TokenKind::Решітка, start_column))),
            '@' => Ok(Some(self.make_token(TokenKind::Собака, start_column))),
            '_' if !self.peek().is_alphanumeric() => {
                Ok(Some(self.make_token(TokenKind::Підкреслення, start_column)))
            }

            // Двокрапка та подвійна двокрапка
            ':' => {
                if self.match_char(':') {
                    Ok(Some(self.make_token(TokenKind::ПодвійнаДвокрапка, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Двокрапка, start_column)))
                }
            }

            // Крапка та діапазони
            '.' => {
                if self.match_char('.') {
                    if self.match_char('=') {
                        Ok(Some(self.make_token(TokenKind::ДіапазонВключ, start_column)))
                    } else {
                        Ok(Some(self.make_token(TokenKind::Діапазон, start_column)))
                    }
                } else {
                    Ok(Some(self.make_token(TokenKind::Крапка, start_column)))
                }
            }

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
                    // Багаторядковий коментар (вкладені)
                    self.skip_block_comment()?;
                    Ok(None)
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::ПоділитиПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Поділити, start_column)))
                }
            }
            '%' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::ЗалишокПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Залишок, start_column)))
                }
            }

            // Порівняння та присвоєння
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
                if self.match_char('<') {
                    if self.match_char('=') {
                        Ok(Some(self.make_token(TokenKind::ЗсувЛівоПрисвоїти, start_column)))
                    } else {
                        Ok(Some(self.make_token(TokenKind::ЗсувЛіво, start_column)))
                    }
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::МеншеАбоДорівнює, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Менше, start_column)))
                }
            }
            '>' => {
                if self.match_char('>') {
                    if self.match_char('=') {
                        Ok(Some(self.make_token(TokenKind::ЗсувПравоПрисвоїти, start_column)))
                    } else {
                        Ok(Some(self.make_token(TokenKind::ЗсувПраво, start_column)))
                    }
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::БільшеАбоДорівнює, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Більше, start_column)))
                }
            }
            '^' => {
                if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::БітВиключнеПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::БітВиключне, start_column)))
                }
            }
            '~' => Ok(Some(self.make_token(TokenKind::БітНе, start_column))),

            // Логічні та спеціальні
            '&' => {
                if self.match_char('&') {
                    Ok(Some(self.make_token(TokenKind::І, start_column)))
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::БітІПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Амперсанд, start_column)))
                }
            }
            '|' => {
                if self.match_char('|') {
                    Ok(Some(self.make_token(TokenKind::Або, start_column)))
                } else if self.match_char('>') {
                    Ok(Some(self.make_token(TokenKind::Конвеєр, start_column)))
                } else if self.match_char('=') {
                    Ok(Some(self.make_token(TokenKind::БітАбоПрисвоїти, start_column)))
                } else {
                    Ok(Some(self.make_token(TokenKind::Вертикальна, start_column)))
                }
            }

            // Рядки
            '"' => self.scan_string(start_column),
            '\'' => self.scan_char(start_column),

            // Числа
            '0'..='9' => self.scan_number(start_column),

            // Ідентифікатори та ключові слова
            // Спеціальна обробка 'ф"...' для форматованих рядків
            _ if ch == '\u{0444}' && self.peek() == '"' => {
                self.advance(); // Пропускаємо '"'
                self.scan_format_string(start_column)
            }

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
                    '0' => '\0',
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

    /// Сканує форматований рядок ф"текст {вираз} текст"
    fn scan_format_string(&mut self, start_column: usize) -> Result<Option<Token>> {
        let mut parts = Vec::new();
        let mut current_text = String::new();

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
                    '{' => '{',
                    '}' => '}',
                    _ => self.peek(),
                };
                current_text.push(escaped);
                self.advance();
            } else if self.peek() == '{' {
                self.advance(); // Пропускаємо '{'

                // Зберігаємо поточний текст
                if !current_text.is_empty() {
                    parts.push(StringPart::Text(current_text.clone()));
                    current_text.clear();
                }

                // Читаємо вираз до '}'
                let mut expr = String::new();
                let mut brace_depth = 1;
                while brace_depth > 0 && !self.is_at_end() {
                    if self.peek() == '{' {
                        brace_depth += 1;
                    } else if self.peek() == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        }
                    }
                    expr.push(self.advance());
                }

                if self.is_at_end() {
                    return Err(LexerError::НезавершенаІнтерполяція(self.line).into());
                }

                self.advance(); // Пропускаємо '}'
                parts.push(StringPart::Expr(expr));
            } else {
                current_text.push(self.advance());
            }
        }

        if self.is_at_end() {
            return Err(LexerError::НезавершенийРядок(self.line).into());
        }

        // Зберігаємо останній текст
        if !current_text.is_empty() {
            parts.push(StringPart::Text(current_text));
        }

        self.advance(); // Закриваюча лапка

        Ok(Some(Token {
            kind: TokenKind::ФормРядок(parts.clone()),
            lexeme: format!("ф\"...\""),
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
                '0' => '\0',
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

        // Підтримка hex (0x), octal (0o), binary (0b)
        if self.previous() == '0' {
            match self.peek() {
                'x' | 'X' => {
                    value.push(self.advance());
                    while self.peek().is_ascii_hexdigit() || self.peek() == '_' {
                        if self.peek() != '_' {
                            value.push(self.advance());
                        } else {
                            self.advance();
                        }
                    }
                    let hex_str = &value[2..];
                    let int_value = i64::from_str_radix(hex_str, 16)
                        .map_err(|_| LexerError::НеправильнеЧисло(value.clone(), self.line))?;
                    return Ok(Some(Token {
                        kind: TokenKind::ЦілеЧисло(int_value),
                        lexeme: value,
                        line: self.line,
                        column: start_column,
                    }));
                }
                'o' | 'O' => {
                    value.push(self.advance());
                    while self.peek() >= '0' && self.peek() <= '7' || self.peek() == '_' {
                        if self.peek() != '_' {
                            value.push(self.advance());
                        } else {
                            self.advance();
                        }
                    }
                    let oct_str = &value[2..];
                    let int_value = i64::from_str_radix(oct_str, 8)
                        .map_err(|_| LexerError::НеправильнеЧисло(value.clone(), self.line))?;
                    return Ok(Some(Token {
                        kind: TokenKind::ЦілеЧисло(int_value),
                        lexeme: value,
                        line: self.line,
                        column: start_column,
                    }));
                }
                'b' | 'B' => {
                    value.push(self.advance());
                    while self.peek() == '0' || self.peek() == '1' || self.peek() == '_' {
                        if self.peek() != '_' {
                            value.push(self.advance());
                        } else {
                            self.advance();
                        }
                    }
                    let bin_str = &value[2..];
                    let int_value = i64::from_str_radix(bin_str, 2)
                        .map_err(|_| LexerError::НеправильнеЧисло(value.clone(), self.line))?;
                    return Ok(Some(Token {
                        kind: TokenKind::ЦілеЧисло(int_value),
                        lexeme: value,
                        line: self.line,
                        column: start_column,
                    }));
                }
                _ => {}
            }
        }

        while self.peek().is_digit(10) || self.peek() == '_' {
            if self.peek() != '_' {
                value.push(self.advance());
            } else {
                self.advance(); // Пропускаємо роздільник _
            }
        }

        // Дробова частина
        if self.peek() == '.' && self.peek_next().is_digit(10) {
            value.push(self.advance()); // '.'
            while self.peek().is_digit(10) || self.peek() == '_' {
                if self.peek() != '_' {
                    value.push(self.advance());
                } else {
                    self.advance();
                }
            }

            // Наукова нотація (1.5e10, 2.0E-3)
            if self.peek() == 'e' || self.peek() == 'E' {
                value.push(self.advance());
                if self.peek() == '+' || self.peek() == '-' {
                    value.push(self.advance());
                }
                while self.peek().is_digit(10) {
                    value.push(self.advance());
                }
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

        // Наукова нотація без дробової частини (1e5)
        if self.peek() == 'e' || self.peek() == 'E' {
            value.push(self.advance());
            if self.peek() == '+' || self.peek() == '-' {
                value.push(self.advance());
            }
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
            // Оголошення
            "змінна" => TokenKind::Змінна,
            "стала" => TokenKind::Стала,
            "функція" => TokenKind::Функція,
            "повернути" => TokenKind::Повернути,
            "структура" => TokenKind::Структура,
            "модуль" => TokenKind::Модуль,
            "імпорт" => TokenKind::Імпорт,
            "експорт" => TokenKind::Експорт,

            // Типи та трейти
            "тип" => TokenKind::Тип,
            "трейт" => TokenKind::Трейт,
            "реалізація" => TokenKind::Реалізація,
            "інтерфейс" => TokenKind::Інтерфейс,
            "реалізує" => TokenKind::Реалізує,

            // Керування потоком
            "якщо" => TokenKind::Якщо,
            "інакше" => TokenKind::Інакше,
            "зіставити" => TokenKind::Зіставити,
            "поки" => TokenKind::Поки,
            "для" => TokenKind::Для,
            "в" => TokenKind::В,
            "від" => TokenKind::Від,
            "до" => TokenKind::До,
            "через" => TokenKind::Через,
            "переривати" => TokenKind::Переривати,
            "продовжити" => TokenKind::Продовжити,

            // Модифікатори
            "приватний" => TokenKind::Приватний,
            "публічний" => TokenKind::Публічний,
            "статичний" => TokenKind::Статичний,
            "асинхронний" => TokenKind::Асинхронний,
            "чекати" => TokenKind::Чекати,

            // Обробка помилок
            "спробувати" => TokenKind::Спробувати,
            "зловити" => TokenKind::Зловити,
            "нарешті" => TokenKind::Нарешті,

            // OOP та посилання
            "новий" => TokenKind::Новий,
            "це" => TokenKind::Це,
            "себе" => TokenKind::Себе,
            "супер" => TokenKind::Супер,

            // Значення
            "нуль" => TokenKind::Нуль,
            "істина" => TokenKind::Істина,
            "хиба" => TokenKind::Хиба,

            // Concurrency
            "потік" => TokenKind::Потік,
            "канал" => TokenKind::Канал,

            // Системне програмування
            "небезпечний" => TokenKind::Небезпечний,
            "асемблер" => TokenKind::Асемблер,
            "зовнішній" => TokenKind::Зовнішній,
            "розмір" => TokenKind::Розмір,
            "зміщення" => TokenKind::Зміщення,
            "вирівняний" => TokenKind::Вирівняний,
            "упакований" => TokenKind::Упакований,
            "мінливий" => TokenKind::Мінливий,
            "переривання" => TokenKind::Переривання,

            // Час компіляції та оптимізація
            "компчас" => TokenKind::КомпЧас,
            "вбудований" => TokenKind::Вбудований,
            "невбудований" => TokenKind::НеВбудований,
            "гарячий" => TokenKind::Гарячий,
            "холодний" => TokenKind::Холодний,
            "макрос" => TokenKind::Макрос,

            // Система ефектів
            "чистий" => TokenKind::Чистий,
            "ефект" => TokenKind::Ефект,
            "з_обробником" => TokenKind::ЗОбробником,

            // Контракти
            "вимагає" => TokenKind::Вимагає,
            "гарантує" => TokenKind::Гарантує,
            "старе" => TokenKind::Старе,
            "інваріант" => TokenKind::Інваріант,

            // Тестування
            // Персистентність
            "персистентний" => TokenKind::Персистентний,
            "летючий" => TokenKind::Летючий,
            "стійкий" => TokenKind::Стійкий,
            "спільний" => TokenKind::Спільний,

            // Security
            "можливість" => TokenKind::Можливість,
            "дозвіл" => TokenKind::Дозвіл,
            "пісочниця" => TokenKind::Пісочниця,

            // UI
            "вікно" => TokenKind::Вікно,
            "стовпець" => TokenKind::Стовпець,
            "сітка" => TokenKind::Сітка,
            "кнопка" => TokenKind::Кнопка,

            // Відлагодження
            "відлагодити" => TokenKind::Відлагодити,
            "зупинка" => TokenKind::Зупинка,
            "назад" => TokenKind::Назад,
            "розгалужити" => TokenKind::Розгалужити,

            // Тестування
            "тест" => TokenKind::Тест,
            "фаз" => TokenKind::Фаз,
            "бенчмарк" => TokenKind::Бенчмарк,
            "перевірити" => TokenKind::Перевірити,
            "виміряти" => TokenKind::Виміряти,

            // Інше
            "як" => TokenKind::Як,
            "де" => TokenKind::Де,

            // Типи даних
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
            "чсрозм" => TokenKind::ЧсРозм,
            "цлрозм" => TokenKind::ЦлРозм,
            "вказівник" => TokenKind::Вказівник,
            "пусто" => TokenKind::Пусто,

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
        let lexeme = self.get_lexeme_for_kind(&kind);
        Token {
            kind,
            lexeme,
            line: self.line,
            column,
        }
    }

    fn get_lexeme_for_kind(&self, kind: &TokenKind) -> String {
        match kind {
            TokenKind::ЛіваДужка => "(",
            TokenKind::ПраваДужка => ")",
            TokenKind::ЛіваФігурна => "{",
            TokenKind::ПраваФігурна => "}",
            TokenKind::ЛіваКвадратна => "[",
            TokenKind::ПраваКвадратна => "]",
            TokenKind::Крапка => ".",
            TokenKind::Кома => ",",
            TokenKind::КрапкаЗКомою => ";",
            TokenKind::Двокрапка => ":",
            TokenKind::ПодвійнаДвокрапка => "::",
            TokenKind::Плюс => "+",
            TokenKind::Мінус => "-",
            TokenKind::Помножити => "*",
            TokenKind::Поділити => "/",
            TokenKind::Залишок => "%",
            TokenKind::Степінь => "**",
            TokenKind::Присвоїти => "=",
            TokenKind::ПлюсПрисвоїти => "+=",
            TokenKind::МінусПрисвоїти => "-=",
            TokenKind::ПомножитиПрисвоїти => "*=",
            TokenKind::ПоділитиПрисвоїти => "/=",
            TokenKind::ЗалишокПрисвоїти => "%=",
            TokenKind::Дорівнює => "==",
            TokenKind::НеДорівнює => "!=",
            TokenKind::Менше => "<",
            TokenKind::Більше => ">",
            TokenKind::МеншеАбоДорівнює => "<=",
            TokenKind::БільшеАбоДорівнює => ">=",
            TokenKind::І => "&&",
            TokenKind::Або => "||",
            TokenKind::Не => "!",
            TokenKind::Стрілка => "->",
            TokenKind::ПодвійнаСтрілка => "=>",
            TokenKind::Конвеєр => "|>",
            TokenKind::Діапазон => "..",
            TokenKind::ДіапазонВключ => "..=",
            TokenKind::ЗнакПитання => "?",
            TokenKind::Амперсанд => "&",
            TokenKind::Вертикальна => "|",
            TokenKind::Підкреслення => "_",
            _ => "",
        }.to_string()
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

    #[test]
    fn test_pipeline_operator() {
        let source = "x |> f |> g";
        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Конвеєр));
    }

    #[test]
    fn test_range_operators() {
        let source = "1..10";
        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Діапазон));

        let source2 = "1..=10";
        let tokens2 = tokenize(source2).unwrap();
        assert!(tokens2.iter().any(|t| t.kind == TokenKind::ДіапазонВключ));
    }

    #[test]
    fn test_question_mark() {
        let source = "результат?";
        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::ЗнакПитання));
    }

    #[test]
    fn test_new_keywords() {
        let source = "зіставити трейт реалізація себе";
        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Зіставити));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Трейт));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Реалізація));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Себе));
    }

    #[test]
    fn test_lambda_pipes() {
        let source = "|x| x + 1";
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Вертикальна);
        assert_eq!(tokens[2].kind, TokenKind::Вертикальна);
    }

    #[test]
    fn test_hex_binary_octal() {
        let tokens = tokenize("0xFF").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::ЦілеЧисло(255));

        let tokens = tokenize("0b1010").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::ЦілеЧисло(10));

        let tokens = tokenize("0o77").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::ЦілеЧисло(63));
    }

    #[test]
    fn test_number_separators() {
        let tokens = tokenize("1_000_000").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::ЦілеЧисло(1000000));
    }

    #[test]
    fn test_scientific_notation() {
        let tokens = tokenize("1.5e10").unwrap();
        if let TokenKind::ДробовеЧисло(f) = tokens[0].kind {
            assert!((f - 1.5e10).abs() < 1.0);
        } else {
            panic!("Expected float");
        }
    }

    #[test]
    fn test_double_colon() {
        let tokens = tokenize("модуль::функція").unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::ПодвійнаДвокрапка));
    }
}
