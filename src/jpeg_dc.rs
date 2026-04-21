//! DC-only baseline JPEG decoder — üretir 1/8 grayscale thumbnail.
//!
//! Amaç: triage için full IDCT yapmadan, sadece her 8×8 DCT bloğunun DC
//! katsayısını okuyup doğrudan thumbnail pikseli üretmek. AC katsayılarının
//! Huffman çözümlemesi bitstream'i ilerletmek için yapılır ama değerleri
//! kullanılmaz. Cb/Cr tamamen atlanır.
//!
//! Desteklenen:
//!   - Baseline sequential DCT (SOF0) — tam scan decode (DC + AC skip)
//!   - Progressive DCT (SOF2) — sadece ilk DC scan'ı (Ss=0, Se=0, Ah=0);
//!     sonraki AC scan'ları görmezden gelir. Al point transform geri şifre
//!     edilir. Refinement scan'ları işlenmez — triage için high-bit DC yeter.
//!   - 8-bit samples
//!   - 1 veya 3 komponent (grayscale ya da YCbCr)
//!   - 4:4:4, 4:2:2, 4:2:0, 4:1:1 chroma subsampling
//!   - Restart markers (DRI)
//!   - Standard JFIF dosyaları
//!   - Interleaved ve non-interleaved progressive DC scan
//!
//! Düşülen durumlar (`None` döner, caller fallback'e geçer):
//!   - SOF1 (extended), SOF3 (lossless)
//!   - 16-bit precision
//!   - 2 veya 4+ komponent (RGB, CMYK)
//!   - Progressive'de ilk scan DC-only değilse
//!   - Malformed / truncated bitstream
//!
//! Tüm public API'ler panic-free — adversarial input güvenli.

// ---------- Marker kodları ----------
const M_SOI: u8 = 0xD8;
#[allow(dead_code)]
const M_EOI: u8 = 0xD9;
const M_SOF0: u8 = 0xC0;
const M_SOF1: u8 = 0xC1;
const M_SOF2: u8 = 0xC2;
const M_DHT: u8 = 0xC4;
const M_DQT: u8 = 0xDB;
const M_DRI: u8 = 0xDD;
const M_SOS: u8 = 0xDA;

// APP0..APP15: 0xE0..0xEF
// COM: 0xFE
// RSTn: 0xD0..0xD7 (bitstream içinde)

// ---------- Public API ----------

/// JPEG bytes → 1/8 grayscale thumbnail. `None` dönerse format baseline
/// değil ya da bozuk; caller fallback decoder'a düşer.
pub fn decode_dc_thumbnail(bytes: &[u8]) -> Option<DcThumbnail> {
    let mut reader = ByteReader::new(bytes);

    // SOI zorunlu.
    if reader.read_u8()? != 0xFF || reader.read_u8()? != M_SOI {
        return None;
    }

    let mut frame: Option<FrameHeader> = None;
    let mut qtables: [Option<[u16; 64]>; 4] = [None, None, None, None];
    let mut dc_tables: [Option<HuffTable>; 4] = [None, None, None, None];
    let mut ac_tables: [Option<HuffTable>; 4] = [None, None, None, None];
    let mut restart_interval: u16 = 0;

    // Marker loop until SOS.
    loop {
        // Align to next marker: skip any 0xFF fill bytes, then marker byte.
        let marker = read_marker(&mut reader)?;
        match marker {
            M_SOF0 => {
                frame = Some(parse_sof_any(&mut reader, false)?);
            }
            M_SOF2 => {
                frame = Some(parse_sof_any(&mut reader, true)?);
            }
            M_SOF1 => {
                // Extended huffman — fallback.
                return None;
            }
            M_DQT => parse_dqt(&mut reader, &mut qtables)?,
            M_DHT => parse_dht(&mut reader, &mut dc_tables, &mut ac_tables)?,
            M_DRI => restart_interval = parse_dri(&mut reader)?,
            M_SOS => break,
            // APPn, COM, diğer her şey: length-prefixed, atla.
            0xE0..=0xEF | 0xFE | 0xC3..=0xCF | 0xD8..=0xDF => {
                skip_segment(&mut reader)?;
            }
            _ => {
                // Unknown marker — try to skip as length-prefixed segment.
                skip_segment(&mut reader)?;
            }
        }
    }

    let frame = frame?;
    let scan = parse_sos(&mut reader, &frame)?;

    // Entropy-coded segment starts where reader.pos is.
    let ecs = &bytes[reader.pos..];

    // Baseline sequential: Ss=0, Se=63, Ah=Al=0.
    let is_baseline_scan = scan.ss == 0 && scan.se == 63 && scan.ah == 0 && scan.al == 0;
    // Progressive ilk DC scan: Ss=0, Se=0, Ah=0.
    let is_progressive_dc_first = scan.ss == 0 && scan.se == 0 && scan.ah == 0;

    if !frame.is_progressive && is_baseline_scan {
        decode_baseline_scan(ecs, &frame, &scan, &qtables, &dc_tables, &ac_tables, restart_interval)
    } else if frame.is_progressive && is_progressive_dc_first {
        // Progressive JPEG: ilk scan DC-only olmalı. Sadece onu işleyip bail.
        // Sonraki AC ve DC-refinement scan'ları görmezden gel.
        decode_progressive_dc_scan(ecs, &frame, &scan, &qtables, &dc_tables, restart_interval)
    } else {
        // Unsupported: progressive first scan AC, or mid-stream DC refinement.
        None
    }
}

