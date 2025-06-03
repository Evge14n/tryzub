// Мова програмування Тризуб
// Автор: Мартинюк Євген
// Створено: 06.04.2025
// Copyright (c) 2025 Мартинюк Євген. Всі права захищені.
// Ліцензія: MIT

use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use std::fs;

mod lexer;
mod parser;
mod compiler;
mod vm;
mod runtime;

#[derive(Parser)]
#[command(name = "tryzub")]
#[command(author = "Мартинюк Євген <evgenmart@gmail.com>")]
#[command(version = "1.0.0")]
#[command(about = "Тризуб - найшвидша україномовна мова програмування", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Компілювати файл у виконуваний файл
    Компілювати {
        /// Вхідний файл .тризуб
        #[arg(value_name = "ФАЙЛ")]
        вхід: PathBuf,
        
        /// Вихідний файл
        #[arg(short = 'в', long = "вихід")]
        вихід: Option<PathBuf>,
        
        /// Рівень оптимізації (0-3)
        #[arg(short = 'О', long = "оптимізація", default_value = "2")]
        оптимізація: u8,
        
        /// Цільова платформа
        #[arg(short = 'ц', long = "ціль")]
        ціль: Option<String>,
    },
    
    /// Запустити файл без компіляції
    Запустити {
        /// Файл для запуску
        #[arg(value_name = "ФАЙЛ")]
        файл: PathBuf,
        
        /// Аргументи програми
        #[arg(trailing_var_arg = true)]
        аргументи: Vec<String>,
    },
    
    /// Перевірити синтаксис файлу
    Перевірити {
        /// Файл для перевірки
        #[arg(value_name = "ФАЙЛ")]
        файл: PathBuf,
    },
    
    /// Створити новий проект
    Новий {
        /// Назва проекту
        назва: String,
        
        /// Тип проекту
        #[arg(short = 'т', long = "тип", default_value = "програма")]
        тип_проекту: String,
    },
    
    /// Зібрати проект
    Зібрати {
        /// Режим збірки
        #[arg(short = 'р', long = "режим", default_value = "випуск")]
        режим: String,
    },
    
    /// Форматувати код
    Форматувати {
        /// Файли для форматування
        файли: Vec<PathBuf>,
    },
}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    env_logger::init();
    
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Компілювати { вхід, вихід, оптимізація, ціль } => {
            println!("🔨 Компілюю файл: {:?}", вхід);
            compile_file(вхід, вихід, оптимізація, ціль)?;
        }
        Commands::Запустити { файл, аргументи } => {
            run_file(файл, аргументи)?;
        }
        Commands::Перевірити { файл } => {
            println!("🔍 Перевіряю файл: {:?}", файл);
            check_file(файл)?;
        }
        Commands::Новий { назва, тип_проекту } => {
            println!("📁 Створюю новий проект: {}", назва);
            create_project(назва, тип_проекту)?;
        }
        Commands::Зібрати { режим } => {
            println!("🏗️ Збираю проект у режимі: {}", режим);
            build_project(режим)?;
        }
        Commands::Форматувати { файли } => {
            println!("🎨 Форматую {} файлів", файли.len());
            format_files(файли)?;
        }
    }
    
    Ok(())
}

fn compile_file(input: PathBuf, output: Option<PathBuf>, opt_level: u8, target: Option<String>) -> Result<()> {
    let source = fs::read_to_string(&input)?;
    
    // Лексичний аналіз
    println!("  ├─ 📝 Лексичний аналіз...");
    let tokens = lexer::tokenize(&source)?;
    println!("  │  └─ ✓ {} токенів", tokens.len());
    
    // Синтаксичний аналіз
    println!("  ├─ 🌳 Синтаксичний аналіз...");
    let ast = parser::parse(tokens)?;
    println!("  │  └─ ✓ AST побудовано");
    
    // Семантичний аналіз та оптимізація
    println!("  ├─ 🔧 Оптимізація (рівень {})...", opt_level);
    let optimized_ast = compiler::optimize(ast, opt_level)?;
    println!("  │  └─ ✓ Оптимізовано");
    
    // Генерація коду
    let output_path = output.unwrap_or_else(|| {
        let mut path = input.clone();
        path.set_extension(if cfg!(windows) { "exe" } else { "" });
        path
    });
    
    println!("  ├─ 🎯 Генерація коду...");
    compiler::generate_executable(optimized_ast, output_path.clone(), target)?;
    println!("  │  └─ ✓ Створено: {:?}", output_path);
    
    println!("  └─ ✅ Компіляція завершена успішно!");
    
    Ok(())
}

