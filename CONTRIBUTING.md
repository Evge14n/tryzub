# Внесок у проект Тризуб

Дякуємо за інтерес до мови Тризуб!

## Вимоги

- Rust 1.70+
- Git

## Як почати

```bash
git clone https://github.com/Evge14n/tryzub.git
cd tryzub
cargo build
cargo test -p tryzub-lexer -p tryzub-parser -p tryzub-vm
```

## Структура проекту

```
src/
  lexer/    — Лексичний аналізатор (токенізація)
  parser/   — Синтаксичний аналізатор (AST)
  vm/       — Віртуальна машина (інтерпретація)
  compiler/ — LLVM компілятор (optional, потребує LLVM 15)
  runtime/  — Runtime система (optional)
stdlib/     — Стандартна бібліотека мовою Тризуб
examples/   — Приклади програм
docs/       — Сайт та документація
```

## Як внести зміни

1. Форкніть репозиторій
2. Створіть гілку: `git checkout -b фіча/моя-фіча`
3. Напишіть тести для нової функціональності
4. Переконайтеся що `cargo test` проходить
5. Закомітьте: `git commit -m 'Додано нову фічу'`
6. Запуште: `git push origin фіча/моя-фіча`
7. Створіть Pull Request

## Стиль коміт-повідомлень

Використовуйте описові повідомлення українською:

```
Додано pattern matching для кортежів
Виправлено обробку помилок у VM
Оновлено документацію stdlib
```

## Тестування

Кожна нова конструкція мови повинна мати:
- Unit-тести в `#[cfg(test)]` модулі відповідного крейту
- Приклад програми в `examples/`

```bash
# Запуск всіх тестів
cargo test -p tryzub-lexer -p tryzub-parser -p tryzub-vm

# Запуск конкретного тесту
cargo test -p tryzub-vm test_match_expression
```

## Ліцензія

Всі внески ліцензуються під MIT.
