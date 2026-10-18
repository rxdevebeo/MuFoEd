//! The options window: the ribbon's choices, the dictionaries, the local
//! models, and whether the window's place is kept.

use eframe::egui::{self, vec2, Key, RichText};

use super::ai_ui::ModelKind;
use super::{Action, App};
use crate::ai;
use crate::spell;

/// Which page of the options window is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    /// Language, theme, panes and zoom.
    View,
    /// Spelling, fetched dictionaries and the personal word list.
    Dictionaries,
    /// The model that rewrites and translates.
    Ai,
    /// The model that reads a scan.
    Recognition,
    /// Size and position for the next start.
    Window,
}

/// What the options window remembers while it is open.
#[derive(Clone, Debug)]
pub(crate) struct SettingsDialog {
    /// The page showing.
    pub(crate) page: SettingsPage,
    /// A word being typed into the personal dictionary.
    word: String,
    /// The personal word chosen in the list.
    selected: Option<usize>,
}

impl SettingsDialog {
    /// A window opened on `page`.
    pub(crate) fn new(page: SettingsPage) -> Self {
        Self {
            page,
            word: String::new(),
            selected: None,
        }
    }

    /// Draws the window. `true` when it should close.
    pub(crate) fn show(
        &mut self,
        app: &mut App,
        ctx: &egui::Context,
        actions: &mut Vec<Action>,
    ) -> bool {
        let mut close = false;
        let response = crate::dialog::modal(ctx, "settings").show(ctx, |ui| {
            ui.set_width(640.0);
            crate::dialog::title(ui, app.i18n.t("settings.title"));
            let view = app.i18n.t("settings.page.view").to_owned();
            let dictionaries = app.i18n.t("settings.page.dictionaries").to_owned();
            let ai = app.i18n.t("settings.page.ai").to_owned();
            let recognition = app.i18n.t("settings.page.recognition").to_owned();
            let window = app.i18n.t("settings.page.window").to_owned();
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.page, SettingsPage::View, view);
                ui.selectable_value(&mut self.page, SettingsPage::Dictionaries, dictionaries);
                ui.selectable_value(&mut self.page, SettingsPage::Ai, ai);
                ui.selectable_value(&mut self.page, SettingsPage::Recognition, recognition);
                ui.selectable_value(&mut self.page, SettingsPage::Window, window);
            });
            ui.separator();
            let hint = match self.page {
                SettingsPage::View => "settings.hint.view",
                SettingsPage::Dictionaries => "settings.hint.dictionaries",
                SettingsPage::Ai => "settings.hint.ai",
                SettingsPage::Recognition => "settings.hint.recognition",
                SettingsPage::Window => "settings.hint.window",
            };
            ui.label(RichText::new(app.i18n.t(hint)).weak().size(12.0));
            egui::ScrollArea::vertical()
                .id_salt(("settings-page", page_salt(self.page)))
                .max_height(420.0)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    match self.page {
                        SettingsPage::View => view_page(ui, app),
                        SettingsPage::Dictionaries => {
                            dictionaries_page(ui, app, &mut self.word, &mut self.selected, actions);
                        }
                        SettingsPage::Ai => ai_page(ui, app, ctx),
                        SettingsPage::Recognition => recognition_page(ui, app, ctx),
                        SettingsPage::Window => window_page(ui, app, ctx),
                    }
                });
            crate::dialog::buttons(ui, |ui| {
                if crate::dialog::primary(ui, app.i18n.t("button.ok")).clicked() {
                    close = true;
                    ui.close();
                }
            });
        });
        close || response.should_close()
    }
}

fn page_salt(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::View => "view",
        SettingsPage::Dictionaries => "dictionaries",
        SettingsPage::Ai => "ai",
        SettingsPage::Recognition => "recognition",
        SettingsPage::Window => "window",
    }
}

/// One token, the form stored in the personal dictionary.
fn personal_word(text: &str) -> Option<&str> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        None
    } else {
        Some(text)
    }
}

