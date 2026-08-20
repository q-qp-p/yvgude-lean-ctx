from copy import deepcopy

from lean_ctx.receipt import parse_execution_receipt


def _payload():
    import hashlib
    import json

    value = {
        "schema_version": "1",
        "receipt_id": "receipt-v1",
        "session_id": "session-v1",
        "task_id": "task-v1",
        "run_id": "run-v1",
        "trace_id": "trace-v1",
        "agent_id": "agent-v1",
        "project_id": "project-v1",
        "profile": None,
        "kits": [],
        "integration_depth": "wrap",
        "coverage": "not_addressable",
        "execution_receipt_ids": [],
        "integrity_status": "sealed",
        "outcome": "succeeded",
        "degradations": [],
        "savings": {
            "original_tokens": None,
            "delivered_tokens": None,
            "saved_tokens": None,
            "saved_pct": None,
            "provider_input_tokens": None,
            "provider_cached_tokens": None,
            "provider_output_tokens": None,
            "reasoning_tokens": None,
            "methodology": "compression_observation",
            "baseline_ref": None,
            "quality_status": "unknown",
        },
    }
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    value["canonical_json"] = canonical.decode("utf-8")
    value["canonical_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()
    return value


def test_local_canonical_digest_verifies_without_network():
    receipt = parse_execution_receipt(_payload())
    assert receipt.verify() is True
    assert receipt.savings.methodology == "compression_observation"


def test_mismatched_digest_and_unsealed_are_false():
    invalid = _payload()
    invalid["canonical_hash"] = "sha256:" + "0" * 64
    assert parse_execution_receipt(invalid).verify() is False
    unsealed = _payload()
    unsealed["integrity_status"] = "unsealed"
    assert parse_execution_receipt(unsealed).verify() is False


def test_runtime_verifier_answer_is_honored(v1_proxy):
    state, base_url = v1_proxy
    payload = _payload()
    state.receipts["receipt-v1"] = payload
    receipt = parse_execution_receipt(payload, verify_url=base_url)
    assert receipt.verify() is True
    state.verification_false = True
    assert receipt.verify() is False
