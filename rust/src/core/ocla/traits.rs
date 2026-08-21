use crate::core::a2a::message::{MessagePriority, PrivacyLevel};
use lean_ctx_protocol::CapabilityManifestV1;

use super::types::{
    AgentEnvelope, CompressionRequest, CompressionResult, ConfigProposal, ConfigTuningRequest,
    ConnectorJob, DeliveryEntry, DeliveryRecord, DeliveryRecordResult, DeliveryStats,
    EfficiencyAnalysis,
    EfficiencySample, ExperimentRequest, ExperimentResult, IntentDecision, IntentRequest,
    MetricPoint, ModelRouteRequest, Observation, OclaCapability, OclaResult, Outcome,
    ResponseOptimizationRequest, ResponseOptimizationResult, RoutingDecision, SavingsEvidence,
    ScheduledJob, UsageRecord,
};

/// Common, versioned discovery surface for every OCLA capability.
pub trait OclaService: Send + Sync {
    fn capability(&self) -> OclaCapability;

    /// Returns a discoverable manifest for this service.
    ///
    /// Services without a dedicated capability contract use this valid,
    /// explicitly unsupported placeholder until they are promoted to a
    /// versioned capability.
    fn manifest(&self) -> CapabilityManifestV1 {
        let capability = self.capability();
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "capability_id": format!(
                "capability://leanctx/placeholder/{:?}",
                capability.kind
            )
            .to_ascii_lowercase(),
            "provider": "leanctx",
            "kind": "context_source",
            "version": "0.0.0",
            "surfaces": ["context"],
            "support_matrix": {"context": {"supported": false}},
            "local": true,
            "remote": false,
            "reversibility": "reversible",
            "determinism": "deterministic",
            "data_movement": "local_only",
            "supported_classifications": ["Public"],
            "measurement_support": {
                "latency": false,
                "tokens": false,
                "quality": false
            },
            "conformance_version": 1
        }))
        .expect("static OCLA placeholder manifest is valid")
    }
}

pub trait ObservationHook: OclaService {
    fn observe(&self, observation: Observation) -> OclaResult<()>;
}

pub trait UsageSink: OclaService {
    fn record_usage(&self, usage: UsageRecord) -> OclaResult<()>;
}

pub trait MetricsExporter: OclaService {
    fn export_metrics(&self, metrics: Vec<MetricPoint>) -> OclaResult<()>;
}

pub trait SavingsLedger: OclaService {
    fn record_savings(&self, evidence: SavingsEvidence) -> OclaResult<String>;
}

pub trait IntentClassifier: OclaService {
    fn classify_intent(&self, request: IntentRequest) -> OclaResult<IntentDecision>;
}

pub trait OutcomeTracker: OclaService {
    fn record_outcome(&self, outcome: Outcome) -> OclaResult<()>;
}

pub trait CompressionProvider: OclaService {
    fn compress(&self, request: CompressionRequest) -> OclaResult<CompressionResult>;
}

pub trait ResponseOptimizer: OclaService {
    fn optimize_response(
        &self,
        request: ResponseOptimizationRequest,
    ) -> OclaResult<ResponseOptimizationResult>;
}

pub trait ModelRouter: OclaService {
    fn route_model(&self, request: ModelRouteRequest) -> OclaResult<RoutingDecision>;
}

pub trait EfficiencyAnalyzer: OclaService {
    fn analyze_efficiency(&self, sample: EfficiencySample) -> OclaResult<EfficiencyAnalysis>;
}

pub trait ConfigTuner: OclaService {
    fn propose_tuning(&self, request: ConfigTuningRequest) -> OclaResult<ConfigProposal>;
}

pub trait ExperimentRunner: OclaService {
    fn run_experiment(&self, request: ExperimentRequest) -> OclaResult<ExperimentResult>;
}

pub trait ConnectorScheduler: OclaService {
    fn schedule_connector(&self, job: ConnectorJob) -> OclaResult<ScheduledJob>;
}

pub trait AgentGateway: OclaService {
    fn relay_agent(&self, envelope: AgentEnvelope) -> OclaResult<AgentEnvelope>;
    fn route_message(
        &self,
        from: &str,
        to: Option<&str>,
        category: &str,
        message: &str,
        privacy: PrivacyLevel,
        priority: MessagePriority,
        ttl_hours: Option<u64>,
    ) -> OclaResult<String>;
}

pub trait DeliveryRegistry: OclaService {
    fn check_delivery(
        &self,
        blake3: &[u8; 12],
        mtime: u64,
        path: &str,
        requester_agent_id: Option<&str>,
        requester_conversation_id: Option<&str>,
    ) -> Option<DeliveryRecord>;
    fn record_stub_served(&self, record: &DeliveryRecord, stub_tokens: u64);
    fn record_delivery(&self, entry: DeliveryEntry) -> DeliveryRecordResult;
    fn delivery_stats(&self) -> DeliveryStats;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_trait_is_object_safe() {
        fn assert_object_safe<T: ?Sized>() {}
        assert_object_safe::<dyn ObservationHook>();
        assert_object_safe::<dyn UsageSink>();
        assert_object_safe::<dyn MetricsExporter>();
        assert_object_safe::<dyn SavingsLedger>();
        assert_object_safe::<dyn IntentClassifier>();
        assert_object_safe::<dyn OutcomeTracker>();
        assert_object_safe::<dyn CompressionProvider>();
        assert_object_safe::<dyn ResponseOptimizer>();
        assert_object_safe::<dyn ModelRouter>();
        assert_object_safe::<dyn EfficiencyAnalyzer>();
        assert_object_safe::<dyn ConfigTuner>();
        assert_object_safe::<dyn ExperimentRunner>();
        assert_object_safe::<dyn ConnectorScheduler>();
        assert_object_safe::<dyn AgentGateway>();
        assert_object_safe::<dyn DeliveryRegistry>();
    }
}