fn view_page(ui: &mut egui::Ui, app: &mut App) {
    let mut language = None;
    crate::dialog::form("settings-view").show(ui, |ui| {
        ui.label(app.i18n.t("settings.language"));
        let current = app.i18n.language().to_owned();
        let selected = app
            .i18n
            .languages()
            .find(|(code, _)| *code == current)
            .map_or(current.clone(), |(_, name)| name.to_owned());
        egui::ComboBox::from_id_salt("settings-language")
            .selected_text(selected)
            .width(220.0)
            .show_ui(ui, |ui| {
                for (code, name) in app.i18n.languages() {
                    if ui.selectable_label(code == current, name).clicked() {
                        language = Some(code.to_owned());
                    }
                }
            });
        ui.end_row();
        ui.label(app.i18n.t("settings.author"));
        ui.text_edit_singleline(&mut app.settings.author);
        ui.end_row();
    });
    ui.label(
        RichText::new(app.i18n.t("settings.locales_hint"))
            .weak()
            .size(11.5),
    );
    if let Some(dir) = crate::settings::config_dir() {
        ui.label(
            RichText::new(dir.join("locales").display().to_string())
                .monospace()
                .size(11.0),
        );
    }
    ui.add_space(4.0);
    let dark = app.i18n.t("cmd.dark").to_owned();
    ui.checkbox(&mut app.settings.dark, dark);
    let ruler = app.i18n.t("cmd.ruler").to_owned();
    ui.checkbox(&mut app.settings.ruler, ruler);
    let navigation = app.i18n.t("cmd.navigation").to_owned();
    ui.checkbox(&mut app.settings.navigation, navigation);
    let report = app.i18n.t("cmd.report_pane").to_owned();
    if ui.checkbox(&mut app.report_open, report).changed() {
        app.settings.report = app.report_open;
    }
    let fonts = app.i18n.t("cmd.system_fonts").to_owned();
    if ui.checkbox(&mut app.settings.system_fonts, fonts).changed() {
        for tab in &app.tabs {
            tab.send(crate::worker::Request::SystemFonts(
                app.settings.system_fonts,
            ));
        }
    }
    ui.add_space(6.0);
    ui.label(app.i18n.t("settings.zoom"));
    let mut percent = app.settings.zoom * 100.0;
    if ui
        .add(egui::Slider::new(&mut percent, 25.0..=500.0).suffix("%"))
        .changed()
    {
        let value = (percent / 100.0).clamp(0.25, 5.0);
        app.settings.zoom = value;
        if let Some(tab) = app.active_tab_mut() {
            tab.zoom = value;
            tab.view.zoom_changed = ui.input(|input| input.time);
            tab.view.scroll_to_page = Some(tab.view.current_page);
        }
    }
    if let Some(code) = language {
        if app.i18n.set_language(&code) {
            app.settings.language = Some(code);
        }
    }
}

fn dictionaries_page(
    ui: &mut egui::Ui,
    app: &mut App,
    word: &mut String,
    selected: &mut Option<usize>,
    actions: &mut Vec<Action>,
) {
    let spelling = app.i18n.t("tip.spelling").to_owned();
    if ui.checkbox(&mut app.settings.spelling, spelling).changed() {
        let speller = app.speller_for_tabs();
        for tab in &mut app.tabs {
            tab.set_speller(speller.clone());
        }
    }
    ui.add_space(4.0);
    let busy = app.dictionary_download.is_some();
    let installed = app.i18n.t("settings.installed").to_owned();
    let missing = app.i18n.t("settings.not_installed").to_owned();
    let remove = app.i18n.t("spell.remove").to_owned();
    egui::Grid::new("settings-dictionaries")
        .num_columns(3)
        .spacing(vec2(16.0, 8.0))
        .show(ui, |ui| {
            for language in spell::LANGUAGES {
                let name = app.i18n.t(language.name_key).to_owned();
                ui.label(name);
                let present = spell::installed(language.code);
                ui.label(if present {
                    installed.clone()
                } else {
                    missing.clone()
                });
                if present {
                    if ui
                        .add_enabled(!busy, egui::Button::new(remove.clone()))
                        .clicked()
                    {
                        actions.push(Action::RemoveDictionary(language.code));
                    }
                } else {
                    let download = format!(
                        "{} ({:.1} MB)",
                        app.i18n.t("spell.download"),
                        language.size_mb
                    );
                    if ui.add_enabled(!busy, egui::Button::new(download)).clicked() {
                        actions.push(Action::DownloadDictionary(language.code));
                    }
                }
                ui.end_row();
            }
        });
    if let Some(dir) = spell::folder() {
        ui.label(
            RichText::new(format!(
                "{} {}",
                app.i18n.t("settings.dict_folder"),
                dir.display()
            ))
            .weak()
            .size(11.0),
        );
    }
    ui.add_space(8.0);
    ui.label(RichText::new(app.i18n.t("settings.user_words")).strong());
    let words = app
        .speller
        .as_ref()
        .map(|speller| speller.own_words())
        .unwrap_or_default();
    if selected.is_some_and(|index| index >= words.len()) {
        *selected = None;
    }
    egui::ScrollArea::vertical()
        .id_salt("settings-user-words")
        .max_height(140.0)
        .show(ui, |ui| {
            if words.is_empty() {
                ui.label(RichText::new(app.i18n.t("settings.no_user_words")).weak());
            }
            for (index, entry) in words.iter().enumerate() {
                if ui
                    .selectable_label(*selected == Some(index), entry)
                    .clicked()
                {
                    *selected = Some(index);
                }
            }
        });
    let hint = app.i18n.t("settings.word_hint").to_owned();
    let add_label = app.i18n.t("settings.add_word").to_owned();
    let remove_label = app.i18n.t("spell.remove").to_owned();
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(word)
                .hint_text(hint)
                .desired_width(220.0),
        );
        let candidate = personal_word(word).map(str::to_owned);
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        let add = ui.add_enabled(
            candidate.is_some() && app.speller.is_some(),
            egui::Button::new(add_label),
        );
        if add.clicked() || entered {
            if let (Some(entry), Some(speller)) = (candidate, app.speller.as_ref()) {
                speller.add_own(&entry);
                word.clear();
                *selected = None;
            }
        }
        if ui
            .add_enabled(selected.is_some(), egui::Button::new(remove_label))
            .clicked()
        {
            if let (Some(index), Some(speller)) = (*selected, app.speller.as_ref()) {
                if let Some(entry) = words.get(index) {
                    speller.remove_own(entry);
                    *selected = None;
                }
            }
        }
    });
    if app.speller.is_none() {
        ui.label(RichText::new(app.i18n.t("settings.dictionaries_loading")).weak());
    }
}

