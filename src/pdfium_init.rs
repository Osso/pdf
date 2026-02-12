use crate::error::Error;
use pdfium_render::prelude::*;

/// Initialize pdfium by searching for the library in standard locations.
///
/// Search order:
/// 1. Next to the current executable
/// 2. System library paths (LD_LIBRARY_PATH, /usr/lib, etc.)
pub fn load_pdfium() -> Result<Pdfium, Error> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let bindings = exe_dir
        .and_then(|dir| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                dir.as_path(),
            ))
            .ok()
        })
        .map(Ok)
        .unwrap_or_else(|| {
            Pdfium::bind_to_system_library()
        })
        .map_err(|e| Error::PdfiumNotFound(e.to_string()))?;

    Ok(Pdfium::new(bindings))
}