/// Decoded 1/8 thumbnail. `gray.len() == width * height`.
pub struct DcThumbnail {
    pub gray: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// ---------- Frame / scan structs ----------

#[derive(Clone, Copy)]
struct Component {
    id: u8,
    h_samp: u8,
    v_samp: u8,
    qtable_id: u8,
}

struct FrameHeader {
    width: u16,
    height: u16,
    components: Vec<Component>,
    h_max: u8,
    v_max: u8,
    // Y component index (in `components`). None ise YCbCr / grayscale değil.
    y_idx: usize,
    is_progressive: bool,
}

struct ScanComponent {
    comp_idx: usize, // index into frame.components
    dc_table: u8,
    ac_table: u8,
}

struct ScanHeader {
    components: Vec<ScanComponent>,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
}

// ---------- Marker parsing ----------

fn read_marker(r: &mut ByteReader) -> Option<u8> {
    // İlk byte 0xFF olmalı; değilse malformed.
    let mut b = r.read_u8()?;
    if b != 0xFF {
        return None;
    }
    // Birden fazla 0xFF fill byte olabilir, marker byte'ını ara.
    while b == 0xFF {
        b = r.read_u8()?;
    }
    // 0x00 marker pozisyonunda yoktur.
    if b == 0x00 {
        return None;
    }
    Some(b)
}

fn parse_sof_any(r: &mut ByteReader, is_progressive: bool) -> Option<FrameHeader> {
    let len = r.read_u16_be()?;
    if len < 8 {
        return None;
    }
    let precision = r.read_u8()?;
    if precision != 8 {
        return None; // 16-bit not supported
    }
    let height = r.read_u16_be()?;
    let width = r.read_u16_be()?;
    let ncomp = r.read_u8()?;
    if ncomp != 1 && ncomp != 3 {
        return None; // gray or YCbCr only
    }
    let expected_len = 8u16 + (ncomp as u16) * 3;
    if len != expected_len {
        return None;
    }

    let mut components = Vec::with_capacity(ncomp as usize);
    let mut h_max = 1u8;
    let mut v_max = 1u8;
    for _ in 0..ncomp {
        let id = r.read_u8()?;
        let samp = r.read_u8()?;
        let h = samp >> 4;
        let v = samp & 0x0F;
        let qid = r.read_u8()?;
        if h == 0 || h > 4 || v == 0 || v > 4 || qid > 3 {
            return None;
        }
        if h > h_max {
            h_max = h;
        }
        if v > v_max {
            v_max = v;
        }
        components.push(Component {
            id,
            h_samp: h,
            v_samp: v,
            qtable_id: qid,
        });
    }

    // Grayscale: tek komponent → Y.
    // YCbCr: id'lerden bağımsız olarak sampling en büyük olan Y kabul edilir
    // (standartta 1=Y ama bazı encoder'lar başka ID kullanabilir; sampling
    // factor en büyük olan her zaman Y'dir).
    let y_idx = if components.len() == 1 {
        0
    } else {
        // En büyük sampling factor'a sahip komponent Y.
        let mut best = 0usize;
        let mut best_score = 0u16;
        for (i, c) in components.iter().enumerate() {
            let s = c.h_samp as u16 * c.v_samp as u16;
            if s > best_score {
                best_score = s;
                best = i;
            }
        }
        best
    };

    Some(FrameHeader {
        width,
        height,
        components,
        h_max,
        v_max,
        y_idx,
        is_progressive,
    })
}

fn parse_dqt(r: &mut ByteReader, tables: &mut [Option<[u16; 64]>; 4]) -> Option<()> {
    let len = r.read_u16_be()?;
    if len < 2 {
        return None;
    }
    let mut remaining = (len as i32) - 2;
    while remaining > 0 {
        let info = r.read_u8()?;
        let precision = info >> 4; // 0=8-bit, 1=16-bit
        let tid = (info & 0x0F) as usize;
        if tid > 3 {
            return None;
        }
        let mut q = [0u16; 64];
        if precision == 0 {
            for i in 0..64 {
                q[i] = r.read_u8()? as u16;
            }
            remaining -= 1 + 64;
        } else if precision == 1 {
            for i in 0..64 {
                q[i] = r.read_u16_be()?;
            }
            remaining -= 1 + 128;
        } else {
            return None;
        }
        tables[tid] = Some(q);
    }
    Some(())
}

fn parse_dht(
    r: &mut ByteReader,
    dc_tables: &mut [Option<HuffTable>; 4],
    ac_tables: &mut [Option<HuffTable>; 4],
) -> Option<()> {
    let len = r.read_u16_be()?;
    if len < 2 {
        return None;
    }
    let mut remaining = (len as i32) - 2;
    while remaining > 0 {
        let info = r.read_u8()?;
        let class = info >> 4; // 0=DC, 1=AC
        let tid = (info & 0x0F) as usize;
        if class > 1 || tid > 3 {
            return None;
        }
        let mut bits = [0u8; 16];
        for i in 0..16 {
            bits[i] = r.read_u8()?;
        }
        let total: u32 = bits.iter().map(|&b| b as u32).sum();
        if total > 256 {
            return None;
        }
        let mut huffval = Vec::with_capacity(total as usize);
        for _ in 0..total {
            huffval.push(r.read_u8()?);
        }
        remaining -= 1 + 16 + total as i32;

        let tbl = HuffTable::build(&bits, &huffval)?;
        if class == 0 {
            dc_tables[tid] = Some(tbl);
        } else {
            ac_tables[tid] = Some(tbl);
        }
    }
    Some(())
}

fn parse_dri(r: &mut ByteReader) -> Option<u16> {
    let len = r.read_u16_be()?;
    if len != 4 {
        return None;
    }
    r.read_u16_be()
}

fn parse_sos(r: &mut ByteReader, frame: &FrameHeader) -> Option<ScanHeader> {
    let len = r.read_u16_be()?;
    if len < 6 {
        return None;
    }
    let ncomp = r.read_u8()?;
    if ncomp == 0 || ncomp as usize > frame.components.len() {
        return None;
    }
    let expected_len = 6u16 + (ncomp as u16) * 2;
    if len != expected_len {
        return None;
    }

    let mut scan_components = Vec::with_capacity(ncomp as usize);
    for _ in 0..ncomp {
        let cid = r.read_u8()?;
        let tables = r.read_u8()?;
        let dc_t = tables >> 4;
        let ac_t = tables & 0x0F;
        if dc_t > 3 || ac_t > 3 {
            return None;
        }
        let comp_idx = frame.components.iter().position(|c| c.id == cid)?;
        scan_components.push(ScanComponent {
            comp_idx,
            dc_table: dc_t,
            ac_table: ac_t,
        });
    }

    let ss = r.read_u8()?;
    let se = r.read_u8()?;
    let ah_al = r.read_u8()?;
    let ah = ah_al >> 4;
    let al = ah_al & 0x0F;

    Some(ScanHeader {
        components: scan_components,
        ss,
        se,
        ah,
        al,
    })
}

fn skip_segment(r: &mut ByteReader) -> Option<()> {
    let len = r.read_u16_be()?;
    if len < 2 {
        return None;
    }
    r.skip((len as usize) - 2)
}

// ---------- Byte reader ----------

struct ByteReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        ByteReader { bytes, pos: 0 }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn read_u16_be(&mut self) -> Option<u16> {
        let hi = self.read_u8()?;
        let lo = self.read_u8()?;
        Some(((hi as u16) << 8) | (lo as u16))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos.checked_add(n)? > self.bytes.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }
}

