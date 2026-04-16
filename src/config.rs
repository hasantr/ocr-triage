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
    /// Validation set üstünde Positive minimum 0.269 (Courier-font screenshot),
    /// Negative maximum 0.169 → 0.25 güvenli orta. Üstte ~0.08 marj var.
    pub fn conservative() -> Self {
        TriageConfig {
            threshold: 0.25,
            thumbnail_short_edge: 256,
        }
    }

    /// FP minimize — yüksek eşik, sadece kesin text görünenler geçer.
    /// CPU tavanda batch processing için; bir miktar FN tolere edilir.
    pub fn aggressive() -> Self {
        TriageConfig {
            threshold: 0.40,
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
