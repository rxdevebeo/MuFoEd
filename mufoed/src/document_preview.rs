//! Read-only document preview using the editor's importers and page renderer.
use crate::formats;
use file_manager::documents::{Cancellation, Document, Page, Provider};
use std::{io::Read as _, path::Path};
use strict_ooxml::{OpenControl, PlacedPage, RenderOptions};
pub(crate) struct Documents;
impl Provider for Documents {
    fn supports(&self, path: &Path) -> bool {
        formats::kind_by_extension(path).is_some()
    }
    fn open(&self, path: &Path, cancel: &Cancellation) -> Result<Box<dyn Document>, String> {
        let control = OpenControl::new();
        let stop = control.clone();
        cancel.on_cancel(move || stop.cancel());
        if control.is_cancelled() {
            return Err("Preview cancelled".into());
        }
        let _scope = control.enter();
        match formats::kind_by_extension(path) {
            Some(formats::Kind::Pdf) => {
                let limits = strict_ooxml_pdf::PdfLimits::default();
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .and_then(|file| file.take(256 * 1024 * 1024 + 1).read_to_end(&mut bytes))
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 256 * 1024 * 1024 {
                    return Err("PDF exceeds preview input limit".into());
                }
                let raster = strict_ooxml_pdf::raster::Rasterizer::new(&bytes, limits)
                    .map_err(|e| e.to_string())?;
                Ok(Box::new(Pdf { raster, control }))
            }
            Some(formats::Kind::Djvu) => Ok(Box::new(Djvu {
                document: formats::read_djvu(path)?,
                control,
            })),
            Some(_) => {
                let source = formats::Source::File {
                    path: path.into(),
                    import: formats::ImportOptions::as_original(),
                };
                let loaded = formats::open(&source, &control)?;
                let (font_assets, _) = crate::worker::installed_fonts(
                    loaded.document.document(),
                    crate::settings::Settings::load().system_fonts,
                );
                let options = RenderOptions {
                    font_assets,
                    control: Some(control.clone()),
                    ..RenderOptions::default()
                };
                let placement = loaded.document.place(&options).map_err(|e| e.to_string())?;
                Ok(Box::new(Layout {
                    pages: placement.pages,
                    options,
                    control,
                }))
            }
            None => Err("Unsupported document format".into()),
        }
    }
}
struct Layout {
    pages: Vec<PlacedPage>,
    options: RenderOptions,
    control: OpenControl,
}
impl Document for Layout {
    fn page_count(&self) -> usize {
        self.pages.len()
    }
    fn render(&mut self, page: usize, side: u32) -> Result<Page, String> {
        if self.control.is_cancelled() {
            return Err("Preview cancelled".into());
        }
        let _scope = self.control.enter();
        let page = self.pages.get(page).ok_or("Page is not in the document")?;
        let scale = f64::from(side.saturating_sub(1)) / page.width_px.max(page.height_px).max(1.0);
        let svg = strict_ooxml::paint_page(page, &self.options);
        let bitmap = crate::raster::rasterize(&svg, scale as f32)?;
        Ok(Page {
            size: [bitmap.width, bitmap.height],
            rgba: bitmap.rgba,
        })
    }
}
struct Pdf {
    raster: strict_ooxml_pdf::raster::Rasterizer,
    control: OpenControl,
}
impl Document for Pdf {
    fn page_count(&self) -> usize {
        self.raster.page_count()
    }
    fn render(&mut self, page: usize, side: u32) -> Result<Page, String> {
        use strict_ooxml_pdf::raster::{RasterOptions, Region};
        if self.control.is_cancelled() {
            return Err("Preview cancelled".into());
        }
        let size = self
            .raster
            .page_size(
                page + 1,
                &RasterOptions {
                    scale: 1.0,
                    ..RasterOptions::default()
                },
            )
            .map_err(|e| e.to_string())?;
        let scale =
            f64::from(side.saturating_sub(1)) / f64::from(size.width.max(size.height).max(1));
        let pixels = self
            .raster
            .region_pixels(
                page + 1,
                Region::page(f64::from(size.width), f64::from(size.height)),
                &RasterOptions {
                    scale,
                    max_pixels: 4096 * 4096,
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(Page {
            size: [pixels.size.width as usize, pixels.size.height as usize],
            rgba: pixels.rgba,
        })
    }
}
struct Djvu {
    document: strict_ooxml::DjvuDocument,
    control: OpenControl,
}
impl Document for Djvu {
    fn page_count(&self) -> usize {
        self.document.pages().len()
    }
    fn render(&mut self, page: usize, side: u32) -> Result<Page, String> {
        if self.control.is_cancelled() {
            return Err("Preview cancelled".into());
        }
        let png = self
            .document
            .pages()
            .get(page)
            .ok_or("Page is not in the document")?
            .image_png(self.document.limits)
            .map_err(|e| e.to_string())?;
        Page::from_png(&png, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strict_ooxml::{RtfOptions, StrictDocument, WriteOptions};
    fn fixture_rtf() -> &'static [u8] {
        br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\paperw11906\paperh16838\margl720\margr720\margt720\margb720\f0\fs28 First page\par\page Second page\par}"
    }
    fn check(path: &Path, pages: usize) {
        let mut document = Documents
            .open(path, &Cancellation::default())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(document.page_count(), pages, "{}", path.display());
        for number in 0..pages {
            let page = document.render(number, 640).unwrap();
            assert!(page.size.iter().all(|s| *s > 0 && *s <= 640));
            assert_eq!(page.rgba.len(), page.size[0] * page.size[1] * 4);
            assert!(page
                .rgba
                .chunks_exact(4)
                .any(|p| p[0] < 200 || p[1] < 200 || p[2] < 200));
        }
        assert!(document.render(pages, 640).is_err());
    }
    #[test]
    fn office_text_and_pdf_previews_render_all_pages_without_editing() {
        let root = tempfile::tempdir().unwrap();
        let rtf = fixture_rtf();
        let imported = StrictDocument::import_rtf(rtf, &RtfOptions::default())
            .unwrap()
            .document;
        let docx = strict_ooxml::write_package(
            imported.document(),
            Some(imported.package()),
            &WriteOptions::default(),
        )
        .unwrap()
        .bytes;
        for ext in ["docx", "docm", "dotx"] {
            let path = root.path().join(format!("test.{ext}"));
            std::fs::write(&path, &docx).unwrap();
            check(&path, 2);
            assert_eq!(std::fs::read(&path).unwrap(), docx);
        }
        let path = root.path().join("test.rtf");
        std::fs::write(&path, rtf).unwrap();
        check(&path, 2);
        for ext in ["html", "htm"] {
            let path = root.path().join(format!("test.{ext}"));
            std::fs::write(
                &path,
                "<html><body><h1>Hello</h1><p>Formatted preview</p></body></html>",
            )
            .unwrap();
            check(&path, 1);
        }
        for ext in ["tex", "ltx"] {
            let path = root.path().join(format!("test.{ext}"));
            std::fs::write(
                &path,
                r"\documentclass{article}\begin{document}Hello preview.\end{document}",
            )
            .unwrap();
            check(&path, 1);
        }
        let path = root.path().join("test.pdf");
        std::fs::write(
            &path,
            imported
                .render_pdf(&RenderOptions::default())
                .unwrap()
                .bytes,
        )
        .unwrap();
        check(&path, 2);
    }
    fn chunk(id: [u8; 4], bytes: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_be_bytes());
        out.extend_from_slice(bytes);
        out
    }
    #[test]
    fn djvu_previews_decode_native_images_and_resolve_components() {
        let root = tempfile::tempdir().unwrap();
        let mut body = b"DJVU".to_vec();
        body.extend(chunk(*b"INFO", &[0, 16, 0, 16, 26, 0, 72, 0, 22, 1]));
        if body.len() % 2 == 1 {
            body.push(0);
        }
        body.extend(chunk(
            *b"BGjp",
            include_bytes!("../../strict-ooxml-djvu/tests/fixtures/red.jpg"),
        ));
        let mut bytes = b"AT&T".to_vec();
        bytes.extend(chunk(*b"FORM", &body));
        for ext in ["djvu", "djv"] {
            let path = root.path().join(format!("test.{ext}"));
            std::fs::write(&path, &bytes).unwrap();
            check(&path, 1);
        }
    }
    #[test]
    fn provider_matches_every_editor_extension_and_cancels() {
        for ext in formats::OPEN_EXTENSIONS {
            assert!(Documents.supports(Path::new(&format!("file.{ext}"))));
        }
        assert!(!Documents.supports(Path::new("archive.zip")));
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(Documents.open(Path::new("missing.docx"), &cancel).is_err());
    }
}
