# R01: исходный корпус RTF и независимые эталоны

Дата: 2026-10-04. Worktree: `codex/rtf-import`, база `7a8e6d37d4d96543db00a9c4f2be51f879397c1f`.

Статус: integrity/census harness реализован и проверен. После уточнения пользователя WPS Writer успешно экспортировал все 100 файлов; независимые DOCX facts извлечены для 100/100. Индекс references зафиксирован. Reviewed import outcomes, детальные grid/merge comparators и визуальные эталоны пока не заморожены. R01 не закрыта в части полной матрицы ожиданий; следующий API scaffold имеет независимые эталоны для разработки.

## Что измерено

Корпус `testdata/CC0_RTF` содержит ровно 100 RTF и 100 уникальных записей манифеста. Размеры и SHA-256 всех файлов совпали с манифестом. Суммарный размер 52 185 834 байта; крупнейший вход 8 352 331 байт. Полный census: `target/rtf-import/R01/corpus/census.json`; устойчивый снимок с именами, хешами и признаками: [census.summary.json](census.summary.json).

Ограниченный лексический scanner прошёл все 100 входов; это не семантическая валидация. Все корневые заголовки содержат `rtf1`. Максимальная глубина групп 10, максимальное число лексических токенов 1 444 382. Найдено 708 различных control words. Значения и счётчики включают header, styles, ignored destinations; они не равны числу видимых объектов.

| Лексический признак | Файлов |
|---|---:|
| fonttbl | 100 |
| Unicode u/uc/upr/ud | 75 |
| colortbl | 70 |
| stylesheet | 31 |
| section controls | 30 |
| RTL controls | 28 |
| fields | 24 |
| list controls | 23 |
| revision controls / metadata | 19 |
| headers/footers | 10 |
| pict | 8 |
| table controls | 6 |
| metafile controls | 5 |
| shapes | 5 |
| notes | 5 |
| JPEG | 3 |
| columns | 2 |
| PNG | 2 |
| merge controls | 1 |
| OLE/object controls | 1 |

Настоящие revision content, ненулевые columns/sections и использование charset подтверждаются после semantic/oracle проверки. Например, metadata `revauth` не доказывает наличие видимых tracked changes. FontTable может объявлять шрифт для письменности, которая не встречается в тексте.

Nested-table controls и `itap > 1` не обнаружены в полном лексическом census. `itap1` присутствует в 58 токенах, `itap0` в 8341; считать `itap1` доказательством вложенности было бы ошибкой. Это ограниченная лексическая отрицательная находка, не обещание отсутствия всех нестандартных вложенных объектов. Binary `bin` не найден, его корректность проверяется синтетическими fixtures.

## Кодировки и особенности экспортёров

В ansicpg встречаются 1252 (76 объявлений), 1251 (6), 936 (3), 0, 10000, 1250, 1254, 1255, 1256 (по одному); в остальных файлах такого control word нет. Ноль требует решения по default charset, не запуска платформенной ACP. `uc` встречается со значениями 0, 1, 2. Font charset содержит как западные/Cyrillic/Hebrew/Arabic, так и CJK, Thai и Symbol declarations. Декодирование только UTF-8 или только CP1252 не соответствует корпусу.

`cocoartf` встречается в 33 файлах: Cocoa control words и расширенные color tables должны либо преобразовываться по проверенным правилам, либо иметь диагностику, а не трактоваться как обычные RGB без доказательств.

У 33 файлов после корневой `}` стоят `CR LF NUL`; последняя позиция — один NUL. Их полный документ лексически разобран, trailer отмечен `nul_trailer_candidate`. Не изменять исходники и не замораживать expected Failed по этой находке. Предлагаемый импортный контракт: разрешить ровно один завершающий NUL после root/whitespace с Info diagnostic; дополнительный произвольный хвост отвергать. Это явная совместимость с контейнерным padding, не восстановление повреждённых групп. Oracle должен подтвердить поведение по этим входам до reviewed expectations.

## Harness и воспроизведение

Все команды выполняются из worktree. Python использует только стандартную библиотеку; текущая проверенная версия 3.13.5.

```powershell
python -m unittest discover -s xtool/rtf -p 'test_*.py' -v
python xtool/rtf/census.py
python xtool/rtf/snapshot.py --refresh-pending
python xtool/rtf/validate.py --output target/rtf-import/R01/validation-new-run
python xtool/rtf/oracle.py --output target/rtf-import/R01/oracle-new-run
# При установленном LibreOffice:
python xtool/rtf/oracle.py --soffice 'C:\path\to\soffice.exe' --output target/rtf-import/R01/oracle-with-engine
# Независимый WPS oracle (Windows COM):
python -X utf8 xtool/rtf/wps_oracle.py --output target/rtf-import/R01/wps-new-run
python -X utf8 xtool/rtf/docx_facts.py --oracle target/rtf-import/R01/wps-new-run
python -X utf8 xtool/rtf/font_inventory.py --output target/rtf-import/R01/wps-new-run/available-fonts.json
python -X utf8 xtool/rtf/summarize_wps.py --oracle target/rtf-import/R01/wps-new-run --output docs/rtf/wps.reference-index-new.json
```

