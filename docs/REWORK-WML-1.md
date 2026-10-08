# Заказ на доработку. WML — пролог перед корневым элементом

**Шифр:** ZAKAZ-WML-1 (REWORK-WML-1)  
**Крейт:** `strict-ooxml-wml`  
**Основание:** целенаправленный поиск реальных Strict-файлов на GitHub; оба найденных Strict-документа **не парсятся**  
**Обнаружил:** приёмка (поиск Strict-корпуса)  
**Дата:** 2026-09-28  
**Приоритет:** высокий (блокирует любые реальные Strict-документы, сохранённые типовым образом)  
**Оценка:** 4–8 ч

> Только требования и критерии приёмки. Код не приводится.

---

## 1. Проблема C-3. Пробел в прологе перед корнем ломает парсер

**Факт.** Два реальных Strict-документа (найдены на GitHub) не парсятся:

```
error: invalid XML at /word/document.xml:1:56: expected 'w:document' root element
```

- `kklimuk/docx-cli` → `tests/fixtures/strict-profile.docx` (MIT);
- `Esword618/unioffice` → `document/testdata/strict.docx` (AGPL, локально).

Оба: `conformance: Strict` (детекция core работает), но `check`/`report`/`render`
падают (`exit 2`). В обоих `document.xml` декларация и корень — на разных строках:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main" ...>
```

**Контролируемый эксперимент (воспроизведено):**

| `document.xml` | `check` |
|---|---|
| `<?xml …?><w:document …>…` (одна строка) | ✅ `ok: strict` |
| `<?xml …?>\n<w:document …>…` (как в реальных файлах) | ❌ `expected 'w:document' root element` |

**Причина.** XML разрешает пробелы (и комментарии/PI) в прологе между
декларацией и корневым элементом. `XmlReader` (core) отдаёт такие пробелы как
`XmlEvent::Text`. Парсеры корневых частей Этапа 2 принимают **первым событием
только `StartElement`** и на ведущем текстовом узле возвращают ошибку.

Затронуты (одинаковый шаблон `match next_event { StartElement if local==… => …,
_ => Err("expected '…' root element") }`):
- `parse_document_root` (`document.xml`);
- `parse_styles_root` (`styles.xml`);
- `parse_numbering_root` (`numbering.xml`);
- `parse_settings_root` (`settings.xml`).

**Почему тесты не поймали.** Все синтетические фикстуры писались в одну строку
(декларация и корень вместе); реальные Strict-файлы всегда разбивают их по
строкам. Тот же класс «самосогласованных фикстур», что D-1/D-2 (Этап 2).

**Последствия.** Любой реальный Strict-документ, сохранённый типовым образом
(Word «Strict Open XML Document»), не читается вообще. Это блокирует корпуса
Этапов 2–4 и продуктовое обещание «читает Strict».

---

## 2. Требование C-3

1. Перед корневым элементом части **пропускать пролог**: пробелы/переводы строк
   (`XmlEvent::Text`/`CData` из одних пробельных символов), а также уже
   игнорируемые комментарии/PI/декларацию, — до первого `StartElement`.
2. Корневой элемент определять по локальному имени **и** строго по
   Strict-пространству имён (как сейчас — не ослаблять проверку конформанса;
   сама конформанс-детекция уже корректна).
3. Применить ко всем четырём корневым парсерам (`document`/`styles`/`numbering`/
   `settings`), не меняя поведение на уже корректных входах.
4. Не «пропускать» значимый текст: пропуск допускается только для
   пробельных/служебных узлов до корня; невалидный контент до корня — ошибка.
5. Никаких паник; ошибка — `Result` с локацией.

**Критерии приёмки C-3.**
1. `decl\nroot` для `document.xml` парсится (`ok: strict`), как и `decl root`
   (одна строка) — регресс-тест на оба варианта.
2. То же для `styles.xml`, `numbering.xml`, `settings.xml` (ведущий `\n`/CRLF
   перед корнем) — тесты на каждый.
3. `tests/strict/strict-profile.docx` (MIT, теперь в репозитории) **парсится**:
   `check` → `exit 0` (или `1` только при реальных `unsupported`), `report`
   формируется, `render` выпускает ≥ 1 страницу.
4. Пробельный/комментарийный пролог с несколькими строками и `\r\n` — проходит.
5. Невалидный префикс/чужой namespace в корне — по-прежнему ошибка.

---

## 3. Корпус (включение документов по согласованной схеме)

- Включить реальный Strict-фикстур `strict-profile.docx` из `kklimuk/docx-cli`
  (**MIT**) в `strict-ooxml-core/tests/strict/` — **выполнено** (с атрибуцией,
  см. `tests/strict/README.md`).
- AGPL-файл `Esword618/unioffice` **не коммитить** (несовместимая лицензия) —
  только локальная проверка.
- После фикса C-3 добавить тест(ы) по `tests/strict/`: WML-парсинг; Feature Report
  (Этап 3) — валидный отчёт; рендер (Этап 4) — ≥ 1 страница. Это закрывает
  открытый пункт O2 приёмки Этапа 4 (реальный Strict-вход).
- Желательно ≥ 3 реальных Strict-документа; найденных перспективных источников
  (кроме MIT-файла) пока нет — расширять по мере находок.

> Лицензионное примечание: при добавлении сторонних фикстур сохранять их
> лицензию/атрибуцию; AGPL/иные несовместимые — не включать.

---

## 4. Что НЕ входит

- Ослабление проверки пространств имён Strict/Transitional.
- Приём Transitional (нормализация — Этап 6).
- Изменение модели/DOM.

---

## 5. Порядок сдачи и повторная приёмка

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test  --workspace --all-features
cargo run -p strict-ooxml-cli -- check  strict-ooxml-core/tests/strict/strict-profile.docx
cargo run -p strict-ooxml-cli -- report strict-ooxml-core/tests/strict/strict-profile.docx --text
cargo run -p strict-ooxml-cli -- render strict-ooxml-core/tests/strict/strict-profile.docx --out out/
```

Ожидается: `check` открывает файл; отчёт и SVG формируются; все гейты зелёные.

---

## 6. Definition of Done

- [ ] **C-3**: пролог (пробелы/комментарии/PI/декларация) перед корнем
      допускается во всех четырёх корневых парсерах.
- [ ] Регресс-тесты `decl\nroot` для document/styles/numbering/settings.
- [ ] `tests/strict/strict-profile.docx` (MIT) парсится, отчёт и рендер проходят;
      добавлены тесты по этому каталогу.
- [ ] AGPL-файл не закоммичен.
- [ ] Гейты (fmt/clippy/test/coverage) зелёные; повторная приёмка пройдена.
- [ ] Обновлён `docs/strict-corpus`/README при необходимости.

---

## 7. Приложение. Репро

- Файлы: `tests/strict/strict-profile.docx` (MIT, репозиторий);
  `<local>/Esword618__unioffice/document/testdata/strict.docx` (AGPL, только локально).
- Оба: `conformance: Strict`, `check` → `expected 'w:document' root element`.
- Мини-репро: тот же Strict `document.xml` в одну строку — `ok: strict`; с `\n`
  между `?>` и `<w:document` — ошибка.
- Затронутые функции: `strict-ooxml-wml/src/parse/{document,styles,numbering,settings}.rs`
  (парсеры корневых частей).

---

**Конец заказа.**
