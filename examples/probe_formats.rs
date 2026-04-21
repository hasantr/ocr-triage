//! Format coverage matrix — aynı text image'ı birden fazla formata encode edip
//! has_text() her birinde çalışıyor mu ve ne sürüyor, ölçer.
//!
//! Amaç: "bu kütüphane hangi formatları kaldırır?" sorusuna net cevap vermek.

use image::{DynamicImage, ImageFormat, Luma, Rgb, RgbImage};
use std::io::Cursor;
use std::time::Instant;

fn synth_text_image() -> RgbImage {
    // 512×512 beyaz arka, dikey siyah çizgiler (text satırı simülasyonu).
    let mut img = RgbImage::from_pixel(512, 512, Rgb([255, 255, 255]));
    for y in (40..500).step_by(30) {
        for line_y in y..(y + 18).min(500) {
            for x in 40..470 {
                // "Glyph" yoğun bölge: her 16 pixelde 10 siyah, 6 beyaz.
                if (x / 16) % 3 < 2 && (x % 16) < 10 {
                    img.put_pixel(x as u32, line_y as u32, Rgb([20, 20, 20]));
                }
            }
        }
    }
    img
}

fn encode_to(img: &DynamicImage, fmt: ImageFormat) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), fmt).ok()?;
    Some(buf)
}

fn measure(bytes: &[u8], label: &str) {
    // warmup
    for _ in 0..3 {
        let _ = ocr_triage::has_text(bytes);
    }
    const N: usize = 20;
    let mut samples = Vec::with_capacity(N);
    let mut last_verdict = None;
    for _ in 0..N {
        let t = Instant::now();
        let v = ocr_triage::has_text(bytes);
        samples.push(t.elapsed().as_micros() as u64);
        last_verdict = Some(v);
    }
    samples.sort_unstable();
    let mean = samples.iter().sum::<u64>() / N as u64;
    let p50 = samples[N / 2];
    let max = *samples.last().unwrap();

    match last_verdict {
        Some(v) => {
            let supported = if v.score > 0.0 || v.has_text {
                "✓ decoded"
            } else {
                // v.score == 0.0 may mean either "genuinely no text" (unlikely for our synthetic)
                // or "decode failed".
                if bytes.len() < 50 {
                    "✓ decoded (solid)"
                } else {
                    "✗ likely decode FAILED"
                }
            };
            println!(
                "  {:<16} {:>9} bytes  {:>8} µs mean  p50 {:>6}  max {:>6}  score={:.3}  has_text={}  [{}]",
                label, bytes.len(), mean, p50, max, v.score, v.has_text, supported
            );
        }
        None => println!("  {:<16} (no result)", label),
    }
}

fn make_minimal_gif() -> Vec<u8> {
    // Geçerli en küçük GIF89a (1×1 tek piksel). Decoder varsa çalışmalı.
    vec![
        // Header "GIF89a"
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61,
        // Logical screen descriptor: 1x1, GCT flag, background=0
        0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00,
        // Global color table: 2 entries (white, black)
        0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
        // Image descriptor: at 0,0, 1x1, no local table
        0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
        // LZW min code size 2, then image data
        0x02, 0x02, 0x44, 0x01, 0x00,
        // Trailer
        0x3B,
    ]
}

fn main() {
    let rgb = synth_text_image();
    let dyn_img = DynamicImage::ImageRgb8(rgb);

    println!("=== Encoded format coverage matrix ===\n");
    println!("Synthetic 512×512 text-like RGB source, re-encoded to each format.\n");

    // --- Formats where image crate has our features enabled ---
    for (fmt, label) in [
        (ImageFormat::Png, "PNG"),
        (ImageFormat::WebP, "WebP"),
        (ImageFormat::Tiff, "TIFF"),
        (ImageFormat::Bmp, "BMP"),
    ] {
        match encode_to(&dyn_img, fmt) {
            Some(bytes) => measure(&bytes, label),
            None => println!("  {:<16} encoder missing (feature not enabled)", label),
        }
    }

    // --- GIF now enabled too ---
    if let Some(bytes) = encode_to(&dyn_img, ImageFormat::Gif) {
        measure(&bytes, "GIF");
    } else {
        println!("  GIF              encoder not available in build");
    }

    // --- Formats NOT in our image-crate feature set ---
    println!("\n=== Formats NOT compiled in (expected failures) ===");
    for (fmt, label) in [
        (ImageFormat::Jpeg, "JPEG (via img)"), // image crate'te jpeg feature kapalı
        (ImageFormat::Ico, "ICO"),
        (ImageFormat::Avif, "AVIF"),
    ] {
        match encode_to(&dyn_img, fmt) {
            Some(bytes) => measure(&bytes, label),
            None => println!("  {:<16} encoder not available in build", label),
        }
    }

    // GIF decode-only sanity — manual minimal GIF.
    println!("\n=== GIF minimal decode sanity (1×1 GIF89a) ===");
    let gif = make_minimal_gif();
    measure(&gif, "minimal.gif");

    // --- Real JPEG from testset ---
    println!("\n=== Real JPEG from testset (via custom DC-only decoder) ===");
    if let Ok(bytes) = std::fs::read("D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset/positive/zamfir_cd_dark_bg.jpeg") {
        measure(&bytes, "zamfir CD .jpeg");
    }
    if let Ok(bytes) = std::fs::read("D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset/positive/test_page.jpg") {
        measure(&bytes, "A4 progressive");
    }

    // --- Raw pixel path (bypasses all decoders) ---
    println!("\n=== Raw pixel path (bypasses all decoders — Kreuzberg PDF production) ===");
    let gray_a4: Vec<u8> = (0..(2480u32 * 3508u32))
        .map(|i| if (i / 2480) % 40 < 24 && (i % 16) < 10 { 20u8 } else { 255u8 })
        .collect();
    let _ = ocr_triage::has_text_pixels(&gray_a4, 2480, 3508);
    const N: usize = 20;
    let mut samples = Vec::with_capacity(N);
    for _ in 0..N {
        let t = Instant::now();
        let _ = ocr_triage::has_text_pixels(&gray_a4, 2480, 3508);
        samples.push(t.elapsed().as_micros() as u64);
    }
    samples.sort_unstable();
    println!(
        "  raw gray A4 2480×3508  mean {} µs  p50 {}",
        samples.iter().sum::<u64>() / N as u64,
        samples[N / 2]
    );
}
