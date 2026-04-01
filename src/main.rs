// Мова програмування Тризуб v9.0.0
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
#[command(version = "5.9.0")]
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

        /// Bytecode VM (швидше, обмежений набір операцій)
        #[arg(long = "швидко", default_value = "false")]
        fast: bool,

        /// JIT компіляція в машинний код x86_64
        #[arg(long = "jit", default_value = "false")]
        jit: bool,

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

    /// Встановити залежності
    #[command(name = "встановити")]
    Install {
        /// Git URL або назва пакету (якщо порожньо — встановити з тризуб.yaml)
        #[arg(value_name = "ПАКЕТ")]
        package: Option<String>,
    },

    /// Інтерактивний режим (REPL)
    #[command(name = "інтерактив")]
    Repl,

    /// Оновити всі залежності
    #[command(name = "оновити")]
    Update,

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

    /// Генерувати HTML документацію
    #[command(name = "док")]
    Doc {
        #[arg(value_name = "ШЛЯХ")]
        path: PathBuf,
        #[arg(long = "вивід", default_value = "docs")]
        output: PathBuf,
    },

    /// Профілювання програми
    #[command(name = "профіль")]
    Profile {
        /// Файл для профілювання
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// Hot reload — перезапуск при зміні файлу
    #[command(name = "спостерігати")]
    Watch {
        /// Файл для спостереження
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// LSP сервер для IDE підтримки
    #[command(name = "lsp")]
    Lsp,

    /// Форматувати файл
    #[command(name = "формат")]
    Format {
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
        #[arg(long = "перевірка", default_value = "false")]
        check_only: bool,
    },

    /// Перевірити стиль коду (лінтер)
    #[command(name = "лінт")]
    Lint {
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,
    },

    /// Компілювати в standalone виконуваний файл
    #[command(name = "компілювати")]
    Compile {
        /// Файл для компіляції
        #[arg(value_name = "ФАЙЛ")]
        file: PathBuf,

        /// Вихідний файл
        #[arg(short = 'о', long = "вихід")]
        output: Option<PathBuf>,

        /// Нативна компіляція в flat binary (x86_64 machine code)
        #[arg(long = "нативний", default_value = "false")]
        native: bool,

        /// Компілювати як bootable OS kernel image
        #[arg(long = "ядро", default_value = "false")]
        kernel: bool,
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
    if let Some(source) = extract_embedded_source() {
        let result = run_embedded_source(&source);
        if let Err(e) = result {
            eprintln!("\x1b[1;31m[X] {}\x1b[0m", e);
            std::process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Lsp => run_lsp(),
        Commands::Format { file, check_only } => run_format(file, check_only),
        Commands::Lint { file } => run_lint(file),
        Commands::Doc { path, output } => run_doc(path, output),
        Commands::Install { package } => run_install(package),
        Commands::Update => run_update(),
        Commands::Run { file, fast, jit, args } => run_file(file, fast, jit, args),
        Commands::Watch { file } => watch_file(file),
        Commands::Compile { file, output, native, kernel } => compile_file(file, output, native, kernel),
        Commands::Check { file } => check_file(file),
        Commands::Test { file } => run_tests(file),
        Commands::New { name } => create_project(name),
        Commands::Repl => run_repl(),
        Commands::Web { action } => match action {
            WebCommands::New { name } => create_web_project(name),
            WebCommands::Run { file, port } => run_file(file, false, false, vec![port.to_string()]),
        },
        Commands::Benchmark { iterations } => {
            println!("\nТризуб VM — Бенчмарк швидкості\n");
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
            println!("Тризуб v9.0.0");
            println!("Ліцензія: MIT");
            println!("https://github.com/Evge14n/tryzub");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("\x1b[1;31m[X] {}\x1b[0m", e);
        std::process::exit(1);
    }
}

fn format_error_with_source(source: &str, file: &std::path::Path, error: &str) -> String {
    let line_num = extract_line_number(error);
    if line_num == 0 {
        return error.to_string();
    }
    let lines: Vec<&str> = source.lines().collect();
    let mut out = String::new();
    out.push_str(&format!("\x1b[1;31mПомилка\x1b[0m: {}\n", error));
    out.push_str(&format!(" \x1b[36m-->\x1b[0m {}:{}\n", file.display(), line_num));
    out.push_str("  \x1b[36m|\x1b[0m\n");
    let start = line_num.saturating_sub(2);
    let end = std::cmp::min(line_num + 1, lines.len());
    for i in start..end {
        let marker = if i + 1 == line_num { "\x1b[1;31m>\x1b[0m" } else { " " };
        let num_color = if i + 1 == line_num { "\x1b[1;31m" } else { "\x1b[36m" };
        out.push_str(&format!("{} {}{:>4}\x1b[0m \x1b[36m|\x1b[0m {}\n", marker, num_color, i + 1, lines[i]));
    }
    out.push_str("  \x1b[36m|\x1b[0m\n");
    out
}

fn extract_line_number(error: &str) -> usize {
    if let Some(pos) = error.rfind("рядку ") {
        let after = &error[pos + "рядку ".len()..];
        let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse().unwrap_or(0);
    }
    0
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
        println!("\n  Профіль: {:?}", file);
        println!("  Час виконання: {:.2} мс", elapsed.as_secs_f64() * 1000.0);
        if let tryzub_vm::Value::Dict(pairs) = stats {
            for (k, v) in pairs {
                println!("  {}: {}", k.to_display_string(), v.to_display_string());
            }
        }
    }
    Ok(())
}

fn run_file(file: PathBuf, fast: bool, jit: bool, args: Vec<String>) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    let has_main = source.contains("функція головна(") || source.contains("функція головна (");
    let has_declarations = source.lines().any(|l| {
        let t = l.trim();
        t.starts_with("функція ") || t.starts_with("тип ") || t.starts_with("тест ")
    });
    let effective_source = if has_main || has_declarations {
        source.clone()
    } else {
        format!("функція головна() {{\n{}\n}}", source)
    };

    let tokens = match tryzub_lexer::tokenize(&effective_source) {
        Ok(t) => t,
        Err(e) => {
            eprint!("{}", format_error_with_source(&source, &file, &e.to_string()));
            std::process::exit(1);
        }
    };

    let ast = match tryzub_parser::parse(tokens) {
        Ok(a) => a,
        Err(e) => {
            eprint!("{}", format_error_with_source(&source, &file, &e.to_string()));
            std::process::exit(1);
        }
    };

    if jit {
        #[cfg(target_arch = "x86_64")]
        {
            let compiler = tryzub_vm::compiler::Compiler::new();
            let chunk = compiler.compile_program(&ast);
            let jit_compiler = tryzub_vm::jit::JitCompiler::new();
            let jit_fn = jit_compiler.compile(&chunk);
            let start = std::time::Instant::now();
            let result = jit_fn.execute();
            let elapsed = start.elapsed();
            if result != 0 {
                eprintln!("  [JIT] Результат: {} ({:.3}мс)", result, elapsed.as_secs_f64() * 1000.0);
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        return Err(anyhow::anyhow!("JIT доступний тільки на x86_64"));
    } else if fast {
        let compiler = tryzub_vm::compiler::Compiler::new();
        let chunk = compiler.compile_program(&ast);
        let mut bc_vm = tryzub_vm::bytecode::BytecodeVM::new(chunk.local_count);
        bc_vm.execute(&chunk);
        Ok(())
    } else {
        let mut vm = tryzub_vm::VM::new();
        if let Some(parent) = file.parent() {
            vm.add_module_path(parent.to_string_lossy().to_string());
        }
        vm.execute_program(ast, args)
    }
}

fn compile_file(file: PathBuf, output: Option<PathBuf>, native: bool, kernel: bool) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати {:?}: {}", file, e))?;

    let tokens = tryzub_lexer::tokenize(&source)?;
    let _ast = tryzub_parser::parse(tokens)?;

    let stem = file.file_stem().unwrap_or_default().to_string_lossy().to_string();

    if kernel {
        let out_name = output.unwrap_or_else(|| PathBuf::from(format!("{}.bin", stem)));
        tryzub_vm::native::NativeCompiler::compile_to_bootable(&source, &out_name.to_string_lossy())?;
        let size = fs::metadata(&out_name)?.len();
        println!("Ядро скомпільовано: {} ({} байт)", out_name.display(), size);
        println!("Запустити: qemu-system-x86_64 -drive format=raw,file={}", out_name.display());
        return Ok(());
    }

    if native {
        let out_name = output.unwrap_or_else(|| PathBuf::from(format!("{}.bin", stem)));
        tryzub_vm::native::NativeCompiler::compile_to_flat_binary(&source, &out_name.to_string_lossy())?;
        let size = fs::metadata(&out_name)?.len();
        println!("Нативний код: {} ({} байт)", out_name.display(), size);
        return Ok(());
    }

    // Бандл: interpreter + source
    let out_name = output.unwrap_or_else(|| PathBuf::from(format!("{}.exe", stem)));
    let exe_bytes = fs::read(std::env::current_exe()?)?;
    let magic = b"\xd0\xa2\xd0\xa0\xd0\x98\xd0\x97";
    let source_bytes = source.as_bytes();

    let mut output_bytes = exe_bytes;
    output_bytes.extend_from_slice(source_bytes);
    output_bytes.extend_from_slice(&(source_bytes.len() as u64).to_le_bytes());
    output_bytes.extend_from_slice(magic);

    fs::write(&out_name, &output_bytes)?;
    let size_mb = output_bytes.len() as f64 / 1024.0 / 1024.0;
    println!("Скомпільовано: {} ({:.1} MB)", out_name.display(), size_mb);
    Ok(())
}

fn watch_file(file: PathBuf) -> Result<()> {
    use notify::{Watcher, RecursiveMode};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Vec<std::path::PathBuf>>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            if event.kind.is_modify() {
                let tryzub_paths: Vec<std::path::PathBuf> = event.paths.iter()
                    .filter(|p| p.extension().map_or(false, |e| e == "тризуб" || e == "tryzub"))
                    .cloned()
                    .collect();
                if !tryzub_paths.is_empty() {
                    let _ = tx.send(tryzub_paths);
                }
            }
        }
    })?;

    let watch_path = if file.is_dir() {
        watcher.watch(&file, RecursiveMode::Recursive)?;
        file.clone()
    } else {
        let watch_dir = file.parent().unwrap_or(std::path::Path::new("."));
        watcher.watch(watch_dir, RecursiveMode::Recursive)?;
        file.clone()
    };

    println!("\x1b[36m👁 Спостерігаю за {:?}\x1b[0m", watch_path);
    println!("   Зміни автоматично перезапустять програму\n");

    let run_target = if file.is_dir() {
        let mut main_file = file.join("головна.тризуб");
        if !main_file.exists() { main_file = file.join("main.tryzub"); }
        if !main_file.exists() {
            return Err(anyhow::anyhow!("Не знайдено головна.тризуб у директорії {:?}", file));
        }
        main_file
    } else {
        file.clone()
    };

    loop {
        println!("\x1b[33m▶ Запуск...\x1b[0m");
        let start = std::time::Instant::now();
        match run_file(run_target.clone(), false, false, vec![]) {
            Ok(_) => {
                let elapsed = start.elapsed();
                println!("\x1b[32m✓ Виконано за {:.1}мс\x1b[0m", elapsed.as_secs_f64() * 1000.0);
            }
            Err(e) => {
                eprintln!("\x1b[31m✗ {}\x1b[0m", e);
            }
        }
        println!("\x1b[36m  Чекаю на зміни...\x1b[0m\n");

        while rx.try_recv().is_ok() {}
        let changed = rx.recv().unwrap_or_default();
        std::thread::sleep(std::time::Duration::from_millis(300));
        while rx.try_recv().is_ok() {}

        print!("\x1b[2J\x1b[H");
        let changed_names: Vec<String> = changed.iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .collect();
        let display = if changed_names.is_empty() {
            run_target.file_name().unwrap_or_default().to_string_lossy().to_string()
        } else {
            changed_names.join(", ")
        };
        println!("\x1b[33m🔄 Змінено: {} → перезапуск\x1b[0m\n", display);
    }
}

