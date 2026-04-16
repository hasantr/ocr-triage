//! testset/positive (text içeren) + testset/negative (text yok) üzerinde
//! triage davranışını ölçer: FN/FP sayısı, latency dağılımı, yanlış sınıflananlar.
//!
//! Kullanım:
//!   cargo run --release --example bench
//!   cargo run --release --example bench -- --mode aggressive
//!   cargo run --release --example bench -- --positive ../V2 --negative testset/negative

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ocr_triage::{has_text_with_config, TriageConfig, TriageMode};

fn parse_args() -> (TriageConfig, PathBuf, PathBuf) {
    let mut mode = TriageMode::Conservative;
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut positive = manifest.join("testset").join("positive");
    let mut negative = manifest.join("testset").join("negative");

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                mode = match args.get(i).map(String::as_str) {
                    Some("aggressive") => TriageMode::Aggressive,
                    _ => TriageMode::Conservative,
                };
            }
            "--positive" => {
                i += 1;
                positive = PathBuf::from(&args[i]);
            }
            "--negative" => {
                i += 1;
                negative = PathBuf::from(&args[i]);
            }
            _ => {}
        }
        i += 1;
    }
    (TriageConfig::from_mode(mode), positive, negative)
}

fn collect_images(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
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
    out.sort();
    out
}

#[derive(Default)]
struct Stats {
    correct: u32,
    wrong: u32,
    latencies_us: Vec<u32>,
    wrong_files: Vec<(PathBuf, f32)>,
    all_scores: Vec<(PathBuf, f32)>,
}

fn run_dir(dir: &Path, expect_text: bool, cfg: &TriageConfig) -> Stats {
    let mut s = Stats::default();
    for path in collect_images(dir) {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let v = has_text_with_config(&bytes, cfg);
        s.latencies_us.push(v.elapsed_us);
        s.all_scores.push((path.clone(), v.score));
        if v.has_text == expect_text {
            s.correct += 1;
        } else {
            s.wrong += 1;
            s.wrong_files.push((path, v.score));
        }
    }
    s
}

fn percentile(sorted: &[u32], p: f32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f32 - 1.0) * p) as usize;
    sorted[idx]
}

fn print_dir(label: &str, s: &Stats) {
    let total = s.correct + s.wrong;
    let acc = if total > 0 {
        s.correct as f32 * 100.0 / total as f32
    } else {
        0.0
    };
    let mut sorted = s.latencies_us.clone();
    sorted.sort_unstable();
    println!("\n{label}: {}/{} doğru (%{:.1})", s.correct, total, acc);
    if !sorted.is_empty() {
        let mean = sorted.iter().map(|&v| v as u64).sum::<u64>() / sorted.len() as u64;
        println!(
            "  latency µs: mean={mean} p50={} p95={} p99={} max={}",
            percentile(&sorted, 0.50),
            percentile(&sorted, 0.95),
            percentile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0),
        );
    }
    if !s.wrong_files.is_empty() {
        println!("  yanlış sınıflananlar (skor):");
        for (p, score) in &s.wrong_files {
            println!(
                "    {:.3}  {}",
                score,
                p.file_name().unwrap().to_string_lossy()
            );
        }
    }
    let mut sorted_scores: Vec<_> = s.all_scores.iter().collect();
    sorted_scores.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    if let (Some(min), Some(max)) = (sorted_scores.first(), sorted_scores.last()) {
        println!(
            "  skor: min={:.3} ({})  max={:.3} ({})",
            min.1,
            min.0.file_name().unwrap().to_string_lossy(),
            max.1,
            max.0.file_name().unwrap().to_string_lossy()
        );
    }
}

fn main() {
    let t0 = Instant::now();
    let (cfg, positive_dir, negative_dir) = parse_args();
    println!(
        "config: threshold={:.3} thumbnail={}",
        cfg.threshold, cfg.thumbnail_short_edge
    );
    println!("positive dir: {}", positive_dir.display());
    println!("negative dir: {}", negative_dir.display());

    let pos = run_dir(&positive_dir, true, &cfg);
    let neg = run_dir(&negative_dir, false, &cfg);

    print_dir("POSITIVE (text bekleniyor)", &pos);
    print_dir("NEGATIVE (text YOK bekleniyor)", &neg);

    let total = pos.correct + pos.wrong + neg.correct + neg.wrong;
    let correct = pos.correct + neg.correct;
    let acc = if total > 0 {
        correct as f32 * 100.0 / total as f32
    } else {
        0.0
    };
    let fn_rate = if pos.correct + pos.wrong > 0 {
        pos.wrong as f32 * 100.0 / (pos.correct + pos.wrong) as f32
    } else {
        0.0
    };
    let fp_rate = if neg.correct + neg.wrong > 0 {
        neg.wrong as f32 * 100.0 / (neg.correct + neg.wrong) as f32
    } else {
        0.0
    };

    println!("\n=== ÖZET ===");
    println!("Toplam: {}/{} doğru (%{:.1})", correct, total, acc);
    println!("FN (text kaçırıldı): %{:.1}", fn_rate);
    println!("FP (boşa OCR): %{:.1}", fp_rate);
    println!("Bench süresi: {:.2}s", t0.elapsed().as_secs_f32());
}
