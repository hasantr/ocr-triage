use crate::simd;
use image::GrayImage;

/// Score entry from a decoded `GrayImage` — wraps the raw-slice path.
pub fn compute(img: &GrayImage) -> f32 {
    let (w, h) = img.dimensions();
    compute_raw(img.as_raw(), w, h)
}

/// Score entry from a raw grayscale buffer (length = `width * height`).
///
/// Pipeline:
///  1. **Otsu threshold** — image'in kendi histogram'ından foreground/background
///     ayrımı çıkarır. Polarity-invariant: siyah zeminde altın metin de, beyaz
///     zeminde siyah metin de aynı binary "ink-on-bg" temsiline indirgenir.
///  2. **Edge density** — binary üstünde yatay stroke kenarı sayımı.
///  3. **Projection variance** — binary üstünde satır-bazlı foreground kaplama
///     varyansı. Text satırları ile satır-araları arasında keskin fark üretir;
///     smooth noise'da her satır benzer kaplamaya sahip → düşük variance.
///  4. **Global + 2×2 regional TOP-2** — uniform arka plan dolgunun (CD kapak
///     frame'i vb.) küçük metin bölgesini seyreltmesinden korunur.
///
/// Geometric mean (√(edge × variance)) korunur — tek sinyalle tetiklenme yok.
pub fn compute_raw(gray: &[u8], width: u32, height: u32) -> f32 {
    if width == 0 || height == 0 {
        return 0.0;
    }
    if gray.len() != (width as usize) * (height as usize) {
        return 0.0;
    }

    // Binarize: Otsu ile image-adaptive foreground mask üret.
    // Konvansiyon: foreground (ink) = 1, background = 0.
    let binary = otsu_binarize(gray);

    let global = score_block(&binary, width, height, 0, 0, width, height);
    // 4×4 TOP-K=2: sparse text (image'ın <%10'unda yoğunlaşmış metin) için
    // 2×2 dilüsyon'u aşar. Cell başına 1/4 alan, metin-yoğun tek cell'in skoru
    // 3× daha yüksek çıkar. Uniform pattern'lerde (icon grid, QR) tüm cell'ler
    // benzer olduğundan top-k=2 aynı kalır, yeni FP üretmez.
    let regional = regional_top_k(&binary, width, height, 4, 2);
    let raw_score = global.max(regional * 0.9);

    // Coverage filter: foreground oranı "text" aralığında mı?
    //
    //   Text belgeleri:   %3 - %30 foreground (bg dominant, ink seyrek)
    //   Noise (random):   %45 - %55 (Otsu her iki tarafı dengeli böler)
    //   Solid büyük obje: %40 - %80 (logo, blob, tek renkli dolgu)
    //   Neredeyse boş:    < %2 (Otsu gürültüyü threshold eder)
    //
    // Sabit [0,1] aralıklı bir multiplier uygulanır; hard cutoff yok çünkü
    // koyu-zemin CD kapak gibi mixed content skora zarar vermesin.
    let fg_coverage = simd::sum_u8(&binary) as f32 / binary.len().max(1) as f32;
    let coverage_factor = coverage_weight(fg_coverage);

    raw_score * coverage_factor
}

/// Foreground kaplama oranına göre skor multiplier'ı.
///  - Sweet spot (text aralığı %3-30): tam skor
///  - Çok düşük / çok yüksek: skor agresif düşer
fn coverage_weight(fg: f32) -> f32 {
    if fg < 0.01 {
        return 0.20;
    } // neredeyse boş — güvenilmez
    if fg < 0.03 {
        return 0.50;
    } // çok seyrek — belki mini logo
    if fg <= 0.30 {
        return 1.00;
    } // text aralığı
    if fg <= 0.38 {
        return 0.70;
    } // dense text veya grafik
    if fg <= 0.45 {
        return 0.40;
    } // şüpheli — grafik/noise sınırı
    if fg <= 0.55 {
        return 0.15;
    } // noise range (Otsu dengeli böler)
    if fg <= 0.70 {
        return 0.30;
    } // solid büyük obje
    0.20 // çok yoğun — muhtemelen dolgu
}

