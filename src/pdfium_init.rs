#[cfg(not(test))]
use crate::error::Error;
#[cfg(not(test))]
use pdfium_render::prelude::*;

/// Initialize pdfium by searching for the library in standard locations.
///
/// Search order:
/// 1. PDFIUM_LIBRARY_PATH, when set
/// 2. Next to the current executable
/// 3. System library paths (LD_LIBRARY_PATH, /usr/lib, etc.)
#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn load_pdfium() -> Result<Pdfium, Error> {
    if let Some(bindings) = load_from_env()? {
        return Ok(Pdfium::new(bindings));
    }

    if let Some(bindings) = load_from_exe_dir()? {
        return Ok(Pdfium::new(bindings));
    }

    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| Error::PdfiumNotFound(e.to_string()))
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn load_from_env() -> Result<Option<Box<dyn PdfiumLibraryBindings>>, Error> {
    let Ok(path) = std::env::var("PDFIUM_LIBRARY_PATH") else {
        return Ok(None);
    };

    Pdfium::bind_to_library(&path)
        .map(Some)
        .map_err(|e| Error::PdfiumNotFound(format!("failed to load {path}: {e}")))
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn load_from_exe_dir() -> Result<Option<Box<dyn PdfiumLibraryBindings>>, Error> {
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
    else {
        return Ok(None);
    };

    let library_path = Pdfium::pdfium_platform_library_name_at_path(exe_dir.as_path());
    if !library_path.exists() {
        return Ok(None);
    }

    Pdfium::bind_to_library(&library_path)
        .map(Some)
        .map_err(|e| {
            Error::PdfiumNotFound(format!("failed to load {}: {e}", library_path.display()))
        })
}
