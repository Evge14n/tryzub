#!/bin/bash
# Скрипт встановлення мови програмування Тризуб
# Використання: curl -sSL https://evge14n.github.io/tryzub/install.sh | sh

set -e

REPO="Evge14n/tryzub"
INSTALL_DIR="$HOME/.tryzub"
BIN_DIR="$INSTALL_DIR/bin"

# Кольори
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

echo ""
echo -e "${YELLOW}🔱 Встановлення мови програмування Тризуб${NC}"
echo -e "${CYAN}───────────────────────────────────────────${NC}"
echo ""

# Визначаємо ОС та архітектуру
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux*)     PLATFORM="linux";;
    Darwin*)    PLATFORM="macos";;
    MINGW*|MSYS*|CYGWIN*) PLATFORM="windows";;
    *)          echo -e "${RED}Непідтримувана ОС: $OS${NC}"; exit 1;;
esac

case "$ARCH" in
    x86_64|amd64)   ARCH="x86_64";;
    aarch64|arm64)   ARCH="aarch64";;
    *)              echo -e "${RED}Непідтримувана архітектура: $ARCH${NC}"; exit 1;;
esac

echo -e "${BLUE}Платформа:${NC} $PLATFORM-$ARCH"

# Перевіряємо залежності
check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo -e "${RED}Потрібен $1 для встановлення${NC}"
        exit 1
    fi
}

# Вибираємо метод встановлення
if command -v cargo &> /dev/null; then
    echo -e "${GREEN}Знайдено Rust/Cargo — збірка з вихідного коду${NC}"
    echo ""

    # Клонуємо та збираємо
    TEMP_DIR=$(mktemp -d)
    echo -e "${BLUE}Завантаження вихідного коду...${NC}"
    git clone --depth 1 "https://github.com/$REPO.git" "$TEMP_DIR/tryzub" 2>/dev/null

    echo -e "${BLUE}Збірка (може зайняти кілька хвилин)...${NC}"
    cd "$TEMP_DIR/tryzub"
    cargo build --release 2>&1 | tail -1

    # Встановлюємо
    mkdir -p "$BIN_DIR"
    cp "target/release/tryzub" "$BIN_DIR/tryzub" 2>/dev/null || \
    cp "target/release/tryzub.exe" "$BIN_DIR/tryzub.exe" 2>/dev/null || true

    # Копіюємо стандартну бібліотеку
    mkdir -p "$INSTALL_DIR/stdlib"
    cp -r stdlib/* "$INSTALL_DIR/stdlib/" 2>/dev/null || true

    # Прибираємо
    rm -rf "$TEMP_DIR"
else
    echo -e "${YELLOW}Rust не знайдено — завантаження попередньо зібраного бінарного файлу${NC}"
    echo ""

    check_command curl

    # Визначаємо URL для завантаження
    LATEST_TAG=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$LATEST_TAG" ]; then
        LATEST_TAG="v2.0.0"
    fi

    if [ "$PLATFORM" = "windows" ]; then
        EXT="zip"
        BINARY="tryzub.exe"
    else
        EXT="tar.gz"
        BINARY="tryzub"
    fi

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/tryzub-$PLATFORM-$ARCH.$EXT"

    echo -e "${BLUE}Завантаження $LATEST_TAG...${NC}"
    mkdir -p "$BIN_DIR"

    TEMP_FILE=$(mktemp)
    if curl -sSLf "$DOWNLOAD_URL" -o "$TEMP_FILE" 2>/dev/null; then
        if [ "$EXT" = "tar.gz" ]; then
            tar xzf "$TEMP_FILE" -C "$BIN_DIR"
        else
            unzip -qo "$TEMP_FILE" -d "$BIN_DIR"
        fi
        rm -f "$TEMP_FILE"
        chmod +x "$BIN_DIR/$BINARY" 2>/dev/null || true
    else
        echo -e "${RED}Не вдалося завантажити бінарний файл.${NC}"
        echo -e "${YELLOW}Встановіть Rust (https://rustup.rs) та запустіть цей скрипт знову.${NC}"
        rm -f "$TEMP_FILE"
        exit 1
    fi
fi

# Додаємо до PATH
SHELL_CONFIG=""
case "$SHELL" in
    */zsh)  SHELL_CONFIG="$HOME/.zshrc";;
    */bash) SHELL_CONFIG="$HOME/.bashrc";;
    */fish) SHELL_CONFIG="$HOME/.config/fish/config.fish";;
esac

PATH_LINE="export PATH=\"$BIN_DIR:\$PATH\""

if [ -n "$SHELL_CONFIG" ]; then
    if ! grep -q "$BIN_DIR" "$SHELL_CONFIG" 2>/dev/null; then
        echo "" >> "$SHELL_CONFIG"
        echo "# Тризуб - мова програмування" >> "$SHELL_CONFIG"
        echo "$PATH_LINE" >> "$SHELL_CONFIG"
        echo -e "${GREEN}Додано до PATH в $SHELL_CONFIG${NC}"
    fi
fi

echo ""
echo -e "${GREEN}────────────────────────────────────────────${NC}"
echo -e "${GREEN}🔱 Тризуб успішно встановлено!${NC}"
echo -e "${GREEN}────────────────────────────────────────────${NC}"
echo ""
echo -e "  ${CYAN}Розташування:${NC} $BIN_DIR"
echo ""
echo -e "  ${YELLOW}Щоб почати:${NC}"
echo -e "    ${BLUE}tryzub запустити програма.тризуб${NC}"
echo ""
echo -e "  ${YELLOW}Або створіть новий проект:${NC}"
echo -e "    ${BLUE}tryzub новий мій_проект${NC}"
echo ""

if [ -n "$SHELL_CONFIG" ]; then
    echo -e "  ${YELLOW}Перезавантажте термінал або виконайте:${NC}"
    echo -e "    ${BLUE}source $SHELL_CONFIG${NC}"
    echo ""
fi

echo -e "  ${CYAN}Документація:${NC} https://github.com/$REPO"
echo -e "  ${CYAN}Приклади:${NC}      https://github.com/$REPO/tree/main/examples"
echo ""
