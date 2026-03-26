// Мова програмування Тризуб v2.0
// Автор: Мартинюк Євген
// Copyright (c) 2025 Мартинюк Євген. Всі права захищені.
// Ліцензія: MIT

use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use std::fs;

#[derive(Parser)]
#[command(name = "tryzub")]
#[command(author = "Мартинюк Євген <evgenmart@gmail.com>")]
#[command(version = "3.6.0")]
#[command(about = "Тризуб — сучасна українська мова програмування 🔱")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Запустити файл через VM
    #[command(name = "запустити")]
    Run {
        /// Файл для запуску
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,

        /// Аргументи програми
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Перевірити синтаксис файлу
    #[command(name = "перевірити")]
    Check {
        /// Файл для перевірки
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// Створити новий проект
    #[command(name = "новий")]
    New {
        /// Назва проекту
        name: String,
    },

    /// Запустити тести у файлі
    #[command(name = "тестувати")]
    Test {
        /// Файл з тестами
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// Інтерактивний режим (REPL)
    #[command(name = "інтерактив")]
    Repl,

    /// Показати версію та інформацію
    #[command(name = "версія")]
    Version,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Run { file, args } => run_file(file, args),
        Commands::Check { file } => check_file(file),
        Commands::Test { file } => run_tests(file),
        Commands::New { name } => create_project(name),
        Commands::Repl => run_repl(),
        Commands::Version => {
            println!("🔱 Тризуб v3.6.0");
            println!("Автор: Мартинюк Євген");
            println!("Ліцензія: MIT");
            println!("https://github.com/Evge14n/tryzub");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("❌ {}", e);
        std::process::exit(1);
    }
}

fn run_file(file: PathBuf, args: Vec<String>) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    let tokens = tryzub_lexer::tokenize(&source)
        .map_err(|e| anyhow::anyhow!("Помилка лексичного аналізу: {}", e))?;

    let ast = tryzub_parser::parse(tokens)
        .map_err(|e| anyhow::anyhow!("Помилка синтаксичного аналізу: {}", e))?;

    tryzub_vm::execute(ast, args)
}

fn check_file(file: PathBuf) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    println!("🔍 Перевіряю: {:?}", file);

    let tokens = tryzub_lexer::tokenize(&source)?;
    println!("  ✓ Лексичний аналіз: {} токенів", tokens.len());

    let _ast = tryzub_parser::parse(tokens)?;
    println!("  ✓ Синтаксичний аналіз: OK");

    println!("✅ Файл синтаксично правильний");
    Ok(())
}

fn run_tests(file: PathBuf) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    let tokens = tryzub_lexer::tokenize(&source)?;
    let ast = tryzub_parser::parse(tokens)?;

    // Знаходимо всі тест-блоки
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    println!("🧪 Запуск тестів з {:?}\n", file);

    for decl in &ast.declarations {
        if let tryzub_parser::Declaration::Test { name, body } = decl {
            total += 1;
            // Створюємо VM для кожного тесту (ізольоване середовище)
            let test_program = tryzub_parser::Program {
                declarations: ast.declarations.iter()
                    .filter(|d| !matches!(d, tryzub_parser::Declaration::Test { .. }))
                    .cloned()
                    .chain(std::iter::once(tryzub_parser::Declaration::Function {
                        name: "головна".to_string(),
                        params: vec![],
                        return_type: None,
                        body: body.clone(),
                        is_async: false,
                        visibility: tryzub_parser::Visibility::Public,
                        contract: None,
                    }))
                    .collect(),
            };

            match tryzub_vm::execute(test_program, vec![]) {
                Ok(()) => {
                    passed += 1;
                    println!("  ✅ {}", name);
                }
                Err(e) => {
                    failed += 1;
                    println!("  ❌ {} — {}", name, e);
                }
            }
        }
    }

    println!("\n─────────────────────────────");
    println!("Всього: {} | Пройшли: {} | Провалені: {}", total, passed, failed);

    if failed > 0 {
        println!("\n❌ {} тестів провалено!", failed);
        std::process::exit(1);
    } else if total > 0 {
        println!("\n✅ Всі {} тестів пройшли!", total);
    } else {
        println!("\n⚠️ Тестів не знайдено");
    }

    Ok(())
}

