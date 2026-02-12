use crate::error::Error;
use crate::page_range::divide_pages;
use crate::pdfium_init::load_pdfium;
use crate::render_worker::BoxType;
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

#[derive(Serialize)]
struct RenderSummary {
    pages_rendered: u32,
    workers_used: u32,
    elapsed_secs: f64,
    output_dir: String,
}

struct RenderPlan {
    page_list: Vec<u32>,
    effective_workers: u32,
}

/// Orchestrate multi-process PDF rendering.
///
/// Reads page count, divides work across workers, spawns `render-worker` subprocesses.
pub fn run(
    pdf_path: &Path,
    output_dir: &Path,
    target_width: u32,
    quality: u8,
    box_type: BoxType,
    pages: Option<&str>,
    num_workers: u32,
) -> Result<(), Error> {
    let start = Instant::now();
    let plan = build_render_plan(pdf_path, pages, num_workers)?;
    std::fs::create_dir_all(output_dir)?;

    eprintln!(
        "Rendering {} pages from {} with {} workers",
        plan.page_list.len(),
        pdf_path.display(),
        plan.effective_workers
    );

    let (rendered, errors) = if plan.effective_workers <= 1 {
        run_single_process(pdf_path, output_dir, &plan.page_list, target_width, quality, box_type)?
    } else {
        run_multi_process(pdf_path, output_dir, &plan, target_width, quality, box_type)?
    };

    print_summary(rendered, plan.effective_workers, start, output_dir);
    check_errors(errors)
}

fn build_render_plan(pdf_path: &Path, pages: Option<&str>, num_workers: u32) -> Result<RenderPlan, Error> {
    let pdfium = load_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| Error::PdfInvalid(format!("{}: {e}", pdf_path.display())))?;
    let total_pages = document.pages().len() as u32;

    if total_pages == 0 {
        return Err(Error::PdfInvalid("PDF has no pages".into()));
    }

    let page_list = match pages {
        Some(range_str) => crate::page_range::parse_page_range(range_str, total_pages)?,
        None => (1..=total_pages).collect(),
    };

    let effective_workers = num_workers.min(page_list.len() as u32);
    Ok(RenderPlan { page_list, effective_workers })
}

fn run_single_process(
    pdf_path: &Path,
    output_dir: &Path,
    pages: &[u32],
    target_width: u32,
    quality: u8,
    box_type: BoxType,
) -> Result<(u32, Vec<String>), Error> {
    let result = crate::render_worker::render_pages(pdf_path, output_dir, pages, target_width, quality, box_type)?;
    Ok((result.pages_rendered, result.errors))
}

fn run_multi_process(
    pdf_path: &Path,
    output_dir: &Path,
    plan: &RenderPlan,
    target_width: u32,
    quality: u8,
    box_type: BoxType,
) -> Result<(u32, Vec<String>), Error> {
    let ranges = divide_pages(plan.page_list.len() as u32, plan.effective_workers);
    let current_exe = std::env::current_exe()?;

    let children: Vec<_> = ranges
        .iter()
        .map(|&(start, end)| {
            let worker_pages = &plan.page_list[(start as usize - 1)..=(end as usize - 1)];
            let pages_str = format_page_list(worker_pages);
            spawn_worker(&current_exe, pdf_path, output_dir, &pages_str, target_width, quality, box_type)
        })
        .collect::<Result<Vec<_>, _>>()?;

    collect_worker_results(children)
}

fn collect_worker_results(children: Vec<std::process::Child>) -> Result<(u32, Vec<String>), Error> {
    let mut total_rendered = 0u32;
    let mut all_errors = Vec::new();

    for (i, child) in children.into_iter().enumerate() {
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            all_errors.push(format!("worker {i}: exit {}: {stderr}", output.status));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(result) = serde_json::from_str::<WorkerOutput>(&stdout) {
            total_rendered += result.pages_rendered;
            all_errors.extend(result.errors);
        }
    }

    Ok((total_rendered, all_errors))
}

fn check_errors(errors: Vec<String>) -> Result<(), Error> {
    if errors.is_empty() {
        return Ok(());
    }
    for err in &errors {
        eprintln!("error: {err}");
    }
    Err(Error::Render(format!("{} errors during rendering", errors.len())))
}

fn spawn_worker(
    exe: &Path,
    pdf_path: &Path,
    output_dir: &Path,
    pages: &str,
    target_width: u32,
    quality: u8,
    box_type: BoxType,
) -> Result<std::process::Child, Error> {
    let box_str = match box_type {
        BoxType::Crop => "crop",
        BoxType::Bleed => "bleed",
    };

    Command::new(exe)
        .arg("render-worker")
        .arg(pdf_path)
        .arg("-o")
        .arg(output_dir)
        .arg("--pages")
        .arg(pages)
        .arg("--target-width")
        .arg(target_width.to_string())
        .arg("--quality")
        .arg(quality.to_string())
        .arg("--box")
        .arg(box_str)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(Error::Io)
}

fn format_page_list(pages: &[u32]) -> String {
    if pages.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    let mut range_start = pages[0];
    let mut range_end = pages[0];

    for &page in &pages[1..] {
        if page == range_end + 1 {
            range_end = page;
        } else {
            push_range(&mut parts, range_start, range_end);
            range_start = page;
            range_end = page;
        }
    }
    push_range(&mut parts, range_start, range_end);

    parts.join(",")
}

fn push_range(parts: &mut Vec<String>, start: u32, end: u32) {
    if start == end {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}-{end}"));
    }
}

fn print_summary(pages_rendered: u32, workers: u32, start: Instant, output_dir: &Path) {
    let elapsed = start.elapsed().as_secs_f64();
    let summary = RenderSummary {
        pages_rendered,
        workers_used: workers,
        elapsed_secs: (elapsed * 100.0).round() / 100.0,
        output_dir: output_dir.display().to_string(),
    };
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

#[derive(serde::Deserialize)]
struct WorkerOutput {
    pages_rendered: u32,
    #[serde(default)]
    errors: Vec<String>,
}
