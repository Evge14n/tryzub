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
            println!("Тризуб v5.9.0");
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

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            if event.kind.is_modify() {
                let _ = tx.send(());
            }
        }
    })?;

    let watch_dir = file.parent().unwrap_or(std::path::Path::new("."));
    watcher.watch(watch_dir, RecursiveMode::Recursive)?;

    println!("\x1b[36m👁 Спостерігаю за {:?}\x1b[0m", file);
    println!("   Зміни автоматично перезапустять програму\n");

    loop {
        println!("\x1b[33m▶ Запуск...\x1b[0m");
        let start = std::time::Instant::now();
        match run_file(file.clone(), false, false, vec![]) {
            Ok(_) => {
                let elapsed = start.elapsed();
                println!("\x1b[32m✓ Виконано за {:.1}мс\x1b[0m", elapsed.as_secs_f64() * 1000.0);
            }
            Err(e) => {
                eprintln!("\x1b[31m✗ {}\x1b[0m", e);
            }
        }
        println!("\x1b[36m  Чекаю на зміни...\x1b[0m\n");

        // Drain any pending events
        while rx.try_recv().is_ok() {}
        // Wait for next change
        let _ = rx.recv();
        // Small debounce
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        print!("\x1b[2J\x1b[H"); // clear screen
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
    use std::io::{self, Write, BufRead};

    println!("\x1b[36mТризуб v5.9.0\x1b[0m — Інтерактивний режим");
    println!("Введіть :допомога для списку команд");
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
            Err(e) => println!("[X] {}", e),
        }
    }

    Ok(())
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
// Створено за допомогою мови Тризуб v5.9.0

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
    use std::io::{self, BufRead, Write, Read, BufReader};
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
                if let tryzub_parser::Declaration::Function { name, params, return_type, .. } = decl {
                    if name == &word {
                        let params_str: Vec<String> = params.iter().map(|p| {
                            if let Some(ref ty) = p.default { format!("{} = ...", p.name) }
                            else { p.name.clone() }
                        }).collect();
                        let ret = match return_type {
                            Some(ty) => format!(" -> {:?}", ty),
                            None => String::new(),
                        };
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\nфункція {}({}){}\n```", name, params_str.join(", "), ret) }
                        });
                    }
                }
                if let tryzub_parser::Declaration::Struct { name, fields, .. } = decl {
                    if name == &word {
                        let f: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                        return serde_json::json!({
                            "contents": { "kind": "markdown", "value": format!("```тризуб\nструктура {} {{ {} }}\n```", name, f.join(", ")) }
                        });
                    }
                }
            }
        }
    }
    serde_json::json!({ "contents": format!("**{}**", word) })
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

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if !prev_was_empty {
                formatted.push('\n');
                prev_was_empty = true;
            }
            continue;
        }
        prev_was_empty = false;

        if trimmed.starts_with('}') || trimmed.starts_with(']') {
            indent_level = (indent_level - 1).max(0);
        }

        let indent = "    ".repeat(indent_level as usize);
        formatted.push_str(&indent);
        formatted.push_str(trimmed);
        formatted.push('\n');

        if trimmed.ends_with('{') || trimmed.ends_with('[') {
            indent_level += 1;
        }
    }

    if check_only {
        if formatted.trim() != source.trim() {
            eprintln!("Форматування потрібне: {}", file.display());
            std::process::exit(1);
        } else {
            println!("OK: {}", file.display());
        }
    } else {
        std::fs::write(&file, formatted)?;
        println!("Відформатовано: {}", file.display());
    }

    Ok(())
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
    use tryzub_parser::{Declaration, Statement, Expression};

    for decl in &program.declarations {
        if let Declaration::Function { name, params, body, .. } = decl {
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
