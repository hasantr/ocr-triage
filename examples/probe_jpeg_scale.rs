//! jpeg-decoder'ın scale() metodunun gerçekten DCT-level mi yoksa post-decode mı
//! çalıştığını direkt ölçerek teşhis eder.

use std::time::Instant;

use jpeg_decoder::{Decoder, PixelFormat};

fn bench_no_scale(bytes: &[u8]) -> (u128, (u16, u16), PixelFormat, usize) {
    let mut d = Decoder::new(std::io::Cursor::new(bytes));
    d.read_info().unwrap();
    let info = d.info().unwrap();
    let t0 = Instant::now();
    let px = d.decode().unwrap();
    let us = t0.elapsed().as_micros();
    (us, (info.width, info.height), info.pixel_format, px.len())
}

fn bench_with_scale(bytes: &[u8], target_w: u16, target_h: u16) -> (u128, (u16, u16), PixelFormat, usize) {
    let mut d = Decoder::new(std::io::Cursor::new(bytes));
    d.read_info().unwrap();
    let info_pre = d.info().unwrap();
    let t0 = Instant::now();
    let (aw, ah) = d.scale(target_w, target_h).unwrap();
    let px = d.decode().unwrap();
    let us = t0.elapsed().as_micros();
    let _ = info_pre;
    (us, (aw, ah), d.info().unwrap().pixel_format, px.len())
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset/positive/test_page.jpg".into());
    let bytes = std::fs::read(&path).unwrap();
    println!("file: {} ({} KB)\n", path, bytes.len() / 1024);

    // 5 iter warmup + ölçüm
    for _ in 0..3 {
        let _ = bench_no_scale(&bytes);
    }
    for _ in 0..3 {
        let _ = bench_with_scale(&bytes, 256, 256);
    }

    println!("{:<40} {:>10} µs   {:>12}   {:>9}", "path", "decode", "dims", "bytes");
    println!("{}", "-".repeat(78));
    for _ in 0..5 {
        let (us, dims, fmt, n) = bench_no_scale(&bytes);
        println!("{:<40} {:>10} µs   {:>6}x{:<5}   {:>9} ({:?})", "no_scale", us, dims.0, dims.1, n, fmt);
    }
    println!();
    for req in [(256u16, 256u16), (512, 512), (1024, 1024), (2048, 2048)] {
        for _ in 0..5 {
            let (us, dims, fmt, n) = bench_with_scale(&bytes, req.0, req.1);
            println!(
                "{:<40} {:>10} µs   {:>6}x{:<5}   {:>9} ({:?})",
                format!("scale({}, {})", req.0, req.1),
                us,
                dims.0,
                dims.1,
                n,
                fmt
            );
        }
    }
}
