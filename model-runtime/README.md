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
