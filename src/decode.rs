use image::{imageops::FilterType, GenericImageView, GrayImage};
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

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

/// Bytes → grayscale thumbnail (kısa kenar `short_edge`).
/// JPEG için zune-jpeg fast path (Luma direkt çıktı, downsample sonra).
/// Diğer formatlar image crate (geniş destek).
pub fn decode_thumbnail(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    match sniff(bytes) {
        Format::Jpeg => {
            decode_jpeg_luma(bytes, short_edge).or_else(|| decode_image_fallback(bytes, short_edge))
        }
        _ => decode_image_fallback(bytes, short_edge),
    }
}

fn decode_jpeg_luma(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma);
    let cursor = std::io::Cursor::new(bytes);
    let mut decoder = JpegDecoder::new_with_options(cursor, opts);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    if w == 0 || h == 0 {
        return None;
    }

    let pixels = decoder.decode().ok()?;
    // Luma: single channel, w*h bytes.
    if pixels.len() != (w * h) as usize {
        return None;
    }
    let full = GrayImage::from_raw(w, h, pixels)?;
    Some(downsample(full, short_edge))
}

fn decode_image_fallback(bytes: &[u8], short_edge: u32) -> Option<GrayImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = target_dims(w, h, short_edge);
    let small = if (tw, th) == (w, h) {
        img
    } else {
        img.resize_exact(tw, th, FilterType::Triangle)
    };
    Some(small.to_luma8())
}

fn downsample(img: GrayImage, short_edge: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    let (tw, th) = target_dims(w, h, short_edge);
    if (tw, th) == (w, h) {
        return img;
    }
    image::imageops::resize(&img, tw, th, FilterType::Triangle)
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
