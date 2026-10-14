# RC00: интеграция и повторный прогон public import

Дата замера: 2026-10-07. Worktree: `C:\Users\gamer\.codex\worktrees\rtf-import-integrated\StrictLib`; ветка `codex/rtf-import-integrated`. Основной checkout `D:\projects\StrictLib` не изменялся.

Интеграционная история: `ac51b68` (main) → `c1e9a91` (RTF parser/scaffold) → `a13739c` (явный WPS профиль) → `48beb68` (структурная интеграция) → `8e612a8` (диагностика отсутствующих glyph). Во время интеграции сохранены main P5/P6, cluster-aware ToUnicode CMap, writer suffix validation, XSD policy gate и multi-corpus viewer; RTF wiring объединён с этими контрактами. Модель RTF WML обновлена к текущим полям main.

## Проверки после интеграции

| Проверка | Результат |
| --- | --- |
| Public corpus import | 100/100 измерены |
| Точный body text | 100/100 Pass |
| Целостность результата и supervised resource gate | 100/100 Pass |
| Exit code | 100/100 = 1 (Degraded, из-за зарегистрированных import losses); exit 0 = 0 |
| Import loss events | 1,327,900 суммарно; это события диагностики, не число уникальных ошибок |
| Writer losses | 38, все относятся к потере `pic:cNvPr@id` (при записи id становится 0) |
| Максимальный peak RSS / p95 времени | 264.5 MiB / 0.634 s |
| Strict XSD | 100 documents validated, 0 missing/refused/unmatched/ours; 2,133 drivers compiled, 0 failures, 0 uncovered roots |
| OPC | 100 packages, 0 violations |

Corpus runner завершился успешно, но помечен как development measurement (`public_path_development_measurement_not_full_acceptance`). Его параметры: `--wps-text --allow-unsigned-unicode --drop-nul`; manifest SHA-256 `0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`; CLI SHA-256 `688e69f2cf18382ea8013196a12a82374b079dec50ffb9e160c30818bcd4b40a`. Результаты и DOCX находятся в ignored каталоге `target/rtf-import/RC00-main-integration-corpus/`, полный построчный ledger — `results.json`. Git object id бинарного source diff первоначального RC00 среза: `8db8ec57457b003fcee91cb546daf76f9aabe2bf`.

Проверенные команды:

```powershell
cargo +1.92 test -p strict-ooxml-render-pdf -p strict-ooxml-view -p strict-ooxml-rtf -p strict-ooxml-cli -p strict-ooxml --locked
cargo +1.92 test -p strict-ooxml-view rtf_render_keeps_import_stages_and_pages_in_the_corpus_view
cargo +1.92 build -p strict-ooxml-cli --locked
python xtool/xsd-gate/xsd_gate.py --corpus target/rtf-import/RC00-main-integration-corpus --written target/rtf-import/RC00-main-integration-corpus --cli target/debug/strict-ooxml.exe --quiet-messages
python xtool/xsd-gate/opc_gate.py --written target/rtf-import/RC00-main-integration-corpus
```

Изменённые Rust файлы прошли точечный `rustfmt +1.92 --edition 2021 --check`; `git diff --check` чист. Полный `cargo fmt --all -- --check` и широкий Clippy остаются затронуты уже существовавшими несоответствиями main baseline; Clippy для RTF пакета прошёл при подавлении только этих базовых lint-категорий.

## Открытые критерии

Это не полный compatibility acceptance. После интеграции ещё не повторялись независимые WPS region/structure probes, media byte roundtrip, pagination/render reports, pixel/SSIM gate и полная feature matrix. Следовательно, цифры 61/39 pagination и 2/98 pixels из исторического снимка `a7b3c9f` нельзя приписывать HEAD `8e612a8`.

