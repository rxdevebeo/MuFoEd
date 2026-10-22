//! The command line under the panels and its expandable log.

use super::{FileManager, Location, Settings};
use crate::terminal::{self, History, Line, Request, Running, Shell};
use crate::Locale;

/// Lines kept in the log.
const LOG_LIMIT: usize = 20_000;

pub(crate) struct TerminalState {
    pub(crate) input: String,
    pub(crate) open: bool,
    pub(crate) height: f32,
    pub(crate) focus: bool,
    log: Vec<Line>,
    running: Option<Running>,
    history: History,
    shell: Shell,
    scroll_to_end: bool,
}

impl TerminalState {
    pub(crate) fn new(settings: &Settings) -> Self {
        Self {
            input: String::new(),
            open: settings.terminal_open,
            height: settings.terminal_height.clamp(80.0, 900.0),
            focus: false,
            log: Vec::new(),
            running: None,
            history: History::default(),
            shell: settings.shell,
            scroll_to_end: false,
        }
    }

    fn push(&mut self, line: Line) {
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            let excess = self.log.len() - LOG_LIMIT;
            self.log = self.log.split_off(excess);
        }
        self.scroll_to_end = true;
    }
}

impl FileManager {
    pub(super) fn terminal_poll(&mut self) {
        let Some(running) = self.terminal.running.as_mut() else {
            return;
        };
        let lines = running.take_lines();
        let finished = running.finished();
        for line in lines {
            self.terminal.push(line);
        }
        if let Some(code) = finished {
            // Output may still be in the pipes: take the rest.
            if let Some(running) = self.terminal.running.take() {
                for line in running.take_lines() {
                    self.terminal.push(line);
                }
            }
            let note = match code {
                Some(0) => None,
                Some(code) => Some(format!("{} {code}", self.locale.text("exit code"))),
                None => Some(self.locale.text("stopped").to_owned()),
            };
            if let Some(note) = note {
                self.terminal.push(Line::Note(note));
            }
            // A command may have changed the folder's files.
            let function = self.wake.function();
            self.panels[self.active].refresh(&function);
        }
    }

    /// Runs the typed command line.
    fn execute_line(&mut self) {
        let text = std::mem::take(&mut self.terminal.input);
        let Some(folder) = self.current_folder().map(std::path::Path::to_path_buf) else {
            self.terminal.push(Line::Note(
                self.locale
                    .text("Commands run in a folder, not inside an archive")
                    .to_owned(),
            ));
            return;
        };
        self.terminal.history.push(&text);
        match terminal::interpret(&text, &folder) {
            Request::Nothing => {}
            Request::Clear => self.terminal.log.clear(),
            Request::ChangeFolder(path) => {
                if path.is_dir() {
                    let function = self.wake.function();
                    self.panels[self.active].go(Location::Folder(path), None, &function);
                } else {
                    self.terminal.push(Line::Note(format!(
                        "{}: {}",
                        self.locale.text("No such folder"),
                        path.display()
                    )));
                }
            }
            Request::Run(command) => {
                if self.terminal.running.is_some() {
                    self.terminal.push(Line::Note(
                        self.locale.text("A command is still running").to_owned(),
                    ));
                    return;
                }
                // Several lines run one after another.
                let joined = match (self.terminal.shell, cfg!(windows)) {
                    (Shell::PowerShell, _) => command.lines().collect::<Vec<_>>().join("; "),
                    (Shell::System, true) => command.lines().collect::<Vec<_>>().join(" & "),
                    (Shell::System, false) => command,
                };
                self.terminal
                    .push(Line::Command(format!("{}> {joined}", folder.display())));
                match Running::start(&joined, &folder, self.terminal.shell, self.wake.function()) {
                    Ok(running) => {
                        self.terminal.running = Some(running);
                        self.terminal.open = true;
                    }
                    Err(error) => self.terminal.push(Line::Note(error.to_string())),
                }
            }
        }
    }

