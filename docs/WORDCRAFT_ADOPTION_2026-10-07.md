# WordCraft — что перенять и как добиться never-crash

**Источник:** [storytold/wordcraft](https://github.com/storytold/wordcraft) (clean-room Word на Rust; см. также `AGENTS.md` в том репо).  
**Дата:** 2026-10-07  
**Контекст:** заметки после обзора; не ADR и не обязательство внедрять сразу.  
**Обновлено:** 2026-10-08 — сверено с кодом после слияния `task/hardening-2026-10-07`; план §5 принят к реализации (ветка `task/never-crash-2026-10-08`).  
**Связано у нас:** `docs/fuzz-protocol.md`, `docs/core-limits-audit.md`, workspace lints в корневом `Cargo.toml`, property-тесты `*_never_panic*`.

WordCraft — редактор с UX-паритетом Word. StrictLib — Strict OOXML write/render с доказательной приёмкой (census, inventory). Цели разные; ниже только то, что полезно **нам**.

---

## 1. Что можно перенять

### Высокий приоритет (близко к текущей работе)

| Идея WordCraft | Как применить в StrictLib | Не путать с |
|---|---|---|
| Слои: семантика → layout (display list) → draw | **Уже в основном так:** `render-pdf` не раскладывает сам, а берёт `place_pages` и `layout::{RectItem, LineItem, ImageItem, …}` из `render-svg`. Осталось оформить display list как явный модуль/API, а не набор реэкспортов. | Census/Strict identity пакета — **другая ось**, layout её не заменяет |
| Font resolve: metric-compatible substitutes + Windows ascent/descent | **Таблица подстановок уже есть** (`render-svg/src/font/family.rs`, `render-pdf/src/font.rs`: Carlito, Caladea, Liberation). Не хватает теста метрик строки (ascent/descent/line gap как у Word). | Не бандлить проприетарные шрифты; не подменять identity embedded fonts в write |
| PDF: selectable text, subset fonts, JPEG passthrough, tagged (best effort) | Сверять с нашим PDF-пайплайном; не ломать text extraction ради «красивой картинки» | Их `krilla`-стек не обязан быть нашим |
| Честная метрика «команды ≠ глубина» | Для визуала: отдельно % inventory cleared vs реальная pixel/layout fidelity | Их `parity.md` (кнопки ленты) нам не подходит |

### Средний приоритет (инженерия / процесс)

| Идея | Как применить |
|---|---|
| `never-crash` выше фич | Любой crash fix раньше нового P-пакета; регрессия обязательна |
| Hostile input: `get` вместо `[i]`, saturating/checked, clamp, cap alloc, bound recursion | Уже частично есть (limits, fuzz); довести до единого чеклиста ниже |
| Layering check (у них `xtask layers`) | Не тянуть UI/egui в core/wml/write; render не пишет пакет |
| Asset attribution gate | У нас уже `ATTRIBUTION-*`; не тащить MS/Adobe ассеты |
| Offscreen visual harness (`ui_shot`) | Аналог: стабильные SVG/PDF fixtures + ROI compare без ручного кликанья |
| Agent surface: command ids / inspect без скриншота | Опционально для CLI: `inspect` JSON структуры layout/report; не блокер M4 |

### Низкий приоритет / позже

| Идея | Зачем когда-нибудь |
|---|---|
| Кеш layout по ревизии абзаца | Если preview/relayout станет интерактивным |
| MCP / control channel | Только если понадобится agent-driven UI; для CLI convert не обязателен |
| egui ribbon / WASM app | Вне продукта StrictLib |

---

## 2. Чего не перенимать

- Их **DOCX write как эталон fidelity**: модельный roundtrip, charts/SmartArt/OLE drop, corpus real-world у них ещё не начат. Наши P9/P10 (fonts, `resource:*` SHA) — строже.
- Подсчёт «87% паритета» по каталогу команд ленты.
- Политику «неизвестную картинку → серый прямоугольник» **в write-пути**: в convert/write отсутствие digest = unclassified loss; skip допустим только в render с явным report.
- Копирование кода/ассетов Word или чужих GPL (LibreOffice и т.п.) — у них clean-room; у нас то же.

---

## 3. Never-crash — определение для StrictLib

**Never-crash** здесь значит: на враждебном или битом вводе публичный путь (`open` → normalize/report → render → write → reopen) **не паникует, не UB, не ест память без верхней границы**. Ошибка — это `Result` / отказ / limit, не abort процесса.

Уже есть база:

- `unsafe_code = "deny"` в workspace;
- fuzz-цели и инварианты в `docs/fuzz-protocol.md`;
- лимиты ZIP/XML в `strict-ooxml-core` (`limits.rs`, `docs/core-limits-audit.md`);
- property: `package_open_never_panics`, `xml_reader_never_panics`, `random_bytes_never_panic`, harness с `catch_unwind` в testkit/view.

Ниже — как **дожать** до стандарта уровня WordCraft, без слепого копирования их clippy-deny на всё сразу.

---

## 4. Как добиться never-crash (практический план)

### 4.1. Контракт по слоям

| Слой | На panic | На ошибку |
|---|---|---|
| OPC / XML / WML parse | запрещено | `Err` / skip part с записью в report |
| Normalize / write | запрещено | `Err` или named_loss + цитата; silent drop = census FAIL |
| Layout / render-svg / render-pdf | запрещено | skip объекта + report; пустая страница лучше abort |
| CLI | запрещено наружу | non-zero exit + сообщение; опционально `catch_unwind` только как last resort с логом |

`catch_unwind` в продакшене — **сеть**, не лицензия паниковать. Каждый сработавший guard → issue + регрессия.

### 4.2. Правила кода (production paths)

1. Нет `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` / `unreachable!` на путях open→write→render (тесты — исключение).
2. Индексы и срезы: `get` / `get_mut`; строки — только по char boundary.
3. Арифметика размеров: `checked_*` / `saturating_*`; затем clamp к лимитам из `OpenOptions` / render options.
4. Аллокации: верхние границы (уже есть uncompressed totals; то же для числа частей, глубины XML, размера display list).
5. Рекурсия (стили, numbering, DrawingML): явный max depth → `Err` / degrade.
6. Новый публичный API — `Result`; «ещё не умеем» ≠ panic.

**Clippy-запрет (сделано 2026-10-08, сразу на весь workspace).** Замер показал, что
вне `#[cfg(test)]` оставалось всего 17 мест с `unwrap`/`expect`/`panic!`/`unreachable!`,
поэтому поэтапность не понадобилась. Во всех библиотечных корнях и в `main.rs` CLI/view:

```rust
#![cfg_attr(not(test), deny(
    clippy::unwrap_used, clippy::expect_used, clippy::panic,
    clippy::todo, clippy::unimplemented, clippy::unreachable
))]
```

`not(test)` оставляет `unwrap` в unit-тестах; интеграционные тесты — отдельные
крейты, на них атрибут не действует. Исключения — только точечный `#[allow(…, reason = "…")]`
(сейчас один: `fidelity::GatePolicy::shared`, тестовый гейт без политики обязан остановиться).
Вне запрета: `strict-ooxml-testkit`, `xtool`.

Найденное при зачистке: `partial_cmp(..).unwrap()` в сортировке глифов
(`render-svg/src/layout/paragraph.rs`) паниковал на NaN-координате из враждебных метрик —
заменён на `total_cmp`.

Workspace `missing_panics_doc = "allow"` — не отменяет запрет паник в коде.

**Что clippy-запрет не ловит** (порядок по реальному вкладу в краши):

1. **Индексация `[i]` и срезы `[a..b]`** — основной оставшийся источник паник, его и находит fuzz.
   Лечение: `clippy::indexing_slicing` по одному крейту (начать с `pdf`, затем `convert`, `write`).
2. **Арифметика.** В debug переполнение — паника, в release — молча неверный размер.
   Лечение: `clippy::arithmetic_side_effects` в парсерах размеров/смещений (`pdf`, `core::zip`), `checked_*`/`saturating_*`.
3. **`assert!` / `debug_assert!` в библиотечном коде** — допустимы только для внутренних инвариантов, не для входных данных.

### 4.3. Доказательства (обязательные гейты)

| Гейт | Что делать | Уже есть? |
|---|---|---|
| Unit/property never_panic | Случайные байты / фрагменты на каждый новый парсер | частично |
| Fuzz smoke CI (60s × targets) | Держать зелёным на PR | да (`fuzz-protocol`) |
| Fuzz 1h nightly | Ноль crashes/timeouts/OOM | протокол есть |
| Fuzz 24h | Release gate Stage-7 | открыт |
| Hostile corpus | Битые ZIP, гигантские attrs, nested bomby ниже лимитов, пустые media | расширять |
| Регрессия на каждый crash | Минимальный fixture + тест с именем `*_never_panic` / fuzz seed | практика |

Минимум для «never-crash claim» на convert:

```text
Package::open → support_report → render (N страниц) → write_package → reopen
```

инвариант как у `fuzz_docx_full`: no panic; successful write reopens `Ok`.

### 4.4. Чеклист на PR (короткий)

- [ ] Нет новых `unwrap`/`expect` на library path (или `allow` с однострочным why + issue).
- [ ] Входные числа/длины проходят через limits или локальный clamp.
- [ ] Ошибка наблюдаема (Result / report / census), не «тихо и упали»).
- [ ] Если чинили panic — есть регрессия (unit, proptest или fuzz seed).
- [ ] Render: неизвестный ресурс не роняет процесс (report + placeholder/skip).

