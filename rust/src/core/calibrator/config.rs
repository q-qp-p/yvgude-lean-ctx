use serde::{Deserialize, Serialize};

/// Calibration bounds with conservative defaults for a representative, bounded search.
///
/// [`Default`] evaluates at most 20 profiles across 16k–128k token budgets,
/// three compression levels, and reuse thresholds from 70% to 95%. The 95%
/// quality floor favors profiles suitable for production use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CalibrationConfig {
    pub quality_floor: f64,
    pub max_candidates: usize,
    pub budget_range: (usize, usize),
    pub compression_levels: Vec<String>,
    pub reuse_range: (f64, f64),
    pub capability_variants: Vec<String>,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            quality_floor: 0.95,
            max_candidates: 20,
            budget_range: (16_000, 128_000),
            compression_levels: vec!["lossless".into(), "balanced".into(), "aggressive".into()],
            reuse_range: (0.70, 0.95),
            capability_variants: vec!["leanctx".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let cfg = CalibrationConfig::default();
        assert!((cfg.quality_floor - 0.95).abs() < f64::EPSILON);
        assert_eq!(cfg.max_candidates, 20);
        assert_eq!(cfg.budget_range, (16_000, 128_000));
        assert_eq!(cfg.compression_levels.len(), 3);
        assert_eq!(cfg.reuse_range, (0.70, 0.95));
        assert_eq!(cfg.capability_variants, ["leanctx"]);
    }

    #[test]
    fn config_roundtrip() {
        let cfg = CalibrationConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: CalibrationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_candidates, cfg.max_candidates);
    }
}
