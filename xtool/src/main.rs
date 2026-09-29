//! `xtool` — development utility for the Stage-2 inventory and corpus.
//!
//! Subcommands:
//!
//! - `xsd-inventory [--xsd <file>...] [--out <path>]` — emits
//!   `coverage/wml-elements.toml` for the declared WordprocessingML Strict
//!   element inventory. When `--xsd` files are given, element names discovered
//!   there are added as `unsupported`.
//! - `coverage [--file <path>] [--min <percent>]` — checks the optional-element
//!   coverage gate and exits non-zero when it is below `--min`.
//! - `corpus-elements [--corpus <dir>]` — the independent cross-check required by
//!   REWORK M1: reports which element names actually occurring in the corpus are
//!   not marked `supported` in the inventory.
//! - `gen-docx --out <path> [--paragraphs <n>]` — writes a synthetic Strict
//!   `.docx` for benchmarks and no-panic corpus runs.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::process::ExitCode;

/// Elements that MUST be parsed (STAGE-2 §7.1); excluded from the ratio.
const MANDATORY: &[&str] = &[
    "w:document",
    "w:body",
    "w:p",
    "w:r",
    "w:t",
    "w:br",
    "w:tab",
    "w:tbl",
    "w:tr",
    "w:tc",
    "w:tblGrid",
    "w:sectPr",
    "w:pPr",
    "w:rPr",
    "w:pStyle",
    "w:rStyle",
    "w:numPr",
    "w:numId",
    "w:ilvl",
    "w:drawing",
    "w:hyperlink",
    "w:bookmarkStart",
    "w:bookmarkEnd",
    "w:fldSimple",
    "w:instrText",
    "w:fldChar",
];

