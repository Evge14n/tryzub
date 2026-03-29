# Тризуб для VS Code

Syntax highlighting та LSP для мови програмування Тризуб.

## Встановлення

1. Скопіюйте цю папку в `~/.vscode/extensions/tryzub-lang`
2. Перезапустіть VS Code
3. Відкрийте `.тризуб` або `.tryzub` файл

## Можливості

- Syntax highlighting (ключові слова, рядки, числа, коментарі)
- Autocomplete (вбудовані функції + функції з файлу)
- Hover (сигнатури функцій)
- Diagnostics (помилки парсера в реальному часі)

## LSP

Для повної підтримки LSP потрібен `tryzub` в PATH:
```bash
tryzub lsp
```
