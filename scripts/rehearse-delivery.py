#!/usr/bin/env python3
"""Hermetic, no-deployment rehearsal for two verified delivery manifests."""

import argparse
import hashlib
import importlib.util
import json
import os
import stat
import sys
from pathlib import Path


_SPEC = importlib.util.spec_from_file_location(
    "delivery_manifest_verifier", Path(__file__).with_name("verify-delivery-manifest.py")
)
VERIFIER = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(VERIFIER)

MAX_PLAN_BYTES = 256 * 1024


class InvalidRehearsal(ValueError):
    """The local rehearsal plan cannot safely model a promotion."""


def canonical_json(value):
    return VERIFIER.canonical_json(value)


def _fail(message):
    raise InvalidRehearsal(message)


def _exact_keys(value, keys, label):
    if not isinstance(value, dict) or set(value) != set(keys):
        _fail(f"{label} must contain exactly {sorted(keys)}")


def _sha256(value):
    return hashlib.sha256(value).hexdigest()


def _read_path(root, relative, limit, label):
    try:
        path = VERIFIER.confined_file(root, relative)
        return path, VERIFIER._regular_file(path, limit, label)
    except VERIFIER.InvalidManifest as error:
        _fail(str(error))


def _canonical_path(root, relative, limit, label):
    path, raw = _read_path(root, relative, limit, label)
    try:
        value = json.loads(raw)
    except (TypeError, ValueError, UnicodeDecodeError) as error:
        _fail(f"{label} is not valid JSON: {error}")
    if canonical_json(value) != raw:
        _fail(f"{label} is not canonical JSON")
    return path, value, raw


def _artifact(root, value, label):
    _exact_keys(value, {"path", "sha256"}, label)
    if not isinstance(value["path"], str) or not isinstance(value["sha256"], str):
        _fail(f"{label} has invalid artifact fields")
    path, raw = _read_path(root, value["path"], VERIFIER.MAX_ARTIFACT_BYTES, label)
    if value["sha256"] != _sha256(raw):
        _fail(f"{label} digest mismatch")
    return path, raw


def _release(root, value, trust_root, label):
    _exact_keys(value, {"manifest", "image", "migration", "configuration_schema_version"}, label)
    manifest_path, manifest_raw = _artifact(root, value["manifest"], f"{label} manifest")
    image_path, image_raw = _artifact(root, value["image"], f"{label} image")
    migration_path, migration_raw = _artifact(root, value["migration"], f"{label} migration")
    if not isinstance(value["configuration_schema_version"], str) or not value["configuration_schema_version"]:
        _fail(f"{label} configuration schema version is invalid")
    try:
        manifest = VERIFIER.verify(manifest_path, root, trust_root=trust_root)
    except VERIFIER.InvalidManifest as error:
        _fail(f"{label} delivery manifest rejected: {error}")
    if value["image"]["sha256"] != manifest["image"]["digest"][7:]:
        _fail(f"{label} image is not bound to delivery manifest")
    try:
        image = json.loads(image_raw)
    except (TypeError, ValueError, UnicodeDecodeError) as error:
        _fail(f"{label} image is not valid JSON: {error}")
    if canonical_json(image) != image_raw or image.get("schemaVersion") != 2 or image.get("mediaType") != "application/vnd.oci.image.manifest.v1+json":
        _fail(f"{label} image is not a canonical OCI manifest")
    if value["migration"]["path"] != manifest["configuration"]["migration"]:
        _fail(f"{label} migration path is not bound to delivery manifest")
    if value["configuration_schema_version"] != manifest["configuration"]["schema_version"]:
        _fail(f"{label} configuration schema is not bound to delivery manifest")
    return {
        "manifest": manifest,
        "manifest_sha256": _sha256(manifest_raw),
        "image_sha256": _sha256(image_raw),
        "migration_sha256": _sha256(migration_raw),
        "migration_path": migration_path,
        "image_path": image_path,
    }


