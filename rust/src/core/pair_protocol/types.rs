use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PairCode(pub String);

#[allow(dead_code)]
impl PairCode {
    pub(crate) fn generate() -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let hash = hasher.finish();
        let mut code = String::with_capacity(9);
        code.push_str("LCTX-");
        for i in 0..4u64 {
            code.push(CHARS[((hash >> (i * 8)) % CHARS.len() as u64) as usize] as char);
        }
        Self(code)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum PairState {
    Unpaired,
    Pairing { code: PairCode, expires_at: u64 },
    Paired { session_id: String, paired_at: u64 },
    Expired,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PairRequest {
    pub code: String,
    pub runner_info: RunnerInfo,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunnerInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub agents: Vec<String>,
    pub profile: Option<String>,
}

#[allow(dead_code)]
impl RunnerInfo {
    pub(crate) fn detect() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            agents: Vec::new(),
            profile: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PairResponse {
    pub success: bool,
    pub session_id: Option<String>,
    pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum WsMessage {
    Pair(PairRequest),
    PairAck(PairResponse),
    StartBenchmark {
        spec_id: String,
        profile_hash: Option<String>,
    },
    Progress {
        task_index: usize,
        total: usize,
        task_id: String,
        status: String,
    },
    TaskResult {
        task_id: String,
        passed: bool,
        cost_usd: f64,
        quality_score: f64,
        latency_ms: f64,
    },
    BenchmarkComplete {
        result_id: String,
        summary_json: String,
    },
    Error {
        code: u16,
        message: String,
    },
    Ping,
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_code_format() {
        let c = PairCode::generate();
        assert!(c.0.starts_with("LCTX-"));
        assert_eq!(c.0.len(), 9);
    }
    #[test]
    fn ws_message_roundtrip() {
        let msg = WsMessage::Progress {
            task_index: 1,
            total: 5,
            task_id: "explore".into(),
            status: "running".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, WsMessage::Progress { task_index: 1, .. }));
    }
    #[test]
    fn runner_info_detect() {
        let info = RunnerInfo::detect();
        assert!(!info.version.is_empty());
    }
    #[test]
    fn pair_state_transitions() {
        assert!(matches!(
            PairState::Pairing {
                code: PairCode::generate(),
                expires_at: 999
            },
            PairState::Pairing { .. }
        ));
    }
    #[test]
    fn ping_pong_roundtrip() {
        let j = serde_json::to_string(&WsMessage::Ping).unwrap();
        assert!(matches!(
            serde_json::from_str::<WsMessage>(&j).unwrap(),
            WsMessage::Ping
        ));
    }
}
