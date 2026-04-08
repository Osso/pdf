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
    let pdfium = load_pdfium()?;
    let mut document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| Error::PdfInvalid(format!("{}: {e}", pdf_path.display())))?;
    let render_config = PdfRenderConfig::new().set_target_width(opts.target_width as i32);

    let mut result = WorkerResult {
        pages_rendered: 0,
        pages_extracted: 0,
        errors: Vec::new(),
    };
    for &page_num in pages {
        process_page(
            &mut document,
            &render_config,
            output_dir,
            page_num,
            opts,
            &mut result,
        );
    }
    if result.pages_rendered + result.pages_extracted > 0 {
        eprintln!();
    }
    Ok(result)
}

fn process_page(
    document: &mut PdfDocument,
    render_config: &PdfRenderConfig,
    output_dir: &Path,
    page_num: u32,
    opts: &RenderOptions,
    result: &mut WorkerResult,
) {
    let page_index = (page_num - 1) as u16;
    if opts.box_type == BoxType::Bleed {
        apply_bleed_box(document, page_index);
    }
    let page = match document.pages().get(page_index) {
        Ok(page) => page,
        Err(e) => {
            result.errors.push(format!("page {page_num}: {e}"));
            return;
        }
    };

    if opts.extract_images
        && let Some(Ok(())) = try_extract_jpeg(&page, output_dir, page_num)
    {
        result.pages_extracted += 1;
        eprint!("\rExtracted page {page_num}");
        return;
    }

    match render_page_to_jpeg(
        &page,
        render_config,
        output_dir,
        page_num,
        opts.quality,
        opts.encoder,
    ) {
        Ok(()) => {
            result.pages_rendered += 1;
            eprint!("\rRendered page {page_num}");
        }
        Err(e) => {
            result.errors.push(format!("page {page_num}: {e}"));
        }
    }
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
fn try_extract_jpeg(page: &PdfPage, output_dir: &Path, page_num: u32) -> Option<Result<(), Error>> {
    let objects = page.objects();
    if objects.len() != 1 {
        return None;
    }

    let obj = objects.get(0).ok()?;
    let image_obj = obj.as_image_object()?;

    if !is_extractable_jpeg(image_obj) {
        return None;
    }
    // PDFs can embed a full two-page spread and use CropBox to show one half.
    // Skip extraction if the image aspect ratio doesn't match the page.
    if !image_matches_page_aspect(image_obj, page) {
        return None;
    }

    Some(write_raw_jpeg(image_obj, output_dir, page_num))
}

/// Check if the embedded image's aspect ratio roughly matches the page's.
///
/// A spread image (landscape) embedded in a portrait page means the page is
/// cropping to show only part of the image — raw extraction would be wrong.
fn image_matches_page_aspect(image_obj: &PdfPageImageObject, page: &PdfPage) -> bool {
    let (Ok(img_w), Ok(img_h)) = (image_obj.width(), image_obj.height()) else {
        return true;
    };
    let (pw, ph) = (page.width().value as f64, page.height().value as f64);
    aspect_ratios_match(img_w as f64, img_h as f64, pw, ph)
}

/// Returns true if two rectangles have similar aspect ratios (within 10%).
fn aspect_ratios_match(w1: f64, h1: f64, w2: f64, h2: f64) -> bool {
    if w1 == 0.0 || h1 == 0.0 || w2 == 0.0 || h2 == 0.0 {
        return true;
    }
    let ratio_diff = ((w1 / h1) - (w2 / h2)).abs() / (w2 / h2);
    ratio_diff < 0.1
}

fn is_extractable_jpeg(image_obj: &PdfPageImageObject) -> bool {
    let filters = image_obj.filters();
    if filters.len() != 1 {
        return false;
    }
    if !matches!(filters.get(0).ok(), Some(f) if f.name() == "DCTDecode") {
        return false;
    }
    // CMYK JPEGs extracted raw get their colors inverted when decoded as RGB.
    // Detect CMYK via raw JPEG SOF marker (covers DeviceCMYK and ICC-based CMYK).
    let data = match image_obj.get_raw_image_data() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if jpeg_is_cmyk(&data) {
        return false;
    }
    true
}

/// Check if raw JPEG data is CMYK by reading the SOF marker's component count.
/// CMYK JPEGs have 4 components; RGB has 3, grayscale has 1.
fn jpeg_is_cmyk(data: &[u8]) -> bool {
    // Scan for SOF0 (0xFFC0) or SOF2 (0xFFC2) marker
    let mut i = 0;
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        // SOF0 = 0xC0, SOF1 = 0xC1, SOF2 = 0xC2
        if matches!(marker, 0xC0..=0xC2) {
            // SOF layout: FF Cn [length:2] [precision:1] [height:2] [width:2] [components:1]
            if i + 9 < data.len() {
                let num_components = data[i + 9];
                return num_components == 4;
            }
            return false;
        }
        // Skip non-SOF markers (read length and advance)
        if marker == 0xD8 || marker == 0xD9 || marker == 0x00 {
            i += 2;
        } else if i + 3 < data.len() {
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            i += 2 + len;
        } else {
            break;
        }
    }
    false
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

    // Validate with libjpeg-turbo (same decoder as libvips). If it fails,
    // the `image` crate can still decode it — re-encode to produce a clean
    // JPEG that vips will accept.
    if turbojpeg::decompress(&data, turbojpeg::PixelFormat::RGB).is_err() {
        let img = image::load_from_memory_with_format(&data, image::ImageFormat::Jpeg)
            .map_err(|e| Error::Render(format!("JPEG decode failed: {e}")))?;
        let file = File::create(&path)?;
        let writer = BufWriter::new(file);
        img.into_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(writer, 100))
            .map_err(|e| Error::Render(format!("JPEG re-encode failed: {e}")))?;
        eprintln!("  (re-encoded corrupt JPEG for page {page_num})");
        return Ok(());
    }

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

