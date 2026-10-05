# Проверка импорта DjVu и OCR — 2026-10-05

Корпус владельца: `D:/projects/StrictLib/testdata/CC0_DJVU/`.
Данные остаются внешними; тестовые отчёты содержат имена и SHA256, без копий книг.
Оракул — только бинарные `djvutxt` / `ddjvu` из DjVuLibre
3.5.28-2ubuntu0.24.04.2 в изолированном каталоге под WSL Ubuntu.
Он не связан с production-библиотекой; исходники кодеков DjVuLibre не читались.

| Проверка | Результат | Свидетельство |
| --- | --- | --- |
| Ридер на всём корпусе | 100/100 файлов; 8 050 страниц, 7 758 Readable, 290 Empty, 2 Unusable | [native-corpus.tsv](native-corpus.tsv) |
| Native текст → Strict DOCX → независимый XML extraction | 100/100, символы совпали после удаления whitespace, structural separators и недопустимых XML controls; images отключены | [docx-corpus.tsv](docx-corpus.tsv) |
| Независимый текстовый оракул | 99 файлов совпали; отличие одного файла отдельно проверено и объяснено ниже | [text-oracle.json](text-oracle.json) |
| Native JB2 против ddjvu | 4/4 реальных страниц: точное RGB pixel equality при native resolution | [jb2-pixel-oracle.json](jb2-pixel-oracle.json) |
| Реальный скан → CLI → HTTP OCR → DOCX | PNG передан mock Ollama, текст Recovered и исходный скан записаны | [ocr-cli-smoke.json](ocr-cli-smoke.json) |
| Reader / image / converter / CLI / OCR tests | all-features lib/tests прошли; отдельная offline CLI сборка — 3/3 | команды ниже |
| Workspace default all-targets check / all-features Clippy / fmt | прошли; Clippy с `-D warnings` | команды ниже |
| cargo-deny advisories / bans / licenses / sources | прошли | `cargo +1.92.0 deny check` |
| Fuzz target Rust compilation | прошла; target подключён к Linux CI smoke и nightly | `cargo check --manifest-path fuzz/Cargo.toml --bin fuzz_djvu` |

## Единственное отличие text oracle

В `IslamFelsefesineIsikVerenSeyyidler.djvu`, страница 51, дерево содержит
пустой text-zone rectangle. Наш ридер возвращает Unusable и сохраняет UTF-8
буфер; CLI сообщает `djvu.text.unusable` и пишет доступный текст без геометрии.
`djvutxt --page=51` возвращает пустой stdout с exit 0. Если исключить только
эту явно указанную страницу из native текста, остальные символы файла
полностью совпадают с djvutxt. Поэтому JSON отдельно различает `equal=99`
и `accounted=100`; это не объявлено 100 точными совпадениями.
Вторая Unusable-страница находится в `HaiggeretElRomim.djvu`.

NUL — native terminator, который djvutxt преобразует в newline. Только
whitespace, NUL и structural separators 0x1D/0x1F исключены из text oracle;
диакритика, пунктуация и прочие символы сравниваются без замены.

## Воспроизведение

```powershell
cargo +1.92.0 test -p strict-ooxml-convert -p strict-ooxml-djvu -p strict-ooxml-cli -p strict-ooxml-ocr --all-features --lib --tests
cargo +1.92.0 test -p strict-ooxml-cli --test djvu
cargo +1.92.0 check --workspace --all-targets
cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.92.0 fmt --all -- --check
cargo +1.92.0 run -p strict-ooxml-djvu --example corpus -- D:/projects/StrictLib/testdata/CC0_DJVU .scratch/djvu-corpus-text
cargo +1.92.0 run -p strict-ooxml-convert --features djvu --example djvu_corpus -- D:/projects/StrictLib/testdata/CC0_DJVU
cargo +1.92.0 build -p strict-ooxml-cli --features ocr-ollama
python xtool/djvu-ocr-smoke.py --cli target/debug/strict-ooxml.exe --input D:/projects/StrictLib/testdata/CC0_DJVU/keraleeya-kalakal.djvu --out .scratch/djvu-ocr-smoke.docx --report .scratch/ocr-cli-smoke.json
```

Для native-text oracle Python и бинарный djvutxt должны работать в одном
окружении; под WSL пути корпуса и worktree нужно передать как `/mnt/d/...`
и `/mnt/c/...`, при необходимости задать `LD_LIBRARY_PATH` каталога оракула:

```sh
python3 xtool/djvu-oracle.py --corpus /path/to/CC0_DJVU --text /path/to/native-text --oracle /path/to/djvutxt --report /path/to/text-oracle.json --retained-page IslamFelsefesineIsikVerenSeyyidler.djvu:51
```

`--resume` разрешает повторно использовать успешные строки только при
совпадении SHA256 входа и извлечённого native текста. Необъяснённые отличия
остаются ошибкой. Known retention проверяется отдельно, а не игнорируется.

## Пределы доказательства

OCR smoke проверяет integration contract с mock-сервером, а не качество
реальной vision-модели. Ни один установленный Ollama model не запускался.
Изображения в corpus DOCX gate отключены: он проверяет сохранность текста,
а не полное визуальное сходство всех книг. Битональный pixel oracle покрывает
четыре страницы: `keraleeya-kalakal` 5/7/10 и `PlaysHistoriesAndNovelsOfTheIngeniousMrs.AphraBehnV.1` 1.
IW44 проверен синтетическими progressive fixtures; gamma/interpolation,
полный SSIM corpus, native text rotation и rectangle oracle ещё не закрыты.
Реальный scan inventory выявил отказы при превышении default 16 Mi-pixel
бюджета; увеличить его можно явно через `--max-image-pixels`.

Локальный `cargo +nightly fuzz run ... --sanitizer none` на Windows MSVC не
дошёл до исполнения: linker не разрешил libFuzzer `__start___sancov_pcs` /
`__stop___sancov_pcs`. Это не успешный fuzz smoke. Rust target check прошёл,
CI использует Linux target с sanitizer; внешняя CI этого worktree не запускалась.
Аннотации, hyperlinks, NAVM, metadata, mixed-region OCR и неподдерживаемые
image codecs перечислены в [руководстве](../DJVU_IMPORT.md); полный Stage 9
не объявлен завершённым.

## Decoder extension after baseline commit 93cc8ff

[decoder-oracle.json](decoder-oracle.json) records exact RGB pixel equality for
synthetic JPEG, ordinary/inverted MMR, external legacy BM44 and PM44, and
FGbz page 1. FGbz page 4 foreground-only also matches exactly. The full page 4
composite differs in the existing IW44 background (mean channel error 0.00328,
maximum 93); no full-IW44 accuracy claim is made.

External fixtures: MIT `matyushkin/djvu-rs/tests/fixtures/` on GitHub, filenames
and input hashes recorded in the JSON. To reproduce any single page:

```powershell
cargo +1.92.0 run -p strict-ooxml-djvu --features djvu-image --example codec_probe -- input.djvu 1 native.png
# Optional --foreground removes only background chunks; compare with ddjvu -mode=foreground.
```

Striped Smmr remains unsupported. This extension does not close annotations,
bookmarks, region OCR, native-zone rotation or the other open Stage 9 criteria.
