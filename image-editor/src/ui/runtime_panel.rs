//! Measured shared-model status. All telemetry/cleanup runs on scheduler workers.
use super::ImageEditor;
use mufoed_model_runtime::{Memory, Phase, Task};
use std::{
    sync::{atomic::AtomicBool, mpsc},
    time::Duration,
};
#[derive(Default)]
pub(super) struct RuntimeUi {
    pub open: bool,
    pub(super) pending: Option<mpsc::Receiver<Result<(), String>>>,
    message: String,
}
#[allow(clippy::cast_precision_loss)] // Display GiB, not allocator accounting.
fn bytes(value: Option<u64>) -> String {
    value.map_or_else(
        || "неизвестно".into(),
        |v| format!("{:.2} GiB", v as f64 / 1_073_741_824.0),
    )
}
fn phase(phase: Phase) -> &'static str {
    match phase {
        Phase::Checking => "проверка памяти",
        Phase::Loading => "загрузка",
        Phase::Running => "вычисление",
        Phase::Offloading => "GPU → CPU",
        Phase::Releasing => "выгрузка",
        Phase::Unknown => "выгрузка не подтверждена",
        Phase::Retrying => "повтор после OOM",
        Phase::Ready => "готово",
        Phase::Failed => "ошибка",
    }
}
impl ImageEditor {
    /// Open measured memory/residency diagnostics shared with the embedding host.
    pub fn open_memory_panel(&mut self) {
        self.runtime_ui.open = true;
        self.refresh_model_memory();
    }
    /// Cancel owned image jobs and queue idle resource release. The host must keep
    /// polling the receiver before exiting; active compute retains its lease meanwhile.
    pub fn shutdown_models(&mut self) -> mpsc::Receiver<Result<(), String>> {
        self.job = None;
        self.model_ui.reset_project();
        self.segmentation_ui.reset_project();
        self.upscale_ui.reset_project();
        self.runtime.release_idle_async()
    }
    fn refresh_model_memory(&mut self) {
        if self.runtime_ui.pending.is_some() {
            return;
        }
        let runtime = self.runtime.clone();
        let (sender, receiver) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("model-memory-refresh".into())
            .spawn(move || {
                let result = (|| {
                    let mut task = Task::new("host", "memory observation");
                    task.device = "cpu".into();
                    task.memory = Memory {
                        ram: Some(0),
                        vram: Some(0),
                    };
                    let _lease = runtime
                        .enqueue(task)
                        .map_err(|e| e.to_string())?
                        .wait(&AtomicBool::new(false), Duration::from_secs(900))
                        .map_err(|e| e.to_string())?;
                    Ok(())
                })();
                let _ = sender.send(result);
            });
        self.runtime_ui.pending = Some(receiver);
    }
    pub(super) fn runtime_panel(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.runtime_ui.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.runtime_ui.message =
                        result.map_or_else(|e| e, |()| "Операция менеджера завершена".into());
                    self.runtime_ui.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.runtime_ui.message = "Worker менеджера остановился".into();
                    self.runtime_ui.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let mut open = self.runtime_ui.open;
        egui::Window::new("Модели и память").open(&mut open).default_width(610.0).default_height(610.0).max_height(650.0).vscroll(true).show(ctx,|ui|{
            ui.label("Общий менеджер MuFoEd: текст · OCR · изображения");
            if let Some(error)=self.runtime.blocked_reason(){ui.colored_label(egui::Color32::from_rgb(235,145,145),format!("Новые загрузки заблокированы: {error}"));}
            let state=self.runtime.snapshot();
            if let Some(task)=state.active {ui.strong(format!("{} · {} · {}",task.owner,task.model,state.phase.map_or("ожидание",phase)));}else{ui.strong(if self.runtime.is_shutting_down(){"Менеджер закрывается"}else{"Локальный менеджер свободен"});}
            ui.weak(format!("В очереди: {}",state.queued.len()));
            if let Some(admission)=self.runtime.admission() {
                ui.separator();ui.strong("Измерено ОС / драйвером");
                ui.label(format!("RAM: доступно {} / всего {}",bytes(admission.observation.ram_available),bytes(admission.observation.ram_total)));
                for gpu in &admission.observation.gpus {ui.label(format!("GPU {}: свободно {} / всего {}",gpu.uuid,bytes(Some(gpu.free)),bytes(Some(gpu.total))));}
                if admission.observation.gpus.is_empty(){ui.weak("VRAM: неизвестно");}
                ui.weak(format!("Возраст замера: {:.1} с · выбран {}",admission.observation.at.elapsed().as_secs_f32(),admission.device));
                ui.label(format!("Оценка дополнительного пика: RAM {} · VRAM {}",bytes(admission.estimate.ram),bytes(admission.estimate.vram)));
                if let Some(error)=admission.error {ui.colored_label(egui::Color32::from_rgb(235,145,145),error);}
                if let Some(diagnostic)=admission.observation.diagnostic {ui.weak(diagnostic);}
            }else{ui.weak("Измерения ещё нет. Обновите показания или запустите задачу.");}
            ui.weak("Резерв по умолчанию: VRAM max(1 GiB, 10%), RAM max(2 GiB, 5%).");
            ui.horizontal(|ui|{
                if ui.add_enabled(self.runtime_ui.pending.is_none(),egui::Button::new("Обновить замер")).clicked(){
                    self.refresh_model_memory();
                }
                if ui.add_enabled(self.runtime_ui.pending.is_none(),egui::Button::new("Освободить idle-модели")).clicked(){self.runtime_ui.pending=Some(self.runtime.release_idle_async());}
            });
            if self.runtime_ui.pending.is_some(){ui.label("Ожидание свободного менеджера / подтверждения…");}
            ui.label(&self.runtime_ui.message);
            ui.separator();ui.strong("Резидентные ресурсы");
            let residents=self.runtime.residents();
            if residents.is_empty(){ui.weak("Кэш пуст");}
            for resource in residents {let name=resource.key.split(':').take(if resource.key.starts_with("plakat:"){3}else{2}).collect::<Vec<_>>().join(" · ");egui::CollapsingHeader::new(format!("{name} · RAM {} · VRAM {} · active {} · hits {}",bytes(resource.memory.ram),bytes(resource.memory.vram),resource.pins,resource.hits)).id_salt(&resource.key).show(ui,|ui|{ui.add(egui::Label::new(&resource.key).wrap());});}
            ui.separator();ui.strong("Последние стадии");
            for event in self.runtime.events().iter().rev().take(12){ui.small(format!("{:.1} с назад · {} · {}",event.at.elapsed().as_secs_f32(),phase(event.phase),event.detail));}
            ui.weak("Недостаточно памяти: освободите idle-кэш, завершите чужую GPU-задачу либо явно выберите меньший размер/контекст или CPU. Параметры задачи автоматически не меняются.");
            ctx.request_repaint_after(Duration::from_millis(500));
        });
        self.runtime_ui.open = open;
    }
}
