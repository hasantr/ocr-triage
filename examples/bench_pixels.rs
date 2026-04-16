//! Raw pixel path için mikro-benchmark — Kreuzberg'in decoded pixel feed
//! senaryosunu simüle eder. Hedef: 50µs altı.

use std::time::Instant;

use ocr_triage::{has_text_pixels, has_text_rgb};

fn bench_pixels(label: &str, w: u32, h: u32, fill: impl Fn(u32, u32) -> u8) {
    let n = (w * h) as usize;
    let mut gray = Vec::with_capacity(n);
    for y in 0..h {
        for x in 0..w {
            gray.push(fill(x, y));
        }
    }

    // warmup
    for _ in 0..32 {
        let _ = has_text_pixels(&gray, w, h);
    }

    let iters = 1000;
    let t0 = Instant::now();
    let mut score_sum = 0.0f32;
    for _ in 0..iters {
        let v = has_text_pixels(&gray, w, h);
        score_sum += v.score;
    }
    let elapsed = t0.elapsed();
    let per_iter_ns = elapsed.as_nanos() / iters as u128;
    let per_iter_us = per_iter_ns as f32 / 1000.0;
    println!(
        "{label:32} {w}×{h}  {per_iter_us:7.2} µs/call   (score avg {:.3})",
        score_sum / iters as f32
    );
}

fn bench_rgb(label: &str, w: u32, h: u32, fill: impl Fn(u32, u32) -> [u8; 3]) {
    let n = (w * h) as usize * 3;
    let mut rgb = Vec::with_capacity(n);
    for y in 0..h {
        for x in 0..w {
            let p = fill(x, y);
            rgb.push(p[0]);
            rgb.push(p[1]);
            rgb.push(p[2]);
        }
    }

    for _ in 0..32 {
        let _ = has_text_rgb(&rgb, w, h);
    }

    let iters = 1000;
    let t0 = Instant::now();
    let mut score_sum = 0.0f32;
    for _ in 0..iters {
        let v = has_text_rgb(&rgb, w, h);
        score_sum += v.score;
    }
    let elapsed = t0.elapsed();
    let per_iter_ns = elapsed.as_nanos() / iters as u128;
    let per_iter_us = per_iter_ns as f32 / 1000.0;
    println!(
        "{label:32} {w}×{h}  {per_iter_us:7.2} µs/call   (score avg {:.3})",
        score_sum / iters as f32
    );
}

fn main() {
    println!("=== has_text_pixels ===");
    bench_pixels("solid white", 256, 256, |_, _| 255);
    bench_pixels("solid gray", 256, 256, |_, _| 128);
    bench_pixels("gradient", 256, 256, |x, _| (x * 255 / 255) as u8);
    bench_pixels("checker", 256, 256, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 {
            240
        } else {
            20
        }
    });

    bench_pixels("solid white (full)", 2480, 3508, |_, _| 255);
    bench_pixels("checker (full)", 2480, 3508, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 {
            240
        } else {
            20
        }
    });

    println!("\n=== has_text_rgb (RGB→Gray dahil) ===");
    bench_rgb("solid white rgb", 256, 256, |_, _| [255, 255, 255]);
    bench_rgb("checker rgb", 256, 256, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 {
            [240; 3]
        } else {
            [20; 3]
        }
    });
    bench_rgb("solid white (full)", 2480, 3508, |_, _| [255, 255, 255]);
    bench_rgb("checker (full)", 2480, 3508, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 {
            [240; 3]
        } else {
            [20; 3]
        }
    });
}
