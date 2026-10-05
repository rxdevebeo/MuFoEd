# NUL и девять расхождений: результат

2026-10-05. Изолированный worktree, ветка `codex/rtf-import`, baseline commit `6b6d6d2`. Исправления включены в коммит завершения данного этапа.

Все десять исходных проблем разобраны. В явном WPS text profile весь корпус даёт **100 Imported, 0 ImportError, 100/100 exact body text matches**. WPS facts v2 не менялись, исключений по именам файлов и нормализации сравниваемого текста нет.

| Файлы | Подтверждённая причина | Решение |
| --- | --- | --- |
| 006 | 1171 явный NUL внутри body text, несовместимый с XML 1.0; WPS удаляет их | `NulPolicy::DropAndReport`, Loss и точный счётчик `discarded_nuls`; сохраняется последний пустой абзац. Default остаётся Reject |
| 002, 017 | WPS преобразует discretionary `\-` в обычный дефис, но Unicode `u173` сохраняет как U+00AD | Преобразование только в `WpsText`, с Loss; обычный режим сохраняет soft hyphen |
| 005, 059 | WPS сохраняет начальные LF в literal group; header whitespace перед control word отбрасывает, включая tab | Проверенное правило для группы в `WpsText`, с Loss для изменения body characters |
| 058 | Leading LF в группе с обычными пробелами | Literal group prefix в `WpsText`; raw LF после обычного текста по-прежнему игнорируется |
| 075 | Группы, начинающиеся с raw CRLF, дают два LF в WPS | `WpsText` сохраняет оба символа, включая пустые paragraphs |
| 061, 100 | WPS выводит inline `AppleTypeServices` / `CocoaLigature1` в видимый текст; в начале группы их пропускает | Только проверенные inline spellings в `WpsText`, с Loss; обычный режим продолжает сообщать unsupported control |
| 073 | Отдельная пустая merged continuation cell и пропуск последнего empty paragraph перед `cell` | Исправлено в обычном parser: horizontal merges → gridSpan/общая ширина/содержимое; `cell` завершает final paragraph даже после `par` |

Минимальные probes выявили и устранили новую утечку CRLF из ignorable metadata groups. Заголовок `\*` распознаётся до вывода leading whitespace, поэтому skip не оставляет мусор в body.

## Независимые доказательства

51 минимальный вход независимо captured в WPS: [compatibility.wps-probes.json](compatibility.wps-probes.json). Индекс содержит исходные bytes в `input_hex`, input/reference hashes, результаты и путь к raw artifacts. Его можно повторить:

```powershell
python xtool/rtf/priority_probes.py --fixtures docs/rtf/compatibility.wps-probes.json --output target/rtf-import/NEW-INDEPENDENT-PROBES
```

Регрессионные Rust tests проверяют отдельно сохранение последнего пустого cell paragraph, merged width/span/text и writer roundtrip, malformed merge continuation и overflow, default rejection NUL, удаление только NUL, отказ остальных invalid encodings, issue cap, unsupported rejection, leading group whitespace, отсутствие утечки ignorable groups и различие WPS hyphen от Unicode soft hyphen.

## Итоговые режимы

| Режим | Imported | Exact WPS body text | Text/media roundtrip |
| --- | --- | --- | --- |
| Standard default | 97/100 | 89/100 | 97/97 |
| Standard + unsigned UTF-16 + DropAndReport NUL | 100/100 | 92/100 | 100/100 |
| WpsText + unsigned UTF-16 + DropAndReport NUL | 100/100 | 100/100 | 100/100 |

Финальные папки: `target/rtf-import/priority/final-{default,repaired,wps}/`. Каждый `results.json` сохраняет actual options. Все режимы имеют 22 media parts, 0 writer errors и 0 writer report records. Pinned summaries: [default](import.resolved-default-summary.json), [repaired](import.resolved-repaired-summary.json), [WPS](import.resolved-wps-summary.json).

35 behavioral tests + 1 doctest, clippy all targets `-D warnings`, 21 Python harness tests проходят. Все 100 WPS-profile DOCX проходят Strict XSD и OPC gates без violations; receipt: [validation.resolved-receipt.json](validation.resolved-receipt.json).

Это закрытие конкретных corpus text failures. Оно не доказывает visual/layout/font resolution fidelity, полную поддержку regions/lists/styles/fields/revisions или complete WPS emulation. Профиль не пытается выводить все unsupported commands буквально: непроверенные конструкции сохраняют named Loss. Review support-profile expectations и release acceptance остаются отдельными задачами.