fn ai_page(ui: &mut egui::Ui, app: &mut App, ctx: &egui::Context) {
    crate::dialog::form("settings-ai").show(ui, |ui| {
        ui.label(app.i18n.t("settings.ocr_endpoint"));
        let endpoint = ui.text_edit_singleline(&mut app.settings.ocr_endpoint);
        if endpoint.lost_focus() {
            app.ollama_endpoint_committed(ctx);
        }
        ui.end_row();
        ui.label(app.i18n.t("settings.ai_language"));
        let current = app.settings.ai_language.clone();
        let selected = ai::LANGUAGES
            .iter()
            .find(|(name, _)| *name == current)
            .map_or(current.clone(), |(_, key)| app.i18n.t(key).to_owned());
        egui::ComboBox::from_id_salt("settings-ai-language")
            .selected_text(selected)
            .width(220.0)
            .show_ui(ui, |ui| {
                for (name, key) in ai::LANGUAGES {
                    let label = app.i18n.t(key);
                    if ui.selectable_label(*name == current, label).clicked() {
                        (*name).clone_into(&mut app.settings.ai_language);
                    }
                }
            });
        ui.end_row();
    });
    ui.label(
        RichText::new(app.i18n.t("settings.shared_endpoint"))
            .weak()
            .size(12.0),
    );
    ui.add_space(6.0);
    app.ollama_model_picker(ui, ctx, ModelKind::Text);
}

fn recognition_page(ui: &mut egui::Ui, app: &mut App, ctx: &egui::Context) {
    if !cfg!(feature = "ocr") {
        ui.label(RichText::new(app.i18n.t("import.no_ocr_build")).weak());
    }
    crate::dialog::form("settings-recognition").show(ui, |ui| {
        ui.label(app.i18n.t("settings.ocr_endpoint"));
        let endpoint = ui.text_edit_singleline(&mut app.settings.ocr_endpoint);
        if endpoint.lost_focus() {
            app.ollama_endpoint_committed(ctx);
        }
        ui.end_row();
        ui.label(app.i18n.t("settings.ocr_language"));
        ui.add(egui::TextEdit::singleline(&mut app.settings.ocr_language).hint_text("ru, en, de…"));
        ui.end_row();
    });
    ui.label(
        RichText::new(app.i18n.t("settings.shared_endpoint"))
            .weak()
            .size(12.0),
    );
    ui.add_space(6.0);
    app.ollama_model_picker(ui, ctx, ModelKind::Vision);
}

fn window_page(ui: &mut egui::Ui, app: &mut App, ctx: &egui::Context) {
    let remember = app.i18n.t("settings.remember_window").to_owned();
    ui.checkbox(&mut app.settings.remember_window, remember);
    let (size, maximized) = ctx.input(|input| {
        let view = input.viewport();
        (
            view.inner_rect.map(|rect| (rect.width(), rect.height())),
            view.maximized.unwrap_or(false),
        )
    });
    if let Some((width, height)) = size {
        let state = if maximized {
            format!(" ({})", app.i18n.t("settings.maximized"))
        } else {
            String::new()
        };
        ui.label(format!(
            "{}: {width:.0}×{height:.0}{state}",
            app.i18n.t("settings.window_now")
        ));
    }
    if crate::dialog::button(ui, app.i18n.t("settings.window_reset")).clicked() {
        app.settings.window = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
            crate::settings::DEFAULT_WIDTH,
            crate::settings::DEFAULT_HEIGHT,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::personal_word;

    #[test]
    fn a_personal_word_is_one_token() {
        assert_eq!(personal_word("  кот "), Some("кот"));
        assert_eq!(personal_word("two words"), None);
        assert_eq!(personal_word("  "), None);
    }
}
