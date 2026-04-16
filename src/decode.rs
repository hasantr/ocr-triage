use image::{GenericImageView, GrayImage};
use jpeg_decoder::{Decoder as JpegDecoder, PixelFormat};
use zune_core::colorspace::ColorSpace;
use zune_png::PngDecoder;

#[derive(Debug, Clone, Copy)]
enum Format {
    Jpeg,
    Png,
    Webp,
    Tiff,
    Bmp,
    Unknown,
}

fn sniff(bytes: &[u8]) -> Format {
    if bytes.len() < 8 {
        return Format::Unknown;
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Format::Jpeg;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Format::Png;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Format::Webp;
    }
    if bytes.starts_with(&[b'I', b'I', 0x2A, 0x00]) || bytes.starts_with(&[b'M', b'M', 0x00, 0x2A])
    {
        return Format::Tiff;
    }
    if bytes.starts_with(b"BM") {
        return Format::Bmp;
    }
    Format::Unknown
}

/// Bytes → grayscale thumbnail (kısa kenar ≈ `short_edge`).
/// JPEG için DCT seviyesinde **scaled decode** (jpeg-decoder). 1/2, 1/4, 1/8 seçimi
/// dosya boyutu ile `short_edge` oranına göre otomatik. PNG/WebP/TIFF/BMP için
/// image crate ile full decode + Nearest subsample.
pub fn decode_thumbnail(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    match sniff(bytes) {
        Format::Jpeg => decode_jpeg_scaled(bytes, short_edge)
            .or_else(|| decode_image_fallback(bytes, short_edge)),
        Format::Png => {
            decode_png_zune(bytes, short_edge).or_else(|| decode_image_fallback(bytes, short_edge))
        }
        _ => decode_image_fallback(bytes, short_edge),
    }
}

/// Pure Rust SIMD-heavy PNG decoder (zune-png). image crate'in kullandığı png
/// crate'inden genelde 1.5-2× hızlı.
fn decode_png_zune(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    let mut decoder = PngDecoder::new(std::io::Cursor::new(bytes));
    decoder.decode_headers().ok()?;
    let (w, h) = decoder.dimensions()?;
    let (w, h) = (w as u32, h as u32);
    if w == 0 || h == 0 {
        return None;
    }
    let colorspace = decoder.colorspace()?;
    let depth = decoder.depth()?;

    // 16-bit derinlik varsa image fallback'e bırak (nadir).
    if !matches!(depth, zune_core::bit_depth::BitDepth::Eight) {
        return None;
    }

    let pixels = decoder.decode_raw().ok()?;
    let gray = match colorspace {
        ColorSpace::Luma => pixels,
        ColorSpace::LumaA => luma_a_to_luma(&pixels),
        ColorSpace::RGB => rgb_to_luma(&pixels),
        ColorSpace::RGBA => rgba_to_luma(&pixels),
        _ => return None,
    };

    let img = GrayImage::from_raw(w, h, gray)?;
    Some(box_downsample(img, short_edge))
}

/// DCT-level 1/N scaled JPEG decode — tüm pikseli açmadan küçük versiyonu verir.
/// jpeg-decoder 0.3: `Decoder::scale(w, h)` 1, 2, 4, 8 factor'larından biri ile
/// resolution düşürüp decode eder; inverse-DCT daha küçük blok boyutunda çalışır.
fn decode_jpeg_scaled(bytes: &[u8], target_short: u32) -> Option<GrayImage> {
    let mut decoder = JpegDecoder::new(std::io::Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    if w == 0 || h == 0 {
        return None;
    }

    // Hedef scale: kısa kenar `target_short`'a yaklaşsın diye istenen boyut.
    // jpeg-decoder en yakın (≥) 1/N factor'ü seçiyor.
    let short = w.min(h).max(1);
    let tgt_w = ((w as u64 * target_short as u64 / short as u64).max(1)).min(u16::MAX as u64) as u16;
    let tgt_h = ((h as u64 * target_short as u64 / short as u64).max(1)).min(u16::MAX as u64) as u16;
    let (actual_w, actual_h) = decoder.scale(tgt_w, tgt_h).ok()?;
    let pixels = decoder.decode().ok()?;
    let (dw, dh) = (actual_w as u32, actual_h as u32);

    let gray = match info.pixel_format {
        PixelFormat::L8 => {
            if pixels.len() != (dw * dh) as usize {
                return None;
            }
            pixels
        }
        PixelFormat::RGB24 => {
            if pixels.len() != (dw * dh * 3) as usize {
                return None;
            }
            rgb_to_luma(&pixels)
        }
        // L16 / CMYK32 nadir; bu durumda fallback image crate deneyecek.
        _ => return None,
    };

    let img = GrayImage::from_raw(dw, dh, gray)?;

    // DCT scale ≥ target_short olabilir (1/N tam uymaz). Gerekirse Nearest ile tam boyuta indir.
    if dw.min(dh) > target_short {
        Some(box_downsample(img, target_short))
    } else {
        Some(img)
    }
}

fn decode_image_fallback(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    // Tam resolution luma al, sonra manuel stride subsample (image crate'in resize'ı yavaş).
    let luma = img.to_luma8();
    Some(box_downsample(luma, short_edge))
}

/// Aspect-preserving area-average downsample. Her çıkış pikseli, kaynak
/// image'daki tam olarak karşılık gelen **dikdörtgen pencerenin ortalaması**.
/// Integer math, tek-pass. Triangle filter'a çok yakın sonuç (aslında
/// mathematical identity bir çoğu durumda) ama float-pahalılığı yok.
/// Previous implementation integer stride kullanıyordu → stride=1 case'de
/// downsample yapmıyor ve Triangle'dan sapıyor idi; bu fonksiyon tam target
/// dimension'a indirir.
fn box_downsample(img: GrayImage, short_edge: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    let short = w.min(h);
    if short <= short_edge || short_edge == 0 {
        return img;
    }
    let (tw, th) = if w <= h {
        (
            short_edge,
            (h as u64 * short_edge as u64 / w as u64) as u32,
        )
    } else {
        (
            (w as u64 * short_edge as u64 / h as u64) as u32,
            short_edge,
        )
    };
    if tw == 0 || th == 0 {
        return img;
    }

    let src = img.as_raw();
    let w_u = w as usize;
    let h_u = h as usize;
    let mut out = Vec::with_capacity((tw * th) as usize);

    for sy in 0..th {
        let y0 = (sy as u64 * h as u64 / th as u64) as usize;
        let y1 = (((sy + 1) as u64 * h as u64 / th as u64) as usize)
            .min(h_u)
            .max(y0 + 1);
        for sx in 0..tw {
            let x0 = (sx as u64 * w as u64 / tw as u64) as usize;
            let x1 = (((sx + 1) as u64 * w as u64 / tw as u64) as usize)
                .min(w_u)
                .max(x0 + 1);
            let mut sum: u32 = 0;
            let mut count: u32 = 0;
            for y in y0..y1 {
                let row_off = y * w_u;
                for x in x0..x1 {
                    sum += src[row_off + x] as u32;
                    count += 1;
                }
            }
            out.push((sum / count.max(1)) as u8);
        }
    }
    GrayImage::from_raw(tw, th, out).expect("valid subsample dims")
}

/// BT.601 luma (yaklaşık): Y = (77R + 150G + 29B) >> 8
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
