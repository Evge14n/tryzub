@echo off
chcp 65001 >nul
cls

echo.
echo ========================================
echo    🇺🇦 ТРИЗУБ - Тестування v1.0.0
echo    Автор: Мартинюк Євген
echo    Створено: 06.04.2025
echo ========================================
echo.

REM Перевірка наявності виконуваного файлу
if not exist "target\release\tryzub.exe" (
    echo ❌ Помилка: tryzub.exe не знайдено!
    echo.
    echo Спочатку зберіть проект:
    echo   cargo build --release
    echo.
    pause
    exit /b 1
)

echo ✅ Знайдено компілятор Тризуб
echo.
echo Запускаємо тестову програму...
echo ----------------------------------------

REM Запуск тестової програми
target\release\tryzub.exe запустити examples\тест.тризуб

echo.
echo ----------------------------------------
echo.
echo 💡 Спробуйте інші приклади:
echo   - target\release\tryzub.exe запустити examples\привіт_світ.тризуб
echo   - target\release\tryzub.exe запустити examples\фібоначчі.тризуб
echo   - target\release\tryzub.exe запустити examples\банк.тризуб
echo.
echo 📚 Документація: ВИКОРИСТАННЯ.md
echo.
pause