fn check_file(file: PathBuf) -> Result<()> {
    let source = fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("Не вдалося прочитати файл {:?}: {}", file, e))?;

    println!("Перевіряю: {:?}", file);

    let tokens = tryzub_lexer::tokenize(&source)?;
    println!("  ✓ Лексичний аналіз: {} токенів", tokens.len());

    let _ast = tryzub_parser::parse(tokens)?;
    println!("  ✓ Синтаксичний аналіз: OK");

    println!("[OK] Файл синтаксично правильний");
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
                        generic_params: vec![],
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
                    println!("  [OK] {}", name);
                }
                Err(e) => {
                    failed += 1;
                    println!("  [X] {} — {}", name, e);
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
                        generic_params: vec![],
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
                        generic_params: vec![],
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
                        println!("  [X] {} (фаз ітерація {}) — {}", name, i, e);
                        break;
                    }
                }
            }
            if fuzz_passed {
                passed += 1;
                println!("  [OK] {} (50 фаз-ітерацій)", name);
            } else {
                failed += 1;
            }
        }
    }

    println!("\n─────────────────────────────");
    println!("Всього: {} | Пройшли: {} | Провалені: {}", total, passed, failed);

    if failed > 0 {
        println!("\n[X] {} тестів провалено!", failed);
        std::process::exit(1);
    } else if total > 0 {
        println!("\n[OK] Всі {} тестів пройшли!", total);
    } else {
        println!("\n⚠️ Тестів не знайдено");
    }

    Ok(())
}

