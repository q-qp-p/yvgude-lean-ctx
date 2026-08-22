//! Typed, immutable Context SDK session metadata and checkpoint state.

use crate::{
    AgentId, KitId, PlanId, PolicyId, ProfileId, ProjectId, ProtocolReference, ReceiptId, RunId,
    SemanticVersion, SessionId, Sha256Digest, TaskId, TenantId, TraceId, UtcTimestamp,
    V1_SCHEMA_VERSION, ValidationError, WorkspaceId, deserialize_schema_version,
    validate_schema_version,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Immutable identity established when a Context SDK session is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIdentityV1 {
    pub session_id: SessionId,
    pub task_id: TaskId,
    pub run_id: RunId,
    pub trace_id: TraceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<TaskId>,
    pub agent_id: AgentId,
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<TenantId>,
    pub project_root_ref: ProtocolReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_revision_ref: Option<ProtocolReference>,
    pub created_at: UtcTimestamp,
}

impl SessionIdentityV1 {
    /// Validate the identity invariants shared by direct Rust construction and decoding.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.parent_task_id.as_ref() == Some(&self.task_id) {
            return Err(ValidationError::new(
                "SessionIdentityV1 parent_task_id must not equal task_id",
            ));
        }
        Ok(())
    }
}

/// Exact immutable identity of a TuningProfile resolved for a session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningProfilePinV1 {
    pub profile_id: ProfileId,
    pub version: SemanticVersion,
    pub content_digest: Sha256Digest,
    pub source_ref: ProtocolReference,
}

/// Exact immutable identity of a Context Kit resolved for a session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextKitPinV1 {
    pub kit_id: KitId,
    pub version: SemanticVersion,
    pub package_digest: Sha256Digest,
    pub activation_ref: ProtocolReference,
}

/// Integration coverage claimed by a session, never an agent-hosting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSdkIntegrationDepthV1 {
    Attach,
    Wrap,
    Embed,
}

/// Frozen configuration inputs that may affect Context SDK behavior or evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSessionConfigurationV1 {
    pub tuning_profile: TuningProfilePinV1,
    #[serde(default)]
    pub kits: Vec<ContextKitPinV1>,
    #[serde(default)]
    pub policy_refs: Vec<PolicyId>,
    pub integration_depth: ContextSdkIntegrationDepthV1,
}

impl ContextSessionConfigurationV1 {
    /// Validate uniqueness and collision invariants for immutable resolved pins.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut kit_digests = BTreeMap::new();
        for kit in &self.kits {
            let logical_identity = (kit.kit_id.clone(), kit.version.clone());
            if let Some(existing_digest) = kit_digests.insert(logical_identity, &kit.package_digest)
            {
                if existing_digest == &kit.package_digest {
                    return Err(ValidationError::new(
                        "ContextSessionConfigurationV1 contains a duplicate Context Kit pin",
                    ));
                }
                return Err(ValidationError::new(
                    "ContextSessionConfigurationV1 detects a Context Kit logical identity collision",
                ));
            }
        }

        let mut policy_refs = BTreeSet::new();
        if self
            .policy_refs
            .iter()
            .any(|policy_ref| !policy_refs.insert(policy_ref))
        {
            return Err(ValidationError::new(
                "ContextSessionConfigurationV1 contains a duplicate policy_ref",
            ));
        }
        Ok(())
    }
}

/// Lifecycle phase for a persisted Context SDK session snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSessionPhaseV1 {
    Created,
    Configured,
    Executing,
    ReceiptReady,
    Closed,
    Aborted,
}

/// Result of checking whether a persisted session can safely resume execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSessionRecoveryStateV1 {
    Resumable,
    ResumableWithDegradation,
    InspectOnly,
    Corrupt,
}

/// Mutable checkpoint projection; it never replaces the append-only event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSessionStateV1 {
    pub phase: ContextSessionPhaseV1,
    pub revision: u64,
    pub next_event_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_plan_id: Option<PlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checkpoint_digest: Option<Sha256Digest>,
    pub recovery_state: ContextSessionRecoveryStateV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<ProtocolReference>,
}

