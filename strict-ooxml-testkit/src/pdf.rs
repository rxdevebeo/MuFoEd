//! A PDF 1.7 builder with a correct cross-reference table.
//!
//! Object 1 is the catalog and object 2 the page tree; both are written by
//! [`PdfBuilder::build`]. Every other object is added by the test, so a hostile
//! dictionary is one string away.

use std::fmt::Write as _;

/// Builds a PDF file object by object.
#[derive(Debug, Clone)]
pub struct PdfBuilder {
    /// Index `i` holds object `i + 1`; `None` is reserved and not yet set.
    objects: Vec<Option<Vec<u8>>>,
    pages: Vec<u32>,
    media_box: [i32; 4],
}

const CATALOG: u32 = 1;
const PAGES: u32 = 2;

impl Default for PdfBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfBuilder {
    /// An empty document: catalog and page tree only, US Letter pages.
    pub fn new() -> Self {
        Self {
            objects: vec![None, None],
            pages: Vec::new(),
            media_box: [0, 0, 612, 792],
        }
    }

    /// Sets the media box of pages added afterwards.
    #[must_use]
    pub fn media_box(mut self, media_box: [i32; 4]) -> Self {
        self.set_media_box(media_box);
        self
    }

    /// Sets the media box of pages added afterwards, without consuming the builder.
    ///
    /// Coordinates are PDF user-space units (1 unit = 1 pt in these fixtures).
    pub fn set_media_box(&mut self, media_box: [i32; 4]) {
        self.media_box = media_box;
    }

    /// Reserves an object number, to be filled with [`set`](Self::set) — for
    /// objects that refer to each other (or to themselves).
    pub fn reserve(&mut self) -> u32 {
        self.objects.push(None);
        u32::try_from(self.objects.len()).expect("object count fits u32")
    }

    /// Sets the body of a reserved object.
    ///
    /// # Panics
    ///
    /// If `id` was never reserved.
    pub fn set(&mut self, id: u32, body: impl Into<Vec<u8>>) {
        let index = usize::try_from(id).expect("id fits usize") - 1;
        self.objects[index] = Some(body.into());
    }

    /// Adds an object whose body is `body` (a dictionary, an array, a number...)
    /// and returns its number.
    pub fn object(&mut self, body: impl Into<Vec<u8>>) -> u32 {
        let id = self.reserve();
        self.set(id, body);
        id
    }

    /// Adds a stream object. `dict_entries` are the entries other than
    /// `/Length` and `/Filter`, e.g. `"/Type /XObject /Subtype /Image"`. With
    /// `compress` the data is zlib-compressed and `/Filter /FlateDecode` added.
    pub fn stream(&mut self, dict_entries: &str, data: &[u8], compress: bool) -> u32 {
        let id = self.reserve();
        self.set_stream(id, dict_entries, data, compress);
        id
    }

    /// The same as [`stream`](Self::stream), into a reserved object.
    pub fn set_stream(&mut self, id: u32, dict_entries: &str, data: &[u8], compress: bool) {
        let payload = if compress {
            miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
        } else {
            data.to_vec()
        };
        let filter = if compress {
            " /Filter /FlateDecode"
        } else {
            ""
        };
        let mut body = format!(
            "<< {dict_entries} /Length {}{filter} >>\nstream\n",
            payload.len()
        )
        .into_bytes();
        body.extend_from_slice(&payload);
        body.extend_from_slice(b"\nendstream");
        self.set(id, body);
    }

    /// Adds a page whose content stream is `content` and whose resources are
    /// empty.
    pub fn page(&mut self, content: &[u8]) -> u32 {
        self.page_with(content, "<< >>")
    }

    /// Adds a page with a resource dictionary given as PDF source, e.g.
    /// `"<< /Font << /F1 7 0 R >> >>"`.
    pub fn page_with(&mut self, content: &[u8], resources: &str) -> u32 {
        self.page_with_stream(content, resources, false)
    }

    /// Like [`page_with`](Self::page_with), but zlib-compresses the content
    /// stream (`/Filter /FlateDecode`). Used by hostile tests that need a
    /// small compressed payload that expands past a budget (AUD-13).
    pub fn page_flate(&mut self, content: &[u8], resources: &str) -> u32 {
        self.page_with_stream(content, resources, true)
    }

    fn page_with_stream(&mut self, content: &[u8], resources: &str, compress: bool) -> u32 {
        let contents = self.stream("", content, compress);
        let [x0, y0, x1, y1] = self.media_box;
        let page = self.object(format!(
            "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [{x0} {y0} {x1} {y1}] /Resources {resources} /Contents {contents} 0 R >>"
        ));
        self.pages.push(page);
        page
    }

    /// Writes the file.
    ///
    /// # Panics
    ///
    /// If an object was reserved and never set.
    pub fn build(&self) -> Vec<u8> {
        let mut objects = self.objects.clone();
        objects[0] = Some(format!("<< /Type /Catalog /Pages {PAGES} 0 R >>").into_bytes());
        let kids: String = self
            .pages
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        objects[1] = Some(
            format!(
                "<< /Type /Pages /Kids [{kids}] /Count {} >>",
                self.pages.len()
            )
            .into_bytes(),
        );

        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            let body = object
                .as_ref()
                .unwrap_or_else(|| panic!("object {} reserved but never set", index + 1));
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            let _ = writeln!(table, "{offset:010} 00000 n ");
        }
        let _ = write!(
            table,
            "trailer\n<< /Size {} /Root {CATALOG} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        );
        out.extend_from_slice(table.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    fn find(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap()
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let mut pdf = PdfBuilder::new();
        pdf.page(b"BT ET");
        let bytes = pdf.build();
        let xref = find(&bytes, b"xref\n");
        let tail = text(&bytes[xref..]);
        let startxref: usize = tail
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(startxref, xref);
        for (index, line) in tail.lines().skip(3).take(4).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", index + 1).as_bytes()));
        }
    }

    #[test]
    fn self_reference_is_expressible() {
        let mut pdf = PdfBuilder::new();
        let id = pdf.reserve();
        pdf.set_stream(id, &format!("/SMask {id} 0 R"), b"\x00", false);
        assert!(text(&pdf.build()).contains(&format!("/SMask {id} 0 R")));
    }
}
