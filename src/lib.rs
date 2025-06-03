// Мова програмування Тризуб
// Автор: Мартинюк Євген
// Створено: 06.04.2025
// Copyright (c) 2025 Мартинюк Євген. Всі права захищені.

//! # Тризуб - Українська мова програмування
//! 
//! Найшвидша україномовна мова програмування у світі.
//! 
//! ## Приклад
//! 
//! ```tryzub
//! функція головна() {
//!     друк("Привіт, світ!")
//! }
//! ```

pub use lexer::*;
pub use parser::*;
pub use compiler::*;
pub use vm::*;
pub use runtime::*;

pub mod lexer {
    pub use tryzub_lexer::*;
}

pub mod parser {
    pub use tryzub_parser::*;
}

pub mod compiler {
    pub use tryzub_compiler::*;
}

pub mod vm {
    pub use tryzub_vm::*;
}

pub mod runtime {
    pub use tryzub_runtime::*;
}

/// Версія мови Тризуб
pub const VERSION: &str = "1.0.0";

/// Автор мови
pub const AUTHOR: &str = "Мартинюк Євген";

/// Дата створення
pub const CREATED: &str = "06.04.2025";

/// Інформація про мову
pub fn about() -> String {
    format!(
        "Тризуб v{}\nАвтор: {}\nСтворено: {}\n\n🇺🇦 Найшвидша україномовна мова програмування",
        VERSION, AUTHOR, CREATED
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(VERSION, "1.0.0");
    }

    #[test]
    fn test_author() {
        assert_eq!(AUTHOR, "Мартинюк Євген");
    }
}
