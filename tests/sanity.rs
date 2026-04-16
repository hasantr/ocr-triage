use ocr_triage::{has_text, has_text_with_config, TriageConfig, TriageMode};

fn solid_png(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(color));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

#[test]
fn empty_bytes_returns_no_text() {
    let v = has_text(&[]);
    assert!(!v.has_text);
}

#[test]
fn malformed_bytes_returns_no_text_no_panic() {
    let v = has_text(b"not an image at all");
    assert!(!v.has_text);
}

#[test]
fn solid_white_is_text_free() {
    let png = solid_png(256, 256, [255, 255, 255]);
    let v = has_text(&png);
    assert!(
        !v.has_text,
        "solid white should be text-free, got score={}",
        v.score
    );
}

#[test]
fn solid_black_is_text_free() {
    let png = solid_png(256, 256, [0, 0, 0]);
    let v = has_text(&png);
    assert!(
        !v.has_text,
        "solid black should be text-free, got score={}",
        v.score
    );
}

#[test]
fn aggressive_mode_has_higher_threshold() {
    let cons = TriageConfig::from_mode(TriageMode::Conservative);
    let aggr = TriageConfig::from_mode(TriageMode::Aggressive);
    assert!(aggr.threshold > cons.threshold);
}

#[test]
fn verdict_carries_elapsed() {
    let png = solid_png(64, 64, [128, 128, 128]);
    let v = has_text_with_config(&png, &TriageConfig::conservative());
    assert!(v.elapsed_us > 0);
}
