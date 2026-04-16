# ocr-triage

[![Crates.io](https://img.shields.io/crates/v/ocr-triage.svg)](https://crates.io/crates/ocr-triage)
[![Documentation](https://docs.rs/ocr-triage/badge.svg)](https://docs.rs/ocr-triage)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![CI](https://github.com/hasantr/ocr-triage/actions/workflows/ci.yml/badge.svg)](https://github.com/hasantr/ocr-triage/actions)

**Does this image contain text?** A sub-millisecond binary classifier, written in pure Rust. Zero ML weights, zero C dependencies, language-agnostic, format-agnostic.

Türkçe: [README.tr.md](README.tr.md)

---

## Why

OCR engines — Tesseract, PaddleOCR, RapidOCR — take **500–2000 ms per image**. Most images fed to them have no text at all: logos, photos, gradients, solid fills, icons. In a 100k-image batch, that wasted time is hours.

`ocr-triage` answers one question before you spend that time:

> **Should I bother running OCR on this?**

Typical speedup on a mixed batch where ~60% of images are text-free:

| Path | Time per text-free image |
|------|--------------------------|
| Tesseract directly | 500–2000 ms |
| `ocr-triage` skip + Tesseract on rest | **~300 µs** decision |

## Features

- **Fast.** ~300 µs on raw-RGB pages (2480×3508, A4 at 300 dpi). 1–20 ms on encoded bytes (JPEG/PNG/WebP, depending on decoder work).
- **Language-agnostic.** No text recognition, no language models — only geometric signals. Works on Latin, Cyrillic, Arabic, Hebrew, CJK, Devanagari, and anything else built from horizontal ink strokes.
- **Format-agnostic.** Accepts encoded bytes (JPEG/PNG/WebP/TIFF/BMP) or raw pixel buffers (grayscale/RGB). The latter path skips decode entirely — ideal when you already have pixels in hand (PDF page renderer, image pipeline).
- **Polarity-invariant.** Dark text on light background, light text on dark background — same verdict. Uses Otsu thresholding per image.
- **Zero model weights.** ~220 lines of Rust. The whole library is auditable in an afternoon.
- **Two tuning modes.** `Conservative` (FN=0 target, safe default) and `Aggressive` (FP minimizer, for CPU-starved batch jobs).

## Quick start

```toml
[dependencies]
ocr-triage = "0.1"
```

### Encoded bytes (JPEG/PNG/WebP/TIFF/BMP)

```rust
use ocr_triage::has_text;

let bytes = std::fs::read("scanned_page.jpg")?;
let verdict = has_text(&bytes);

if verdict.has_text {
    run_tesseract(&bytes)?;        // spend the 500–2000 ms knowingly
} else {
    // Skip — 300 µs decision, no OCR cost.
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Raw pixel buffers (no decode)

Use this when your upstream already decoded the image — e.g., a PDF page rasterized to RGB in memory. Avoids the PNG encode → decode round-trip entirely.

```rust
use ocr_triage::{has_text_pixels, has_text_rgb};

// Grayscale: len == width * height
let v = has_text_pixels(&gray, width, height);

// RGB8: len == width * height * 3
let v = has_text_rgb(&rgb, width, height);
```

### Custom configuration

```rust
use ocr_triage::{has_text_with_config, TriageConfig, TriageMode};

// Default: Conservative (threshold 0.27, target FN = 0).
let v = has_text_with_config(&bytes, &TriageConfig::conservative());

// Aggressive: threshold 0.40, lower FP rate, some FN tolerated.
let v = has_text_with_config(&bytes, &TriageConfig::from_mode(TriageMode::Aggressive));

// Fully manual.
let cfg = TriageConfig {
    threshold: 0.33,
    thumbnail_short_edge: 192,
};
let v = has_text_with_config(&bytes, &cfg);
```

## Output

```rust
pub struct TriageVerdict {
    pub has_text: bool,
    pub score: f32,      // 0.0 .. ~1.0
    pub elapsed_us: u32, // wall-clock microseconds
}
```

Malformed bytes, empty inputs, or decoder failures all return `has_text = false` without panicking — safe to call on untrusted data.

## How it works

The pipeline runs on a downsampled grayscale thumbnail (short edge 256 px by default):

1. **Otsu binarization** — image-adaptive threshold yields a foreground/background mask. The smaller class is always labeled foreground, so dark-on-light and light-on-dark collapse to the same representation.
2. **Horizontal edge density** — count 0↔1 transitions across each row of the binary image. Text has many horizontal strokes per row; photos and gradients do not.
3. **Row-projection variance** — compute foreground coverage per row, then its variance. Text alternates dense (glyph rows) and sparse (interline) rows → high variance. Uniform noise has low variance.
4. **Global + 2×2 regional TOP-K** — take the max of the whole-image score and the top-cell score of a 2×2 grid. Prevents a text-filled quadrant from being diluted by a large uniform background.
5. **Coverage gating** — multiply by a weight based on foreground fraction. Text typically lives in 3–30% coverage; 45–55% is almost always Otsu-split noise; very low is an empty page.

Final score is the **geometric mean** of edge density and projection variance (√(edge × variance)), then scaled by the coverage weight. Geometric mean is deliberate: a single high signal cannot trigger the verdict on its own.

## Accuracy

On a mixed synthetic + real-world validation set (20 text + 14 non-text images):

| Mode | Accuracy | FN rate | FP rate |
|------|----------|---------|---------|
| Conservative (default) | 34/34 (100%) | 0% | 0% |
| Aggressive | 31/34 (91%) | 15% | 0% |

Real-world smoke test on a mixed DOCX (7 embedded images): 6 true positives, 1 borderline FP (dense app-icon tile). FN = 0.

> Bring your own fixtures — drop text images into `testset/positive/` and text-free images into `testset/negative/`, then run `cargo run --release --example bench`. The `gen_negatives` example populates `testset/negative/` with synthetic solid-color/gradient/logo shapes to get you started.

**Conservative mode targets FN = 0.** If it's wrong, it's wrong in the "unnecessary Tesseract call" direction — no text is ever lost.

## Edge cases honest list

- **Vertical CJK writing.** Algorithm assumes horizontal rows; traditional top-to-bottom vertical layouts may score low. Horizontal-written CJK works fine.
- **Handwriting / calligraphy.** Not widely tested. Expect lower accuracy on free-form scripts.
- **Icon grids.** Dense app-icon pages can score high (edge-heavy) — acceptable false positive, since they're ambiguous even to humans.
- **Heavy JPEG noise.** On very noisy low-res JPEGs, edge count can inflate. Conservative mode still catches text but may pass some noise through.

For production use on an unfamiliar corpus, run `cargo run --release --example bench -- --positive your/text/dir --negative your/no-text/dir` and inspect the borderline scores before committing to a threshold.

## Benchmarks

Examples live in `examples/`:

```bash
# Raw pixel micro-benchmark (has_text_pixels, has_text_rgb)
cargo run --release --example bench_pixels

# Dataset accuracy + latency report
cargo run --release --example bench
cargo run --release --example bench -- --mode aggressive
cargo run --release --example bench -- --positive /path/to/text --negative /path/to/no-text

# Regenerate the synthetic negative set
cargo run --release --example gen_negatives
```

Representative numbers on a modern laptop (Conservative mode, release build):

| Input | Time |
|-------|------|
| `has_text_pixels` 256×256 | ~40 µs |
| `has_text_pixels` 2480×3508 (subsampled internally) | ~300 µs |
| `has_text_rgb` 2480×3508 (RGB→gray + subsample) | ~500 µs |
| `has_text` JPEG encoded bytes | 1–20 ms (dominated by decode) |

## Integrating with OCR engines

`ocr-triage` is **not** an OCR engine. It's a pre-filter. A real pipeline looks like:

```rust
for image in batch {
    let verdict = ocr_triage::has_text(&image);
    if verdict.has_text {
        let text = tesseract::ocr(&image)?;
        collect(text);
    }
}
```

Pairs naturally with Tesseract, PaddleOCR, RapidOCR, Azure/AWS OCR APIs, or any custom engine. The companion project [`kreuzberg-ocr-triage`](https://github.com/hasantr/kreuzberg-text-triage) wires this into the Kreuzberg document-extraction pipeline as an `OcrBackend` adapter with configurable delegation.

## Safety and dependencies

- Pure Rust, no `unsafe` in library code.
- Dependencies: [`image`](https://crates.io/crates/image) (default features off, only jpeg/png/webp/tiff/bmp enabled), [`zune-jpeg`](https://crates.io/crates/zune-jpeg), [`zune-core`](https://crates.io/crates/zune-core) for the fast JPEG luma-only path.
- Zero C/C++ linkage. Builds cleanly on Linux, macOS, Windows, and cross-targets.
- All public functions are panic-free on adversarial input (empty bytes, truncated headers, impossible dimensions).

## Compatibility

- Minimum Rust version: **1.75**.
- `no_std`: not currently supported (uses `std::time::Instant` for latency metrics and `Vec` for subsampling buffers). PR welcome.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any additional terms or conditions.

## Credits

- Design and maintenance: **Hasan Salihoğlu**.
- Algorithm implementation and documentation co-authored with **Claude (Opus 4.6, Anthropic)**.
