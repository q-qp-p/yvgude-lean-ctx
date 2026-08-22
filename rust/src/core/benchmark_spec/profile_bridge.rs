use super::types::{
    BenchmarkConfiguration, BenchmarkSpecV1, BenchmarkSuite, BenchmarkTask, TaskKind,
};
use crate::core::profiles;
use crate::core::profiles::Profile;

pub(crate) fn configuration_from_profile(profile: &Profile) -> BenchmarkConfiguration {
    BenchmarkConfiguration {
        profile_hash: Some(profile_hash(profile)),
        agent: None,
        model: None,
        runtime_version: env!("CARGO_PKG_VERSION").to_string(),
        repeats: 1,
        quality_floor: profile.constraints.quality_floor_effective(),
    }
}

pub(crate) fn create_spec(
    profile_name: &str,
    suite: BenchmarkSuite,
) -> anyhow::Result<BenchmarkSpecV1> {
    let profile = profiles::load_profile(profile_name)
        .ok_or_else(|| anyhow::anyhow!("profile '{profile_name}' not found"))?;
    let config = configuration_from_profile(&profile);

    Ok(BenchmarkSpecV1 {
        id: format!("leanbench-{profile_name}"),
        version: "1.0.0".into(),
        name: format!("LeanBench — {profile_name}"),
        description: format!("Benchmark with profile '{profile_name}'"),
        suite,
        configuration: config,
        created_at: chrono_stub(),
    })
}

pub(crate) fn default_coding_suite() -> BenchmarkSuite {
    BenchmarkSuite {
        kind: super::types::BenchmarkKind::TaskScore,
        tasks: vec![
            BenchmarkTask {
                id: "explore-repo".into(),
                name: "Explore repository".into(),
                description: "Navigate and understand the codebase structure".into(),
                kind: TaskKind::Explore,
                timeout_ms: Some(120_000),
                evaluation: None,
            },
            BenchmarkTask {
                id: "locate-regression".into(),
                name: "Locate regression".into(),
                description: "Find the source of a known regression".into(),
                kind: TaskKind::LocateRegression,
                timeout_ms: Some(180_000),
                evaluation: None,
            },
            BenchmarkTask {
                id: "fix-bug".into(),
                name: "Fix bug".into(),
                description: "Diagnose and fix a reported bug".into(),
                kind: TaskKind::FixBug,
                timeout_ms: Some(300_000),
                evaluation: None,
            },
            BenchmarkTask {
                id: "run-tests".into(),
                name: "Run tests".into(),
                description: "Execute the test suite and interpret results".into(),
                kind: TaskKind::RunTests,
                timeout_ms: Some(120_000),
                evaluation: None,
            },
            BenchmarkTask {
                id: "explain-arch".into(),
                name: "Explain architecture".into(),
                description: "Produce a clear architectural summary".into(),
                kind: TaskKind::ExplainArchitecture,
                timeout_ms: Some(120_000),
                evaluation: None,
            },
        ],
    }
}

fn profile_hash(profile: &Profile) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let json = serde_json::to_string(profile).unwrap_or_default();
    json.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn chrono_stub() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coder_profile() -> Profile {
        profiles::load_profile("coder").expect("builtin coder profile must exist")
    }

    #[test]
    fn configuration_uses_profile_constraints() {
        let mut profile = coder_profile();
        profile.constraints.quality_floor = Some(0.92);
        let config = configuration_from_profile(&profile);
        assert!((config.quality_floor - 0.92).abs() < f64::EPSILON);
        assert!(config.profile_hash.is_some());
    }

    #[test]
    fn configuration_defaults_without_constraints() {
        let profile = coder_profile();
        let config = configuration_from_profile(&profile);
        assert!((config.quality_floor - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn default_suite_has_five_tasks() {
        let suite = default_coding_suite();
        assert_eq!(suite.tasks.len(), 5);
        assert_eq!(suite.tasks[0].id, "explore-repo");
        assert_eq!(suite.tasks[4].id, "explain-arch");
    }

    #[test]
    fn create_spec_with_builtin_profile() {
        let suite = default_coding_suite();
        let spec = create_spec("coder", suite).unwrap();
        assert_eq!(spec.id, "leanbench-coder");
        assert_eq!(spec.suite.tasks.len(), 5);
        assert!(spec.configuration.profile_hash.is_some());
    }

    #[test]
    fn create_spec_unknown_profile_errors() {
        let suite = default_coding_suite();
        assert!(create_spec("nonexistent-profile-xyz", suite).is_err());
    }

    #[test]
    fn profile_hash_is_deterministic() {
        let profile = coder_profile();
        let h1 = profile_hash(&profile);
        let h2 = profile_hash(&profile);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }
}