### 4.5. Стек и рекурсия

**Переполнение стека — не паника.** `catch_unwind` его не ловит, процесс получает abort.
Это единственный реальный краш цикла 2026-10-07: 12 вложенных таблиц переполняли
стек тестового потока в debug-сборке (кадр рекурсивной функции резервирует место под
*все* её локальные переменные). Что сделано и что держим:

- **Явный лимит глубины на каждом рекурсивном пути** (инвентаризация 2026-10-08):

  | Путь | Лимит |
  |---|---|
  | таблицы, блочные SDT, `customXml`, сноски, колонтитулы | `max_block_nesting = 12` → отказ документа |
  | надписи (`wps:txbx`) | `max_text_box_nesting = 5` → пропуск с report |
  | inline-обёртки (`w:ins`, `w:hyperlink`, `w:fldSimple`, inline `w:sdt`, …) | `max_inline_nesting = 16` → пропуск с report |
  | группы DrawingML (`wpg:wgp`/`grpSp`) | **новое:** `max_group_nesting = 16` → пропуск с report |
  | `mc:AlternateContent` внутри своей ветки | **новое:** `MAX_MCE_NESTING = 8` → пропуск с report |
  | OMML | `max_math_depth = 64`, `max_math_nodes = 4096` |
  | `basedOn`, `numStyleLink`, граф связей OPC | итеративно, `MAX_CHAIN 64`, `MAX_LINK_HOPS 8`, `max_rel_depth 32` |
  | PDF: form XObject, дерево страниц, вложенность объектов | `max_form_depth 12`, lopdf 256 / 100 |
  | PDF: `/SMask` | кэш с множеством `in_progress`; **исправлено:** публичный `image::decode_from` больше не рекурсирует по циклу |

  Модель, построенная в коде, проверяется `nesting::check_document` в renderer и writer:
  **исправлено** — обход заходит в `w:r/w:drawing` и в inline-обёртки, writer проверяет
  колонтитулы и сноски, а не только тело.
