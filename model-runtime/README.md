# mufoed-model-runtime

Общий прикладной координатор модельных backend для MuFoEd. Независим от UI и
inference-библиотек. [План, состояния этапов и критерии](PLAN.md).

```rust
use mufoed_model_runtime::{Policy, Runtime, Task};
use std::{sync::atomic::AtomicBool, time::Duration};

let runtime = Runtime::new(Policy::default()); // один на host, передавать clone()
let task = runtime.enqueue(Task::new("text", "ollama:model"))?;
let lease = task.wait(&AtomicBool::new(false), Duration::from_secs(60))?;
// На worker: inference и подтверждённая выгрузка/освобождение ресурсов backend.
drop(lease);
# Ok::<(), mufoed_model_runtime::Error>(())
```

Для GUI использовать `Ticket::try_start`, возвращающий `None` при ожидании.
Передать тот же runtime в `ImageEditor::with_runtime(document, runtime.clone())`.
Не удерживать общий lease снаружи адаптера, который сам запрашивает такой же lease:
повторный вход приведёт к ожиданию самого себя.

Без настроенных лимитов менеджер сериализует задачи, но не проверяет фактическую
свободную память. Настроенный лимит требует явной оценки пика; неизвестное значение
не заменяется нулём. Бюджеты относятся к одному локальному resource domain.

Процессный singleton — удобный default для отдельных виджетов одного приложения.
Для взаимодействия нескольких OS-процессов нужен отдельный broker (ещё не реализован).

С feature `ollama` доступен `ollama::Client`: loopback использует переданный host
runtime, удалённые endpoint — отдельные очереди. `run` оборачивает синхронный вызов
существующего HTTP-клиента или OCR-адаптера. Выгрузка подтверждается сервером до
освобождения lease; при обрыве соединения cleanup остаётся у recovery worker.
`ollama::owned_models()` и `Runtime::snapshot().phase` позволяют показать это в UI.
Ранее загруженная сторонним клиентом модель не принимается во владение автоматически.
Кэш между запросами пока не сохраняется. Ограничения и протокол описаны в [R02](PLAN.md#r02-реальные-точки-интеграции).
