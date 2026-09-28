//! Public entry point for reading WordprocessingML Strict documents.
//!
//! The meta-crate ties the Stage-1 core ([`strict_ooxml_core`]) to the Stage-2
//! document model ([`strict_ooxml_wml`]). `StrictDocument` owns both the opened
//! OPC package and the parsed, immutable [`Document`].
//!
//! # Example
//!
//! ```no_run
//! use strict_ooxml::{OpenOptions, StrictDocument};
//!
//! let doc = StrictDocument::open_path("document.docx", &OpenOptions::default())?;
//! println!("blocks: {}", doc.document().body.blocks.len());
//! println!("{}", doc.support_debug());
//! # Ok::<(), strict_ooxml_core::error::StrictError>(())
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(rust_2018_idioms)]
#![allow(clippy::doc_markdown)]

use std::io::Read;
use std::path::Path;

use strict_ooxml_core::error::Result;
use strict_ooxml_core::opc::Package;
use strict_ooxml_wml::model::support::SupportModel;

pub use strict_ooxml_core::error::StrictError;
pub use strict_ooxml_core::limits::ResourceLimits;
pub use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions};
pub use strict_ooxml_wml::model;
pub use strict_ooxml_wml::model::Document;
pub use strict_ooxml_wml::ParseOptions as WmlOptions;
pub use strict_ooxml_wml::{parse_document, ParseOptions};

/// An opened Strict document: the OPC package plus its parsed model.
pub struct StrictDocument {
    package: Package,
    document: Document,
}

impl StrictDocument {
    /// Parses an already-opened package.
    ///
    /// # Errors
    ///
    /// Returns a [`StrictError`] if the package is not Strict or the document
    /// cannot be parsed.
    pub fn from_package(package: Package, options: &ParseOptions) -> Result<Self> {
        let document = parse_document(&package, options)?;
        Ok(Self { package, document })
    }

    /// Opens and parses a `.docx` from a filesystem path.
    ///
    /// # Errors
    ///
    /// See [`StrictDocument::from_package`].
    pub fn open_path(path: impl AsRef<Path>, options: &OpenOptions) -> Result<Self> {
        let package = Package::open_path(path, options)?;
        Self::from_package(package, &parse_options_from(options))
    }

    /// Opens and parses a `.docx` from any reader.
    ///
    /// # Errors
    ///
    /// See [`StrictDocument::from_package`].
    pub fn open_reader<R: Read>(reader: R, options: &OpenOptions) -> Result<Self> {
        let package = Package::open_reader(reader, options)?;
        Self::from_package(package, &parse_options_from(options))
    }

    /// Returns the parsed document model.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Returns the opened OPC package.
    #[must_use]
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Returns the support model for this document.
    #[must_use]
    pub fn support(&self) -> &SupportModel {
        self.document.support()
    }

    /// Renders a human-readable support summary.
    ///
    /// The full JSON Feature Report is delivered in Stage 3.
    #[must_use]
    pub fn support_debug(&self) -> String {
        self.document.support_debug()
    }
}

/// Builds Stage-2 parse options from the core open options.
fn parse_options_from(options: &OpenOptions) -> ParseOptions {
    ParseOptions {
        conformance: options.conformance,
        limits: options.limits,
    }
}