RC01 начат отдельным исправлением PDF диагностики: missing glyph отделён от missing font; регрессионный тест проверяет U+10FFFD. RC01 ledger из 100 отчётов приведён в [rc01-loss-ledger.json](rc01-loss-ledger.json): он фиксирует 1,327,900 import loss events, 154,626 видимых loss details и 443 видимых feature names; 8 report streams усечены. Эти feature names не подтверждают первопричины. Отдельно подтверждены 38 writer losses класса `pic:cNvPr@id`, требующие решения в RC07. Backlog остаётся `reviewed: false`, `root_cause: null` до пофайлового triage.


## RC01: точные per-feature totals и RC02: associated-script properties

После первого import baseline добавлена bounded per-feature агрегация: максимум 4,096 имён до 128 UTF-8 bytes; избыток явно учитывается как unattributed и помечается флагом. CLI JSON schema `strictlib.rtf-import.v1` получила additive поле `feature_counts` и overflow totals. Результат измерения после commit `9a4c457`: CLI SHA-256 `9454a6732755e10a1185ac0c43b9bcfc8c4ace4f8e21ae39403a254c4ebafcff`, тот же corpus manifest `0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`. Все 100 документов прошли runner, text и resource/integrity gates.

Per-feature суммы сходятся с общими totals во всех файлах: 1,327,900 Loss и 95 Info, 459 distinct features, 0 unattributed, 0 feature-map truncations. Retained issue details по-прежнему ограничены: 8 отчётов показывают `details_truncated=true`, но это больше не теряет feature totals. `control:*` — 1,284,441 Loss (337 имён); `field-instruction-control:*` — 33,753; `destination:*` — 6,097. Это учёт неподдержанных токенов parser, а не измеритель визуального ущерба. Самые частые control names и per-document counts лежат в JSON ledger; semantic mapping и влияние проверяются отдельными RC02–RC09 fixtures. На новых DOCX повторно прошли Strict XSD: 100/100 validated, 0 violations, и OPC: 100 packages, 0 violations. Оба corpus gates завершились PASS.

Проверки RC01: полный targeted workspace test command выше прошёл; bounded-cap test создаёт 4,097 разных unhandled control words и доказывает ограничение в 4,096 keys с точной сверкой tracked + unattributed totals. CLI integration test проверяет JSON per-feature values. Clippy для `strict-ooxml-rtf` и `strict-ooxml-cli` прошёл при suppressions для уже существующих lint в общих пакетах main baseline.

В RC02 обработаны `\\ab`, `\\ai` и `\\afs`: они записываются как `bCs`, `iCs`, `szCs`, не меняя обычные латинские `b`, `i`, `sz`. Synthetic regression проходит import → DOCX write → reopen и проверяет значения и отсутствие generic import Loss. `\\af` теперь разрешается отдельно в `eastAsia` face: оно переживает последующий `\\f`, сбрасывается `\\plain`, а RTF-группы сохраняют scoped state. Regression проверяет import → DOCX write → reopen.

Последний срез связал `\\lang`/`\\langnp` с основным language slot, `\\langfe`/`\\langfenp` с `eastAsia`, а `\\alang` — с WML `bidi`. LCID без подтверждённого тега остаются явными Loss; group scoping и import → write → reopen покрываются fixtures. `\\loch`, `\\hich`, `\\dbch`, `\\rtlch`/`\\ltrch` и `\\fcs0/1` выбирают `ascii`, `hAnsi`, `eastAsia` и `cs` font slots, куда последующие `\\f`/`\\af` записывают шрифт. В corpus body text и resource/integrity — 100/100 Pass; 208,166 Loss, 95 Info и 455 feature names. Все известные языковые и script-font controls сняты с generic Loss; неизвестные LCID 0, 255, 1024 и 8192 остаются `language:unknown-lcid:*`. Strict XSD — 100/100, OPC — 100 packages, оба без нарушений. WPS pagination — 62/100 Pass; pixel gate — 3/100 Pass. После `\\alang` ни один page count или PDF hash не изменился. Это development measurements, не полная визуальная приёмка. Corpus manifest SHA-256 `0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`; CLI SHA-256 `d40c14cccb694212dffa9feba8aaa0e5044a40d1a006775e6087584bc287ef56`.

