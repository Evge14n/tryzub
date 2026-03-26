// Мова програмування Тризуб v3.5
// Автор: Мартинюк Євген
// Copyright (c) 2025 Мартинюк Євген. Всі права захищені.

pub mod lexer {
    pub use tryzub_lexer::*;
}

pub mod parser {
    pub use tryzub_parser::*;
}

pub mod vm {
    pub use tryzub_vm::*;
}

pub const VERSION: &str = "4.0.0";
pub const AUTHOR: &str = "Мартинюк Євген";

pub fn about() -> String {
    format!(
        "🔱 Тризуб v{}\nАвтор: {}\nСучасна українська мова програмування",
        VERSION, AUTHOR
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(VERSION, "4.0.0");
    }
}