/// Otsu threshold — histogram-based between-class variance maximizer.
/// Siyah zemin + beyaz yazı, ya da beyaz zemin + siyah yazı ayrımı fark etmez;
/// foreground (daha az) her zaman 1, background (daha çok) her zaman 0 olur.
fn otsu_binarize(gray: &[u8]) -> Vec<u8> {
    // Histogram
    let mut hist = [0u32; 256];
    for &p in gray {
        hist[p as usize] += 1;
    }
    let total = gray.len() as f64;
    let sum_all: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();

    // Optimal threshold arama
    let mut best_thr = 128u8;
    let mut best_var = 0.0f64;
    let mut w_bg = 0.0f64;
    let mut sum_bg = 0.0f64;
    for t in 0..=255u8 {
        w_bg += hist[t as usize] as f64;
        if w_bg == 0.0 {
            continue;
        }
        let w_fg = total - w_bg;
        if w_fg <= 0.0 {
            break;
        }
        sum_bg += t as f64 * hist[t as usize] as f64;
        let mean_bg = sum_bg / w_bg;
        let mean_fg = (sum_all - sum_bg) / w_fg;
        let var = w_bg * w_fg * (mean_bg - mean_fg) * (mean_bg - mean_fg);
        if var > best_var {
            best_var = var;
            best_thr = t;
        }
    }

    // Hangi sınıf "foreground"? Sayısı az olan. Normalde karanlık (metin).
    // Ama siyah zeminde açık metin ise threshold üstündekiler az olur → onları fg işaretle.
    let below = hist[..=best_thr as usize].iter().sum::<u32>();
    let above = (gray.len() as u32) - below;
    let fg_is_below = below <= above;

    let mut out = vec![0u8; gray.len()];
    simd::binarize(gray, &mut out, best_thr, fg_is_below);
    out
}

/// 2x2 grid TOP-K scoring — uniform arka plan dolgunun küçük metin bölgesini
/// ezmemesi için. TOP-K ortalaması, tek hücredeki rastlantısal outlier'ları
/// yumuşatır.
fn regional_top_k(binary: &[u8], width: u32, height: u32, grid: u32, top_k: usize) -> f32 {
    let cw = width / grid;
    let ch = height / grid;
    if cw < 16 || ch < 16 {
        return 0.0;
    }

    let mut scores: Vec<f32> = Vec::with_capacity((grid * grid) as usize);
    for gy in 0..grid {
        for gx in 0..grid {
            let x0 = gx * cw;
            let y0 = gy * ch;
            let cell_w = if gx + 1 == grid { width - x0 } else { cw };
            let cell_h = if gy + 1 == grid { height - y0 } else { ch };
            scores.push(score_block(binary, width, height, x0, y0, cell_w, cell_h));
        }
    }
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = top_k.min(scores.len());
    if k == 0 {
        return 0.0;
    }
    scores.iter().take(k).sum::<f32>() / k as f32
}

/// Belirli dikdörtgen bölgede edge × variance skoru (binary input).
///
/// Hem yatay (horizontal) hem dikey (vertical) orientation'lar için ayrı ayrı
/// edge density + projection variance hesaplanır, geometric mean (√(edge·var))
/// alınır; sonra iki yön skorlarının max'ı döner.
///
/// - Horizontal path (latin, kiril, arap, yunan, hebrew, Devanagari): h_score
///   yüksek, v_score düşük → max = h_score (eski davranış korunur).
/// - Vertical path (geleneksel CJK dikey yazı, rotated scan'lar): h_score
///   düşük, v_score yüksek → max = v_score (yeni kapsam).
/// - Uniform noise / solid / gradient: hepsi düşük → max düşük (FP değil).
/// - Vertical stripes: v_edge yüksek ama coverage_weight 45-55% bandında
///   agresif penalize edildiği için global'de filtrelenir.
#[inline]
fn score_block(binary: &[u8], stride: u32, _height: u32, x0: u32, y0: u32, w: u32, h: u32) -> f32 {
    let edge_h = horizontal_edge_density_block(binary, stride, x0, y0, w, h);
    let var_h = projection_variance_block(binary, stride, x0, y0, w, h);
    let h_score = (edge_h * var_h).sqrt();

    let edge_v = vertical_edge_density_block(binary, stride, x0, y0, w, h);
    let var_v = vertical_projection_variance_block(binary, stride, x0, y0, w, h);
    let v_score = (edge_v * var_v).sqrt();

    // Vertical'a %15 discount: script'lerin %95'i yatay, vertical sadece CJK
    // geleneksel dikey ve rotated scan için. Discount CJK'yı (v_score ≥ 0.5+)
    // rahatça eşik üstünde tutarken, geometrik şekillerin (logo outline vb.)
    // yanlışlıkla yüksek v_score'la FP olmasını engeller.
    h_score.max(v_score * 0.85)
}

