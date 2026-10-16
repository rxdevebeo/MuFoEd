//! Printing: the machine's printers, and a print job of the document's laid
//! out pages.
//!
//! On Windows the pages are drawn the way the screen draws them, at the
//! printer's resolution, and handed to the printer through GDI: what is
//! printed is what is shown. Elsewhere the document goes to CUPS as a PDF
//! (`lp`), which prints it at its own size.

use std::sync::Arc;

use crate::worker::PageData;

/// What to print.
#[derive(Clone, Debug)]
pub(crate) struct Job {
    pub(crate) printer: String,
    pub(crate) copies: u32,
    /// Each copy whole before the next, rather than each page's copies
    /// together.
    pub(crate) collate: bool,
    /// Zero-based pages, in order.
    pub(crate) pages: Vec<usize>,
    pub(crate) title: String,
    /// Print to this file instead of the device (a PDF printer, say).
    pub(crate) output: Option<std::path::PathBuf>,
}

/// The pages a range like `1-3, 5, 8-` names, zero-based, among `count`
/// pages; `None` when it names none or does not read.
pub(crate) fn parse_range(text: &str, count: usize) -> Option<Vec<usize>> {
    let mut pages = Vec::new();
    for part in text.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (from, to) = if let Some((from, to)) = part.split_once('-') {
            let from = if from.trim().is_empty() {
                1
            } else {
                from.trim().parse::<usize>().ok()?
            };
            let to = if to.trim().is_empty() {
                count
            } else {
                to.trim().parse::<usize>().ok()?
            };
            (from, to)
        } else {
            let page = part.parse::<usize>().ok()?;
            (page, page)
        };
        if from == 0 || from > to {
            return None;
        }
        pages.extend((from..=to.min(count)).map(|page| page - 1));
    }
    (!pages.is_empty()).then_some(pages)
}

