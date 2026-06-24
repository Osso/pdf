#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod error;
mod info;
mod page_range;
mod pdfium_init;
mod render;
mod render_worker;

#[cfg(not(test))]
use clap::{Parser, Subcommand};
#[cfg(not(test))]
use render_worker::{BoxType, JpegEncoderType, RenderOptions};
#[cfg(not(test))]
use std::path::PathBuf;
#[cfg(not(test))]
use std::process::ExitCode;

#[cfg(not(test))]
#[derive(Parser)]
#[command(name = "pdf", about = "PDF rendering and info extraction using pdfium")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[cfg(not(test))]
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

        /// JPEG encoder backend
        #[arg(long, value_enum, default_value = "image")]
        encoder: JpegEncoderType,
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

        /// JPEG encoder backend
        #[arg(long, value_enum, default_value = "image")]
        encoder: JpegEncoderType,
    },
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn main() -> ExitCode {
    command_result_to_exit_code(dispatch(Cli::parse()))
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn dispatch(cli: Cli) -> Result<(), error::Error> {
    match cli.command {
        Commands::Info { pdf, all_pages } => info::run(&pdf, all_pages),
        command @ Commands::Render { .. } => run_render_command(command),
        command @ Commands::RenderWorker { .. } => run_render_worker_command(command),
    }
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn run_render_command(command: Commands) -> Result<(), error::Error> {
    let Commands::Render {
        pdf,
        output,
        target_width,
        quality,
        r#box,
        pages,
        workers,
        extract_images,
        encoder,
    } = command
    else {
        unreachable!("render command handler called with non-render command");
    };

    render::run(
        &pdf,
        &output,
        pages.as_deref(),
        workers,
        render_options(target_width, quality, r#box, extract_images, encoder),
    )
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn run_render_worker_command(command: Commands) -> Result<(), error::Error> {
    let Commands::RenderWorker {
        pdf,
        output,
        pages,
        target_width,
        quality,
        r#box,
        extract_images,
        encoder,
    } = command
    else {
        unreachable!("render-worker command handler called with non-worker command");
    };

    run_worker(
        &pdf,
        &output,
        &pages,
        render_options(target_width, quality, r#box, extract_images, encoder),
    )
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn render_options(
    target_width: u32,
    quality: u8,
    box_type: BoxType,
    extract_images: bool,
    encoder: JpegEncoderType,
) -> RenderOptions {
    RenderOptions {
        target_width,
        quality,
        box_type,
        extract_images,
        encoder,
    }
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn command_result_to_exit_code(result: Result<(), error::Error>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    }
}

#[cfg(not(test))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn run_worker(
    pdf: &std::path::Path,
    output: &std::path::Path,
    pages: &str,
    opts: RenderOptions,
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
    let result = render_worker::render_pages(pdf, output, &page_list, &opts)?;

    // Output result as JSON on stdout for parent to collect
    println!("{}", serde_json::to_string(&result).unwrap());

    Ok(())
}