Визуальный прогон по WPS `visual-corpus-v1` дал 62/100 совпадения числа страниц. После `\\alang` все page counts и PDF hashes совпали с предыдущим snapshot. Pixel receipt — 3/100 Pass, 97/100 Fail при неизменённом строгом comparator. Gates остаются development measurement, не acceptance; полная visual compatibility не закрыта.

После commit `176195f` font-table selectors `\\fbimajor/minor`, `\\fdbmajor/minor`, `\\fhimajor/minor` и `\\flomajor/minor` отображаются в соответствующие WML theme slots вместе с fallback face. Новый fixture проверяет все восемь ссылок через import → DOCX write → reopen. На тех же 100 источниках снято 751 Loss; далее commit `4725086` распознал font-table `\\fbidis` и реализовал документальный режим single-font, уменьшив totals до 207,405 Loss и 95 Info, 445 features. Fixtures проверяют font-table hint и запрет переключения на associated font через `\\af`. Text/resource — 100/100; Strict XSD и OPC для предыдущего среза — 100 без нарушений; новый срез сохранил все 100 DOCX hashes. WPS pagination осталась 62/100, pixel receipt — 3/100 Pass (97 Fail; 423 страницы сравнены, 6 Pass и 417 Fail). Это development measurement, а не visual acceptance. CLI SHA-256 `12addb9284fdda6187de138f99a7877d1df3bf72fad0bc7afacc14928e23dc4b`; manifest SHA-256 `0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`.

### RC02: East Asian face selection — 2026-10-07

В `ComputedRun` сохраняется отдельный East Asian font slot: direct `w:eastAsia`
или theme `w:eastAsiaTheme`. CJK-only текст выбирает этот face через общий
`chosen_family`; смешанный Latin/CJK run сохраняет основной face. Math-run
сбрасывает оба script slots. PDF collection, shaping и выбор embedded face теперь
используют тот же resolver, что SVG measurement/paint. Direct/theme SVG regression,
RTF → SVG regression и PDF embedded BaseFont regression проходят. Проверка PDF
доказывает выбор bundled substitute (Arimo/Tinos), а не наличие CJK glyph coverage.
Owned font registry, exact CJK assets, per-script splitting смешанных runs и полный
resolved-face identity contract остаются открытыми; RC02 не закрыт.

Повторно прочитаны receipts `target/rtf-import/RC02-eastasia-face-{corpus,visual,pixels}`:
100/100 Measured, text/resource/integrity Pass; pagination 62/100 Pass;
pixels 3/100 Pass, 97 Fail, 423 страницы (6 Pass, 417 Fail). Comparator и thresholds
не изменялись. Это development measurement, не visual acceptance. CLI SHA-256:
`5665763b1c104346810ee7ed692f96379f1ab954d20f508c13d718d2e1b7d683`.
XSD/OPC 100/100 без нарушений — результат предыдущего запуска, в этой сессии
эти gates не повторялись.

В текущем worktree `C:/Users/gamer/.codex/worktrees/rtf-import-integrated/StrictLib`
(ветка `codex/rtf-import-integrated`) повторены full PDF/public API suites с
`cargo +1.98.0 test --locked -p strict-ooxml-render-pdf -p strict-ooxml --features rtf,pdf`
и SVG `--test themes`: Pass. Логи `RC02-eastasia-face-pdf-rtf-tests.log` и
`RC02-eastasia-face-themes-tests.log` в `target/rtf-import/`.
Full SVG baseline имеет противоречивый `maps_regular_postscript_names_to_the_same_bundled_faces`:
он ожидает сохранения `Arial-BoldMT`, но unchanged resolver возвращает Carlito.
Первый повтор также не имел ignored Clio DOCX fixture; fixture восстановлен из
основной копии. Эти результаты не заменяют безусловный full-suite Pass.

