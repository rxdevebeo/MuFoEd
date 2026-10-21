//! The search window (Alt+F7).

use std::path::PathBuf;

use super::{FileManager, Location};
use crate::search::{Query, Search};
use crate::Locale;

pub(crate) struct SearchState {
    query: Query,
    min_kb: String,
    max_kb: String,
    days: String,
    depth: String,
    running: Option<Search>,
    results: Vec<crate::entry::Entry>,
    chosen: Option<usize>,
}

fn number<T: std::str::FromStr>(text: &str) -> Option<T> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        text.parse().ok()
    }
}

impl FileManager {
    pub(super) fn open_search(&mut self) {
        let mut query = self.settings.search.clone();
        if let Some(folder) = self.current_folder() {
            query.root = folder.to_path_buf();
        }
        let text = |value: Option<u64>| {
            value
                .map(|bytes| (bytes / 1024).to_string())
                .unwrap_or_default()
        };
        self.search = Some(SearchState {
            min_kb: text(query.min_size),
            max_kb: text(query.max_size),
            days: query
                .within_days
                .map(|days| days.to_string())
                .unwrap_or_default(),
            depth: query
                .depth
                .map(|depth| depth.to_string())
                .unwrap_or_default(),
            query,
            running: None,
            results: Vec::new(),
            chosen: None,
        });
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn search_window(&mut self, ctx: &egui::Context, locale: &Locale) {
        let Some(state) = self.search.as_mut() else {
            return;
        };
        let mut open = true;
        let mut start = false;
        let mut go_to: Option<PathBuf> = None;
        let mut feed = false;
        egui::Window::new(locale.text("Find files"))
            .id(egui::Id::new("file-manager-search"))
            .open(&mut open)
            .default_size([560.0, 480.0])
            .resizable(true)
            .show(ctx, |ui| {
                egui::Grid::new("file-manager-search-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(locale.text("Search in"));
                        let mut root = state.query.root.display().to_string();
                        if ui
                            .add(egui::TextEdit::singleline(&mut root).desired_width(f32::INFINITY))
                            .changed()
                        {
                            state.query.root = PathBuf::from(root);
                        }
                        ui.end_row();
                        ui.label(locale.text("Name mask"));
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut state.query.mask)
                                .hint_text("*.txt *.md | ~*")
                                .desired_width(f32::INFINITY),
                        );
                        if response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            start = true;
                        }
                        ui.end_row();
                        ui.label(locale.text("Containing text"));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut state.query.text)
                                    .desired_width(220.0),
                            );
                            ui.checkbox(&mut state.query.case_sensitive, locale.text("Match case"));
                        });
                        ui.end_row();
                        ui.label(locale.text("Size, KB"));
                        ui.horizontal(|ui| {
                            ui.label(locale.text("from"));
                            ui.add(
                                egui::TextEdit::singleline(&mut state.min_kb).desired_width(70.0),
                            );
                            ui.label(locale.text("to"));
                            ui.add(
                                egui::TextEdit::singleline(&mut state.max_kb).desired_width(70.0),
                            );
                        });
                        ui.end_row();
                        ui.label(locale.text("Changed within, days"));
                        ui.add(egui::TextEdit::singleline(&mut state.days).desired_width(70.0));
                        ui.end_row();
                        ui.label(locale.text("Depth (empty = all)"));
                        ui.add(egui::TextEdit::singleline(&mut state.depth).desired_width(70.0));
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.query.folders, locale.text("Find folders too"));
                    ui.checkbox(&mut state.query.hidden, locale.text("Hidden files"));
                });
                ui.horizontal(|ui| {
                    let running = state
                        .running
                        .as_ref()
                        .is_some_and(|search| !search.is_finished());
                    if running {
                        if ui.button(locale.text("Stop")).clicked() {
                            if let Some(search) = &state.running {
                                search.cancel();
                            }
                        }
                        ui.spinner();
                    } else if ui.button(locale.text("Start search")).clicked() {
                        start = true;
                    }
                    let enabled = !state.results.is_empty();
                    if ui
                        .add_enabled(enabled, egui::Button::new(locale.text("Feed to panel")))
                        .clicked()
                    {
                        feed = true;
                    }
                    if ui
                        .add_enabled(
                            state.chosen.is_some(),
                            egui::Button::new(locale.text("Go to file")),
                        )
                        .clicked()
                    {
                        go_to = state
                            .chosen
                            .and_then(|index| state.results.get(index))
                            .map(|entry| entry.path.clone());
                    }
                });
                if let Some(search) = &state.running {
                    state.results = search.results();
                    ui.small(locale.format(
                        "{} found, {} looked at · {}",
                        &[
                            state.results.len().to_string(),
                            search.scanned().to_string(),
                            search.current().display().to_string(),
                        ],
                    ));
                    if !search.is_finished() {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(200));
                    }
                }
                ui.separator();
                let height = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
                egui::ScrollArea::vertical()
                    .id_salt("file-manager-search-results")
                    .auto_shrink([false, false])
                    .show_rows(ui, height, state.results.len(), |ui, range| {
                        for index in range {
                            let entry = &state.results[index];
                            let label = if entry.is_dir {
                                format!("[{}]", entry.path.display())
                            } else {
                                entry.path.display().to_string()
                            };
                            let response = ui.selectable_label(state.chosen == Some(index), label);
                            if response.clicked() {
                                state.chosen = Some(index);
                            }
                            if response.double_clicked() {
                                go_to = Some(entry.path.clone());
                            }
                        }
                    });
            });
        if start {
            state.query.min_size = number::<u64>(&state.min_kb).map(|kb| kb * 1024);
            state.query.max_size = number::<u64>(&state.max_kb).map(|kb| kb * 1024);
            state.query.within_days = number(&state.days);
            state.query.depth = number(&state.depth);
            state.results.clear();
            state.chosen = None;
            state.running = Some(Search::start(state.query.clone(), self.wake.function()));
            self.settings.search = state.query.clone();
        }
        let function = self.wake.function();
        if feed {
            let results = std::mem::take(&mut state.results);
            let title = format!("{}: {}", locale.text("Found"), state.query.mask);
            self.panels[self.active].go(
                Location::Results {
                    title,
                    entries: results,
                },
                None,
                &function,
            );
            self.search = None;
            return;
        }
        if let Some(path) = go_to {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            if let Some(parent) = path.parent() {
                self.panels[self.active].go(
                    Location::Folder(parent.to_path_buf()),
                    name,
                    &function,
                );
            }
            self.search = None;
            return;
        }
        if !open {
            self.search = None;
        }
    }
}
