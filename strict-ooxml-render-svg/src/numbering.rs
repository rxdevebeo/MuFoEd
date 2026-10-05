//! List numbering evaluation (`STAGE-5` S5.10).
//!
//! Walks the document body in order and computes the marker text for every
//! numbered paragraph from `numbering.xml` (multi-level counters, `lvlText`
//! `%n` substitution, `lvlOverride`/`startOverride`, restart and level
//! indentation).

use std::collections::HashMap;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::ids::{Ilvl, NumId};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, LevelOverride, NumberingTable};
use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::Indentation;
use strict_ooxml_wml::model::{Block, Document, Paragraph, Table};

use crate::notes::NumberFormat;
use crate::style::{apply_run_props, ComputedRun};

/// The marker computed for one numbered paragraph.
#[derive(Clone, Debug)]
pub(crate) struct NumberingMarker {
    /// Rendered marker text (`1.`, `1.1`, `•`, …).
    pub text: String,
    /// Effective run format of the marker.
    pub run: ComputedRun,
    /// Level indentation start, in points.
    pub indent_start_pt: Option<f64>,
    /// Level first-line offset (negative for hanging), in points.
    pub first_line_pt: Option<f64>,
    /// Suffix after the marker (`w:suff`): `tab`, `space` or `nothing`.
    pub suffix: Option<String>,
}

/// Markers for every numbered paragraph, keyed by source location.
#[derive(Clone, Debug, Default)]
pub(crate) struct NumberingMarkers {
    markers: HashMap<SourceLocation, NumberingMarker>,
}

impl NumberingMarkers {
    /// Builds the markers for `document`.
    #[must_use]
    pub(crate) fn build(document: &Document) -> Self {
        let mut engine = Engine::new(&document.numbering, document.theme.as_ref());
        let mut markers = HashMap::new();
        collect_blocks(&document.body.blocks, &mut |paragraph| {
            if let Some(marker) = engine.marker_for_paragraph(paragraph) {
                markers.insert(paragraph.location.clone(), marker);
            }
        });
        Self { markers }
    }

    /// Returns the marker for a paragraph location.
    #[must_use]
    pub(crate) fn get(&self, location: &SourceLocation) -> Option<&NumberingMarker> {
        self.markers.get(location)
    }
}

/// Stateful numbering evaluator (one per document layout).
struct Engine<'a> {
    table: &'a NumberingTable,
    theme: Option<&'a Theme>,
    counters: HashMap<u32, [Option<u32>; 9]>,
}

impl<'a> Engine<'a> {
    /// Creates an engine over a numbering table.
    fn new(table: &'a NumberingTable, theme: Option<&'a Theme>) -> Self {
        Self {
            table,
            theme,
            counters: HashMap::new(),
        }
    }

    /// Computes the marker for a paragraph, if it is numbered.
    fn marker_for_paragraph(&mut self, paragraph: &Paragraph) -> Option<NumberingMarker> {
        let numbering = paragraph.props.numbering?;
        let num_id = numbering.num_id?.0;
        let ilvl = numbering.ilvl.map_or(0, |ilvl| ilvl.0);
        self.marker(num_id, ilvl)
    }

