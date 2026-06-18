use image::GenericImageView;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FOUR_GIB: u64 = 0x1_0000_0000;
const PATCHED_PDFIUM: &str = "/home/osso/Repos/pdfium/pdfium/out/Shared/libpdfium.so";

#[test]
fn renders_page_with_xref_stream_offsets_above_4gb() {
    assert!(
        Path::new(PATCHED_PDFIUM).exists(),
        "patched pdfium library missing at {PATCHED_PDFIUM}; build /home/osso/Repos/pdfium/pdfium/out/Shared/libpdfium.so first"
    );

    let temp_dir = create_temp_dir("pdf-large-xref");
    let pdf_path = temp_dir.join("large-xref.pdf");
    let output_dir = temp_dir.join("out");
    fs::create_dir(&output_dir).unwrap();

    write_sparse_pdf_with_large_xref_offset(&pdf_path);

    let output = Command::new(env!("CARGO_BIN_EXE_pdf"))
        .arg("render")
        .arg(&pdf_path)
        .arg("-o")
        .arg(&output_dir)
        .arg("--pages")
        .arg("1")
        .arg("--target-width")
        .arg("64")
        .arg("--workers")
        .arg("1")
        .env("PDFIUM_LIBRARY_PATH", PATCHED_PDFIUM)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "pdf render failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rendered_page = output_dir.join("page-0001.jpg");
    let average_luma = average_luma(&rendered_page);
    assert!(
        average_luma < 80.0,
        "expected black stream from >4GiB offset, got average luma {average_luma:.2}: {}",
        rendered_page.display()
    );
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir(&path).unwrap();
    path
}

fn write_sparse_pdf_with_large_xref_offset(path: &Path) {
    let mut file = File::create(path).unwrap();

    write_bytes(&mut file, b"%PDF-1.7\n");

    let object_offsets = write_page_objects(&mut file);
    let content_offset = write_duplicate_content_streams(&mut file);
    write_xref_stream(&mut file, &object_offsets, content_offset);
}

fn write_page_objects(file: &mut File) -> Vec<u64> {
    vec![
        write_object(file, 1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        write_object(file, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        write_object(
            file,
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>",
        ),
    ]
}

fn write_duplicate_content_streams(file: &mut File) -> u64 {
    let truncated_content_offset = write_stream_object(file, 4, b"1 1 1 rg\n0 0 200 200 re\nf\n");
    let large_content_offset = FOUR_GIB + truncated_content_offset;

    file.seek(SeekFrom::Start(large_content_offset)).unwrap();
    write_stream_object(file, 4, b"0 0 0 rg\n0 0 200 200 re\nf\n")
}

fn write_xref_stream(file: &mut File, object_offsets: &[u64], content_offset: u64) {
    let xref_offset = file.stream_position().unwrap();
    let xref_stream = build_xref_stream(&[
        XrefEntry::Free,
        XrefEntry::Normal(object_offsets[0]),
        XrefEntry::Normal(object_offsets[1]),
        XrefEntry::Normal(object_offsets[2]),
        XrefEntry::Normal(content_offset),
        XrefEntry::Normal(xref_offset),
    ]);

    write_bytes(
        file,
        format!(
            "5 0 obj\n<< /Type /XRef /Size 6 /Root 1 0 R /W [1 8 2] /Index [0 6] /Length {} >>\nstream\n",
            xref_stream.len()
        )
        .as_bytes(),
    );
    write_bytes(file, &xref_stream);
    write_bytes(
        file,
        format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
}

fn write_object(file: &mut File, number: u32, body: &[u8]) -> u64 {
    let offset = file.stream_position().unwrap();
    write_bytes(file, format!("{number} 0 obj\n").as_bytes());
    write_bytes(file, body);
    write_bytes(file, b"\nendobj\n");
    offset
}

fn write_stream_object(file: &mut File, number: u32, stream: &[u8]) -> u64 {
    let offset = file.stream_position().unwrap();
    write_bytes(
        file,
        format!("{number} 0 obj\n<< /Length {} >>\nstream\n", stream.len()).as_bytes(),
    );
    write_bytes(file, stream);
    write_bytes(file, b"endstream\nendobj\n");
    offset
}

fn write_bytes(file: &mut File, bytes: &[u8]) {
    file.write_all(bytes).unwrap();
}

enum XrefEntry {
    Free,
    Normal(u64),
}

fn build_xref_stream(entries: &[XrefEntry]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entries.len() * 11);
    for entry in entries {
        match entry {
            XrefEntry::Free => {
                bytes.push(0);
                bytes.extend_from_slice(&0u64.to_be_bytes());
                bytes.extend_from_slice(&u16::MAX.to_be_bytes());
            }
            XrefEntry::Normal(offset) => {
                bytes.push(1);
                bytes.extend_from_slice(&offset.to_be_bytes());
                bytes.extend_from_slice(&0u16.to_be_bytes());
            }
        }
    }
    bytes
}

fn average_luma(path: &Path) -> f64 {
    let image = image::open(path).unwrap();
    let total_luma: u64 = image
        .pixels()
        .map(|(_, _, pixel)| {
            let [red, green, blue, _alpha] = pixel.0;
            u64::from(red) + u64::from(green) + u64::from(blue)
        })
        .sum();

    total_luma as f64 / (image.width() as f64 * image.height() as f64 * 3.0)
}