/// Optional elements fully represented in the Stage-2 model.
const SUPPORTED: &[&str] = &[
    // Block level.
    "w:altChunk",
    "w:commentRangeStart",
    "w:commentRangeEnd",
    "w:commentReference",
    "w:customXml",
    "w:del",
    "w:endnoteReference",
    "w:footnoteReference",
    "w:ins",
    "w:moveFrom",
    "w:moveTo",
    "w:permEnd",
    "w:permStart",
    "w:proofErr",
    "w:sdt",
    "w:sdtContent",
    "w:sdtPr",
    "w:tag",
    "w:alias",
    "w:id",
    "w:showingPlcHdr",
    // Paragraph properties.
    "w:bidi",
    "w:contextualSpacing",
    "w:ind",
    "w:jc",
    "w:keepLines",
    "w:keepNext",
    "w:outlineLvl",
    "w:pageBreakBefore",
    "w:pBdr",
    "w:shd",
    "w:snapToGrid",
    "w:spacing",
    "w:suppressLineNumbers",
    "w:tabs",
    "w:tab",
    "w:textDirection",
    "w:widowControl",
    "w:wordWrap",
    // Run content.
    "w:cr",
    "w:delText",
    "w:delInstrText",
    "w:lastRenderedPageBreak",
    "w:noBreakHyphen",
    "w:softHyphen",
    "w:sym",
    // Run properties.
    "w:b",
    "w:caps",
    "w:color",
    "w:dstrike",
    "w:emboss",
    "w:em",
    "w:highlight",
    "w:i",
    "w:imprint",
    "w:kern",
    "w:lang",
    "w:noProof",
    "w:outline",
    "w:position",
    "w:rFonts",
    "w:rtl",
    "w:shadow",
    "w:smallCaps",
    "w:strike",
    "w:sz",
    "w:szCs",
    "w:u",
    "w:vanish",
    "w:vertAlign",
    "w:w",
    // Table model.
    "w:bidiVisual",
    "w:cantSplit",
    "w:gridAfter",
    "w:gridBefore",
    "w:gridCol",
    "w:gridSpan",
    "w:hideMark",
    "w:noWrap",
    "w:rsid",
    // Border edges parsed by `parse_borders` (paragraph/table/cell borders).
    "w:top",
    "w:bottom",
    "w:left",
    "w:right",
    "w:start",
    "w:end",
    "w:insideH",
    "w:insideV",
    "w:tcBorders",
    "w:tcFitText",
    "w:tcMar",
    "w:tcW",
    "w:tblBorders",
    "w:tblCellMar",
    "w:tblHeader",
    "w:tblInd",
    "w:tblLayout",
    "w:tblLook",
    "w:tblPr",
    "w:tblStyle",
    "w:tblW",
    "w:trHeight",
    "w:trPr",
    "w:vAlign",
    "w:vMerge",
    "w:wAfter",
    "w:wBefore",
    "w:tcPr",
    // Section properties.
    "w:cols",
    "w:col",
    "w:docGrid",
    "w:footerReference",
    "w:gutterAtTop",
    "w:headerReference",
    "w:lnNumType",
    "w:pgMar",
    "w:pgSz",
    "w:rtlGutter",
    "w:titlePg",
    "w:type",
    // Styles.
    "w:style",
    "w:styles",
    "w:basedOn",
    "w:hidden",
    "w:link",
    "w:name",
    "w:next",
    "w:semiHidden",
    "w:uiPriority",
    "w:tblPr",
    // Numbering.
    "w:numbering",
    "w:abstractNum",
    "w:abstractNumId",
    "w:lvl",
    "w:lvlJc",
    "w:lvlOverride",
    "w:lvlRestart",
    "w:lvlText",
    "w:multiLevelType",
    "w:num",
    "w:numFmt",
    "w:numStyleLink",
    "w:start",
    "w:startOverride",
    "w:styleLink",
    "w:suff",
    "w:tentative",
    "w:isLgl",
    // Settings.
    "w:settings",
    "w:autoHyphenation",
    "w:compat",
    "w:compatSetting",
    "w:decimalSymbol",
    "w:defaultTabStop",
    "w:displayBackgroundShape",
    "w:documentProtection",
    "w:doNotHyphenateCaps",
    "w:evenAndOddHeaders",
    "w:hideGrammaticalErrors",
    "w:hideSpellingErrors",
    "w:hyphenationZone",
    "w:listSeparator",
    "w:mirrorMargins",
    "w:proofState",
    "w:themeFontLang",
    "w:trackRevisions",
    "w:zoom",
    // DrawingML inline.
    "wp:inline",
    "wp:extent",
    "wp:docPr",
    "a:graphic",
    "a:graphicData",
    "pic:pic",
    "pic:nvPicPr",
    "pic:cNvPr",
    "pic:blipFill",
    "a:blip",
    "pic:spPr",
    "a:ext",
    "a:xfrm",
];

/// Optional elements only partially represented.
const PARTIAL: &[&str] = &[
    "w:background",
    "w:bdr",
    "w:docDefaults",
    "w:pgBorders",
    "w:tblPrEx",
];

/// Optional elements in Stage-2 scope but not yet represented.
const UNSUPPORTED: &[&str] = &[
    "w:mirrorIndents",
    "w:suppressOverlap",
    "w:textAlignment",
    "w:tblCaption",
    "w:tblDescription",
];

/// Elements deliberately out of the Stage-2 scope (parsed structurally, or
/// skipped and recorded): footnotes/endnotes, comments, VML, math, themes.
const IGNORED: &[&str] = &[
    "w:footnotes",
    "w:endnotes",
    "w:comments",
    "w:hdr",
    "w:ftr",
    "w:object",
    "w:pict",
    "w:ink",
    "w:theme",
    "m:oMath",
    "m:oMathPara",
    "wp:anchor",
    "w:latentStyles",
    "w:qFormat",
    "w:unhideWhenUsed",
    "w:effect",
    "w:fitText",
    "w:cs",
    "w:bCs",
    "w:iCs",
    "w:webHidden",
    "w:specVanish",
    "w:paperSrc",
    "w:pgNumType",
    "w:formProt",
    "w:noEndnote",
    "w:printerSettings",
    "w:sectPrChange",
    "w:footnotePr",
    "w:endnotePr",
    "w:nsid",
    "w:tmpl",
    "w:legacy",
    "w:lvlPicBulletId",
    "w:picBullet",
    "w:tblOverlap",
    "w:tblCellSpacing",
    "w:tblpPr",
    "w:tblStyleRowBandSize",
    "w:tblStyleColBandSize",
];

