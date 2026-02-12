# pdf

PDF rendering and info extraction CLI using [pdfium](https://pdfium.googlesource.com/pdfium/). Replaces Ghostscript + GNU parallel + pdfinfo for PDF-to-image conversion pipelines.

## Install

Requires `libpdfium.so` on the system. On Arch Linux:

```bash
yay -S pdfium-binaries-bin
```

Build and install:

```bash
cargo build --release
cp target/release/pdf ~/bin/
```

## Usage

### Get PDF info

```bash
pdf info document.pdf
pdf info document.pdf --all-pages
```

Output:
```json
{
  "page_count": 50,
  "pages": [
    { "page": 1, "width_pt": 480.0, "height_pt": 738.38 }
  ]
}
```

### Render pages to JPEG

```bash
pdf render document.pdf -o /tmp/output
pdf render document.pdf -o /tmp/output --workers 4 --target-width 2560
pdf render document.pdf -o /tmp/output --pages 1-10 --quality 90
pdf render document.pdf -o /tmp/output --box bleed
pdf render document.pdf -o /tmp/output --extract-images
```

Outputs `page-NNNN.jpg` files. Progress on stderr, JSON summary on stdout:

```json
{
  "pages_rendered": 50,
  "workers_used": 4,
  "elapsed_secs": 6.5,
  "output_dir": "/tmp/output"
}
```

### Direct image extraction

With `--extract-images`, pages containing a single JPEG image are extracted directly from the PDF stream without re-rendering or re-encoding. This is common in comic PDFs where each page is a single image.

Detection criteria: page has exactly 1 object (an `Image`) with a `DCTDecode` filter. Pages that don't match fall back to normal rendering automatically.

Note: extracted images preserve their original dimensions and quality, bypassing `--target-width` and `--quality`.

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--target-width` | 2560 | Target width in pixels |
| `--quality` | 100 | JPEG quality (1-100) |
| `--box` | crop | Page boundary: `crop` or `bleed` |
| `--pages` | all | Page range: `1-10`, `3,5,7`, `1-5,8` |
| `--workers` | 4 | Number of worker processes |
| `--extract-images` | off | Extract raw JPEG from single-image pages |

## Architecture

pdfium serializes all rendering behind a mutex, so threads give zero speedup. Instead, the `render` command spawns N worker processes, each loading the PDF independently via pdfium:

```
pdf render input.pdf -o /tmp/out --workers 4
  ├─ pdf render-worker input.pdf --pages 1-13
  ├─ pdf render-worker input.pdf --pages 14-25
  ├─ pdf render-worker input.pdf --pages 26-38
  └─ pdf render-worker input.pdf --pages 39-50
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid arguments |
| 2 | PDF not found or invalid |
| 3 | pdfium library not found |
| 4 | Rendering error |
| 5 | I/O error |

## Benchmarks

Tested on 6 comic PDFs (8-300 pages, 3MB-344MB). pdfium with 4 workers vs single-process Ghostscript (2x oversample + downscale):

| PDF | Pages | Size | Ghostscript | pdfium 4w | Speedup |
|-----|-------|------|-------------|-----------|---------|
| GUN LEGS MAN V4 | 8 | 3MB | 31.3s | 1.2s | 25x |
| matching-our-answers | 22 | 21MB | 4.3s | 2.3s | 1.9x |
| 9798892150095 | 30 | 137MB | 61.1s | 30.2s | 2.0x |
| pesilat | 36 | 37MB | 11.4s | 4.3s | 2.7x |
| godslap | 50 | 177MB | 18.5s | 6.9s | 2.7x |
| 9781534339835 | 300 | 344MB | 88.2s | 35.9s | 2.5x |

Output dimensions match. pdfium produces ~30% smaller JPEG files at the same quality setting.

### With `--extract-images`

For PDFs where pages are single JPEG images, extraction skips rendering entirely:

| PDF | Pages | Render | Extract | Extracted | Speedup |
|-----|-------|--------|---------|-----------|---------|
| Legacy7_Issue2_highquality | 11 | 2.79s | 0.01s | 11/11 | 279x |
| pesilat | 36 | 4.07s | 0.03s | 36/36 | 136x |
| murinae-after-midnight-preview | 24 | 3.08s | 0.03s | 24/24 | 103x |
| Spawn3 | 1139 | 159.6s | 100.9s | 639/1139 | 1.6x |

Pages that aren't single JPEG images fall back to normal rendering (e.g. Spawn3 has 500 rendered + 639 extracted).

## pdfium version

The `pdfium_7350` feature flag is used to match the pdfium 7428 binary from AUR. To use a newer pdfium (7543+), change the feature in `Cargo.toml` to `pdfium_7543` or `pdfium_latest`.
