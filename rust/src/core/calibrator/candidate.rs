use super::config::CalibrationConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CandidateProfile {
    pub id: String,
    pub label: String,
    pub budget_tokens: usize,
    pub compression: String,
    pub reuse_threshold: f64,
    pub capability_variant: String,
}

pub(crate) fn generate_candidates(config: &CalibrationConfig) -> Vec<CandidateProfile> {
    let budget_steps = distribute_steps(config.budget_range.0, config.budget_range.1, 4);
    let reuse_steps = distribute_reuse(config.reuse_range.0, config.reuse_range.1, 3);
    let mut candidates = Vec::new();
    let mut count = 0usize;
    for &budget in &budget_steps {
        for compression in &config.compression_levels {
            for &reuse in &reuse_steps {
                for variant in &config.capability_variants {
                    if candidates.len() >= config.max_candidates {
                        return candidates;
                    }
                    count += 1;
                    candidates.push(CandidateProfile {
                        id: format!("candidate-{count:03}"),
                        label: format!(
                            "{variant}/{compression}/{}k/r{:.0}",
                            budget / 1000,
                            reuse * 100.0
                        ),
                        budget_tokens: budget,
                        compression: compression.clone(),
                        reuse_threshold: reuse,
                        capability_variant: variant.clone(),
                    });
                }
            }
        }
    }
    candidates
}

fn distribute_steps(min: usize, max: usize, count: usize) -> Vec<usize> {
    if count <= 1 {
        return vec![min];
    }
    let step = (max - min) / (count - 1);
    (0..count).map(|i| min + step * i).collect()
}

fn distribute_reuse(min: f64, max: f64, count: usize) -> Vec<f64> {
    if count <= 1 {
        return vec![min];
    }
    let step = (max - min) / (count - 1) as f64;
    (0..count).map(|i| min + step * i as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_within_max() {
        let config = CalibrationConfig {
            max_candidates: 10,
            ..CalibrationConfig::default()
        };
        let candidates = generate_candidates(&config);
        assert!(candidates.len() <= 10);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn unique_ids() {
        let candidates = generate_candidates(&CalibrationConfig::default());
        let ids: Vec<_> = candidates.iter().map(|c| &c.id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn budget_within_range() {
        let config = CalibrationConfig::default();
        for c in &generate_candidates(&config) {
            assert!(c.budget_tokens >= config.budget_range.0);
            assert!(c.budget_tokens <= config.budget_range.1);
        }
    }
}