impl ContextSessionStateV1 {
    /// Validate lifecycle fields without relying on a mutable runtime implementation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.next_event_sequence < self.revision {
            return Err(ValidationError::new(
                "ContextSessionStateV1 next_event_sequence must not precede revision",
            ));
        }
        if self.phase == ContextSessionPhaseV1::ReceiptReady && self.receipt_id.is_none() {
            return Err(ValidationError::new(
                "ContextSessionStateV1 receipt_ready requires a receipt_id",
            ));
        }
        if !matches!(
            self.phase,
            ContextSessionPhaseV1::ReceiptReady
                | ContextSessionPhaseV1::Closed
                | ContextSessionPhaseV1::Aborted
        ) && self.receipt_id.is_some()
        {
            return Err(ValidationError::new(
                "ContextSessionStateV1 receipt_id is allowed only for receipt_ready, closed, or aborted",
            ));
        }
        if matches!(
            self.phase,
            ContextSessionPhaseV1::Created
                | ContextSessionPhaseV1::Configured
                | ContextSessionPhaseV1::Aborted
        ) && self.active_plan_id.is_some()
        {
            return Err(ValidationError::new(
                "ContextSessionStateV1 active_plan_id requires an executing or terminal configured session",
            ));
        }
        if self.phase == ContextSessionPhaseV1::Aborted && self.abort_reason.is_none() {
            return Err(ValidationError::new(
                "ContextSessionStateV1 aborted requires an abort_reason",
            ));
        }
        if self.phase != ContextSessionPhaseV1::Aborted && self.abort_reason.is_some() {
            return Err(ValidationError::new(
                "ContextSessionStateV1 abort_reason is allowed only for aborted",
            ));
        }
        if matches!(
            self.recovery_state,
            ContextSessionRecoveryStateV1::InspectOnly | ContextSessionRecoveryStateV1::Corrupt
        ) && self.phase == ContextSessionPhaseV1::Executing
        {
            return Err(ValidationError::new(
                "ContextSessionStateV1 inspect_only or corrupt state cannot execute",
            ));
        }
        Ok(())
    }
}

/// Portable V1 metadata/checkpoint contract for one Context SDK root task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSessionSnapshotV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub identity: SessionIdentityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ContextSessionConfigurationV1>,
    pub state: ContextSessionStateV1,
}

impl ContextSessionSnapshotV1 {
    /// Schema version represented by this Context SDK session snapshot.
    pub const SCHEMA_VERSION: u32 = V1_SCHEMA_VERSION;

    /// Validate schema, pin, and lifecycle invariants for a portable snapshot.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_schema_version(self.schema_version)?;
        self.identity.validate()?;
        if let Some(configuration) = &self.configuration {
            configuration.validate()?;
        }
        self.state.validate()?;

