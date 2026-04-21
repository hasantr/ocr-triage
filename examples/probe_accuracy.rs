//! Accuracy probe: mevcut testset skor dağılımı + sentetik edge case'ler.
//!
//! Amaç: algılama formülünün marj ve başarısızlık modlarını gözlemlemek.

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use ocr_triage::{has_text, TriageConfig};

fn collect_pngs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "tiff" | "bmp") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn dump_sorted_scores(dir: &Path, expected: bool, cfg: &TriageConfig) {
    let mut rows: Vec<(f32, String, bool)> = Vec::new();
    for path in collect_pngs(dir) {
        let Ok(bytes) = fs::read(&path) else { continue };
        let v = has_text(&bytes);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        rows.push((v.score, name, v.has_text));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (score, name, has_text) in rows {
        let correct = has_text == expected;
        let tag = if correct { "✓" } else { "✗ FAIL" };
        let margin = score - cfg.threshold;
        println!(
            "  {:.3}  Δ{:+.3}  {}  {}",
            score, margin, tag, name
        );
    }
}

fn synth_png(img: &DynamicImage) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .unwrap();
    buf
}

fn eval_synth(label: &str, expect_text: bool, bytes: &[u8], cfg: &TriageConfig) {
    let v = has_text(bytes);
    let margin = v.score - cfg.threshold;
    let correct = v.has_text == expect_text;
    let tag = if correct { "✓" } else { "✗ FAIL" };
    println!(
        "  {:<48} score {:.3}  Δ{:+.3}  has_text={}  expect={}  {}",
        label, v.score, margin, v.has_text, expect_text, tag
    );
}

// ---------- Synthetic generators ----------

fn mk_blank(color: [u8; 3], w: u32, h: u32) -> RgbImage {
    RgbImage::from_pixel(w, h, Rgb(color))
}

fn mk_text_rows(rows: u32, density_h: u32, w: u32, h: u32, bg: [u8; 3], fg: [u8; 3]) -> RgbImage {
    let mut img = RgbImage::from_pixel(w, h, Rgb(bg));
    let row_spacing = h / (rows + 1).max(2);
    for r in 1..=rows {
        let y_start = r * row_spacing - 10;
        for yy in 0..20 {
            let y = y_start + yy;
            if y >= h { break; }
            for x in 40..(w.saturating_sub(40)) {
                // Glyph-like pattern: density_h pixels on / off
                if (x / density_h) % 3 < 2 && (x % density_h) < (density_h * 3 / 5) {
                    img.put_pixel(x, y, Rgb(fg));
                }
            }
        }
    }
    img
}

fn mk_icon_grid(w: u32, h: u32) -> RgbImage {
    // Yoğun ikon-benzeri, bizim FP tuzağımız.
    let mut img = RgbImage::from_pixel(w, h, Rgb([240, 240, 245]));
    let cell = 64;
    for cy in (0..h).step_by(cell as usize) {
        for cx in (0..w).step_by(cell as usize) {
            for yy in 0..cell {
                for xx in 0..cell {
                    let x = cx + xx;
                    let y = cy + yy;
                    if x >= w || y >= h { continue; }
                    // Rounded-ish filled square
                    let dx = xx as i32 - (cell as i32) / 2;
                    let dy = yy as i32 - (cell as i32) / 2;
                    if dx.abs() < 20 && dy.abs() < 20 {
                        img.put_pixel(x, y, Rgb([50, 80, 150]));
                    }
                }
            }
        }
    }
    img
}

fn mk_qr_like(w: u32, h: u32) -> RgbImage {
    // QR benzeri: beyaz arka üstünde ~8 px modüllerle siyah/beyaz rastgele blok.
    let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
    let m = 8;
    let mut seed = 0x12345678u32;
    for cy in (0..h).step_by(m) {
        for cx in (0..w).step_by(m) {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let dark = (seed >> 16) & 1 == 0;
            if dark {
                for yy in 0..m as u32 {
                    for xx in 0..m as u32 {
                        let x = cx + xx;
                        let y = cy + yy;
                        if x < w && y < h {
                            img.put_pixel(x, y, Rgb([0, 0, 0]));
                        }
                    }
                }
            }
        }
    }
    img
}

fn mk_stripes(w: u32, h: u32, period: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if (x / period) % 2 == 0 { 255 } else { 0 };
            img.put_pixel(x, y, Rgb([v, v, v]));
        }
    }
    img
}

fn mk_random_noise(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    let mut seed = 0xdeadbeefu32;
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let v = ((seed >> 16) & 0xFF) as u8;
            img.put_pixel(x, y, Rgb([v, v, v]));
        }
    }
    img
}

fn mk_gradient(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y) * 255 / (w + h).max(1)) as u8;
            img.put_pixel(x, y, Rgb([v, v, v]));
        }
    }
    img
}

fn mk_tiny_text(w: u32, h: u32) -> RgbImage {
    // Tek satır text, image'ın %5'i gibi küçük bir bölgede.
    let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
    for y in 20..40 {
        for x in 40..80 {
            if (x / 4) % 2 == 0 && (x % 4) < 2 {
                img.put_pixel(x, y, Rgb([20, 20, 20]));
            }
        }
    }
    img
}

