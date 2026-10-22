# R03–R05: admission, resident cache, recovery

Дата: 2026-10-20. Worktree `codex/image-studio`, база `d9a67d7d` (R02),
изменения R03–R05 и ранее начатый UI01 остаются незакоммиченными.
Библиотечные крейты StrictLib не изменены. Основной граф индексирует D:/ checkout,
не этот worktree; coverage для новых файлов отсутствует, поэтому проверены
непосредственные исходники, diff и исполняемые проверки.

## Контракт и реализация

- `telemetry.rs`: RAM ОС через sysinfo, NVIDIA total/free/UUID через ограниченный
  по времени nvidia-smi. Проверка вне UI. Strict: неизвестные/старше 5 с данные
  не превращаются в ноль и не дают allocation. При неоднозначном auto GPU — отказ.
- `residency.rs`: полный холодный peak, вычет только точного собственного reusable
  ресурса под exclusive lease, пересчёт после eviction, offload GPU→CPU при запасе
  RAM либо unload LRU, затем новый физический замер. Фиксированные host caps
  проверяются также для отложенного плана native adapter. Резерв RAM max(2 GiB,5%),
  VRAM max(1 GiB,10%); idle-cache target известных CPU bytes 12 GiB.
- Pin связан с lifetime lease. Active ресурсы не вытесняются, callbacks не исполняются
  под mutex очереди/реестра. Неуспешный/panicked unload сохраняет запись и quarantine,
  новые allocation запрещены до подтверждённой очистки. Maintenance-release доступен
  даже после закрытия normal admission.
- Plakat Git SHA `ce0d67a45bc9b5f742c3772aef85afe0d15890f6`, без fork:
  публичные CLIP/UNet/VAE компоненты. Веса читаются в CPU-cache по content identity,
  device-модели строятся/уничтожаются по стадиям. XL fp16, CPU f32; CLIP-L, CLIP-G,
  img2img VAE encode, UNet, VAE decode не держатся одновременно на GPU.
  Device-модели и native conditioning не сохраняются между запросами.
- Native key: SHA256 весов, pinned backend revision, family, dtype, device UUID,
  упорядоченные LoRA content hashes/scales; отдельный VAE key. Токенизаторы читаются
  заново, native conditioning cache отсутствует. Fingerprint имеет stat-cache до
  512 записей. Файлы неизменяемы во время inference; publisher сериализует замену.
  При намеренной перезаписи с сохранением size/mtime/ctime нужен `fingerprint::invalidate`.
- Python: принадлежащие manager persistent workers; BiRefNet ONNX session, SAM model
  и CPU image embeddings, UltraSharp/AuraSR сохраняются по model/config/script/package
  fingerprint и UUID. Points не входят в ключ SAM image embedding; смена RGB/размера
  его инвалидирует. Смесь апскейлеров offload-ит первый GPU-компонент перед вторым.
  Ошибка/timeout/cancel завершают worker с kill/reap; temporary directory живёт до cleanup.
- Ollama: реальные context tokens передаются в адаптер; metadata weights + F16 KV
  cache + workspace образуют preflight estimate. Warm ownership повторно проверяется
  под lease через digest/ps/expiry; сторонняя residency не принимается во владение.
  Удерживаются только собственные совместимые модели. Unload подтверждается ACK и ps.
  RAM серверного процесса неизвестна, ps size_vram — backend reported, не driver free.
  Remote endpoint — отдельный domain, связанный с host shutdown, без смешения памяти.
- R05: максимум 2 попытки (один OOM-retry) с теми же seed/размером/dtype/tile/моделью,
  только после подтверждённого cleanup. Другие ошибки не повторяются. Native cancellation
  между компонентами/denoising steps; Python kill/reap; неопределённый HTTP держит lease
  до recovery. Host shutdown прекращает admission и ждёт активные local/remote jobs.
- UI «Память моделей»: измерения с возрастом, оценки дополнительного пика, unknown,
  cache footprint/pins/hits, подтверждённые backend stages, refresh и idle-release вне UI.
  Preview не меняет документ; ошибочный/отменённый результат не принимается.

## Автоматические проверки

- `r345-clippy-final.log`: строгий Clippy, все targets runtime/image-editor,
  features demo,plakat-cuda,ollama, `-D warnings` — PASS.
- `r345-tests-final.log`: runtime **32 PASS**, image-editor **46 PASS**
  (25 unit + 11 editing + 7 manual + 3 runtime integration), 0 failures.
- `r345-mufoed-tests-final.log`: **143 PASS, 6 ignored**, 0 failures.
- Python AST: все 5 model scripts PASS. Persistent argparse проверяет tile/device;
  Rust fake-child проверки success residency, cancel/timeout/error и confirmed reap.
- Runtime tests: unknown/stale/insufficient memory, deferred caps, warm reuse/pins,
  LRU offload + remeasure, cache evicted while queued, failed load/unload quarantine,
  OOM retry с неизменным seed, foreign Ollama/expiry, recovery и remote shutdown.
- L8 inpaint-mask regression: PNG хранит реальные grayscale значения; непрозрачный
  RGBA-mask раньше интерпретировался Plakat как полностью белый.

Команды (PowerShell, из корня worktree):

```powershell
. ./image-editor/scripts/enter-cuda.ps1
cargo +1.92.0 clippy -p mufoed-model-runtime -p image-editor --all-targets `
  --features image-editor/demo,image-editor/plakat-cuda,image-editor/ollama `
  --config 'profile.dev.package.plakat.opt-level=2' `
  --config 'profile.dev.package.sha2.opt-level=3' -- -D warnings
