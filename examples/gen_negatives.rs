//! testset/negative/ altına sentetik metin-içermeyen örnekler üretir:
//! solid color, gradient, noise, geometric shapes, blurred photo-like.

use std::path::PathBuf;

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testset")
        .join("negative")
}

fn save(name: &str, w: u32, h: u32, gen: impl Fn(u32, u32) -> [u8; 3]) {
    let mut buf = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let p = gen(x, y);
            let i = ((y * w + x) * 3) as usize;
            buf[i] = p[0];
            buf[i + 1] = p[1];
            buf[i + 2] = p[2];
        }
    }
    let img = image::RgbImage::from_raw(w, h, buf).unwrap();
    let path = out_dir().join(format!("{}.png", name));
    img.save(&path).unwrap();
    println!("wrote {}", path.display());
}

fn main() {
    std::fs::create_dir_all(out_dir()).unwrap();

    // 1) Solid colors
    for (name, color) in [
        ("solid_white", [255u8, 255, 255]),
        ("solid_black", [0u8, 0, 0]),
        ("solid_gray", [128u8, 128, 128]),
        ("solid_red", [220u8, 40, 40]),
        ("solid_blue", [40u8, 80, 200]),
    ] {
        save(name, 512, 512, |_, _| color);
    }

    // 2) Linear gradients (no edges, no text-like structure)
    save("gradient_h", 512, 512, |x, _| {
        let v = (x * 255 / 511) as u8;
        [v, v, v]
    });
    save("gradient_v", 512, 512, |_, y| {
        let v = (y * 255 / 511) as u8;
        [v, v, v]
    });
    save("gradient_diag", 512, 512, |x, y| {
        let v = ((x + y) * 255 / 1022) as u8;
        [v, v, v]
    });

    // 3) Smooth random noise (low-frequency — more like photo)
    let mut seed = 0x9E3779B9u32;
    let mut rng = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed
    };
    let noise: Vec<u8> = (0..(64 * 64)).map(|_| (rng() & 0xFF) as u8).collect();
    save("smooth_noise", 512, 512, |x, y| {
        // Bilinear-ish upsample 64x64 → 512x512
        let nx = (x * 63 / 511) as usize;
        let ny = (y * 63 / 511) as usize;
        let v = noise[ny * 64 + nx];
        [v, v, v]
    });

    // 4) Faux photo: low-freq color blobs
    save("photo_like_a", 512, 512, |x, y| {
        let fx = x as f32 / 512.0;
        let fy = y as f32 / 512.0;
        let r = ((fx * std::f32::consts::PI).sin() * 0.5 + 0.5) * 255.0;
        let g = ((fy * 2.7).cos() * 0.5 + 0.5) * 255.0;
        let b = (((fx + fy) * 1.5).sin() * 0.5 + 0.5) * 255.0;
        [r as u8, g as u8, b as u8]
    });
    save("photo_like_b", 512, 512, |x, y| {
        let cx = x as f32 - 256.0;
        let cy = y as f32 - 256.0;
        let d = (cx * cx + cy * cy).sqrt();
        let v = (255.0 - (d / 360.0).min(1.0) * 200.0) as u8;
        [v, (v as u32 * 2 / 3) as u8, (v as u32 / 2) as u8]
    });

    // 5) Geometric shapes (logo-like): no text
    save("logo_circle", 512, 512, |x, y| {
        let cx = x as f32 - 256.0;
        let cy = y as f32 - 256.0;
        let d = (cx * cx + cy * cy).sqrt();
        if d < 180.0 {
            [40, 80, 200]
        } else {
            [255, 255, 255]
        }
    });
    save("logo_triangle", 512, 512, |x, y| {
        let cx = (x as f32 - 256.0).abs();
        let cy = y as f32;
        if cy > 100.0 && cy < 400.0 && cx < (cy - 100.0) * 0.6 {
            [220, 40, 40]
        } else {
            [255, 255, 255]
        }
    });
    save("logo_squares", 512, 512, |x, y| {
        let bx = (x / 80) % 2;
        let by = (y / 80) % 2;
        if (bx + by) % 2 == 0 {
            [200, 200, 220]
        } else {
            [80, 80, 100]
        }
    });
}
