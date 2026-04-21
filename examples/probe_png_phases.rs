//! PNG decode fazlarını ayrıştırır: headers + raw decode + colorspace→luma + resize.
//! Küçük bir PNG (512×512) üstünde 500 iter alıp hangi fazın baskın olduğunu gösterir.

use std::time::Instant;

use image::{imageops::FilterType, GrayImage};
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_png::PngDecoder;

fn bench_file(path: &str, iters: usize) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path} okunamadı: {e}");
            return;
        }
    };

    // Warmup
    for _ in 0..50 {
        let mut d = PngDecoder::new(std::io::Cursor::new(&bytes));
        let _ = d.decode_headers();
        let _ = d.decode_raw();
    }

    let mut hdr = Vec::with_capacity(iters);
    let mut dec = Vec::with_capacity(iters);
    let mut conv = Vec::with_capacity(iters);
    let mut resize = Vec::with_capacity(iters);
    let mut total = Vec::with_capacity(iters);

    for _ in 0..iters {
        let t0 = Instant::now();
        let mut decoder = PngDecoder::new(std::io::Cursor::new(&bytes));
        decoder.decode_headers().unwrap();
        let hdr_us = t0.elapsed().as_micros() as u64;

        let (w, h) = decoder.dimensions().unwrap();
        let (w, h) = (w as u32, h as u32);
        let colorspace = decoder.colorspace().unwrap();
        let depth = decoder.depth().unwrap();
        if !matches!(depth, BitDepth::Eight) {
            continue;
        }

        let t1 = Instant::now();
        let pixels = decoder.decode_raw().unwrap();
        let dec_us = t1.elapsed().as_micros() as u64;

        let t2 = Instant::now();
        let gray = match colorspace {
            ColorSpace::Luma => pixels,
            ColorSpace::LumaA => luma_a_to_luma(&pixels),
            ColorSpace::RGB => rgb_to_luma(&pixels),
            ColorSpace::RGBA => rgba_to_luma(&pixels),
            _ => continue,
        };
        let conv_us = t2.elapsed().as_micros() as u64;

        let img = GrayImage::from_raw(w, h, gray).unwrap();
        let t3 = Instant::now();
        let small = {
            let (tw, th) = target_dims(w, h, 256);
            if (tw, th) == (w, h) {
                img
            } else {
                image::imageops::resize(&img, tw, th, FilterType::Nearest)
            }
        };
        let resize_us = t3.elapsed().as_micros() as u64;

        let total_us = hdr_us + dec_us + conv_us + resize_us;
        let _ = small.dimensions();

        hdr.push(hdr_us);
        dec.push(dec_us);
        conv.push(conv_us);
        resize.push(resize_us);
        total.push(total_us);
    }

    println!("\n=== {path} ({} KB, {} iter) ===", bytes.len() / 1024, iters);
    let colorspace = {
        let mut d = PngDecoder::new(std::io::Cursor::new(&bytes));
        d.decode_headers().unwrap();
        d.colorspace().unwrap()
    };
    let (w, h) = {
        let mut d = PngDecoder::new(std::io::Cursor::new(&bytes));
        d.decode_headers().unwrap();
        d.dimensions().unwrap()
    };
    println!("  dims: {}x{}  colorspace: {:?}", w, h, colorspace);
    print_phase("headers", &mut hdr);
    print_phase("decode", &mut dec);
    print_phase("conv→luma", &mut conv);
    print_phase("resize", &mut resize);
    print_phase("TOTAL", &mut total);
}

fn print_phase(label: &str, v: &mut Vec<u64>) {
    v.sort_unstable();
    let n = v.len();
    let mean = v.iter().sum::<u64>() / n.max(1) as u64;
    let p50 = v[n / 2];
    let p99 = v[(n as f32 * 0.99) as usize];
    let min = v[0];
    println!(
        "  {:<10} min {:>5}   mean {:>6}   p50 {:>5}   p99 {:>6}  µs",
        label, min, mean, p50, p99
    );
}

fn target_dims(w: u32, h: u32, short_edge: u32) -> (u32, u32) {
    let (tw, th) = if w <= h {
        let scale = short_edge as f32 / w as f32;
        (short_edge, (h as f32 * scale).round() as u32)
    } else {
        let scale = short_edge as f32 / h as f32;
        ((w as f32 * scale).round() as u32, short_edge)
    };
    (tw.max(1).min(w), th.max(1).min(h))
}

fn rgb_to_luma(rgb: &[u8]) -> Vec<u8> {
    let n = rgb.len() / 3;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = rgb[i * 3] as u32;
        let g = rgb[i * 3 + 1] as u32;
        let b = rgb[i * 3 + 2] as u32;
        out.push(((77 * r + 150 * g + 29 * b) >> 8) as u8);
    }
    out
}

fn rgba_to_luma(rgba: &[u8]) -> Vec<u8> {
    let n = rgba.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = rgba[i * 4] as u32;
        let g = rgba[i * 4 + 1] as u32;
        let b = rgba[i * 4 + 2] as u32;
        out.push(((77 * r + 150 * g + 29 * b) >> 8) as u8);
    }
    out
}

fn luma_a_to_luma(la: &[u8]) -> Vec<u8> {
    let n = la.len() / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(la[i * 2]);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        for path in &args {
            bench_file(path, 20);
        }
        return;
    }
    let testset = "D:/PROJELER/kreuzberg-text-triage/ocr-triage/testset";
    for f in [
        // küçük, basit
        "negative/solid_white.png",
        "negative/logo_circle.png",
        // text screenshot
        "positive/test_Consolas_48.png",
        "positive/test_Times_New_Roman_72.png",
        // photo-like
        "negative/photo_like_a.png",
    ] {
        bench_file(&format!("{testset}/{f}"), 500);
    }
}
