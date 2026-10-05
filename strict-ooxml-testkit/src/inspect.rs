//! Independent ZIP and XML checks for generated fixtures.
//!
//! This is not the production OPC reader. It accepts a classic ZIP whose
//! central directory matches the local headers and whose XML parts are
//! well-formed, and it rejects a truncated archive, a bad CRC, a bad local
//! offset, and markup that is not an element tree.

use miniz_oxide::inflate::decompress_to_vec;

use crate::zip::crc32;

/// Why a package was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectError {
    /// Stable class: `zip`, `crc`, `xml`, or `pdf`.
    pub kind: &'static str,
    /// Human-readable cause. Not a production error type.
    pub detail: String,
}

impl InspectError {
    fn zip(detail: impl Into<String>) -> Self {
        Self {
            kind: "zip",
            detail: detail.into(),
        }
    }

    fn crc(detail: impl Into<String>) -> Self {
        Self {
            kind: "crc",
            detail: detail.into(),
        }
    }

    fn xml(detail: impl Into<String>) -> Self {
        Self {
            kind: "xml",
            detail: detail.into(),
        }
    }

    fn pdf(detail: impl Into<String>) -> Self {
        Self {
            kind: "pdf",
            detail: detail.into(),
        }
    }
}

/// One stored part after inflation and a CRC check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// ZIP name, as stored.
    pub name: String,
    /// Uncompressed bytes.
    pub bytes: Vec<u8>,
}

/// Reads a `.docx` and checks every XML or relationships part.
///
/// # Errors
///
/// Returns [`InspectError`] when the archive, a CRC, or an XML part is not acceptable.
pub fn inspect_docx(bytes: &[u8]) -> Result<Vec<Part>, InspectError> {
    let parts = read_zip(bytes)?;
    if parts.is_empty() {
        return Err(InspectError::zip("archive has no parts"));
    }
    let mut saw_document = false;
    for part in &parts {
        if part.name == "word/document.xml" {
            saw_document = true;
        }
        if is_xml_part(&part.name) {
            xml_well_formed(&part.bytes)
                .map_err(|detail| InspectError::xml(format!("{}: {detail}", part.name)))?;
        }
    }
    if !saw_document {
        return Err(InspectError::zip("word/document.xml is absent"));
    }
    Ok(parts)
}

/// Checks the classic PDF trailer this testkit writes.
///
/// # Errors
///
/// Returns [`InspectError`] when the header, xref, or `startxref` does not line up.
pub fn inspect_pdf(bytes: &[u8]) -> Result<(), InspectError> {
    if bytes.len() < 16 || &bytes[..5] != b"%PDF-" {
        return Err(InspectError::pdf("missing %PDF- header"));
    }
    if !bytes.windows(5).any(|window| window == b"%%EOF") {
        return Err(InspectError::pdf("missing %%EOF"));
    }
    let xref = bytes
        .windows(5)
        .position(|window| window == b"xref\n")
        .ok_or_else(|| InspectError::pdf("missing xref"))?;
    let text = String::from_utf8_lossy(bytes);
    let marker = "startxref\n";
    let start = text
        .rfind(marker)
        .ok_or_else(|| InspectError::pdf("missing startxref"))?;
    let rest = text[start + marker.len()..].trim();
    let line = rest
        .lines()
        .next()
        .ok_or_else(|| InspectError::pdf("startxref has no offset"))?;
    let offset: usize = line
        .trim()
        .parse()
        .map_err(|_| InspectError::pdf(format!("startxref is not an integer: {line}")))?;
    if offset != xref {
        return Err(InspectError::pdf(format!(
            "startxref {offset} does not point at xref {xref}"
        )));
    }
    Ok(())
}

fn is_xml_part(name: &str) -> bool {
    let extension = std::path::Path::new(name).extension();
    extension.is_some_and(|ext| ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("rels"))
}

fn read_zip(bytes: &[u8]) -> Result<Vec<Part>, InspectError> {
    let eocd = find_eocd(bytes)?;
    if eocd + 22 > bytes.len() {
        return Err(InspectError::zip("end of central directory is truncated"));
    }
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]);
    let cd_size = u32::from_le_bytes([
        bytes[eocd + 12],
        bytes[eocd + 13],
        bytes[eocd + 14],
        bytes[eocd + 15],
    ]);
    let cd_offset = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]);
    let cd_offset = usize::try_from(cd_offset).map_err(|_| InspectError::zip("cd offset"))?;
    let cd_size = usize::try_from(cd_size).map_err(|_| InspectError::zip("cd size"))?;
    let cd_end = cd_offset
        .checked_add(cd_size)
        .ok_or_else(|| InspectError::zip("central directory overflows"))?;
    if cd_end > bytes.len() {
        return Err(InspectError::zip("central directory is truncated"));
    }
    parse_central(bytes, cd_offset, cd_end, count)
}