    /// Computes the marker for `(num_id, ilvl)` and advances the counters.
    fn marker(&mut self, num_id: u32, ilvl: u8) -> Option<NumberingMarker> {
        let table = self.table;
        let num = table.num(NumId(num_id))?;
        let abstract_num = table.resolved_abstract(NumId(num_id))?;
        let override_ = num.overrides.iter().find(|over| over.ilvl.0 == ilvl);
        let level = effective_level(abstract_num, override_, ilvl)?;

        let start = override_
            .and_then(|over| over.start_override)
            .or(level.start)
            .unwrap_or(1);
        let is_bullet = level.format.as_deref() == Some("bullet");
        let level_index = usize::from(ilvl.min(8));

        let counters = self.counters.entry(num_id).or_insert([None; 9]);
        // Reset deeper levels (Word restarts them when a higher level is used)
        // unless the deeper level opts out with `w:lvlRestart w:val="0"`.
        for (deeper, slot) in counters.iter_mut().enumerate().skip(level_index + 1) {
            let deeper_ilvl = u8::try_from(deeper).unwrap_or(8);
            let never_restarts = effective_level(abstract_num, override_, deeper_ilvl)
                .and_then(|level| level.restart)
                .is_some_and(|value| value == 0);
            if !never_restarts {
                *slot = None;
            }
        }
        if !is_bullet {
            // A list that starts at `u32::MAX` increments past it; in debug that is
            // a panic and in release it wraps to zero and the numbering restarts
            // in the middle of the document (AUD-09).
            counters[level_index] =
                Some(counters[level_index].map_or(start, |value| value.saturating_add(1)));
        }

        let text = render_text(level, counters, abstract_num, override_)?;
        let mut run = ComputedRun::default();
        apply_run_props(&mut run, &level.run, self.theme);
        let (indent_start_pt, first_line_pt) = match &level.paragraph.indentation {
            Some(indent) => indentation(indent),
            None => (None, None),
        };
        Some(NumberingMarker {
            text,
            run,
            indent_start_pt,
            first_line_pt,
            suffix: level.suffix.as_ref().map(ToString::to_string),
        })
    }
}

/// Sentinels the current paragraph is on, for its own override.
type Override<'a> = Option<&'a LevelOverride>;

/// Returns the effective level for `ilvl` (override replacement or abstract).
fn effective_level<'a>(
    abstract_num: &'a AbstractNum,
    override_: Override<'a>,
    ilvl: u8,
) -> Option<&'a Level> {
    override_
        .and_then(|over| over.level.as_ref())
        .or_else(|| abstract_num.level(Ilvl(ilvl)))
}

/// Renders a level's `w:lvlText`, substituting `%n` placeholders.
fn render_text(
    level: &Level,
    counters: &[Option<u32>; 9],
    abstract_num: &AbstractNum,
    override_: Override<'_>,
) -> Option<String> {
    let template = level.text.as_deref()?;
    // A bullet level renders its literal glyph.
    if level.format.as_deref() == Some("bullet") {
        return (!template.is_empty()).then(|| template.to_owned());
    }
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(digit) = chars.peek().and_then(|value| value.to_digit(10)) {
                chars.next();
                let referenced = u8::try_from(digit).unwrap_or(1);
                let value = placeholder_value(counters, abstract_num, override_, referenced);
                out.push_str(&value);
                continue;
            }
        }
        out.push(ch);
    }
    (!out.is_empty()).then_some(out)
}

/// Returns the formatted counter for a `%n` placeholder (1-based).
fn placeholder_value(
    counters: &[Option<u32>; 9],
    abstract_num: &AbstractNum,
    override_: Override<'_>,
    referenced: u8,
) -> String {
    let index = usize::from(referenced.saturating_sub(1)).min(8);
    let ilvl = u8::try_from(index).unwrap_or(0);
    let level = effective_level(abstract_num, override_, ilvl);
    let format = NumberFormat::from_strict(
        level.and_then(|level| level.format.as_deref()),
        NumberFormat::Decimal,
    );
    let start = level.and_then(|level| level.start).unwrap_or(1);
    let value = counters[index].unwrap_or(start);
    format.format(value)
}

/// Extracts `(indent start, first-line offset)` in points.
fn indentation(indentation: &Indentation) -> (Option<f64>, Option<f64>) {
    let start = indentation
        .start
        .map(|value| f64::from(value.value()) / 20.0);
    let first_line = if let Some(hanging) = indentation.hanging {
        Some(-f64::from(hanging.value()) / 20.0)
    } else {
        indentation
            .first_line
            .map(|value| f64::from(value.value()) / 20.0)
    };
    (start, first_line)
}

