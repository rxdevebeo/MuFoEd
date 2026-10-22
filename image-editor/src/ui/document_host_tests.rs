use super::*;
#[test]
fn standard_file_shortcuts_work_with_text_focus_and_dialogs_own_their_keys() {
    for (key, shift, expected) in [
        (egui::Key::S, false, FileAction::SaveProject),
        (egui::Key::S, true, FileAction::SaveProjectAs),
        (egui::Key::O, false, FileAction::OpenImage),
        (egui::Key::N, false, FileAction::NewDocument),
        (egui::Key::W, false, FileAction::CloseDocument),
    ] {
        for modal in [false, true] {
            let ctx = egui::Context::default();
            let mut e = ImageEditor::new(Document::new(8, 6).unwrap());
            if modal {
                e.open_settings();
            }
            let id = egui::Id::new("host-focused-text");
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut text = "draft".to_owned();
                ui.add(egui::TextEdit::singleline(&mut text).id(id))
                    .request_focus();
            });
            let mods = egui::Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Default::default()
            };
            let mut actions = Vec::new();
            let _ = ctx.run_ui(
                egui::RawInput {
                    modifiers: mods,
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: mods,
                    }],
                    ..Default::default()
                },
                |ui| {
                    let mut text = "draft".to_owned();
                    assert!(ui.ctx().egui_wants_keyboard_input());
                    actions.extend(e.show(ui));
                    ui.add(egui::TextEdit::singleline(&mut text).id(id));
                },
            );
            assert_eq!(actions, if modal { Vec::new() } else { vec![expected] });
        }
    }
}
#[test]
fn cancelling_owned_background_work_keeps_preferences_and_staged_results_unchanged() {
    let mut e = ImageEditor::new(Document::new(8, 6).unwrap());
    e.request.model = "local/retained".into();
    e.request.seed = 171;
    e.generation_ui.seed_text = "171".into();
    e.generation_ui.random_seed = false;
    e.generation_ui.model_name = "Selected model".into();
    let settings = e.settings();
    let original = e.document.clone();
    e.candidate = Some(Candidate {
        raster: Raster::solid(8, 6, [100, 20, 40, 255]).unwrap(),
        source: e.document.flatten(),
        request: e.request.clone(),
    });
    assert!(e.has_staged_result());
    e.poll_background(&egui::Context::default());
    assert_eq!(
        e.document, original,
        "polling cannot accept its result automatically"
    );
    e.cancel_background();
    assert_eq!(
        e.settings(),
        settings,
        "closing or cancelling jobs cannot reset the shared preferences"
    );
    assert_eq!(e.document, original);
    let other = ImageEditor::new(Document::new(3, 4).unwrap());
    assert!(!other.has_staged_result());
}