        let configuration_required = matches!(
            self.state.phase,
            ContextSessionPhaseV1::Configured
                | ContextSessionPhaseV1::Executing
                | ContextSessionPhaseV1::ReceiptReady
        );
        if configuration_required && self.configuration.is_none() {
            return Err(ValidationError::new(
                "ContextSessionSnapshotV1 configured, executing, and receipt_ready phases require configuration",
            ));
        }
        if self.state.phase == ContextSessionPhaseV1::Created && self.configuration.is_some() {
            return Err(ValidationError::new(
                "ContextSessionSnapshotV1 created phase must not contain configuration",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const DIGEST_A: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn id<T>(value: &str) -> T
    where
        T: TryFrom<String>,
        <T as TryFrom<String>>::Error: std::fmt::Debug,
    {
        T::try_from(value.to_owned()).expect("test identity should be valid")
    }

    fn reference(value: &str) -> ProtocolReference {
        ProtocolReference::new(value).expect("test reference should be valid")
    }

    fn digest(value: &str) -> Sha256Digest {
        Sha256Digest::new(value).expect("test digest should be valid")
    }

    fn identity() -> SessionIdentityV1 {
        SessionIdentityV1 {
            session_id: id("session-1"),
            task_id: id("task-1"),
            run_id: id("run-1"),
            trace_id: id("trace-1"),
            parent_task_id: None,
            agent_id: id("agent-1"),
            project_id: id("project-1"),
            workspace_id: Some(id("workspace-1")),
            tenant_id: Some(id("tenant-1")),
            project_root_ref: reference("project:root-redacted"),
            project_revision_ref: Some(reference("git:abcdef")),
            created_at: UtcTimestamp::new("2026-08-22T10:20:30Z").expect("valid timestamp"),
        }
    }

    fn configuration() -> ContextSessionConfigurationV1 {
        ContextSessionConfigurationV1 {
            tuning_profile: TuningProfilePinV1 {
                profile_id: id("acme/payments-review"),
                version: SemanticVersion::new("1.4.2").expect("valid semver"),
                content_digest: digest(DIGEST_A),
                source_ref: reference("profile:acme/payments-review"),
            },
            kits: vec![ContextKitPinV1 {
                kit_id: id("payments-security"),
                version: SemanticVersion::new("2.1.0").expect("valid semver"),
                package_digest: digest(DIGEST_B),
                activation_ref: reference("activation:payments-security"),
            }],
            policy_refs: vec![id("policy:local-only")],
            integration_depth: ContextSdkIntegrationDepthV1::Wrap,
        }
    }

    fn snapshot() -> ContextSessionSnapshotV1 {
        ContextSessionSnapshotV1 {
            schema_version: 1,
            identity: identity(),
            configuration: Some(configuration()),
            state: ContextSessionStateV1 {
                phase: ContextSessionPhaseV1::Executing,
                revision: 3,
                next_event_sequence: 4,
                active_plan_id: Some(id("plan-1")),
                receipt_id: None,
                last_checkpoint_digest: Some(digest(DIGEST_A)),
                recovery_state: ContextSessionRecoveryStateV1::Resumable,
                abort_reason: None,
            },
        }
    }

    #[test]
    fn golden_snapshot_round_trips_with_exact_pins() {
        let snapshot = snapshot();
        snapshot.validate().expect("snapshot must validate");
        let value = serde_json::to_value(&snapshot).expect("snapshot serializes");
        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "identity": {
                    "session_id": "session-1", "task_id": "task-1", "run_id": "run-1",
                    "trace_id": "trace-1", "agent_id": "agent-1", "project_id": "project-1",
                    "workspace_id": "workspace-1", "tenant_id": "tenant-1",
                    "project_root_ref": "project:root-redacted", "project_revision_ref": "git:abcdef",
                    "created_at": "2026-08-22T10:20:30Z"
                },
                "configuration": {
                    "tuning_profile": {
                        "profile_id": "acme/payments-review", "version": "1.4.2",
                        "content_digest": DIGEST_A, "source_ref": "profile:acme/payments-review"
                    },
                    "kits": [{
                        "kit_id": "payments-security", "version": "2.1.0",
                        "package_digest": DIGEST_B, "activation_ref": "activation:payments-security"
                    }],
                    "policy_refs": ["policy:local-only"],
                    "integration_depth": "wrap"
                },
                "state": {
                    "phase": "executing", "revision": 3, "next_event_sequence": 4,
                    "active_plan_id": "plan-1", "last_checkpoint_digest": DIGEST_A,
                    "recovery_state": "resumable"
                }
            })
        );
        let decoded: ContextSessionSnapshotV1 =
            serde_json::from_value(value).expect("golden snapshot decodes");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn strict_wire_contract_rejects_unknown_fields_and_bad_hashes() {
        let mut value = serde_json::to_value(snapshot()).expect("snapshot serializes");
        value["unknown"] = Value::Bool(true);
        assert!(serde_json::from_value::<ContextSessionSnapshotV1>(value).is_err());

        let mut value = serde_json::to_value(snapshot()).expect("snapshot serializes");
        value["configuration"]["tuning_profile"]["content_digest"] =
            Value::String("sha256:uppercase".to_owned());
        assert!(serde_json::from_value::<ContextSessionSnapshotV1>(value).is_err());
    }

    #[test]
    fn semantic_validation_rejects_invalid_lifecycle_and_colliding_kits() {
        let mut invalid = snapshot();
        invalid.configuration = None;
        assert!(invalid.validate().is_err());

        let mut invalid = snapshot();
        invalid.state.phase = ContextSessionPhaseV1::ReceiptReady;
        invalid.state.active_plan_id = None;
        assert!(invalid.validate().is_err());

        let mut invalid = snapshot();
        let kit = invalid
            .configuration
            .as_mut()
            .expect("configuration exists")
            .kits[0]
            .clone();
        invalid
            .configuration
            .as_mut()
            .expect("configuration exists")
            .kits
            .push(kit);
        assert!(invalid.validate().is_err());

        let mut invalid = snapshot();
        invalid.state.recovery_state = ContextSessionRecoveryStateV1::Corrupt;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn created_and_aborted_states_have_explicit_safe_shape() {
        let mut created = snapshot();
        created.configuration = None;
        created.state = ContextSessionStateV1 {
            phase: ContextSessionPhaseV1::Created,
            revision: 0,
            next_event_sequence: 0,
            active_plan_id: None,
            receipt_id: None,
            last_checkpoint_digest: None,
            recovery_state: ContextSessionRecoveryStateV1::Resumable,
            abort_reason: None,
        };
        created.validate().expect("created state is valid");

        created.state.phase = ContextSessionPhaseV1::Aborted;
        assert!(created.validate().is_err());
        created.state.abort_reason = Some(reference("abort:setup-failure"));
        created.validate().expect("aborted state has a reason");
    }
}
