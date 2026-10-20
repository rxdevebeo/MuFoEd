# TODO — Image Studio

- [x] Выделить общий для MuFoEd прикладной крейт `mufoed-model-runtime`: FIFO, leases,
  отмена ожидания, ограничения оценок RAM/VRAM и host injection в графические backend.
- [x] Подключить текст/чат/OCR MuFoEd и управление Ollama с подтверждением выгрузки (R02).
- [x] R03–R05: физические RAM/VRAM, admission с резервом, LRU offload/unload,
  resident-кэш, один подтверждённый OOM-retry, shutdown и окно диагностики.
- [x] **Доработать кэш моделей встроенного Plakat.** CPU-веса сохраняются по
  fingerprint/revision/dtype/device/LoRA+scale, device-модели CLIP/UNet/VAE создаются
  и освобождаются по стадиям. GPU pipeline целиком не сохраняется.
- [x] Persistent BiRefNet ONNX/SAM/апскейлеры; SAM image embeddings переиспользуются
  при изменении points. Смесь UltraSharp/AuraSR выполняет GPU-компоненты последовательно.
- [x] Проверки совместимости, cancellation, failed load/unload, OOM и shutdown;
  реальные cold/warm, смена LoRA, inpaint и освобождение на RTX 5070 Ti.

Контракты, ограничения и свидетельства: [R03–R05](../model-runtime/R345_VALIDATION.md).
Дальнейшее развитие: межпроцессный broker; аппаратная проверка других GPU/семейств
checkpoint; возможное удержание device-моделей при большем бюджете.

Остальные этапы и критерии: [README.md](README.md#следующие-этапы-и-критерии-готовности).

- [x] UI01 после R02: полноценная палитра ручных инструментов и менеджер слоёв в стандартной компоновке; общий стиль MuFoEd, группы инструментов, параметры активного инструмента, видимость/блокировка/порядок/opacity слоёв и понятные действия добавления, удаления и дублирования.

Проверки UI01 и ограничения: [UI01-VALIDATION.md](UI01-VALIDATION.md).

- [x] PSD RGB8 как формат проекта: растровые слои, свойства, merged preview, private mask/recipe metadata; legacy JSON import и независимые fixtures. [Приёмка](PSD-VALIDATION.md).
- [ ] Расширение PSD: ICC/цветоуправление, layer masks/groups и off-canvas layers; каждый профиль требует отдельной приёмки.

- [x] После генерации: low-denoise Img2Img и UltraSharp 1×, отмена/ошибки без частичного применения, сохранение recipe в PSD.
- [x] Центрирование всех egui-диалогов; parent window для системных файловых диалогов demo.
