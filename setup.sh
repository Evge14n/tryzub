#!/bin/bash

echo "🇺🇦 Налаштування проекту Тризуб..."
echo "================================"

# Перевірка Rust
if ! command -v rustc &> /dev/null; then
    echo "❌ Rust не встановлено. Встановлюємо..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    source $HOME/.cargo/env
else
    echo "✅ Rust встановлено: $(rustc --version)"
fi

# Перевірка LLVM
if ! command -v llvm-config &> /dev/null; then
    echo "❌ LLVM не встановлено. Будь ласка, встановіть LLVM 15:"
    echo "   Ubuntu/Debian: sudo apt-get install llvm-15-dev"
    echo "   macOS: brew install llvm@15"
    echo "   Windows: choco install llvm"
    exit 1
else
    echo "✅ LLVM встановлено: $(llvm-config --version)"
fi

# Встановлення додаткових інструментів
echo ""
echo "📦 Встановлення додаткових інструментів..."
cargo install cargo-tarpaulin
cargo install cargo-audit
cargo install cargo-outdated

# Збірка проекту
echo ""
echo "🔨 Збірка проекту..."
cargo build

# Запуск тестів
echo ""
echo "🧪 Запуск тестів..."
cargo test

echo ""
echo "✅ Проект готовий до роботи!"
echo ""
echo "🚀 Швидкий старт:"
echo "   cargo run -- запустити examples/привіт_світ.тризуб"
echo "   cargo run -- компілювати examples/привіт_світ.тризуб -в привіт"
echo ""
echo "📚 Документація: cargo doc --open"
echo "🔍 Перевірка коду: cargo clippy"
echo "🎨 Форматування: cargo fmt"