fn status_for(name: &str) -> &'static str {
    if MANDATORY.contains(&name) {
        "mandatory"
    } else if SUPPORTED.contains(&name) {
        "supported"
    } else if PARTIAL.contains(&name) {
        "partial"
    } else if UNSUPPORTED.contains(&name) {
        "unsupported"
    } else if IGNORED.contains(&name) {
        "ignored"
    } else {
        "unsupported"
    }
}

fn all_names() -> BTreeMap<String, &'static str> {
    let mut map = BTreeMap::new();
    for name in MANDATORY
        .iter()
        .chain(SUPPORTED)
        .chain(PARTIAL)
        .chain(UNSUPPORTED)
        .chain(IGNORED)
    {
        map.insert((*name).to_owned(), status_for(name));
    }
    map
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("xsd-inventory") => xsd_inventory(&args[1..]),
        Some("coverage") => coverage(&args[1..]),
        Some("corpus-elements") => corpus_elements(&args[1..]),
        Some("gen-docx") => gen_docx(&args[1..]),
        Some("--help" | "-h") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown command '{other}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: xtool <xsd-inventory|coverage|corpus-elements|gen-docx> [options]\n\
         \n\
         xsd-inventory   [--xsd <file>]... [--out <path>]\n\
         coverage        [--file <path>] [--min <percent>]\n\
         corpus-elements [--corpus <dir>]\n\
         gen-docx        --out <path> [--paragraphs <n>] [--stage5] [--stage5b]"
    );
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn arg_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut rest = args;
    while let Some(index) = rest.iter().position(|arg| arg == flag) {
        if let Some(value) = rest.get(index + 1) {
            values.push(value.as_str());
        }
        rest = &rest[index + 1..];
    }
    values
}

fn xsd_inventory(args: &[String]) -> ExitCode {
    let out = arg_value(args, "--out").unwrap_or("coverage/wml-elements.toml");
    let mut map = all_names();
    for xsd in arg_values(args, "--xsd") {
        match std::fs::read_to_string(xsd) {
            Ok(text) => {
                for name in scan_xsd_elements(&text) {
                    map.entry(format!("w:{name}")).or_insert("unsupported");
                }
            }
            Err(error) => {
                eprintln!("warning: cannot read {xsd}: {error}");
            }
        }
    }
    if let Some(parent) = std::path::Path::new(out).parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("error: cannot create {}: {error}", parent.display());
            return ExitCode::from(2);
        }
    }
    match std::fs::write(out, render_inventory(&map)) {
        Ok(()) => {
            println!("wrote {out} ({} elements)", map.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: cannot write {out}: {error}");
            ExitCode::from(2)
        }
    }
}

/// Naively scans an XSD document for `name="..."` on element declarations.
fn scan_xsd_elements(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for marker in ["<xsd:element", "<xs:element", "<element"] {
        let mut rest = text;
        while let Some(index) = rest.find(marker) {
            let after = &rest[index + marker.len()..];
            let end = after.find('>').unwrap_or(after.len());
            let tag = &after[..end];
            if let Some(name) = extract_name(tag) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            rest = &after[end.min(after.len())..];
        }
    }
    names
}

fn extract_name(tag: &str) -> Option<String> {
    let at = tag.find("name=")?;
    let after = tag[at + "name=".len()..].trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

fn render_inventory(map: &BTreeMap<String, &'static str>) -> String {
    let mut out = String::new();
    out.push_str("# WordprocessingML Strict element inventory (STAGE-2 S2.16).\n");
    out.push_str("# Generated by `xtool xsd-inventory`; do not edit by hand.\n");
    out.push_str("# status: mandatory | supported | partial | unsupported | ignored\n");
    out.push_str("# Coverage = (supported + partial) / (supported + partial + unsupported),\n");
    out.push_str("# mandatory and ignored elements are excluded from the denominator.\n\n");
    out.push_str("[meta]\n");
    out.push_str("standard = \"ISO/IEC 29500-1:2008 Strict\"\n");
    out.push_str("stage = 2\n");
    // Provenance (REWORK M1): the statuses are a curated record of the Stage-2
    // element subset; `xtool corpus-elements` is the independent cross-check.
    out.push_str("source = \"curated-stage-2\"\n");
    out.push_str("revision = \"2026-09-28\"\n");
    out.push_str("generator = \"xtool xsd-inventory\"\n");
    for (name, status) in map {
        out.push_str("\n[[elements]]\n");
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "status = \"{status}\"");
    }
    out
}