fn encode_jpeg_image(image: &image::RgbImage, path: &Path, quality: u8) -> Result<(), Error> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let encoder = JpegEncoder::new_with_quality(writer, quality);
    image
        .write_with_encoder(encoder)
        .map_err(|e| Error::Render(format!("JPEG encode failed: {e}")))?;
    Ok(())
}

#[cfg(feature = "vips")]
fn encode_jpeg_vips(image: &image::RgbImage, path: &Path, quality: u8) -> Result<(), Error> {
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

    let path_str = path
        .to_str()
        .ok_or_else(|| Error::Render("non-UTF8 path".into()))?;
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
fn encode_jpeg_vips(_image: &image::RgbImage, _path: &Path, _quality: u8) -> Result<(), Error> {
    Err(Error::InvalidArgs(
        "--encoder vips requires building with --features vips".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_match_same_ratio() {
        assert!(aspect_ratios_match(2560.0, 3937.0, 477.0, 733.5));
    }

    #[test]
    fn aspect_match_slightly_different() {
        // 5% difference — within tolerance
        assert!(aspect_ratios_match(2560.0, 3937.0, 480.0, 733.5));
    }

    #[test]
    fn aspect_mismatch_spread_on_portrait_page() {
        // Spread image (8284x6400 landscape) on portrait page (477x733.5)
        assert!(!aspect_ratios_match(8284.0, 6400.0, 477.0, 733.5));
    }

    #[test]
    fn aspect_mismatch_landscape_image_on_portrait_page() {
        // Landscape image on portrait page — image wider than page expects
        assert!(!aspect_ratios_match(6400.0, 3937.0, 477.0, 733.5));
    }

    #[test]
    fn aspect_match_zero_dimensions_allow_extraction() {
        assert!(aspect_ratios_match(0.0, 100.0, 477.0, 733.5));
        assert!(aspect_ratios_match(100.0, 0.0, 477.0, 733.5));
        assert!(aspect_ratios_match(100.0, 100.0, 0.0, 733.5));
    }
}
