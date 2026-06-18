FROM rust:1.92-bookworm AS builder

WORKDIR /build
RUN apt-get update && apt-get install -y cmake nasm && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release && cp target/release/pdf /usr/local/bin/pdf

FROM debian:bookworm-slim
COPY --from=builder /usr/local/bin/pdf /usr/local/bin/pdf
COPY vendor/pdfium/*.so /usr/lib/
RUN ldconfig
