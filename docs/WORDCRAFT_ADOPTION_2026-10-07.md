# WordCraft — что перенять и как добиться never-crash

**Источник:** [storytold/wordcraft](https://github.com/storytold/wordcraft) (clean-room Word на Rust; см. также `AGENTS.md` в том репо).  
**Дата:** 2026-10-07  
**Контекст:** заметки после обзора; не ADR и не обязательство внедрять сразу.  
**Связано у нас:** `docs/fuzz-protocol.md`, `docs/core-limits-audit.md`, workspace lints в корневом `Cargo.toml`, property-тесты `*_never_panic*`.

WordCraft — редактор с UX-паритетом Word. StrictLib — Strict OOXML write/render с доказательной приёмкой (census, inventory). Цели разные; ниже только то, что полезно **нам**.

---

## 1. Что можно перенять

### Высокий приоритет (близко к текущей работе)

| Идея WordCraft | Как применить в StrictLib | Не путать с |
|---|---|---|
| Слои: семантика → layout (display list) → draw | Явно держать модель/write отдельно от пагинации и от SVG/PDF/raster. Один layout — несколько бэкендов. | Census/Strict identity пакета — **другая ось**, layout её не заменяет |
| Font resolve: metric-compatible substitutes + Windows ascent/descent | Для `render-svg` / `render-pdf`: Carlito↔Calibri, Caladea↔Cambria, Liberation; линия как у Word | Не бандлить проприетарные шрифты; не подменять identity embedded fonts в write |
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

**Постепенный clippy (не big-bang):**

```toml
# сначала на crate roots write / render-pdf / render-svg / cli entry:
# #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
# затем подтянуть wml/core после зачистки
```

Workspace сейчас `missing_panics_doc = "allow"` — не отменяет запрет паник в коде.

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

### 4.5. Что не смешивать с never-crash

- **Census FAIL / unclassified** — это fidelity, не crash.
- **Отказ открыть Transitional в StrictOnly** — корректный `Err`.
- **OOM killer ОС** при снятых лимитах — вне контракта; контракт = наши caps.

---

## 5. Рекомендуемый порядок внедрения (если решим делать)

1. Зафиксировать этот документ как ориентир (сделано).
2. Включить `clippy::unwrap_used` / `expect_used` / `panic` на `strict-ooxml-cli` + render crates; чинить по мере CI.
3. Добавить один интеграционный `convert_pipeline_never_panics` (proptest или corpus hostile) рядом с `fuzz_docx_full`.
4. Для визуала: вынести «display list» как границу API, если ещё размазано между SVG и PDF.
5. Font substitution таблицу — только в render, с тестом метрик строки; write не трогать.

---

## 6. Ссылки

- WordCraft README / ROADMAP / AGENTS: https://github.com/storytold/wordcraft  
- Наш fuzz: `docs/fuzz-protocol.md`  
- Лимиты: `docs/core-limits-audit.md`, `strict-ooxml-core/src/limits.rs`  
- Визуальный план: `docs/AUDIT_VISUAL_PRIORITY_PLAN_2026-10-06.md`
