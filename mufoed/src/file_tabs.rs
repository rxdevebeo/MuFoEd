//! Stable, ordered file identities shared by every editor in the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FileTab {
    Document(u64),
    Image(u64),
}
pub(crate) enum TabCommand {
    Select(FileTab),
    Close(FileTab),
}
#[derive(Default)]
pub(crate) struct FileTabs {
    order: Vec<FileTab>,
    active: Option<FileTab>,
}
impl FileTabs {
    pub(crate) fn order(&self) -> &[FileTab] {
        &self.order
    }
    pub(crate) fn active(&self) -> Option<FileTab> {
        self.active
    }
    pub(crate) fn open(&mut self, key: FileTab) {
        if !self.order.contains(&key) {
            self.order.push(key);
        }
        self.active = Some(key);
    }
    pub(crate) fn select(&mut self, key: Option<FileTab>) {
        if key.is_none_or(|key| self.order.contains(&key)) {
            self.active = key;
        }
    }
    pub(crate) fn remove(&mut self, key: FileTab) {
        if let Some(index) = self.order.iter().position(|&k| k == key) {
            self.order.remove(index);
            if self.active == Some(key) {
                self.active = self.order.get(index).or_else(|| self.order.last()).copied();
            }
        }
    }
    pub(crate) fn synchronize(&mut self, live: &[FileTab]) {
        let removed: Vec<_> = self
            .order
            .iter()
            .copied()
            .filter(|k| !live.contains(k))
            .collect();
        for key in removed {
            self.remove(key);
        }
        for &key in live {
            if !self.order.contains(&key) {
                self.order.push(key);
            }
        }
    }
    pub(crate) fn shortcut(&self, ctx: &egui::Context) -> Option<TabCommand> {
        let modifiers = ctx.input(|i| i.modifiers);
        if !modifiers.command || modifiers.alt {
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(modifiers, egui::Key::Tab)) {
            return self.neighbor(modifiers.shift).map(TabCommand::Select);
        }
        if ctx.input_mut(|i| {
            i.consume_key(modifiers, egui::Key::W) || i.consume_key(modifiers, egui::Key::F4)
        }) {
            return self.active.map(TabCommand::Close);
        }
        None
    }
    pub(crate) fn neighbor(&self, backwards: bool) -> Option<FileTab> {
        if self.order.is_empty() {
            return None;
        }
        let index = self
            .active
            .and_then(|key| self.order.iter().position(|&k| k == key));
        let next = index.map_or_else(
            || if backwards { self.order.len() - 1 } else { 0 },
            |i| {
                if backwards {
                    (i + self.order.len() - 1) % self.order.len()
                } else {
                    (i + 1) % self.order.len()
                }
            },
        );
        Some(self.order[next])
    }
}
#[cfg(test)]
mod tests {
    use super::{
        FileTab::{Document as D, Image as I},
        FileTabs,
    };
    #[test]
    fn mixed_files_keep_open_order_and_independent_namespaces() {
        let mut tabs = FileTabs::default();
        for key in [D(1), I(1), D(2), I(2)] {
            tabs.open(key);
        }
        tabs.open(I(1));
        assert_eq!(tabs.order(), &[D(1), I(1), D(2), I(2)]);
        assert_eq!(tabs.neighbor(false), Some(D(2)));
        assert_eq!(tabs.neighbor(true), Some(D(1)));
        tabs.select(Some(D(1)));
        assert_eq!(tabs.neighbor(true), Some(I(2)));
    }
    #[test]
    fn removing_tabs_chooses_mixed_neighbor_and_retains_home() {
        let mut tabs = FileTabs::default();
        for key in [D(1), I(1), D(2)] {
            tabs.open(key);
        }
        tabs.select(Some(I(1)));
        tabs.remove(D(1));
        assert_eq!(tabs.active(), Some(I(1)));
        tabs.remove(I(1));
        assert_eq!(tabs.active(), Some(D(2)));
        tabs.select(None);
        tabs.synchronize(&[D(2), I(3)]);
        assert_eq!(tabs.active(), None);
        tabs.select(Some(I(3)));
        tabs.synchronize(&[D(2)]);
        assert_eq!(tabs.active(), Some(D(2)));
        tabs.remove(D(2));
        assert_eq!(tabs.active(), None);
        assert_eq!(tabs.neighbor(false), None);
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    #[test]
    fn shortcuts_cross_editor_types_with_text_focus_and_consume_the_event() {
        let mut tabs = FileTabs::default();
        tabs.open(FileTab::Document(1));
        tabs.open(FileTab::Image(1));
        tabs.open(FileTab::Document(2));
        let ctx = egui::Context::default();
        let mut field = String::from("Prompt or document search");
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut field).request_focus();
        });
        assert!(ctx.memory(|m| m.focused().is_some()));
        for (key, shift, expected) in [
            (egui::Key::Tab, true, FileTab::Image(1)),
            (egui::Key::Tab, false, FileTab::Document(2)),
            (egui::Key::W, false, FileTab::Document(2)),
        ] {
            let modifiers = egui::Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Default::default()
            };
            let input = egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let mut action = None;
            let _ = ctx.run_ui(input, |ui| {
                action = tabs.shortcut(ui.ctx());
                assert!(tabs.shortcut(ui.ctx()).is_none());
                ui.text_edit_singleline(&mut field);
            });
            match action.unwrap() {
                TabCommand::Select(actual) => {
                    assert_eq!(actual, expected);
                    tabs.select(Some(actual));
                }
                TabCommand::Close(actual) => assert_eq!(actual, expected),
            }
        }
    }
}