def _check_continuity(previous, candidate, rollback):
    _exact_keys(rollback, {"target_manifest_sha256"}, "rollback")
    if rollback["target_manifest_sha256"] != previous["manifest_sha256"]:
        _fail("rollback target is not bound to previous delivery manifest")
    if candidate["manifest_sha256"] == previous["manifest_sha256"]:
        _fail("candidate and previous releases must be distinct")
    candidate_manifest = candidate["manifest"]
    previous_manifest = previous["manifest"]
    if candidate_manifest["component"]["name"] != previous_manifest["component"]["name"]:
        _fail("candidate and previous components are discontinuous")
    if candidate_manifest["source"]["repository"] != previous_manifest["source"]["repository"]:
        _fail("candidate and previous repositories are discontinuous")
    if candidate["image_sha256"] == previous["image_sha256"]:
        _fail("candidate and previous images must be distinct")
    if candidate_manifest["component"]["version"] == previous_manifest["component"]["version"]:
        _fail("candidate and previous versions must be distinct")
    if candidate_manifest["source"]["commit"] == previous_manifest["source"]["commit"]:
        _fail("candidate and previous commits must be distinct")


def rehearse(plan_path, root, trust_root):
    root = Path(root).resolve(strict=True)
    plan_file, plan, plan_raw = _canonical_path(root, plan_path, MAX_PLAN_BYTES, "rehearsal plan")
    _exact_keys(plan, {"schema_version", "candidate", "previous", "rollback"}, "rehearsal plan")
    if plan["schema_version"] != "leanctx.deployment-rehearsal/v1":
        _fail("rehearsal plan has unsupported schema")
    trust_file, trust_raw = _read_path(root, trust_root, 16 * 1024, "trust root")
    previous = _release(root, plan["previous"], str(trust_file.relative_to(root)), "previous")
    candidate = _release(root, plan["candidate"], str(trust_file.relative_to(root)), "candidate")
    _check_continuity(previous, candidate, plan["rollback"])
    transitions = [
        {"phase": "previous-active", "scope": "in-memory-simulation", "manifest_sha256": previous["manifest_sha256"]},
        {"phase": "candidate-active", "scope": "in-memory-simulation", "manifest_sha256": candidate["manifest_sha256"]},
        {"phase": "previous-restored", "scope": "in-memory-simulation", "manifest_sha256": previous["manifest_sha256"]},
    ]
    return {
        "schema_version": "leanctx.deployment-rehearsal-evidence/v1",
        "status": "passed",
        "rehearsal_kind": "hermetic-local-no-deployment",
        "plan_sha256": _sha256(plan_raw),
        "trust_root_sha256": _sha256(trust_raw),
        "previous_manifest_sha256": previous["manifest_sha256"],
        "candidate_manifest_sha256": candidate["manifest_sha256"],
        "transitions": transitions,
    }


def confined_output(root, relative):
    root = Path(root).resolve(strict=True)
    try:
        parts = VERIFIER._relative_parts(relative)
    except VERIFIER.InvalidManifest as error:
        _fail(str(error))
    current = root
    for part in parts[:-1]:
        current = current / part
        try:
            metadata = os.lstat(current)
        except OSError as error:
            _fail(f"output parent is unavailable: {error}")
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
            _fail("output parent uses a symlink or is not a directory")
    output = current / parts[-1]
    try:
        os.lstat(output)
    except FileNotFoundError:
        return output
    except OSError as error:
        _fail(f"output cannot be inspected: {error}")
    _fail("output already exists or is a symlink")


def write_new(path, content):
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
    except OSError as error:
        _fail(f"cannot create rehearsal evidence: {error}")
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as error:
        _fail(f"cannot write rehearsal evidence: {error}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan")
    parser.add_argument("--root", required=True)
    parser.add_argument("--trust-root", required=True)
    parser.add_argument("--output", required=True)
    arguments = parser.parse_args(argv)
    try:
        evidence = rehearse(arguments.plan, arguments.root, arguments.trust_root)
        output = confined_output(arguments.root, arguments.output)
        write_new(output, canonical_json(evidence))
    except InvalidRehearsal as error:
        print(f"delivery rehearsal rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
