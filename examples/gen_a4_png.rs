//! Diagnostic: büyük A4 PNG üret (text ve photo-like) — PNG decode ölçümü için.

use image::{GrayImage, ImageFormat};
use std::io::Cursor;

fn main() {
    let w: u32 = 2480;
    let h: u32 = 3508;

    // Text-like: çoğunluk beyaz, satır satır siyah çizgiler (text simülasyonu).
    let mut text = GrayImage::new(w, h);
    for y in 0..h {
        let is_text_line = (y / 40) % 2 == 0 && (y % 40) < 28;
        for x in 0..w {
            let p = if is_text_line && (x >= 100 && x < 2300) {
                // "text" ink distribution
                let in_glyph = (x / 18) % 3 < 2 && (x % 18) < 11;
                if in_glyph { 30 } else { 255 }
            } else {
                255
            };
            text.put_pixel(x, y, image::Luma([p as u8]));
        }
    }
    let mut buf = Vec::new();
    text.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
    std::fs::write("/tmp/a4_text.png", &buf).unwrap();
    println!("a4_text.png: {} KB", buf.len() / 1024);

    // Photo-like: gradient + noise
    let mut photo = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let grad = ((x + y) / 12) as u32;
            let noise = ((x.wrapping_mul(1103515245).wrapping_add(12345) ^ y.wrapping_mul(214013)) >> 8) & 0x1F;
            let v = ((grad + noise) % 256) as u8;
            photo.put_pixel(x, y, image::Luma([v]));
        }
    }
    let mut buf = Vec::new();
    photo.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
    std::fs::write("/tmp/a4_photo.png", &buf).unwrap();
    println!("a4_photo.png: {} KB", buf.len() / 1024);
}