fn run_repl() -> Result<()> {
    use rustyline::error::ReadlineError;

    let history_path = dirs_history_path();

    let mut rl = rustyline::DefaultEditor::new()?;
    let _ = rl.load_history(&history_path);

    println!("\x1b[36mТризуб v9.0.0\x1b[0m — Інтерактивний режим");
    println!("Введіть :допомога для списку команд");
    println!();

    let mut declarations_source = String::new();

    loop {
        let readline = rl.readline("тризуб> ");
        let line = match readline {
            Ok(l) => l.trim().to_string(),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(e) => { eprintln!("Помилка: {}", e); break; }
        };
        if line.is_empty() { continue; }
        rl.add_history_entry(&line).ok();

        if line.is_empty() {
            continue;
        }

        // Спеціальні команди
        if line == ":вихід" || line == ":quit" || line == ":q" {
            println!("До побачення! ");
            break;
        }

        if line == ":допомога" || line == ":help" {
            println!("  \x1b[36mКоманди:\x1b[0m");
            println!("  :тип <вираз>          — показати тип значення");
            println!("  :час <код>            — виміряти час виконання");
            println!("  :завантажити <файл>   — завантажити .тризуб файл");
            println!("  :функції              — список вбудованих функцій");
            println!("  :очистити             — очистити контекст");
            println!("  :вихід                — вийти");
            println!();
            println!("  \x1b[36mСинтаксис:\x1b[0m");
            println!("  змінна/стала          — оголошення змінних");
            println!("  якщо/інакше           — умови (дужки опціональні)");
            println!("  для і в 1..10 {{}}     — цикл range");
            println!("  поки умова {{}}        — цикл while");
            println!("  функція ім_я(а, б = 0) — функція з дефолтами");
            println!("  друк(ф\"x = {{x}}\")    — інтерполяція рядків");
            continue;
        }

        if line == ":функції" || line == ":functions" {
            println!("  \x1b[36mВбудовані функції:\x1b[0m");
            println!("  друк(x)              — вивести значення");
            println!("  тип_значення(x)      — тип змінної");
            println!("  довжина(x)           — довжина рядка/масиву");
            println!("  діапазон(від, до)    — створити масив чисел");
            println!("  ціле(x)              — перетворити в число");
            println!("  дробове(x)           — перетворити в дробове");
            println!("  рядок(x)             — перетворити в рядок");
            println!();
            println!("  \x1b[36mМетоди рядків:\x1b[0m .довжина() .великі() .малі() .обрізати() .містить() .замінити() .розділити()");
            println!("  \x1b[36mМетоди масивів:\x1b[0m .довжина() .додати() .видалити() .сортувати() .обернути() .згорнути()");
            println!("  \x1b[36mМетоди словників:\x1b[0m .ключі() .значення() .містить_ключ() .видалити()");
            continue;
        }

        if line == ":очистити" {
            declarations_source.clear();
            println!("Контекст очищено.");
            continue;
        }

        if let Some(expr) = line.strip_prefix(":тип ") {
            let full_source = format!(
                "{}\nфункція головна() {{ змінна __р = {} \n друк(тип_значення(__р)) }}",
                declarations_source, expr
            );
            match run_source(&full_source) {
                Ok(()) => {}
                Err(e) => println!("[X] {}", e),
            }
            continue;
        }

        if let Some(code) = line.strip_prefix(":час ") {
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
                Err(e) => println!("[X] {}", e),
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
                Err(e) => println!("[X] Не вдалося прочитати {}: {}", path, e),
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
                match rl.readline("  ... ") {
                    Ok(next_line) => {
                        full.push('\n');
                        full.push_str(next_line.trim());
                        brace_count += next_line.matches('{').count() as i32 - next_line.matches('}').count() as i32;
                    }
                    _ => break,
                }
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
            Err(e) => println!("[X] {}", e),
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn dirs_history_path() -> String {
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        format!("{}/.тризуб_історія", home.to_string_lossy())
    } else {
        ".тризуб_історія".to_string()
    }
}

fn extract_embedded_source() -> Option<String> {
    let data = fs::read(std::env::current_exe().ok()?).ok()?;
    let magic = b"\xd0\xa2\xd0\xa0\xd0\x98\xd0\x97";
    if data.len() <= 16 || &data[data.len()-8..] != magic { return None; }
    let len_bytes: [u8; 8] = data[data.len()-16..data.len()-8].try_into().ok()?;
    let source_len = u64::from_le_bytes(len_bytes) as usize;
    let source_start = data.len().checked_sub(16 + source_len)?;
    String::from_utf8(data[source_start..source_start+source_len].to_vec()).ok()
}

fn run_embedded_source(source: &str) -> Result<()> {
    let tokens = tryzub_lexer::tokenize(source)?;
    let ast = tryzub_parser::parse(tokens)?;
    let mut vm = tryzub_vm::VM::new();
    vm.execute_program(ast, std::env::args().skip(1).collect())
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
// Створено за допомогою мови Тризуб v9.0.0

функція головна() {{
    друк("Привіт з проекту {}! ")

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

    let project_file = format!(r#"назва: {}
версія: 0.1.0
автор: ""
опис: ""

залежності: {{}}

скрипти:
  запустити: тризуб запустити src/головна.тризуб
  тестувати: тризуб тестувати src/
"#, name);

    fs::write(format!("{}/тризуб.yaml", name), project_file)?;

    let gitignore = "target/\n*.exe\n*.db\n";
    fs::write(format!("{}/.gitignore", name), gitignore)?;

    println!("[OK] Проект '{}' створено", name);
    println!("{}/", name);
    println!("   ├── тризуб.yaml");
    println!("   └── src/");
    println!("       └── головна.тризуб");
    println!();
    println!("Запустити: tryzub запустити {}/src/головна.тризуб", name);

    Ok(())
}

fn run_install(package: Option<String>) -> Result<()> {
    let modules_dir = ".тризуб_модулі";
    fs::create_dir_all(modules_dir)?;

    match package {
        Some(url) => {
            let (git_url, version_spec) = if url.contains('@') {
                let parts: Vec<&str> = url.splitn(2, '@').collect();
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else {
                (url.clone(), None)
            };

            let name = git_url.rsplit('/').next().unwrap_or("модуль")
                .trim_end_matches(".git").to_string();

            let target = format!("{}/{}", modules_dir, name);
            if std::path::Path::new(&target).exists() {
                println!("Оновлення {}...", name);
                let output = std::process::Command::new("git")
                    .args(["pull"])
                    .current_dir(&target)
                    .output()?;
                if !output.status.success() {
                    return Err(anyhow::anyhow!("git pull провалився: {}", String::from_utf8_lossy(&output.stderr)));
                }
            } else {
                println!("Встановлення {}...", name);
                let output = std::process::Command::new("git")
                    .args(["clone", &git_url, &target])
                    .output()?;
                if !output.status.success() {
                    return Err(anyhow::anyhow!("git clone провалився: {}", String::from_utf8_lossy(&output.stderr)));
                }
            }

            if let Some(ref ver) = version_spec {
                let tag = resolve_version_tag(&target, ver)?;
                let output = std::process::Command::new("git")
                    .args(["checkout", &tag])
                    .current_dir(&target)
                    .output()?;
                if !output.status.success() {
                    eprintln!("Попередження: не вдалося переключитись на версію {}", ver);
                }
            }

            let hash = get_git_hash(&target)?;
            let ver_display = version_spec.as_deref().unwrap_or("latest");
            update_lock_file(&name, &git_url, &hash)?;

            println!("[OK] {} встановлено ({}, {})", name, ver_display, &hash[..8.min(hash.len())]);
        }
        None => {
            let yaml_path = "тризуб.yaml";
            if !std::path::Path::new(yaml_path).exists() {
                return Err(anyhow::anyhow!("тризуб.yaml не знайдено"));
            }
            let content = fs::read_to_string(yaml_path)?;
            let mut installed = 0;
            let mut in_deps = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "залежності:" || trimmed == "dependencies:" {
                    in_deps = true;
                    continue;
                }
                if !trimmed.starts_with('-') && !trimmed.starts_with(' ') && !trimmed.is_empty() && in_deps {
                    in_deps = false;
                }
                if in_deps {
                    let dep = trimmed.trim_start_matches('-').trim().trim_matches('"').trim_matches('\'');
                    if dep.contains("://") || dep.ends_with(".git") || dep.contains('@') {
                        run_install(Some(dep.to_string()))?;
                        installed += 1;
                    } else if !dep.is_empty() {
                        let parts: Vec<&str> = dep.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let pkg_name = parts[0].trim();
                            let pkg_ver = parts[1].trim().trim_matches('"').trim_matches('\'');
                            println!("Пакет {}: {} (потрібен реєстр пакетів)", pkg_name, pkg_ver);
                        }
                    }
                }
                if trimmed.contains("://") || trimmed.ends_with(".git") {
                    let url = trimmed.split(':').last().unwrap_or("").trim().trim_matches('"');
                    if !url.is_empty() && !in_deps {
                        run_install(Some(url.to_string()))?;
                        installed += 1;
                    }
                }
            }
            if installed == 0 {
                println!("Немає залежностей для встановлення");
            }
        }
    }
    Ok(())
}

fn resolve_version_tag(repo_dir: &str, version_spec: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["tag", "-l"])
        .current_dir(repo_dir)
        .output()?;
    let tags_str = String::from_utf8_lossy(&output.stdout);
    let tags: Vec<&str> = tags_str.lines().collect();

    if version_spec.starts_with(">=") {
        let min_ver = version_spec.trim_start_matches(">=").trim();
        let matching: Vec<&&str> = tags.iter().filter(|t| {
            let v = t.trim_start_matches('v');
            v >= min_ver
        }).collect();
        matching.last().map(|t| t.to_string()).ok_or_else(|| anyhow::anyhow!("Немає тегів >= {}", min_ver))
    } else if version_spec.starts_with('~') {
        let base = version_spec.trim_start_matches('~').trim();
        let prefix = base.rsplit_once('.').map(|(p, _)| p).unwrap_or(base);
        let matching: Vec<&&str> = tags.iter().filter(|t| {
            let v = t.trim_start_matches('v');
            v.starts_with(prefix)
        }).collect();
        matching.last().map(|t| t.to_string()).ok_or_else(|| anyhow::anyhow!("Немає тегів ~{}", base))
    } else {
        let exact = format!("v{}", version_spec);
        if tags.contains(&exact.as_str()) {
            Ok(exact)
        } else if tags.contains(&version_spec) {
            Ok(version_spec.to_string())
        } else {
            Ok(version_spec.to_string())
        }
    }
}

fn run_update() -> Result<()> {
    let modules_dir = ".тризуб_модулі";
    if !std::path::Path::new(modules_dir).exists() {
        println!("Немає встановлених модулів");
        return Ok(());
    }
    let mut updated = 0;
    for entry in fs::read_dir(modules_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join(".git").exists() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            print!("Оновлення {}... ", name);
            let output = std::process::Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&path)
                .output()?;
            if output.status.success() {
                let hash = get_git_hash(&path.to_string_lossy())?;
                println!("✓ ({})", &hash[..8.min(hash.len())]);
                updated += 1;
            } else {
                println!("✗ помилка");
            }
        }
    }
    println!("Оновлено модулів: {}", updated);
    Ok(())
}

fn get_git_hash(dir: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn update_lock_file(name: &str, url: &str, hash: &str) -> Result<()> {
    let lock_path = "тризуб.lock";
    let mut content = if std::path::Path::new(lock_path).exists() {
        fs::read_to_string(lock_path)?
    } else {
        String::new()
    };

    let entry = format!("[[пакет]]\nназва = \"{}\"\nджерело = \"{}\"\nверсія = \"{}\"\n\n", name, url, hash);

    let marker = format!("назва = \"{}\"", name);
    if let Some(start) = content.find(&marker) {
        if let Some(block_start) = content[..start].rfind("[[пакет]]") {
            let end = content[start..].find("\n\n").map(|i| start + i + 2).unwrap_or(content.len());
            content.replace_range(block_start..end, &entry);
        }
    } else {
        content.push_str(&entry);
    }

    fs::write(lock_path, content)?;
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

// ════════════════════════════════════════════════════════════════════
// LSP сервер (Блок 8)
// ════════════════════════════════════════════════════════════════════

fn run_lsp() -> Result<()> {
    use std::io::{self, BufRead, Write, Read};
    use std::collections::HashMap;

    let mut documents: HashMap<String, String> = HashMap::new();
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    eprintln!("Тризуб LSP сервер запущено");

    let lsp_send = |out: &mut io::StdoutLock, msg: &serde_json::Value| -> Result<()> {
        let s = serde_json::to_string(msg)?;
        write!(out, "Content-Length: {}\r\n\r\n{}", s.len(), s)?;
        out.flush()?;
        Ok(())
    };

    let mut header_buf = String::new();
    loop {
        header_buf.clear();
        let mut content_length: usize = 0;

        loop {
            let mut line = String::new();
            if stdin.read_line(&mut line)? == 0 { return Ok(()); }
            let trimmed = line.trim();
            if trimmed.is_empty() { break; }
            if trimmed.starts_with("Content-Length:") {
                content_length = trimmed[15..].trim().parse().unwrap_or(0);
            }
        }

        if content_length == 0 { continue; }

        {
            let mut body = vec![0u8; content_length];
            stdin.read_exact(&mut body)?;
            let body_str = String::from_utf8_lossy(&body);

                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&body_str) {
                    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    let id = msg.get("id").cloned();
                    let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

                    match method {
                        "initialize" => {
                            lsp_send(&mut stdout, &serde_json::json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {
                                    "capabilities": {
                                        "textDocumentSync": 1,
                                        "completionProvider": { "triggerCharacters": [".", ":"] },
                                        "hoverProvider": true,
                                        "definitionProvider": true
                                    },
                                    "serverInfo": { "name": "тризуб-lsp", "version": "2.0.0" }
                                }
                            }))?;
                        }
                        "initialized" => {}
                        "shutdown" => {
                            lsp_send(&mut stdout, &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }))?;
                        }
                        "exit" => return Ok(()),

                        "textDocument/didOpen" => {
                            if let (Some(uri), Some(text)) = (
                                params.pointer("/textDocument/uri").and_then(|v| v.as_str()),
                                params.pointer("/textDocument/text").and_then(|v| v.as_str()),
                            ) {
                                documents.insert(uri.to_string(), text.to_string());
                                let diags = lsp_diagnose(text);
                                lsp_send(&mut stdout, &serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "method": "textDocument/publishDiagnostics",
                                    "params": { "uri": uri, "diagnostics": diags }
                                }))?;
                            }
                        }
                        "textDocument/didChange" => {
                            if let Some(uri) = params.pointer("/textDocument/uri").and_then(|v| v.as_str()) {
                                if let Some(text) = params.pointer("/contentChanges/0/text").and_then(|v| v.as_str()) {
                                    documents.insert(uri.to_string(), text.to_string());
                                    let diags = lsp_diagnose(text);
                                    lsp_send(&mut stdout, &serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "method": "textDocument/publishDiagnostics",
                                        "params": { "uri": uri, "diagnostics": diags }
                                    }))?;
                                }
                            }
                        }
                        "textDocument/didClose" => {
                            if let Some(uri) = params.pointer("/textDocument/uri").and_then(|v| v.as_str()) {
                                documents.remove(uri);
                            }
                        }

                        "textDocument/completion" => {
                            let uri = params.pointer("/textDocument/uri").and_then(|v| v.as_str()).unwrap_or("");
                            let source = documents.get(uri).cloned().unwrap_or_default();
                            let items = lsp_completions(&source);
                            lsp_send(&mut stdout, &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": items }))?;
                        }

                        "textDocument/hover" => {
                            let uri = params.pointer("/textDocument/uri").and_then(|v| v.as_str()).unwrap_or("");
                            let line = params.pointer("/position/line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let col = params.pointer("/position/character").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let source = documents.get(uri).cloned().unwrap_or_default();
                            let hover = lsp_hover(&source, line, col);
                            lsp_send(&mut stdout, &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": hover }))?;
                        }

                        "textDocument/definition" => {
                            let uri = params.pointer("/textDocument/uri").and_then(|v| v.as_str()).unwrap_or("");
                            let line = params.pointer("/position/line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let col = params.pointer("/position/character").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let source = documents.get(uri).cloned().unwrap_or_default();
                            let def = lsp_definition(&source, uri, line, col);
                            lsp_send(&mut stdout, &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": def }))?;
                        }

                        _ => {
                            if id.is_some() {
                                lsp_send(&mut stdout, &serde_json::json!({
                                    "jsonrpc": "2.0", "id": id,
                                    "error": { "code": -32601, "message": "Метод не підтримується" }
                                }))?;
                            }
                        }
                    }
                }
            }
        }
    }

