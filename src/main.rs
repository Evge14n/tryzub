// Мова програмування Тризуб v4.5
// Автор: *******
// Copyright (c) 2025 *******. Всі права захищені.
// Ліцензія: MIT

use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;
use std::fs;

#[derive(Parser)]
#[command(name = "tryzub")]
#[command(author = "******* <*******>")]
#[command(version = "4.5.0")]
#[command(about = "Тризуб — сучасна українська мова програмування ")]
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

    /// Веб-сервер команди
    #[command(name = "веб")]
    Web {
        #[command(subcommand)]
        action: WebCommands,
    },

    /// Бенчмарк VM — вимірює швидкість
    #[command(name = "бенчмарк")]
    Benchmark {
        /// Кількість ітерацій (за замовчуванням 1000000)
        #[arg(value_name = "ІТЕРАЦІЙ", default_value = "1000000")]
        iterations: u64,
    },

    /// Профілювання програми
    #[command(name = "профіль")]
    Profile {
        /// Файл для профілювання
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// Показати версію та інформацію
    #[command(name = "версія")]
    Version,
}

#[derive(Subcommand)]
enum WebCommands {
    /// Створити новий веб-проект
    #[command(name = "новий")]
    New {
        /// Назва проекту
        name: String,
    },

    /// Запустити веб-сервер (production)
    #[command(name = "запустити")]
    Run {
        /// Головний файл
        #[arg(default_value = "головна.тризуб")]
        file: PathBuf,

        /// Порт
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Run { file, args } => run_file(file, args),
        Commands::Check { file } => check_file(file),
        Commands::Test { file } => run_tests(file),
        Commands::New { name } => create_project(name),
        Commands::Repl => run_repl(),
        Commands::Web { action } => match action {
            WebCommands::New { name } => create_web_project(name),
            WebCommands::Run { file, port } => run_file(file, vec![port.to_string()]),
        },
        Commands::Benchmark { iterations } => {
            println!("\n🔱 Тризуб VM — Бенчмарк швидкості\n");
            let mut vm = tryzub_vm::VM::new();
            let _ = vm.call_builtin("бенчмарк_вбудований", vec![tryzub_vm::Value::Integer(iterations as i64)]);

            println!("\n  Тризуб-код:");
            let source = r#"
                змінна сума = 0
                для і в 1..10001 {
                    сума = сума + і
                }
            "#;
            let start = std::time::Instant::now();
            if let Ok(tokens) = tryzub_lexer::tokenize(source) {
                if let Ok(ast) = tryzub_parser::parse(tokens) {
                    let _ = vm.execute_program(ast, vec![]);
                }
            }
            let tryzub_time = start.elapsed();
            println!("  Сума 1..10000:  {:>8.2} мс", tryzub_time.as_secs_f64() * 1000.0);

            // Bytecode VM бенчмарк
            println!("\n  Bytecode VM:");
            let (bc_result, bc_time) = tryzub_vm::bytecode::benchmark_sum_bytecode(10001);
            println!("  Сума 1..10000:  {:>8.2} мс (результат: {})", bc_time.as_secs_f64() * 1000.0, bc_result);

            // Великий тест
            let (bc_result_big, bc_time_big) = tryzub_vm::bytecode::benchmark_sum_bytecode(10_000_001);
            println!("  Сума 1..10M:    {:>8.2} мс (результат: {})", bc_time_big.as_secs_f64() * 1000.0, bc_result_big);

            println!("\n  Порівняння (сума 1..10000):");
            println!("  Тризуб Tree:   {:>8.2} мс (AST interpreter + pattern opt)", tryzub_time.as_secs_f64() * 1000.0);
            println!("  Тризуб Byte:   {:>8.2} мс (Stack bytecode VM)", bc_time.as_secs_f64() * 1000.0);
            println!("  Python ~3.12:  ~  15-25 мс (CPython, GC overhead)");
            println!("  Ruby ~3.3:     ~  20-40 мс (YARV, GC)");
            println!("  Node.js ~22:   ~   2-5  мс (V8 JIT, GC)");
            println!("  Lua ~5.4:      ~   5-10 мс (Register VM)");
            println!("  C/Rust:        ~   0.01 мс (Native compiled)");

            Ok(())
        }
        Commands::Profile { file } => profile_file(file),
        Commands::Version => {
            println!("Тризуб v4.5.0");
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

fn profile_file(file: PathBuf) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;
    let tokens = tryzub_lexer::tokenize(&source)
        .map_err(|e| anyhow::anyhow!("Помилка лексичного аналізу: {}", e))?;
    let ast = tryzub_parser::parse(tokens)
        .map_err(|e| anyhow::anyhow!("Помилка синтаксичного аналізу: {}", e))?;

    let mut vm = tryzub_vm::VM::new();
    let start = std::time::Instant::now();
    vm.execute_program(ast, vec![])?;
    let elapsed = start.elapsed();

    if let Ok(stats) = vm.call_builtin("статистика_vm", vec![]) {
        println!("\n  📊 Профіль: {:?}", file);
        println!("  Час виконання: {:.2} мс", elapsed.as_secs_f64() * 1000.0);
        if let tryzub_vm::Value::Dict(pairs) = stats {
            for (k, v) in pairs {
                println!("  {}: {}", k.to_display_string(), v.to_display_string());
            }
        }
    }
    Ok(())
}

fn run_file(file: PathBuf, args: Vec<String>) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    let tokens = tryzub_lexer::tokenize(&source)
        .map_err(|e| anyhow::anyhow!("Помилка лексичного аналізу: {}", e))?;

    let ast = tryzub_parser::parse(tokens)
        .map_err(|e| anyhow::anyhow!("Помилка синтаксичного аналізу: {}", e))?;

    // Додаємо директорію файлу до шляхів пошуку модулів
    let mut vm = tryzub_vm::VM::new();
    if let Some(parent) = file.parent() {
        vm.add_module_path(parent.to_string_lossy().to_string());
    }
    vm.execute_program(ast, args)
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

        // Бенчмарки
        if let tryzub_parser::Declaration::Benchmark { name, body, .. } = decl {
            let test_program = tryzub_parser::Program {
                declarations: ast.declarations.iter()
                    .filter(|d| !matches!(d,
                        tryzub_parser::Declaration::Test { .. } |
                        tryzub_parser::Declaration::Benchmark { .. } |
                        tryzub_parser::Declaration::FuzzTest { .. }
                    ))
                    .cloned()
                    .chain(std::iter::once(tryzub_parser::Declaration::Function {
                        name: "головна".to_string(),
                        params: vec![], return_type: None,
                        body: body.clone(), is_async: false,
                        visibility: tryzub_parser::Visibility::Public,
                        contract: None,
                    }))
                    .collect(),
            };

            let start = std::time::Instant::now();
            let iterations = 100;
            for _ in 0..iterations {
                let _ = tryzub_vm::execute(test_program.clone(), vec![]);
            }
            let elapsed = start.elapsed();
            println!("  ⏱ {} — {:.1}мс/ітерація ({} ітерацій)", name,
                elapsed.as_secs_f64() * 1000.0 / iterations as f64, iterations);
        }

        // Фаз-тести
        if let tryzub_parser::Declaration::FuzzTest { name, body, .. } = decl {
            total += 1;
            let test_program = tryzub_parser::Program {
                declarations: ast.declarations.iter()
                    .filter(|d| !matches!(d,
                        tryzub_parser::Declaration::Test { .. } |
                        tryzub_parser::Declaration::Benchmark { .. } |
                        tryzub_parser::Declaration::FuzzTest { .. }
                    ))
                    .cloned()
                    .chain(std::iter::once(tryzub_parser::Declaration::Function {
                        name: "головна".to_string(),
                        params: vec![], return_type: None,
                        body: body.clone(), is_async: false,
                        visibility: tryzub_parser::Visibility::Public,
                        contract: None,
                    }))
                    .collect(),
            };

            // Запускаємо фаз-тест 50 разів з різними seed-ами
            let mut fuzz_passed = true;
            for i in 0..50 {
                match tryzub_vm::execute(test_program.clone(), vec![i.to_string()]) {
                    Ok(()) => {}
                    Err(e) => {
                        fuzz_passed = false;
                        println!("  ❌ {} (фаз ітерація {}) — {}", name, i, e);
                        break;
                    }
                }
            }
            if fuzz_passed {
                passed += 1;
                println!("  ✅ {} (50 фаз-ітерацій)", name);
            } else {
                failed += 1;
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

    println!("Тризуб v4.5.0 — Інтерактивний режим");
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
// Створено за допомогою мови Тризуб v4.5.0

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

fn create_web_project(name: String) -> Result<()> {
    fs::create_dir_all(format!("{}/шаблони/компоненти", name))?;
    fs::create_dir_all(format!("{}/статичні/css", name))?;
    fs::create_dir_all(format!("{}/статичні/js", name))?;
    fs::create_dir_all(format!("{}/статичні/img", name))?;
    fs::create_dir_all(format!("{}/маршрути", name))?;
    fs::create_dir_all(format!("{}/тести", name))?;

    let main_content = format!(r#"// {name} — Веб-додаток на Тризуб
// Запуск: тризуб веб запустити

бд_відкрити("{name}.db")

веб_сервер(3000)

веб_отримати("/", |запит| {{
    веб_html("<html><head><meta charset='utf-8'><title>{name}</title>
    <link rel='stylesheet' href='/css/стиль.css'>
    </head><body>
    <h1>Ласкаво просимо до {name}!</h1>
    <p>Веб-додаток працює на Тризуб Web</p>
    </body></html>")
}})

веб_отримати("/api/статус", |запит| {{
    веб_json(#{{"статус" -> "працює", "версія" -> "0.1.0"}})
}})

веб_статичні("статичні")

друк("Запускаю {name}...")
веб_запустити()
"#);

    fs::write(format!("{}/головна.тризуб", name), main_content)?;

    let css = r#"* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
       max-width: 900px; margin: 50px auto; padding: 0 20px; color: #333; }
h1 { color: #0057b7; margin-bottom: 20px; }
a { color: #0057b7; }
.card { background: #f8f9fa; padding: 20px; border-radius: 8px; margin: 15px 0; }
.btn { background: #0057b7; color: white; border: none; padding: 10px 24px;
       border-radius: 6px; cursor: pointer; font-size: 16px; }
.btn:hover { background: #004494; }
"#;
    fs::write(format!("{}/статичні/css/стиль.css", name), css)?;

    let base_template = r#"<!DOCTYPE html>
<html lang="uk">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{заголовок}</title>
    <link rel="stylesheet" href="/css/стиль.css">
</head>
<body>
    {включити "навігація"}
    <main>{зміст}</main>
    {включити "підвал"}
</body>
</html>"#;
    fs::write(format!("{}/шаблони/основа.тхтмл", name), base_template)?;

    let nav = r#"<nav style="display:flex;justify-content:space-between;align-items:center;padding:15px 0;border-bottom:1px solid #eee;margin-bottom:30px">
    <a href="/" style="font-size:1.3em;font-weight:bold;text-decoration:none">Тризуб Web</a>
    <div><a href="/">Головна</a> | <a href="/api/статус">API</a></div>
</nav>"#;
    fs::write(format!("{}/шаблони/компоненти/навігація.тхтмл", name), nav)?;

    let footer = r#"<footer style="margin-top:50px;padding:20px 0;border-top:1px solid #eee;text-align:center;color:#888">
    <p>Створено з Тризуб Web</p>
</footer>"#;
    fs::write(format!("{}/шаблони/компоненти/підвал.тхтмл", name), footer)?;

    let project_file = format!(r#"[проект]
назва = "{name}"
версія = "0.1.0"
тип = "веб"

[веб]
порт = 3000
статичні = "статичні"
шаблони = "шаблони"
"#);
    fs::write(format!("{}/проект.toml", name), project_file)?;

    println!("  Веб-проект '{}' створено", name);
    println!("  {}/", name);
    println!("  ├── головна.тризуб");
    println!("  ├── проект.toml");
    println!("  ├── шаблони/");
    println!("  │   ├── основа.тхтмл");
    println!("  │   └── компоненти/");
    println!("  │       ├── навігація.тхтмл");
    println!("  │       └── підвал.тхтмл");
    println!("  ├── статичні/");
    println!("  │   └── css/стиль.css");
    println!("  ├── маршрути/");
    println!("  └── тести/");
    println!();
    println!("  Запустити: cd {} && тризуб веб запустити", name);

    Ok(())
}
