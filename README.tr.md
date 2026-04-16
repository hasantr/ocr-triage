# ocr-triage

[![Crates.io](https://img.shields.io/crates/v/ocr-triage.svg)](https://crates.io/crates/ocr-triage)
[![Documentation](https://docs.rs/ocr-triage/badge.svg)](https://docs.rs/ocr-triage)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#lisans)
[![CI](https://github.com/hasantr/ocr-triage/actions/workflows/ci.yml/badge.svg)](https://github.com/hasantr/ocr-triage/actions)

**Bu görselde metin var mı?** Saf Rust ile yazılmış, milisaniye altı ikili sınıflandırıcı. Sıfır ML ağırlığı, sıfır C bağımlılığı, dil-bağımsız, format-bağımsız.

English: [README.md](README.md)

---

## Neden

OCR motorları — Tesseract, PaddleOCR, RapidOCR — **image başına 500–2000 ms** harcar. Çoğu zaman beslenen image'da zaten metin yoktur: logolar, fotoğraflar, gradient'ler, düz dolgular, ikonlar. 100 binlik bir batch'te boşa giden zaman saatlerle ölçülür.

`ocr-triage` o zamanı harcamadan önce tek soru sorar:

> **Buna gerçekten OCR çağırmaya değer mi?**

Metin-içermeyen image başına tipik kazanç:

| Yol | Süre |
|-----|------|
| Doğrudan Tesseract | 500–2000 ms |
| `ocr-triage` skip + sonra Tesseract | **~300 µs** karar |

## Özellikler

- **Hızlı.** Raw-RGB sayfa üstünde (2480×3508, 300 dpi A4) ~300 µs. Encoded bytes (JPEG/PNG/WebP) için 1–20 ms (decode'a bağlı).
- **Dil-bağımsız.** Metin tanıma yok, dil modeli yok — yalnızca geometrik sinyaller. Latin, Kiril, Arap, İbranice, CJK, Devanagari ve yatay stroke temelli tüm yazılar çalışır.
- **Format-bağımsız.** Encoded bytes (JPEG/PNG/WebP/TIFF/BMP) veya raw pixel buffer (grayscale/RGB) kabul eder. Raw path decode'u tamamen atlar — PDF sayfa render'ından veya image pipeline'dan gelen pixel'iniz varsa ideal.
- **Polarite-bağımsız.** Açık zemin koyu metin, koyu zemin açık metin — aynı sonuç. Image'a özel Otsu threshold.
- **Sıfır model ağırlığı.** ~220 satır Rust. Tüm kütüphane bir öğle arasında okunabilir.
- **İki mod.** `Conservative` (FN=0 hedef, güvenli default) ve `Aggressive` (FP minimize, CPU-kısıtlı batch için).

## Hızlı başlangıç

```toml
[dependencies]
ocr-triage = "0.1"
```

### Encoded bytes

```rust
use ocr_triage::has_text;

let bytes = std::fs::read("sayfa.jpg")?;
let verdict = has_text(&bytes);

if verdict.has_text {
    run_tesseract(&bytes)?;
} else {
    // Skip — 300 µs'de karar, OCR hiç çağrılmadı.
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Raw pixel path (decode yok)

Upstream image'ı zaten decode etmişse (PDF sayfa renderer, image pipeline) PNG encode→decode gidişi komple atlanır.

```rust
use ocr_triage::{has_text_pixels, has_text_rgb};

// Grayscale: len == width * height
let v = has_text_pixels(&gray, width, height);

// RGB8: len == width * height * 3
let v = has_text_rgb(&rgb, width, height);
```

### Özel konfigürasyon

```rust
use ocr_triage::{has_text_with_config, TriageConfig, TriageMode};

let v = has_text_with_config(&bytes, &TriageConfig::conservative());
let v = has_text_with_config(&bytes, &TriageConfig::from_mode(TriageMode::Aggressive));

let cfg = TriageConfig { threshold: 0.33, thumbnail_short_edge: 192 };
let v = has_text_with_config(&bytes, &cfg);
```

## Çıktı

```rust
pub struct TriageVerdict {
    pub has_text: bool,
    pub score: f32,      // 0.0 .. ~1.0
    pub elapsed_us: u32, // duvar saati mikrosaniye
}
```

Bozuk bytes, boş input veya decoder hatası → `has_text = false`, panic yok. Güvensiz input üzerinde çağırmak güvenli.

## Nasıl çalışır

Pipeline downsampled gri ton thumbnail üstünde çalışır (default kısa kenar 256 px):

1. **Otsu binarization** — image-adaptif threshold foreground/background mask verir. Azınlık sınıf her zaman foreground işaretlenir; koyu-açık ve açık-koyu aynı temsile düşer.
2. **Yatay kenar yoğunluğu** — binary üstünde her satırdaki 0↔1 transition sayısı. Metinde her satır çok sayıda yatay stroke içerir; fotoğraf ve gradient'te yoktur.
3. **Satır projeksiyon varyansı** — her satırdaki foreground oranı ve varyansı. Metin satırları yoğun, satır arası seyrektir → yüksek varyans. Uniform gürültüde varyans düşük.
4. **Global + 2×2 bölgesel TOP-K** — tüm image skoru ile 2×2 grid top hücre skorunun max'ı. Büyük uniform arka planın metin içeren çeyreği ezmesini önler.
5. **Coverage gating** — foreground oranına göre multiplier. Metin tipik olarak %3–30 kaplamada yaşar; %45–55 neredeyse her zaman Otsu-split gürültü; çok düşük boş sayfa.

Final skor: **geometric mean** (√(edge × variance)), sonra coverage weight ile çarpılır. Geometric mean kasıtlı: tek sinyal başına buyruk olamaz.

## Doğruluk

Sentetik + gerçek karma validation seti (20 text + 14 non-text):

| Mod | Doğruluk | FN | FP |
|-----|----------|-----|-----|
| Conservative (default) | 34/34 (%100) | %0 | %0 |
| Aggressive | 31/34 (%91) | %15 | %0 |

Gerçek DOCX smoke test (7 gömülü image): 6 true positive, 1 borderline FP (yoğun app-icon tile). FN = 0.

> Kendi fixture'larınızı kullanın — text içeren görüntüleri `testset/positive/`, içermeyenleri `testset/negative/` altına koyup `cargo run --release --example bench` çalıştırın. `gen_negatives` örneği başlangıç için `testset/negative/`'i sentetik solid-color/gradient/logo şekilleriyle doldurur.

**Conservative mod FN=0 hedefler.** Yanlışlık "gereksiz Tesseract çağrıldı" yönünde — metin kaybolmaz.

## Dürüst kenar durumları

- **Dikey CJK yazım.** Algoritma yatay satır varsayar; geleneksel yukarıdan-aşağı dikey düzenler düşük skor alabilir. Yatay yazılmış CJK sorunsuz.
- **El yazısı / kaligrafi.** Geniş test edilmedi. Serbest yazıda doğruluk beklenenden düşük olabilir.
- **İkon grid'leri.** Yoğun app-icon sayfaları yüksek skor alabilir (kenar-ağır) — kabul edilebilir FP, insan için de sınır.
- **Ağır JPEG gürültüsü.** Çok gürültülü düşük-çöz JPEG'lerde kenar sayımı şişebilir. Conservative yine metni yakalar ama bir miktar gürültüyü geçirebilir.

Tanımadığınız korpüs için: `cargo run --release --example bench -- --positive senin/dizin --negative senin/dizin` ile skor dağılımını görmeden threshold'a bağlanmayın.

## Benchmark'lar

```bash
cargo run --release --example bench_pixels
cargo run --release --example bench
cargo run --release --example bench -- --mode aggressive
cargo run --release --example gen_negatives
```

Modern laptop referansı (Conservative, release build):

| Girdi | Süre |
|-------|------|
| `has_text_pixels` 256×256 | ~40 µs |
| `has_text_pixels` 2480×3508 (iç subsample) | ~300 µs |
| `has_text_rgb` 2480×3508 (RGB→gray + subsample) | ~500 µs |
| `has_text` JPEG encoded bytes | 1–20 ms (decode bağımlı) |

## OCR motorları ile entegrasyon

`ocr-triage` OCR **değil** — ön filtre. Gerçek kullanım:

```rust
for image in batch {
    let verdict = ocr_triage::has_text(&image);
    if verdict.has_text {
        let text = tesseract::ocr(&image)?;
        collect(text);
    }
}
```

Tesseract, PaddleOCR, RapidOCR, Azure/AWS OCR API veya kendi motorunuz ile doğal eşleşir. Kreuzberg döküman çıkarım pipeline'ı için hazır adapter: [`kreuzberg-ocr-triage`](https://github.com/hasantr/kreuzberg-text-triage).

## Güvenlik ve bağımlılıklar

- Saf Rust, kütüphane kodunda `unsafe` yok.
- Bağımlılıklar: [`image`](https://crates.io/crates/image) (default feature off, yalnızca jpeg/png/webp/tiff/bmp), [`zune-jpeg`](https://crates.io/crates/zune-jpeg), [`zune-core`](https://crates.io/crates/zune-core) (hızlı JPEG luma yolu).
- C/C++ link yok. Linux, macOS, Windows ve cross-target'larda temiz build.
- Tüm public fonksiyonlar adversarial input karşısında panic-free (boş, truncated header, imkansız boyut).

## Uyumluluk

- Minimum Rust: **1.75**.
- `no_std`: şimdilik yok (`std::time::Instant` + subsample `Vec`). PR açık.

## Lisans

Aşağıdakilerden biri, senin seçimin:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

### Katkı

Aksini açıkça belirtmediğiniz sürece, bu esere dahil edilmek üzere bilerek gönderdiğiniz her katkı — Apache-2.0 lisansında tanımlandığı şekliyle — ek bir koşul olmadan yukarıdaki gibi dual-license edilmiş sayılır.

## Teşekkür

- Tasarım ve bakım: **Hasan Salihoğlu**.
- Algoritma implementasyonu ve dokümantasyonu **Claude (Opus 4.6, Anthropic)** ile birlikte yazıldı.