- **Тонкие кадры:** тяжёлые ветви рекурсивной функции — в `#[inline(never)]`-помощники
  (так кадр `open` для 12 таблиц ужался 1031 → 583 KiB).
- **Гейт по стеку — `strict-ooxml/tests/hostile.rs`:** каждая вложенность на своём пределе
  проходит весь конвейер на потоке 1 MiB (`testkit::harness`), а за пределом — отказ или
  пропуск без переполнения. Измерительный probe (`STACK-PROBE stage=open|svg|write|pdf`,
  бинарный поиск размера стека в дочернем процессе) был временным, коммит `600fd90`;
  поднимать его при подозрении на регрессию кадра.
- **Библиотека не требует от вызывающего большого стека.** 64 MiB-поток в CLI — страховка
  для CLI, не контракт; документированный бюджет — стандартные 2 MiB потока Rust на
  документе в пределах лимитов по умолчанию.

### 4.6. Бюджеты PDF-ридера

К лимитам ZIP/XML (`core/limits.rs`) добавились лимиты чтения PDF (цикл 2026-10-07):
`LimitKind::ObjectStreamBytes` (суммарная распаковка object streams — защита от ObjStm-бомбы)
и `max_cached_form_bytes` (кэш form XObject). Новые декодеры (CCITT, JBIG2, JPX) обязаны
иметь такой же явный бюджет выхода до того, как попадут в путь convert.

### 4.7. Что не смешивать с never-crash

- **Census FAIL / unclassified** — это fidelity, не crash.
- **Отказ открыть Transitional в StrictOnly** — корректный `Err`.
- **OOM killer ОС** при снятых лимитах — вне контракта; контракт = наши caps.

---

## 5. Порядок внедрения (принят 2026-10-08)

| # | Шаг | Статус |
|---|---|---|
| 1 | Документ сверен с кодом | сделано |
| 2 | `deny(unwrap_used, expect_used, panic, todo, unimplemented, unreachable)` во всех библиотечных крейтах, CLI и view; 17 мест исправлено | сделано, ветка `task/never-crash-2026-10-08` |
| 3 | Явные лимиты глубины на всех рекурсивных путях (§4.5): группы, `mc:AlternateContent`, обход run content, все части в writer | сделано |
| 4 | `pipeline_never_panics`: `open → report → svg → pdf → write → reopen` на CC0 ci-core + proptest-мутации тела | сделано |
| 5 | `clippy::indexing_slicing` во всех библиотечных крейтах (кроме тестовой `fidelity`), один `allow` — const-построение CRC-таблицы | сделано; `arithmetic_side_effects` — сделано в `pdf` и `core::opc::zip` (2026-10-08) |
| 6 | Fuzz 24h как release-гейт Stage-7: назначить владельца и дату | открыто |
| 7 | Display list как явный модуль; тест метрик строки для подстановочных шрифтов | позже |

Hostile-корпус пополнять из CC0 lock-файла: документы, на которых census даёт отказ
или limit, — готовые hostile-фикстуры.

---

## 6. Ссылки

- WordCraft README / ROADMAP / AGENTS: https://github.com/storytold/wordcraft  
- Наш fuzz: `docs/fuzz-protocol.md`  
- Лимиты: `docs/core-limits-audit.md`, `strict-ooxml-core/src/limits.rs`  
- Визуальный план: `docs/AUDIT_VISUAL_PRIORITY_PLAN_2026-10-06.md`
