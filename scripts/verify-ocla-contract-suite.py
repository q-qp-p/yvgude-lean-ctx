#!/usr/bin/env python3
"""Run the public OCLA verifier conformance profile against one executable."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path


PROFILE = "leanctx.ocla-verifier-conformance/v1"
MAX_WIRE_BYTES = 64 * 1024
MAX_OUTPUT_BYTES = 64 * 1024
MAX_VERIFIER_BYTES = 128 * 1024 * 1024
MAX_CONTRACT_ARTIFACT_BYTES = 8 * 1024 * 1024
CASE_TIMEOUT_SECONDS = 5.0
ROOT = Path(__file__).resolve().parents[1]
DEFAULT_FIXTURES = ROOT / "clients/rust/lean-ctx-client/tests/fixtures"
CONTRACT_PACK = ROOT / "docs/contracts/ocla-contract-pack-v1.json"
ENGINE_MANIFEST = ROOT / "rust/Cargo.toml"
CONTRACT_PACK_ARTIFACTS = frozenset(
    {
        "clients/rust/lean-ctx-client/tests/fixtures/agent-envelope-v1.json",
        "clients/rust/lean-ctx-client/tests/fixtures/canonical-token-envelope-v1.json",
        "clients/rust/lean-ctx-client/tests/fixtures/invalid-agent-envelope-v1.json",
        "clients/rust/lean-ctx-client/tests/fixtures/invalid-token-envelope-v1.json",
        "clients/rust/lean-ctx-client/tests/fixtures/self-relay-agent-envelope-v1.json",
        "docs/contracts/ocla/v1/ocla.proto",
        "docs/contracts/capabilities-contract-v1.md",
        "docs/contracts/conformance-v1.md",
        "docs/contracts/ocla-agent-envelope-v1.schema.json",

        "docs/contracts/README.md",
        "docs/contracts/DEPRECATION.md",
        "docs/contracts/certification-levels-v1.md",
        "docs/contracts/ocla-verifier-conformance-v1.md",
        "docs/contracts/ocla-wire-v1.schema.json",
        "scripts/verify-ocla-contract-suite.py",
    }
)
SCHEMA_MIRRORS = (
    "ocla-wire-v1.schema.json",
    "ocla-agent-envelope-v1.schema.json",
)
SEMVER_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
ENGINE_VERSION_RE = re.compile(r'^version\s*=\s*"([^"]+)"\s*$', re.MULTILINE)


class SuiteError(ValueError):
    """A stable infrastructure error that is safe to expose."""


@dataclass(frozen=True)
class Case:
    name: str
    kind: str
    wire: bytes
    expected_exit: int
    suffix: tuple[str, ...] = ()
    reject_markers: tuple[bytes, ...] = ()


class BoundedOutput:
    """Drain both process pipes while enforcing one shared byte budget."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._total = 0
        self.stdout = bytearray()
        self.stderr = bytearray()
        self.overflow = threading.Event()

    def append(self, stream: str, chunk: bytes) -> None:
        with self._lock:
            remaining = MAX_OUTPUT_BYTES - self._total
            if remaining > 0:
                target = self.stdout if stream == "stdout" else self.stderr
                target.extend(chunk[:remaining])
            self._total += len(chunk)
            if self._total > MAX_OUTPUT_BYTES:
                self.overflow.set()


def canonical_json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n"


def safe_open_flags() -> int:
    flags = os.O_RDONLY
    if os.name == "posix":
        for name in ("O_NOFOLLOW", "O_NONBLOCK"):
            if not hasattr(os, name):
                raise SuiteError("platform_missing_safe_open")
            flags |= getattr(os, name)
    if hasattr(os, "O_BINARY"):
        flags |= os.O_BINARY
    return flags


