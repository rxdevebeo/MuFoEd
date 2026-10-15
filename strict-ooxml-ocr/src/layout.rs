//! A page's layout as a model sees it: blocks in reading order, each with a
//! kind, a box and its text.
//!
//! A scanned page sent whole to a model comes back as lines of text: the
//! headings, the alignment, the pictures are gone. Asked for its layout, a
//! model such as Qwen-VL names the blocks - title, paragraph, figure - with a
//! box on a 0-1000 grid and the text in Markdown. That answer is a proposal: a
//! converter checks each box against the page's own pixels before it builds
//! anything from it.

/// What a block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// The page's or the document's title.
    Title,
    /// A heading.
    Heading,
    /// Running text.
    Paragraph,
    /// One item of a list.
    ListItem,
    /// A table (its text is a Markdown table).
    Table,
    /// A picture; its text is what is written inside it.
    Figure,
    /// A figure's or a table's caption.
    Caption,
    /// A running header.
    Header,
    /// A running footer.
    Footer,
    /// A page number.
    PageNumber,
    /// Anything else the model named.
    Other,
}

impl BlockKind {
    /// The kind a model's word names.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "title" => Self::Title,
            "heading" | "section_header" | "section_heading" | "subtitle" => Self::Heading,
            "paragraph" | "text" | "body" => Self::Paragraph,
            "list_item" | "list" => Self::ListItem,
            "table" => Self::Table,
            "figure" | "image" | "picture" | "illustration" | "logo" | "photo" => Self::Figure,
            "caption" => Self::Caption,
            "header" | "page_header" => Self::Header,
            "footer" | "page_footer" | "footnote" => Self::Footer,
            "page_number" => Self::PageNumber,
            _ => Self::Other,
        }
    }
}

/// How a block's lines are set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Flush left.
    Left,
    /// Centred.
    Center,
    /// Flush right.
    Right,
    /// Justified.
    Justify,
}

impl Align {
    /// The alignment a model's word names.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Some(Self::Left),
            "center" | "centre" | "centered" => Some(Self::Center),
            "right" | "end" => Some(Self::Right),
            "justify" | "justified" | "both" => Some(Self::Justify),
            _ => None,
        }
    }
}

/// One block.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBlock {
    /// What it is.
    pub kind: BlockKind,
    /// Its box as fractions of the image: left, top, right, bottom in 0..=1.
    pub bbox: [f64; 4],
    /// The heading level (1 to 3) a title or heading was given.
    pub level: Option<u8>,
    /// How its lines are set, when the model said.
    pub align: Option<Align>,
    /// Its text, Markdown as the model wrote it.
    pub text: String,
}

/// A page's blocks and who read them.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The blocks, in reading order.
    pub blocks: Vec<LayoutBlock>,
    /// The model, for the report.
    pub model: String,
    /// Its version or digest.
    pub version: String,
}

/// Puts a model's box on the 0..=1 scale: a box on Qwen-VL's 0-1000 grid, or
/// in pixels of an image of `size` when a coordinate passes 1000. `None` for a
/// box with no area or outside the image.
#[must_use]
pub fn normalize_bbox(raw: [f64; 4], size: Option<(u32, u32)>) -> Option<[f64; 4]> {
    if raw.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let max = raw.iter().copied().fold(0.0_f64, f64::max);
    let (width, height) = match size {
        Some((width, height)) if max > 1000.0 => (f64::from(width), f64::from(height)),
        _ => (1000.0, 1000.0),
    };
    let [x1, y1, x2, y2] = raw;
    let clamp = |value: f64| value.clamp(0.0, 1.0);
    let bbox = [
        clamp(x1.min(x2) / width),
        clamp(y1.min(y2) / height),
        clamp(x1.max(x2) / width),
        clamp(y1.max(y2) / height),
    ];
    (bbox[2] > bbox[0] && bbox[3] > bbox[1]).then_some(bbox)
}

/// The width and height a PNG's header states.
#[must_use]
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let read = |at: usize| -> Option<u32> {
        let slice = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([
            *slice.first()?,
            *slice.get(1)?,
            *slice.get(2)?,
            *slice.get(3)?,
        ]))
    };
    Some((read(16)?, read(20)?))
}

#[cfg(test)]
mod tests {
    use super::{normalize_bbox, png_size, Align, BlockKind};

    #[test]
    fn kinds_and_alignments_parse_leniently() {
        assert_eq!(BlockKind::parse("Section-Header"), BlockKind::Heading);
        assert_eq!(BlockKind::parse("figure"), BlockKind::Figure);
        assert_eq!(BlockKind::parse("sidebar"), BlockKind::Other);
        assert_eq!(Align::parse("Centered"), Some(Align::Center));
        assert_eq!(Align::parse("both"), Some(Align::Justify));
        assert_eq!(Align::parse("?"), None);
    }

    #[test]
    fn boxes_come_to_fractions() {
        assert_eq!(
            normalize_bbox([100.0, 200.0, 500.0, 300.0], None),
            Some([0.1, 0.2, 0.5, 0.3])
        );
        // Corners in either order.
        assert_eq!(
            normalize_bbox([500.0, 300.0, 100.0, 200.0], None),
            Some([0.1, 0.2, 0.5, 0.3])
        );
        // Pixels of a 2000x4000 image.
        assert_eq!(
            normalize_bbox([200.0, 400.0, 1000.0, 2000.0], Some((2000, 4000))),
            Some([0.1, 0.1, 0.5, 0.5])
        );
        assert_eq!(normalize_bbox([10.0, 10.0, 10.0, 50.0], None), None);
        assert_eq!(normalize_bbox([f64::NAN, 0.0, 1.0, 1.0], None), None);
    }

    #[test]
    fn a_png_header_states_its_size() {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        header.extend_from_slice(&1185u32.to_be_bytes());
        header.extend_from_slice(&1684u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((1185, 1684)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