/// Aggregated status counts from an inventory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CoverageCounts {
    supported: u32,
    partial: u32,
    unsupported: u32,
    ignored: u32,
    mandatory: u32,
}

/// Parses an inventory's `[[elements]]` and counts statuses.
fn coverage_counts(text: &str) -> Result<CoverageCounts, String> {
    let value: toml::Value =
        toml::from_str(text).map_err(|error| format!("invalid TOML: {error}"))?;
    let elements = value
        .get("elements")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "no [[elements]] arrays".to_owned())?;
    let mut counts = CoverageCounts::default();
    for element in elements {
        match element.get("status").and_then(toml::Value::as_str) {
            Some("supported") => counts.supported += 1,
            Some("partial") => counts.partial += 1,
            Some("ignored") => counts.ignored += 1,
            Some("mandatory") => counts.mandatory += 1,
            // An absent or unknown status is treated as unsupported so that the
            // gate cannot be satisfied by an incomplete inventory.
            _ => counts.unsupported += 1,
        }
    }
    Ok(counts)
}

/// Computes optional-element coverage in percent.
fn coverage_percent(counts: &CoverageCounts) -> f64 {
    let denominator = counts.supported + counts.partial + counts.unsupported;
    if denominator == 0 {
        100.0
    } else {
        f64::from(counts.supported + counts.partial) * 100.0 / f64::from(denominator)
    }
}

fn coverage(args: &[String]) -> ExitCode {
    let file = arg_value(args, "--file").unwrap_or("coverage/wml-elements.toml");
    let min: f64 = arg_value(args, "--min")
        .and_then(|value| value.parse().ok())
        .unwrap_or(90.0);
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: cannot read {file}: {error}");
            return ExitCode::from(2);
        }
    };
    let counts = match coverage_counts(&text) {
        Ok(counts) => counts,
        Err(error) => {
            eprintln!("error: {file}: {error}");
            return ExitCode::from(2);
        }
    };
    let percent = coverage_percent(&counts);
    println!(
        "coverage: {percent:.1}% (supported {}, partial {}, unsupported {}, ignored {}, mandatory {})",
        counts.supported, counts.partial, counts.unsupported, counts.ignored, counts.mandatory
    );
    if percent + f64::EPSILON < min {
        eprintln!("error: optional-element coverage {percent:.1}% is below {min:.1}%");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// The independent corpus cross-check (REWORK M1 option b).
///
/// Walks every `document.xml` in the corpus, records each element's local name,
/// and classifies it against `status_for`. Elements not marked
/// `supported`/`partial`/`mandatory` are listed.
fn corpus_elements(args: &[String]) -> ExitCode {
    let dir = arg_value(args, "--corpus").unwrap_or("strict-ooxml-core/tests/samples");
    let options = strict_ooxml_core::opc::OpenOptions::default()
        .conformance(strict_ooxml_core::opc::ConformancePolicy::Permissive);
    let limits = strict_ooxml_core::limits::ResourceLimits::default();
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let mut files = 0u32;

    for path in docx_paths(std::path::Path::new(dir)) {
        let Ok(package) = strict_ooxml_core::opc::Package::open_path(&path, &options) else {
            continue;
        };
        let Ok(main) = package.main_document_part().cloned() else {
            continue;
        };
        let Ok(bytes) = package.read_part(&main) else {
            continue;
        };
        let Ok(mut reader) = strict_ooxml_core::xml::XmlReader::from_vec(bytes, main, &limits)
        else {
            continue;
        };
        loop {
            match reader.next_event() {
                Ok(strict_ooxml_core::xml::XmlEvent::StartElement { name, .. }) => {
                    let prefix = name.prefix.clone().unwrap_or_else(|| "w".to_owned());
                    *seen
                        .entry(format!("{prefix}:{}", name.local()))
                        .or_insert(0) += 1;
                }
                Ok(strict_ooxml_core::xml::XmlEvent::Eof) | Err(_) => break,
                _ => {}
            }
        }
        files += 1;
    }

    let mut covered = 0usize;
    let mut unsupported: Vec<(&String, u32)> = Vec::new();
    let mut ignored = 0usize;
    for (name, count) in &seen {
        match status_for(name) {
            "supported" | "partial" | "mandatory" => covered += 1,
            "ignored" => ignored += 1,
            _ => unsupported.push((name, *count)),
        }
    }
    unsupported.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    println!(
        "corpus: {files} file(s), {} distinct elements; {covered} covered, {ignored} ignored, {} not marked supported",
        seen.len(),
        unsupported.len()
    );
    for (name, count) in &unsupported {
        println!("  {name} x{count}");
    }
    ExitCode::SUCCESS
}

/// Returns the sorted `.docx` paths in a directory.
fn docx_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("docx"))
        .collect();
    files.sort();
    files
}

