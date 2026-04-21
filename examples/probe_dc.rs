//! Diagnostic: DC-only JPEG decoder'ın üretime hazır JPEG'lerde başarılı olup
//! olmadığını göster + 20 iter mean decode süresi.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: probe_dc FILE.jpg [FILE2.jpg ...]");
        std::process::exit(2);
    }
    for path in &args {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{}: okunamadı", path);
            continue;
        };
        let kb = bytes.len() / 1024;

        // Önce warmup
        for _ in 0..3 {
            let _ = ocr_triage::__internal::try_dc_only_jpeg(&bytes);
        }

        const N: usize = 20;
        let mut samples = Vec::with_capacity(N);
        let mut dims: Option<(u32, u32)> = None;
        for _ in 0..N {
            let t = Instant::now();
            let r = ocr_triage::__internal::try_dc_only_jpeg(&bytes);
            let us = t.elapsed().as_micros() as u64;
            samples.push(us);
            if dims.is_none() {
                dims = r;
            }
        }
        samples.sort_unstable();
        let mean = samples.iter().sum::<u64>() / N as u64;
        let p50 = samples[N / 2];
        let p95 = samples[(N * 95) / 100];
        let max = *samples.last().unwrap();

        match dims {
            Some((w, h)) => println!(
                "{}\n  {} KB  DC-only thumbnail {}×{}\n  mean {} µs  p50 {}  p95 {}  max {}",
                path, kb, w, h, mean, p50, p95, max
            ),
            None => println!("{}\n  {} KB  DC-only FAILED (fallback to jpeg-decoder would run)", path, kb),
        }

        // Full pipeline (has_text) comparison.
        let mut total_samples = Vec::with_capacity(N);
        for _ in 0..3 {
            let _ = ocr_triage::has_text(&bytes);
        }
        for _ in 0..N {
            let t = Instant::now();
            let _ = ocr_triage::has_text(&bytes);
            total_samples.push(t.elapsed().as_micros() as u64);
        }
        total_samples.sort_unstable();
        let tmean = total_samples.iter().sum::<u64>() / N as u64;
        let tp50 = total_samples[N / 2];
        println!("  full has_text:  mean {} µs  p50 {}  max {}", tmean, tp50, total_samples.last().unwrap());
        println!();
    }
}
