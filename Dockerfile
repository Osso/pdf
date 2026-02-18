FROM rust:1.92-bookworm AS builder

WORKDIR /build
RUN curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz \
    | tar xz -C /tmp

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release && cp target/release/pdf /usr/local/bin/pdf

FROM debian:bookworm-slim
COPY --from=builder /usr/local/bin/pdf /usr/local/bin/pdf
COPY --from=builder /tmp/lib/libpdfium.so /usr/lib/libpdfium.so
RUN ldconfig