fn gen_docx(args: &[String]) -> ExitCode {
    let Some(out) = arg_value(args, "--out") else {
        eprintln!("error: gen-docx requires --out <path>");
        return ExitCode::from(2);
    };
    let stage5 = args.iter().any(|arg| arg == "--stage5");
    let stage5b = args.iter().any(|arg| arg == "--stage5b");
    let (bytes, label) = if stage5b {
        (stage5b_docx(), "stage-5B fixture".to_owned())
    } else if stage5 {
        (stage5_docx(), "stage-5 fixture".to_owned())
    } else {
        let paragraphs: usize = arg_value(args, "--paragraphs")
            .and_then(|value| value.parse().ok())
            .unwrap_or(10);
        (
            build_strict_docx(&document_xml(paragraphs)),
            format!("{paragraphs} paragraphs"),
        )
    };
    match std::fs::write(out, bytes) {
        Ok(()) => {
            println!("wrote {out} ({label})");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: cannot write {out}: {error}");
            ExitCode::from(2)
        }
    }
}

const W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const A_NS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
const R_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
const REL_BASE: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

fn document_xml(paragraphs: usize) -> Vec<u8> {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>");
    let _ = write!(xml, "<w:document xmlns:w=\"{W_NS}\"><w:body>");
    for index in 0..paragraphs {
        let _ = write!(
            xml,
            "<w:p><w:r><w:t xml:space=\"preserve\">Paragraph {index}</w:t></w:r></w:p>"
        );
    }
    xml.push_str("</w:body></w:document>");
    xml.into_bytes()
}

/// Builds a Strict fixture exercising the Stage-5 subsystems.
#[allow(clippy::too_many_lines)]
fn stage5_docx() -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\"><w:body>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Heading One</w:t></w:r></w:p>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Sub item A</w:t></w:r></w:p>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Sub item B</w:t></w:r></w:p>\
<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>Heading Two</w:t></w:r></w:p>\
<w:tbl><w:tblPr><w:tblW w:w=\"8000\" w:type=\"dxa\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"2666\"/><w:gridCol w:w=\"2666\"/><w:gridCol w:w=\"2668\"/></w:tblGrid>\
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val=\"3\"/><w:shd w:val=\"clear\" w:fill=\"D9E2F3\"/></w:tcPr><w:p><w:r><w:t>Merged header</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p><w:r><w:t>VMerge</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>C1</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>\
<w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc>\
<w:tc><w:p><w:r><w:t>C2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
<w:p><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r>\
<w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple>\
<w:r><w:t xml:space=\"preserve\"> of </w:t></w:r>\
<w:fldSimple w:instr=\" NUMPAGES \"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>\
<w:p><w:r><w:t xml:space=\"preserve\">A footnote</w:t></w:r><w:r><w:footnoteReference w:id=\"1\"/></w:r>\
<w:r><w:t xml:space=\"preserve\"> and an endnote</w:t></w:r><w:r><w:endnoteReference w:id=\"1\"/></w:r></w:p>\
<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\
<w:p><w:r><w:t>Second page body</w:t></w:r></w:p>\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\"/>\
<w:headerReference w:type=\"default\" r:id=\"rIdHeader1\"/>\
<w:headerReference w:type=\"even\" r:id=\"rIdHeader2\"/>\
<w:footerReference w:type=\"default\" r:id=\"rIdFooter1\"/>\
<w:docGrid w:type=\"lines\" w:linePitch=\"360\"/>\
<w:titlePg/></w:sectPr>\
</w:body></w:document>"
    );

    let header_default = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:hdr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>Default header</w:t></w:r></w:p></w:hdr>"
    );
    let header_even = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:hdr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>Even header</w:t></w:r></w:p></w:hdr>"
    );
    let footer = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:ftr xmlns:w=\"{W_NS}\"><w:p><w:r><w:t>Default footer</w:t></w:r></w:p></w:ftr>"
    );
    let settings = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:settings xmlns:w=\"{W_NS}\"><w:evenAndOddHeaders/></w:settings>"
    );
    let numbering = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/><w:pPr><w:ind w:start=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl>\
