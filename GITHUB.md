# 🚀 Завантаження на GitHub

## Швидкий старт

### 1. Створіть репозиторій на GitHub

1. Перейдіть на https://github.com/new
2. Назва репозиторію: `tryzub` 
3. Опис: `Тризуб - найшвидша україномовна мова програмування. Автор: Мартинюк Євген`
4. Зробіть репозиторій **публічним**
5. НЕ додавайте README, .gitignore або ліцензію

### 2. Завантажте код

Відкрийте термінал PowerShell в папці проекту:

```powershell
cd C:\Users\Євген\Desktop\main\Tryzub

# Ініціалізація Git
git init

# Додавання файлів
git add .

# Перший коміт
git commit -m "🇺🇦 Тризуб v1.0.0 - Початковий реліз

Автор: Мартинюк Євген
Створено: 06.04.2025

- Повна реалізація лексера, парсера та VM
- Український синтаксис
- Стандартна бібліотека
- Приклади програм
- Документація"

# Додайте remote (замініть YOUR_USERNAME)
git branch -M main
git remote add origin https://github.com/YOUR_USERNAME/tryzub.git

# Завантажте
git push -u origin main
```

### 3. Налаштуйте GitHub

Після завантаження:

1. **Додайте теми**: 
   - `ukrainian`
   - `programming-language`
   - `compiler`
   - `llvm`
   - `rust`

2. **Увімкніть GitHub Pages**:
   - Settings → Pages
   - Source: Deploy from a branch
   - Branch: main, folder: /docs

3. **Створіть Release**:
   - Перейдіть в Releases → Create new release
   - Tag: `v1.0.0`
   - Title: `Тризуб v1.0.0 - Перший реліз`
   - Опис:
   ```markdown
   # 🇺🇦 Тризуб v1.0.0
   
   Перший офіційний реліз української мови програмування Тризуб!
   
   ## ✨ Можливості
   - Повна підтримка українського синтаксису
   - Інтерпретатор та компілятор
   - Стандартна бібліотека
   - Приклади програм
   
   ## 👨‍💻 Автор
   Мартинюк Євген
   
   ## 📅 Дата створення
   06.04.2025
   ```

### 4. Додайте файл .gitattributes

Створіть файл `.gitattributes`:

```
*.тризуб linguist-language=Tryzub
*.rs linguist-language=Rust
*.toml linguist-language=TOML
docs/* linguist-documentation
examples/* linguist-documentation=false
```

### 5. Створіть GitHub Actions для автоматичної збірки

Файл вже створено в `.github/workflows/ci.yml`

## 📋 Чек-лист перед публікацією

- [ ] Перевірте, що всі файли додані
- [ ] Запустіть `cargo test`
- [ ] Перевірте приклади програм
- [ ] Оновіть контактну інформацію
- [ ] Додайте скріншоти в папку docs/images

## 🎯 Після публікації

1. **Поділіться проектом**:
   - Reddit: r/programming, r/Ukraine_ua
   - Twitter/X з хештегами #УкрПрограмування #Тризуб
   - LinkedIn
   - DOU.ua

2. **Створіть демо відео**:
   - Покажіть встановлення
   - Напишіть просту програму
   - Покажіть швидкість компіляції

3. **Напишіть статтю**:
   - Про створення мови
   - Технічні деталі
   - Плани на майбутнє

## 💡 Поради

- Використовуйте GitHub Discussions для спілкування
- Створіть шаблони для Issues та PR
- Додайте Contributing Guidelines
- Налаштуйте Code of Conduct

---

**Успіхів з публікацією мови Тризуб! 🇺🇦**
