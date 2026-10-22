//! The modal dialogs: copy and move, new folder, rename, delete, select by
//! mask, pack, unpack, and the error report.

use std::path::{Path, PathBuf};

use super::FileManager;
use crate::archive::Kind;
use crate::entry::Entry;
use crate::ops::{Conflict, Operation, Report};
use crate::Locale;

/// An open dialog and its fields.
pub(crate) enum Dialog {
    Transfer {
        moving: bool,
        sources: Vec<Entry>,
        /// Set when the sources are members of this archive.
        from_archive: Option<PathBuf>,
        destination: String,
        conflict: Conflict,
    },
    MakeFolder {
        name: String,
    },
    Rename {
        from: PathBuf,
        name: String,
    },
    Delete {
        sources: Vec<PathBuf>,
        permanent: bool,
    },
    SelectMask {
        select: bool,
        mask: String,
        folders: bool,
    },
    Pack {
        sources: Vec<PathBuf>,
        name: String,
        kind: Kind,
        destination: String,
    },
    Unpack {
        archive: PathBuf,
        members: Option<Vec<String>>,
        destination: String,
        overwrite: bool,
    },
    Report {
        title: &'static str,
        report: Report,
    },
}

/// What the user did with a dialog this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Open,
    Cancel,
    Confirm,
}

fn conflict_name(conflict: Conflict) -> &'static str {
    match conflict {
        Conflict::Skip => "Skip existing files",
        Conflict::Overwrite => "Overwrite",
        Conflict::OverwriteOlder => "Overwrite older files",
        Conflict::Rename => "Keep both (numbered name)",
    }
}

/// A text field that takes the focus when the dialog opens; Enter confirms.
fn field(ui: &mut egui::Ui, text: &mut String, outcome: &mut Outcome) {
    let response = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
    if !response.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }
    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
        *outcome = Outcome::Confirm;
    }
}

/// OK and Cancel; Enter and Escape too.
fn buttons(ui: &mut egui::Ui, locale: &Locale, ok: &str, outcome: &mut Outcome) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(locale.text(ok)).clicked() {
            *outcome = Outcome::Confirm;
        }
        if ui.button(locale.text("Cancel")).clicked() {
            *outcome = Outcome::Cancel;
        }
    });
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        *outcome = Outcome::Cancel;
    }
}

/// A path typed relative to `base` made absolute.
fn absolute(text: &str, base: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(text.trim().trim_matches('"'));
    match base {
        Some(base) if !path.is_absolute() => base.join(path),
        _ => path,
    }
}

fn names(sources: &[PathBuf]) -> String {
    match sources {
        [single] => single
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        many => many.len().to_string(),
    }
}