cargo +1.92.0 test -p mufoed-model-runtime -p image-editor `
  --features image-editor/demo,image-editor/plakat-cuda,image-editor/ollama `
  --config 'profile.dev.package.plakat.opt-level=2' `
  --config 'profile.dev.package.sha2.opt-level=3'
cargo +1.92.0 test -p mufoed --config 'profile.dev.package.sha2.opt-level=3'
cargo +1.92.0 run -p image-editor --example residency_smoke `
  --features demo,plakat-cuda,ollama `
  --config 'profile.dev.package.plakat.opt-level=2' `
  --config 'profile.dev.package.sha2.opt-level=3'
```

## Аппаратная проверка

Windows, RTX 5070 Ti: driver total 17,094,934,528 bytes (16303 MiB),
RAM 102,830,108,672 bytes. CUDA 12.9/SM120, Rust 1.92.0.
SDXL local/image-studio-sdxl fp16, LoRA add-detail-xl 0.2→0.4,
Ollama llama3:latest num_ctx=8192; локальные SAM 2.1 Hiera Large,
BiRefNet tiny.onnx CPU, UltraSharp + AuraSR v2 65/35 CUDA.

Полный сценарий и метрики: `image-editor/artifacts/r345-acceptance.log`.
Изображения `resident-native-0/1.png`, `resident-1024.png`,
`resident-img2img.png`, `resident-inpaint.png`, `resident-lora-02/04.png`
сохраняются рядом. Веса/логи/изображения находятся в игнорируемых runtime/artifacts.

Смешанный прогон завершён с exit 0:

| Операция | Cold | Warm | Подтверждение |
|---|---:|---:|---|
| SDXL 512×512, 4 steps | 12.132 с | 4.187 с | Результаты с одним seed побитово равны, четыре CPU-weight записи reused |
| SDXL 1024×1024, 2 steps | 6.227 с | — | Успешный inference |
| SAM 2.1 | 10.107 с | 0.454 с | Один PID 74504, loads=1, hits=1, embedding_hits=1 |
| BiRefNet tiny CPU | 7.381 с | — | ONNX session resident, process working set 6.65 GB |
| UltraSharp/AuraSR, 32→128 RGBA | 12.064 с | 3.124 с | Один PID 66640, loads=2, hits=2; вход RGBA |
| Ollama load_duration, ctx8192 | 3.371 с | 0.004 с | Собственная совместимая residency |
| Возврат к Ollama load_duration | 2.949 с | 0.007 с | Перезагрузка после image-переходов, затем reuse |

Это end-to-end времена примера, кроме явно обозначенного server load_duration;
результаты апскейла относятся к маленькому тестовому RGBA, не к фото полного размера.

- Реальные img2img, inpaint с L8 mask и feather, strength=0, сохранность каждого
  защищённого пикселя и исходного `Document` — PASS.
- Смена LoRA scale 0.2→0.4 — PASS; активная отмена после UNet stage и свободная
  очередь после cleanup — PASS. Cold LoRA merge на CPU занимает минуты, warm cache
  не повторяет merge при совместимом ключе; это не выдаётся за мгновенную смену LoRA.
- SAM points менялись без пересчёта image embedding; Rust получил стадии Python
  из phase RPC (в том числе BiRefNet и отдельные UltraSharp/AuraSR стадии).
- Явный release: residents=[], active=None. Driver free до сценария
  15,017,705,472 bytes, после release 14,729,347,072 bytes. Разница около 275 MiB
  не объявляется освобождённой: контекст CUDA/драйвер и внешние allocations входят
  в общий замер. Реестр подтверждённых модельных ресурсов пуст.
- Запрос с оценкой 64 GiB VRAM получил OverBudget до backend allocation — PASS.
- Дополнительный `r345-vae-switch.log`, exit 0: default → подготовленный
  `D:/Models/sdxl_vae.safetensors` → default, успешный alternate inference,
  побитовое восстановление baseline и пустой registry после release — PASS.

Повтор VAE-проверки после сборки примера:

```powershell
$env:IMAGE_STUDIO_RESIDENCY_VAE = (Get-Content image-editor/.runtime/vae-result.json | ConvertFrom-Json).result
& ./target/debug/examples/residency_smoke.exe
Remove-Item Env:IMAGE_STUDIO_RESIDENCY_VAE
```

Настоящее окно `image-editor-demo` собрано с demo,plakat-cuda,ollama.
Автоснимки `r345-memory-1440.png` и `r345-memory-1100.png` проверены визуально:
реальные показания RAM/VRAM, читаемые контролы, список/стадии в прокручиваемом окне.
Снимок ждёт worker замера; после сохранения host дожидается shutdown.
`r345-receipt.json` содержит baseline SHA, SHA256 изменённых файлов и логов,
привязывая локальные свидетельства к незакоммиченному дереву.

## Практические границы

Физический OOM намеренно не провоцировался: policy retry проверен injected OOM,
а аппаратный отказ — оценкой 64 GiB VRAM на реальной 16 GiB GPU до allocation.
Непредсказуемые чужие allocations/драйвер и allocator abort остаются вне гарантии.
Оценки пика консервативны, это не allocator measurements. PyTorch reserved bytes,
CPU tensor bytes, process working set и driver free имеют разные значения;
неизвестные bytes не показываются как точный ноль.

Полный GPU pipeline не удерживается: warm native повторно загружает CPU tensors на
устройство, но не читает веса с диска и не повторяет совместимый LoRA merge.
Несовместимый ресурс удаляется по давлению/LRU, а не обязательно сразу при выборе
другой модели. CLI диагностический одноразовый, без resident reuse/auto retry.
Другие checkpoint/семейства, CPU inference и многокарточные конфигурации требуют
отдельной аппаратной проверки; не заявляется универсальная совместимость.
Межпроцессный broker не реализован. Ollama API не даёт exclusive server lease;
одновременный чужой клиент требует выделенного endpoint/broker для строгого владения.