fn find_eocd(bytes: &[u8]) -> Result<usize, InspectError> {
    if bytes.len() < 22 {
        return Err(InspectError::zip(
            "shorter than an end-of-central-directory",
        ));
    }
    let start = bytes.len().saturating_sub(22 + 65_535);
    let mut index = bytes.len() - 22;
    loop {
        if &bytes[index..index + 4] == b"PK\x05\x06" {
            return Ok(index);
        }
        if index == start {
            break;
        }
        index -= 1;
    }
    Err(InspectError::zip("end of central directory not found"))
}

fn parse_central(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    count: u16,
) -> Result<Vec<Part>, InspectError> {
    let mut parts = Vec::new();
    for _ in 0..count {
        if cursor + 46 > end {
            return Err(InspectError::zip("central directory entry is truncated"));
        }
        if &bytes[cursor..cursor + 4] != b"PK\x01\x02" {
            return Err(InspectError::zip("central directory signature mismatch"));
        }
        let method = u16::from_le_bytes([bytes[cursor + 10], bytes[cursor + 11]]);
        let crc = u32::from_le_bytes([
            bytes[cursor + 16],
            bytes[cursor + 17],
            bytes[cursor + 18],
            bytes[cursor + 19],
        ]);
        let compressed = u32_at(bytes, cursor + 20)?;
        let name_len = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]);
        let extra_len = u16::from_le_bytes([bytes[cursor + 30], bytes[cursor + 31]]);
        let comment_len = u16::from_le_bytes([bytes[cursor + 32], bytes[cursor + 33]]);
        let offset = u32_at(bytes, cursor + 42)?;
        let name_len = usize::from(name_len);
        let name_at = cursor + 46;
        let name_end = name_at
            .checked_add(name_len)
            .ok_or_else(|| InspectError::zip("name overflows"))?;
        if name_end > end {
            return Err(InspectError::zip("entry name is truncated"));
        }
        let name = String::from_utf8_lossy(&bytes[name_at..name_end]).into_owned();
        let data = read_local(bytes, offset, method, compressed, crc, &name)?;
        parts.push(Part { name, bytes: data });
        let tail = usize::from(extra_len) + usize::from(comment_len);
        cursor = name_end
            .checked_add(tail)
            .ok_or_else(|| InspectError::zip("entry tail overflows"))?;
    }
    Ok(parts)
}

fn read_local(
    bytes: &[u8],
    offset: usize,
    method: u16,
    compressed: usize,
    crc: u32,
    name: &str,
) -> Result<Vec<u8>, InspectError> {
    if offset + 30 > bytes.len() || &bytes[offset..offset + 4] != b"PK\x03\x04" {
        return Err(InspectError::zip(format!(
            "{name}: local header is missing or mis-pointed"
        )));
    }
    let name_len = usize::from(u16::from_le_bytes([bytes[offset + 26], bytes[offset + 27]]));
    let extra_len = usize::from(u16::from_le_bytes([bytes[offset + 28], bytes[offset + 29]]));
    let data_at = offset
        .checked_add(30 + name_len + extra_len)
        .ok_or_else(|| InspectError::zip(format!("{name}: data offset overflows")))?;
    let data_end = data_at
        .checked_add(compressed)
        .ok_or_else(|| InspectError::zip(format!("{name}: payload overflows")))?;
    if data_end > bytes.len() {
        return Err(InspectError::zip(format!("{name}: payload is truncated")));
    }
    let payload = &bytes[data_at..data_end];
    let data = match method {
        0 => payload.to_vec(),
        8 => decompress_to_vec(payload)
            .map_err(|_| InspectError::zip(format!("{name}: deflate failed")))?,
        _ => {
            return Err(InspectError::zip(format!(
                "{name}: compression method {method} is not checked here"
            )));
        }
    };
    if crc32(&data) != crc {
        return Err(InspectError::crc(format!("{name}: CRC-32 does not match")));
    }
    Ok(data)
}

fn u32_at(bytes: &[u8], index: usize) -> Result<usize, InspectError> {
    let value = u32::from_le_bytes([
        bytes[index],
        bytes[index + 1],
        bytes[index + 2],
        bytes[index + 3],
    ]);
    usize::try_from(value).map_err(|_| InspectError::zip("length does not fit usize"))
}

/// Well-formedness only: balanced elements, quoted attributes, known escapes.
fn xml_well_formed(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "XML is not UTF-8".to_owned())?;
    let mut parser = XmlCheck {
        text,
        index: 0,
        stack: Vec::new(),
    };
    parser.skip_bom_and_declaration()?;
    parser.node()?;
    parser.skip_space();
    if parser.index != parser.text.len() {
        return Err("trailing junk after the root element".to_owned());
    }
    if !parser.stack.is_empty() {
        return Err("unclosed element".to_owned());
    }
    Ok(())
}

struct XmlCheck<'a> {
    text: &'a str,
    index: usize,
    stack: Vec<&'a str>,
}