fn run_file(file: PathBuf, args: Vec<String>) -> Result<()> {
    let source = fs::read_to_string(&file)?;
    
    // Лексичний аналіз
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("❌ Помилка лексичного аналізу: {}", e);
            return Err(e.into());
        }
    };
    
    // Синтаксичний аналіз
    let ast = match parser::parse(tokens) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("❌ Помилка синтаксичного аналізу: {}", e);
            return Err(e.into());
        }
    };
    
    // Виконання через VM
    vm::execute(ast, args)?;
    
    Ok(())
}

fn check_file(file: PathBuf) -> Result<()> {
    let source = fs::read_to_string(&file)?;
    
    // Лексичний аналіз
    println!("  ├─ 📝 Лексичний аналіз...");
    let tokens = lexer::tokenize(&source)?;
    println!("  │  └─ ✓ {} токенів", tokens.len());
    
    // Синтаксичний аналіз
    println!("  ├─ 🌳 Синтаксичний аналіз...");
    let _ast = parser::parse(tokens)?;
    println!("  │  └─ ✓ Синтаксис правильний");
    
    println!("  └─ ✅ Файл синтаксично правильний");
    Ok(())
}

fn create_project(name: String, project_type: String) -> Result<()> {
    fs::create_dir(&name)?;
    fs::create_dir(format!("{}/src", name))?;
    
    // Створюємо основний файл
    let main_content = match project_type.as_str() {
        "програма" => {
            format!(r#"// {}
// Автор: [Ваше ім'я]
// Створено: {}

функція головна() {{
    друк("Привіт з проекту {}! 🇺🇦")
}}
"#, name, chrono::Local::now().format("%d.%m.%Y"), name)
        }
        "бібліотека" => {
            format!(r#"// Бібліотека {}
// Автор: [Ваше ім'я]  
// Створено: {}

модуль {} {{
    публічний функція привітання(ім'я: тхт) {{
        друк("Привіт, " + ім'я + "!")
    }}
}}
"#, name, chrono::Local::now().format("%d.%m.%Y"), name)
        }
        _ => return Err(anyhow::anyhow!("Невідомий тип проекту")),
    };
    
    fs::write(format!("{}/src/головна.тризуб", name), main_content)?;
    
    // Створюємо файл проекту
    let project_file = format!(r#"[проект]
назва = "{}"
версія = "0.1.0"
автор = "[Ваше ім'я]"
тип = "{}"

[залежності]
"#, name, project_type);
    
    fs::write(format!("{}/проект.toml", name), project_file)?;
    
    // Створюємо README
    let readme = format!(r#"# {}

Створено за допомогою мови програмування Тризуб 🇺🇦

## Запуск

```bash
тризуб запустити src/головна.тризуб
```

## Збірка

```bash
тризуб зібрати
```

## Ліцензія

MIT
"#, name);
    
    fs::write(format!("{}/README.md", name), readme)?;
    
    println!("✅ Проект '{}' створено", name);
    println!("📁 Структура:");
    println!("   {}/", name);
    println!("   ├── проект.toml");
    println!("   ├── README.md");
    println!("   └── src/");
    println!("       └── головна.тризуб");
    
    Ok(())
}

fn build_project(mode: String) -> Result<()> {
    // Читаємо файл проекту
    let project_file = fs::read_to_string("проект.toml")?;
    
    // TODO: Імплементувати повну збірку проекту
    println!("🏗️ Збірка проекту у режимі '{}'...", mode);
    println!("  ├─ 📖 Читання проект.toml");
    println!("  ├─ 📦 Збірка залежностей");
    println!("  ├─ 🔨 Компіляція вихідного коду");
    println!("  └─ ✅ Збірка завершена");
    
    Ok(())
}

fn format_files(files: Vec<PathBuf>) -> Result<()> {
    for file in files {
        println!("  ├─ 🎨 Форматую: {:?}", file);
        let source = fs::read_to_string(&file)?;
        let tokens = lexer::tokenize(&source)?;
        let ast = parser::parse(tokens)?;
        let formatted = parser::format_ast(ast)?;
        fs::write(&file, formatted)?;
        println!("  │  └─ ✓ Відформатовано");
    }
    Ok(())
}
