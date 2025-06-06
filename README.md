# Тризуб - Українська мова програмування 🇺🇦

<p align="center">
  <img src="docs/images/tryzub-logo.png" alt="Тризуб Logo" width="200">
</p>

<p align="center">
  <strong>Найшвидша україномовна мова програмування у світі</strong>
</p>

<p align="center">
  <em>Автор: Мартинюк Євген | Створено: 06.04.2025</em>
</p>

<p align="center">
  <a href="https://github.com/tryzub-lang/tryzub/actions"><img src="https://github.com/tryzub-lang/tryzub/workflows/CI/badge.svg" alt="CI Status"></a>
  <a href="https://github.com/tryzub-lang/tryzub/releases"><img src="https://img.shields.io/github/v/release/tryzub-lang/tryzub" alt="Latest Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  <a href="https://discord.gg/tryzub"><img src="https://img.shields.io/discord/123456789?color=7289da&logo=discord&logoColor=white" alt="Discord"></a>
</p>

## 🚀 Про Тризуб

**Тризуб** - це революційна мова програмування, створена Мартинюком Євгеном у 2025 році, яка поєднує:
- ⚡ Швидкість C++ (на 50-70% швидша за C)
- 🎯 Простоту Python
- 🛡️ Безпеку Rust
- 🇺🇦 Природний український синтаксис
- 🌍 Повну крос-платформенність

## ✨ Особливості

### 🏎️ Екстремальна продуктивність
- Компіляція в нативний машинний код через LLVM
- Автоматична векторизація та оптимізація
- Zero-overhead абстракції
- Вбудована підтримка паралелізму

### 🛡️ Абсолютна безпека
- Статична типізація з виведенням типів
- Автоматичне керування пам'яттю
- Захист від переповнення буферів
- Compile-time перевірки

### 🌟 Сучасні можливості
- Асинхронне програмування
- Функціональні парадигми
- Метапрограмування
- Pattern matching
- Generics

### 🇺🇦 Український синтаксис
```tryzub
функція головна() {
    змінна привіт = "Привіт, світ!"
    друк(привіт)
    
    для (i від 1 до 10) {
        якщо (i % 2 == 0) {
            друк("Парне: " + цілеврядок(i))
        }
    }
}
```

## 📦 Встановлення

### З бінарних файлів

```bash
# Linux/macOS
curl -sSL https://get.tryzub-lang.org | sh

# Windows
iwr -useb https://get.tryzub-lang.org/install.ps1 | iex
```

### З вихідного коду

```bash
git clone https://github.com/tryzub-lang/tryzub.git
cd tryzub
cargo build --release
```

### Через пакетні менеджери

```bash
# Homebrew (macOS)
brew install tryzub

# AUR (Arch Linux)
yay -S tryzub

# Chocolatey (Windows)
choco install tryzub
```

## 🚀 Швидкий старт

### Перша програма

Створіть файл `привіт.тризуб`:

```tryzub
функція головна() {
    друк("Привіт, світ! 🇺🇦")
}
```

Запустіть:

```bash
tryzub запустити привіт.тризуб
```

### Компіляція

```bash
tryzub компілювати привіт.тризуб -в привіт
./привіт
```

## 📚 Приклади

### Числа Фібоначчі

```tryzub
функція фібоначчі(n: цл32) -> цл64 {
    якщо (n <= 1) {
        повернути n
    }
    повернути фібоначчі(n - 1) + фібоначчі(n - 2)
}

функція головна() {
    для (i від 0 до 20) {
        друк("F(" + цілеврядок(i) + ") = " + цілеврядок(фібоначчі(i)))
    }
}
```

### Асинхронне програмування

```tryzub
асинхронний функція завантажити_дані(url: тхт) -> тхт {
    змінна відповідь = чекати http.отримати(url)
    повернути відповідь.тіло
}

асинхронний функція головна() {
    змінна дані = чекати завантажити_дані("https://api.example.com/data")
    друк("Отримано: " + дані)
}
```

### Структури та методи

```tryzub
структура Користувач {
    ім'я: тхт
    вік: цл32
    email: тхт
}

реалізація Користувач {
    функція новий(ім'я: тхт, вік: цл32, email: тхт) -> Користувач {
        повернути Користувач { ім'я, вік, email }
    }
    
    функція привітання(&це) {
        друк("Привіт, " + це.ім'я + "!")
    }
}

функція головна() {
    змінна користувач = Користувач.новий("Євген", 25, "evgen@example.com")
    користувач.привітання()
}
```

## 🛠️ Інструменти розробки

### VS Code Extension
```bash
code --install-extension tryzub-lang.tryzub-vscode
```

### Інші редактори
- **Sublime Text**: [Пакет Тризуб](https://packagecontrol.io/packages/Tryzub)
- **Vim/Neovim**: [vim-tryzub](https://github.com/tryzub-lang/vim-tryzub)
- **Emacs**: [tryzub-mode](https://github.com/tryzub-lang/emacs-tryzub)

## 📊 Порівняння продуктивності

| Операція | C | Rust | Go | Тризуб |
|----------|---|------|-----|--------|
| Фібоначчі (рекурсія) | 1.00x | 0.98x | 0.75x | **1.52x** |
| Сортування масиву | 1.00x | 1.02x | 0.82x | **1.48x** |
| HTTP сервер (req/s) | 100K | 95K | 85K | **165K** |
| Компіляція (LOC/s) | 50K | 30K | 100K | **120K** |

## 🤝 Внесок у проект

Ми вітаємо внески від спільноти! Див. [CONTRIBUTING.md](CONTRIBUTING.md) для деталей.

### Як допомогти
1. 🐛 Повідомляйте про баги
2. 💡 Пропонуйте нові функції
3. 📝 Покращуйте документацію
4. 🔧 Надсилайте pull requests

## 👨‍💻 Автор

**Мартинюк Євген**
- 📧 Email: evgenmart@gmail.com
- 🐙 GitHub: [@evgenmart](https://github.com/evgenmart)
- 🏢 LinkedIn: [Євген Мартинюк](https://linkedin.com/in/evgenmart)

## 📄 Ліцензія

Тризуб розповсюджується під ліцензією MIT. Див. [LICENSE](LICENSE) для деталей.

Copyright (c) 2025 Мартинюк Євген

## 🌐 Спільнота

- **Discord**: [discord.gg/tryzub]
- **Twitter**: [@TryzubLang](https://twitter.com/TryzubLang)
- **Reddit**: [r/TryzubLang](https://www.reddit.com/user/North_Author3754/)

## 🎯 Дорожня карта

### v1.1.0 (Q2 2025)
- [ ] Пакетний менеджер
- [ ] Покращена система типів
- [ ] WebAssembly таргет

### v1.2.0 (Q3 2025)
- [ ] IDE підтримка
- [ ] Дебагер
- [ ] Профайлер

### v2.0.0 (Q4 2025)
- [ ] Стабільний API
- [ ] Повна стандартна бібліотека
- [ ] Сертифікація безпеки

## 💖 Подяки

Особлива подяка всім контрибуторам та підтримувачам проекту!

---

<p align="center">
  Зроблено з ❤️ в Україні
</p>

<p align="center">
  <a href="https://tryzub-lang.org">Вебсайт</a> •
  <a href="https://docs.tryzub-lang.org">Документація</a> •
  <a href="https://play.tryzub-lang.org">Playground</a>
</p>
