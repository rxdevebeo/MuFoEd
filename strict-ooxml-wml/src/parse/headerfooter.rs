//! Parsing of header (`w:hdr`) and footer (`w:ftr`) parts (STAGE-5 S5.2).
//!
//! Headers and footers contain the same block-level content as the document
//! body, so the body block parser is reused. Only the root element differs; the
//! parts are discovered by relationship type, never by file name (STAGE-5 §5.1).

use strict_ooxml_core::error::{Result, SourceLocation};

use crate::model::block::Block;
use crate::model::support::SupportStatus;

use super::PartParser;

impl PartParser<'_> {
    /// Parses a `w:hdr` root, returning its blocks and the root location.
    pub(crate) fn parse_header_root(&mut self) -> Result<(Vec<Block>, SourceLocation)> {
        self.parse_decoration_root("hdr", "w:hdr")
    }

    /// Parses a `w:ftr` root, returning its blocks and the root location.
    pub(crate) fn parse_footer_root(&mut self) -> Result<(Vec<Block>, SourceLocation)> {
        self.parse_decoration_root("ftr", "w:ftr")
    }

    /// Parses a header/footer root element and its block children.
    fn parse_decoration_root(
        &mut self,
        root: &str,
        feature: &str,
    ) -> Result<(Vec<Block>, SourceLocation)> {
        self.enter()?;
        self.expect_root(root)?;
        let location = self.location();
        self.record(
            feature,
            SupportStatus::Supported,
            None,
            Some(location.clone()),
        );
        // `parse_block_children` consumes the root's matching end element.
        let (blocks, _sections) = self.parse_block_children()?;
        self.expect_end_of_part()?;
        self.leave();
        Ok((blocks, location))
    }
}