<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.%2\"/><w:pPr><w:ind w:start=\"1440\" w:hanging=\"360\"/></w:pPr></w:lvl>\
</w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
    );
    let theme = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><a:theme xmlns:a=\"{A_NS}\" name=\"Office\"><a:themeElements>\
<a:clrScheme name=\"Office\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1>\
<a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"44546A\"/></a:dk2>\
<a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1>\
<a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2>\
<a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3>\
<a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4>\
<a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5>\
<a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink>\
<a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink>\
</a:clrScheme>\
<a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"Cambria\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"Office\"/>\
</a:themeElements></a:theme>"
    );
    let footnotes = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:footnotes xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\">\
<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>\
<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>\
<w:footnote w:id=\"1\"><w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> Footnote body text.</w:t></w:r></w:p></w:footnote>\
</w:footnotes>"
    );
    let endnotes = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:endnotes xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\">\
<w:endnote w:id=\"1\"><w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:endnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> Endnote body text.</w:t></w:r></w:p></w:endnote>\
</w:endnotes>"
    );
    let document_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdSettings\" Type=\"{REL_BASE}/settings\" Target=\"settings.xml\"/>\
<Relationship Id=\"rIdNumbering\" Type=\"{REL_BASE}/numbering\" Target=\"numbering.xml\"/>\
<Relationship Id=\"rIdTheme\" Type=\"{REL_BASE}/theme\" Target=\"theme/theme1.xml\"/>\
<Relationship Id=\"rIdHeader1\" Type=\"{REL_BASE}/header\" Target=\"header1.xml\"/>\
<Relationship Id=\"rIdHeader2\" Type=\"{REL_BASE}/header\" Target=\"header2.xml\"/>\
<Relationship Id=\"rIdFooter1\" Type=\"{REL_BASE}/footer\" Target=\"footer1.xml\"/>\
<Relationship Id=\"rIdFootnotes\" Type=\"{REL_BASE}/footnotes\" Target=\"footnotes.xml\"/>\
<Relationship Id=\"rIdEndnotes\" Type=\"{REL_BASE}/endnotes\" Target=\"endnotes.xml\"/>\
</Relationships>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
<Override PartName=\"/word/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>\
<Override PartName=\"/word/header2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>\
<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>\
<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>\
<Override PartName=\"/word/endnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml\"/>\
</Types>";
    let root_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_BASE}/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
    );

    let entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/settings.xml", settings.as_bytes()),
        ("word/numbering.xml", numbering.as_bytes()),
        ("word/theme/theme1.xml", theme.as_bytes()),
        ("word/header1.xml", header_default.as_bytes()),
        ("word/header2.xml", header_even.as_bytes()),
        ("word/footer1.xml", footer.as_bytes()),
        ("word/footnotes.xml", footnotes.as_bytes()),
        ("word/endnotes.xml", endnotes.as_bytes()),
    ];
    zip(&entries)
}

const WP_NS: &str = "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing";
const PIC_NS: &str = "http://purl.oclc.org/ooxml/drawingml/picture";
// `WordprocessingShape`/`WordprocessingGroup` are Microsoft extensions, not ISO
// Strict schemas; real Strict documents (e.g. strict-profile.docx) use these.
const WPS_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const WPG_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";