fn lsp_diagnose(source: &str) -> Vec<serde_json::Value> {
    let mut diags = Vec::new();
    match tryzub_lexer::tokenize(source) {
        Ok(tokens) => {
            if let Err(e) = tryzub_parser::parse(tokens) {
                let msg = e.to_string();
                let line = msg.split("рядку ").nth(1)
                    .and_then(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u64>().ok())
                    .unwrap_or(1).saturating_sub(1);
                diags.push(serde_json::json!({
                    "range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 100 } },
                    "severity": 1,
                    "source": "тризуб",
                    "message": msg
                }));
            }
        }
        Err(e) => {
            diags.push(serde_json::json!({
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 100 } },
                "severity": 1,
                "source": "тризуб",
                "message": e.to_string()
            }));
        }
    }
    diags
}

fn lsp_completions(source: &str) -> Vec<serde_json::Value> {
    let mut items = Vec::new();

    let builtins = [
        ("друк", "Вивести значення в консоль"),
        ("довжина", "Довжина масиву або рядка"),
        ("діапазон", "Створити ліниве Range(від, до)"),
        ("фільтрувати", "Фільтрувати масив за предикатом"),
        ("перетворити", "Перетворити кожен елемент масиву"),
        ("згорнути", "Згорнути масив до одного значення"),
        ("сортувати", "Відсортувати масив"),
        ("корінь", "Квадратний корінь"), ("синус", "sin(x)"), ("косинус", "cos(x)"),
        ("абс", "Модуль числа"), ("мін", "Мінімум"), ("макс", "Максимум"),
        ("тип_значення", "Тип значення як рядок"),
        ("файл_прочитати", "Прочитати файл як рядок"),
        ("файл_записати", "Записати рядок у файл"),
        ("json_розібрати", "Розібрати JSON рядок"),
        ("все", "Виконати всі функції паралельно"),
        ("перегони", "Перший результат з масиву функцій"),
        ("потік", "Запустити функцію в потоці"),
    ];
    for (name, detail) in &builtins {
        items.push(serde_json::json!({ "label": name, "kind": 3, "detail": detail }));
    }

    if let Ok(tokens) = tryzub_lexer::tokenize(source) {
        if let Ok(program) = tryzub_parser::parse(tokens) {
            for decl in &program.declarations {
                match decl {
                    tryzub_parser::Declaration::Function { name, params, .. } => {
                        let params_str: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                        items.push(serde_json::json!({
                            "label": name,
                            "kind": 3,
                            "detail": format!("функція {}({})", name, params_str.join(", "))
                        }));
                    }
                    tryzub_parser::Declaration::Variable { name, .. } => {
                        items.push(serde_json::json!({ "label": name, "kind": 6 }));
                    }
                    tryzub_parser::Declaration::Struct { name, fields, .. } => {
                        let fields_str: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                        items.push(serde_json::json!({
                            "label": name,
                            "kind": 22,
                            "detail": format!("структура {} {{ {} }}", name, fields_str.join(", "))
                        }));
                    }
                    tryzub_parser::Declaration::Module { name, .. } => {
                        items.push(serde_json::json!({ "label": name, "kind": 9 }));
                    }
                    _ => {}
                }
            }
        }
    }
    items
}

