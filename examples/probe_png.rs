//! PNG decode latency probe. Mean/p50/p95 over 20 iterations.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: probe_png FILE.png [FILE2.png ...]");
        std::process::exit(2);
    }
    for path in &args {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{}: okunamadı", path);
            continue;
        };
        let kb = bytes.len() / 1024;

        // warmup
        for _ in 0..3 {
            let _ = ocr_triage::__internal::decode_thumbnail(&bytes, 256);
        }

        const N: usize = 20;
        let mut decode_samples = Vec::with_capacity(N);
        let mut total_samples = Vec::with_capacity(N);
        for _ in 0..N {
            let t = Instant::now();
            let thumb = ocr_triage::__internal::decode_thumbnail(&bytes, 256);
            let dec_us = t.elapsed().as_micros() as u64;
            decode_samples.push(dec_us);
            let _ = thumb;

            let t2 = Instant::now();
            let _ = ocr_triage::has_text(&bytes);
            total_samples.push(t2.elapsed().as_micros() as u64);
        }
        decode_samples.sort_unstable();
        total_samples.sort_unstable();
        let d_mean = decode_samples.iter().sum::<u64>() / N as u64;
        let t_mean = total_samples.iter().sum::<u64>() / N as u64;

        println!(
            "{} ({} KB)\n  decode  mean {} µs  p50 {}  p95 {}  max {}\n  total   mean {} µs  p50 {}",
            path,
            kb,
            d_mean,
            decode_samples[N / 2],
            decode_samples[(N * 95) / 100],
            decode_samples[N - 1],
            t_mean,
            total_samples[N / 2],
        );
    }
}
