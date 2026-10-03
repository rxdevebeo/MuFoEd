//! Feature Report for WordprocessingML Strict documents (`TZ` §11).
//!
//! `strict-ooxml-report` turns the Stage-2 [`SupportModel`] into a
//! deterministic, schema-valid report of which mechanisms are fully, partially
//! or not at all supported, with source locations. It is the analysis layer
//! (`TZ` §5.3) between the document model and consumers (the meta-crate and the
//! CLI).
//!
//! [`SupportModel`]: strict_ooxml_wml::model::support::SupportModel
//!
//! # Pipeline
//!
//! 1. [`ReportInput`] bundles the file name, tool metadata, conformance and the
//!    [`SupportModel`](strict_ooxml_wml::model::support::SupportModel).
//! 2. [`build()`] produces a [`SupportReport`] (sorted features, summary,
//!    `overall_status`, severity).
//! 3. [`json::to_json`] serializes it deterministically; [`text::render`]
//!    renders it for humans.
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml_report::{build, json, text, ReportInput};
//! use strict_ooxml_wml::model::support::SupportModel;
//!
//! let support = SupportModel::new();
//! let report = build(ReportInput::new("document.docx", &support));
//! println!("{}", json::to_json(&report));
//! println!("{}", text::render(&report));
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(clippy::doc_markdown)]

pub mod build;
pub mod json;
pub mod model;
pub mod schema;
pub mod severity;
pub mod text;

pub use build::{build, normalization_block_from, ReportInput};
pub use model::{
    AppliedTransform, ConformanceBlock, ConformanceName, Feature, FeatureStatus, Location, Loss,
    NormalizationBlock, OverallStatus, Severity, Summary, SupportReport, Tool, SCHEMA_VERSION,
    STANDARD,
};
pub use schema::{schema_json, SUPPORT_REPORT_SCHEMA};
