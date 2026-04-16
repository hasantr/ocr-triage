//! ocr-triage — Bir görselde metin olup olmadığına milisaniye altında karar verir.
//!
//! İki API katmanı:
//!
//! ```ignore
//! // Encoded bytes (JPEG/PNG/WebP/...) path — decode dahil.
//! let bytes = std::fs::read("photo.jpg").unwrap();
//! let v = ocr_triage::has_text(&bytes);
//!
//! // Raw pixel path — caller zaten decoded pixel elinde (Kreuzberg PDF
//! // page renderer gibi). <50µs hedef.
//! let v = ocr_triage::has_text_pixels(&gray, width, height);
//! let v = ocr_triage::has_text_rgb(&rgb, width, height);
//! ```

mod config;
mod decode;
mod score;

pub use config::{TriageConfig, TriageMode};

#[derive(Debug, Clone, Copy)]
pub struct TriageVerdict {
    pub has_text: bool,
    pub score: f32,
    pub elapsed_us: u32,
}

impl TriageVerdict {
    fn empty(elapsed_us: u32) -> Self {
        TriageVerdict {
            has_text: false,
            score: 0.0,
            elapsed_us,
        }
    }
}

// ------------------------------------------------------------
// Encoded bytes path (decode + downsample + score)
// ------------------------------------------------------------

/// Default Conservative mode ile çağırır (FN minimize).
pub fn has_text(bytes: &[u8]) -> TriageVerdict {
    has_text_with_config(bytes, &TriageConfig::conservative())
}

pub fn has_text_with_config(bytes: &[u8], cfg: &TriageConfig) -> TriageVerdict {
    let t0 = std::time::Instant::now();
    let img = match decode::decode_thumbnail(bytes, cfg.thumbnail_short_edge) {
        Some(g) => g,
        None => return TriageVerdict::empty(t0.elapsed().as_micros() as u32),
    };
    let s = score::compute(&img);
    let elapsed_us = t0.elapsed().as_micros() as u32;
    TriageVerdict {
        has_text: s >= cfg.threshold,
        score: s,
        elapsed_us,
    }
}

// ------------------------------------------------------------
// Raw pixel path (no decode — fast)
// ------------------------------------------------------------

/// Raw grayscale buffer (length = `width * height`). <50µs hedef.
pub fn has_text_pixels(gray: &[u8], width: u32, height: u32) -> TriageVerdict {
    has_text_pixels_with_config(gray, width, height, &TriageConfig::conservative())
}

pub fn has_text_pixels_with_config(
    gray: &[u8],
    width: u32,
    height: u32,
    cfg: &TriageConfig,
) -> TriageVerdict {
    let t0 = std::time::Instant::now();
    if gray.len() != (width as usize) * (height as usize) {
        return TriageVerdict::empty(t0.elapsed().as_micros() as u32);
    }
    let stride = subsample_stride(width, height, cfg.thumbnail_short_edge);
    let s = if stride == 1 {
        score::compute_raw(gray, width, height)
    } else {
        let (sample, sw, sh) = subsample_gray(gray, width, height, stride);
        score::compute_raw(&sample, sw, sh)
    };
    let elapsed_us = t0.elapsed().as_micros() as u32;
    TriageVerdict {
        has_text: s >= cfg.threshold,
        score: s,
        elapsed_us,
    }
}

/// Raw RGB8 buffer (length = `width * height * 3`). Gray'e çevirip puanlar.
pub fn has_text_rgb(rgb: &[u8], width: u32, height: u32) -> TriageVerdict {
    has_text_rgb_with_config(rgb, width, height, &TriageConfig::conservative())
}

pub fn has_text_rgb_with_config(
    rgb: &[u8],
    width: u32,
    height: u32,
    cfg: &TriageConfig,
) -> TriageVerdict {
    let t0 = std::time::Instant::now();
    let expected = (width as usize) * (height as usize) * 3;
    if rgb.len() != expected {
        return TriageVerdict::empty(t0.elapsed().as_micros() as u32);
    }
    // Subsample + RGB→Gray tek pas — büyük image'da tüm pikseli taramaktan
    // kaçınır. 2480×3508 için stride=13, ~72× daha az iş.
    let stride = subsample_stride(width, height, cfg.thumbnail_short_edge);
    let (gray, sw, sh) = rgb_to_gray_subsampled(rgb, width, height, stride);
    let s = score::compute_raw(&gray, sw, sh);
    let elapsed_us = t0.elapsed().as_micros() as u32;
    TriageVerdict {
        has_text: s >= cfg.threshold,
        score: s,
        elapsed_us,
    }
}

/// Kısa kenarı `target` piksele indirgemek için tam sayı stride.
fn subsample_stride(width: u32, height: u32, target: u32) -> u32 {
    let short = width.min(height).max(1);
    (short / target.max(1)).max(1)
}

/// Grayscale buffer'ı stride ile nearest-neighbor subsample.
fn subsample_gray(gray: &[u8], width: u32, height: u32, stride: u32) -> (Vec<u8>, u32, u32) {
    let sw = (width / stride).max(1);
    let sh = (height / stride).max(1);
    let mut out = Vec::with_capacity((sw * sh) as usize);
    for sy in 0..sh {
        let y = (sy * stride) as usize;
        let row = &gray[y * width as usize..(y + 1) * width as usize];
        for sx in 0..sw {
            let x = (sx * stride) as usize;
            out.push(row[x]);
        }
    }
    (out, sw, sh)
}

/// RGB8 → Gray dönüşümü ve stride-subsample tek pasta.
/// BT.601 luma (yaklaşık): Y = (77R + 150G + 29B) >> 8
fn rgb_to_gray_subsampled(rgb: &[u8], width: u32, height: u32, stride: u32) -> (Vec<u8>, u32, u32) {
    let sw = (width / stride).max(1);
    let sh = (height / stride).max(1);
    let mut out = Vec::with_capacity((sw * sh) as usize);
    let row_bytes = (width * 3) as usize;
    for sy in 0..sh {
        let y = (sy * stride) as usize;
        let row_start = y * row_bytes;
        for sx in 0..sw {
            let x = (sx * stride) as usize;
            let p = row_start + x * 3;
            let r = rgb[p] as u32;
            let g = rgb[p + 1] as u32;
            let b = rgb[p + 2] as u32;
            out.push(((77 * r + 150 * g + 29 * b) >> 8) as u8);
        }
    }
    (out, sw, sh)
}
