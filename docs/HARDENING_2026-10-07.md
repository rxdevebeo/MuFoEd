# Hardening 2026-10-07

Дата: 7–8 октября 2026 года. Ветка: `task/hardening-2026-10-07` от `462882a`.
Статус: в работе.

Автономная задача владельца: исправить всё исправимое, причесать код, внедрить
оправданные практики и архитектурные правки, покрыть тестами.

Правила работы:

- Сборка и тесты — только на Windows-хосте через `tools/hostq` (`gate` по точному
  SHA). В VM не собирается ничего.
- Master и основная копия не трогаются; ветка не пушится и не мержится.
- Каждое изменение — отдельный коммит с объяснением причины; каждая партия
  коммитов проверяется `gate` на хосте, номер прогона записан ниже.

## 1. Исходное состояние

| SHA | fmt | clippy | test | Примечание |
|---|---|---|---|---|
| `9fec5cb` | ✅ | ✅ | ✅ 1452/0 | последний зелёный CI (Linux), зелёный на Windows-хосте |
| `7ea3fd9` | ❌ | ❌ | ❌ 1471/8 + abort | |
| `462882a` | ❌ | ❌ | ❌ 1478/8 + abort | HEAD master на начало работы |

Падения на `462882a` (Windows, debug, `--all-features`):

| Тест | Симптом |
|---|---|
| `strict-ooxml --test hostile` | переполнение стека в `testkit-bounded`, бинарник аварийно завершён |
| `wml misc::model_variant_sizes_are_bounded` | `Block is 3104 bytes` |
| `wml drawing_stage5b::inline_shape_parse_and_unknown_preset` | `a:fill` больше не в support |
| `wml unmodelled_pr::property_change_and_tbl_style_pr_are_recorded` | `unwrap()` на `None` |
| `write renamed_media::copied_media_is_not_lost_but_an_omitted_part_is_reported` | потеря части не названа в отчёте |
| `convert f18_geometry::f18_pdf_baseline_and_x_survive_write` | baseline 124,351 px вместо 122,667 |
| `pdf pdf_pixels` ×2 | запас SSIM `strict-stage5b` +0,0094 < +0,0100 |
| `render-pdf p6_table_page` | пишет в несуществующий `../target` |

## 2. Журнал

(заполняется по ходу работы)
