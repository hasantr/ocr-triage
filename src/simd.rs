//! SIMD primitives — binarize, count_transitions, sum_u8.
//!
//! Runtime CPU dispatch on x86/x86_64 (AVX2 → SSE2 → scalar).
//! Compile-time NEON on aarch64 (baseline by ABI).
//! Scalar fallback on any other target.
//!
//! Kernels are deliberately structured 1:1 with `ocr-triage-c/src/simd.c`
//! so that parity with the C implementation is auditable line-for-line.

#![allow(clippy::missing_safety_doc)]

// ------------------------------------------------------------
// binarize: dst[i] = (below_is_fg ^ (gray[i] > threshold)) as u8
// ------------------------------------------------------------

pub fn binarize(gray: &[u8], dst: &mut [u8], threshold: u8, below_is_fg: bool) {
    debug_assert_eq!(gray.len(), dst.len());

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                return binarize_avx2(gray, dst, threshold, below_is_fg);
            }
        }
        if is_x86_feature_detected!("sse2") {
            unsafe {
                return binarize_sse2(gray, dst, threshold, below_is_fg);
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            return binarize_neon(gray, dst, threshold, below_is_fg);
        }
    }

    #[allow(unreachable_code)]
    binarize_scalar(gray, dst, threshold, below_is_fg);
}

