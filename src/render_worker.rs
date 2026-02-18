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

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum JpegEncoderType {
    /// Rust `image` crate (default)
    #[default]
    Image,
    /// libvips (requires --features vips)
    Vips,
}

#[derive(Serialize)]
pub struct WorkerResult {
    pub pages_rendered: u32,
    pub pages_extracted: u32,
    pub errors: Vec<String>,
}

/// Rendering options shared between single-process and multi-process modes.
pub struct RenderOptions {
    pub target_width: u32,
    pub quality: u8,
    pub box_type: BoxType,
    pub extract_images: bool,
    pub encoder: JpegEncoderType,
}

/// Render a range of pages from a PDF to JPEG files.
///
/// Pages are 1-based. Each page produces `page-NNNN.jpg` in `output_dir`.
/// When `extract_images` is true, pages containing a single JPEG image are
/// extracted directly without re-encoding.
pub fn render_pages(
    pdf_path: &Path,
    output_dir: &Path,
    pages: &[u32],
    opts: &RenderOptions,
) -> Result<WorkerResult, Error> {
    let RenderOptions { target_width, quality, box_type, extract_images, encoder } = opts;
    let pdfium = load_pdfium()?;

    let mut document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| Error::PdfInvalid(format!("{}: {e}", pdf_path.display())))?;

    let render_config = PdfRenderConfig::new().set_target_width(*target_width as i32);

    let mut pages_rendered = 0u32;
    let mut pages_extracted = 0u32;
    let mut errors = Vec::new();

    for &page_num in pages {
        let page_index = (page_num - 1) as u16;

        // Apply BleedBox as CropBox if requested
        if *box_type == BoxType::Bleed {
            apply_bleed_box(&mut document, page_index);
        }

        let page = match document.pages().get(page_index) {
            Ok(page) => page,
            Err(e) => {
                errors.push(format!("page {page_num}: {e}"));
                continue;
            }
        };

        // Try direct JPEG extraction first
        if *extract_images
            && let Some(result) = try_extract_jpeg(&page, output_dir, page_num)
        {
            match result {
                Ok(()) => {
                    pages_extracted += 1;
                    eprint!("\rExtracted page {page_num}");
                    continue;
                }
                Err(e) => {
                    errors.push(format!("page {page_num} extract: {e}"));
                    continue;
                }
            }
        }

        match render_page_to_jpeg(&page, &render_config, output_dir, page_num, *quality, *encoder) {
            Ok(()) => {
                pages_rendered += 1;
                eprint!("\rRendered page {page_num}");
            }
            Err(e) => {
                errors.push(format!("page {page_num}: {e}"));
            }
        }
    }
    if pages_rendered + pages_extracted > 0 {
        eprintln!();
    }

    Ok(WorkerResult {
        pages_rendered,
        pages_extracted,
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

/// Try to extract a raw JPEG from a page that contains a single image object.
///
/// Returns `None` if the page is not a single-image page or the image is not
/// stored as JPEG (DCTDecode filter). Returns `Some(Ok(()))` on successful
/// extraction, `Some(Err(..))` on I/O failure.
fn try_extract_jpeg(
    page: &PdfPage,
    output_dir: &Path,
    page_num: u32,
) -> Option<Result<(), Error>> {
    let objects = page.objects();
    if objects.len() != 1 {
        return None;
    }

    let obj = objects.get(0).ok()?;
    let image_obj = obj.as_image_object()?;

    // Check that the image has exactly one filter and it's DCTDecode (JPEG)
    if !is_jpeg_encoded(image_obj) {
        return None;
    }

    Some(write_raw_jpeg(image_obj, output_dir, page_num))
}

fn is_jpeg_encoded(image_obj: &PdfPageImageObject) -> bool {
    let filters = image_obj.filters();
    if filters.len() != 1 {
        return false;
    }
    matches!(filters.get(0).ok(), Some(f) if f.name() == "DCTDecode")
}

fn write_raw_jpeg(
    image_obj: &PdfPageImageObject,
    output_dir: &Path,
    page_num: u32,
) -> Result<(), Error> {
    let data = image_obj
        .get_raw_image_data()
        .map_err(|e| Error::Render(format!("extract image data: {e}")))?;
    if data.is_empty() {
        return Err(Error::Render("empty image data".into()));
    }

    let filename = format!("page-{page_num:04}.jpg");
    let path = output_dir.join(filename);
    std::fs::write(&path, &data)?;
    Ok(())
}

fn render_page_to_jpeg(
    page: &PdfPage,
    config: &PdfRenderConfig,
    output_dir: &Path,
    page_num: u32,
    quality: u8,
    encoder_type: JpegEncoderType,
) -> Result<(), Error> {
    let bitmap = page
        .render_with_config(config)
        .map_err(|e| Error::Render(format!("render failed: {e}")))?;

    let image = bitmap.as_image().into_rgb8();

    let filename = format!("page-{page_num:04}.jpg");
    let path = output_dir.join(filename);

    match encoder_type {
        JpegEncoderType::Image => encode_jpeg_image(&image, &path, quality),
        JpegEncoderType::Vips => encode_jpeg_vips(&image, &path, quality),
    }
}

fn encode_jpeg_image(
    image: &image::RgbImage,
    path: &Path,
    quality: u8,
) -> Result<(), Error> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let encoder = JpegEncoder::new_with_quality(writer, quality);
    image
        .write_with_encoder(encoder)
        .map_err(|e| Error::Render(format!("JPEG encode failed: {e}")))?;
    Ok(())
}

#[cfg(feature = "vips")]
fn encode_jpeg_vips(
    image: &image::RgbImage,
    path: &Path,
    quality: u8,
) -> Result<(), Error> {
    let (width, height) = image.dimensions();
    let raw = image.as_raw();

    let vips_image = libvips::VipsImage::new_from_memory(
        raw,
        width as i32,
        height as i32,
        3,
        libvips::ops::BandFormat::Uchar,
    )
    .map_err(|e| Error::Render(format!("vips from memory: {e}")))?;

    let path_str = path.to_str().ok_or_else(|| Error::Render("non-UTF8 path".into()))?;
    libvips::ops::jpegsave_with_opts(
        &vips_image,
        path_str,
        &libvips::ops::JpegsaveOptions {
            q: quality as i32,
            ..Default::default()
        },
    )
    .map_err(|e| Error::Render(format!("vips jpegsave: {e}")))?;

    Ok(())
}

#[cfg(not(feature = "vips"))]
fn encode_jpeg_vips(
    _image: &image::RgbImage,
    _path: &Path,
    _quality: u8,
) -> Result<(), Error> {
    Err(Error::InvalidArgs(
        "--encoder vips requires building with --features vips".into(),
    ))
}