fn lsp_hover(source: &str, line: usize, col: usize) -> serde_json::Value {
    let target_line = source.lines().nth(line).unwrap_or("");
    let word = extract_word_at(target_line, col);

    if word.is_empty() {
        return serde_json::Value::Null;
    }

    if let Ok(tokens) = tryzub_lexer::tokenize(source) {
        if let Ok(program) = tryzub_parser::parse(tokens) {
            for decl in &program.declarations {
                match decl {
                    tryzub_parser::Declaration::Function { name, params, return_type, is_async, .. } if name == &word => {
                        let prefix = if *is_async { "асинхронна функція" } else { "функція" };
                        let params_str: Vec<String> = params.iter().map(|p| {
                            format!("{}: {}", p.name, type_to_string(&p.ty))
                        }).collect();
                        let ret = return_type.as_ref().map(|t| format!(" -> {}", type_to_string(t))).unwrap_or_default();
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\n{} {}({}){}\n```", prefix, name, params_str.join(", "), ret) }
                        });
                    }
                    tryzub_parser::Declaration::Struct { name, fields, .. } if name == &word => {
                        let f: Vec<String> = fields.iter().map(|f| format!("{}: {}", f.name, type_to_string(&f.ty))).collect();
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\nструктура {} {{\n  {}\n}}\n```", name, f.join(",\n  ")) }
                        });
                    }
                    tryzub_parser::Declaration::Trait { name, methods, .. } if name == &word => {
                        let m: Vec<String> = methods.iter().map(|m| {
                            let p = m.params.iter().filter(|p| p.name != "себе").map(|p| format!("{}: {}", p.name, type_to_string(&p.ty))).collect::<Vec<_>>().join(", ");
                            let r = m.return_type.as_ref().map(|t| format!(" -> {}", type_to_string(t))).unwrap_or_default();
                            format!("  {}({}){}", m.name, p, r)
                        }).collect();
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\nтрейт {} {{\n{}\n}}\n```", name, m.join("\n")) }
                        });
                    }
                    tryzub_parser::Declaration::Enum { name, variants, .. } if name == &word => {
                        let v: Vec<String> = variants.iter().map(|v| {
                            if v.fields.is_empty() { format!("  {}", v.name) }
                            else {
                                let fields = v.fields.iter().map(|f| {
                                    if let Some(ref n) = f.name { format!("{}: {}", n, type_to_string(&f.ty)) }
                                    else { type_to_string(&f.ty) }
                                }).collect::<Vec<_>>().join(", ");
                                format!("  {}({})", v.name, fields)
                            }
                        }).collect();
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\nперелік {} {{\n{}\n}}\n```", name, v.join("\n")) }
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    serde_json::json!({ "contents": format!("**{}**", word) })
}

fn lsp_definition(source: &str, uri: &str, line: usize, col: usize) -> serde_json::Value {
    let target_line = source.lines().nth(line).unwrap_or("");
    let word = extract_word_at(target_line, col);
    if word.is_empty() { return serde_json::Value::Null; }

    for (i, src_line) in source.lines().enumerate() {
        let trimmed = src_line.trim();
        let is_def = trimmed.contains(&format!("функція {}(", word))
            || trimmed.contains(&format!("функція {} (", word))
            || trimmed.contains(&format!("структура {} ", word))
            || trimmed.contains(&format!("структура {}{{", word))
            || trimmed.contains(&format!("трейт {} ", word))
            || trimmed.contains(&format!("трейт {}{{", word))
            || trimmed.contains(&format!("перелік {} ", word))
            || trimmed.contains(&format!("перелік {}{{", word))
            || trimmed.starts_with(&format!("змінна {} ", word))
            || trimmed.starts_with(&format!("змінна {}=", word));
        if is_def && i != line {
            let char_pos = src_line.find(&word).unwrap_or(0);
            return serde_json::json!({
                "uri": uri,
                "range": {
                    "start": { "line": i, "character": char_pos },
                    "end": { "line": i, "character": char_pos + word.len() }
                }
            });
        }
    }
    serde_json::Value::Null
}

fn extract_word_at(line: &str, col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if col >= chars.len() { return String::new(); }
    let mut start = col;
    let mut end = col;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') { start -= 1; }
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') { end += 1; }
    chars[start..end].iter().collect()
}

// ════════════════════════════════════════════════════════════════════
// Форматер (Блок 9)
// ════════════════════════════════════════════════════════════════════

fn run_format(file: PathBuf, check_only: bool) -> Result<()> {
    let source = std::fs::read_to_string(&file)?;
    let mut formatted = String::new();
    let mut indent_level: i32 = 0;
    let mut prev_was_empty = false;
    let mut prev_was_func_end = false;
    let mut consecutive_empty = 0;

    for line in source.lines() {
        let content = line.trim();

        if content.is_empty() {
            consecutive_empty += 1;
            if consecutive_empty <= 1 {
                formatted.push('\n');
                prev_was_empty = true;
            }
            continue;
        }
        consecutive_empty = 0;

        let is_func_start = content.starts_with("функція ")
            || content.starts_with("асинхронна функція ")
            || content.starts_with("публічний функція ")
            || content.starts_with("структура ")
            || content.starts_with("трейт ")
            || content.starts_with("реалізація ");

        if is_func_start && !formatted.is_empty() && !prev_was_empty {
            formatted.push('\n');
        }

        if prev_was_func_end && !is_func_start && !content.starts_with('}') && !prev_was_empty {
            formatted.push('\n');
        }

        prev_was_empty = false;
        prev_was_func_end = false;

        if content.starts_with('}') || content.starts_with(']') {
            indent_level = (indent_level - 1).max(0);
            if content == "}" { prev_was_func_end = true; }
        }

        let processed = format_line_ops(content);
        let processed = format_keyword_spacing(&processed);

        let indent = "    ".repeat(indent_level as usize);
        let full_line = format!("{}{}", indent, processed);

        formatted.push_str(&full_line);
        formatted.push('\n');

        if content.ends_with('{') || content.ends_with('[') {
            indent_level += 1;
        }
    }

    while formatted.ends_with("\n\n\n") {
        formatted.truncate(formatted.len() - 1);
    }
    while formatted.ends_with("\n\n") {
        formatted.pop();
    }
    if !formatted.ends_with('\n') {
        formatted.push('\n');
    }

    if check_only {
        if formatted != source {
            eprintln!("Форматування потрібне: {}", file.display());
            std::process::exit(1);
        } else {
            println!("OK: {}", file.display());
        }
    } else {
        std::fs::write(&file, &formatted)?;
        println!("Відформатовано: {}", file.display());
    }
    Ok(())
}

fn format_line_ops(line: &str) -> String {
    if line.starts_with("//") || line.starts_with("///") {
        return line.to_string();
    }

    let mut result = String::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = '"';

    while i < len {
        let c = chars[i];

        if !in_string && (c == '"' || c == '\'') {
            in_string = true;
            string_char = c;
            result.push(c);
            i += 1;
            continue;
        }
        if in_string {
            result.push(c);
            if c == string_char && (i == 0 || chars[i - 1] != '\\') {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == ',' && i + 1 < len && chars[i + 1] != ' ' {
            result.push(',');
            result.push(' ');
            i += 1;
            continue;
        }

        if c == '&' && i + 1 < len && chars[i + 1] == '&' {
            let prev_c = if i > 0 { chars[i - 1] } else { ' ' };
            if prev_c != ' ' { result.push(' '); }
            result.push_str("&&");
            if i + 2 < len && chars[i + 2] != ' ' { result.push(' '); }
            i += 2;
            continue;
        }

        if c == '|' && i + 1 < len && chars[i + 1] == '|' {
            let prev_c = if i > 0 { chars[i - 1] } else { ' ' };
            if prev_c != ' ' { result.push(' '); }
            result.push_str("||");
            if i + 2 < len && chars[i + 2] != ' ' { result.push(' '); }
            i += 2;
            continue;
        }

        let is_op = matches!(c, '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!');
        if is_op && !in_string {
            let next = if i + 1 < len { chars[i + 1] } else { ' ' };
            let prev_c = if i > 0 { chars[i - 1] } else { ' ' };

            if c == '/' && next == '/' { result.push_str(&chars[i..].iter().collect::<String>()); return result; }
            if c == '=' && next == '>' { result.push_str(" => "); i += 2; continue; }
            if c == '-' && next == '>' { result.push_str(" -> "); i += 2; continue; }
            if (c == '=' || c == '!' || c == '<' || c == '>') && next == '=' {
                let op: String = chars[i..i+2].iter().collect();
                if i > 0 && prev_c != ' ' { result.push(' '); }
                result.push_str(&op);
                if i + 2 < len && chars[i + 2] != ' ' { result.push(' '); }
                i += 2;
                continue;
            }
            if (c == '+' && next == '=') || (c == '-' && next == '=') || (c == '*' && next == '=') || (c == '/' && next == '=') || (c == '%' && next == '=') {
                let op: String = chars[i..i+2].iter().collect();
                if i > 0 && prev_c != ' ' { result.push(' '); }
                result.push_str(&op);
                if i + 2 < len && chars[i + 2] != ' ' { result.push(' '); }
                i += 2;
                continue;
            }
            if c == '|' && next == '>' { result.push_str(" |> "); i += 2; continue; }

            if c == '=' && prev_c != '!' && prev_c != '<' && prev_c != '>' && prev_c != '+' && prev_c != '-' && prev_c != '*' && prev_c != '/' && prev_c != '%' {
                if i > 0 && prev_c != ' ' { result.push(' '); }
                result.push(c);
                if next != ' ' && next != '=' { result.push(' '); }
                i += 1;
                continue;
            }

            if (c == '+' || c == '-') && (prev_c == '(' || prev_c == ',' || prev_c == '=' || prev_c == '[' || i == 0) {
                result.push(c);
                i += 1;
                continue;
            }

            if matches!(c, '+' | '-' | '*' | '/' | '%') && prev_c != ' ' && next != '=' {
                if c != '*' || (prev_c != '*' && next != '*') {
                    result.push(' ');
                    result.push(c);
                    if next != ' ' { result.push(' '); }
                    i += 1;
                    continue;
                }
            }
            if matches!(c, '+' | '-' | '*' | '/' | '%') && next != ' ' && next != '=' && prev_c == ' ' {
                result.push(c);
                if next != ' ' && next != ')' && next != ']' { result.push(' '); }
                i += 1;
                continue;
            }

            if matches!(c, '<' | '>') && next != '=' && prev_c != '-' && prev_c != '=' {
                if i > 0 && prev_c != ' ' { result.push(' '); }
                result.push(c);
                if next != ' ' && next != '=' { result.push(' '); }
                i += 1;
                continue;
            }
        }

        result.push(c);
        i += 1;
    }
    result
}

fn format_keyword_spacing(line: &str) -> String {
    let keywords = ["якщо", "поки", "для", "інакше якщо"];
    let mut result = line.to_string();
    for kw in &keywords {
        let no_space = format!("{}(", kw);
        let with_space = format!("{} (", kw);
        result = result.replace(&no_space, &with_space);
    }
    result
}

// ════════════════════════════════════════════════════════════════════
// Лінтер (Блок 9)
// ════════════════════════════════════════════════════════════════════

fn run_lint(file: PathBuf) -> Result<()> {
    let source = std::fs::read_to_string(&file)?;
    let mut warnings = Vec::new();

    // Текстові перевірки
    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();
        if trimmed.chars().count() > 100 {
            warnings.push(format!("рядок {}: занадто довгий ({} символів, макс 100)", line_num, trimmed.chars().count()));
        }
        if trimmed.contains("// TODO") || trimmed.contains("// FIXME") || trimmed.contains("// HACK") {
            warnings.push(format!("рядок {}: знайдено TODO/FIXME/HACK", line_num));
        }
    }

    // AST аналіз
    match tryzub_lexer::tokenize(&source) {
        Ok(tokens) => {
            match tryzub_parser::parse(tokens) {
                Ok(program) => lint_ast(&program, &source, &mut warnings),
                Err(e) => warnings.push(format!("синтаксична помилка: {}", e)),
            }
        }
        Err(e) => warnings.push(format!("лексична помилка: {}", e)),
    }

    if warnings.is_empty() {
        println!("✓ {} — помилок не знайдено", file.display());
    } else {
        println!("⚠ {} — {} попереджень:", file.display(), warnings.len());
        for w in &warnings {
            println!("  • {}", w);
        }
    }
    Ok(())
}

fn lint_ast(program: &tryzub_parser::Program, _source: &str, warnings: &mut Vec<String>) {
    use tryzub_parser::Declaration;

    let mut all_used_idents = std::collections::HashSet::new();
    let mut defined_enums: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    for decl in &program.declarations {
        if let Declaration::Enum { name, variants, .. } = decl {
            defined_enums.insert(name.clone(), variants.iter().map(|v| v.name.clone()).collect());
        }
    }

    for decl in &program.declarations {
        match decl {
            Declaration::Function { name, params, body, .. } => {
                if body.len() > 50 {
                    warnings.push(format!("функція '{}': занадто довга ({} операторів, макс 50)", name, body.len()));
                }

                let mut declared: Vec<String> = params.iter()
                    .filter(|p| p.name != "себе")
                    .map(|p| p.name.clone())
                    .collect();
                for stmt in body { collect_declared_vars(stmt, &mut declared); }

                let mut used = std::collections::HashSet::new();
                for stmt in body { collect_used_idents_stmt(stmt, &mut used); }
                all_used_idents.extend(used.clone());

                for var in &declared {
                    if !used.contains(var.as_str()) && !var.starts_with('_') {
                        let is_param = params.iter().any(|p| &p.name == var);
                        if is_param {
                            warnings.push(format!("функція '{}': параметр '{}' не використовується", name, var));
                        } else {
                            warnings.push(format!("функція '{}': змінна '{}' оголошена але не використовується", name, var));
                        }
                    }
                }

                for stmt in body { check_empty_catch(stmt, name, warnings); }
                check_shadowing(body, &declared, name, warnings);
                check_match_arms(body, &defined_enums, name, warnings);
            }
            _ => {}
        }
    }

    check_unused_imports(program, &all_used_idents, warnings);
}

fn check_shadowing(stmts: &[tryzub_parser::Statement], outer_vars: &[String], fn_name: &str, warnings: &mut Vec<String>) {
    use tryzub_parser::Statement;
    for stmt in stmts {
        match stmt {
            Statement::Block(inner) => {
                let mut inner_declared = Vec::new();
                for s in inner { collect_declared_vars(s, &mut inner_declared); }
                for var in &inner_declared {
                    if outer_vars.contains(var) && !var.starts_with('_') {
                        warnings.push(format!("функція '{}': змінна '{}' затінює зовнішню змінну (shadowing)", fn_name, var));
                    }
                }
                let mut combined = outer_vars.to_vec();
                combined.extend(inner_declared);
                check_shadowing(inner, &combined, fn_name, warnings);
            }
            Statement::If { then_branch, else_branch, .. } => {
                check_shadowing_stmt(then_branch, outer_vars, fn_name, warnings);
                if let Some(eb) = else_branch { check_shadowing_stmt(eb, outer_vars, fn_name, warnings); }
            }
            Statement::While { body, .. } => check_shadowing_stmt(body, outer_vars, fn_name, warnings),
            Statement::For { body, .. } => check_shadowing_stmt(body, outer_vars, fn_name, warnings),
            Statement::ForIn { body, .. } => check_shadowing_stmt(body, outer_vars, fn_name, warnings),
            _ => {}
        }
    }
}

fn check_shadowing_stmt(stmt: &tryzub_parser::Statement, outer_vars: &[String], fn_name: &str, warnings: &mut Vec<String>) {
    use tryzub_parser::Statement;
    match stmt {
        Statement::Block(stmts) => {
            let mut inner_declared = Vec::new();
            for s in stmts { collect_declared_vars(s, &mut inner_declared); }
            for var in &inner_declared {
                if outer_vars.contains(var) && !var.starts_with('_') {
                    warnings.push(format!("функція '{}': змінна '{}' затінює зовнішню змінну (shadowing)", fn_name, var));
                }
            }
            let mut combined = outer_vars.to_vec();
            combined.extend(inner_declared);
            check_shadowing(stmts, &combined, fn_name, warnings);
        }
        _ => {}
    }
}

fn check_match_arms(stmts: &[tryzub_parser::Statement], enums: &std::collections::HashMap<String, Vec<String>>, fn_name: &str, warnings: &mut Vec<String>) {
    use tryzub_parser::{Statement, Expression, Pattern};
    for stmt in stmts {
        match stmt {
            Statement::Expression(Expression::Match { subject, arms }) => {
                if let Expression::Identifier(ref name) = **subject {
                    let _ = name;
                }
                let mut covered_variants: Vec<String> = Vec::new();
                let mut has_wildcard = false;
                for arm in arms {
                    match &arm.pattern {
                        Pattern::Variant { name, .. } => { covered_variants.push(name.clone()); }
                        Pattern::Wildcard | Pattern::Binding(_) => { has_wildcard = true; }
                        _ => {}
                    }
                }
                if !has_wildcard && !covered_variants.is_empty() {
                    for (enum_name, variants) in enums {
                        let all_match = covered_variants.iter().all(|v| variants.contains(v));
                        if all_match && covered_variants.len() < variants.len() {
                            let missing: Vec<&String> = variants.iter().filter(|v| !covered_variants.contains(v)).collect();
                            warnings.push(format!("функція '{}': зіставлення '{}' не покриває варіанти: {}", fn_name, enum_name, missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
                        }
                    }
                }
            }
            Statement::Block(inner) => check_match_arms(inner, enums, fn_name, warnings),
            Statement::If { then_branch, else_branch, .. } => {
                check_match_arms_stmt(then_branch, enums, fn_name, warnings);
                if let Some(eb) = else_branch { check_match_arms_stmt(eb, enums, fn_name, warnings); }
            }
            Statement::While { body, .. } | Statement::For { body, .. } | Statement::ForIn { body, .. } => {
                check_match_arms_stmt(body, enums, fn_name, warnings);
            }
            _ => {}
        }
    }
}

fn check_match_arms_stmt(stmt: &tryzub_parser::Statement, enums: &std::collections::HashMap<String, Vec<String>>, fn_name: &str, warnings: &mut Vec<String>) {
    if let tryzub_parser::Statement::Block(stmts) = stmt {
        check_match_arms(stmts, enums, fn_name, warnings);
    }
}

fn check_unused_imports(program: &tryzub_parser::Program, used_idents: &std::collections::HashSet<String>, warnings: &mut Vec<String>) {
    use tryzub_parser::Declaration;
    for decl in &program.declarations {
        if let Declaration::Import { path, items, alias } = decl {
            if let Some(items) = items {
                for item in items {
                    if !used_idents.contains(item) {
                        warnings.push(format!("невикористаний імпорт: '{}'", item));
                    }
                }
            } else if let Some(alias) = alias {
                if !used_idents.contains(alias) {
                    warnings.push(format!("невикористаний імпорт: '{}'", alias));
                }
            } else if let Some(last) = path.last() {
                if !used_idents.contains(last) {
                    warnings.push(format!("невикористаний імпорт: '{}'", last));
                }
            }
        }
    }
}

fn collect_declared_vars(stmt: &tryzub_parser::Statement, declared: &mut Vec<String>) {
    use tryzub_parser::Statement;
    match stmt {
        Statement::Declaration(tryzub_parser::Declaration::Variable { name, .. }) => {
            declared.push(name.clone());
        }
        Statement::Block(stmts) => {
            for s in stmts { collect_declared_vars(s, declared); }
        }
        Statement::If { then_branch, else_branch, .. } => {
            collect_declared_vars(then_branch, declared);
            if let Some(eb) = else_branch { collect_declared_vars(eb, declared); }
        }
        Statement::While { body, .. } => collect_declared_vars(body, declared),
        Statement::For { variable, body, .. } => {
            declared.push(variable.clone());
            collect_declared_vars(body, declared);
        }
        Statement::ForIn { body, .. } => collect_declared_vars(body, declared),
        _ => {}
    }
}

fn collect_used_idents_stmt(stmt: &tryzub_parser::Statement, used: &mut std::collections::HashSet<String>) {
    use tryzub_parser::Statement;
    match stmt {
        Statement::Expression(expr) => collect_used_idents_expr(expr, used),
        Statement::Declaration(tryzub_parser::Declaration::Variable { value: Some(expr), .. }) => {
            collect_used_idents_expr(expr, used);
        }
        Statement::Return(Some(expr)) => collect_used_idents_expr(expr, used),
        Statement::Block(stmts) => {
            for s in stmts { collect_used_idents_stmt(s, used); }
        }
        Statement::If { condition, then_branch, else_branch, .. } => {
            collect_used_idents_expr(condition, used);
            collect_used_idents_stmt(then_branch, used);
            if let Some(eb) = else_branch { collect_used_idents_stmt(eb, used); }
        }
        Statement::While { condition, body } => {
            collect_used_idents_expr(condition, used);
            collect_used_idents_stmt(body, used);
        }
        Statement::For { from, to, body, .. } => {
            collect_used_idents_expr(from, used);
            collect_used_idents_expr(to, used);
            collect_used_idents_stmt(body, used);
        }
        Statement::ForIn { iterable, body, .. } => {
            collect_used_idents_expr(iterable, used);
            collect_used_idents_stmt(body, used);
        }
        Statement::Yield(expr) => collect_used_idents_expr(expr, used),
        Statement::Assignment { target, value, .. } => {
            collect_used_idents_expr(target, used);
            collect_used_idents_expr(value, used);
        }
        _ => {}
    }
}

fn collect_used_idents_expr(expr: &tryzub_parser::Expression, used: &mut std::collections::HashSet<String>) {
    use tryzub_parser::Expression;
    match expr {
        Expression::Identifier(name) => { used.insert(name.clone()); }
        Expression::Binary { left, right, .. } => {
            collect_used_idents_expr(left, used);
            collect_used_idents_expr(right, used);
        }
        Expression::Unary { operand, .. } => collect_used_idents_expr(operand, used),
        Expression::Call { callee, args } => {
            collect_used_idents_expr(callee, used);
            for a in args { collect_used_idents_expr(a, used); }
        }
        Expression::MethodCall { object, args, .. } => {
            collect_used_idents_expr(object, used);
            for a in args { collect_used_idents_expr(a, used); }
        }
        Expression::MemberAccess { object, .. } => collect_used_idents_expr(object, used),
        Expression::Index { object, index } => {
            collect_used_idents_expr(object, used);
            collect_used_idents_expr(index, used);
        }
        Expression::Array(elems) | Expression::Tuple(elems) => {
            for e in elems { collect_used_idents_expr(e, used); }
        }
        Expression::Pipeline { left, right } => {
            collect_used_idents_expr(left, used);
            collect_used_idents_expr(right, used);
        }
        Expression::Struct { fields, .. } => {
            for (_, e) in fields { collect_used_idents_expr(e, used); }
        }
        Expression::Lambda { body, .. } => collect_used_idents_expr(body, used),
        Expression::LambdaBlock { body, .. } => {
            for s in body { collect_used_idents_stmt(s, used); }
        }
        Expression::If { condition, then_expr, else_expr } => {
            collect_used_idents_expr(condition, used);
            collect_used_idents_expr(then_expr, used);
            collect_used_idents_expr(else_expr, used);
        }
        Expression::Await(inner) => collect_used_idents_expr(inner, used),
        _ => {}
    }
}

fn check_empty_catch(stmt: &tryzub_parser::Statement, fn_name: &str, warnings: &mut Vec<String>) {
    use tryzub_parser::Statement;
    match stmt {
        Statement::TryCatch { catch_body: None, .. } => {
            warnings.push(format!("функція '{}': порожній catch блок", fn_name));
        }
        Statement::TryCatch { catch_body: Some(body), .. } => {
            if let Statement::Block(stmts) = body.as_ref() {
                if stmts.is_empty() {
                    warnings.push(format!("функція '{}': порожній catch блок", fn_name));
                }
            }
        }
        Statement::Block(stmts) => {
            for s in stmts { check_empty_catch(s, fn_name, warnings); }
        }
        Statement::If { then_branch, else_branch, .. } => {
            check_empty_catch(then_branch, fn_name, warnings);
            if let Some(eb) = else_branch { check_empty_catch(eb, fn_name, warnings); }
        }
        _ => {}
    }
}

// ════════════════════════════════════════════════════════════════════
// Генерація документації
// ════════════════════════════════════════════════════════════════════

fn run_doc(path: PathBuf, output: PathBuf) -> Result<()> {
    fs::create_dir_all(&output)?;

    let files: Vec<PathBuf> = if path.is_file() {
        vec![path]
    } else {
        let mut f: Vec<PathBuf> = fs::read_dir(&path)?
            .filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext == "тризуб" || ext == "tryzub"))
            .collect();
        f.sort();
        f
    };

    let mut nav = String::new();
    let mut content = String::new();
    let mut all_items: Vec<String> = Vec::new();

    for file in &files {
        let source = fs::read_to_string(file)?;
        let filename = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let module_id = filename.replace('.', "_");
        nav.push_str(&format!("<li class='nav-module'><a href='#{}'>{}</a><ul>\n", module_id, filename));
        content.push_str(&format!("<h2 id='{}' class='module-header'>{}</h2>\n", module_id, filename));

        if let Ok(tokens) = tryzub_lexer::tokenize(&source) {
            if let Ok(program) = tryzub_parser::parse(tokens) {
                doc_generate_decls(&program.declarations, &module_id, &mut nav, &mut content, &mut all_items, &source);
            }
        }

        if nav.ends_with("<ul>\n") {
            let mut doc_comments: Vec<String> = Vec::new();
            for line in source.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("///") {
                    doc_comments.push(trimmed[3..].trim().to_string());
                } else if trimmed.starts_with("функція ") || trimmed.starts_with("публічний функція ") {
                    let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
                    let doc_html = if !doc_comments.is_empty() {
                        format!("<div class='doc'>{}</div>", doc_to_html(&doc_comments))
                    } else { String::new() };
                    content.push_str(&format!("<div class='item fn'>{}<code class='sig'>{}</code></div>\n", doc_html, html_escape(sig)));
                    doc_comments.clear();
                } else if !trimmed.starts_with("//") { doc_comments.clear(); }
            }
        }
        nav.push_str("</ul></li>\n");
    }

    let html = format!(r#"<!DOCTYPE html>
<html lang="uk"><head><meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Тризуб — Документація</title>
<style>
:root {{ --bg: #fff; --fg: #2d3748; --blue: #0057b7; --gold: #ffd700; --border: #e2e8f0; --code-bg: #f7fafc; --sidebar-bg: #f8fafc; --hover: #ebf5ff; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #1a202c; --fg: #e2e8f0; --blue: #63b3ed; --gold: #ecc94b; --border: #2d3748; --code-bg: #2d3748; --sidebar-bg: #1e2533; --hover: #2a4365; }} }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; display: flex; min-height: 100vh; color: var(--fg); background: var(--bg); }}
nav {{ width: 280px; background: var(--sidebar-bg); border-right: 1px solid var(--border); padding: 20px; position: fixed; height: 100vh; overflow-y: auto; }}
nav h3 {{ color: var(--blue); margin-bottom: 15px; font-size: 1.1em; display: flex; align-items: center; gap: 8px; }}
nav ul {{ list-style: none; }}
nav li {{ margin: 2px 0; }}
nav li ul {{ padding-left: 14px; }}
nav a {{ color: var(--fg); text-decoration: none; font-size: 0.85em; padding: 2px 6px; border-radius: 3px; display: block; }}
nav a:hover {{ background: var(--hover); color: var(--blue); }}
.nav-module > a {{ font-weight: 600; font-size: 0.9em; color: var(--blue); }}
.nav-fn::before {{ content: 'fn'; font-size: 0.7em; color: #805ad5; background: #faf5ff; padding: 1px 4px; border-radius: 2px; margin-right: 4px; font-family: monospace; }}
.nav-struct::before {{ content: 'S'; font-size: 0.7em; color: #c05621; background: #fffaf0; padding: 1px 5px; border-radius: 2px; margin-right: 4px; font-family: monospace; }}
.nav-trait::before {{ content: 'T'; font-size: 0.7em; color: #2b6cb0; background: #ebf8ff; padding: 1px 5px; border-radius: 2px; margin-right: 4px; font-family: monospace; }}
.nav-enum::before {{ content: 'E'; font-size: 0.7em; color: #2f855a; background: #f0fff4; padding: 1px 5px; border-radius: 2px; margin-right: 4px; font-family: monospace; }}
main {{ margin-left: 280px; padding: 30px 50px; max-width: 900px; line-height: 1.7; }}
h1 {{ color: var(--blue); border-bottom: 3px solid var(--gold); padding-bottom: 10px; margin-bottom: 30px; font-size: 1.8em; }}
.module-header {{ color: var(--blue); margin-top: 40px; padding: 10px 0; border-bottom: 2px solid var(--border); font-size: 1.3em; }}
h3 {{ color: var(--fg); margin-top: 25px; font-size: 1.1em; opacity: 0.8; }}
.item {{ margin: 12px 0; padding: 12px 16px; border-left: 3px solid var(--blue); background: var(--code-bg); border-radius: 0 6px 6px 0; }}
.item.fn {{ border-left-color: #805ad5; }}
.item.struct {{ border-left-color: #c05621; }}
.item.trait {{ border-left-color: var(--gold); }}
.item.enum {{ border-left-color: #2f855a; }}
.item .fields {{ margin: 8px 0 0 20px; font-size: 0.9em; }}
.item .fields li {{ margin: 2px 0; }}
.item .methods {{ margin: 8px 0 0 10px; }}
.item .methods .method {{ padding: 6px 10px; margin: 4px 0; background: var(--bg); border-radius: 4px; border: 1px solid var(--border); }}
.doc {{ color: #718096; font-size: 0.92em; margin-bottom: 6px; line-height: 1.6; }}
.doc strong {{ color: var(--fg); }}
.doc code {{ background: var(--code-bg); padding: 1px 5px; border-radius: 3px; font-size: 0.9em; font-family: 'Cascadia Code', 'Fira Code', monospace; }}
.doc ul {{ margin: 4px 0 4px 20px; }}
.doc li {{ margin: 2px 0; }}
.sig {{ font-family: 'Cascadia Code', 'Fira Code', monospace; font-size: 0.9em; color: var(--fg); word-break: break-all; }}
.sig .kw {{ color: #805ad5; }}
.sig .type {{ color: #2b6cb0; }}
.sig .name {{ color: var(--fg); font-weight: 600; }}
#search {{ width: 100%; padding: 8px 12px; border: 1px solid var(--border); border-radius: 6px; margin-bottom: 15px; font-size: 0.9em; background: var(--bg); color: var(--fg); }}
#search:focus {{ outline: none; border-color: var(--blue); box-shadow: 0 0 0 3px rgba(0,87,183,0.1); }}
.badge {{ display: inline-block; font-size: 0.7em; padding: 1px 6px; border-radius: 3px; margin-left: 6px; font-weight: normal; }}
.badge-pub {{ background: #c6f6d5; color: #22543d; }}
.badge-async {{ background: #e9d8fd; color: #553c9a; }}
@media (max-width: 768px) {{ nav {{ display: none; }} main {{ margin-left: 0; padding: 20px; }} }}
</style></head><body>
<nav>
<h3>🔱 Тризуб Документація</h3>
<input type="text" id="search" placeholder="Пошук..." oninput="filterDocs(this.value)">
<ul>{}</ul>
</nav>
<main>
<h1>🔱 Тризуб — Документація API</h1>
{}
</main>
<script>
function filterDocs(q) {{
  const lower = q.toLowerCase();
  document.querySelectorAll('.item').forEach(el => {{
    el.style.display = el.textContent.toLowerCase().includes(lower) ? '' : 'none';
  }});
  document.querySelectorAll('nav li').forEach(el => {{
    if (el.classList.contains('nav-module')) return;
    el.style.display = el.textContent.toLowerCase().includes(lower) ? '' : 'none';
  }});
}}
</script>
</body></html>"#, nav, content);

    let out_file = output.join("index.html");
    fs::write(&out_file, html)?;
    println!("Документація згенерована: {}", out_file.display());
    println!("Файлів оброблено: {}", files.len());
    Ok(())
}

fn doc_generate_decls(decls: &[tryzub_parser::Declaration], module_id: &str, nav: &mut String, content: &mut String, items: &mut Vec<String>, source: &str) {
    use tryzub_parser::Declaration;
    let doc_lines = collect_doc_comments(source);

    for decl in decls {
        match decl {
            Declaration::Function { name, params, return_type, is_async, visibility, .. } => {
                let vis_badge = if *visibility == tryzub_parser::Visibility::Public { "<span class='badge badge-pub'>публічний</span>" } else { "" };
                let async_badge = if *is_async { "<span class='badge badge-async'>асинхронна</span>" } else { "" };
                let params_str = params.iter().map(|p| format!("{}: {}", p.name, type_to_string(&p.ty))).collect::<Vec<_>>().join(", ");
                let ret_str = return_type.as_ref().map(|t| format!(" -> {}", type_to_string(t))).unwrap_or_default();
                let sig = format!("<span class='kw'>функція</span> <span class='name'>{}</span>({}){}", name, html_escape(&params_str), html_escape(&ret_str));

                let doc_html = doc_lines.get(name.as_str()).map(|lines| format!("<div class='doc'>{}</div>", doc_to_html(lines))).unwrap_or_default();

                let item_id = format!("{}_{}", module_id, name);
                nav.push_str(&format!("<li class='nav-fn'><a href='#{}'>{}</a></li>\n", item_id, name));
                content.push_str(&format!("<div class='item fn' id='{}'>{}{}{}<code class='sig'>{}</code></div>\n", item_id, vis_badge, async_badge, doc_html, sig));
                items.push(name.clone());
            }
            Declaration::Struct { name, fields, methods, visibility, .. } => {
                let vis_badge = if *visibility == tryzub_parser::Visibility::Public { "<span class='badge badge-pub'>публічний</span>" } else { "" };
                let item_id = format!("{}_{}", module_id, name);
                let doc_html = doc_lines.get(name.as_str()).map(|lines| format!("<div class='doc'>{}</div>", doc_to_html(lines))).unwrap_or_default();

                nav.push_str(&format!("<li class='nav-struct'><a href='#{}'>{}</a></li>\n", item_id, name));
                content.push_str(&format!("<div class='item struct' id='{}'>{}{}<code class='sig'><span class='kw'>структура</span> <span class='name'>{}</span></code>\n", item_id, vis_badge, doc_html, name));

                if !fields.is_empty() {
                    content.push_str("<ul class='fields'>\n");
                    for f in fields {
                        content.push_str(&format!("<li><code>{}: <span class='type'>{}</span></code></li>\n", f.name, html_escape(&type_to_string(&f.ty))));
                    }
                    content.push_str("</ul>\n");
                }
                if !methods.is_empty() {
                    content.push_str("<div class='methods'>\n");
                    for m in methods {
                        if let Declaration::Function { name: mname, params, return_type, .. } = m {
                            let mp = params.iter().filter(|p| p.name != "себе").map(|p| format!("{}: {}", p.name, type_to_string(&p.ty))).collect::<Vec<_>>().join(", ");
                            let mr = return_type.as_ref().map(|t| format!(" -> {}", type_to_string(t))).unwrap_or_default();
                            content.push_str(&format!("<div class='method'><code class='sig'>.{}({}){}</code></div>\n", mname, html_escape(&mp), html_escape(&mr)));
                        }
                    }
                    content.push_str("</div>\n");
                }
                content.push_str("</div>\n");
                items.push(name.clone());
            }
            Declaration::Trait { name, methods, visibility, .. } => {
                let vis_badge = if *visibility == tryzub_parser::Visibility::Public { "<span class='badge badge-pub'>публічний</span>" } else { "" };
                let item_id = format!("{}_{}", module_id, name);
                let doc_html = doc_lines.get(name.as_str()).map(|lines| format!("<div class='doc'>{}</div>", doc_to_html(lines))).unwrap_or_default();

                nav.push_str(&format!("<li class='nav-trait'><a href='#{}'>{}</a></li>\n", item_id, name));
                content.push_str(&format!("<div class='item trait' id='{}'>{}{}<code class='sig'><span class='kw'>трейт</span> <span class='name'>{}</span></code>\n", item_id, vis_badge, doc_html, name));

                if !methods.is_empty() {
                    content.push_str("<div class='methods'>\n");
                    for m in methods {
                        let mp = m.params.iter().filter(|p| p.name != "себе").map(|p| format!("{}: {}", p.name, type_to_string(&p.ty))).collect::<Vec<_>>().join(", ");
                        let mr = m.return_type.as_ref().map(|t| format!(" -> {}", type_to_string(t))).unwrap_or_default();
                        let default = if m.default_body.is_some() { " <span class='badge' style='background:#e2e8f0;color:#4a5568;'>за замовч.</span>" } else { "" };
                        content.push_str(&format!("<div class='method'><code class='sig'>.{}({}){}</code>{}</div>\n", m.name, html_escape(&mp), html_escape(&mr), default));
                    }
                    content.push_str("</div>\n");
                }
                content.push_str("</div>\n");
                items.push(name.clone());
            }
            Declaration::Enum { name, variants, visibility, .. } => {
                let vis_badge = if *visibility == tryzub_parser::Visibility::Public { "<span class='badge badge-pub'>публічний</span>" } else { "" };
                let item_id = format!("{}_{}", module_id, name);
                let doc_html = doc_lines.get(name.as_str()).map(|lines| format!("<div class='doc'>{}</div>", doc_to_html(lines))).unwrap_or_default();

                nav.push_str(&format!("<li class='nav-enum'><a href='#{}'>{}</a></li>\n", item_id, name));
                content.push_str(&format!("<div class='item enum' id='{}'>{}{}<code class='sig'><span class='kw'>перелік</span> <span class='name'>{}</span></code>\n", item_id, vis_badge, doc_html, name));

                if !variants.is_empty() {
                    content.push_str("<ul class='fields'>\n");
                    for v in variants {
                        if v.fields.is_empty() {
                            content.push_str(&format!("<li><code>{}</code></li>\n", v.name));
                        } else {
                            let fields_str = v.fields.iter().map(|f| {
                                if let Some(ref n) = f.name { format!("{}: {}", n, type_to_string(&f.ty)) }
                                else { type_to_string(&f.ty) }
                            }).collect::<Vec<_>>().join(", ");
                            content.push_str(&format!("<li><code>{}({})</code></li>\n", v.name, html_escape(&fields_str)));
                        }
                    }
                    content.push_str("</ul>\n");
                }
                content.push_str("</div>\n");
                items.push(name.clone());
            }
            Declaration::Module { name, declarations, .. } => {
                content.push_str(&format!("<h3>Модуль: {}</h3>\n", name));
                doc_generate_decls(declarations, &format!("{}_{}", module_id, name), nav, content, items, source);
            }
            _ => {}
        }
    }
}

fn collect_doc_comments(source: &str) -> std::collections::HashMap<&str, Vec<String>> {
    let mut map = std::collections::HashMap::new();
    let mut current_docs: Vec<String> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("///") {
            current_docs.push(trimmed[3..].trim().to_string());
        } else if !current_docs.is_empty() {
            let name_part = trimmed
                .trim_start_matches("публічний ")
                .trim_start_matches("асинхронна ")
                .trim_start_matches("функція ")
                .trim_start_matches("структура ")
                .trim_start_matches("трейт ")
                .trim_start_matches("перелік ");
            if let Some(name) = name_part.split(|c: char| c == '(' || c == '{' || c == '<' || c.is_whitespace()).next() {
                if !name.is_empty() {
                    map.insert(name, current_docs.clone());
                }
            }
            current_docs.clear();
        } else {
            current_docs.clear();
        }
    }
    map
}

fn doc_to_html(lines: &[String]) -> String {
    let mut html = String::new();
    let mut in_list = false;
    for line in lines {
        if line.starts_with("- ") || line.starts_with("* ") {
            if !in_list { html.push_str("<ul>"); in_list = true; }
            let item = &line[2..];
            html.push_str(&format!("<li>{}</li>", doc_inline_format(item)));
        } else {
            if in_list { html.push_str("</ul>"); in_list = false; }
            if line.is_empty() { html.push_str("<br>"); }
            else { html.push_str(&doc_inline_format(line)); html.push(' '); }
        }
    }
    if in_list { html.push_str("</ul>"); }
    html
}

fn doc_inline_format(text: &str) -> String {
    let result = html_escape(text);
    let mut out = String::new();
    let chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(end) = chars[i+1..].iter().position(|&c| c == '`') {
                let code: String = chars[i+1..i+1+end].iter().collect();
                out.push_str(&format!("<code>{}</code>", code));
                i = i + 2 + end;
                continue;
            }
        }
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find_double(&chars, i + 2, '*') {
                let bold: String = chars[i+2..end].iter().collect();
                out.push_str(&format!("<strong>{}</strong>", bold));
                i = end + 2;
                continue;
            }
        }
        if chars[i] == '*' && (i == 0 || chars[i-1] != '*') {
            if let Some(end) = chars[i+1..].iter().position(|&c| c == '*') {
                if i + 1 + end < chars.len() && (i + 1 + end + 1 >= chars.len() || chars[i + 1 + end + 1] != '*') {
                    let em: String = chars[i+1..i+1+end].iter().collect();
                    out.push_str(&format!("<em>{}</em>", em));
                    i = i + 2 + end;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_double(chars: &[char], start: usize, ch: char) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == ch && chars[i + 1] == ch { return Some(i); }
        i += 1;
    }
    None
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn type_to_string(ty: &tryzub_parser::Type) -> String {
    use tryzub_parser::Type;
    match ty {
        Type::Цл8 => "цл8".into(), Type::Цл16 => "цл16".into(), Type::Цл32 => "цл32".into(), Type::Цл64 => "цл64".into(),
        Type::Чс8 => "чс8".into(), Type::Чс16 => "чс16".into(), Type::Чс32 => "чс32".into(), Type::Чс64 => "чс64".into(),
        Type::Дрб32 => "дрб32".into(), Type::Дрб64 => "дрб64".into(),
        Type::Лог => "лог".into(), Type::Сим => "сим".into(), Type::Тхт => "тхт".into(),
        Type::Named(n) => n.clone(),
        Type::Array(inner, size) => format!("[{}; {}]", type_to_string(inner), size),
        Type::Slice(inner) => format!("[{}]", type_to_string(inner)),
        Type::Tuple(types) => format!("({})", types.iter().map(|t| type_to_string(t)).collect::<Vec<_>>().join(", ")),
        Type::Optional(inner) => format!("Опція<{}>", type_to_string(inner)),
        Type::Result(ok, err) => format!("Результат<{}, {}>", type_to_string(ok), type_to_string(err)),
        Type::Generic(name, args) => format!("{}<{}>", name, args.iter().map(|t| type_to_string(t)).collect::<Vec<_>>().join(", ")),
        Type::Reference(inner, mutable) => format!("{}{}", if *mutable { "&мут " } else { "&" }, type_to_string(inner)),
        Type::Function(params, ret) => {
            let p = params.iter().map(|t| type_to_string(t)).collect::<Vec<_>>().join(", ");
            if let Some(r) = ret { format!("функція({}) -> {}", p, type_to_string(r)) }
            else { format!("функція({})", p) }
        }
        Type::SelfType => "себе".into(),
    }
}