/// The order pages go to the printer: copies collated or not.
pub(crate) fn sheets(job: &Job) -> Vec<usize> {
    let copies = job.copies.max(1) as usize;
    if job.collate {
        (0..copies)
            .flat_map(|_| job.pages.iter().copied())
            .collect()
    } else {
        job.pages
            .iter()
            .flat_map(|page| std::iter::repeat_n(*page, copies))
            .collect()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // Narrow Win32 boundary: the printers, a GDI print job.
mod windows {
    use super::{sheets, Arc, Job, PageData};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HORZRES, LOGPIXELSX, LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX,
        PHYSICALOFFSETY, PHYSICALWIDTH, SRCCOPY, VERTRES,
    };
    use windows_sys::Win32::Graphics::Printing::{
        EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
        PRINTER_INFO_4W,
    };
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A NUL-terminated wide string.
    ///
    /// # Safety
    ///
    /// `pointer` is null or points at a NUL-terminated UTF-16 string.
    unsafe fn read_wide(pointer: *const u16) -> String {
        if pointer.is_null() {
            return String::new();
        }
        let mut length = 0;
        // SAFETY: the string is NUL-terminated, so every unit up to the NUL
        // is readable.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: the `length` units before the NUL were just read.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
    }

    pub(crate) fn printers() -> Vec<String> {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut needed = 0u32;
        let mut count = 0u32;
        // SAFETY: a null buffer of size 0 asks for the size needed.
        unsafe {
            EnumPrintersW(
                flags,
                std::ptr::null(),
                4,
                std::ptr::null_mut(),
                0,
                &raw mut needed,
                &raw mut count,
            );
        }
        if needed == 0 {
            return Vec::new();
        }
        // u64 storage keeps the PRINTER_INFO_4W records aligned.
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer holds `needed` bytes.
        let done = unsafe {
            EnumPrintersW(
                flags,
                std::ptr::null(),
                4,
                buffer.as_mut_ptr().cast(),
                needed,
                &raw mut needed,
                &raw mut count,
            )
        };
        if done == 0 {
            return Vec::new();
        }
        let infos = buffer.as_ptr().cast::<PRINTER_INFO_4W>();
        (0..count as usize)
            .map(|index| {
                // SAFETY: EnumPrintersW wrote `count` records at the start of
                // the buffer, their strings after them.
                unsafe { read_wide((*infos.add(index)).pPrinterName) }
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    pub(crate) fn default_printer() -> Option<String> {
        let mut size = 0u32;
        // SAFETY: a null buffer asks for the size.
        unsafe { GetDefaultPrinterW(std::ptr::null_mut(), &raw mut size) };
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0u16; size as usize];
        // SAFETY: the buffer holds `size` units.
        let done = unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &raw mut size) };
        // SAFETY: on success the buffer holds a NUL-terminated name.
        (done != 0).then(|| unsafe { read_wide(buffer.as_ptr()) })
    }

    /// Prints the job; `progress` is told each sheet sent.
    pub(crate) fn print(
        job: &Job,
        pages: &[Arc<PageData>],
        progress: &dyn Fn(usize, usize),
    ) -> Result<(), String> {
        let driver = wide("WINSPOOL");
        let device = wide(&job.printer);
        // SAFETY: both strings are NUL-terminated; no DEVMODE: the
        // printer's own settings.
        let dc = unsafe {
            CreateDCW(
                driver.as_ptr(),
                device.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if dc.is_null() {
            return Err(format!(
                "the printer \"{}\" could not be opened",
                job.printer
            ));
        }
        let cap = |index: u32| {
            // SAFETY: `dc` is a printer DC until DeleteDC below.
            unsafe { GetDeviceCaps(dc, index.cast_signed()) }
        };
        let (dpi_x, dpi_y) = (cap(LOGPIXELSX).max(72), cap(LOGPIXELSY).max(72));
        let (paper_w, paper_h) = (cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT));
        let (offset_x, offset_y) = (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY));
        let (area_w, area_h) = (cap(HORZRES), cap(VERTRES));
        let title = wide(&job.title);
        let output = job
            .output
            .as_ref()
            .map(|path| wide(&path.display().to_string()));
        let info = DOCINFOW {
            cbSize: i32::try_from(std::mem::size_of::<DOCINFOW>()).unwrap_or(0),
            lpszDocName: title.as_ptr(),
            lpszOutput: output
                .as_ref()
                .map_or(std::ptr::null(), Vec::as_ptr),
            ..DOCINFOW::default()
        };
        // SAFETY: `info` and its title live past the call.
        if unsafe { StartDocW(dc, &raw const info) } <= 0 {
            // SAFETY: `dc` came from CreateDCW.
            unsafe { DeleteDC(dc) };
            return Err("the print job could not be started".to_owned());
        }
        let order = sheets(job);
        let total = order.len();
        let mut failure = None;
        for (done, index) in order.into_iter().enumerate() {
            let Some(page) = pages.get(index) else {
                continue;
            };
            progress(done, total);
            // The page at its real size on the paper, turned when the paper
            // runs the other way, shrunk only when it does not fit.
            let landscape_page = page.width > page.height;
            let landscape_paper = paper_w > paper_h;
            let turn = landscape_page != landscape_paper;
            let (inch_w, inch_h) = if turn {
                (page.height / 96.0, page.width / 96.0)
            } else {
                (page.width / 96.0, page.height / 96.0)
            };
            let (mut target_w, mut target_h) =
                (inch_w * f64::from(dpi_x), inch_h * f64::from(dpi_y));
            let fit = (f64::from(paper_w) / target_w)
                .min(f64::from(paper_h) / target_h)
                .min(1.0);
            target_w *= fit;
            target_h *= fit;
            // Pixels per page pixel, so the bitmap matches the target.
            #[allow(clippy::cast_possible_truncation)]
            let scale = (target_w / if turn { page.height } else { page.width }) as f32;
            let svg = strict_ooxml::paint_page(
                &page.placed,
                &strict_ooxml::RenderOptions::default().embed_fonts(false),
            );
            let Ok(bitmap) = crate::raster::rasterize(&svg, scale) else {
                failure = Some("a page could not be drawn".to_owned());
                break;
            };
            let (width, height, rgba) = if turn {
                turned(bitmap.width, bitmap.height, &bitmap.rgba)
            } else {
                (bitmap.width, bitmap.height, bitmap.rgba)
            };
            // GDI wants BGRA.
            let mut bgra = rgba;
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            let header = BITMAPINFOHEADER {
                biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
                biWidth: i32::try_from(width).unwrap_or(0),
                // Negative: rows top down.
                biHeight: -i32::try_from(height).unwrap_or(0),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..BITMAPINFOHEADER::default()
            };
            let bitmap_info = BITMAPINFO {
                bmiHeader: header,
                ..BITMAPINFO::default()
            };
            // Centred on the paper, in the printable area's coordinates.
            #[allow(clippy::cast_possible_truncation)]
            let (x, y) = (
                ((f64::from(paper_w) - target_w) / 2.0) as i32 - offset_x,
                ((f64::from(paper_h) - target_h) / 2.0) as i32 - offset_y,
            );
            let _ = (area_w, area_h);
            // SAFETY: the page is drawn between StartPage and EndPage on the
            // DC; the pixels and the header outlive the call.
            let drawn = unsafe {
                StartPage(dc);
                #[allow(clippy::cast_possible_truncation)]
                let lines = StretchDIBits(
                    dc,
                    x,
                    y,
                    target_w.round() as i32,
                    target_h.round() as i32,
                    0,
                    0,
                    i32::try_from(width).unwrap_or(0),
                    i32::try_from(height).unwrap_or(0),
                    bgra.as_ptr().cast(),
                    &raw const bitmap_info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                EndPage(dc);
                lines
            };
            if drawn == 0 {
                failure = Some("the printer refused a page".to_owned());
                break;
            }
        }
        // SAFETY: the document was started on this DC.
        unsafe {
            if failure.is_some() {
                AbortDoc(dc);
            } else {
                EndDoc(dc);
            }
            DeleteDC(dc);
        }
        progress(total, total);
        failure.map_or(Ok(()), Err)
    }

    /// RGBA pixels turned a quarter clockwise.
    fn turned(width: usize, height: usize, rgba: &[u8]) -> (usize, usize, Vec<u8>) {
        let mut out = vec![255u8; rgba.len()];
        for y in 0..height {
            for x in 0..width {
                let from = (y * width + x) * 4;
                // (x, y) goes to (height - 1 - y, x) in a height-wide image.
                let to = (x * height + (height - 1 - y)) * 4;
                if let (Some(source), Some(target)) =
                    (rgba.get(from..from + 4), out.get_mut(to..to + 4))
                {
                    target.copy_from_slice(source);
                }
            }
        }
        (height, width, out)
    }
}

#[cfg(not(windows))]
mod cups {
    /// The CUPS queues `lpstat -a` lists.
    pub(crate) fn printers() -> Vec<String> {
        std::process::Command::new("lpstat")
            .arg("-a")
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn default_printer() -> Option<String> {
        let output = std::process::Command::new("lpstat")
            .arg("-d")
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .split(':')
            .nth(1)
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    }
}

/// The machine's printers, the default first.
pub(crate) fn printers() -> Vec<String> {
    #[cfg(windows)]
    let (mut all, default) = (windows::printers(), windows::default_printer());
    #[cfg(not(windows))]
    let (mut all, default) = (cups::printers(), cups::default_printer());
    if let Some(default) = default {
        all.retain(|name| *name != default);
        all.insert(0, default);
    }
    all
}

/// Prints `job` on a thread of its own; `report` gets the sheets sent and
/// the outcome.
#[cfg(windows)]
pub(crate) fn start(
    job: Job,
    pages: Vec<Arc<PageData>>,
    report: impl Fn(Progress) + Send + 'static,
) {
    std::thread::spawn(move || {
        let result = windows::print(&job, &pages, &|done, total| {
            report(Progress::Sheets { done, total });
        });
        report(Progress::Done(result));
    });
}

/// Prints `pdf` (the document as PDF) with `lp`, on a thread of its own.
#[cfg(not(windows))]
pub(crate) fn start_pdf(
    job: Job,
    pdf: std::path::PathBuf,
    report: impl Fn(Progress) + Send + 'static,
) {
    std::thread::spawn(move || {
        let ranges: Vec<String> = job
            .pages
            .iter()
            .map(|page| (page + 1).to_string())
            .collect();
        let mut command = std::process::Command::new("lp");
        command
            .arg("-d")
            .arg(&job.printer)
            .arg("-n")
            .arg(job.copies.max(1).to_string())
            .arg("-o")
            .arg(format!("page-ranges={}", ranges.join(",")))
            .arg("-t")
            .arg(&job.title);
        if job.collate {
            command.arg("-o").arg("collate=true");
        }
        let result = command
            .arg(&pdf)
            .output()
            .map_err(|error| error.to_string())
            .and_then(|output| {
                output
                    .status
                    .success()
                    .then_some(())
                    .ok_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_owned())
            });
        report(Progress::Done(result));
    });
}

/// How a print job is going.
#[derive(Clone, Debug)]
pub(crate) enum Progress {
    Sheets { done: usize, total: usize },
    Done(Result<(), String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_read_as_word_reads_them() {
        assert_eq!(parse_range("1-3, 5", 10), Some(vec![0, 1, 2, 4]));
        assert_eq!(parse_range("8-", 10), Some(vec![7, 8, 9]));
        assert_eq!(parse_range("2-30", 4), Some(vec![1, 2, 3]));
        assert_eq!(parse_range("0", 4), None);
        assert_eq!(parse_range("x", 4), None);
        let job = Job {
            printer: String::new(),
            copies: 2,
            collate: true,
            pages: vec![0, 1],
            title: String::new(),
            output: None,
        };
        assert_eq!(sheets(&job), vec![0, 1, 0, 1]);
        assert_eq!(
            sheets(&Job {
                collate: false,
                ..job
            }),
            vec![0, 0, 1, 1]
        );
    }

    /// A document printed through GDI to Windows' PDF printer, into a file.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs the Microsoft Print to PDF printer"]
    fn a_page_prints_to_the_pdf_printer() {
        use crate::worker::{spawn, Reply, Request};
        let printer = "Microsoft Print to PDF";
        assert!(printers().iter().any(|p| p == printer), "{:?}", printers());
        let ctx = eframe::egui::Context::default();
        let (requests, replies) = spawn(
            crate::formats::Source::New,
            false,
            strict_ooxml::OpenControl::new(),
            ctx,
        );
        requests
            .send(Request::Type("Printed text".to_owned()))
            .unwrap();
        let shows = |page: &crate::worker::PageData| {
            page.placed
                .items
                .iter()
                .flat_map(strict_ooxml_render_svg::Item::leaves)
                .any(|item| matches!(item, strict_ooxml_render_svg::Item::Text(t) if t.text.contains("Printed")))
        };
        let pages = loop {
            if let Reply::Pages { pages, .. } = replies
                .recv_timeout(std::time::Duration::from_secs(60))
                .unwrap()
            {
                if pages.first().is_some_and(|page| shows(page)) {
                    break pages;
                }
            }
        };
        let out = std::env::temp_dir().join("mufoed-print-test.pdf");
        let _ = std::fs::remove_file(&out);
        let job = Job {
            printer: printer.to_owned(),
            copies: 1,
            collate: true,
            pages: vec![0],
            title: "mufoed print test".to_owned(),
            output: Some(out.clone()),
        };
        windows::print(&job, &pages, &|_, _| {}).expect("printed");
        let bytes = std::fs::read(&out).expect("the PDF");
        assert!(bytes.starts_with(b"%PDF"), "{} bytes", bytes.len());
        requests.send(Request::Shutdown).unwrap();
    }
}