/// Builds a Strict fixture exercising the Stage-5B drawing subsystem.
#[allow(clippy::too_many_lines)]
fn stage5b_docx() -> Vec<u8> {
    fn anchor(id: u32, name: &str, uri: &str, h: i64, v: i64, graphic_data: &str) -> String {
        format!(
            "<w:p><w:r><w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{id}\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\">\
<wp:simplePos x=\"0\" y=\"0\"/>\
<wp:positionH relativeFrom=\"page\"><wp:posOffset>{h}</wp:posOffset></wp:positionH>\
<wp:positionV relativeFrom=\"page\"><wp:posOffset>{v}</wp:posOffset></wp:positionV>\
<wp:extent cx=\"1371600\" cy=\"914400\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>\
<wp:wrapNone/><wp:docPr id=\"{id}\" name=\"{name}\"/>\
<a:graphic><a:graphicData uri=\"{uri}\">{graphic_data}</a:graphicData></a:graphic>\
</wp:anchor></w:drawing></w:r></w:p>"
        )
    }
    fn rect(name: &str, fill: &str, x: i64) -> String {
        format!(
            "<wps:wsp><wps:cNvPr id=\"1\" name=\"{name}\"/><wps:cNvSpPr/><wps:spPr>\
<a:xfrm><a:off x=\"{x}\" y=\"0\"/><a:ext cx=\"1371600\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"roundRect\"/><a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill>\
<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"1F3864\"/></a:solidFill></a:ln>\
</wps:spPr></wps:wsp>"
        )
    }
    let text_box = "<wps:wsp><wps:cNvPr id=\"4\" name=\"Box\"/><wps:cNvSpPr txBox=\"1\"/><wps:spPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"1828800\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/><a:noFill/><a:ln><a:noFill/></a:ln></wps:spPr>\
<wps:txbx><w:txbxContent><w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
<w:r><w:rPr><w:sz w:val=\"48\"/></w:rPr><w:t>Floating text</w:t></w:r></w:p></w:txbxContent></wps:txbx>\
<wps:bodyPr anchor=\"ctr\"/></wps:wsp>";
    let group = format!(
        "<wpg:wgp><wpg:cNvPr id=\"2\" name=\"Group\"/><wpg:cNvSpPr/><wpg:grpSpPr>\
<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"2743200\" cy=\"914400\"/>\
<a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"2743200\" cy=\"914400\"/></a:xfrm></wpg:grpSpPr>\
{}{}</wpg:wgp>",
        rect("Left", "70AD47", 0),
        rect("Right", "ED7D31", 1_371_600)
    );
    let picture = "<pic:pic><pic:nvPicPr><pic:cNvPr id=\"5\" name=\"img\" descr=\"anchor image\"/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImage1\"/><a:srcRect l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"914400\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"/></pic:spPr></pic:pic>";

    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:wp=\"{WP_NS}\" xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\" xmlns:wps=\"{WPS_NS}\" xmlns:wpg=\"{WPG_NS}\"><w:body>\
<w:p><w:r><w:t>Stage 5B floating objects</w:t></w:r></w:p>\
{}{}{}{}\
<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>\
<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/>\
<w:pgBorders w:offsetFrom=\"page\">\
<w:top w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"1F3864\"/>\
<w:left w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"1F3864\"/>\
<w:bottom w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"1F3864\"/>\
<w:right w:val=\"single\" w:sz=\"16\" w:space=\"24\" w:color=\"1F3864\"/>\
</w:pgBorders></w:sectPr></w:body></w:document>",
        anchor(1, "Shape", WPS_NS, 914_400, 457_200, &rect("Shape", "4472C4", 0)),
        anchor(2, "Group", WPG_NS, 2_743_200, 1_371_600, &group),
        anchor(3, "TextBox", WPS_NS, 914_400, 2_286_000, text_box),
        anchor(4, "Picture", PIC_NS, 2_743_200, 3_200_400, picture),
    );
    let document_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdImage1\" Type=\"{REL_BASE}/image\" Target=\"media/image1.png\"/></Relationships>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";
    let root_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_BASE}/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
    );
    let entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/media/image1.png", TINY_PNG),
    ];
    zip(&entries)
}

