//! Production path breakdown: `decode_thumbnail` + `has_text_pixels` fazlarını ayrı ölçer.
//! Format bazında mean/p50/p95 verir, dosya başına en yavaş 5'i listeler.
//!
//! Kullanım:
//!   cargo run --release --example bench_breakdown -- --input /path/to/images
//!
//! İç akış:
//!   1. bytes → `decode_thumbnail` (sniff + scaled/full decode + subsample) → GrayImage
//!   2. `has_text_pixels` (score üzerine thumbnail)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ocr_triage::__internal::decode_thumbnail;
use ocr_triage::has_text_pixels;

fn parse_args() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut input = manifest.join("testset");
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--input" {
            i += 1;
            if i < args.len() {
                input = PathBuf::from(&args[i]);
            }
        }
        i += 1;
    }
    input
}

fn collect_images_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_images_recursive(&p, out);
        } else if p.is_file() {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "tiff" | "tif" | "bmp"
            ) {
                out.push(p);
            }
        }
    }
}

#[derive(Default, Clone)]
struct Samples {
    decode_us: Vec<u64>,
    score_us: Vec<u64>,
    total_us: Vec<u64>,
    worst: Vec<(PathBuf, u64)>, // per-file median total
}

fn percentile(sorted: &[u64], p: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f32 - 1.0) * p) as usize;
    sorted[idx]
}

fn mean(v: &[u64]) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.iter().sum::<u64>() / v.len() as u64
}

fn phase_line(label: &str, mut v: Vec<u64>) {
    v.sort_unstable();
    let n = v.len();
    println!(
        "  {:<8} mean {:>6} µs  p50 {:>5}  p95 {:>6}  max {:>6}  (n={n})",
        label,
        mean(&v),
        percentile(&v, 0.50),
        percentile(&v, 0.95),
        v.last().copied().unwrap_or(0),
    );
}

fn main() {
    let input = parse_args();
    println!("input: {}\n", input.display());

    let mut paths = Vec::new();
    collect_images_recursive(&input, &mut paths);
    paths.sort();
    if paths.is_empty() {
        eprintln!("Uygun image bulunamadı.");
        return;
    }

    let mut by_ext: BTreeMap<String, Samples> = BTreeMap::new();
    let mut all = Samples::default();

    const SHORT_EDGE: u32 = 256;
    const WARMUP_PER_FILE: usize = 3;
    const ITERS_PER_FILE: usize = 10;

    for path in &paths {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // warmup
        for _ in 0..WARMUP_PER_FILE {
            let _ = decode_thumbnail(&bytes, SHORT_EDGE);
        }

        let mut file_totals = Vec::with_capacity(ITERS_PER_FILE);
        for _ in 0..ITERS_PER_FILE {
            let t0 = Instant::now();
            let img = match decode_thumbnail(&bytes, SHORT_EDGE) {
                Some(g) => g,
                None => continue,
            };
            let decode_us = t0.elapsed().as_micros() as u64;

            let (w, h) = img.dimensions();
            let t1 = Instant::now();
            let v = has_text_pixels(img.as_raw(), w, h);
            let score_us = t1.elapsed().as_micros() as u64;
            let _ = v.has_text;

            let total = decode_us + score_us;
            file_totals.push(total);
            all.decode_us.push(decode_us);
            all.score_us.push(score_us);
            all.total_us.push(total);

            let s = by_ext.entry(ext.clone()).or_default();
            s.decode_us.push(decode_us);
            s.score_us.push(score_us);
            s.total_us.push(total);
        }
        file_totals.sort_unstable();
        let median_total = file_totals
            .get(file_totals.len() / 2)
            .copied()
            .unwrap_or(0);
        all.worst.push((path.clone(), median_total));
    }

    // rapor
    for (ext, s) in &by_ext {
        let n = s.total_us.len();
        let dec_sum: u64 = s.decode_us.iter().sum();
        let sco_sum: u64 = s.score_us.iter().sum();
        let tot = dec_sum + sco_sum;
        println!("--- {} (n={n}) ---", ext.to_uppercase());
        phase_line("decode", s.decode_us.clone());
        phase_line("score", s.score_us.clone());
        phase_line("TOTAL", s.total_us.clone());
        if tot > 0 {
            println!(
                "  payı:    decode %{:.1}   score %{:.1}\n",
                dec_sum as f64 * 100.0 / tot as f64,
                sco_sum as f64 * 100.0 / tot as f64
            );
        }
    }

    // Worst offenders (median per-file)
    all.worst.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    println!("=== EN YAVAŞ 10 DOSYA (median) ===");
    for (p, t) in all.worst.iter().take(10) {
        println!("  {:>7} µs   {}", t, p.file_name().unwrap().to_string_lossy());
    }

    println!("\n=== TÜMÜ ===");
    phase_line("decode", all.decode_us.clone());
    phase_line("score", all.score_us.clone());
    phase_line("TOTAL", all.total_us.clone());
    let dec_sum: u64 = all.decode_us.iter().sum();
    let sco_sum: u64 = all.score_us.iter().sum();
    let tot = dec_sum + sco_sum;
    if tot > 0 {
        println!(
            "  payı:    decode %{:.1}   score %{:.1}",
            dec_sum as f64 * 100.0 / tot as f64,
            sco_sum as f64 * 100.0 / tot as f64
        );
    }
}
