# Clio Der Sarkissian — pages 54, 56, 104

Captured from *Mitochondrial DNA in Ancient Human Populations of Europe* (2011) using **LibreOffice** (no Microsoft Word).

## Method

1. `soffice --headless --convert-to pdf` → full A4 PDF (LibreOffice 25.2.3.2 Producer).
2. Extract **printed** pages 54, 56, 104 (source PDF indices 53, 54, 100) with pymupdf into one 3-page PDF plus per-page PDFs.
3. Extract text runs and image/figure bounding boxes with pymupdf; coordinates in PDF points (pt), origin top-left, **y from top of page**.

## Deliverables

| File | Description |
|------|-------------|
| `pages_54_56_104.pdf` | Exactly 3 pages (printed 54, 56, 104), A4 595.304×841.89 pt |
| `page_54.pdf` / `page_56.pdf` / `page_104.pdf` | Single-page PDFs |
| `pages_54_56_104_text_images.json` | Per-page `texts[]` and `images[]` with x, y, width, height |
| `manifest.json` | Producer, method, hashes, page size, timestamps |
| `README.md` | This note |

## Notes

- Page size is fixed A4 from LibreOffice export (~WPS/A4: 210×297 mm).
- Figures on pp. 54–56 are mostly **vector drawings** (not rasters); JSON includes a `vector_figure_union` bbox. Page 104 has two **raster** chart images (panels C/D) plus vector paths.
- Printed page numbers differ from LibreOffice PDF indices due to front matter.

