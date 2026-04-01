# Історія змін — Тризуб

## [9.3.0] - 2026-03-31

### VM та компіляція
- **Iterative bytecode VM**: Fibonacci(25) 969мс → 30мс (32x швидше)
- **64MB stack**: рекурсія фіб(25) працює без overflow
- **Multi-chunk compiler**: Call/Return opcodes для функцій
- **JIT Return**: proper stack frame cleanup

### DX інструменти (Production рівень)
- **Лінтер**: shadowing, невикористані імпорти/параметри, порожні catch, match coverage
- **Форматер**: пробіли навколо операторів, `--перевірка` для CI
- **Doc Generator**: AST парсинг, типи, трейти, enum, markdown, CSS як Rust docs
- **Hot Reload**: фільтр .тризуб файлів, підтримка директорій
- **LSP**: definition, hover з типами, diagnostics, enum/trait
- **VS Code**: 105 ключових слів, 25 snippets, format strings ф"..."
- **Пакетний менеджер**: версії `>=1.0` / `~2.3`, YAML залежності
- **Playground**: /api/run сервер, 5 прикладів, sandbox, share URL

### Low-level
- **FFI**: float повернення, CString pointer
- **Inline ASM**: аргументи, мітки (labels), multi-instruction
- **Native compiler**: div, mod, neg, jump, return opcodes

### Тести: 67 integration + 1 unit = 68 total, 0 warnings

---

## [9.0.0] - 2026-03-29

### Production release
- Криптографія: HMAC-SHA256, AES-GCM, MD5, Base64
- Мережа: WebSocket (tungstenite), HTTP (ureq)
- SQLite: rusqlite з bundled feature
- Regex, compression (flate2), serialport, image
- Бенчмарк: сума 1..10K за 0.02мс (bytecode VM)

---

## [8.5.0] - 2026-03-29

- stdlib: ос, випадкове, мережа
- Бенчмарк suite з порівнянням VM/Bytecode/JIT

## [8.4.0] - 2026-03-29

- Лямбда без параметрів: `|| { блок }`

## [8.3.0] - 2026-03-29

- Multi-line strings з `"""`
- README повністю переписано
- Cargo.toml metadata для crates.io

## [8.2.0] - 2026-03-29

- **Playground**: інтерактивний веб-редактор коду
- Приклади з підсвіткою

## [8.1.0] - 2026-03-28

- **Doc generator**: /// коментарі → HTML документація
- Команда `тризуб док`

## [8.0.0] - 2026-03-28

- **Type inference**: змінні запам'ятовують тип першого присвоєння

---

## [7.9.0] - 2026-03-28

- **REPL** з persistent history (rustyline)

## [7.8.0] - 2026-03-28

- **VS Code extension**: TextMate grammar + LSP client

## [7.7.0] - 2026-03-28

- **Пакетний менеджер**: install з git, lock file, module resolution

## [7.6.0] - 2026-03-28

- **GC**: scope chain pruning через Rc strong_count sweep

## [7.5.0] - 2026-03-28

- **Форматер**: оператори, коми, trailing whitespace

## [7.4.0] - 2026-03-28

- **LSP сервер**: file parsing, completions, hover, diagnostics

## [7.3.0] - 2026-03-28

- **Лінтер**: AST-based, unused variables

## [7.0.0] - 2026-03-28

- **GC для циклічних посилань**
- CallFrame з file/line tracking
- Stack traces, 'Did you mean?' помилки

---

## [6.9.0] - 2026-03-27

- Базова тип-система

## [6.8.0] - 2026-03-27

- **Async/await**: `все()`, `перегони()`, `потік()`

## [6.7.0] - 2026-03-27

- **Ліниві ітератори**: `фільтрувати`, `перетворити`, `згорнути`

## [6.6.0] - 2026-03-27

- **Трейти**: dynamic dispatch + operator overloading

## [6.5.0] - 2026-03-27

- **Генерики** (параметричний поліморфізм)

## [6.4.0] - 2026-03-27

- **Модульна система** + stdlib

## [6.3.0] - 2026-03-27

- Stack traces, error codes, test framework (`перевірити_рівне`, `перевірити_нерівне`, `перевірити_помилку`)

## [6.2.0] - 2026-03-27

- 51 новий метод: String(+17), Array(+18), Dict(+7), Set(+5)

## [6.1.0] - 2026-03-27

- **Native compiler**: AST → x86_64 → flat binary / bootable kernel

## [6.0.0] - 2026-03-27

- **x86_64 assembler**: 40+ інструкцій (mov, add, sub, mul, div, cmp, jmp, call, ret, push, pop, xor, and, or, shl, shr...)
- **FFI**: 6 аргументів + string + close

---

## [5.9.0] - 2026-03-27

- **JIT x86_64 compiler**: bytecode → native machine code
- Bounds-checked memory, memcpy/memset

## [5.8.0] - 2026-03-27

- WebSocket сервер (tungstenite)
- HTTP клієнт (ureq)
- Regex підтримка

## [5.7.0] - 2026-03-27

- Serial port (serialport crate)
- Hardware feature flag

## [5.6.0] - 2026-03-27

- SQLite через rusqlite
- Криптографія: HMAC, SHA2, AES-GCM

## [5.5.0] - 2026-03-27

- Image processing (image crate)
- Compression (flate2)

## [5.4.0] - 2026-03-27

- Pattern matching розширено: деструктуризація масивів/кортежів

## [5.3.0] - 2026-03-27

- Enum з даними: `Варіант(поле: тип)`

## [5.2.0] - 2026-03-27

- Структури з методами через `реалізація`
- Trait impl

---

## [4.1.0] - 2026-03-27

### Веб-фреймворк
- HTTP сервер на TcpListener
- Маршрутизація GET/POST/PUT/DELETE
- Шаблонізатор з XSS захистом
- SQLite ORM, JWT автентифікація

## [4.0.0] - 2026-03-26

- JSON, математика, файловий I/O, ввід, час, конвертація

## [3.9.0] - 2026-03-26

- Async/await, макроси, фаз-тести, бенчмарки

## [3.6.0] - 2026-03-26

- Словники, множини, оператор `в`, pipeline каррінг

## [3.5.0] - 2026-03-26

- Побітові оператори, контракти, тест-раннер, unsafe/comptime

---

## [2.0.0] - 2025-10-25

- Алгебраїчні типи, pattern matching, трейти
- Pipeline `|>`, лямбди, `?`, ф"...", діапазони
- Try/catch/finally, for-in, Опція/Результат

## [1.0.0] - 2025-04-06

### Перший реліз
- Лексер з українським синтаксисом
- Парсер, VM, базова LLVM інтеграція
- Типи: цл8-64, чс8-64, дрб32-64, тхт, сим, лог
- CLI: запустити, компілювати, перевірити, новий
