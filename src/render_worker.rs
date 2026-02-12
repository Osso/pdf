use crate::error::Error;
use crate::pdfium_init::load_pdfium;
use image::codecs::jpeg::JpegEncoder;
use pdfium_render::prelude::*;
use serde::Serialize;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BoxType {
    Crop,
    Bleed,
}

#[derive(Serialize)]
pub struct WorkerResult {
    pub pages_rendered: u32,
    pub errors: Vec<String>,
}

/// Render a range of pages from a PDF to JPEG files.
///
/// Pages are 1-based. Each page produces `page-NNNN.jpg` in `output_dir`.
pub fn render_pages(
    pdf_path: &Path,
    output_dir: &Path,
    pages: &[u32],
    target_width: u32,
    quality: u8,
    box_type: BoxType,
) -> Result<WorkerResult, Error> {
    let pdfium = load_pdfium()?;

    let mut document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| Error::PdfInvalid(format!("{}: {e}", pdf_path.display())))?;

    let render_config = PdfRenderConfig::new().set_target_width(target_width as i32);

    let mut pages_rendered = 0u32;
    let mut errors = Vec::new();

    for &page_num in pages {
        let page_index = (page_num - 1) as u16;

        // Apply BleedBox as CropBox if requested
        if box_type == BoxType::Bleed {
            apply_bleed_box(&mut document, page_index);
        }

        let page = match document.pages().get(page_index) {
            Ok(page) => page,
            Err(e) => {
                errors.push(format!("page {page_num}: {e}"));
                continue;
            }
        };

        match render_page_to_jpeg(&page, &render_config, output_dir, page_num, quality) {
            Ok(()) => {
                pages_rendered += 1;
                eprint!("\rRendered page {page_num}");
            }
            Err(e) => {
                errors.push(format!("page {page_num}: {e}"));
            }
        }
    }
    if pages_rendered > 0 {
        eprintln!();
    }

    Ok(WorkerResult {
        pages_rendered,
        errors,
    })
}

fn apply_bleed_box(document: &mut PdfDocument, page_index: u16) {
    let Ok(mut page) = document.pages().get(page_index) else {
        return;
    };

    let bleed_rect = match page.boundaries().bleed() {
        Ok(b) => b.bounds,
        Err(_) => return, // No BleedBox defined, use default CropBox
    };

    // Override CropBox with BleedBox bounds
    let _ = page
        .boundaries_mut()
        .set(PdfPageBoundaryBoxType::Crop, bleed_rect);
}

fn render_page_to_jpeg(
    page: &PdfPage,
    config: &PdfRenderConfig,
    output_dir: &Path,
    page_num: u32,
    quality: u8,
) -> Result<(), Error> {
    let bitmap = page
        .render_with_config(config)
        .map_err(|e| Error::Render(format!("render failed: {e}")))?;

    let image = bitmap.as_image().into_rgb8();

    let filename = format!("page-{page_num:04}.jpg");
    let path = output_dir.join(filename);
    let file = File::create(&path)?;
    let writer = BufWriter::new(file);

    let encoder = JpegEncoder::new_with_quality(writer, quality);
    image
        .write_with_encoder(encoder)
        .map_err(|e| Error::Render(format!("JPEG encode failed: {e}")))?;

    Ok(())
}
