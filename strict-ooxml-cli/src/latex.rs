//! Public LaTeX command; input diagnostics are never hidden by writer success.
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};
use strict_ooxml::{
    LatexOpenError, LatexOptions, LatexStatus, LatexUnsupportedPolicy, StrictDocument,
};

pub(super) fn run(args: &[String]) -> ExitCode {
    match convert(args) {
        Ok(status) => ExitCode::from(match status {
            LatexStatus::Clean => 0,
            LatexStatus::Degraded => 1,
            LatexStatus::Failed => 2,
        }),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn convert(args: &[String]) -> Result<LatexStatus, Box<dyn std::error::Error>> {
    let input = args.first().ok_or("import-latex requires an input path")?;
    let mut output: Option<PathBuf> = None;
    let mut report_path: Option<PathBuf> = None;
    let mut options = LatexOptions::default();
    let mut rest = args.iter().skip(1);
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--out" if output.is_none() => {
                output = Some(rest.next().ok_or("--out requires a path")?.into());
            }
            "--report" if report_path.is_none() => {
                report_path = Some(rest.next().ok_or("--report requires a path")?.into());
            }
            "--reject-unsupported" => {
                options.unsupported_policy = LatexUnsupportedPolicy::Reject;
            }
            _ => return Err(format!("unknown or duplicate argument {flag}").into()),
        }
    }
    let output = output.ok_or("import-latex requires --out")?;
    let report_path = report_path.unwrap_or_else(|| {
        let mut p = output.as_os_str().to_owned();
        p.push(".latex-report.json");
        p.into()
    });
    // Protect the source and the report even when existing paths are aliases.
    for (a, b) in [
        (Path::new(input), output.as_path()),
        (Path::new(input), report_path.as_path()),
        (output.as_path(), report_path.as_path()),
    ] {
        if a == b || matches!((a.canonicalize(),b.canonicalize()),(Ok(a),Ok(b)) if a == b) {
            return Err("input, output and report paths must be different".into());
        }
    }
    let opened = match StrictDocument::import_latex_path(input, &options) {
        Ok(opened) => opened,
        Err(error) => {
            use strict_ooxml_core::pipeline::PipelineStage;
            let stage = match &error {
                LatexOpenError::Io(_) => PipelineStage::Input,
                LatexOpenError::Import(_) => PipelineStage::Convert,
                LatexOpenError::Package(_) => PipelineStage::Write,
            };
            let report = json!({"schema":"strictlib.latex-import.v1","status":"Failed",
                "input":input,"output":output,"options":options_json(&options),
                "import":null,"writer":null,
                "error":{"stage":format!("{stage:?}"),"message":error.to_string()}});
            std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
            return Err(error.into());
        }
    };
    let status = opened.status();
    let r = &opened.import_report;
    let issues: Vec<_> = r
        .issues
        .iter()
        .map(|i| {
            json!({"feature":i.feature,"severity":format!("{:?}",i.severity),
                "file":i.file,"span":{"start":i.span.start,"end":i.span.end}})
        })
        .collect();
    let writer: Vec<_> = opened
        .write_report
        .losses()
        .iter()
        .map(|i| json!({"feature":i.feature_id,"reason":i.reason,"severity":format!("{:?}",i.severity)}))
        .collect();
    let feature_counts: serde_json::Map<_, _> = r
        .feature_counts
        .iter()
        .map(|(feature, counts)| {
            (
                feature.clone(),
                json!({"loss_count":counts.loss_count,"info_count":counts.info_count}),
            )
        })
        .collect();
    let mut report = json!({"schema":"strictlib.latex-import.v1","status":format!("{status:?}"),
        "input":input,"output":output,"options":options_json(&options),
        "import":{"loss_count":r.loss_count,"info_count":r.info_count,"feature_counts":feature_counts,
            "unattributed_loss_count":r.unattributed_loss_count,"unattributed_info_count":r.unattributed_info_count,
            "feature_counts_truncated":r.feature_counts_truncated,"details_truncated":r.details_truncated,
            "included_files":r.included_files,"tokens":r.tokens,"nodes":r.nodes,"text_scalars":r.text_scalars,
            "issues":issues},
        "writer":{"loss_count":writer.len(),"issues":writer}});
    // A report write failure prevents creation of a success-shaped package.
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    if let Err(error) = std::fs::write(&output, &opened.strict_bytes) {
        report["status"] = json!("Failed");
        report["error"] = json!({"stage":"Write","message":error.to_string()});
        std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
        return Err(error.into());
    }
    println!(
        "{status:?}: {} ({} import losses, {} writer losses); report {}",
        output.display(),
        r.loss_count,
        opened.write_report.losses().len(),
        report_path.display()
    );
    Ok(status)
}

fn options_json(options: &LatexOptions) -> serde_json::Value {
    json!({"unsupported_policy":format!("{:?}",options.unsupported_policy)})
}
