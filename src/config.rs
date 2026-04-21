#[derive(Debug, Clone, Copy)]
pub enum TriageMode {
    /// FN minimize — text kaçırma riski düşük, FP yüksek olabilir.
    Conservative,
    /// FP minimize — CPU tavanda kullanım için, kesin text görünenler geçer.
    Aggressive,
}

#[derive(Debug, Clone, Copy)]
pub struct TriageConfig {
    pub threshold: f32,
    pub thumbnail_short_edge: u32,
}

impl TriageConfig {
    /// FN minimize — düşük eşik, text kaçırma riski en aza, FP biraz tolere.
    ///
    /// v0.4.0 kalibrasyonu (4×4 regional + vertical analiz sonrası):
    /// Validation set üstünde Positive minimum 0.381 (Courier 48px screenshot),
    /// Negative maximum 0.278 (geometric logo edge'leri) → 0.32 güvenli orta.
    /// Her iki tarafta ~0.05 margin — v0.3.0'ın 0.019 Courier margin'ından
    /// 2.7× daha dayanıklı.
    pub fn conservative() -> Self {
        TriageConfig {
            threshold: 0.32,
            thumbnail_short_edge: 256,
        }
    }

    /// FP minimize — yüksek eşik, sadece kesin text görünenler geçer.
    /// CPU tavanda batch processing için; bir miktar FN tolere edilir.
    ///
    /// v0.4.0 kalibrasyonu: 0.40 → 0.50 (tüm skorlar boost'lanmış durumda).
    pub fn aggressive() -> Self {
        TriageConfig {
            threshold: 0.50,
            thumbnail_short_edge: 256,
        }
    }

    pub fn from_mode(mode: TriageMode) -> Self {
        match mode {
            TriageMode::Conservative => Self::conservative(),
            TriageMode::Aggressive => Self::aggressive(),
        }
    }
}

impl Default for TriageConfig {
    fn default() -> Self {
        Self::conservative()
    }
}
