//! Source-aware editing on the public document facade.
use crate::{Page, RenderOptions, StrictDocument, WriteOptions};
use strict_ooxml_core::{
    opc::Package,
    pipeline::{PipelineStage, PipelineSummary},
};
use strict_ooxml_edit::{
    EditError, EditLimits, Editor, SaveError, SavePolicy, SavedDocument, VisualError, VisualMap,
};
/// An editing session sharing the facade's original package/media source.
pub struct DocumentEditor<'a> {
    editor: Editor<'a>,
    package: &'a Package,
    dirty: &'a mut bool,
    was_dirty: bool,
}
impl StrictDocument {
    /// Opens a transaction session over the live model, retaining the original
    /// package for pictures, hyperlinks, charts and normalization provenance.
    pub fn edit(&mut self, limits: EditLimits) -> Result<DocumentEditor<'_>, EditError> {
        let pipeline = self
            .package
            .normalization_report()
            .map_or_else(PipelineSummary::new, |r| {
                PipelineSummary::from_normalization(PipelineStage::Normalize, &r)
            });
        let editor = Editor::new(&mut self.document, limits)?.with_pipeline(pipeline);
        let was_dirty = self.support_dirty;
        Ok(DocumentEditor {
            editor,
            package: &self.package,
            dirty: &mut self.support_dirty,
            was_dirty,
        })
    }
}
impl DocumentEditor<'_> {
    /// Includes edits made before this session was opened.
    pub fn support_is_stale(&self) -> bool {
        self.was_dirty || self.editor.support_is_stale()
    }
    /// Saves through the checked writer pipeline with the original source.
    pub fn save(
        &self,
        revision: u64,
        options: &WriteOptions,
        policy: SavePolicy,
    ) -> Result<SavedDocument, SaveError> {
        self.editor
            .save(revision, Some(self.package), options, policy)
    }
    /// Refreshes parser support without overwriting the live model or its history.
    pub fn refresh_support(
        &mut self,
        revision: u64,
        options: &WriteOptions,
    ) -> Result<(), SaveError> {
        self.editor
            .refresh_support(revision, Some(self.package), options)?;
        self.was_dirty = false;
        Ok(())
    }
    /// Gets caret/selection geometry using the same source and renderer as preview.
    pub fn visual_map(
        &self,
        revision: u64,
        options: &RenderOptions,
    ) -> Result<VisualMap, VisualError> {
        self.editor
            .visual_map(revision, options, Some(self.package))
    }
    /// Renders the live edited model, resolving images from its original package.
    pub fn render_svg(
        &self,
        options: &RenderOptions,
    ) -> strict_ooxml_core::error::Result<Vec<Page>> {
        strict_ooxml_render_svg::render_with_media(
            self.editor.document(),
            options,
            Some(self.package),
        )
    }
}
impl<'a> std::ops::Deref for DocumentEditor<'a> {
    type Target = Editor<'a>;
    fn deref(&self) -> &Self::Target {
        &self.editor
    }
}
impl std::ops::DerefMut for DocumentEditor<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.editor
    }
}
impl Drop for DocumentEditor<'_> {
    fn drop(&mut self) {
        *self.dirty = self.was_dirty || self.editor.support_is_stale();
    }
}