fn mk_vertical_text(w: u32, h: u32) -> RgbImage {
    // Dikey yazılan CJK benzeri: karakterler sütun-bazlı alt alta
    // dizilmiş, satırlar yerine sütunlar glyph/interline ayrımı taşır.
    let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
    // 3 sütun text, her sütun ~14 px kalın glyph bölgesi
    for col_x in (40..w - 40).step_by(60) {
        for x in col_x..(col_x + 28).min(w - 40) {
            // Her karakter ~30 px yükseklikte, aralarda ~10 px boşluk
            for y_base in (40..h - 40).step_by(40) {
                for y in y_base..(y_base + 30).min(h - 40) {
                    // Glyph iç pattern: dikey-yatay çizgiler (CJK benzeri kompleks stroke)
                    let gx = (x - col_x) as i32;
                    let gy = (y - y_base) as i32;
                    let stroke = (gx == 0 || gx == 14 || gx == 27)
                        || (gy == 0 || gy == 15 || gy == 29)
                        || (gx.abs_diff(14) == 0);
                    if stroke && x < w && y < h {
                        img.put_pixel(x, y, Rgb([20, 20, 20]));
                    }
                }
            }
        }
    }
    img
}

fn mk_inverted_text(w: u32, h: u32) -> RgbImage {
    // Siyah arka, beyaz yazı — polarity-invariant test.
    let mut img = RgbImage::from_pixel(w, h, Rgb([10, 10, 15]));
    for y in (40..h - 40).step_by(30) {
        for line_y in y..y + 18 {
            for x in 40..w - 40 {
                if (x / 14) % 3 < 2 && (x % 14) < 8 {
                    img.put_pixel(x, line_y, Rgb([240, 240, 240]));
                }
            }
        }
    }
    img
}

fn main() {
    let cfg = TriageConfig::conservative();
    let positive_dir = "D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset/positive";
    let negative_dir = "D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset/negative";

    println!("config: threshold={:.3} thumbnail={}", cfg.threshold, cfg.thumbnail_short_edge);

    println!("\n=== POSITIVE (text expected — score >= {:.2}) ===", cfg.threshold);
    dump_sorted_scores(Path::new(positive_dir), true, &cfg);

    println!("\n=== NEGATIVE (no text expected — score <  {:.2}) ===", cfg.threshold);
    dump_sorted_scores(Path::new(negative_dir), false, &cfg);

    // Marj analizi
    println!("\n=== Margin analysis ===");
    let (mut min_pos, mut max_neg) = (f32::MAX, f32::MIN);
    for path in collect_pngs(Path::new(positive_dir)) {
        let Ok(b) = fs::read(&path) else { continue };
        min_pos = min_pos.min(has_text(&b).score);
    }
    for path in collect_pngs(Path::new(negative_dir)) {
        let Ok(b) = fs::read(&path) else { continue };
        max_neg = max_neg.max(has_text(&b).score);
    }
    println!("  Positive min: {:.3}  (Δ vs threshold: {:+.3})", min_pos, min_pos - cfg.threshold);
    println!("  Negative max: {:.3}  (Δ vs threshold: {:+.3})", max_neg, max_neg - cfg.threshold);
    println!("  Total safe margin (pos_min - neg_max): {:.3}", min_pos - max_neg);

    // Sentetik stres testleri
    println!("\n=== Synthetic stress cases (600×400) ===\n");
    let (w, h) = (600u32, 400u32);

    // Positive cases
    eval_synth(
        "text-rows dark on white (standard)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_text_rows(
            8, 14, w, h, [255, 255, 255], [20, 20, 20],
        ))),
        &cfg,
    );
    eval_synth(
        "inverted text: white on dark (polarity)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_inverted_text(w, h))),
        &cfg,
    );
    eval_synth(
        "vertical CJK-like (dikey yazı)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_vertical_text(w, h))),
        &cfg,
    );
    eval_synth(
        "low-contrast text (gray on lighter gray)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_text_rows(
            6, 14, w, h, [180, 180, 180], [120, 120, 120],
        ))),
        &cfg,
    );
    eval_synth(
        "dense text (many rows)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_text_rows(
            16, 10, w, h, [255, 255, 255], [10, 10, 10],
        ))),
        &cfg,
    );
    eval_synth(
        "tiny text (tek satır, image'ın %5'i)",
        true,
        &synth_png(&DynamicImage::ImageRgb8(mk_tiny_text(w, h))),
        &cfg,
    );

    // Negative cases
    eval_synth(
        "solid white",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_blank([255; 3], w, h))),
        &cfg,
    );
    eval_synth(
        "solid gray",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_blank([128; 3], w, h))),
        &cfg,
    );
    eval_synth(
        "random noise (pure)",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_random_noise(w, h))),
        &cfg,
    );
    eval_synth(
        "horizontal gradient",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_gradient(w, h))),
        &cfg,
    );
    eval_synth(
        "vertical stripes (period=8)",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_stripes(w, h, 8))),
        &cfg,
    );
    eval_synth(
        "dense icon grid (expected FP risk)",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_icon_grid(w, h))),
        &cfg,
    );
    eval_synth(
        "QR-like random blocks",
        false,
        &synth_png(&DynamicImage::ImageRgb8(mk_qr_like(w, h))),
        &cfg,
    );
}
