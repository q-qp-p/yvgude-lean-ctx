//! `/api/roi` — local evidence and context-usage observations for the dashboard.
//!
//! This route is local-only. It exposes the local ledger report, daily trend,
//! and observed output shaping data. Hosted plans, entitlements, organization
//! usage, and billing surfaces are intentionally absent from the public Runtime.

use serde_json::json;
use std::sync::Mutex;
use std::time::Instant;

static ROI_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);
const ROI_TTL_SECS: u64 = 300;

pub(super) fn handle(
    path: &str,
    _query_str: &str,
    _method: &str,
    _body: &str,
) -> Option<(&'static str, &'static str, String)> {
    match path {
        "/api/roi" => Some(roi_cached()),
        _ => None,
    }
}

fn roi_cached() -> (&'static str, &'static str, String) {
    if let Ok(guard) = ROI_CACHE.lock() {
        if let Some((ts, ref body)) = *guard {
            if ts.elapsed().as_secs() < ROI_TTL_SECS {
                return ("200 OK", "application/json", body.clone());
            }
        }
    }
    let result = roi();
    if let Ok(mut guard) = ROI_CACHE.lock() {
        *guard = Some((Instant::now(), result.2.clone()));
    }
    result
}

fn roi() -> (&'static str, &'static str, String) {
    let agent_id = crate::core::agent_identity::current_agent_id();
    let report = crate::core::savings_ledger::roi_report(agent_id);
    let summary = crate::core::savings_ledger::summary();
    let output = crate::proxy::output_savings::to_json(&crate::proxy::output_savings::current());

    let payload = json!({
        "roi": report,
        "trend": summary.by_day,
        "output": output,
    });
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    ("200 OK", "application/json", body)
}
