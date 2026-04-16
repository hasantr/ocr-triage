# Changelog

All notable changes to `ocr-triage` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — Unreleased

Initial public release.

### Added
- `has_text` — classify encoded bytes (JPEG/PNG/WebP/TIFF/BMP).
- `has_text_pixels` — classify raw grayscale buffers.
- `has_text_rgb` — classify raw RGB8 buffers (in-pass RGB→gray + subsample).
- `TriageConfig` + `TriageMode::{Conservative, Aggressive}`.
- `TriageVerdict { has_text, score, elapsed_us }` return type; panic-free on adversarial input.
- Examples: `bench`, `bench_pixels`, `gen_negatives`.
- Sanity tests (empty/malformed bytes, solid fills, mode thresholds).
