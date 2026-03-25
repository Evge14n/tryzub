# Скрипт встановлення мови програмування Тризуб для Windows
# Використання: iwr -useb https://evge14n.github.io/tryzub/install.ps1 | iex

$ErrorActionPreference = "Stop"

$REPO = "Evge14n/tryzub"
$INSTALL_DIR = "$env:USERPROFILE\.tryzub"
$BIN_DIR = "$INSTALL_DIR\bin"

Write-Host ""
Write-Host "  🔱 Встановлення мови програмування Тризуб" -ForegroundColor Yellow
Write-Host "  ─────────────────────────────────────────────" -ForegroundColor Cyan
Write-Host ""

# Створюємо директорії
New-Item -ItemType Directory -Force -Path $BIN_DIR | Out-Null

# Перевіряємо чи є Cargo
$hasCargo = Get-Command cargo -ErrorAction SilentlyContinue

if ($hasCargo) {
    Write-Host "  Знайдено Rust/Cargo - збірка з вихідного коду" -ForegroundColor Green
    Write-Host ""

    $TEMP_DIR = [System.IO.Path]::GetTempPath() + "tryzub_build"
    if (Test-Path $TEMP_DIR) { Remove-Item -Recurse -Force $TEMP_DIR }

    Write-Host "  Завантаження вихідного коду..." -ForegroundColor Blue
    git clone --depth 1 "https://github.com/$REPO.git" $TEMP_DIR 2>$null

    Write-Host "  Збірка (може зайняти кілька хвилин)..." -ForegroundColor Blue
    Push-Location $TEMP_DIR
    cargo build --release 2>&1 | Select-Object -Last 1
    Pop-Location

    Copy-Item "$TEMP_DIR\target\release\tryzub.exe" "$BIN_DIR\tryzub.exe" -Force -ErrorAction SilentlyContinue

    # Копіюємо стандартну бібліотеку
    $stdlibDir = "$INSTALL_DIR\stdlib"
    New-Item -ItemType Directory -Force -Path $stdlibDir | Out-Null
    Copy-Item "$TEMP_DIR\stdlib\*" $stdlibDir -Recurse -Force -ErrorAction SilentlyContinue

    Remove-Item -Recurse -Force $TEMP_DIR -ErrorAction SilentlyContinue
} else {
    Write-Host "  Rust не знайдено - завантаження бінарного файлу" -ForegroundColor Yellow
    Write-Host ""

    $arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "x86" }

    try {
        $latestRelease = Invoke-RestMethod "https://api.github.com/repos/$REPO/releases/latest" -ErrorAction SilentlyContinue
        $tag = $latestRelease.tag_name
    } catch {
        $tag = "v2.0.0"
    }

    $downloadUrl = "https://github.com/$REPO/releases/download/$tag/tryzub-windows-$arch.zip"

    Write-Host "  Завантаження $tag..." -ForegroundColor Blue
    $tempFile = [System.IO.Path]::GetTempFileName() + ".zip"

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile -UseBasicParsing
        Expand-Archive -Path $tempFile -DestinationPath $BIN_DIR -Force
        Remove-Item $tempFile -ErrorAction SilentlyContinue
    } catch {
        Write-Host "  Не вдалося завантажити. Встановіть Rust: https://rustup.rs" -ForegroundColor Red
        Remove-Item $tempFile -ErrorAction SilentlyContinue
        exit 1
    }
}

# Додаємо до PATH
$currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($currentPath -notlike "*$BIN_DIR*") {
    [Environment]::SetEnvironmentVariable("Path", "$BIN_DIR;$currentPath", "User")
    $env:Path = "$BIN_DIR;$env:Path"
    Write-Host "  Додано до PATH" -ForegroundColor Green
}

Write-Host ""
Write-Host "  ────────────────────────────────────────────" -ForegroundColor Green
Write-Host "  🔱 Тризуб успішно встановлено!" -ForegroundColor Green
Write-Host "  ────────────────────────────────────────────" -ForegroundColor Green
Write-Host ""
Write-Host "  Розташування: $BIN_DIR" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Щоб почати:" -ForegroundColor Yellow
Write-Host "    tryzub запустити програма.тризуб" -ForegroundColor Blue
Write-Host ""
Write-Host "  Або створіть новий проект:" -ForegroundColor Yellow
Write-Host "    tryzub новий мій_проект" -ForegroundColor Blue
Write-Host ""
Write-Host "  Документація: https://github.com/$REPO" -ForegroundColor Cyan
Write-Host ""
