@echo off
echo =======================================
echo 🇺🇦 Налаштування проекту Тризуб
echo =======================================
echo.

REM Перевірка Rust
where rustc >nul 2>nul
if %errorlevel% neq 0 (
    echo ❌ Rust не встановлено. Встановлюємо...
    echo Завантажте та запустіть: https://win.rustup.rs/
    pause
    exit /b 1
) else (
    echo ✅ Rust встановлено
    rustc --version
)

REM Перевірка LLVM
where llvm-config >nul 2>nul
if %errorlevel% neq 0 (
    echo ❌ LLVM не встановлено. 
    echo Встановіть через Chocolatey: choco install llvm
    echo Або завантажте з: https://releases.llvm.org/
    pause
    exit /b 1
) else (
    echo ✅ LLVM встановлено
)

echo.
echo 📦 Встановлення додаткових інструментів...
cargo install cargo-tarpaulin
cargo install cargo-audit
cargo install cargo-outdated

echo.
echo 🔨 Збірка проекту...
cargo build

echo.
echo 🧪 Запуск тестів...
cargo test

echo.
echo ✅ Проект готовий до роботи!
echo.
echo 🚀 Швидкий старт:
echo    cargo run -- запустити examples\привіт_світ.тризуб
echo    cargo run -- компілювати examples\привіт_світ.тризуб -в привіт
echo.
echo 📚 Документація: cargo doc --open
echo 🔍 Перевірка коду: cargo clippy
echo 🎨 Форматування: cargo fmt
echo.
pause
