#!/bin/bash
# Скрипт для створення релізу мови Тризуб
# Автор: Мартинюк Євген

VERSION=$(cat VERSION)
AUTHOR="Мартинюк Євген"
DATE="06.04.2025"

echo "🇺🇦 Створення релізу Тризуб v$VERSION"
echo "Автор: $AUTHOR"
echo "Дата: $DATE"
echo "================================"

# Збірка для різних платформ
echo "🔨 Збірка для різних платформ..."

# Windows
echo "  Windows x64..."
cargo build --release --target x86_64-pc-windows-msvc

# Linux
echo "  Linux x64..."
cargo build --release --target x86_64-unknown-linux-gnu

# macOS
echo "  macOS x64..."
cargo build --release --target x86_64-apple-darwin

# Створення архівів
echo "📦 Створення архівів..."
mkdir -p releases/v$VERSION

# Windows
zip -r releases/v$VERSION/tryzub-v$VERSION-windows-x64.zip \
  target/x86_64-pc-windows-msvc/release/tryzub.exe \
  README.md LICENSE AUTHORS.md examples/ stdlib/

# Linux
tar -czf releases/v$VERSION/tryzub-v$VERSION-linux-x64.tar.gz \
  target/x86_64-unknown-linux-gnu/release/tryzub \
  README.md LICENSE AUTHORS.md examples/ stdlib/

# macOS
tar -czf releases/v$VERSION/tryzub-v$VERSION-macos-x64.tar.gz \
  target/x86_64-apple-darwin/release/tryzub \
  README.md LICENSE AUTHORS.md examples/ stdlib/

echo "✅ Релізи створено в папці releases/v$VERSION"
echo ""
echo "📝 Не забудьте:"
echo "  1. Створити git tag: git tag -a v$VERSION -m 'Реліз v$VERSION'"
echo "  2. Запушити tag: git push origin v$VERSION"
echo "  3. Створити реліз на GitHub з цими файлами"
