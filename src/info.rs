use crate::error::Error;
use crate::pdfium_init::load_pdfium;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct PdfInfo {
    pub page_count: u32,
    pub pages: Vec<PageInfo>,
}

#[derive(Serialize)]
pub struct PageInfo {
    pub page: u32,
    pub width_pt: f32,
    pub height_pt: f32,
}

pub fn run(pdf_path: &Path, all_pages: bool) -> Result<(), Error> {
    let pdfium = load_pdfium()?;

    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| Error::PdfInvalid(format!("{}: {e}", pdf_path.display())))?;

    let page_count = document.pages().len() as u32;

    let pages = if all_pages {
        document
            .pages()
            .iter()
            .enumerate()
            .map(|(i, page)| PageInfo {
                page: i as u32 + 1,
                width_pt: page.width().value,
                height_pt: page.height().value,
            })
            .collect()
    } else {
        // Just first page by default
        let first = document.pages().first().map_err(|_| {
            Error::PdfInvalid("PDF has no pages".into())
        })?;
        vec![PageInfo {
            page: 1,
            width_pt: first.width().value,
            height_pt: first.height().value,
        }]
    };

    let info = PdfInfo { page_count, pages };
    println!("{}", serde_json::to_string_pretty(&info).unwrap());

    Ok(())
}