fn binarize_scalar(gray: &[u8], dst: &mut [u8], threshold: u8, below_is_fg: bool) {
    if below_is_fg {
        for (g, d) in gray.iter().zip(dst.iter_mut()) {
            *d = if *g <= threshold { 1 } else { 0 };
        }
    } else {
        for (g, d) in gray.iter().zip(dst.iter_mut()) {
            *d = if *g > threshold { 1 } else { 0 };
        }
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
unsafe fn binarize_avx2(gray: &[u8], dst: &mut [u8], threshold: u8, below_is_fg: bool) {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let n = gray.len();
    let bias = _mm256_set1_epi8(0x80u8 as i8);
    let one = _mm256_set1_epi8(1);
    let thr_v = _mm256_set1_epi8((threshold ^ 0x80) as i8);

    let mut i = 0usize;
    if below_is_fg {
        while i + 32 <= n {
            let g = _mm256_loadu_si256(gray.as_ptr().add(i) as *const __m256i);
            let gb = _mm256_xor_si256(g, bias);
            let gt = _mm256_cmpgt_epi8(gb, thr_v); // 0xFF if gray > threshold (unsigned)
            let le = _mm256_andnot_si256(gt, one); // 1 if gray <= threshold
            _mm256_storeu_si256(dst.as_mut_ptr().add(i) as *mut __m256i, le);
            i += 32;
        }
    } else {
        while i + 32 <= n {
            let g = _mm256_loadu_si256(gray.as_ptr().add(i) as *const __m256i);
            let gb = _mm256_xor_si256(g, bias);
            let gt = _mm256_cmpgt_epi8(gb, thr_v);
            let m = _mm256_and_si256(gt, one);
            _mm256_storeu_si256(dst.as_mut_ptr().add(i) as *mut __m256i, m);
            i += 32;
        }
    }
    if i < n {
        binarize_scalar(&gray[i..], &mut dst[i..], threshold, below_is_fg);
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
unsafe fn binarize_sse2(gray: &[u8], dst: &mut [u8], threshold: u8, below_is_fg: bool) {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let n = gray.len();
    let bias = _mm_set1_epi8(0x80u8 as i8);
    let one = _mm_set1_epi8(1);
    let thr_v = _mm_set1_epi8((threshold ^ 0x80) as i8);

    let mut i = 0usize;
    if below_is_fg {
        while i + 16 <= n {
            let g = _mm_loadu_si128(gray.as_ptr().add(i) as *const __m128i);
            let gb = _mm_xor_si128(g, bias);
            let gt = _mm_cmpgt_epi8(gb, thr_v);
            let le = _mm_andnot_si128(gt, one);
            _mm_storeu_si128(dst.as_mut_ptr().add(i) as *mut __m128i, le);
            i += 16;
        }
    } else {
        while i + 16 <= n {
            let g = _mm_loadu_si128(gray.as_ptr().add(i) as *const __m128i);
            let gb = _mm_xor_si128(g, bias);
            let gt = _mm_cmpgt_epi8(gb, thr_v);
            let m = _mm_and_si128(gt, one);
            _mm_storeu_si128(dst.as_mut_ptr().add(i) as *mut __m128i, m);
            i += 16;
        }
    }
    if i < n {
        binarize_scalar(&gray[i..], &mut dst[i..], threshold, below_is_fg);
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn binarize_neon(gray: &[u8], dst: &mut [u8], threshold: u8, below_is_fg: bool) {
    use std::arch::aarch64::*;
    let n = gray.len();
    let thr_v = vdupq_n_u8(threshold);
    let one = vdupq_n_u8(1);
    let mut i = 0usize;
    if below_is_fg {
        while i + 16 <= n {
            let g = vld1q_u8(gray.as_ptr().add(i));
            let le = vcleq_u8(g, thr_v); // 0xFF if <=, else 0
            vst1q_u8(dst.as_mut_ptr().add(i), vandq_u8(le, one));
            i += 16;
        }
    } else {
        while i + 16 <= n {
            let g = vld1q_u8(gray.as_ptr().add(i));
            let gt = vcgtq_u8(g, thr_v);
            vst1q_u8(dst.as_mut_ptr().add(i), vandq_u8(gt, one));
            i += 16;
        }
    }
    if i < n {
        binarize_scalar(&gray[i..], &mut dst[i..], threshold, below_is_fg);
    }
}

// ------------------------------------------------------------
// count_transitions: sum_{i=0..len-1} (row[i] != row[i+1])
// Binary-only input (values ∈ {0,1}). XOR adjacent bytes → {0,1}
// byte stream, sum = transition count.
// ------------------------------------------------------------

pub fn count_transitions(row: &[u8]) -> u32 {
    if row.len() < 2 {
        return 0;
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { count_transitions_avx2(row) };
        }
        if is_x86_feature_detected!("sse2") {
            return unsafe { count_transitions_sse2(row) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { count_transitions_neon(row) };
    }

    #[allow(unreachable_code)]
    count_transitions_scalar(row)
}

fn count_transitions_scalar(row: &[u8]) -> u32 {
    let mut edges = 0u32;
    for x in 0..row.len() - 1 {
        if row[x] != row[x + 1] {
            edges += 1;
        }
    }
    edges
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
unsafe fn count_transitions_avx2(row: &[u8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let len = row.len();
    let pair_count = len - 1;
    let mut acc = _mm256_setzero_si256();
    let zero = _mm256_setzero_si256();

    let mut x = 0usize;
    while x + 32 <= pair_count {
        let a = _mm256_loadu_si256(row.as_ptr().add(x) as *const __m256i);
        let b = _mm256_loadu_si256(row.as_ptr().add(x + 1) as *const __m256i);
        let d = _mm256_xor_si256(a, b); // binary input → 0/1 per byte
        let sad = _mm256_sad_epu8(d, zero); // 4×u64 partial sums
        acc = _mm256_add_epi64(acc, sad);
        x += 32;
    }

    let mut tmp = [0u64; 4];
    _mm256_storeu_si256(tmp.as_mut_ptr() as *mut __m256i, acc);
    let mut total = (tmp[0] + tmp[1] + tmp[2] + tmp[3]) as u32;

    while x + 1 < len {
        if row[x] != row[x + 1] {
            total += 1;
        }
        x += 1;
    }
    total
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
unsafe fn count_transitions_sse2(row: &[u8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let len = row.len();
    let pair_count = len - 1;
    let mut acc = _mm_setzero_si128();
    let zero = _mm_setzero_si128();

    let mut x = 0usize;
    while x + 16 <= pair_count {
        let a = _mm_loadu_si128(row.as_ptr().add(x) as *const __m128i);
        let b = _mm_loadu_si128(row.as_ptr().add(x + 1) as *const __m128i);
        let d = _mm_xor_si128(a, b);
        let sad = _mm_sad_epu8(d, zero); // 2×u64 partial sums
        acc = _mm_add_epi64(acc, sad);
        x += 16;
    }

    let mut tmp = [0u64; 2];
    _mm_storeu_si128(tmp.as_mut_ptr() as *mut __m128i, acc);
    let mut total = (tmp[0] + tmp[1]) as u32;

    while x + 1 < len {
        if row[x] != row[x + 1] {
            total += 1;
        }
        x += 1;
    }
    total
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn count_transitions_neon(row: &[u8]) -> u32 {
    use std::arch::aarch64::*;
    let len = row.len();
    let pair_count = len - 1;
    let mut acc = vdupq_n_u32(0);
    let mut x = 0usize;
    while x + 16 <= pair_count {
        let a = vld1q_u8(row.as_ptr().add(x));
        let b = vld1q_u8(row.as_ptr().add(x + 1));
        let d = veorq_u8(a, b);
        // pairwise widening add u8→u16→u32
        let s16 = vpaddlq_u8(d);
        let s32 = vpaddlq_u16(s16);
        acc = vaddq_u32(acc, s32);
        x += 16;
    }
    let mut total = vaddvq_u32(acc);
    while x + 1 < len {
        if row[x] != row[x + 1] {
            total += 1;
        }
        x += 1;
    }
    total
}

// ------------------------------------------------------------
// sum_u8: Σ buf[i]
// ------------------------------------------------------------

pub fn sum_u8(buf: &[u8]) -> u32 {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { sum_u8_avx2(buf) };
        }
        if is_x86_feature_detected!("sse2") {
            return unsafe { sum_u8_sse2(buf) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { sum_u8_neon(buf) };
    }

    #[allow(unreachable_code)]
    sum_u8_scalar(buf)
}

fn sum_u8_scalar(buf: &[u8]) -> u32 {
    let mut s = 0u32;
    for &b in buf {
        s += b as u32;
    }
    s
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
unsafe fn sum_u8_avx2(buf: &[u8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let n = buf.len();
    let mut acc = _mm256_setzero_si256();
    let zero = _mm256_setzero_si256();
    let mut i = 0usize;
    while i + 32 <= n {
        let v = _mm256_loadu_si256(buf.as_ptr().add(i) as *const __m256i);
        let sad = _mm256_sad_epu8(v, zero);
        acc = _mm256_add_epi64(acc, sad);
        i += 32;
    }
    let mut tmp = [0u64; 4];
    _mm256_storeu_si256(tmp.as_mut_ptr() as *mut __m256i, acc);
    let mut total = (tmp[0] + tmp[1] + tmp[2] + tmp[3]) as u32;
    while i < n {
        total += buf[i] as u32;
        i += 1;
    }
    total
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
unsafe fn sum_u8_sse2(buf: &[u8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let n = buf.len();
    let mut acc = _mm_setzero_si128();
    let zero = _mm_setzero_si128();
    let mut i = 0usize;
    while i + 16 <= n {
        let v = _mm_loadu_si128(buf.as_ptr().add(i) as *const __m128i);
        let sad = _mm_sad_epu8(v, zero);
        acc = _mm_add_epi64(acc, sad);
        i += 16;
    }
    let mut tmp = [0u64; 2];
    _mm_storeu_si128(tmp.as_mut_ptr() as *mut __m128i, acc);
    let mut total = (tmp[0] + tmp[1]) as u32;
    while i < n {
        total += buf[i] as u32;
        i += 1;
    }
    total
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn sum_u8_neon(buf: &[u8]) -> u32 {
    use std::arch::aarch64::*;
    let n = buf.len();
    let mut acc = vdupq_n_u32(0);
    let mut i = 0usize;
    while i + 16 <= n {
        let v = vld1q_u8(buf.as_ptr().add(i));
        let s16 = vpaddlq_u8(v);
        let s32 = vpaddlq_u16(s16);
        acc = vaddq_u32(acc, s32);
        i += 16;
    }
    let mut total = vaddvq_u32(acc);
    while i < n {
        total += buf[i] as u32;
        i += 1;
    }
    total
}

// ------------------------------------------------------------
// CPU feature string (for telemetry / bench reporting)
// ------------------------------------------------------------

pub fn active_isa() -> &'static str {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("avx2") {
            return "avx2";
        }
        if is_x86_feature_detected!("sse2") {
            return "sse2";
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        return "neon";
    }
    #[allow(unreachable_code)]
    "scalar"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_binarize(gray: &[u8], threshold: u8, below_is_fg: bool) -> Vec<u8> {
        let mut out = vec![0u8; gray.len()];
        binarize_scalar(gray, &mut out, threshold, below_is_fg);
        out
    }

    #[test]
    fn binarize_matches_scalar_various_lengths() {
        for &n in &[0usize, 1, 7, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 256, 1000, 4096, 65_537]
        {
            let gray: Vec<u8> = (0..n).map(|i| ((i * 37) % 256) as u8).collect();
            for &thr in &[0u8, 1, 64, 127, 128, 200, 255] {
                for &below in &[true, false] {
                    let expected = scalar_binarize(&gray, thr, below);
                    let mut actual = vec![0u8; n];
                    binarize(&gray, &mut actual, thr, below);
                    assert_eq!(
                        actual, expected,
                        "binarize mismatch n={} thr={} below={}",
                        n, thr, below
                    );
                }
            }
        }
    }

    #[test]
    fn count_transitions_matches_scalar() {
        for &n in &[0usize, 1, 2, 16, 17, 31, 32, 33, 63, 64, 65, 1000, 65_537] {
            // Binary input only.
            let row: Vec<u8> = (0..n).map(|i| ((i * 5 + 1) & 1) as u8).collect();
            let expected = if n < 2 { 0 } else { count_transitions_scalar(&row) };
            let actual = count_transitions(&row);
            assert_eq!(actual, expected, "count_transitions mismatch n={}", n);
        }
    }

    #[test]
    fn sum_u8_matches_scalar() {
        for &n in &[0usize, 1, 15, 16, 17, 31, 32, 33, 100, 1000, 65_537] {
            let buf: Vec<u8> = (0..n).map(|i| ((i * 13 + 7) % 256) as u8).collect();
            let expected = sum_u8_scalar(&buf);
            let actual = sum_u8(&buf);
            assert_eq!(actual, expected, "sum_u8 mismatch n={}", n);
        }
    }

    #[test]
    fn active_isa_is_known() {
        assert!(matches!(active_isa(), "avx2" | "sse2" | "neon" | "scalar"));
    }
}
