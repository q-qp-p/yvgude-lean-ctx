import hashlib
import json
import os
from pathlib import Path
from types import SimpleNamespace

import pytest

from lean_ctx import ContextSource, ContextView, LeanCTX, LocalEngineClient
from lean_ctx.errors import (
    LeanCtxEngineProtocolError,
    LeanCtxEngineTimeout,
    LeanCtxEngineUnavailable,
)


COMPATIBILITY_FIXTURE = (
    Path(__file__).parent / "fixtures" / "engine-interface-v1" / "compatibility.json"
)


def _digest(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def _response(source_text="# Review\n", view_text="Review view\n"):
    source_digest = _digest(source_text)
    output_digest = _digest(view_text)
    invocation_id = "engine-invocation-test"
    input_ref = "input:snapshot"
    source_ref = "source:file"
    receipt_digest = "sha256:" + "e" * 64
    invocation = {
        "schema_version": 1,
        "invocation_id": invocation_id,
        "engine": {"engine_id": "lean-ctx-local", "engine_version": "3.9.20"},
        "operation": {
            "capability_id": "capability://leanctx/context-optimization",
            "capability_version": "1.0.0",
        },
        "input_ref": input_ref,
        "input_digest": "sha256:" + "b" * 64,
        "source_refs": [input_ref, source_ref],
        "policy_admission": {"policy_ref": "policy:local", "decision": "admitted"},
    }
    observation = {
        "schema_version": 1,
        "invocation_id": invocation_id,
        "status": "succeeded",
        "output_ref": "output:" + output_digest.removeprefix("sha256:"),
        "output_digest": output_digest,
        "source_lineage": [input_ref, source_ref],
        "measurements": [
            {"name": "input_tokens", "unit": "token", "classification": "measured", "value": 2}
        ],
        "failure": None,
        "receipt_link": {
            "schema_version": 1,
            "receipt_id": "engine-receipt-test",
            "receipt_ref": "receipt:" + receipt_digest,
            "receipt_digest": receipt_digest,
            "invocation_id": invocation_id,
        },
    }
    return {
        "schema_version": 1,
        "transport_version": "1.0.0",
        "engine_interface_version": "1.0.0",
        "view": {
            "text": view_text,
            "output_ref": observation["output_ref"],
            "output_digest": output_digest,
        },
        "invocation": invocation,
        "observation": observation,
        "recovery": {
            "recovery_ref": input_ref,
            "source_ref": source_ref,
            "source_digest": source_digest,
        },
    }


def test_embed_uses_real_subprocess_shape_and_preserves_host_result(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")
    calls = []

    def run(args, **kwargs):
        calls.append((args, kwargs))
        request = json.loads(Path(args[-1]).read_text(encoding="utf-8"))
        assert args[:3] == ["lean-ctx", "engine", "context-view"]
        assert request == {
            "schema_version": 1,
            "transport_version": 1,
            "engine_interface_version": "1.0.0",
            "path": "review.md",
            "mode": "aggressive",
        }
        return SimpleNamespace(returncode=0, stdout=json.dumps(_response()), stderr="")

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX({"engine_binary": "lean-ctx"}).embed("Review", project_root=str(tmp_path))
    plan = session.plan(ContextSource(str(source), project_root=str(tmp_path)))
    assert plan.plan_id == session.plan_id
    plan = session.plan(ContextSource(str(source), project_root=str(tmp_path)))
    view = plan.execute()
    host_result = object()
    receipt = session.complete(host_result)

    assert len(calls) == 1
    assert view.text == "Review view\n"
    assert receipt.host_result is host_result
    assert receipt.outcome == "unknown"
    assert receipt.verify() is True
    assert receipt.to_dict()["engine"]["output_digest"] == view.output_digest


def test_engine_v1_compatibility_fixture_projects_exact_sdk_contract(monkeypatch, tmp_path):
    fixture = json.loads(COMPATIBILITY_FIXTURE.read_text(encoding="utf-8"))
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")
    response = {key: fixture[key] for key in (
        "schema_version",
        "transport_version",
        "engine_interface_version",
        "view",
        "invocation",
        "observation",
        "recovery",
    )}
    monkeypatch.setattr(
        "lean_ctx.engine.subprocess.run",
        lambda *args, **kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(response), stderr=""
        ),
    )

    session = LeanCTX().embed("Review", project_root=str(tmp_path))
    session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    receipt = session.complete(object())
    projection = receipt.to_dict()
    engine = projection["engine"]
    actual = {
        "outcome": projection["outcome"],
        "integrity_status": projection["integrity_status"],
        "status": engine["status"],
        "source_digest": engine["source_digest"],
        "output_digest": engine["output_digest"],
        "recovery_ref": engine["recovery_ref"],
    }

    assert receipt.preview_version == fixture["preview_version"]
    assert receipt.engine_version == fixture["engine_version"]
    assert receipt.engine_interface_version == fixture["engine_interface_version"]
    assert receipt.transport_version == f'{fixture["transport_version"]}.0.0'
    assert actual == fixture["expected_sdk_projection"]


@pytest.mark.parametrize(
    ("path", "value"),
    [
        (("transport_version",), "1"),
        (("invocation", "engine", "engine_id"), "other-engine"),
        (("invocation", "engine", "engine_version"), "3"),
        (("invocation", "engine", "engine_version"), "4.0.0"),
        (("invocation", "operation", "capability_id"), "capability://other"),
        (("invocation", "operation", "capability_version"), "1.0.1"),
        (("recovery", "recovery_ref"), None),
    ],
)
def test_engine_v1_rejects_unpinned_identity_version_and_recovery(tmp_path, path, value):
    response = _response()
    target = response
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = value
    source = ContextSource(str(tmp_path / "review.md"), project_root=str(tmp_path))

    with pytest.raises(LeanCtxEngineProtocolError):
        ContextView.from_response(response, source=source, engine=LocalEngineClient())


def test_recovery_requires_exact_admitted_source(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source_text = "# Review\n"
    source.write_text(source_text, encoding="utf-8")
    responses = [_response(source_text=source_text)]

    def run(args, **kwargs):
        request = json.loads(Path(args[-1]).read_text(encoding="utf-8"))
        if args[2] == "recover":
            response = responses[0]
            response = {
                "schema_version": 1,
                "transport_version": "1.0.0",
                "engine_interface_version": "1.0.0",
                "view": {
                    "text": source_text,
                    "output_ref": "output:" + _digest(source_text).removeprefix("sha256:"),
                    "output_digest": _digest(source_text),
                },
                "recovery": response["recovery"],
            }
            assert request["source_digest"] == response["recovery"]["source_digest"]
        else:
            response = responses[0]
        return SimpleNamespace(returncode=0, stdout=json.dumps(response), stderr="")

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX().embed("Review", project_root=str(tmp_path))
    view = session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    recovered = view.recover()
    assert recovered == source_text
    assert recovered.text == source_text
    assert recovered.source_digest == _digest(source_text)


def test_missing_recovery_source_fails_without_lossy_fallback(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")

    def run(args, **kwargs):
        if args[2] == "recover":
            return SimpleNamespace(
                returncode=2,
                stdout="",
                stderr="engine: source_unavailable\n",
            )
        return SimpleNamespace(returncode=0, stdout=json.dumps(_response()), stderr="")

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX().embed("Review", project_root=str(tmp_path))
    view = session.prepare(ContextSource(str(source), project_root=str(tmp_path)))

    with pytest.raises(LeanCtxEngineUnavailable):
        view.recover()


def test_malformed_engine_response_is_not_fail_open(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")

    def run(args, **kwargs):
        response = _response()
        response["unexpected"] = True
        return SimpleNamespace(returncode=0, stdout=json.dumps(response), stderr="")

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX({"fail_open": True}).embed("Review", project_root=str(tmp_path))
    with pytest.raises(LeanCtxEngineProtocolError):
        session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    assert session.state == "aborted"
    assert session.receipt.verify() is False


def test_engine_unavailable_can_degrade_explicitly(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")

    def run(args, **kwargs):
        raise FileNotFoundError("not exposed")

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX({"fail_open": True}).embed("Review", project_root=str(tmp_path))
    assert session.prepare(ContextSource(str(source), project_root=str(tmp_path))) is None
    receipt = session.complete("host output")
    assert receipt.integrity_status == "unsealed"
    assert receipt.outcome == "unknown"
    assert "engine_unavailable" in receipt.degradations

    closed = LeanCTX({"fail_open": False}).embed("Review", project_root=str(tmp_path))
    with pytest.raises(LeanCtxEngineUnavailable):
        closed.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    assert closed.state == "aborted"


def test_engine_timeout_degrades_only_when_fail_open(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")

    def run(args, **kwargs):
        raise __import__("subprocess").TimeoutExpired(args, kwargs["timeout"])

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    opened = LeanCTX({"fail_open": True}).embed("Review", project_root=str(tmp_path))
    assert opened.prepare(ContextSource(str(source), project_root=str(tmp_path))) is None
    assert "engine_timeout" in opened.complete("host output").degradations

    closed = LeanCTX({"fail_open": False}).embed("Review", project_root=str(tmp_path))
    with pytest.raises(LeanCtxEngineTimeout):
        closed.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    assert closed.state == "aborted"


def test_path_jail_rejection_never_fails_open(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")

    def run(args, **kwargs):
        return SimpleNamespace(
            returncode=2,
            stdout="",
            stderr="engine: source_outside_root\n",
        )

    monkeypatch.setattr("lean_ctx.engine.subprocess.run", run)
    session = LeanCTX({"fail_open": True}).embed("Review", project_root=str(tmp_path))
    with pytest.raises(LeanCtxEngineProtocolError):
        session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    assert session.state == "aborted"


def test_local_abort_preserves_original_exception(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source.write_text("# Review\n", encoding="utf-8")
    monkeypatch.setattr(
        "lean_ctx.engine.subprocess.run",
        lambda *args, **kwargs: SimpleNamespace(
            returncode=0, stdout=json.dumps(_response()), stderr=""
        ),
    )
    session = LeanCTX().embed("Review", project_root=str(tmp_path))
    session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    error = RuntimeError("agent failed")
    receipt = session.abort(error)
    assert receipt.exception is error
    assert receipt.outcome == "aborted"
    assert session.state == "aborted"


@pytest.mark.skipif(
    not os.environ.get("LEAN_CTX_ENGINE_BINARY"),
    reason="set LEAN_CTX_ENGINE_BINARY to run the real Rust Engine proof",
)
def test_real_rust_engine_binary_context_view_and_recovery(monkeypatch, tmp_path):
    source = tmp_path / "review.md"
    source_text = "# Review\n"
    source.write_text(source_text, encoding="utf-8")
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(tmp_path / "engine-data"))
    session = LeanCTX(
        {"engine_binary": os.environ["LEAN_CTX_ENGINE_BINARY"], "fail_open": False}
    ).embed("Review", project_root=str(tmp_path))
    view = session.prepare(ContextSource(str(source), project_root=str(tmp_path)))
    assert view.status == "succeeded"
    assert view.text is not None
    calls = []

    def custom_host_agent(context):
        calls.append(context)
        return {"summary": context.splitlines()[0]}

    host_result = custom_host_agent(view.text)
    assert view.recover() == source_text
    receipt = session.complete(host_result)
    assert calls == [view.text]
    assert receipt.host_result is host_result
    assert receipt.output == host_result
    assert receipt.outcome == "unknown"
    assert receipt.verify() is True
