mod error;
mod info;
mod page_range;
mod pdfium_init;
mod render;
mod render_worker;

use clap::{Parser, Subcommand};
use render_worker::BoxType;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "pdf", about = "PDF rendering and info extraction using pdfium")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Output PDF page count and dimensions as JSON
    Info {
        /// Path to the PDF file
        pdf: PathBuf,

        /// Include dimensions for all pages (default: first page only)
        #[arg(long)]
        all_pages: bool,
    },

    /// Render PDF pages to JPEG images
    Render {
        /// Path to the PDF file
        pdf: PathBuf,

        /// Output directory for JPEG files
        #[arg(short, long)]
        output: PathBuf,

        /// Target width in pixels
        #[arg(long, default_value = "2560")]
        target_width: u32,

        /// JPEG quality (1-100)
        #[arg(long, default_value = "100")]
        quality: u8,

        /// Page boundary box to use for rendering
        #[arg(long, rename_all = "lower", value_enum, default_value = "crop")]
        r#box: BoxType,

        /// Page range to render (e.g. "1-10", "3,5,7")
        #[arg(long)]
        pages: Option<String>,

        /// Number of worker processes
        #[arg(long, default_value = "4")]
        workers: u32,

        /// Extract raw JPEG from single-image pages instead of re-rendering
        #[arg(long)]
        extract_images: bool,
    },

    /// Internal: render assigned pages in a single process
    #[command(hide = true)]
    RenderWorker {
        pdf: PathBuf,

        #[arg(short, long)]
        output: PathBuf,

        #[arg(long)]
        pages: String,

        #[arg(long, default_value = "2560")]
        target_width: u32,

        #[arg(long, default_value = "100")]
        quality: u8,

        #[arg(long, rename_all = "lower", value_enum, default_value = "crop")]
        r#box: BoxType,

        #[arg(long)]
        extract_images: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Info { pdf, all_pages } => info::run(&pdf, all_pages),
        Commands::Render {
            pdf,
            output,
            target_width,
            quality,
            r#box,
            pages,
            workers,
            extract_images,
        } => render::run(&pdf, &output, target_width, quality, r#box, pages.as_deref(), workers, extract_images),
        Commands::RenderWorker {
            pdf,
            output,
            pages,
            target_width,
            quality,
            r#box,
            extract_images,
        } => run_worker(&pdf, &output, &pages, target_width, quality, r#box, extract_images),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    }
}

fn run_worker(
    pdf: &std::path::Path,
    output: &std::path::Path,
    pages: &str,
    target_width: u32,
    quality: u8,
    box_type: BoxType,
    extract_images: bool,
) -> Result<(), error::Error> {
    // Worker needs to know max page count for range parsing; open PDF to check
    let pdfium = pdfium_init::load_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(pdf, None)
        .map_err(|e| error::Error::PdfInvalid(format!("{}: {e}", pdf.display())))?;
    let max_page = document.pages().len() as u32;
    drop(document);
    drop(pdfium);

    let page_list = page_range::parse_page_range(pages, max_page)?;
    let result = render_worker::render_pages(pdf, output, &page_list, target_width, quality, box_type, extract_images)?;

    // Output result as JSON on stdout for parent to collect
    println!("{}", serde_json::to_string(&result).unwrap());

    Ok(())
}