Clippy на Rust 1.92 прошёл для трёх crates, `--all-targets --features rtf,pdf`,
с `-A clippy::manual_let_else -A clippy::items_after_statements
-A clippy::single_char_pattern -A clippy::replace_box -A clippy::while_let_loop
-A clippy::unnecessary_sort_by -A clippy::if_same_then_else`.
Дополнительные suppression относятся к unchanged WML/write/layout baseline;
лог `RC02-eastasia-face-clippy-msrv-final.log`. Rust 1.98 дополнительно сообщает
existing `chunks_exact_to_as_chunks` в public RTF test. Changed-file rustfmt
(`--edition 2021 --config skip_children=true`) и `git diff --check`: Pass.
`cargo fmt --all -- --check` выявляет existing formatting differences вне этого diff;
посторонние файлы не форматировались.

После восстановления fixture полный SVG suite с единственным явным исключением
maps_regular_postscript_names_to_the_same_bundled_faces завершился Pass (exit 0).
Команда: cargo +1.98.0 test --locked --no-fail-fast -p strict-ooxml-render-svg --
--skip maps_regular_postscript_names_to_the_same_bundled_faces.
Лог: target/rtf-import/RC02-eastasia-face-svg-final-tests.log.

### RC02: shared font resource contract — 2026-10-07

Добавлены FontResource/ResolvedFaceHandle: borrowed bundled или explicit shared bytes,
program SHA-256 + collection face index + sorted variation coordinates; validation
font/index/axis/range и mapping provenance. Legacy Copy face/source API сохранены.
Shaping caches используют program/instance identity; PDF collection/build разрешает
bundled face через общий cached resolver. Contract tests, cache collision regression,
полные SVG/PDF/public API suites (existing alias-test skip) и MSRV Clippy с existing
baseline suppressions проходят. RC02 остаётся InProgress: registry admission, owned
consumer pipeline, glyph policy и реальные CJK/Indic assets требуют следующих срезов.
Контракт, проверки и ограничения: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

### RC02: bounded registry и owned consumer API — 2026-10-07

Реализован immutable FontAssetRegistry с typed pinned manifest, explicit shared
bytes и budgets; complete preflight предшествует hashing/parsing. Проверяются
hash/index/axes, metadata/duplicates, permission evidence и OS/2 embedding flags.
Lookup требует exact family/style binding, без system discovery или implicit
fallback. Owned resource metrics/coverage/advances и shape_resource используют
одни program bytes/variation location и существующий harfrust/cluster builder.
Missing CJK glyph возвращает DegradedMissingGlyph. Полные SVG/PDF/RTF suites:
418 Pass (existing alias-test skip); registry regressions 6 Pass; MSRV Clippy с
прежними baseline suppression и formatting проходят. Corpus gates в этом API-срезе
не повторялись. Registry ещё не подключён к RenderOptions/placement/SVG/PDF;
следующий срез — сохранить original request и owned handle до delivery/subset,
затем failure policy и свежий corpus run. RC02 остаётся InProgress.
Подробности: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).


### RC02: explicit assets through placement and SVG/PDF — 2026-10-07

Optional RenderOptions font_assets now preserves the selected original script-font
request and retains its registry in placed runs. SVG embeds exact program bytes with
safe resource-derived CSS names, explicit variations and no automatic optical/synthetic
instance changes. PDF collects and subsets exact program/index/all-coordinate keys;
owned programs survive options drop. TTC face 1 is verified in PDF and rejected
explicitly for current SVG delivery. Empty registries preserve established output.
Full SVG/PDF/RTF suites: 425 Pass with the previously recorded alias-test exclusion;
final SVG tests: 10 Pass. MSRV Clippy retains the existing suppressions. Fresh default
public corpus has 100/100 text/integrity/resource Pass and unchanged DOCX hashes;
Strict XSD and OPC have zero violations across 100 packages.
RC02 remains InProgress: runtime failure policy, positive licensed CJK/Indic geometry
and full visual acceptance remain pending. Registry admission is available in the
library API; the corpus CLI still exercises default bundled resources.
Current contract, limits and witness references: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).


