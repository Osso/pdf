# pdf CLI

PDF-to-image rendering CLI using pdfium. Built for GlobalComix archive extraction pipeline.

## Project structure

```
src/
├── main.rs             # clap CLI, subcommand dispatch
├── render.rs           # render orchestrator (spawns workers, collects results)
├── render_worker.rs    # single-process page rendering (pdfium → JPEG)
├── info.rs             # info subcommand (page count + dimensions)
├── pdfium_init.rs      # pdfium library discovery and loading
├── page_range.rs       # page range parsing ("1-10", "3,5,7")
└── error.rs            # error types with exit codes
```

## Key design decisions

- **Multi-process, not multi-thread**: pdfium-render serializes all calls behind a mutex. Rayon/threads give zero rendering speedup. Worker processes each load the PDF independently.
- **Hidden `render-worker` subcommand**: Parent process divides pages and spawns `pdf render-worker` subprocesses. Workers output JSON on stdout for the parent to collect.
- **BleedBox support**: `--box bleed` reads BleedBox bounds and overrides CropBox in-memory before rendering. Document is never written back to disk.
- **No 2x oversample**: Unlike Ghostscript (which renders at 2x DPI then downscales), pdfium renders directly at target resolution with comparable quality.

## Dependencies

- `pdfium-render` 0.8 with `pdfium_7350` feature (matches pdfium 7428 from AUR `pdfium-binaries-bin`)
- `image` 0.25 for JPEG encoding with quality control
- `clap` 4 for CLI
- Requires `libpdfium.so` at runtime (system library or next to binary)

## Testing

```bash
cargo test                    # unit tests (page_range)
pdf info Tests/_data/test-release.pdf --all-pages
pdf render Tests/_data/test-release.pdf -o /tmp/test --workers 4
```

Test PDFs in `/syncthing/Sync/Projects/globalcomix/Comic Samples/`.

## PHP integration (future)

`framework/func/pdf.php` in the gc repo:
- Replace `pdfPageCount`/`pdfPageSize` with `pdf info`
- Replace `pdfToImages` (gs + parallel) with `pdf render`
- `pdfToImagesWithChunking` stays in PHP — call `pdf render --pages 1-10` per chunk to preserve heartbeat callbacks

## Docker deployment

Add to `gctasks_deps` stage in `docker/web/Dockerfile`:
```dockerfile
RUN curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz \
    | tar xz -C /usr/local lib/libpdfium.so --strip-components=1 && ldconfig
COPY --from=pdf-builder /build/target/release/pdf /usr/local/bin/pdf
```

CJK font packages must remain installed (pdfium uses system fonts).
