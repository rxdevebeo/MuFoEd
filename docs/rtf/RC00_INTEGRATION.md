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

После commit `176195f` font-table selectors `\\fbimajor/minor`, `\\fdbmajor/minor`, `\\fhimajor/minor` и `\\flomajor/minor` отображаются в соответствующие WML theme slots вместе с fallback face. Новый fixture проверяет все восемь ссылок через import → DOCX write → reopen. На тех же 100 источниках осталось 207,415 Loss и 95 Info: уменьшение на 751 selector Loss; text/resource — 100/100, Strict XSD — 100/100 без нарушений, OPC — 100 пакетов без нарушений. WPS pagination осталась 62/100; число страниц не изменилось, один PDF hash отличается. Строгий pixel receipt остаётся 3/100 Pass (97 Fail; 423 страницы сравнены, 6 Pass и 417 Fail). Это development measurement, а не visual acceptance. CLI SHA-256 `1f0e094a83f21dfa035fab27a5efc4d09f52350c0f223c2da76fbd628186a4b0`; manifest SHA-256 `0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`.
