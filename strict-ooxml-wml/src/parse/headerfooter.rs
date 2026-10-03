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
        self.nested(|parser| {
            parser.expect_root(root)?;
            let location = parser.location();
            parser.record(
                feature,
                SupportStatus::Supported,
                None,
                Some(location.clone()),
            );
            // `parse_block_children` consumes the root's matching end element. A header is
            // one level of block nesting: a table in a header nests no shallower than
            // a table in the body, and the header's own stack frame is paid for on
            // top of the body's.
            let blocks = parser.nested_block(PartParser::parse_block_children)?;
            parser.expect_end_of_part()?;
            Ok((blocks, location))
        })
    }
}
