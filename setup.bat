@echo off
chcp 65001 >nul
echo =======================================
echo Tryzub Programming Language Setup
echo =======================================
echo.

REM Check Rust
where rustc >nul 2>nul
if %errorlevel% neq 0 (
    echo X Rust not installed. Installing...
    echo Download and run: https://win.rustup.rs/
    pause
    exit /b 1
) else (
    echo OK Rust installed
    rustc --version
)

REM Check LLVM
where llvm-config >nul 2>nul
if %errorlevel% neq 0 (
    echo X LLVM not installed. 
    echo Install via Chocolatey: choco install llvm
    echo Or download from: https://releases.llvm.org/
    echo.
    echo Note: LLVM is optional for basic functionality
) else (
    echo OK LLVM installed
)

echo.
echo Building project...
cargo build

echo.
echo Running tests...
cargo test

echo.
echo Project is ready!
echo.
echo Quick start:
echo    cargo run -- run examples\hello_world.tryzub
echo    cargo run -- compile examples\hello_world.tryzub -o hello
echo.
echo Documentation: cargo doc --open
echo Code check: cargo clippy
echo Format: cargo fmt
echo.
pause