def read_bounded_descriptor(descriptor: int, maximum: int, label: str) -> bytes:
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = os.read(descriptor, min(64 * 1024, maximum + 1 - total))
        if not chunk:
            return b"".join(chunks)
        chunks.append(chunk)
        total += len(chunk)
        if total > maximum:
            raise SuiteError(f"{label}_oversize")


def read_contract_artifact(root: Path, relative: str) -> bytes:
    """Read one contract-pack artifact through a bounded regular-file handle."""

    path = root / relative
    if path.is_symlink() or not path.is_file():
        raise SuiteError("contract_artifact_not_regular")
    flags = os.O_RDONLY
    if os.name == "posix":
        if not hasattr(os, "O_NOFOLLOW"):
            raise SuiteError("platform_missing_safe_open")
        flags |= os.O_NOFOLLOW
    descriptor = -1
    try:
        descriptor = os.open(path, flags)
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise SuiteError("contract_artifact_not_regular")
        return read_bounded_descriptor(
            descriptor, MAX_CONTRACT_ARTIFACT_BYTES, "contract_artifact"
        )
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def parse_semver(value: object, label: str) -> tuple[int, int, int]:
    if not isinstance(value, str):
        raise SuiteError(f"{label}_invalid")
    match = SEMVER_RE.fullmatch(value)
    if match is None:
        raise SuiteError(f"{label}_invalid")
    return tuple(int(part) for part in match.groups())


def engine_version(root: Path = ROOT) -> str:
    try:
        manifest = (root / ENGINE_MANIFEST.relative_to(ROOT)).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise SuiteError("engine_version_unavailable") from error
    match = ENGINE_VERSION_RE.search(manifest)
    if match is None:
        raise SuiteError("engine_version_unavailable")
    parse_semver(match.group(1), "engine_version")
    return match.group(1)


def contract_pack_version_report(root: Path = ROOT, pack: object | None = None) -> dict[str, object]:
    """Report whether the pack is complete and compatible with this engine major."""

    if pack is None:
        try:
            pack = json.loads((root / CONTRACT_PACK.relative_to(ROOT)).read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SuiteError("contract_pack_invalid") from error
    if not isinstance(pack, dict):
        raise SuiteError("contract_pack_schema")
    artifacts = pack.get("artifacts")
    if not isinstance(artifacts, list):
        raise SuiteError("contract_pack_artifacts")
    seen = {
        artifact.get("path")
        for artifact in artifacts
        if isinstance(artifact, dict) and isinstance(artifact.get("path"), str)
    }
    pack_version = pack.get("version")
    pack_major, _, _ = parse_semver(pack_version, "contract_pack_version")
    current_engine_version = engine_version(root)
    engine_major, _, _ = parse_semver(current_engine_version, "engine_version")
    compatibility = pack.get("compatibility")
    supported: set[str] = set()
    if isinstance(compatibility, dict) and isinstance(compatibility.get("supported"), list):
        supported = {
            version
            for version in compatibility["supported"]
            if isinstance(version, str) and SEMVER_RE.fullmatch(version)
        }
    expected_pack_version = f"{engine_major}.0.0"
    expected_supported = {expected_pack_version}
    if engine_major > 0:
        expected_supported.add(f"{engine_major - 1}.0.0")
    missing = sorted(CONTRACT_PACK_ARTIFACTS - seen)
    unexpected = sorted(seen - CONTRACT_PACK_ARTIFACTS)
    version_compatible = (
        pack_version == expected_pack_version
        and pack_major == engine_major
        and expected_supported.issubset(supported)
    )
    return {
        "contract_pack_version": pack_version,
        "engine_version": current_engine_version,
        "missing_artifacts": missing,
        "unexpected_artifacts": unexpected,
        "version_compatible": version_compatible,
    }


def validate_contract_pack(root: Path = ROOT, pack: object | None = None) -> None:
    """Fail closed when the committed OCLA contract pack drifts or is incomplete."""

    if pack is None:
        try:
            pack = json.loads(
                (root / CONTRACT_PACK.relative_to(ROOT)).read_text(encoding="utf-8")
            )
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SuiteError("contract_pack_invalid") from error
    if not isinstance(pack, dict) or pack.get("schema_version") != "leanctx.contract-pack/v1":
        raise SuiteError("contract_pack_schema")
    artifacts = pack.get("artifacts")
    if not isinstance(artifacts, list):
        raise SuiteError("contract_pack_artifacts")
    seen: set[str] = set()
    for artifact in artifacts:
        if not isinstance(artifact, dict) or set(artifact) != {"path", "sha256"}:
            raise SuiteError("contract_pack_artifact_entry")
        relative = artifact["path"]
        digest = artifact["sha256"]
        if not isinstance(relative, str) or not relative or Path(relative).is_absolute():
            raise SuiteError("contract_pack_artifact_path")
        if any(part in ("", ".", "..") for part in Path(relative).parts):
            raise SuiteError("contract_pack_artifact_path")
        if relative in seen:
            raise SuiteError("contract_pack_artifact_duplicate")
        seen.add(relative)
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or digest != digest.lower()
            or any(character not in "0123456789abcdef" for character in digest)
        ):
            raise SuiteError("contract_pack_artifact_digest")
        body = read_contract_artifact(root, relative)
        if hashlib.sha256(body).hexdigest() != digest:
            raise SuiteError("contract_pack_artifact_drift")
    if seen != CONTRACT_PACK_ARTIFACTS:
        raise SuiteError("contract_pack_artifact_set")
    for name in SCHEMA_MIRRORS:
        public = read_contract_artifact(root, f"docs/contracts/{name}")
        packaged = read_contract_artifact(
            root, f"clients/rust/lean-ctx-client/contracts/{name}"
        )
        if public != packaged:
            raise SuiteError("contract_schema_mirror_drift")


