//! Real document-thread and canvas regression for a scheme on an empty page.
use super::*;
use crate::worker::{Motion, ObjectView};

fn wait(tab: &mut DocTab, ready: impl Fn(&DocTab) -> bool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(15);
    loop {
        for event in tab.poll() {
            if let TabEvent::Note(n) = event {
                assert!(!n.error, "{}: {}", n.key, n.detail);
            }
        }
        if ready(tab) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no response: object {:?}, carets {:?}",
            tab.cursor.as_ref().map(|c| &c.object),
            tab.cursor.as_ref().map(|c| &c.carets)
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
fn paint(
    ctx: &egui::Context,
    tab: &mut DocTab,
    i18n: &crate::i18n::I18n,
    time: f64,
    events: Vec<egui::Event>,
) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 850.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            crate::ruler::desk(ui, tab, crate::theme::palette(false), true, true, i18n);
        },
    );
}
fn screen(tab: &DocTab, view: &ObjectView) -> egui::Rect {
    let r = view.rects[0];
    let page = tab.view.page_screens[r.page].unwrap();
    egui::Rect::from_min_size(
        page.min + egui::vec2(r.x as f32, r.y as f32) * tab.zoom,
        egui::vec2(r.width as f32, r.height as f32) * tab.zoom,
    )
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn empty_document_scheme_apply_mouse_selection_and_caret() {
    for (fonts, zoom) in [(false, 1.0), (true, 1.0), (true, 1.079_861_2), (false, 0.75)] {
        let ctx = egui::Context::default();
        let i18n = crate::i18n::I18n::load(&[]);
        let mut tab = DocTab::open(1, Source::New, "empty".into(), zoom, fonts, &ctx);
        wait(&mut tab, |t| t.cursor.is_some());
        tab.send(Request::SchemeOfObject);
        let deadline = Instant::now() + std::time::Duration::from_secs(15);
        let mut plan = loop {
            let mut found = None;
            for event in tab.poll() {
                match event {
                    TabEvent::Scheme(p) => found = Some(p),
                    TabEvent::Note(n) => assert!(!n.error, "{}: {}", n.key, n.detail),
                    _ => {}
                }
            }
            if let Some(p) = found {
                break p;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        for node in &mut plan.diagram.nodes {
            node.text = diagram_editor::Locale::new("ru")
                .text(&node.text)
                .to_owned();
        }
        tab.send(Request::PutScheme(plan));
        wait(&mut tab, |t| {
            t.state.can_undo
                && t.cursor
                    .as_ref()
                    .is_some_and(|c| c.object.as_ref().is_some_and(|o| o.scheme))
        });
        let view = tab.cursor.as_ref().unwrap().object.clone().unwrap();
        assert!(view.rects[0].width < tab.pages[0].width);
        assert!(view.rects[0].x >= 0.0 && view.rects[0].y >= 0.0);
        tab.send(Request::Move {
            motion: Motion::DocEnd,
            extend: false,
        });
        wait(&mut tab, |t| {
            t.cursor
                .as_ref()
                .is_some_and(|c| c.object.is_none() && !c.carets.is_empty())
        });
        for n in 0..3 {
            paint(&ctx, &mut tab, &i18n, f64::from(n) * 0.1, vec![]);
        }
        let corner = screen(&tab, &view).min + egui::vec2(3.0, 3.0);
        paint(&ctx, &mut tab, &i18n, 0.4, pointer(corner, true));
        paint(&ctx, &mut tab, &i18n, 0.5, pointer(corner, false));
        wait(&mut tab, |t| {
            t.cursor
                .as_ref()
                .unwrap()
                .object
                .as_ref()
                .is_some_and(|o| o.scheme)
        });
        paint(&ctx, &mut tab, &i18n, 0.6, vec![]);
        let handle = screen(&tab, &view).right_bottom();
        let destination = handle - egui::vec2(45.0, 30.0);
        paint(&ctx, &mut tab, &i18n, 0.7, pointer(handle, true));
        paint(
            &ctx,
            &mut tab,
            &i18n,
            0.8,
            vec![egui::Event::PointerMoved(destination)],
        );
        paint(&ctx, &mut tab, &i18n, 0.9, pointer(destination, false));
        wait(&mut tab, |t| {
            t.cursor
                .as_ref()
                .unwrap()
                .object
                .as_ref()
                .is_some_and(|o| o.width < view.width)
        });
        paint(&ctx, &mut tab, &i18n, 1.0, vec![]);
        let resized = tab.cursor.as_ref().unwrap().object.as_ref().unwrap();
        let frame = screen(&tab, resized);
        let below = egui::pos2(frame.center().x, frame.bottom() + 16.0);
        paint(&ctx, &mut tab, &i18n, 1.1, pointer(below, true));
        paint(&ctx, &mut tab, &i18n, 1.2, pointer(below, false));
        wait(&mut tab, |t| {
            let c = t.cursor.as_ref().unwrap();
            c.object.is_none() && c.carets.iter().any(|r| r.page == 0)
        });
        let words = tab.state.words;
        tab.send(Request::Type("Beside scheme".into()));
        wait(&mut tab, |t| t.state.words > words);
    }
}
