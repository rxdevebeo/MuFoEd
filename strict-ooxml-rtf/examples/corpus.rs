//! Local corpus runner: import/write/reopen and compare explicit body text.
#![allow(clippy::default_trait_access)]
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_rtf::{
    import, CompatibilityProfile, NulPolicy, RtfOptions, UnicodeParameterPolicy,
};
use strict_ooxml_wml::{
    model::{Block, BreakKind, Inline, RunContent},
    parse_document, ParseOptions,
};

fn inline_text(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Run(r) => r
                .content
                .iter()
                .map(|c| match c {
                    RunContent::Text(t) => t.text.clone(),
                    RunContent::Tab => "\t".into(),
                    RunContent::Break(k) => match k {
                        BreakKind::Page => "\u{c}",
                        BreakKind::Column => "\u{b}",
                        BreakKind::TextWrapping => "\n",
                    }
                    .into(),
                    RunContent::NoBreakHyphen => "\u{2011}".into(),
                    RunContent::SoftHyphen => "\u{ad}".into(),
                    RunContent::Symbol(s) => char::from_u32(0xf000 + u32::from(s.character))
                        .expect("font PUA")
                        .to_string(),
                    _ => String::new(),
                })
                .collect::<String>(),
            Inline::Field(f) => inline_text(&f.inlines),
            Inline::Hyperlink(h) => inline_text(&h.inlines),
            _ => String::new(),
        })
        .collect()
}
fn texts(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph(p) => vec![inline_text(&p.inlines)],
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| r.cells.iter().flat_map(|c| texts(&c.blocks)))
                .collect(),
            _ => vec![],
        })
        .collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: corpus INPUT_DIRECTORY WPS_REFERENCE_DIRECTORY NEW_OUTPUT_DIRECTORY".into(),
        );
    }
    let input = PathBuf::from(&args[0]);
    let references = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    fs::create_dir(&output)?;
    let manifest: Value = serde_json::from_slice(&fs::read(input.join("manifest.json"))?)?;
    let mut rows = vec![];
    let mut options = RtfOptions::default();
    if std::env::var_os("RTF_ALLOW_UNSIGNED_UNICODE").is_some() {
        options.unicode_parameter_policy = UnicodeParameterPolicy::AllowUnsignedAndReport;
    }
    if std::env::var_os("RTF_DROP_NUL").is_some() {
        options.nul_policy = NulPolicy::DropAndReport;
    }
    if std::env::var_os("RTF_WPS_TEXT").is_some() {
        options.compatibility_profile = CompatibilityProfile::WpsText;
    }
    for item in manifest.as_array().ok_or("manifest array")? {
        let name = item["filename"].as_str().ok_or("filename")?;
        let bytes = fs::read(input.join(name))?;
        let stem = name.split('_').next().ok_or("file id")?;
        let row = match import(&bytes, &options) {
            Err(error) => json!({"filename":name,"status":"ImportError","error":error.to_string()}),
            Ok(imported) => {
                let body = texts(&imported.document.body.blocks);
                let facts: Value = serde_json::from_slice(&fs::read(
                    references.join("files").join(stem).join("facts.json"),
                )?)?;
                let region = facts["regions"]
                    .as_array()
                    .ok_or("regions")?
                    .iter()
                    .find(|r| r["kind"] == "document")
                    .ok_or("document region")?;
                let expected: Vec<String> = region["paragraphs"]
                    .as_array()
                    .ok_or("paragraphs")?
                    .iter()
                    .map(|p| p["text"].as_str().unwrap_or("").to_string())
                    .collect();
                let issues: Vec<_>=imported.report.issues.iter().map(|i|json!({"feature":i.feature,"severity":format!("{:?}",i.severity),"start":i.span.start,"end":i.span.end})).collect();
                let mut bag = strict_ooxml_write::package::MediaBag::new();
                for (part, bytes) in &imported.media {
                    bag.insert(part.clone(), bytes.clone());
                }
                let written = strict_ooxml_write::write_package(
                    &imported.document,
                    Some(&bag),
                    &Default::default(),
                );
                let (writer_error, roundtrip, writer_loss_count, media_match) = match written {
                    Err(e) => (Some(e.to_string()), false, None, false),
                    Ok(w) => {
                        fs::write(output.join(format!("{stem}.docx")), &w.bytes)?;
                        let p = Package::open_reader(&w.bytes[..], &OpenOptions::default())?;
                        let d = parse_document(&p, &ParseOptions::default())?;
                        let original: Vec<_> =
                            imported.media.iter().map(|(_, b)| b.clone()).collect();
                        let actual: Vec<_> = d
                            .media
                            .iter()
                            .map(|item| p.read_part(&item.part))
                            .collect::<Result<_, _>>()?;
                        let media_match = original.len() == actual.len()
                            && original.iter().all(|b| actual.iter().any(|v| v == b));
                        (
                            None,
                            texts(&d.body.blocks) == body,
                            Some(w.report.losses().len()),
                            media_match,
                        )
                    }
                };
                fs::write(
                    output.join(format!("{stem}.text.json")),
                    serde_json::to_vec_pretty(&body)?,
                )?;
                json!({"filename":name,"status":"Imported","discarded_nuls":imported.report.discarded_nuls,"loss_count":imported.report.loss_count,"details_truncated":imported.report.details_truncated,"issues":issues,"paragraphs":body.len(),"expected_paragraphs":expected.len(),"exact_body_match":body==expected,"first_mismatch":body.iter().zip(&expected).position(|(a,b)|a!=b),"writer_error":writer_error,"roundtrip_text_match":roundtrip,"writer_loss_count":writer_loss_count,"media_parts":imported.media.len(),"roundtrip_media_bytes_match":media_match})
            }
        };
        eprintln!("{name}: {}", row["status"]);
        rows.push(row);
    }
    fs::write(
        output.join("results.json"),
        serde_json::to_vec_pretty(
            &json!({"claim":"development_measurement_not_acceptance","options":{"unicode_parameter_policy":format!("{:?}",options.unicode_parameter_policy),"nul_policy":format!("{:?}",options.nul_policy),"compatibility_profile":format!("{:?}",options.compatibility_profile),"invalid_text_policy":format!("{:?}",options.invalid_text_policy),"unsupported_policy":format!("{:?}",options.unsupported_policy)},"files":rows}),
        )?,
    )?;
    Ok(())
}
