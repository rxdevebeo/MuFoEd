# Свидетельства текущей приёмки

Главный receipt — `STATUS.json`; исполнимый остаток — `../AUDIT_REMAINING_WORK_2026-10-06.md`. Baseline HEAD и новый отпечаток содержимого различаются: локальные изменения пока не опубликованы. `evidence-sha256.json` проверяет сохранённые файлы; STATUS содержит итоговую интерпретацию. Исторические receipts не заменены.

`source-manifest.json` фиксирует байты кода и новых тестов. `corpus-manifest.json` содержит 248 входов: 27 Strict +121 локальный +100 CC0, с SHA-256 исходных и записанных пакетов. Единственный отсутствующий Strict output — ожидаемый refusal G-10; он учитывается отдельно, не как исчезнувшее измерение. Большие выходные DOCX находятся в `target/remediation-2026-10-06`; manifest позволяет проверить или заново создать их.

Логи сохранены как `*.log.gz`, exits — отдельными текстовыми файлами. Ожидаемый `census-qname.exit=1` означает незакрытый inventory, а `leak-repro.exit=77` — RED старой утечки. Они не объявлены зелёными проверками. `census-inventory.json.gz` содержит весь остаток без blanket dispositions. `opc-comparison.json` отделяет исходные нарушения от добавленных.

В `fuzz/<target>/receipt.json` находятся команда, SHA-256 выполненного ASan-бинарника, начальный корпус, exit и длительность. `run.log.gz` содержит libFuzzer footer. Засчитываются только восемь завершённых сессий по ≥3600 с. Прежний docx_full был остановлен после изменения writer; его exit 72 (`run interrupted`) и прочие старые прогоны исключены. Он заменён отдельной полной сессией.

Для повторения основных локальных проверок из корня проекта:

```powershell
cargo +1.92.0 test --workspace --all-features --locked
cargo +1.92.0 fmt --all --check
cargo +1.92.0 clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo +1.92.0 check --workspace --locked
python xtool/audit-fixes/run.py --task F21 --phase green --receipt-dir target/audit-fixes/F21/green
python xtool/xsd-gate/census_gate_selftest.py
python xtool/xsd-gate/census_gate.py --cli target/release/strict-ooxml.exe --no-build --quiet-messages --keep-written target/census-written --inventory-out target/census-inventory.json
```

Перед `--no-build` CLI должен быть собран из проверяемого кода. Полные feature-matrix команды сохранены в `feature-matrix-accepted.commands.json`; coverage инструментальная версия и toolchain указаны в STATUS. Скрипты `reproduce/` показывают дополнительную диагностику; их пути `target/remediation-2026-10-06` соответствуют этой приёмке, а Linux coverage использует `/mnt/d/projects/StrictLib`. Повторное измерение создаёт новый receipt, а не меняет смысл старого.

`wps-absolute-baselines.json` — диагностика по настоящим PDF glyph origins, не полный WPS ledger. `word-export-request.json` фиксирует DOCX/части/страницы и необходимые поля будущего Word-экспорта. Он не является Word PASS; версия, параметры и шрифты эталона должны быть получены из реального экспорта.