Fresh default pagination: 62/100 Pass, 38 Fail. All 100 actual/reference PDF hashes
and page correspondences match RC02-eastasia-face. The prior pixel receipt is
carried forward by exact PDF identity: 3/100 documents Pass, 97 Fail; 423 pages,
6 Pass and 417 Fail. The existing release comparator SHA-256 matches that receipt.
This is not a new raster run or visual acceptance. Equivalence evidence:
`target/rtf-import/RC02-render-assets-pixel-equivalence.json`; fresh pagination:
`target/rtf-import/RC02-render-assets-visual-verified/results.json`.


## RC02 failure policy and licensed witnesses (2026-10-07)

Implemented bounded original-request reports and ReportAndContinue/Reject across
placement, SVG and PDF. Strict refusal survives zero detail capacity; SVG TTC
continuation omits unsupported text with a diagnostic. Renamed pinned OFL CJK SC
and Devanagari test subsets match independent native HarfBuzz geometry. CJK strict
SVG/PDF/RTF and Unicode extraction pass. Devanagari embeds but continuation CIDs
still produce replacement characters on extraction: it remains degraded and strict
rendering refuses it. Next: fix continuation extraction, review script/font profiles,
wire bounded CLI assets, and pass full visual corpus acceptance. RC02 InProgress.
Contract and limits: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

436 tests/53 groups pass with the existing alias-test exclusion; MSRV Clippy passes
with unchanged baseline suppressions/warnings. Fresh corpus 100/100 text/integrity/
resource; XSD/OPC 100 packages, zero violations. All 100 default DOCX/PDF hashes
match the previous slice. Fresh pagination remains 62 Pass/38 Fail. Historical pixel
measurement remains 3 Pass/97 Fail documents and 6 Pass/417 Fail pages; no reraster.
Receipts: `target/rtf-import/RC02-font-policy-{corpus,visual}/results.json`,
`RC02-font-policy-{full-tests,clippy-final,xsd,opc}.log`.


### RC02: cluster ActualText — 2026-10-07

Implemented logical UTF-16 ActualText around multi-glyph clusters. Independent
MuPDF extraction now preserves the pinned Devanagari text; CJK also passes.
lopdf ignores ActualText and retains its raw continuation-CID extraction limitation.
436 tests/53 groups and MSRV Clippy pass with existing exclusions/suppressions.
Geometry and degraded complex-script status are retained. Corpus gates were not
rerun in this focused slice. Next: broader script/font profiles and complex-script
visual/placement acceptance, bounded CLI configuration, full visual corpus.
RC02 InProgress. Details: [FONT_RESOURCE_CONTRACT.md](FONT_RESOURCE_CONTRACT.md).

## RC00: повторная интеграция master — 2026-10-08

`master` `956948f` влит в `codex/rtf-import-integrated` merge-коммитом `5cccef4`
(родители `bf676d1` и `956948f`), база по-прежнему `ac51b687`. Перечень восьми
конфликтов и решения по ним — в
[REMAINING_COMPATIBILITY_PLAN.md](REMAINING_COMPATIBILITY_PLAN.md); здесь только
результат сборки. RTF `parser.rs` и `picture.rs` адаптированы к полям WML,
добавленным master (`Style` row/cell/conditions/custom_style/auto_redefine,
`Table.grid_change`, `Option<bool>` autospacing, `Picture.nv_id/bw_mode`,
`BlipRef.cstate`); выбранные значения сохраняют прежний вывод.

Проверки на Rust 1.98: `cargo +1.98.0 check --locked --workspace --all-targets` и
то же с `--all-features` — Pass; `cargo fmt --all -- --check` — Pass. Тесты:
`strict-ooxml-rtf` (30 + 1 doc), `strict-ooxml-render-pdf` и
`strict-ooxml --features rtf,pdf`, полный `strict-ooxml-render-svg` и
`strict-ooxml-cli` с единственным документированным `--skip
maps_regular_postscript_names_to_the_same_bundled_faces`, `strict-ooxml-write` —
Pass. Corpus/visual/pixel/XSD/OPC gates в этом срезе не перезапускались.