def open_fixture_directory(root: Path) -> int | None:
    if os.name != "posix":
        if root.is_symlink() or not root.is_dir():
            raise SuiteError("fixture_root_not_directory")
        return None
    flags = os.O_RDONLY | os.O_NOFOLLOW
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    descriptor = os.open(root, flags)
    if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
        os.close(descriptor)
        raise SuiteError("fixture_root_not_directory")
    return descriptor


def read_fixture(root: Path, directory_descriptor: int | None, name: str) -> bytes:
    descriptor = -1
    try:
        if directory_descriptor is None:
            path = root / name
            if path.is_symlink():
                raise SuiteError("fixture_not_regular")
            descriptor = os.open(path, safe_open_flags())
        else:
            descriptor = os.open(name, safe_open_flags(), dir_fd=directory_descriptor)
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise SuiteError("fixture_not_regular")
        return read_bounded_descriptor(descriptor, MAX_WIRE_BYTES, "fixture")
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def snapshot_verifier(source: Path, destination: Path) -> None:
    descriptor = -1
    directory_descriptor = -1
    try:
        if os.name == "posix":
            parent = source.parent.resolve(strict=True)
            directory_flags = os.O_RDONLY | os.O_NOFOLLOW
            if hasattr(os, "O_DIRECTORY"):
                directory_flags |= os.O_DIRECTORY
            directory_descriptor = os.open(parent, directory_flags)
            descriptor = os.open(
                source.name,
                safe_open_flags(),
                dir_fd=directory_descriptor,
            )
        else:
            if source.is_symlink():
                raise SuiteError("verifier_not_regular")
            descriptor = os.open(source, safe_open_flags())
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise SuiteError("verifier_not_regular")
        if os.name == "posix" and metadata.st_mode & 0o111 == 0:
            raise SuiteError("verifier_not_executable")
        body = read_bounded_descriptor(descriptor, MAX_VERIFIER_BYTES, "verifier")
        destination.write_bytes(body)
        if os.name == "posix":
            destination.chmod(0o700)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        if directory_descriptor >= 0:
            os.close(directory_descriptor)


