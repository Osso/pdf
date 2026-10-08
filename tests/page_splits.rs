use image::{GenericImageView, RgbImage};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_dir() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("pdf-splits-{}-{id}", std::process::id()));
    fs::create_dir(&path).unwrap();
    path
}

fn write_pdf(path: &Path, pages: &[(u32, u32, u32)]) {
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes(),
    ];
    for (index, &(width, height, rotation)) in pages.iter().enumerate() {
        let mut bands = String::new();
        for y in (0..height).step_by(25) {
            let color = match (y / 25) % 3 {
                0 => "1 0 0",
                1 => "0 1 0",
                _ => "0 0 1",
            };
            bands.push_str(&format!("{color} rg 20 {} {width} 25 re f\n", 30 + y));
        }
        let page = 3 + index * 2;
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /CropBox [20 30 {} {}] /BleedBox [10 20 {} {}] /Rotate {rotation} /Resources << >> /Contents {} 0 R >>", width+40, height+60, width+20, height+30, width+30, height+40, page+1).into_bytes());
        objects
            .push(format!("<< /Length {} >>\nstream\n{bands}endstream", bands.len()).into_bytes());
    }
    write_objects(path, objects);
}

fn write_objects(path: &Path, objects: Vec<Vec<u8>>) {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let offsets: Vec<_> = objects
        .iter()
        .enumerate()
        .map(|(i, object)| {
            let offset = pdf.len();
            pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
            offset
        })
        .collect();
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    fs::write(path, pdf).unwrap();
}

fn render(pdf: &Path, out: &Path, extra: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_pdf"))
        .arg("render")
        .arg(pdf)
        .arg("-o")
        .arg(out)
        .args(["--target-width", "100", "--quality", "100"])
        .args(extra)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_pixels_match_slices(out: &Path, baseline: &Path, outputs: &[serde_json::Value]) {
    let mut tops = std::collections::HashMap::<u64, u32>::new();
    for entry in outputs {
        let page = entry["source_page"].as_u64().unwrap();
        let image = image::open(out.join(entry["file"].as_str().unwrap())).unwrap();
        let original = image::open(baseline.join(format!("page-{page:04}.jpg")))
            .unwrap()
            .into_rgb8();
        assert_eq!(
            image.dimensions(),
            (
                entry["width"].as_u64().unwrap() as u32,
                entry["height"].as_u64().unwrap() as u32
            )
        );
        let top = tops.entry(page).or_default();
        compare_pixels(&image.into_rgb8(), &original, *top);
        *top += entry["height"].as_u64().unwrap() as u32;
        assert!(*top <= original.height());
    }
    for (page, height) in tops {
        assert_eq!(
            height,
            image::open(baseline.join(format!("page-{page:04}.jpg")))
                .unwrap()
                .height()
        );
    }
}

fn compare_pixels(slice: &RgbImage, original: &RgbImage, top: u32) {
    for y in (5..slice.height().saturating_sub(5)).step_by(10) {
        for x in (5..slice.width().saturating_sub(5)).step_by(10) {
            let a = slice.get_pixel(x, y).0;
            let b = original.get_pixel(x, top + y).0;
            for channel in 0..3 {
                assert!(
                    a[channel].abs_diff(b[channel]) <= 12,
                    "pixel {x},{}: {a:?} != {b:?}",
                    top + y
                );
            }
        }
    }
}

#[test]
fn threshold_tail_and_four_worker_output_order() {
    let root = temp_dir();
    let pdf = root.join("bands.pdf");
    write_pdf(
        &pdf,
        &[(100, 300, 0), (100, 301, 0), (100, 337, 0), (100, 338, 0)],
    );
    let baseline = root.join("normal");
    let normal = render(&pdf, &baseline, &["--workers", "1"]);
    assert!(normal.get("outputs").is_none());
    let out = root.join("split");
    let summary = render(
        &pdf,
        &out,
        &["--workers", "4", "--split-over-pixels", "30000"],
    );
    let outputs = summary["outputs"].as_array().unwrap();
    let actual: Vec<_> = outputs
        .iter()
        .map(|e| {
            (
                e["source_page"].as_u64().unwrap(),
                e["slice"].as_u64().unwrap(),
                e["height"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            (1, 0, 300),
            (2, 0, 150),
            (2, 1, 151),
            (3, 0, 150),
            (3, 1, 187),
            (4, 0, 150),
            (4, 1, 150),
            (4, 2, 38)
        ]
    );
    assert_eq!(summary["pages_rendered"], 8);
    assert_eq!(summary["workers_used"], 4);
    assert_eq!(outputs[0]["file"], "page-0001.jpg");
    assert_eq!(outputs[1]["file"], "page-0002-slice-0000.jpg");
    assert_pixels_match_slices(&out, &baseline, outputs);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cropped_rotated_and_bleed_regions_match_full_page() {
    let root = temp_dir();
    let pdf = root.join("rotation.pdf");
    write_pdf(
        &pdf,
        &[
            (100, 425, 0),
            (425, 100, 90),
            (100, 425, 180),
            (425, 100, 270),
        ],
    );
    for box_type in ["crop", "bleed"] {
        let baseline = root.join(format!("normal-{box_type}"));
        render(&pdf, &baseline, &["--workers", "1", "--box", box_type]);
        let out = root.join(format!("split-{box_type}"));
        let summary = render(
            &pdf,
            &out,
            &[
                "--workers",
                "4",
                "--box",
                box_type,
                "--split-over-pixels",
                "30000",
            ],
        );
        assert_pixels_match_slices(&out, &baseline, summary["outputs"].as_array().unwrap());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn single_jpeg_extraction_cannot_bypass_splitting() {
    let root = temp_dir();
    let pdf = root.join("image.pdf");
    let image = RgbImage::from_fn(100, 425, |_, y| {
        if y < 150 {
            image::Rgb([255, 0, 0])
        } else {
            image::Rgb([0, 0, 255])
        }
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .encode_image(&image)
        .unwrap();
    let content = b"q 100 0 0 425 0 0 cm /Im0 Do Q\n";
    let mut image_obj = format!("<< /Type /XObject /Subtype /Image /Width 100 /Height 425 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",jpeg.len()).into_bytes();
    image_obj.extend(jpeg);
    image_obj.extend(b"\nendstream");
    write_objects(&pdf,vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 425] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>".to_vec(),format!("<< /Length {} >>\nstream\n{}endstream",content.len(),String::from_utf8_lossy(content)).into_bytes(),image_obj]);
    let baseline = root.join("baseline");
    render(&pdf, &baseline, &["--workers", "1"]);
    let out = root.join("out");
    let summary = render(
        &pdf,
        &out,
        &[
            "--workers",
            "1",
            "--extract-images",
            "--split-over-pixels",
            "30000",
        ],
    );
    assert_eq!(summary["pages_rendered"], 3);
    assert!(summary.get("pages_extracted").is_none());
    assert_pixels_match_slices(&out, &baseline, summary["outputs"].as_array().unwrap());
    fs::remove_dir_all(root).unwrap();
}
