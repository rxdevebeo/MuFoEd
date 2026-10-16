//! Source-aware editing on the public document facade.
use crate::{Page, PlacedPage, Placement, RenderOptions, StrictDocument, WriteOptions};
use strict_ooxml_core::{
    opc::Package,
    pipeline::{PipelineStage, PipelineSummary},
};
use strict_ooxml_edit::{
    EditError, EditLimits, Editor, MediaLayer, SaveError, SavePolicy, SavedDocument, VisualError,
    VisualMap,
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
        // The model must stay within the budgets the document was read under.
        let limits = EditLimits {
            resource: self.package.limits(),
            ..limits
        };
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
        let media = self.media();
        self.editor.save(revision, Some(&media), options, policy)
    }
    /// Refreshes parser support without overwriting the live model or its history.
    pub fn refresh_support(
        &mut self,
        revision: u64,
        options: &WriteOptions,
    ) -> Result<(), SaveError> {
        // The bytes are shared, so the copy is cheap.
        let added = self.editor.added_media().clone();
        let media = MediaLayer::new(&added, Some(self.package));
        self.editor
            .refresh_support(revision, Some(&media), options)?;
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
            .visual_map(revision, options, Some(&self.media()))
    }
    /// [`Editor::visual_map_placed`] over pages from [`Self::place`] (placed
    /// with caret anchors on), saving a layout.
    pub fn visual_map_placed<P: std::borrow::Borrow<PlacedPage>>(
        &self,
        revision: u64,
        options: &RenderOptions,
        original: &[P],
    ) -> Result<VisualMap, VisualError> {
        self.editor
            .visual_map_placed(revision, options, Some(&self.media()), original)
    }
    /// Lays the live edited model out once, resolving images from its
    /// original package, for painting page by page ([`crate::paint_page`]).
    pub fn place(&self, options: &RenderOptions) -> strict_ooxml_core::error::Result<Placement> {
        strict_ooxml_render_svg::place_document(
            self.editor.document(),
            options,
            Some(&self.media()),
        )
    }
    /// Renders the live edited model, resolving images from its original package.
    pub fn render_svg(
        &self,
        options: &RenderOptions,
    ) -> strict_ooxml_core::error::Result<Vec<Page>> {
        strict_ooxml_render_svg::render_with_media(
            self.editor.document(),
            options,
            Some(&self.media()),
        )
    }
}
impl DocumentEditor<'_> {
    /// The session's added pictures over the original package.
    fn media(&self) -> MediaLayer<'_, Package> {
        MediaLayer::new(self.editor.added_media(), Some(self.package))
    }
    /// A media part's bytes: a picture the session added, or one of the
    /// original package.
    ///
    /// # Errors
    ///
    /// When the part is in neither.
    pub fn media_bytes(
        &self,
        part: &strict_ooxml_core::part::PartId,
    ) -> strict_ooxml_core::error::Result<Vec<u8>> {
        strict_ooxml_render_svg::MediaSource::read_media(&self.media(), part)
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