def replace_once(wire: bytes, old: bytes, new: bytes) -> bytes:
    if wire.count(old) != 1:
        raise SuiteError("fixture_shape_drift")
    return wire.replace(old, new, 1)


def cases(fixtures: Path) -> tuple[Case, ...]:
    directory_descriptor = open_fixture_directory(fixtures)
    try:
        token = read_fixture(
            fixtures, directory_descriptor, "canonical-token-envelope-v1.json"
        )
        agent = read_fixture(fixtures, directory_descriptor, "agent-envelope-v1.json")
        invalid_token = read_fixture(
            fixtures, directory_descriptor, "invalid-token-envelope-v1.json"
        )
        invalid_agent = read_fixture(
            fixtures, directory_descriptor, "invalid-agent-envelope-v1.json"
        )
        self_relay = read_fixture(
            fixtures,
            directory_descriptor,
            "self-relay-agent-envelope-v1.json",
        )
    finally:
        if directory_descriptor is not None:
            os.close(directory_descriptor)
    token_marker = (b"request-1",)
    agent_marker = (b"agent-request-1",)
    return (
        Case("valid_token", "token", token, 0),
        Case("valid_agent", "agent", agent, 0),
        Case("valid_agent_gateway", "agent", agent, 0, ("--gateway",)),
        Case("valid_self_relay_wire", "agent", self_relay, 0),
        Case(
            "self_relay_gateway",
            "agent",
            self_relay,
            2,
            ("--gateway",),
            agent_marker,
        ),
        Case(
            "unknown_token_field",
            "token",
            invalid_token,
            2,
            reject_markers=token_marker,
        ),
        Case(
            "invalid_agent_invariant",
            "agent",
            invalid_agent,
            2,
            reject_markers=agent_marker,
        ),
        Case(
            "wrong_wire_kind",
            "agent",
            token,
            2,
            reject_markers=token_marker,
        ),
        Case(
            "noncanonical_token",
            "token",
            token + b"\n",
            2,
            reject_markers=token_marker,
        ),
        Case("oversize_document", "token", b" " * (MAX_WIRE_BYTES + 1), 2),
        Case("malformed_document", "token", b"{", 2),
        Case(
            "unsupported_version",
            "token",
            replace_once(token, b'"schema_version":1', b'"schema_version":2'),
            2,
            reject_markers=token_marker,
        ),
        Case(
            "duplicate_field",
            "token",
            b'{"schema_version":1,' + token[1:],
            2,
            reject_markers=token_marker,
        ),
        Case(
            "accounting_invariant",
            "token",
            replace_once(token, b'"delivered_tokens":60', b'"delivered_tokens":90'),
            2,
            reject_markers=token_marker,
        ),
        Case(
            "agent_lineage",
            "agent",
            replace_once(
                agent,
                b'"agent_id":"owner-agent"',
                b'"agent_id":"other-agent"',
            ),
            2,
            reject_markers=agent_marker,
        ),
        Case(
            "relay_integrity",
            "agent",
            replace_once(agent, b'"relay_id":"agent-relay:0', b'"relay_id":"agent-relay:1'),
            2,
            reject_markers=agent_marker,
        ),
        Case(
            "zero_budget",
            "agent",
            replace_once(agent, b'"budget_tokens":900', b'"budget_tokens":0'),
            2,
            reject_markers=agent_marker,
        ),
        Case(
            "u64_overflow",
            "token",
            replace_once(
                token,
                b'"original_tokens":100',
                b'"original_tokens":18446744073709551616',
            ),
            2,
            reject_markers=token_marker,
        ),
    )


def drain(stream: str, pipe, output: BoundedOutput) -> None:
    try:
        while True:
            chunk = pipe.read(4096)
            if not chunk:
                return
            output.append(stream, chunk)
    finally:
        pipe.close()


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGKILL)
            return
        except ProcessLookupError:
            return
    if process.poll() is None:
        process.kill()