impl FileManager {
    #[allow(clippy::too_many_lines)]
    pub(super) fn dialog_ui(&mut self, ctx: &egui::Context, locale: &Locale) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let mut outcome = Outcome::Open;
        let modal = egui::Modal::new(egui::Id::new("file-manager-dialog")).show(ctx, |ui| {
            ui.set_width(460.0);
            match dialog {
                Dialog::Transfer {
                    moving,
                    sources,
                    from_archive,
                    destination,
                    conflict,
                } => {
                    let title = match (from_archive.is_some(), *moving) {
                        (true, _) => "Extract",
                        (false, true) => "Move",
                        (false, false) => "Copy",
                    };
                    ui.heading(locale.text(title));
                    let count = sources.len();
                    let what = if count == 1 {
                        sources[0].name.clone()
                    } else {
                        locale.format("{} items", &[count.to_string()])
                    };
                    ui.label(locale.format("{} to:", &[what]));
                    field(ui, destination, &mut outcome);
                    egui::ComboBox::from_id_salt("file-manager-conflict")
                        .width(260.0)
                        .selected_text(locale.text(conflict_name(*conflict)))
                        .show_ui(ui, |ui| {
                            for option in [
                                Conflict::Skip,
                                Conflict::Overwrite,
                                Conflict::OverwriteOlder,
                                Conflict::Rename,
                            ] {
                                ui.selectable_value(
                                    conflict,
                                    option,
                                    locale.text(conflict_name(option)),
                                );
                            }
                        });
                    buttons(ui, locale, "OK", &mut outcome);
                }
                Dialog::MakeFolder { name } => {
                    ui.heading(locale.text("New folder"));
                    ui.label(locale.text("Name (a/b/c makes nested folders):"));
                    field(ui, name, &mut outcome);
                    buttons(ui, locale, "Create", &mut outcome);
                }
                Dialog::Rename { from, name } => {
                    ui.heading(locale.text("Rename"));
                    ui.label(from.display().to_string());
                    field(ui, name, &mut outcome);
                    buttons(ui, locale, "Rename", &mut outcome);
                }
                Dialog::Delete { sources, permanent } => {
                    ui.heading(locale.text(if *permanent {
                        "Delete for good"
                    } else {
                        "Move to the recycle bin"
                    }));
                    ui.label(locale.format("Delete {}?", &[names(sources)]));
                    if *permanent {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            locale.text("This cannot be undone."),
                        );
                    }
                    ui.checkbox(
                        permanent,
                        locale.text("Delete for good, not to the recycle bin"),
                    );
                    buttons(ui, locale, "Delete", &mut outcome);
                    if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        outcome = Outcome::Confirm;
                    }
                }
                Dialog::SelectMask {
                    select,
                    mask,
                    folders,
                } => {
                    ui.heading(locale.text(if *select {
                        "Select files"
                    } else {
                        "Deselect files"
                    }));
                    ui.label(locale.text("Mask (*.jpg *.png | thumb*):"));
                    field(ui, mask, &mut outcome);
                    ui.checkbox(folders, locale.text("Folders too"));
                    buttons(ui, locale, "OK", &mut outcome);
                }
                Dialog::Pack {
                    sources,
                    name,
                    kind,
                    destination,
                } => {
                    ui.heading(locale.text("Pack"));
                    ui.label(locale.format("Pack {} into:", &[names(sources)]));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(name).desired_width(240.0));
                        egui::ComboBox::from_id_salt("file-manager-pack-kind")
                            .selected_text(kind.extension())
                            .show_ui(ui, |ui| {
                                for option in Kind::WRITABLE {
                                    ui.selectable_value(kind, option, option.extension());
                                }
                            });
                    });
                    ui.label(locale.text("In the folder:"));
                    field(ui, destination, &mut outcome);
                    if *kind == Kind::Gz && (sources.len() != 1 || sources[0].is_dir()) {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            locale.text("gzip holds exactly one file: choose .tar.gz for more"),
                        );
                    }
                    buttons(ui, locale, "Pack", &mut outcome);
                }
                Dialog::Unpack {
                    archive,
                    members,
                    destination,
                    overwrite,
                } => {
                    ui.heading(locale.text("Unpack"));
                    let what = members.as_ref().map_or_else(
                        || {
                            archive
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        },
                        |members| locale.format("{} items", &[members.len().to_string()]),
                    );
                    ui.label(locale.format("{} to:", &[what]));
                    field(ui, destination, &mut outcome);
                    ui.checkbox(overwrite, locale.text("Overwrite existing files"));
                    buttons(ui, locale, "Unpack", &mut outcome);
                }
                Dialog::Report { title, report } => {
                    ui.heading(locale.text(title));
                    ui.label(locale.format(
                        "Done: {} files, {}; skipped {}.",
                        &[
                            report.files.to_string(),
                            crate::entry::human_size(report.bytes),
                            report.skipped.to_string(),
                        ],
                    ));
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        locale.format("Errors: {}", &[report.errors.len().to_string()]),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (path, error) in report.errors.iter().take(500) {
                                ui.small(format!("{}: {error}", path.display()));
                            }
                        });
                    ui.add_space(6.0);
                    if ui.button(locale.text("OK")).clicked()
                        || ui.input(|input| {
                            input.key_pressed(egui::Key::Enter)
                                || input.key_pressed(egui::Key::Escape)
                        })
                    {
                        outcome = Outcome::Cancel;
                    }
                }
            }
        });
        if modal.should_close() && outcome == Outcome::Open {
            outcome = Outcome::Cancel;
        }
        match outcome {
            Outcome::Open => {}
            Outcome::Cancel => self.dialog = None,
            Outcome::Confirm => {
                if let Some(dialog) = self.dialog.take() {
                    self.confirm(dialog);
                }
            }
        }
    }

    fn confirm(&mut self, dialog: Dialog) {
        let base = self.current_folder().map(Path::to_path_buf);
        match dialog {
            Dialog::Transfer {
                moving,
                sources,
                from_archive,
                destination,
                conflict,
            } => {
                let destination = absolute(&destination, base.as_deref());
                let operation = match from_archive {
                    Some(archive) => Operation::Extract {
                        archive,
                        members: Some(
                            sources
                                .iter()
                                .map(|entry| entry.path.to_string_lossy().into_owned())
                                .collect(),
                        ),
                        destination,
                        overwrite: conflict == Conflict::Overwrite,
                    },
                    None if moving => Operation::Move {
                        sources: super::paths(&sources),
                        destination,
                        conflict,
                    },
                    None => Operation::Copy {
                        sources: super::paths(&sources),
                        destination,
                        conflict,
                    },
                };
                self.run(operation);
            }
            Dialog::MakeFolder { name } => {
                let name = name.trim();
                if name.is_empty() {
                    return;
                }
                if let Some(base) = base {
                    let first = name.split(['/', '\\']).next().unwrap_or(name).to_owned();
                    self.panels[self.active].focus_after(first);
                    self.run(Operation::MakeFolder(base.join(name)));
                }
            }
            Dialog::Rename { from, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return;
                }
                let to = from
                    .parent()
                    .map_or_else(|| PathBuf::from(name), |parent| parent.join(name));
                self.panels[self.active].focus_after(name.to_owned());
                self.run(Operation::Rename { from, to });
            }
            Dialog::Delete { sources, permanent } => {
                self.panels[self.active].selected.clear();
                self.run(Operation::Delete { sources, permanent });
            }
            Dialog::SelectMask {
                select,
                mask,
                folders,
            } => {
                self.panels[self.active].select_mask(&mask, select, folders);
            }
            Dialog::Pack {
                sources,
                name,
                kind,
                destination,
            } => {
                self.settings.pack_kind = kind;
                let folder = absolute(&destination, base.as_deref());
                let archive = folder.join(format!("{}{}", name.trim(), kind.extension()));
                self.run(Operation::Pack {
                    sources,
                    archive,
                    kind,
                });
            }
            Dialog::Unpack {
                archive,
                members,
                destination,
                overwrite,
            } => {
                let destination = absolute(&destination, base.as_deref());
                self.run(Operation::Extract {
                    archive,
                    members,
                    destination,
                    overwrite,
                });
            }
            Dialog::Report { .. } => {}
        }
    }
}
