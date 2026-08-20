import base64
import hashlib
import json
from copy import deepcopy

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from lean_ctx.receipt import parse_execution_receipt


def _payload():
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


def test_parse_receipt_with_profile_and_kits():
    payload = _payload()
    payload["profile"] = {
        "id": "balanced",
        "version": "1",
        "content_hash": "b" * 64,
        "source_ref": "profile:balanced@1",
    }
    payload["kits"] = [
        {
            "id": "kit-payments",
            "version": "1",
            "package_hash": "a" * 64,
            "activation_ref": "kit:payments",
            "manifest": {
                "id": "kit-payments",
                "version": "1",
                "package_hash": "a" * 64,
            },
        }
    ]
    canonical = json.dumps(
        {key: value for key, value in payload.items() if key not in ("canonical_json", "canonical_hash")},
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    payload["canonical_json"] = canonical.decode("utf-8")
    payload["canonical_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()

    receipt = parse_execution_receipt(payload)
    assert receipt.profile is not None
    assert receipt.profile.id == "balanced"
    assert len(receipt.kits) == 1
    assert receipt.kits[0].id == "kit-payments"


def test_unknown_usage_remains_none_in_savings():
    payload = _payload()
    payload["savings"] = {
        **payload["savings"],
        "provider_cached_tokens": "unknown",
        "reasoning_tokens": "unknown",
        "saved_pct": "unknown",
    }
    receipt = parse_execution_receipt(payload)
    assert receipt.savings.provider_cached_tokens is None
    assert receipt.savings.reasoning_tokens is None
    assert receipt.savings.saved_pct is None


def test_ed25519_signature_verification():
    payload = _payload()
    canonical = payload["canonical_json"].encode("utf-8")
    private_key = Ed25519PrivateKey.generate()
    signature = private_key.sign(canonical)
    public_bytes = private_key.public_key().public_bytes(
        encoding=Encoding.Raw,
        format=PublicFormat.Raw,
    )
    payload["signature"] = "base64:" + base64.b64encode(signature).decode("ascii")
    payload["signer_public_key"] = "base64:" + base64.b64encode(public_bytes).decode("ascii")

    receipt = parse_execution_receipt(payload)
    assert receipt.verify() is True

    tampered = deepcopy(payload)
    document = json.loads(tampered["canonical_json"])
    document["outcome"] = "aborted"
    tampered_canonical = json.dumps(document, sort_keys=True, separators=(",", ":")).encode("utf-8")
    tampered["canonical_json"] = tampered_canonical.decode("utf-8")
    tampered["canonical_hash"] = "sha256:" + hashlib.sha256(tampered_canonical).hexdigest()
    assert parse_execution_receipt(tampered).verify() is False


def test_cost_evidence_fields_preserved():
    payload = _payload()
    payload["savings"] = {
        **payload["savings"],
        "methodology": "baseline_treatment",
        "baseline_ref": "baseline-v1",
        "baseline_cost_micros": 1200,
        "treatment_cost_micros": 450,
        "avoided_cost_micros": 750,
    }
    receipt = parse_execution_receipt(payload)
    assert receipt.savings.baseline_cost_micros == 1200
    assert receipt.savings.treatment_cost_micros == 450
    assert receipt.savings.avoided_cost_micros == 750


def test_verification_endpoint_mismatched_hash_returns_false(v1_proxy):
    state, base_url = v1_proxy
    payload = _payload()
    state.receipts["receipt-v1"] = payload
    receipt = parse_execution_receipt(payload, verify_url=base_url)
    assert receipt.verify() is True
    state.verification_hash_mismatch = True
    assert receipt.verify() is False