    pub(super) fn terminal_panel(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        if self.terminal.open {
            let response = egui::Panel::bottom(egui::Id::new("file-manager-terminal-log"))
                .resizable(true)
                .default_size(self.terminal.height)
                .size_range(80.0..=900.0)
                .show(ui, |ui| self.log_ui(ui, locale));
            self.terminal.height = response.response.rect.height();
        }
        egui::Panel::bottom(egui::Id::new("file-manager-command-line"))
            .show(ui, |ui| self.command_line_ui(ui, locale));
    }

    fn log_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        let mut stop = false;
        ui.horizontal(|ui| {
            ui.strong(locale.text("Terminal"));
            egui::ComboBox::from_id_salt("file-manager-shell")
                .selected_text(shell_name(self.terminal.shell))
                .show_ui(ui, |ui| {
                    for shell in [Shell::System, Shell::PowerShell] {
                        ui.selectable_value(&mut self.terminal.shell, shell, shell_name(shell));
                    }
                });
            if self.terminal.running.is_some() {
                ui.spinner();
                stop = ui.button(locale.text("Stop")).clicked();
            }
            if ui.small_button(locale.text("Clear")).clicked() {
                self.terminal.log.clear();
            }
        });
        self.settings.shell = self.terminal.shell;
        let colours = ui.visuals().clone();
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        let scroll = std::mem::take(&mut self.terminal.scroll_to_end);
        let log = &self.terminal.log;
        let mut area = egui::ScrollArea::both()
            .id_salt("file-manager-log")
            .auto_shrink([false, false])
            .stick_to_bottom(true);
        if scroll {
            area = area.vertical_scroll_offset(f32::MAX);
        }
        area.show_rows(ui, height, log.len(), |ui, range| {
            for line in &log[range] {
                let (text, colour) = match line {
                    Line::Command(text) => (text.as_str(), colours.strong_text_color()),
                    Line::Out(text) => (text.as_str(), colours.text_color()),
                    Line::Error(text) => (text.as_str(), colours.error_fg_color),
                    Line::Note(text) => (text.as_str(), colours.warn_fg_color),
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(text).monospace().color(colour)).extend(),
                );
            }
        });
        if stop {
            if let Some(running) = self.terminal.running.as_mut() {
                running.kill();
            }
        }
    }

    fn command_line_ui(&mut self, ui: &mut egui::Ui, locale: &Locale) {
        ui.horizontal(|ui| {
            let toggle = if self.terminal.open { "▼" } else { "▲" };
            if ui
                .small_button(toggle)
                .on_hover_text(locale.text("Show or hide the command log (Ctrl+`)"))
                .clicked()
            {
                self.terminal.open = !self.terminal.open;
            }
            let prompt = self.current_folder().map_or_else(
                || locale.text("(archive)").to_owned(),
                |folder| format!("{}>", folder.display()),
            );
            ui.monospace(prompt);
            let lines = self.terminal.input.lines().count().max(1);
            let edit = egui::TextEdit::multiline(&mut self.terminal.input)
                .id_salt("file-manager-command")
                .font(egui::TextStyle::Monospace)
                .desired_rows(lines.min(6))
                .desired_width(f32::INFINITY)
                .return_key(egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                ));
            let response = ui.add(edit);
            if std::mem::take(&mut self.terminal.focus) {
                response.request_focus();
            }
            if response.has_focus() {
                let (enter, up, down, escape) = ui.input_mut(|input| {
                    (
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                        lines == 1 && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                        lines == 1
                            && input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    )
                });
                if enter {
                    self.execute_line();
                    response.request_focus();
                }
                if up {
                    if let Some(previous) = self.terminal.history.previous() {
                        previous.clone_into(&mut self.terminal.input);
                    }
                }
                if down {
                    self.terminal
                        .history
                        .newer()
                        .unwrap_or_default()
                        .clone_into(&mut self.terminal.input);
                }
                if escape {
                    self.terminal.input.clear();
                    response.surrender_focus();
                }
            }
        });
    }
}

fn shell_name(shell: Shell) -> &'static str {
    match (shell, cfg!(windows)) {
        (Shell::System, true) => "cmd",
        (Shell::System, false) => "sh",
        (Shell::PowerShell, _) => "PowerShell",
    }
}