`census.py`: exit 0 — integrity и bounded lexical scan всех 100 входов прошли; 1 — corpus/scan findings; 2 — ошибка подготовки. Каждый manifest input сохраняет отдельную строку, missing/extra/hash mismatch не уменьшают знаменатель. Presence census включает ignored contents; это не извлечение видимого текста.

`snapshot.py` публикует summary и [expectations.pending.json](expectations.pending.json). Значения `expected_import_outcome` остаются null, `reviewed=false`. `--refresh-pending` не позволяет заменить рассмотренные ожидания или named unsupported decisions. Скрипт не превращает собственные результаты scanner в эталон импортёра.

`validate.py` запускает fixtures, mutation binary skip и два одинаковых corpus scan. Изолированная mutation-копия не меняет исходники worktree. Требуются assertion failure конкретного теста и byte-identical census JSON. Receipt включает SHA базы, tracked dirty diff и хеши всех harness Python-файлов (включая untracked), manifest SHA, команды/exit codes и ограничения. Это provenance R01 harness, не hash всей производственной реализации будущего importer.

Проверка выполнена: fixtures GREEN; binary-skip mutation упала на assertion; два census JSON совпали побайтно. Receipt/logs: `target/rtf-import/R01/validation/`. Initial strict trailer scan (67/100) не принимается за baseline дефектов входа: проверка raw tail объяснила все 33 случая, scanner получил явную диагностику и отдельные tests.

## Контракт независимого oracle

Проверены PATH, стандартные каталоги Program Files, App Paths registry и названия установленных Office/LibreOffice каталогов. Word/LibreOffice не найден в проверенных местах. Это не исчерпывающий поиск всех дисков; нестандартный путь может быть указан отдельно. Harness автоматизации Word пока не реализован.

`oracle.py` умеет создавать DOCX через отдельный headless LibreOffice process/profile для каждого input и фиксирует executable SHA/version/filter, exit, время, stdout/stderr и output hash. Выходной каталог каждого run должен быть новым; старые эталоны не перезаписываются. Все 100 входов проходят integrity до запуска приложения. Отсутствующий engine/version failure означает 100 Blocked и exit 2, отсутствие результата/timeout — Fail, а не успешный skip.

Captured DOCX — сырой независимый reference, не GREEN текстового/XSD/visual gate. Последующий extraction adapter должен отдельно извлечь body/headers/footers/notes и paragraph/tab/break boundaries, table grids/merges, styles, section properties, resource hashes. Перед заморозкой требуется версия приложения, options и реестр используемых fonts; приложение может восстанавливать RTF, поэтому спорные решения проверяются минимальной fixture/спецификацией. Потери oracle не объявлять корректным импортом автоматически.

Reviewed expectations публиковать новым `docs/rtf/expectations.json`: input sha256, expected Clean/Degraded/Failed, named unsupported features и их spans, independent expected text/structure facts, references и provenance. Не переименовывать pending в reviewed одним флагом. Истинные invalid inputs принимаются как expected Failed только с независимой причиной. Полные text, structure, Strict XSD, media, visual и resource гейты остаются раздельными.

## Следующие шаги

1. Завершить полный WPS run, извлечь/review references; oracle expectations часть R01 остаётся открытой.
2. R02: компилируемый API/errors/limits/report scaffold и RED поведенческих проверок, используя небольшие известные fixtures. Декодирование, scopes, `bin` и trailer — первые cases.
3. R03: базовый reader и WML mapping с явным учётом неподдержанных из 708 слов. Даже text-only профиль обязан учитывать Cocoa fonts/colors, Cyrillic/RTL и Unicode fallback; неизвестные controls нельзя массово объявить нейтральными.
4. R04–R06: приоритет corpus классов — lists/styles, поля/regions, таблицы, media/WMF/shapes; core profile не обещает Clean на этих файлах до реализации.

Corpus pass 100/100 в R01 не закрывает ни одну задачу совместимости R03–R07 и не уточняет срок полноценного импорта без независимых references.

## WPS Writer: подтверждённый независимый путь

После сообщения пользователя проверен `KWPS.Application`. Зарегистрированный COM server запускает WPS Writer `12.1.0.28485`, executable SHA-256 `3064943ba736eedcefa4077f6221e1b283396c82814adffc467c218077e2dd93`. Уже работающие UI/addon процессы используют также старую версию `12.1.0.28032`; версия oracle определяется по фактически созданному процессу, а не по старому process list. Generic COM `Application.Version` возвращает `12.0`, поэтому одного этого поля недостаточно.

