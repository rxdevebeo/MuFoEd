# mufoed-model-runtime

Общий host-owned менеджер текста, vision/OCR и изображений MuFoEd. Независим от
UI, StrictLib и tensor backend. [План](PLAN.md), [приёмка R03–R05](R345_VALIDATION.md).

```rust
use mufoed_model_runtime::{Memory, Runtime, Task};
use std::{sync::atomic::AtomicBool, time::Duration};
let runtime = Runtime::global(); // managed, OS RAM + NVIDIA VRAM
let mut task = Task::new("example", "backend:model");
task.device = "cpu".into();
task.memory = Memory { ram: Some(128 << 20), vram: Some(0) };
let lease = runtime.enqueue(task)?.wait(&AtomicBool::new(false), Duration::from_secs(60))?;
// На worker: inference; lease удерживается до завершения вычислений/cleanup.
drop(lease);
# Ok::<(), mufoed_model_runtime::Error>(())
```

GUI использует неблокирующий `Ticket::try_start`. Один clone runtime передаётся
в text/OCR adapters и `ImageEditor::with_runtime`. Повторный вход с удерживаемым
lease недопустим. `Runtime::new(Policy)` — только резервирование без телеметрии;
`Runtime::managed(Policy, MemoryPolicy, Arc<dyn Telemetry>)` — политика host.

Допуск проверяет полный peak и фактическую свободную память с резервом. Совместимая
собственная residency вычитается только под lease; после eviction пересчитывается.
`Lease::cached` возвращает pin, связанный временем жизни с lease. Контроллер
`Resource` подтверждает offload/unload; неудачная выгрузка блокирует новые allocation.
Адаптеры вызываются вне mutex менеджера. `Lease::retry` допускает один повтор
только OOM после подтверждённого cleanup, сохраняя seed/параметры.

`residents`, `admission`, `events`, `snapshot`, `blocked_reason` доступны UI.
`release_idle_async` освобождает собственный idle-кэш, не прерывая активные задачи.
`shutdown_async` запрещает новый inference и ждёт собственные local/remote domains;
host должен дождаться receiver, прежде чем завершать процесс.

Feature `ollama`: loopback использует host runtime, remote endpoint имеет отдельный
resource domain, связанный с lifecycle host. Ключ учитывает model digest/context;
`/api/ps` проверяет владение, unload требует acknowledgement и отсутствие residency.
Неопределённый исход удерживает lease в recovery. RAM Ollama неизвестна, VRAM —
серверное `size_vram`, а допуск использует отдельную оценку weights/KV/workspace.

SHA256 fingerprint имеет ограниченный stat-кэш. Модели должны быть неизменяемыми
во время inference. При намеренной перезаписи с сохранением размера/времён вызовите
`fingerprint::invalidate()`; замену файлов host сериализует с inference.
Отдельные OS-процессы не разделяют singleton; broker пока отсутствует.