/// Binary üstünde yatay foreground/background transition sayısı.
/// Binary'de transition kesin (0→1 veya 1→0), gri arayüz yok.
fn horizontal_edge_density_block(
    binary: &[u8],
    stride: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
) -> f32 {
    if w < 2 || h < 1 {
        return 0.0;
    }
    let stride = stride as usize;
    let x0 = x0 as usize;
    let y0 = y0 as usize;
    let w = w as usize;
    let h = h as usize;
    let mut edges = 0u32;
    let total = ((w - 1) * h) as u32;
    for yy in 0..h {
        let row_start = (y0 + yy) * stride + x0;
        let row = &binary[row_start..row_start + w];
        edges += simd::count_transitions(row);
    }
    let raw_density = edges as f32 / total.max(1) as f32;
    // Binary'de tipik text image'da 0.05-0.20 aralığı.
    (raw_density / 0.16).min(1.0)
}

/// Binary üstünde satır-bazlı foreground kaplama varyansı.
/// Text satırı: kaplama ~0.20-0.40. Text-arası: ~0.00. Yüksek varyans.
/// Noise: her satır ~0.50, düşük varyans.
fn projection_variance_block(binary: &[u8], stride: u32, x0: u32, y0: u32, w: u32, h: u32) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    let stride = stride as usize;
    let x0 = x0 as usize;
    let y0 = y0 as usize;
    let w = w as usize;
    let h = h as usize;
    let mut row_cov = Vec::with_capacity(h);
    for yy in 0..h {
        let row_start = (y0 + yy) * stride + x0;
        let row = &binary[row_start..row_start + w];
        let fg: u32 = row.iter().map(|&p| p as u32).sum();
        row_cov.push(fg as f32 / w as f32);
    }
    let n = row_cov.len() as f32;
    let mean = row_cov.iter().copied().sum::<f32>() / n;
    let var = row_cov.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / n;
    // std çoğu text'te 0.10-0.25 aralığında.
    (var.sqrt() / 0.18).min(1.0)
}

/// Binary üstünde dikey foreground/background transition sayısı.
/// Her x için y→y+1 geçişlerini sayar. Dikey yazılan CJK karakterlerde,
/// rotated scan'larda, icon satırlarında yüksek olur.
///
/// Implementation: adjacent scanline pair'ları arası `simd::xor_sum`.
/// Binary input'ta xor_sum == mismatch sayısı.
fn vertical_edge_density_block(
    binary: &[u8],
    stride: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
) -> f32 {
    if w < 1 || h < 2 {
        return 0.0;
    }
    let stride = stride as usize;
    let x0 = x0 as usize;
    let y0 = y0 as usize;
    let w = w as usize;
    let h = h as usize;
    let mut edges = 0u32;
    let total = (w * (h - 1)) as u32;
    for yy in 0..h - 1 {
        let a_start = (y0 + yy) * stride + x0;
        let b_start = (y0 + yy + 1) * stride + x0;
        let row_a = &binary[a_start..a_start + w];
        let row_b = &binary[b_start..b_start + w];
        edges += crate::simd::xor_sum(row_a, row_b);
    }
    // Normalization horizontal ile aynı — text'te tipik 0.05-0.20 aralığı.
    (edges as f32 / total.max(1) as f32 / 0.16).min(1.0)
}

/// Binary üstünde sütun-bazlı foreground kaplama varyansı.
/// Dikey yazı: sütun kaplama alternasyonu (glyph sütunu vs dikey boşluk) →
/// yüksek varyans. Yatay text: sütunlar benzer kaplama → düşük varyans.
/// Uniform stripe'lar: hep-dolu ve hep-boş sütunların karışımı → çok yüksek
/// varyans ama coverage_weight (45-55% bandı) filtrelemesi bu case'i keser.
fn vertical_projection_variance_block(
    binary: &[u8],
    stride: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    let stride = stride as usize;
    let x0 = x0 as usize;
    let y0 = y0 as usize;
    let w = w as usize;
    let h = h as usize;
    // Per-column FG count'u tek pas accumulate et. Stride access cache-unfriendly
    // ama 256×256 buffer L2'ye sığar, toplam ~150 µs.
    let mut col_count = vec![0u32; w];
    for yy in 0..h {
        let row_start = (y0 + yy) * stride + x0;
        let row = &binary[row_start..row_start + w];
        for (xx, &b) in row.iter().enumerate() {
            col_count[xx] += b as u32;
        }
    }
    let h_f = h as f32;
    let n = w as f32;
    let mean: f32 = col_count.iter().map(|&c| c as f32 / h_f).sum::<f32>() / n;
    let var: f32 = col_count
        .iter()
        .map(|&c| {
            let r = c as f32 / h_f;
            (r - mean).powi(2)
        })
        .sum::<f32>()
        / n;
    (var.sqrt() / 0.18).min(1.0)
}