// ---------- Huffman table ----------

struct HuffTable {
    // Canonical Huffman:
    // max_code[l]: last code of length l+1 (or -1 if empty)
    // val_offset[l]: offset into huffval for first symbol of length l+1
    max_code: [i32; 16],
    val_offset: [i32; 16],
    huffval: Vec<u8>,
    // Fast lookup: for codes ≤ 8 bits, direct table lookup (256 entries).
    // Each entry: (symbol, length_in_bits). length=0 means slow path.
    fast: [(u8, u8); 256],
}

impl HuffTable {
    fn build(bits: &[u8; 16], huffval: &[u8]) -> Option<Self> {
        // Generate codes in canonical order.
        let mut code = 0i32;
        let mut max_code = [-1i32; 16];
        let mut val_offset = [-1i32; 16];
        let mut offset = 0i32;
        let mut min_code_tmp = [-1i32; 16];
        for l in 0..16 {
            let n = bits[l] as i32;
            if n > 0 {
                val_offset[l] = offset - code;
                min_code_tmp[l] = code;
                max_code[l] = code + n - 1;
                code += n;
                offset += n;
            }
            code <<= 1;
        }
        let min_code = min_code_tmp;

        // Fast table for first 8 bits.
        let mut fast = [(0u8, 0u8); 256];
        let mut p = 0usize; // index into huffval
        for l in 0..8 {
            let n = bits[l] as usize;
            if n == 0 {
                continue;
            }
            let code_start = min_code[l] as u32;
            let remaining_bits = 7 - l;
            for k in 0..n {
                let sym = huffval[p + k];
                let code = (code_start + k as u32) << remaining_bits;
                let count = 1usize << remaining_bits;
                for j in 0..count {
                    let idx = (code + j as u32) as usize;
                    if idx < 256 {
                        fast[idx] = (sym, (l + 1) as u8);
                    }
                }
            }
            p += n;
        }

        let _ = min_code;
        Some(HuffTable {
            max_code,
            val_offset,
            huffval: huffval.to_vec(),
            fast,
        })
    }
}