Официальные источники API: [Application automation](https://open.wps.cn/documents/app-integration-dev/wps365/client/wpsoffice/jsapi/wps/Application/obj), [SaveAs2](https://open.wps.cn/documents/app-integration-dev/wps365/client/wpsoffice/jsapi/wps/Document/member/SaveAs2), [WdSaveFormat](https://open.wps.cn/documents/app-integration-dev/wps365/client/wpsoffice/jsapi/wps/enum/WdSaveFormat). Используется явный format=12 (DOCX), не пользовательский default format.

`probe_wps.ps1` подтвердил `Hello bold` и `Unicode Ж` в COM text и независимом XML чтении DOCX, а также один bold run. Проверка Unicode выполнена по code point U+0416, без зависимости от кодировки вывода консоли. Smoke-export самого короткого NUL-padded файла и большого документа с изображениями также Captured.

`wps_oracle.py` запускает STA PowerShell worker на каждый input. `wps_export.ps1` сначала открывает собственную скрытую bootstrap fixture, через её Window.Hwnd определяет process id и проверяет, что его не было до COM creation и что в экземпляре только один документ. Лишь после этой проверки применяет локальные settings и открывает корпус readonly/hidden, без Recent Files. Чужие приложения не закрываются. Завершение вызывает Quit только у подтверждённого собственного экземпляра. App.Hwnd до открытия документа может быть 0 — ранний неуспешный probe записан отдельно и не считается экспортом.

Каждая запись сохраняет input hash, executable/version/hash, DOCX и UTF-8 raw Content.Text hashes, COM counts, процесс, options, exit и время. Выходы хешируются после закрытия документа: до этого WPS блокирует файл. Таймаут допускает остановку только PID из ownership marker с совпадением start ticks; без marker чужие WPS процессы не завершаются. Smoke-run оставляет остальные 98 записей NotRun и имеет ненулевой итоговый exit; успешная выборка не выдаётся за весь корпус.

`docx_facts.py` независимо читает ZIP/XML стандартной библиотекой Python: CRC/part hashes, Final-view paragraph text и paths с границами table cells, paragraph/run properties, media hashes, WML construct counts и объявленные font names. Textboxes отделены от enclosing paragraph; instrText/deleted/moveFrom не добавляются в Final текст. Полные raw DOCX остаются доказательством для иных views. Детальный grid/merge adapter, фактическое разрешение шрифтов, XSD и visual references этим инструментом не доказаны.

Raw references и `facts-index.json` хранятся в `target/rtf-import/R01/wps-corpus/`. Pending expectations не заменяются автоматически: независимые факты могут стать text/structure expectations после проверки профиля, но будущие Clean/Degraded/Failed не выбираются по совпадению собственного импортёра.

## Итог полного WPS прогона

Все 100 inputs имеют Captured и Extracted, с одной фактической версией/executable hash. Ошибок и таймаутов 0. Извлечены 82 003 явных абзаца по всем регионам и 69 media parts; это XML-счётчики references, не число видимых объектов исходного RTF. Индекс с hashes, COM counts, региональными text digests и provenance: [wps.reference-index.json](wps.reference-index.json). Полные исходные/выходные тексты остаются в игнорируемых target/testdata, в документацию не копируются.

Время одного worker, включая startup/export/cleanup: p50 3,406 с; p95 5,179 с; max 21,049 с; сумма 405,073 с. Эти измерения относятся к WPS oracle, не к будущему RTF reader. Хеши 338 доступных font files сохранены в `available-fonts.json`; actual rendered faces этим не доказаны.

Финальная проверка fixtures/обратного binary-контроля/детерминизма corpus: `target/rtf-import/R01/validation-wps/`. 20 tests GREEN. После прогона все штатные worker экземпляры завершены; оставшаяся пара процессов первого пустого probe завершена отдельно по exact PID/start-time и broker-child token, без остановки исходных WPS процессов.

Визуальные PDF/PNG эталоны ещё не сняты. Текущий importer и его измеренные gates описаны в [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md); наличие WPS reference не означает полной приёмки.

2026-10-05: facts extractor исправлен — `w:tab` внутри paragraph properties не должен попадать в visible text. Добавлен regression test, facts v2 пересчитаны из прежних raw DOCX с SHA-256 check. Актуальный индекс [wps.reference-index-v2.json](wps.reference-index-v2.json), facts root `target/rtf-import/R01/wps-facts-v2/`. Старый v1 индекс и facts сохранены как история и не используются для text acceptance. Числа raw capture/paragraph/media counts остаются прежними; региональные text digests изменились.
