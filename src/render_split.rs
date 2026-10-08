use crate::error::Error;
use crate::render_worker::{RenderOptions, RenderedOutput, encode_jpeg};
use pdfium_render::prelude::*;
use std::path::Path;

struct PageGeometry {
    bounds: PdfRect,
    rotation: PdfPageRenderRotation,
    width: u32,
    height: u32,
}

pub fn render_page(
    document: &mut PdfDocument,
    output_dir: &Path,
    page_num: u32,
    opts: &RenderOptions,
    threshold: u64,
) -> Result<Vec<RenderedOutput>, Error> {
    let index = (page_num - 1) as u16;
    let geometry = read_page_geometry(document, index, opts.target_width)?;
    let heights = slice_heights(geometry.width, geometry.height, threshold);
    let split = u64::from(geometry.width) * u64::from(geometry.height) > threshold;
    let mut top = 0;
    heights
        .into_iter()
        .enumerate()
        .map(|(slice, height)| {
            if split {
                update_region_crop(document, index, &geometry, top, height)?;
            }
            let file = if split {
                format!("page-{page_num:04}-slice-{slice:04}.jpg")
            } else {
                format!("page-{page_num:04}.jpg")
            };
            write_region(
                document,
                index,
                &output_dir.join(&file),
                geometry.width,
                height,
                opts,
            )?;
            top += height;
            Ok(RenderedOutput {
                file,
                source_page: page_num,
                slice: slice as u32,
                width: geometry.width,
                height,
            })
        })
        .collect()
}

fn read_page_geometry(
    document: &PdfDocument,
    index: u16,
    width: u32,
) -> Result<PageGeometry, Error> {
    let page = document.pages().get(index).map_err(render_error)?;
    if width == 0 || width > i32::MAX as u32 {
        return Err(Error::Render(
            "target width must fit a positive PDFium bitmap dimension".into(),
        ));
    }
    let scale = width as f32 / page.width().value;
    let height = (page.height().value * scale).round();
    if !height.is_finite() || height < 1.0 || height as f64 > u32::MAX as f64 {
        return Err(Error::Render(
            "rendered page height is outside supported pixel range".into(),
        ));
    }
    Ok(PageGeometry {
        bounds: page.boundaries().bounding().map_err(render_error)?.bounds,
        rotation: page.rotation().map_err(render_error)?,
        width,
        height: height as u32,
    })
}

fn slice_heights(width: u32, height: u32, threshold: u64) -> Vec<u32> {
    if u64::from(width) * u64::from(height) <= threshold {
        return vec![height];
    }
    let nominal = ((u64::from(width) * 3).div_ceil(2)) as u32;
    let full = height / nominal;
    let tail = height % nominal;
    let mut heights = vec![nominal; full as usize];
    if tail == 0 {
        return heights;
    }
    if u64::from(tail) * 4 < u64::from(nominal)
        && let Some(last) = heights.last_mut()
    {
        *last += tail;
    } else {
        heights.push(tail);
    }
    heights
}

fn region_bounds(geometry: &PageGeometry, top: u32, height: u32) -> PdfRect {
    let bounds = geometry.bounds;
    let start = top as f32 / geometry.height as f32;
    let end = (top + height) as f32 / geometry.height as f32;
    let (left, bottom, right, upper) = (
        bounds.left().value,
        bounds.bottom().value,
        bounds.right().value,
        bounds.top().value,
    );
    match geometry.rotation {
        PdfPageRenderRotation::None => PdfRect::new_from_values(
            upper - bounds.height().value * end,
            left,
            upper - bounds.height().value * start,
            right,
        ),
        PdfPageRenderRotation::Degrees90 => PdfRect::new_from_values(
            bottom,
            left + bounds.width().value * start,
            upper,
            left + bounds.width().value * end,
        ),
        PdfPageRenderRotation::Degrees180 => PdfRect::new_from_values(
            bottom + bounds.height().value * start,
            left,
            bottom + bounds.height().value * end,
            right,
        ),
        PdfPageRenderRotation::Degrees270 => PdfRect::new_from_values(
            bottom,
            right - bounds.width().value * end,
            upper,
            right - bounds.width().value * start,
        ),
    }
}

fn update_region_crop(
    document: &mut PdfDocument,
    index: u16,
    geometry: &PageGeometry,
    top: u32,
    height: u32,
) -> Result<(), Error> {
    let mut page = document.pages().get(index).map_err(render_error)?;
    let bounds = region_bounds(geometry, top, height);
    page.boundaries_mut()
        .set(PdfPageBoundaryBoxType::Crop, bounds)
        .map_err(render_error)
}

fn write_region(
    document: &PdfDocument,
    index: u16,
    path: &Path,
    width: u32,
    height: u32,
    opts: &RenderOptions,
) -> Result<(), Error> {
    let height = i32::try_from(height)
        .map_err(|_| Error::Render("slice height exceeds PDFium bitmap dimension".into()))?;
    // Reload after the CropBox change; allocate only this physical region's bitmap.
    let page = document.pages().get(index).map_err(render_error)?;
    let config = PdfRenderConfig::new().set_fixed_size(width as i32, height);
    let bitmap = page.render_with_config(&config).map_err(render_error)?;
    let image = bitmap.as_image().into_rgb8();
    encode_jpeg(&image, path, opts.quality, opts.encoder)
}

fn render_error(error: PdfiumError) -> Error {
    Error::Render(error.to_string())
}
