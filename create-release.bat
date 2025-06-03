@echo off
chcp 65001 >nul
cls

REM Скрипт для створення релізу мови Тризуб
REM Автор: Мартинюк Євген

set /p VERSION=<VERSION
set AUTHOR=Мартинюк Євген
set DATE=06.04.2025

echo ╔══════════════════════════════════════════╗
echo ║   🇺🇦 Створення релізу Тризуб v%VERSION%    ║
echo ║   Автор: %AUTHOR%              ║
echo ║   Дата: %DATE%                      ║
echo ╚══════════════════════════════════════════╝
echo.

echo 🔨 Збірка релізної версії...
cargo build --release

if %errorlevel% neq 0 (
    echo ❌ Помилка збірки!
    pause
    exit /b 1
)

echo 📦 Створення архіву...
if not exist releases mkdir releases
if not exist releases\v%VERSION% mkdir releases\v%VERSION%

REM Створення ZIP архіву
echo Архівування файлів...
powershell -Command "Compress-Archive -Path 'target\release\tryzub.exe', 'README.md', 'LICENSE', 'AUTHORS.md', 'CHANGELOG.md', 'examples', 'stdlib', 'docs' -DestinationPath 'releases\v%VERSION%\tryzub-v%VERSION%-windows-x64.zip' -Force"

echo.
echo ✅ Реліз створено: releases\v%VERSION%\tryzub-v%VERSION%-windows-x64.zip
echo.
echo 📝 Наступні кроки:
echo   1. git add .
echo   2. git commit -m "🚀 Реліз v%VERSION%"
echo   3. git tag -a v%VERSION% -m "Тризуб v%VERSION% - Автор: %AUTHOR%"
echo   4. git push origin main --tags
echo   5. Завантажте ZIP на GitHub Releases
echo.
pause