fn run_repl() -> Result<()> {
    use std::io::{self, Write, BufRead};

    println!("🔱 Тризуб v3.6.0 — Інтерактивний режим");
    println!("Введіть вираз або інструкцію. :вихід для виходу.");
    println!("Команди: :тип <вираз>, :допомога");
    println!();

    // Збираємо декларації між введеннями
    let mut declarations_source = String::new();
    let stdin = io::stdin();

    loop {
        print!("тризуб> ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() || line.is_empty() {
            break;
        }
        let line = line.trim().to_string();

        if line.is_empty() {
            continue;
        }

        // Спеціальні команди
        if line == ":вихід" || line == ":quit" || line == ":q" {
            println!("До побачення! 🇺🇦");
            break;
        }

        if line == ":допомога" || line == ":help" {
            println!("  :тип <вираз>          — показати тип значення");
            println!("  :час <код>            — виміряти час виконання");
            println!("  :завантажити <файл>   — завантажити .тризуб файл");
            println!("  :очистити             — очистити контекст");
            println!("  :вихід                — вийти");
            println!("  Будь-який код         — виконати");
            continue;
        }

        if line == ":очистити" {
            declarations_source.clear();
            println!("Контекст очищено.");
            continue;
        }

        if line.starts_with(":тип ") {
            let expr = &line[":тип ".len()..];
            let full_source = format!(
                "{}\nфункція головна() {{ змінна __р = {} \n друк(тип_значення(__р)) }}",
                declarations_source, expr
            );
            match run_source(&full_source) {
                Ok(()) => {}
                Err(e) => println!("❌ {}", e),
            }
            continue;
        }

        if line.starts_with(":час ") {
            let code = &line[":час ".len()..];
            let full_source = format!(
                "{}\nфункція головна() {{ {} }}",
                declarations_source, code
            );
            let start = std::time::Instant::now();
            match run_source(&full_source) {
                Ok(()) => {
                    let elapsed = start.elapsed();
                    println!("⏱ {:.3}мс", elapsed.as_secs_f64() * 1000.0);
                }
                Err(e) => println!("❌ {}", e),
            }
            continue;
        }

        if line.starts_with(":завантажити ") {
            let path = line[":завантажити ".len()..].trim_matches('"');
            match fs::read_to_string(path) {
                Ok(source) => {
                    declarations_source.push('\n');
                    declarations_source.push_str(&source);
                    // Підрахуємо кількість декларацій
                    let funcs = source.matches("функція ").count();
                    let structs = source.matches("структура ").count();
                    let types = source.matches("тип ").count();
                    println!("✓ Завантажено з {}: {} функцій, {} структур, {} типів",
                        path, funcs, structs, types);
                }
                Err(e) => println!("❌ Не вдалося прочитати {}: {}", path, e),
            }
            continue;
        }

        // Перевіряємо чи це декларація (функція, структура, тип, тощо)
        let is_declaration = line.starts_with("функція ")
            || line.starts_with("структура ")
            || line.starts_with("тип ")
            || line.starts_with("трейт ")
            || line.starts_with("реалізація ")
            || line.starts_with("стала ")
            || line.starts_with("ефект ");

        if is_declaration {
            // Збираємо багаторядкову декларацію
            let mut full = line.clone();
            let mut brace_count: i32 = full.matches('{').count() as i32 - full.matches('}').count() as i32;
            while brace_count > 0 {
                print!("  ... ");
                io::stdout().flush().ok();
                let mut next_line = String::new();
                if stdin.lock().read_line(&mut next_line).is_err() { break; }
                full.push('\n');
                full.push_str(next_line.trim());
                brace_count += next_line.matches('{').count() as i32 - next_line.matches('}').count() as i32;
            }
            declarations_source.push('\n');
            declarations_source.push_str(&full);
            println!("  ✓ Додано");
            continue;
        }

        // Виконуємо як вираз/інструкцію
        let full_source = format!(
            "{}\nфункція головна() {{ {} }}",
            declarations_source, line
        );

        match run_source(&full_source) {
            Ok(()) => {}
            Err(e) => println!("❌ {}", e),
        }
    }

    Ok(())
}

fn run_source(source: &str) -> Result<()> {
    let tokens = tryzub_lexer::tokenize(source)?;
    let ast = tryzub_parser::parse(tokens)?;
    tryzub_vm::execute(ast, vec![])
}

fn create_project(name: String) -> Result<()> {
    fs::create_dir(&name)?;
    fs::create_dir(format!("{}/src", name))?;

    let main_content = format!(r#"// Проект: {}
// Створено за допомогою мови Тризуб v2.0

функція головна() {{
    друк("Привіт з проекту {}! 🇺🇦")

    // Приклад pattern matching
    змінна значення = Деякий(42)
    зіставити значення {{
        Деякий(н) => друк(ф"Знайдено: {{н}}"),
        Нічого => друк("Пусто")
    }}

    // Приклад діапазону
    для (і в 1..=5) {{
        друк(ф"  Крок {{і}}")
    }}
}}
"#, name, name);

    fs::write(format!("{}/src/головна.тризуб", name), main_content)?;

    let project_file = format!(r#"[проект]
назва = "{}"
версія = "0.1.0"
автор = ""
"#, name);

    fs::write(format!("{}/проект.toml", name), project_file)?;

    println!("✅ Проект '{}' створено", name);
    println!("📁 {}/", name);
    println!("   ├── проект.toml");
    println!("   └── src/");
    println!("       └── головна.тризуб");
    println!();
    println!("Запустити: tryzub запустити {}/src/головна.тризуб", name);

    Ok(())
}