/// Walks blocks in document order, calling `visit` for each paragraph.
fn collect_blocks(blocks: &[Block], visit: &mut impl FnMut(&Paragraph)) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => visit(paragraph),
            Block::Table(table) => collect_table(table, visit),
            Block::SdtBlock(sdt) => collect_blocks(&sdt.blocks, visit),
            Block::AltChunk(_) | Block::Opaque(_) => {}
        }
    }
}

/// Walks a table's cells in document order.
fn collect_table(table: &Table, visit: &mut impl FnMut(&Paragraph)) {
    for row in &table.rows {
        for cell in &row.cells {
            collect_blocks(&cell.blocks, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Engine, NumberingMarkers};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;
    use strict_ooxml_wml::model::ids::{AbstractNumId, Ilvl, NumId};
    use strict_ooxml_wml::model::numbering::{
        AbstractNum, Level, LevelOverride, Num, NumberingTable,
    };

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/numbering.xml"), 1, 1, 0)
    }

    fn level(ilvl: u8, format: &str, text: &str) -> Level {
        let mut level = Level::new(Ilvl(ilvl));
        level.start = Some(1);
        level.format = Some(format.into());
        level.text = Some(text.into());
        level
    }

    fn table(levels: Vec<Level>) -> NumberingTable {
        let mut table = NumberingTable::new();
        table.insert_abstract(AbstractNum {
            id: AbstractNumId(0),
            multi_level_type: None,
            num_style_link: None,
            style_link: None,
            levels,
            location: location(),
        });
        table.insert_num(Num {
            num_id: NumId(1),
            abstract_num_id: AbstractNumId(0),
            overrides: Vec::new(),
            location: location(),
        });
        table
    }

    #[test]
    fn multi_level_counters_and_restart() {
        let table = table(vec![
            level(0, "decimal", "%1."),
            level(1, "decimal", "%1.%2"),
        ]);
        let mut engine = Engine::new(&table, None);
        assert_eq!(engine.marker(1, 0).unwrap().text, "1.");
        assert_eq!(engine.marker(1, 1).unwrap().text, "1.1");
        assert_eq!(engine.marker(1, 1).unwrap().text, "1.2");
        assert_eq!(engine.marker(1, 0).unwrap().text, "2.");
        // The deeper level restarts after the higher level is used again.
        assert_eq!(engine.marker(1, 1).unwrap().text, "2.1");
    }

    #[test]
    fn start_override_and_bullet() {
        let mut table = NumberingTable::new();
        table.insert_abstract(AbstractNum {
            id: AbstractNumId(0),
            multi_level_type: None,
            num_style_link: None,
            style_link: None,
            levels: vec![level(0, "decimal", "%1."), level(1, "bullet", "\u{2022}")],
            location: location(),
        });
        // Override the first level's start (first-wins insert; build the num once).
        table.insert_num(Num {
            num_id: NumId(1),
            abstract_num_id: AbstractNumId(0),
            overrides: vec![LevelOverride {
                ilvl: Ilvl(0),
                start_override: Some(5),
                level: None,
            }],
            location: location(),
        });
        let mut engine = Engine::new(&table, None);
        assert_eq!(engine.marker(1, 0).unwrap().text, "5.");
        assert_eq!(engine.marker(1, 0).unwrap().text, "6.");
        assert_eq!(engine.marker(1, 1).unwrap().text, "\u{2022}");
    }

    #[test]
    fn format_switches_are_applied() {
        let table = table(vec![
            level(0, "lowerRoman", "%1)"),
            level(1, "upperLetter", "%2)"),
        ]);
        let mut engine = Engine::new(&table, None);
        assert_eq!(engine.marker(1, 0).unwrap().text, "i)");
        assert_eq!(engine.marker(1, 1).unwrap().text, "A)");
    }

    #[test]
    fn markers_default_is_empty() {
        assert!(NumberingMarkers::default().get(&location()).is_none());
    }
}
