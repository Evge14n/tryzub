# Внесок у проект Тризуб

Дякуємо за ваш інтерес до внеску в мову програмування Тризуб! 🇺🇦

## 📋 Зміст

- [Кодекс поведінки](#кодекс-поведінки)
- [Як я можу допомогти?](#як-я-можу-допомогти)
- [Початок роботи](#початок-роботи)
- [Процес розробки](#процес-розробки)
- [Стиль коду](#стиль-коду)
- [Тестування](#тестування)
- [Документація](#документація)
- [Pull Request процес](#pull-request-процес)

## 📜 Кодекс поведінки

Цей проект дотримується [Кодексу поведінки](CODE_OF_CONDUCT.md). Беручи участь, ви погоджуєтесь дотримуватися його умов.

## 🤝 Як я можу допомогти?

### 🐛 Повідомлення про баги

- Перевірте [існуючі issues](https://github.com/tryzub-lang/tryzub/issues), щоб уникнути дублювання
- Використовуйте [шаблон для багів](.github/ISSUE_TEMPLATE/bug_report.md)
- Надайте детальний опис проблеми
- Включіть кроки для відтворення
- Вкажіть версію Тризуб та операційну систему

### 💡 Пропозиції функцій

- Спочатку обговоріть ідею в [Discord](https://discord.gg/tryzub)
- Створіть issue з детальним описом
- Поясніть, чому ця функція буде корисною
- Наведіть приклади використання

### 📝 Покращення документації

- Виправляйте помилки та неточності
- Додавайте приклади коду
- Перекладайте документацію
- Покращуйте пояснення

### 🔧 Внесок коду

- Виправляйте баги
- Реалізуйте нові функції
- Покращуйте продуктивність
- Додавайте тести

## 🚀 Початок роботи

### Налаштування середовища

1. **Форкніть репозиторій**
   ```bash
   gh repo fork tryzub-lang/tryzub
   ```

2. **Клонуйте ваш форк**
   ```bash
   git clone https://github.com/YOUR_USERNAME/tryzub.git
   cd tryzub
   ```

3. **Додайте upstream remote**
   ```bash
   git remote add upstream https://github.com/tryzub-lang/tryzub.git
   ```

4. **Встановіть залежності**
   ```bash
   # Rust (потрібна версія 1.70+)
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # LLVM
   # Ubuntu/Debian
   sudo apt-get install llvm-15-dev
   
   # macOS
   brew install llvm@15
   
   # Windows
   choco install llvm
   ```

5. **Зберіть проект**
   ```bash
   cargo build
   ```

6. **Запустіть тести**
   ```bash
   cargo test
   ```

## 💻 Процес розробки

### Структура проекту

```
tryzub/
├── src/
│   ├── lexer/       # Лексичний аналізатор
│   ├── parser/      # Синтаксичний аналізатор
│   ├── compiler/    # Компілятор (LLVM backend)
│   ├── vm/          # Віртуальна машина
│   └── runtime/     # Runtime бібліотека
├── stdlib/          # Стандартна бібліотека
├── tests/           # Інтеграційні тести
├── examples/        # Приклади коду
└── docs/            # Документація
```

### Гілки

- `main` - стабільна версія
- `develop` - активна розробка
- `feature/*` - нові функції
- `bugfix/*` - виправлення багів
- `release/*` - підготовка релізів

### Workflow

1. **Створіть нову гілку**
   ```bash
   git checkout -b feature/моя-функція
   ```

2. **Внесіть зміни**
   - Пишіть чистий, зрозумілий код
   - Дотримуйтесь стилю проекту
   - Додавайте коментарі де потрібно

3. **Комітьте зміни**
   ```bash
   git add .
   git commit -m "feat: додав нову функцію X"
   ```

   Формат коміт-повідомлень:
   - `feat:` - нова функція
   - `fix:` - виправлення багу
   - `docs:` - зміни документації
   - `style:` - форматування коду
   - `refactor:` - рефакторинг
   - `test:` - додавання тестів
   - `chore:` - інші зміни

4. **Синхронізуйтесь з upstream**
   ```bash
   git fetch upstream
   git rebase upstream/develop
   ```

## 🎨 Стиль коду

### Rust код

- Використовуйте `rustfmt` для форматування
- Дотримуйтесь [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/README.html)
- Запускайте `cargo clippy` перед комітом

```rust
// Приклад
pub fn compile_expression(&mut self, expr: Expression) -> Result<Value> {
    match expr {
        Expression::Literal(lit) => self.compile_literal(lit),
        Expression::Binary { left, op, right } => {
            let lhs = self.compile_expression(*left)?;
            let rhs = self.compile_expression(*right)?;
            self.apply_binary_op(op, lhs, rhs)
        }
        _ => Err(CompileError::NotImplemented),
    }
}
```

### Тризуб код

```tryzub
// Використовуйте описові імена
функція обчислити_площу_кола(радіус: дрб64) -> дрб64 {
    повернути математика.ПІ * радіус * радіус
}

// Документуйте публічні функції
/// Обчислює факторіал числа
/// 
/// # Параметри
/// * `n` - невід'ємне ціле число
/// 
/// # Повертає
/// Факторіал числа n
публічний функція факторіал(n: цл64) -> цл64 {
    якщо (n <= 1) {
        повернути 1
    }
    повернути n * факторіал(n - 1)
}
```

## 🧪 Тестування

### Юніт-тести

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_numbers() {
        let input = "123 45.67";
        let tokens = tokenize(input).unwrap();
        assert_eq!(tokens.len(), 3); // 2 числа + EOF
    }
}
```

### Інтеграційні тести

Додавайте тести в `tests/`:

```rust
// tests/integration_test.rs
#[test]
fn test_compile_and_run() {
    let source = include_str!("../examples/hello.tryzub");
    let result = compile_and_run(source).unwrap();
    assert_eq!(result.exit_code, 0);
}
```

### Запуск тестів

```bash
# Всі тести
cargo test

# Конкретний тест
cargo test test_lexer_numbers

# З виводом
cargo test -- --nocapture

# Тільки інтеграційні
cargo test --test '*'
```

## 📚 Документація

### Коментарі в коді

```rust
/// Компілює AST в LLVM IR
/// 
/// # Arguments
/// * `ast` - Абстрактне синтаксичне дерево
/// * `options` - Опції компіляції
/// 
/// # Returns
/// Скомпільований модуль або помилку
pub fn compile(ast: Program, options: CompileOptions) -> Result<Module> {
    // TODO: Implement optimization passes
    unimplemented!()
}
```

### Документація користувача

- Розміщується в `docs/`
- Використовуйте Markdown
- Додавайте приклади коду
- Перевіряйте орфографію

## 🔄 Pull Request процес

1. **Перевірте готовність**
   - [ ] Код відповідає стилю проекту
   - [ ] Всі тести проходять
   - [ ] Додані нові тести для нового функціоналу
   - [ ] Документація оновлена
   - [ ] Changelog оновлений

2. **Створіть Pull Request**
   - Використовуйте описовий заголовок
   - Заповніть шаблон PR
   - Посилайтесь на відповідні issues
   - Додайте скріншоти якщо доречно

3. **Code Review**
   - Відповідайте на коментарі
   - Вносьте запитані зміни
   - Будьте готові до обговорення

4. **Після схвалення**
   - Squash коміти якщо потрібно
   - Переконайтесь що CI проходить
   - Maintainer зробить merge

## 🏆 Визнання

Всі контрибутори будуть додані до:
- [AUTHORS.md](AUTHORS.md)
- Секції "Подяки" в README
- Списку контрибуторів на сайті

## ❓ Питання?

- **Discord**: [discord.gg/tryzub](https://discord.gg/tryzub)
- **Email**: contribute@tryzub-lang.org
- **Discussions**: [GitHub Discussions](https://github.com/tryzub-lang/tryzub/discussions)

---

Дякуємо за ваш внесок у розвиток української мови програмування! 🇺🇦 💙 💛