impl<'a> XmlCheck<'a> {
    fn skip_bom_and_declaration(&mut self) -> Result<(), String> {
        if self.text.starts_with('\u{feff}') {
            self.index = '\u{feff}'.len_utf8();
        }
        self.skip_space();
        if self.text[self.index..].starts_with("<?") {
            let end = self.text[self.index..]
                .find("?>")
                .ok_or_else(|| "unterminated declaration".to_owned())?;
            self.index += end + 2;
            self.skip_space();
        }
        Ok(())
    }

    fn node(&mut self) -> Result<(), String> {
        self.skip_space();
        if self.rest().starts_with("<!--") {
            let end = self.rest().find("-->").ok_or("unterminated comment")?;
            self.index += end + 3;
            return self.node();
        }
        if !self.rest().starts_with('<') {
            return Err("expected an element".to_owned());
        }
        self.element()
    }

    fn element(&mut self) -> Result<(), String> {
        self.index += 1;
        if self.rest().starts_with('/') {
            return Err("end tag where a start tag was required".to_owned());
        }
        let name = self.name()?;
        self.skip_space();
        while !self.rest().starts_with('>') && !self.rest().starts_with("/>") {
            if self.index >= self.text.len() {
                return Err(format!("unterminated start tag <{name}>"));
            }
            self.attribute()?;
            self.skip_space();
        }
        if self.rest().starts_with("/>") {
            self.index += 2;
            return Ok(());
        }
        self.index += 1;
        self.stack.push(name);
        self.children()?;
        Ok(())
    }

    fn children(&mut self) -> Result<(), String> {
        loop {
            if self.index >= self.text.len() {
                return Err("unclosed element".to_owned());
            }
            if self.rest().starts_with("</") {
                return self.close_from_children();
            }
            if self.rest().starts_with("<!--") {
                let end = self.rest().find("-->").ok_or("unterminated comment")?;
                self.index += end + 3;
                continue;
            }
            if self.rest().starts_with('<') {
                self.element()?;
                continue;
            }
            self.text_or_reference()?;
        }
    }

    fn close_from_children(&mut self) -> Result<(), String> {
        self.index += 2;
        let name = self.name()?;
        self.skip_space();
        if !self.rest().starts_with('>') {
            return Err(format!("end tag </{name} is not closed"));
        }
        self.index += 1;
        let open = self.stack.pop().ok_or("end tag without a start tag")?;
        if open != name {
            return Err(format!("end tag </{name}> does not match <{open}>"));
        }
        Ok(())
    }

    fn attribute(&mut self) -> Result<(), String> {
        let _name = self.name()?;
        self.skip_space();
        if !self.rest().starts_with('=') {
            return Err("attribute is missing '='".to_owned());
        }
        self.index += 1;
        self.skip_space();
        let quote = self.text.as_bytes().get(self.index).copied();
        let quote = match quote {
            Some(b'"') => '"',
            Some(b'\'') => '\'',
            _ => return Err("attribute value is not quoted".to_owned()),
        };
        self.index += 1;
        while self.index < self.text.len() && !self.rest().starts_with(quote) {
            if self.rest().starts_with('&') {
                self.reference()?;
            } else if self.rest().starts_with('<') {
                return Err("raw '<' inside an attribute".to_owned());
            } else {
                self.index += self.text[self.index..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
            }
        }
        if self.index >= self.text.len() {
            return Err("unterminated attribute value".to_owned());
        }
        self.index += 1;
        Ok(())
    }

    fn text_or_reference(&mut self) -> Result<(), String> {
        if self.rest().starts_with('&') {
            return self.reference();
        }
        if self.rest().starts_with('<') {
            return Err("bare '<' in text".to_owned());
        }
        let ch = self.text[self.index..]
            .chars()
            .next()
            .ok_or("truncated text")?;
        self.index += ch.len_utf8();
        Ok(())
    }

    fn reference(&mut self) -> Result<(), String> {
        let end = self
            .rest()
            .find(';')
            .ok_or("unterminated character reference")?;
        let body = &self.rest()[1..end];
        let ok = matches!(body, "amp" | "lt" | "gt" | "quot" | "apos")
            || (body.starts_with('#') && body.len() > 1);
        if !ok {
            return Err(format!("unknown reference &{body};"));
        }
        self.index += end + 1;
        Ok(())
    }

    fn name(&mut self) -> Result<&'a str, String> {
        let start = self.index;
        while self.index < self.text.len() {
            let ch = self.text[self.index..].chars().next().unwrap_or(' ');
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '-' | '.') {
                self.index += ch.len_utf8();
            } else {
                break;
            }
        }
        if start == self.index {
            return Err("expected a name".to_owned());
        }
        Ok(&self.text[start..self.index])
    }

    fn skip_space(&mut self) {
        while self.index < self.text.len() {
            let ch = self.text[self.index..].chars().next().unwrap_or('x');
            if ch.is_whitespace() {
                self.index += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.index..]
    }
}
