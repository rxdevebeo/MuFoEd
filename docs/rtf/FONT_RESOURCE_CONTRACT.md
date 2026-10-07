# RC02: font resource contract

Срез 2026-10-07; RC02 остаётся InProgress.

## Контракт

`font::FontResource` хранит borrowed bundled bytes или явно переданные `Arc<[u8]>`.
Копия handle сохраняет lifetime bytes; обращения к установленным шрифтам отсутствуют.
`from_shared(bytes, face_index, variations)` проверяет font/collection index,
отсутствие повторных axis tags, наличие осей и диапазоны design coordinates.
`FaceVariation::new` отклоняет NaN/infinity и нормализует negative zero.
Ошибки: InvalidFace, DuplicateAxis, InvalidVariation; молчаливого fallback нет.
Эта проверка не заменяет проверку лицензии, embedding rights и полного glyph coverage.

`FaceInstanceKey` содержит SHA-256 полного исходного font program, face index и
отсортированные explicit variation coordinates. Family alias не является resource
identity. Одинаковые program bytes с разным `wght` получают разные instance keys.
Оmitted axis defaults пока не сворачиваются с explicit default coordinates.

`ResolvedFaceHandle` сохраняет requested family, resolved family, requested
bold/italic, mapping provenance и resource. `resolve_bundled_handle` адаптирует
существующий bundled provider без копирования bytes. Старый Copy `ResolvedFace`,
`FaceSource` и их API сохраняются; `ResolvedFace::instance_key` даёт совместимый ключ.
ExactFamily означает совпадение family name, не glyph coverage или полноту script
support. Substitute обозначает известную замену; DefaultFallback — неизвестный face.
Requested bold/italic не являются доказательством synthetic styling single-face font.

Parsed-face/shaped-run caches включают resource/instance identity, сохраняя family
и requested style для legacy metadata. PDF font collection/build теперь получает
source через тот же cached `resolve_face`, что shaping. Его внутренний collector
пока использует legacy bundled family/style keys: admission external assets в PDF
потребует отдельной миграции collector и embedding к resource handle.

## Проверки

Четыре resource tests: borrowed adapter identity/provenance, shared bytes после
освобождения исходного Arc, variable-weight identity, rejection malformed/index/axis
и nonfinite inputs. Отдельный cache regression под одним family/style последовательно
shape-ит разные реальные program bytes: hash и advances не смешиваются, повторный
запрос возвращает исходные glyphs.

Полные SVG/PDF/public API suites на Rust 1.98: exit 0, 411 tests в 51 группах;
единственный skip — existing `maps_regular_postscript_names_to_the_same_bundled_faces`.
После добавления cache regression повторён весь font module: 30 Pass, exit 0.
Логи: `target/rtf-import/RC02-resource-full-tests-utf8.log`,
`target/rtf-import/RC02-resource-font-final-tests.log`.

Clippy Rust 1.92 для SVG/PDF/public API, all-targets, features rtf,pdf: Pass с теми же
baseline suppressions, что предыдущий срез (manual_let_else, items_after_statements,
single_char_pattern, replace_box, while_let_loop, unnecessary_sort_by, if_same_then_else).
Лог: `target/rtf-import/RC02-resource-clippy-utf8.log`.
Changed-file rustfmt и git diff --check проверяются перед commit.
Corpus/XSD/OPC/pixels в этом срезе не повторялись; прежние receipts не выдаются за новый
результат. Следующий full-corpus run нужен после admission registry в consumers.

## Следующий срез

1. Bounded optional font-assets registry с явным manifest: family/style → resource,
   expected program hash, face index, variations, provenance, embedding rights.
   Без registry сохраняется bundled output; запрет неявного system font discovery.
2. Подключение owned handle к measurement, glyph coverage и existing harfrust pipeline,
   затем SVG delivery и PDF subset/collector. Один resource должен дойти до всех consumers.
3. Missing-resource/glyph policies: ReportAndContinue (default, Degraded) или Reject.
   Runtime report должен сохранять exact/substitute/unresolved outcome и original request.
4. CJK/Indic witnesses, Unicode extraction, deterministic hashes и corpus gates.

Registry, RenderOptions wiring, external shaping/embedding, missing-resource policies,
licensed CJK assets и полное закрытие RC02 в этом commit не реализованы.
