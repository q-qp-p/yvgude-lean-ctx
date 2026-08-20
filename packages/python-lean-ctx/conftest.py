"""Shared deterministic loopback Runtime fixture for the SDK v1 tests."""

import hashlib
import json
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, HTTPServer

import pytest


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


class _RuntimeState:
    def __init__(self):
        self.last_request = None
        self.requests = []
        self.sessions = {}
        self.receipts = {}
        self.require_token = False
        self.missing_headers = False
        self.invalid_header = False
        self.failed_seal = False
        self.verification_false = False
        self.profile_mismatch = False
        self.kit_mismatch = False
        self.kit_hash = "a" * 64
        self.baseline_cost_micros = None
        self.treatment_cost_micros = None

    @staticmethod
    def profile():
        return {
            "id": "balanced",
            "version": "1",
            "content_hash": "b" * 64,
            "source_ref": "profile:balanced@1",
            "context_budget": 12000,
        }

    def receipt(self, session, payload):
        receipt = {
            "schema_version": "1",
            "receipt_id": "receipt-v1",
            "session_id": session["session_id"],
            "task_id": session["task_id"],
            "run_id": session["run_id"],
            "trace_id": session["trace_id"],
            "agent_id": session["agent_id"],
            "project_id": "project-v1",
            "profile": session["profile"],
            "kits": session["kits"],
            "integration_depth": "wrap",
            "coverage": payload.get("coverage", "not_addressable"),
            "execution_receipt_ids": [
                item["execution_receipt_id"]
                for item in payload.get("observations", [])
                if item.get("execution_receipt_id")
            ],
            "integrity_status": "sealed",
            "outcome": payload.get("outcome", "succeeded"),
            "degradations": payload.get("degradations", []),
            "savings": {
                "original_tokens": None,
                "delivered_tokens": None,
                "saved_tokens": None,
                "saved_pct": None,
                "provider_input_tokens": None,
                "provider_cached_tokens": None,
                "provider_output_tokens": None,
                "reasoning_tokens": None,
                "methodology": (
                    "baseline_treatment"
                    if self.baseline_cost_micros is not None and self.treatment_cost_micros is not None
                    else "compression_observation"
                ),
                "baseline_ref": (
                    "baseline-v1"
                    if self.baseline_cost_micros is not None and self.treatment_cost_micros is not None
                    else None
                ),
                "quality_status": "unknown",
                "baseline_cost_micros": self.baseline_cost_micros,
                "treatment_cost_micros": self.treatment_cost_micros,
                "avoided_cost_micros": (
                    max(0, self.baseline_cost_micros - self.treatment_cost_micros)
                    if self.baseline_cost_micros is not None and self.treatment_cost_micros is not None
                    else None
                ),
            },
        }
        canonical = _canonical(receipt)
        receipt["canonical_json"] = canonical.decode("utf-8")
        receipt["canonical_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()
        self.receipts[receipt["receipt_id"]] = receipt
        return receipt


class _RuntimeHandler(BaseHTTPRequestHandler):
    def _state(self):
        return self.server.state

    def _body(self):
        size = int(self.headers.get("Content-Length", "0"))
        return json.loads(self.rfile.read(size).decode("utf-8")) if size else {}

    def _request(self, body):
        record = {"path": self.path, "headers": dict(self.headers), "body": body}
        self._state().last_request = record
        self._state().requests.append(record)

    def _authorized(self):
        state = self._state()
        return not state.require_token or self.headers.get("Authorization") == "Bearer test-token"

    def _json(self, status, payload, headers=None):
        encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(encoded)

    def do_POST(self):  # noqa: N802
        if not self._authorized():
            self._json(401, {"error": "unauthorized"})
            return
        body = self._body()
        self._request(body)
        state = self._state()
        if self.path == "/v1/sessions":
            profile = state.profile()
            if state.profile_mismatch:
                profile = {**profile, "content_hash": "d" * 64}
            kits = body.get("requested_kits", [])
            if state.kit_mismatch and kits:
                kits = [{**kits[0], "package_hash": "e" * 64}]
            reply = {
                "session_id": "session-v1",
                "task_id": "task-v1",
                "run_id": "run-v1",
                "trace_id": "trace-v1",
                "agent_id": body["agent_id"],
                "project_id": "project-v1",
                "resolved_profile": profile,
                "resolved_kits": kits,
            }
            state.sessions["session-v1"] = {
                "session_id": "session-v1",
                "task_id": "task-v1",
                "run_id": "run-v1",
                "trace_id": "trace-v1",
                "agent_id": body["agent_id"],
                "profile": profile,
                "kits": kits,
            }
            self._json(200, reply)
            return
        if self.path == "/v1/compress":
            messages = body.get("messages", [])
            rewritten = []
            for message in messages:
                clone = dict(message)
                if isinstance(clone.get("content"), str):
                    clone["content"] = clone["content"][:8]
                rewritten.append(clone)
            response_headers = {}
            if not state.missing_headers and self.headers.get("X-LeanCTX-Session-Id"):
                response_headers = {
                    "X-LeanCTX-Protocol": "1",
                    "X-LeanCTX-Request-Id": "request-v1",
                    "X-LeanCTX-Execution-Receipt-Id": "execution-v1",
                    "X-LeanCTX-Canonical-Hash": "sha256:" + "c" * 64,
                    "X-LeanCTX-Coverage": "compressed",
                    "X-LeanCTX-Input-Tokens": "20",
                    "X-LeanCTX-Output-Tokens": "5",
                    "X-LeanCTX-Cached-Tokens": "unknown",
                    "X-LeanCTX-Reasoning-Tokens": "unknown",
                    "X-LeanCTX-Compression-Original-Tokens": "20",
                    "X-LeanCTX-Compression-Delivered-Tokens": "5",
                    "X-LeanCTX-Provider": "mock",
                    "X-LeanCTX-Model": "mock-1",
                    "X-LeanCTX-Latency-Ms": "1",
                }
                if state.invalid_header:
                    response_headers["X-LeanCTX-Input-Tokens"] = "not-a-number"
            self._json(200, {"messages": rewritten, "stats": {"original_tokens": 20}}, response_headers)
            return
        if self.path.startswith("/v1/sessions/") and self.path.endswith(("/complete", "/abort")):
            if state.failed_seal:
                self._json(503, {"error": "seal unavailable"})
                return
            receipt = state.receipt(state.sessions["session-v1"], body)
            self._json(200, receipt)
            return
        self._json(404, {"error": "not found"})

    def do_GET(self):  # noqa: N802
        if not self._authorized():
            self._json(401, {"error": "unauthorized"})
            return
        self._request({})
        state = self._state()
        if self.path.startswith("/v1/kits/"):
            name = urllib.parse.unquote(self.path.rsplit("/", 1)[-1])
            self._json(
                200,
                {
                    "id": "kit-" + name.replace("/", "-"),
                    "version": "1",
                    "package_hash": state.kit_hash,
                    "activation_ref": "kit:" + name,
                    "manifest": {"id": "kit-" + name.replace("/", "-"), "version": "1", "package_hash": state.kit_hash},
                },
            )
            return
        if self.path.startswith("/v1/receipts/") and self.path.endswith("/verify"):
            receipt_id = self.path.split("/")[3]
            receipt = state.receipts.get(receipt_id)
            if receipt is None:
                self._json(404, {"error": "not found"})
                return
            self._json(
                200,
                {
                    "receipt_id": receipt_id,
                    "canonical_hash": receipt["canonical_hash"],
                    "verified": not state.verification_false,
                },
            )
            return
        self._json(404, {"error": "not found"})

    def log_message(self, *args):
        pass


@pytest.fixture
def v1_proxy():
    httpd = HTTPServer(("127.0.0.1", 0), _RuntimeHandler)
    httpd.state = _RuntimeState()
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        host, port = httpd.server_address
        yield httpd.state, f"http://{host}:{port}"
    finally:
        httpd.shutdown()
        thread.join(timeout=2)
