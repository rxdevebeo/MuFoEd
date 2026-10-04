# strict-ooxml-edit

Transactional editing of the existing WML Document: text/formatting, paragraph
structure, nested tables/SDT/text boxes, headers/footers/notes, drawings, bounded
exact undo/redo and revision checks. Architecture and acceptance:
[EDITING_PLAN.md](../docs/EDITING_PLAN.md).

```rust
use strict_ooxml_edit::{Address, Edit, EditLimits, Editor};
let mut editor = Editor::new(&mut document, EditLimits::default())?;
let changed = editor.transact(editor.revision(), &[Edit::Text {
    at: Address::body(0),
    range: 1..3, // Unicode scalars, not UTF-8 bytes or UTF-16 units
    text: "новый текст".into(),
}])?;
editor.undo(changed.revision)?;
```

Address is revision-scoped. Identify/find(story, ParaId) locate a paragraph again
after structural edits. Commands use previous command results; failure rolls
back everything. No-ops preserve redo/revision. Default history: 100 transactions
and 64 MiB estimated snapshots; media bytes remain external.

Plain Text/Format/Split/Join refuse complex content. TextNode/RunProperties edit
inside supported wrappers without flattening tabs/breaks/drawings. Field caches
and tracked revisions are protected. Validation does not claim full XSD coverage.

Features (both enabled by default):

- save: existing writer/parser pipeline, original Source, explicit Lossless /
  AllowDegraded and retained normalization/conversion issues.
- visual: shared geometry for carets (including empty paragraphs), selections
  across nodes, hit testing and drawing selection. UTF-16/grapheme boundaries
  are explicit. Rebuild maps after revision/render-option changes.

Without default features the core has no writer/renderer runtime dependency.
The original EditSession/Command API remains available for plain body text.
The facade offers the opt-in edit feature:

```rust
use strict_ooxml::edit::{Address, Edit, EditLimits, SavePolicy};
let mut editor = strict_document.edit(EditLimits::default())?;
editor.transact(0, &[Edit::Split { at: Address::body(0), offset: 2 }])?;
let saved = editor.save(1, &Default::default(), SavePolicy::Lossless)?;
let preview = editor.render_svg(&Default::default())?;
editor.refresh_support(1, &Default::default())?;
// Caller publishes saved.bytes; preview resolves the original media source.
```

Support is stale after edits/undo/redo. Refresh changes only support metadata;
lossy saves cannot clear stale support. Attribution is checked against unchanged
placement; unsupported geometry gives an error instead of guessing a source.

```powershell
cargo +1.92.0 test -p strict-ooxml-edit --all-features --locked
cargo +1.92.0 test -p strict-ooxml --features edit --locked --test editing
cargo +1.92.0 clippy -p strict-ooxml-edit --all-targets --all-features --locked -- -D warnings
```