// ---------- Bit reader over entropy-coded segment ----------

struct BitReader<'a> {
    bytes: &'a [u8],
    pos: usize,
    // bit_buf holds unread bits in high positions; bit_count is how many.
    bit_buf: u64,
    bit_count: u8,
    marker: Option<u8>, // if a marker (non-RST, non-0x00) was seen
    error: bool,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        BitReader {
            bytes,
            pos: 0,
            bit_buf: 0,
            bit_count: 0,
            marker: None,
            error: false,
        }
    }

    /// Feed bytes into bit_buf until at least 32 bits available (or end/marker).
    fn fill(&mut self) {
        while self.bit_count <= 24 {
            if self.pos >= self.bytes.len() {
                return;
            }
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == 0xFF {
                // Check next byte for stuffing or marker.
                if self.pos >= self.bytes.len() {
                    // Malformed — trailing 0xFF.
                    self.error = true;
                    return;
                }
                let next = self.bytes[self.pos];
                if next == 0x00 {
                    // Stuffed — treat as literal 0xFF.
                    self.pos += 1;
                } else {
                    // Marker encountered. Roll back; caller will handle.
                    self.pos -= 1;
                    self.marker = Some(next);
                    return;
                }
            }
            self.bit_buf |= (b as u64) << (56 - self.bit_count);
            self.bit_count += 8;
        }
    }

    /// Read `n` bits (n ≤ 16). Returns None on underflow.
    fn get_bits(&mut self, n: u8) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if self.bit_count < n {
            self.fill();
            if self.bit_count < n {
                return None;
            }
        }
        let result = (self.bit_buf >> (64 - n)) as u32;
        self.bit_buf <<= n;
        self.bit_count -= n;
        Some(result)
    }

    /// Decode one Huffman symbol.
    fn decode_huff(&mut self, tbl: &HuffTable) -> Option<u8> {
        // Fast path: peek 8 bits.
        if self.bit_count < 8 {
            self.fill();
        }
        if self.bit_count >= 8 {
            let peek = (self.bit_buf >> 56) as usize;
            let (sym, len) = tbl.fast[peek];
            if len > 0 && len <= 8 {
                self.bit_buf <<= len;
                self.bit_count -= len;
                return Some(sym);
            }
        }

        // Slow path: bit-by-bit for codes > 8 bits.
        let mut code = 0i32;
        for l in 0..16 {
            let bit = self.get_bits(1)? as i32;
            code = (code << 1) | bit;
            if code <= tbl.max_code[l] {
                let idx = (code + tbl.val_offset[l]) as usize;
                if idx >= tbl.huffval.len() {
                    return None;
                }
                return Some(tbl.huffval[idx]);
            }
        }
        None
    }

    /// Read `size` bits and sign-extend into a signed value.
    /// JPEG convention: positive range [2^(size-1), 2^size-1],
    /// negative range [-(2^size-1), -2^(size-1)].
    fn receive_extend(&mut self, size: u8) -> Option<i32> {
        if size == 0 {
            return Some(0);
        }
        let raw = self.get_bits(size)?;
        let half = 1u32 << (size - 1);
        let v = if raw < half {
            (raw as i32) - ((1i32 << size) - 1)
        } else {
            raw as i32
        };
        Some(v)
    }

    /// Align bit buffer to next byte boundary (for restart marker handling).
    fn byte_align(&mut self) {
        let unused = self.bit_count % 8;
        if unused > 0 {
            self.bit_buf <<= unused;
            self.bit_count -= unused;
        }
        // Discard anything left in bit_buf (shouldn't matter after restart).
        self.bit_buf = 0;
        self.bit_count = 0;
    }

    /// Consume a restart marker (FF D0..D7). Does nothing if not at marker.
    fn consume_restart(&mut self) -> Option<()> {
        self.byte_align();
        // If fill saw a marker, marker info is in self.marker.
        if self.marker.is_none() && self.pos + 1 < self.bytes.len() {
            // May need to skip the FF/marker pair.
            if self.bytes[self.pos] == 0xFF {
                let m = self.bytes[self.pos + 1];
                if (0xD0..=0xD7).contains(&m) {
                    self.pos += 2;
                    return Some(());
                }
            }
        }
        if let Some(m) = self.marker {
            if (0xD0..=0xD7).contains(&m) {
                self.pos += 2;
                self.marker = None;
                return Some(());
            }
        }
        None
    }
}