def execute_case(verifier: Path, case: Case, directory: Path) -> dict[str, object]:
    wire_path = directory / f"{case.name}.json"
    wire_path.write_bytes(case.wire)
    try:
        process = subprocess.Popen(
            [str(verifier), case.kind, str(wire_path), *case.suffix],
            cwd=directory,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=os.name == "posix",
        )
    except OSError:
        return {"case": case.name, "passed": False, "reason": "launch_error"}

    assert process.stdout is not None
    assert process.stderr is not None
    output = BoundedOutput()
    readers = (
        threading.Thread(
            target=drain,
            args=("stdout", process.stdout, output),
            daemon=True,
        ),
        threading.Thread(
            target=drain,
            args=("stderr", process.stderr, output),
            daemon=True,
        ),
    )
    for reader in readers:
        reader.start()

    deadline = time.monotonic() + CASE_TIMEOUT_SECONDS
    termination = ""
    while process.poll() is None:
        if output.overflow.is_set():
            termination = "output_limit"
            stop_process(process)
            break
        if time.monotonic() >= deadline:
            termination = "timeout"
            stop_process(process)
            break
        time.sleep(0.01)
    try:
        process.wait(timeout=1)
    except subprocess.TimeoutExpired:
        stop_process(process)
        process.wait()
    for reader in readers:
        reader.join(timeout=1)
    if any(reader.is_alive() for reader in readers):
        stop_process(process)
        for reader in readers:
            reader.join(timeout=1)
        termination = termination or "output_reader"
    if output.overflow.is_set():
        termination = "output_limit"

    reason = termination
    if not reason and process.returncode != case.expected_exit:
        reason = "exit_code"
    if not reason and case.expected_exit == 0 and output.stderr:
        reason = "success_stderr"
    if not reason and case.expected_exit != 0 and output.stdout:
        reason = "rejection_stdout"
    if not reason and case.expected_exit != 0:
        combined = bytes(output.stdout + output.stderr)
        if any(marker in combined for marker in case.reject_markers):
            reason = "document_echo"
    return {"case": case.name, "passed": not reason, "reason": reason}


def run(verifier: Path, fixtures: Path) -> dict[str, object]:
    validate_contract_pack()
    with tempfile.TemporaryDirectory(prefix="leanctx-ocla-suite-") as temporary:
        directory = Path(temporary)
        suffix = verifier.suffix if os.name != "posix" else ""
        snapshot = directory / f"verifier{suffix}"
        snapshot_verifier(verifier, snapshot)
        results = [execute_case(snapshot, case, directory) for case in cases(fixtures)]
    return {
        "all_passed": all(result["passed"] for result in results),
        "certification_claimed": False,
        "profile": PROFILE,
        "results": results,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--verifier", type=Path)
    parser.add_argument("--fixtures", default=DEFAULT_FIXTURES, type=Path)
    parser.add_argument(
        "--check-version",
        action="store_true",
        help="report contract-pack completeness and engine-major compatibility",
    )
    args = parser.parse_args()
    if args.check_version:
        try:
            report = contract_pack_version_report()
        except (OSError, SuiteError):
            print("OCLA contract suite failed: invalid_or_unsafe_input", file=sys.stderr)
            return 2
        report["compatible"] = (
            report["version_compatible"]
            and not report["missing_artifacts"]
            and not report["unexpected_artifacts"]
        )
        sys.stdout.write(canonical_json(report))
        return 0 if report["compatible"] else 1
    if args.verifier is None:
        parser.error("--verifier is required unless --check-version is used")
    try:
        report = run(args.verifier, args.fixtures)
    except (OSError, SuiteError):
        print("OCLA contract suite failed: invalid_or_unsafe_input", file=sys.stderr)
        return 2
    sys.stdout.write(canonical_json(report))
    return 0 if report["all_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
