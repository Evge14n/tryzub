@echo off
chcp 65001 >nul
cls

echo ╔══════════════════════════════════════════╗
echo ║     🇺🇦 ТРИЗУБ - Збірка та тест         ║
echo ║     Автор: Мартинюк Євген               ║
echo ║     Версія: 1.0.0 | 06.04.2025          ║
echo ╚══════════════════════════════════════════╝
echo.

echo [1/4] 🧹 Очищення старих файлів...
cargo clean 2>nul

echo [2/4] 📦 Встановлення залежностей...
cargo fetch

echo [3/4] 🔨 Компіляція проекту...
cargo build --release

if %errorlevel% neq 0 (
    echo.
    echo ❌ Помилка компіляції!
    echo Перевірте встановлення Rust та LLVM.
    pause
    exit /b 1
)

echo [4/4] ✅ Компіляція завершена успішно!
echo.
echo ═══════════════════════════════════════════
echo 🧪 Запуск тестової програми...
echo ═══════════════════════════════════════════
echo.

target\release\tryzub.exe запустити examples\тест.тризуб

echo.
echo ═══════════════════════════════════════════
echo.
echo 🎉 Готово! Мова Тризуб працює!
echo.
echo Наступні кроки:
echo   1. Завантажте на GitHub
echo   2. Прочитайте GITHUB.md
echo   3. Поділіться з друзями!
echo.
pause