// ---------- Scan decode ----------

fn decode_baseline_scan(
    ecs: &[u8],
    frame: &FrameHeader,
    scan: &ScanHeader,
    qtables: &[Option<[u16; 64]>; 4],
    dc_tables: &[Option<HuffTable>; 4],
    ac_tables: &[Option<HuffTable>; 4],
    restart_interval: u16,
) -> Option<DcThumbnail> {
    // SOS'ta tüm frame komponentleri olmalı (baseline interleaved scan).
    if scan.components.len() != frame.components.len() {
        return None;
    }

    let y_comp = &frame.components[frame.y_idx];
    let q_y = qtables[y_comp.qtable_id as usize].as_ref()?;
    let q_y_dc = q_y[0] as i32;

    // MCU geometrisi.
    let mcu_w = 8 * frame.h_max as u32;
    let mcu_h = 8 * frame.v_max as u32;
    let num_mcu_x = (frame.width as u32).div_ceil(mcu_w);
    let num_mcu_y = (frame.height as u32).div_ceil(mcu_h);

    // Y block grid boyutu (thumbnail dimensions'a taban oluşturur).
    let y_blocks_per_mcu_x = y_comp.h_samp as u32;
    let y_blocks_per_mcu_y = y_comp.v_samp as u32;

    // Thumbnail: her Y bloğu = 1 piksel.
    let thumb_w = num_mcu_x * y_blocks_per_mcu_x;
    let thumb_h = num_mcu_y * y_blocks_per_mcu_y;
    let thumb_total = thumb_w.checked_mul(thumb_h)? as usize;
    // Aşırı büyüklüğe karşı güvenlik kapak — 64MP üstü muhtemelen bozuk.
    if thumb_total > 64 * 1024 * 1024 {
        return None;
    }

    // Her komponent için DC predictor (DPCM).
    let mut dc_pred = vec![0i32; frame.components.len()];

    let mut thumb = vec![128u8; thumb_total];

    let mut br = BitReader::new(ecs);
    let mut mcus_since_restart: u32 = 0;

    for my in 0..num_mcu_y {
        for mx in 0..num_mcu_x {
            // Restart marker handling.
            if restart_interval > 0 && mcus_since_restart == restart_interval as u32 {
                br.consume_restart()?;
                for p in dc_pred.iter_mut() {
                    *p = 0;
                }
                mcus_since_restart = 0;
            }

            // Her komponent için Hi × Vi blok decode et (baseline interleaved).
            for sc in &scan.components {
                let comp = &frame.components[sc.comp_idx];
                let dc_tbl = dc_tables[sc.dc_table as usize].as_ref()?;
                let ac_tbl = ac_tables[sc.ac_table as usize].as_ref()?;
                let is_y = sc.comp_idx == frame.y_idx;

                let h = comp.h_samp as u32;
                let v = comp.v_samp as u32;

                for by in 0..v {
                    for bx in 0..h {
                        // DC: symbol = size, then `size` bits of DC delta.
                        let sym = br.decode_huff(dc_tbl)?;
                        let size = sym & 0x0F;
                        if size > 15 {
                            return None;
                        }
                        let delta = br.receive_extend(size)?;
                        dc_pred[sc.comp_idx] += delta;

                        if is_y {
                            // DC coefficient → pixel value for this 8×8 block.
                            // Reconstruction: dc_coef × Q[0] / 8 + 128.
                            let recon = (dc_pred[sc.comp_idx] * q_y_dc) / 8 + 128;
                            let px = recon.clamp(0, 255) as u8;
                            let row = (my * y_blocks_per_mcu_y + by) as usize;
                            let col = (mx * y_blocks_per_mcu_x + bx) as usize;
                            let idx = row * thumb_w as usize + col;
                            if idx < thumb.len() {
                                thumb[idx] = px;
                            }
                        }

                        // AC: sembolleri decode et ama değerlerini kullanma.
                        // Sadece bitstream'i ilerletmek için `size` bit tüketilir.
                        let mut k = 1u32;
                        while k < 64 {
                            let acsym = br.decode_huff(ac_tbl)?;
                            if acsym == 0 {
                                // EOB — remaining AC = 0.
                                break;
                            }
                            let run = (acsym >> 4) as u32;
                            let sz = acsym & 0x0F;
                            if acsym == 0xF0 {
                                // ZRL: skip 16 zeros.
                                k += 16;
                                continue;
                            }
                            k += run;
                            if k >= 64 {
                                return None;
                            }
                            // AC coefficient bits — skip (value not used).
                            let _ = br.get_bits(sz)?;
                            k += 1;
                        }
                    }
                }
            }

            mcus_since_restart += 1;
            if br.error {
                return None;
            }
        }
    }

    // Trim to actual image dimensions (/8 rounded down).
    let real_w = (frame.width as u32).max(1).div_ceil(8);
    let real_h = (frame.height as u32).max(1).div_ceil(8);
    if real_w != thumb_w || real_h != thumb_h {
        // Thumbnail'da image MCU-padding var; gerçek boyuta kırp.
        let actual_w = real_w.min(thumb_w) as usize;
        let actual_h = real_h.min(thumb_h) as usize;
        let mut cropped = Vec::with_capacity(actual_w * actual_h);
        for y in 0..actual_h {
            let row_start = y * thumb_w as usize;
            cropped.extend_from_slice(&thumb[row_start..row_start + actual_w]);
        }
        return Some(DcThumbnail {
            gray: cropped,
            width: actual_w as u32,
            height: actual_h as u32,
        });
    }

    Some(DcThumbnail {
        gray: thumb,
        width: thumb_w,
        height: thumb_h,
    })
}

