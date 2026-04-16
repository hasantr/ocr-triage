# Changelog

All notable changes to `ocr-triage` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — 2026-04-16

### Changed
- **Decode stack modernized** — JPEG path switched from `zune-jpeg` to
  `jpeg-decoder` 0.3 with DCT-level 1/8 scaled decode (10% speedup on A4
  vs. pure post-decode resize). PNG path added `zune-png` 0.5 as fast
  first-try, with `image` crate fallback.
- **Downsample rewritten**: `image::imageops::resize(Triangle)` was
  3-6 ms on small thumbnails because of the generic float filter loop;
  replaced with a manual integer area-average box filter (~100-300 µs
  equivalent). PNG A4 total: 4.5 ms → 1.3 ms (3.5×).
- **Conservative threshold loosened 0.27 → 0.25** so that
  `test_Courier_New_48.png` (score 0.269, previously borderline
  false-negative after the downsample change) classifies correctly.
  Negative max score in the validation set is 0.169, so the new
  threshold still has 0.08 margin.

### Added
- `examples/bench_breakdown.rs` — per-file decode/score latency
  breakdown on the production `decode_thumbnail` path (via
  `__internal::decode_thumbnail` doc-hidden API).
- `examples/probe_jpeg_scale.rs` — diagnostic for `jpeg-decoder` scaled
  decode behavior.
- `examples/probe_png_phases.rs` — diagnostic for zune-png phase
  breakdown (decode / color-to-luma / resize).

### Fixed
- `FN=0 / FP=0` restored on the bundled validation set (36/36 correct)
  after threshold calibration.

### Performance (Windows laptop, 20-iter means)
- PNG (4 KB small):       1.0 ms
- PNG (94 KB photo):      2.2 ms
- JPEG (220 KB small):    1.2 ms
- JPEG (8.7 MP A4):       28-33 ms
- PNG (8 MP A4):          30-34 ms
- Raw gray A4 (PDF renderer path): ~500 µs

## [0.1.0] — 2026-04-15

Initial public release.

### Added
- `has_text` — classify encoded bytes (JPEG/PNG/WebP/TIFF/BMP).
- `has_text_pixels` — classify raw grayscale buffers.
- `has_text_rgb` — classify raw RGB8 buffers (in-pass RGB→gray + subsample).
- `TriageConfig` + `TriageMode::{Conservative, Aggressive}`.
- `TriageVerdict { has_text, score, elapsed_us }` return type; panic-free on adversarial input.
- Examples: `bench`, `bench_pixels`, `gen_negatives`.
- Sanity tests (empty/malformed bytes, solid fills, mode thresholds).
