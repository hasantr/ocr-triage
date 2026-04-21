//! A/B test: zune-png (zune-inflate) vs image crate (png + flate2/zlib-rs)
//! on the same PNG. Measures full decode to GrayImage (no downsample).

use image::GenericImageView;
use std::time::Instant;
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_png::PngDecoder;

fn time_zune(bytes: &[u8], n: usize) -> (u64, u64) {
    let mut decode_samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        let mut decoder = PngDecoder::new(std::io::Cursor::new(bytes));
        decoder.decode_headers().unwrap();
        let (w, h) = decoder.dimensions().unwrap();
        let cs = decoder.colorspace().unwrap();
        let depth = decoder.depth().unwrap();
        assert_eq!(depth, BitDepth::Eight);
        let pixels = decoder.decode_raw().unwrap();
        // Collapse to luma consistent with decode.rs.
        let gray: Vec<u8> = match cs {
            ColorSpace::Luma => pixels,
            ColorSpace::LumaA => pixels.chunks_exact(2).map(|c| c[0]).collect(),
            ColorSpace::RGB => pixels
                .chunks_exact(3)
                .map(|c| ((77 * c[0] as u32 + 150 * c[1] as u32 + 29 * c[2] as u32) >> 8) as u8)
                .collect(),
            ColorSpace::RGBA => pixels
                .chunks_exact(4)
                .map(|c| ((77 * c[0] as u32 + 150 * c[1] as u32 + 29 * c[2] as u32) >> 8) as u8)
                .collect(),
            _ => panic!(),
        };
        let (_w, _h) = (w, h);
        let _ = gray;
        decode_samples.push(t.elapsed().as_micros() as u64);
    }
    decode_samples.sort_unstable();
    (
        decode_samples.iter().sum::<u64>() / n as u64,
        decode_samples[n / 2],
    )
}

fn time_image(bytes: &[u8], n: usize) -> (u64, u64) {
    let mut decode_samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        let img = image::load_from_memory(bytes).unwrap();
        let gray = img.to_luma8();
        let _ = gray.dimensions();
        decode_samples.push(t.elapsed().as_micros() as u64);
    }
    decode_samples.sort_unstable();
    (
        decode_samples.iter().sum::<u64>() / n as u64,
        decode_samples[n / 2],
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: probe_png_ab FILE.png [FILE2.png ...]");
        std::process::exit(2);
    }
    println!(
        "{:<60} {:>12} {:>12} {:>8}",
        "FILE", "zune-png µs", "image µs", "ratio"
    );
    for path in &args {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{}: okunamadı", path);
            continue;
        };
        // warmup
        let _ = time_zune(&bytes, 3);
        let _ = time_image(&bytes, 3);
        let (z_mean, _z_p50) = time_zune(&bytes, 20);
        let (i_mean, _i_p50) = time_image(&bytes, 20);
        let short = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let ratio = z_mean as f64 / i_mean as f64;
        println!("{:<60} {:>12} {:>12} {:>8.2}x", short, z_mean, i_mean, ratio);
    }
}
