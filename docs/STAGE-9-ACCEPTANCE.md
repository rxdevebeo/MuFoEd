# Приёмка этапа 9 — импорт DjVu → Strict

**Заказ:** `STAGE-9-TASK.md` (ZAKAZ-STAGE-9)
**Основание:** отчёт `docs/stage-9-report.md`; запись решений ADR-0012/0013
**Дата приёмки:** 2026-10-10
**Окружение:** Windows x86_64 (MSVC), Rust 1.92; DjVuLibre `3.5.28-2ubuntu0.24.04.2`
(внешний оракул, WSL); WPS Office `12.1.0.28485`; Ollama `0.40.2`,
модель `qwen3-vl:8b-instruct-ctx16k`; корпус `testdata-lock/djvu.toml` (100 файлов)
**Вердикт:** **ПРИНЯТО С ЗАМЕЧАНИЯМИ** — функционально 9A–9E закрыто; `SC-D8`
и реальный OCR измерены 2026-10-10; полный корпусный прогон `SC-D3…D14`
остаётся за `schedule`-CI, покрытие после изменений нужно перемерить (§4).

---

## 1. Итог в одной таблице

| Критерий | Ключевое свидетельство | Статус |
|---|---|---|
| **SC-D1** детерминизм | `tests/djvu.rs::conversion_is_deterministic_with_no_recognition` | выполнено |
| **SC-D2** нет паник | `fuzz_djvu` компилируется; локальный fuzz-smoke не запустился (linker MSVC), запуск — на Linux-CI | частично |
| **SC-D3** индекс = `djvudump` | `dump-oracle.json`: **100/100** | выполнено |
| **SC-D4** текст = `djvutxt` | `text-oracle.json`: 99 равных + 1 объяснённый (`retained-page`) | выполнено |
| **SC-D5** зоны = `djvutxt --detail=word` | `zone-oracle.json`: 3 файла, **63 682 слова**, 0 px | выполнено |
| **SC-D6** текст без потерь | `docx-corpus.tsv`: **100/100** | выполнено |
| **SC-D7** картинка = `ddjvu` | `render-bitonal.json` точное (`max=0`); `render-iw44.json` — floor (mean ≤ 2.0, PSNR ≥ 30 dB) | частично |
| **SC-D8** открывается в WPS | WPS `12.1.0.28485` открыл native и OCR пакеты без repair (§2) | **выполнено (2026-10-10)** |
| **SC-D9** ссылки/оглавление | `ant-oracle.json` 1→1; `outline-oracle.json` 3/3 (DjVuLibre `print-outline`) | выполнено |
| **SC-D10** ничего не теряется молча | отчёт несёт `djvu.*` для каждого исхода; `verify_no_silent_loss` в CLI | частично |
| **SC-D11** OCR не в ядре | `tests/djvu.rs::recognition_is_opt_in` | выполнено |
| **SC-D12** лицензии/лимиты | `cargo +1.92.0 deny check` — advisories/bans/licenses/sources ok | выполнено |
| **SC-D13** indirect | `dump-oracle.json`: 3/3 (72/121/2 страницы из `INCL`) | выполнено |
| **SC-D14** поворот | `zone-rotated.json`/`render-rotated.json`: 90/180/270 точно | выполнено (синтетика) |

---

## 2. SC-D8 — открытие внешним редактором (WPS 12.1.0.28485)

Метод: `KWPS.Application` (COM), `Documents.Open(file, false, true)`;
повреждённый пакет бросил бы исключение/вызвал repair.

| Источник | Пакет | Страниц | Символов | Слов | Repair |
|---|---|---|---|---|---|
| `AdrianMoraru-Marfa.djvu` стр. 1 (native-текст) | `native-p1.docx` | 1 | 1395 | 316 | нет |
| `keraleeya-kalakal.djvu` стр. 10 (OCR) | `keraleeya-p10.docx` | 2 | 15\* | 4\* | нет |

\* малая статистика WPS по малаямскому тексту; сам текст восстановлен (§3).

Версия подтверждена дважды: `wps.exe` FileVersion `12,1,0,28485` и COM
`Build=12.1.0.28485`.

## 3. Реальный OCR (свидетельство `djvu-validation/ocr-real.json`)

Ранее OCR проверялся только мок-сервером (`ocr-cli-smoke.json`). 2026-10-10
прогон через живой Ollama с моделью `qwen3-vl:8b-instruct-ctx16k`:

- `keraleeya-kalakal.djvu` стр. 1 — классифицирована как `illustration`,
  **описана, а не транскрибирована** (`djvu.figure`) — O7 работает на живой модели;
- `keraleeya-kalakal.djvu` стр. 10 — классифицирована как `scan`, OCR вернул
  2 абзаца малаямского текста (U+0D07…) за ~10 с (`djvu.ocr.recovered`);
- `OtnoshenieNoveysheyFiziki.djvu` стр. 5 — отказ `max_ink_pixels`
  (поднять `--max-image-pixels`).

## 4. Замечания и что остаётся

1. **Полный корпусный прогон `SC-D3…D14`** зафиксирован не окончательно:
   в этом worktree не запускался `schedule`-job `djvu-oracle`; часть измерена
   локально, часть — на синтетике.
2. **Покрытие** (`coverage-djvu-nightly`: core ≥ 80 %, all-features ≥ 70 %,
   `djvu.rs` ≥ 80 %) перемерено 2026-10-10 после добавления `xmp.rs`,
   `ink_boxes`/`region_png` и фигур: **84.32 / 84.20 / 87.60 %** — выше порогов.
3. **SC-D2** — локальный fuzz-smoke не шёл (linker MSVC); проверяется Linux-CI.
4. **SC-D7 IW44** — измеренный regression floor, не полный SSIM.
5. **SC-D10** — инвариант «потеряно ⇒ в отчёте» не прогнан как корпусный тест.
6. **XMP-оракул** отсутствует (реализация `docProps/core.xml` есть, внешней
   сверки нет).
7. Качество OCR/фигур на других моделях не измерялось (одна модель, 2 страницы).

## 5. Воспроизведение

```powershell
cargo +1.92.0 test -p strict-ooxml-djvu -p strict-ooxml-convert -p strict-ooxml-cli --all-features --lib --tests
cargo +1.92.0 check --workspace --all-targets
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 deny check
# реальный OCR
cargo +1.92.0 build --release -p strict-ooxml-cli --features ocr-ollama
target/release/strict-ooxml from-djvu testdata/CC0_DJVU/keraleeya-kalakal.djvu --pages 10-10 --ocr --out out.docx
```

Детали: [stage-9-report.md](stage-9-report.md), [DJVU_IMPORT.md](DJVU_IMPORT.md),
[djvu-validation/README.md](djvu-validation/README.md), [adr/0013](adr/0013-djvu-conversion.md).