/// A tiny valid 1x1 PNG used by the Stage-5B fixture.
#[allow(clippy::unreadable_literal)]
const TINY_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

fn build_strict_docx(document: &[u8]) -> Vec<u8> {
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document),
    ])
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content) in entries {
        offsets.push(local.len() as u32);
        let crc = crc32(content);
        push_local(&mut local, name, crc, content.len(), content);
    }
    let cd_offset = local.len() as u32;
    for ((name, content), offset) in entries.iter().zip(offsets) {
        push_central(&mut central, name, crc32(content), content.len(), offset);
    }
    let cd_size = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn push_local(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, content: &[u8]) {
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(content);
}

fn push_central(out: &mut Vec<u8>, name: &str, crc: u32, size: usize, offset: u32) {
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_default_to_unsupported() {
        assert_eq!(status_for("w:p"), "mandatory");
        assert_eq!(status_for("w:ind"), "supported");
        assert_eq!(status_for("w:bdr"), "partial");
        assert_eq!(status_for("w:mirrorIndents"), "unsupported");
        assert_eq!(status_for("w:latentStyles"), "ignored");
        assert_eq!(status_for("w:totallyUnknown"), "unsupported");
    }

    #[test]
    fn coverage_formula_counts_supported_and_partial() {
        let counts = CoverageCounts {
            supported: 8,
            partial: 1,
            unsupported: 1,
            ignored: 5,
            mandatory: 3,
        };
        assert!((coverage_percent(&counts) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn coverage_with_no_optional_elements_is_full() {
        assert!((coverage_percent(&CoverageCounts::default()) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_inventory_and_missing_status_is_unsupported() {
        let text = concat!(
            "[meta]\nstandard = \"x\"\n",
            "[[elements]]\nname = \"w:a\"\nstatus = \"supported\"\n",
            "[[elements]]\nname = \"w:b\"\nstatus = \"partial\"\n",
            "[[elements]]\nname = \"w:c\"\nstatus = \"ignored\"\n",
            "[[elements]]\nname = \"w:d\"\n",
            "[[elements]]\nname = \"w:e\"\nstatus = \"weird\"\n",
            "[[elements]]\nname = \"w:f\"\nstatus = \"mandatory\"\n",
        );
        let counts = coverage_counts(text).expect("parse");
        assert_eq!(counts.supported, 1);
        assert_eq!(counts.partial, 1);
        assert_eq!(counts.unsupported, 2); // missing status + unknown status
        assert_eq!(counts.ignored, 1);
        assert_eq!(counts.mandatory, 1);
    }

    #[test]
    fn missing_elements_is_an_error() {
        assert!(coverage_counts("[meta]\nstandard = \"x\"\n").is_err());
        assert!(coverage_counts("not = toml [").is_err());
    }

    #[test]
    fn scans_xsd_element_names() {
        let xsd = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<xsd:schema xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\n",
            "  <xsd:element name=\"p\" type=\"CT_P\"/>\n",
            "  <xsd:element name=\"r\"/>\n",
            "  <xsd:element name=\"p\"/>\n",
            "</xsd:schema>",
        );
        assert_eq!(scan_xsd_elements(xsd), vec!["p".to_owned(), "r".to_owned()]);
        assert_eq!(extract_name(" name=\"x\""), Some("x".to_owned()));
        assert_eq!(extract_name("nope"), None);
    }

    #[test]
    fn inventory_rendering_has_meta_and_elements() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("w:p".to_owned(), "mandatory");
        map.insert("w:ind".to_owned(), "supported");
        let rendered = render_inventory(&map);
        assert!(rendered.contains("[meta]"));
        assert!(rendered.contains("name = \"w:p\""));
        assert!(rendered.contains("status = \"supported\""));
    }

    #[test]
    fn generated_inventory_passes_the_gate() {
        let counts = coverage_counts(&render_inventory(&all_names())).expect("parse");
        assert!(coverage_percent(&counts) >= 90.0);
    }
}