/// Progressive JPEG'in ilk DC scan'ını decode eder. Ss=Se=Ah=0, Al >= 0.
/// Her blok için sadece 1 DC sembolü okunur (AC yok), DC değeri `<< al`
/// ile yüksek bit pozisyonuna shift edilir (refinement scan'lar alçak
/// bitleri doldurur, onları görmezden geliriz).
///
/// Scan interleaved olabilir (tüm komponentler, MCU = H×V blok per comp)
/// veya non-interleaved (tek komponent, MCU = 1 blok).
fn decode_progressive_dc_scan(
    ecs: &[u8],
    frame: &FrameHeader,
    scan: &ScanHeader,
    qtables: &[Option<[u16; 64]>; 4],
    dc_tables: &[Option<HuffTable>; 4],
    restart_interval: u16,
) -> Option<DcThumbnail> {
    let y_comp = &frame.components[frame.y_idx];
    let q_y = qtables[y_comp.qtable_id as usize].as_ref()?;
    let q_y_dc = q_y[0] as i32;
    let al = scan.al;

    // Scan interleaved mı? Standart: ncomp > 1 iff interleaved.
    let interleaved = scan.components.len() > 1;

    // Thumbnail geometrisi Y grid'ine dayanır.
    let thumb_w = (frame.width as u32).div_ceil(8);
    let thumb_h = (frame.height as u32).div_ceil(8);
    let thumb_total = thumb_w.checked_mul(thumb_h)? as usize;
    if thumb_total > 64 * 1024 * 1024 {
        return None;
    }
    let mut thumb = vec![128u8; thumb_total];

    // Y scan komponenti bu scan'da var mı?
    let y_in_scan = scan.components.iter().any(|sc| sc.comp_idx == frame.y_idx);
    if !y_in_scan {
        // Scan Y içermiyorsa (sadece Cb veya Cr) triage için faydasız.
        return None;
    }

    // MCU boyutu.
    let (mcu_w_blocks, mcu_h_blocks, num_mcu_x, num_mcu_y) = if interleaved {
        // Interleaved: MCU = H_max × V_max pixels × 8, per komponent H_i × V_i blok.
        let mcu_w = 8 * frame.h_max as u32;
        let mcu_h = 8 * frame.v_max as u32;
        let nx = (frame.width as u32).div_ceil(mcu_w);
        let ny = (frame.height as u32).div_ceil(mcu_h);
        (frame.h_max as u32, frame.v_max as u32, nx, ny)
    } else {
        // Non-interleaved: MCU = 1 blok (8×8 pixel) in this component.
        // Pixel genişlik = (comp.h_samp / h_max) × image_width, ama blok grid'i daha basit:
        // bu komponent için blok sayısı.
        let comp = &frame.components[scan.components[0].comp_idx];
        let blocks_x = (frame.width as u32 * comp.h_samp as u32).div_ceil(8 * frame.h_max as u32);
        let blocks_y = (frame.height as u32 * comp.v_samp as u32).div_ceil(8 * frame.v_max as u32);
        (1, 1, blocks_x, blocks_y)
    };

    let mut dc_pred = vec![0i32; frame.components.len()];
    let mut br = BitReader::new(ecs);
    let mut mcus_since_restart: u32 = 0;

    for my in 0..num_mcu_y {
        for mx in 0..num_mcu_x {
            if restart_interval > 0 && mcus_since_restart == restart_interval as u32 {
                br.consume_restart()?;
                for p in dc_pred.iter_mut() {
                    *p = 0;
                }
                mcus_since_restart = 0;
            }

            for sc in &scan.components {
                let comp = &frame.components[sc.comp_idx];
                let dc_tbl = dc_tables[sc.dc_table as usize].as_ref()?;
                let is_y = sc.comp_idx == frame.y_idx;

                let (h, v) = if interleaved {
                    (comp.h_samp as u32, comp.v_samp as u32)
                } else {
                    (1, 1)
                };

                for by in 0..v {
                    for bx in 0..h {
                        let sym = br.decode_huff(dc_tbl)?;
                        let size = sym & 0x0F;
                        if size > 15 {
                            return None;
                        }
                        let delta = br.receive_extend(size)?;
                        dc_pred[sc.comp_idx] += delta;

                        if is_y {
                            let dc = dc_pred[sc.comp_idx] << al;
                            let recon = (dc * q_y_dc) / 8 + 128;
                            let px = recon.clamp(0, 255) as u8;
                            let (row, col) = if interleaved {
                                (
                                    (my * mcu_h_blocks + by) as usize,
                                    (mx * mcu_w_blocks + bx) as usize,
                                )
                            } else {
                                (my as usize, mx as usize)
                            };
                            let idx = row * thumb_w as usize + col;
                            if idx < thumb.len() {
                                thumb[idx] = px;
                            }
                        }
                    }
                }
            }

            mcus_since_restart += 1;
            if br.error {
                return None;
            }
        }
    }

    Some(DcThumbnail {
        gray: thumb,
        width: thumb_w,
        height: thumb_h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_input() {
        assert!(decode_dc_thumbnail(&[]).is_none());
    }

    #[test]
    fn rejects_non_jpeg() {
        assert!(decode_dc_thumbnail(b"not a jpeg at all at all at all").is_none());
    }

    #[test]
    fn rejects_jpeg_missing_sof() {
        // Just an SOI, no structure.
        assert!(decode_dc_thumbnail(&[0xFF, 0xD8, 0xFF, 0xD9]).is_none());
    }
}
