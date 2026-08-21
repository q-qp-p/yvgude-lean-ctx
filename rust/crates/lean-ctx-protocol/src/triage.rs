//! Task triage and profile contracts.

use crate::{
    RiskClass, TaskComplexity,
    common::{
        ValidationError, deserialize_milliunit, deserialize_schema_version, validate_milliunit,
        validate_schema_version,
    },
};
use serde::{Deserialize, Serialize};

/// Scope at which a task's work is expected to occur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScope {
    #[default]
    SingleFile,
    MultiFile,
    CrossModule,
    CrossProject,
}

/// Backend used to produce a triage result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageBackend {
    #[default]
    Rules,
    Semantic,
    Hybrid,
}

/// Classified task profile used by routing and context planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProfileV1 {
    pub primary_intent: String,
    pub task_class: String,
    pub complexity: TaskComplexity,
    pub scope: TaskScope,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub context_need_milli: u16,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub reasoning_need_milli: u16,
    pub risk_signal: RiskClass,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub confidence_milli: u16,
    /// Capability that produced this profile, when the producer is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    /// Version of the capability that produced this profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_version: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub language_hints: Vec<String>,
}

impl Default for TaskProfileV1 {
    fn default() -> Self {
        Self {
            primary_intent: String::new(),
            task_class: String::new(),
            complexity: TaskComplexity::Low,
            scope: TaskScope::SingleFile,
            context_need_milli: 0,
            reasoning_need_milli: 0,
            risk_signal: RiskClass::Low,
            confidence_milli: 0,
            capability_id: None,
            capability_version: None,
            keywords: Vec::new(),
            language_hints: Vec::new(),
        }
    }
}

impl TaskProfileV1 {
    /// Validate profile fields and milliunit bounds.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_milliunit(self.context_need_milli, "context_need_milli")?;
        validate_milliunit(self.reasoning_need_milli, "reasoning_need_milli")?;
        validate_milliunit(self.confidence_milli, "confidence_milli")
    }
}

/// Auditable result produced by task triage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageResultV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub triage_result_id: String,
    pub task_id: String,
    pub profile: TaskProfileV1,
    #[serde(deserialize_with = "deserialize_milliunit")]
    pub confidence_milli: u16,
    pub backend: TriageBackend,
    pub analyzer_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

impl Default for TriageResultV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            triage_result_id: String::new(),
            task_id: String::new(),
            profile: TaskProfileV1::default(),
            confidence_milli: 0,
            backend: TriageBackend::Rules,
            analyzer_version: String::new(),
            model_ref: None,
            degraded_reason: None,
        }
    }
}

impl TriageResultV1 {
    /// Schema version represented by this type.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Validate result schema, nested profile, and confidence bounds.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.profile.validate()?;
        validate_milliunit(self.confidence_milli, "confidence_milli")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> TaskProfileV1 {
        TaskProfileV1 {
            primary_intent: "implement".to_owned(),
            task_class: "coding".to_owned(),
            complexity: TaskComplexity::Medium,
            scope: TaskScope::CrossModule,
            context_need_milli: 700,
            reasoning_need_milli: 800,
            risk_signal: RiskClass::Low,
            confidence_milli: 900,
            capability_id: Some("capability://leanctx/triage".to_owned()),
            capability_version: Some("1.0.0".to_owned()),
            keywords: vec!["rust".to_owned(), "protocol".to_owned()],
            language_hints: vec!["rust".to_owned()],
        }
    }

    fn valid_result() -> TriageResultV1 {
        TriageResultV1 {
            schema_version: 1,
            triage_result_id: "triage-1".to_owned(),
            task_id: "task-1".to_owned(),
            profile: valid_profile(),
            confidence_milli: 850,
            backend: TriageBackend::Hybrid,
            analyzer_version: "rules-1".to_owned(),
            model_ref: Some("model-1".to_owned()),
            degraded_reason: None,
        }
    }

    #[test]
    fn serialization_round_trip() {
        let result = valid_result();
        let json = serde_json::to_string(&result).expect("triage result should serialize");
        let decoded: TriageResultV1 =
            serde_json::from_str(&json).expect("triage result should deserialize");
        assert_eq!(result, decoded);
        result.validate().expect("triage result should be valid");
    }

    #[test]
    fn profile_capability_metadata_is_optional_and_backward_compatible() {
        let profile = valid_profile();
        let json = serde_json::to_value(&profile).expect("profile should serialize");
        assert_eq!(json["capability_id"], "capability://leanctx/triage");
        assert_eq!(
            serde_json::from_value::<TaskProfileV1>(json.clone())
                .expect("profile with capability metadata should deserialize"),
            profile
        );

        let mut legacy = json;
        let object = legacy
            .as_object_mut()
            .expect("serialized profile should be an object");
        object.remove("capability_id");
        object.remove("capability_version");

        let decoded: TaskProfileV1 =
            serde_json::from_value(legacy).expect("legacy profile should deserialize");
        assert_eq!(decoded.capability_id, None);
        assert_eq!(decoded.capability_version, None);

        let without_capability = serde_json::to_value(decoded)
            .expect("profile without capability metadata should serialize");
        assert!(without_capability.get("capability_id").is_none());
        assert!(without_capability.get("capability_version").is_none());
    }

    #[test]
    fn validation_rejects_invalid_schema_and_milliunits() {
        let mut result = valid_result();
        result.schema_version = 2;
        assert!(result.validate().is_err());

        result.schema_version = 1;
        result.confidence_milli = 1001;
        assert!(result.validate().is_err());

        let mut profile = valid_profile();
        profile.context_need_milli = 1001;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn defaults_are_stable_and_valid() {
        let profile = TaskProfileV1::default();
        assert_eq!(profile, TaskProfileV1::default());
        profile.validate().expect("default profile should be valid");

        let result = TriageResultV1::default();
        assert_eq!(result, TriageResultV1::default());
        result.validate().expect("default result should be valid");
        assert_eq!(TaskScope::default(), TaskScope::SingleFile);
        assert_eq!(TriageBackend::default(), TriageBackend::Rules);
    }
}
