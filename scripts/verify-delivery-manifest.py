#!/usr/bin/env python3
"""Fail-closed verifier for canonical ``leanctx.delivery/v1`` manifests."""

import argparse
import base64
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path


MAX_MANIFEST_BYTES = 256 * 1024
MAX_ARTIFACT_BYTES = 8 * 1024 * 1024
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SEMVER_RE = re.compile(
    r"^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-(?:0|[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)


class InvalidManifest(ValueError):
    """The supplied release evidence is not safe to promote."""


def canonical_json(value):
    return (json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")


def _fail(message):
    raise InvalidManifest(message)


def _exact_keys(value, keys, label):
    if not isinstance(value, dict) or set(value) != set(keys):
        _fail(f"{label} must contain exactly {sorted(keys)}")


def _sha256(value):
    return hashlib.sha256(value).hexdigest()


def _require_sha256(value, label, prefix=False):
    if not isinstance(value, str):
        _fail(f"{label} must be a SHA-256 digest")
    raw = value[7:] if prefix else value
    if (prefix and not value.startswith("sha256:")) or not SHA256_RE.fullmatch(raw):
        _fail(f"{label} must be a SHA-256 digest")
    return raw


def _relative_parts(relative):
    if not isinstance(relative, str) or not relative or "\\" in relative:
        _fail("path must be a non-empty repository-relative POSIX path")
    candidate = Path(relative)
    if candidate.is_absolute() or any(part in {"", ".", ".."} for part in candidate.parts):
        _fail("path escapes repository root")
    return candidate.parts


def _regular_file(path, limit, label):
    try:
        metadata = os.lstat(path)
    except OSError as error:
        _fail(f"{label} is unavailable: {error}")
    if stat.S_ISLNK(metadata.st_mode):
        _fail(f"{label} uses a symlink path")
    if not stat.S_ISREG(metadata.st_mode):
        _fail(f"{label} is not a regular file")
    if metadata.st_size > limit:
        _fail(f"{label} exceeds byte bound")
    try:
        with open(path, "rb") as handle:
            value = handle.read(limit + 1)
    except OSError as error:
        _fail(f"{label} cannot be read: {error}")
    if len(value) > limit:
        _fail(f"{label} exceeds byte bound")
    return value


def _regular_followed_file(path, limit, label):
    """Bounded reader for caller-owned direct key inspection only.

    Promotion paths call ``confined_file`` first; this helper lets the rotation
    test construct an otherwise valid reference to a symlink that the policy
    parser subsequently rejects.
    """
    try:
        metadata = os.stat(path)
    except OSError as error:
        _fail(f"{label} is unavailable: {error}")
    if not stat.S_ISREG(metadata.st_mode):
        _fail(f"{label} is not a regular file")
    if metadata.st_size > limit:
        _fail(f"{label} exceeds byte bound")
    try:
        with open(path, "rb") as handle:
            value = handle.read(limit + 1)
    except OSError as error:
        _fail(f"{label} cannot be read: {error}")
    if len(value) > limit:
        _fail(f"{label} exceeds byte bound")
    return value


def confined_file(root, relative):
    root = Path(root).resolve(strict=True)
    current = root
    for part in _relative_parts(relative):
        current = current / part
        try:
            metadata = os.lstat(current)
        except OSError as error:
            _fail(f"referenced path is unavailable: {error}")
        if stat.S_ISLNK(metadata.st_mode):
            _fail("referenced path uses a symlink path")
    if not stat.S_ISREG(os.lstat(current).st_mode):
        _fail("referenced path is not a regular file")
    return current


def _path_under_root(root, path, limit, label):
    root = Path(root).resolve(strict=True)
    candidate = Path(path)
    try:
        relative = candidate.relative_to(root) if candidate.is_absolute() else candidate
    except ValueError:
        _fail(f"{label} escapes repository root")
    return confined_file(root, str(relative)), _regular_file(confined_file(root, str(relative)), limit, label)


def _canonical_object(path, limit, label):
    raw = _regular_file(path, limit, label)
    try:
        value = json.loads(raw)
    except (TypeError, ValueError, UnicodeDecodeError) as error:
        _fail(f"{label} is not valid JSON: {error}")
    if canonical_json(value) != raw:
        _fail(f"{label} is not canonical JSON")
    return value, raw


class TRUST:
    """Minimal strict RFC 8032 Ed25519 and offline rotation support."""

    TrustError = type("TrustError", (ValueError,), {})
    MAX_ROTATION_PLAN_BYTES = 256 * 1024
    _Q = 2 ** 255 - 19
    _L = 2 ** 252 + 27742317777372353535851937790883648493
    _D = (-121665 * pow(121666, 2 ** 255 - 21, 2 ** 255 - 19)) % (2 ** 255 - 19)
    _I = pow(2, (2 ** 255 - 19 - 1) // 4, 2 ** 255 - 19)
    _IDENTITY = (0, 1)

    @staticmethod
    def canonical_json(value):
        return canonical_json(value)

    @classmethod
    def _raise(cls, message):
        raise cls.TrustError(message)

    @classmethod
    def _recover_x(cls, y):
        x_squared = ((y * y - 1) * pow(cls._D * y * y + 1, cls._Q - 2, cls._Q)) % cls._Q
        x = pow(x_squared, (cls._Q + 3) // 8, cls._Q)
        if (x * x - x_squared) % cls._Q:
            x = (x * cls._I) % cls._Q
        if (x * x - x_squared) % cls._Q:
            cls._raise("invalid Ed25519 point")
        return x

    @classmethod
    def _base(cls):
        y = (4 * pow(5, cls._Q - 2, cls._Q)) % cls._Q
        x = cls._recover_x(y)
        return (x if x & 1 == 0 else cls._Q - x, y)

    @classmethod
    def _add(cls, first, second):
        x1, y1 = first
        x2, y2 = second
        denominator = (cls._D * x1 * x2 * y1 * y2) % cls._Q
        x = ((x1 * y2 + x2 * y1) * pow(1 + denominator, cls._Q - 2, cls._Q)) % cls._Q
        y = ((y1 * y2 + x1 * x2) * pow(1 - denominator, cls._Q - 2, cls._Q)) % cls._Q
        return x, y

    @classmethod
    def _scale(cls, point, scalar):
        result = cls._IDENTITY
        current = point
        while scalar:
            if scalar & 1:
                result = cls._add(result, current)
            current = cls._add(current, current)
            scalar >>= 1
        return result

    @classmethod
    def _encode_point(cls, point):
        x, y = point
        return (y | ((x & 1) << 255)).to_bytes(32, "little")

    @classmethod
    def _decode_point(cls, value):
        if not isinstance(value, bytes) or len(value) != 32:
            cls._raise("Ed25519 point must be 32 bytes")
        encoded = int.from_bytes(value, "little")
        sign = encoded >> 255
        y = encoded & ((1 << 255) - 1)
        if y >= cls._Q:
            cls._raise("non-canonical Ed25519 point")
        x = cls._recover_x(y)
        if x == 0 and sign:
            cls._raise("non-canonical Ed25519 point")
        if (x & 1) != sign:
            x = cls._Q - x
        return x, y

    @classmethod
    def _small_order(cls, point):
        return cls._scale(point, 8) == cls._IDENTITY

    @classmethod
    def public_from_seed(cls, seed):
        if not isinstance(seed, bytes) or len(seed) != 32:
            cls._raise("Ed25519 seed must be 32 bytes")
        digest = hashlib.sha512(seed).digest()
        scalar = int.from_bytes(digest[:32], "little")
        scalar &= (1 << 254) - 8
        scalar |= 1 << 254
        return cls._encode_point(cls._scale(cls._base(), scalar))

    @classmethod
    def ed25519_sign(cls, message, seed):
        if not isinstance(message, bytes):
            cls._raise("Ed25519 message must be bytes")
        digest = hashlib.sha512(seed).digest()
        scalar = int.from_bytes(digest[:32], "little")
        scalar &= (1 << 254) - 8
        scalar |= 1 << 254
        nonce = int.from_bytes(hashlib.sha512(digest[32:] + message).digest(), "little") % cls._L
        encoded_r = cls._encode_point(cls._scale(cls._base(), nonce))
        encoded_public = cls.public_from_seed(seed)
        challenge = int.from_bytes(hashlib.sha512(encoded_r + encoded_public + message).digest(), "little") % cls._L
        return encoded_r + ((nonce + challenge * scalar) % cls._L).to_bytes(32, "little")

    @classmethod
    def ed25519_verify(cls, signature, message, public):
        try:
            if not isinstance(signature, bytes) or len(signature) != 64 or not isinstance(message, bytes):
                return False
            encoded_r, encoded_s = signature[:32], signature[32:]
            scalar = int.from_bytes(encoded_s, "little")
            if scalar >= cls._L:
                return False
            point_r = cls._decode_point(encoded_r)
            point_a = cls._decode_point(public)
            if cls._small_order(point_r) or cls._small_order(point_a):
                return False
            challenge = int.from_bytes(hashlib.sha512(encoded_r + public + message).digest(), "little") % cls._L
            return cls._scale(cls._base(), scalar) == cls._add(point_r, cls._scale(point_a, challenge))
        except cls.TrustError:
            return False

    @classmethod
    def _base64(cls, value, size, label):
        if not isinstance(value, str):
            cls._raise(f"{label} must be canonical base64")
        try:
            decoded = base64.b64decode(value, validate=True)
        except (ValueError, TypeError) as error:
            cls._raise(f"{label} must be canonical base64: {error}")
        if len(decoded) != size or base64.b64encode(decoded).decode("ascii") != value:
            cls._raise(f"{label} must be canonical base64")
        return decoded

    @classmethod
    def read_public_key(cls, path):
        try:
            raw = _regular_followed_file(Path(path), 16 * 1024, "trust root")
            value = json.loads(raw)
            if canonical_json(value) != raw:
                _fail("trust root is not canonical JSON")
        except InvalidManifest as error:
            cls._raise(str(error))
        except (TypeError, ValueError, UnicodeDecodeError) as error:
            cls._raise(f"trust root is not valid JSON: {error}")
        if not isinstance(value, dict) or set(value) != {"algorithm", "key_id", "public_key"}:
            cls._raise("trust root has an invalid schema")
        if value["algorithm"] != "Ed25519":
            cls._raise("trust root algorithm must be Ed25519")
        public = cls._base64(value["public_key"], 32, "trust root public key")
        try:
            point = cls._decode_point(public)
        except cls.TrustError:
            cls._raise("trust root public key is invalid")
        if cls._small_order(point):
            cls._raise("trust root public key has small order")
        key_id = "sha256:" + _sha256(public)
        if value["key_id"] != key_id:
            cls._raise("trust root key ID mismatch")
        return public, key_id

    @classmethod
    def promotion_payload(cls, manifest):
        evidence = manifest["evidence"]
        return {
            "component": manifest["component"],
            "configuration": manifest["configuration"],
            "contracts": manifest["contracts"],
            "evidence": {
                "provenance": evidence["provenance"],
                "sbom": evidence["sbom"],
                "vulnerability_report": evidence["vulnerability_report"],
            },
            "image": manifest["image"],
            "schema_version": manifest["schema_version"],
            "source": manifest["source"],
        }

    @classmethod
    def verify_receipt(cls, manifest, receipt, public, expected_key_id):
        if not isinstance(receipt, dict) or set(receipt) != {"algorithm", "key_id", "payload_sha256", "schema_version", "signature"}:
            cls._raise("release receipt has an invalid schema")
        if receipt["schema_version"] != "leanctx.release-signature/v1" or receipt["algorithm"] != "Ed25519":
            cls._raise("release receipt has an unsupported algorithm or schema")
        if receipt["key_id"] != expected_key_id:
            cls._raise("release receipt key is not allowed")
        payload = cls.canonical_json(cls.promotion_payload(manifest))
        if receipt["payload_sha256"] != _sha256(payload):
            cls._raise("release receipt payload digest mismatch")
        signature = cls._base64(receipt["signature"], 64, "release receipt signature")
        if not cls.ed25519_verify(signature, payload, public):
            cls._raise("release receipt signature verification failed")

    @classmethod
    def _read_plan_reference(cls, value, root, label):
        if not isinstance(value, dict) or set(value) != {"path", "sha256", "key_id"}:
            cls._raise(f"{label} has invalid reference fields")
        try:
            path = confined_file(root, value["path"])
            raw = _regular_file(path, 16 * 1024, label)
        except InvalidManifest as error:
            cls._raise(str(error))
        expected_file_id = "sha256:" + _sha256(raw)
        if value["sha256"] != expected_file_id:
            cls._raise(f"{label} file content ID mismatch")
        public, key_id = cls.read_public_key(path)
        if value["key_id"] != key_id:
            cls._raise(f"{label} key content ID mismatch")
        return {"path": path, "public": public, "key_id": key_id, "file_id": expected_file_id}

    @classmethod
    def read_rotation_plan(cls, path, root):
        try:
            plan_path, _ = _path_under_root(root, path, cls.MAX_ROTATION_PLAN_BYTES, "rotation plan")
            value, _ = _canonical_object(plan_path, cls.MAX_ROTATION_PLAN_BYTES, "rotation plan")
        except InvalidManifest as error:
            cls._raise(str(error))
        if not isinstance(value, dict) or set(value) != {"schema_version", "old_trust_root", "new_trust_root", "transition"}:
            cls._raise("rotation plan has invalid schema")
        if value["schema_version"] != "leanctx.release-key-rotation/v1":
            cls._raise("rotation plan has unsupported schema")
        old = cls._read_plan_reference(value["old_trust_root"], root, "old trust root")
        new = cls._read_plan_reference(value["new_trust_root"], root, "new trust root")
        if old["key_id"] == new["key_id"] or old["public"] == new["public"]:
            cls._raise("old and new trust roots must be distinct")
        transition = value["transition"]
        if not isinstance(transition, dict) or set(transition) != {"activation", "overlap", "revocation"}:
            cls._raise("rotation transition has invalid schema")
        states = (transition["activation"], transition["overlap"], transition["revocation"])
        allowed = {
            ("pending", "inactive", "not-started"): {"old"},
            ("complete", "active", "pending"): {"old", "new"},
            ("complete", "complete", "old-key-revoked"): {"new"},
        }.get(states)
        if allowed is None:
            cls._raise("rotation transition is unsupported")
        return {"old": old, "new": new, "allowed": allowed, "id": "sha256:" + _sha256(canonical_json(value))}

    @classmethod
    def verify_rotation(cls, manifest, receipt, plan, root):
        rotation = cls.read_rotation_plan(plan, root)
        key_id = receipt.get("key_id") if isinstance(receipt, dict) else None
        role = "old" if key_id == rotation["old"]["key_id"] else "new" if key_id == rotation["new"]["key_id"] else None
        if role not in rotation["allowed"]:
            cls._raise("receipt key is not allowed by rotation transition")
        trust_root = rotation[role]
        cls.verify_receipt(manifest, receipt, trust_root["public"], trust_root["key_id"])
        return {"schema_version": "leanctx.release-key-rotation-evidence/v1", "rotation_plan_id": rotation["id"], "accepted_role": role, "trust_root_id": trust_root["file_id"], "key_id": trust_root["key_id"]}


def _artifact(root, value, label):
    _exact_keys(value, {"path", "sha256"}, label)
    path = confined_file(root, value["path"])
    raw = _regular_file(path, MAX_ARTIFACT_BYTES, label)
    if value["sha256"] != _sha256(raw):
        _fail(f"{label} digest mismatch")
    return path, raw


def verify_contract_pack_metadata(pack, root=None):
    _exact_keys(pack, {"artifacts", "compatibility", "schema_version", "version"}, "contract pack")
    if pack["schema_version"] != "leanctx.contract-pack/v1" or not isinstance(pack["version"], str) or not SEMVER_RE.fullmatch(pack["version"]):
        _fail("contract pack has invalid metadata")
    compatibility = pack["compatibility"]
    _exact_keys(compatibility, {"policy", "supported"}, "contract pack compatibility")
    major = int(pack["version"].split(".", 1)[0])
    expected = [pack["version"]]
    if major > 1:
        expected.append(f"{major - 1}.0.0")
    if compatibility["policy"] != "N-and-N-minus-1" or compatibility["supported"] != expected:
        _fail("contract pack compatibility set is not the closed N/N-1 set")
    if not isinstance(pack["artifacts"], list) or not pack["artifacts"]:
        _fail("contract pack must list immutable artifacts")
    seen = set()
    for artifact in pack["artifacts"]:
        _exact_keys(artifact, {"path", "sha256"}, "contract pack artifact")
        relative = artifact["path"]
        if not isinstance(relative, str) or relative in seen:
            _fail("contract pack artifact path is invalid or duplicated")
        seen.add(relative)
        _require_sha256(artifact["sha256"], "contract pack artifact digest")
        if root is not None:
            path = confined_file(root, relative)
            raw = _regular_file(path, MAX_ARTIFACT_BYTES, "contract pack artifact")
            if artifact["sha256"] != _sha256(raw):
                _fail("contract pack artifact digest mismatch")


def _check_manifest(manifest):
    _exact_keys(manifest, {"schema_version", "component", "source", "image", "configuration", "contracts", "evidence"}, "delivery manifest")
    if manifest["schema_version"] != "leanctx.delivery/v1":
        _fail("unsupported delivery manifest schema")
    _exact_keys(manifest["component"], {"name", "version"}, "component")
    if not isinstance(manifest["component"]["name"], str) or not manifest["component"]["name"] or not isinstance(manifest["component"]["version"], str) or not SEMVER_RE.fullmatch(manifest["component"]["version"]):
        _fail("component identity is invalid")
    _exact_keys(manifest["source"], {"repository", "commit"}, "source")
    if not isinstance(manifest["source"]["repository"], str) or not manifest["source"]["repository"].startswith("https://") or not isinstance(manifest["source"]["commit"], str) or not COMMIT_RE.fullmatch(manifest["source"]["commit"]):
        _fail("source identity is invalid")
    _exact_keys(manifest["image"], {"reference", "digest"}, "image")
    reference = manifest["image"]["reference"]
    if not isinstance(reference, str) or not reference or "@" in reference or ":" in reference.rsplit("/", 1)[-1]:
        _fail("image reference is mutable")
    _require_sha256(manifest["image"]["digest"], "image digest", prefix=True)
    _exact_keys(manifest["configuration"], {"schema_version", "migration"}, "configuration")
    if not isinstance(manifest["configuration"]["schema_version"], str) or not manifest["configuration"]["schema_version"] or not isinstance(manifest["configuration"]["migration"], str):
        _fail("configuration is invalid")
    _exact_keys(manifest["contracts"], {"pack_version", "pack_digest"}, "contracts")
    if not isinstance(manifest["contracts"]["pack_version"], str) or not SEMVER_RE.fullmatch(manifest["contracts"]["pack_version"]):
        _fail("contract pack version is invalid")
    _require_sha256(manifest["contracts"]["pack_digest"], "contract pack digest", prefix=True)
    _exact_keys(manifest["evidence"], {"sbom", "provenance", "signature", "vulnerability_report"}, "evidence")


def verify(manifest_path, root, trust_root=None, rotation_plan=None):
    if (trust_root is None) == (rotation_plan is None):
        _fail("exactly one trust policy is required")
    manifest_path, _ = _path_under_root(root, manifest_path, MAX_MANIFEST_BYTES, "delivery manifest")
    manifest, _ = _canonical_object(manifest_path, MAX_MANIFEST_BYTES, "delivery manifest")
    _check_manifest(manifest)
    root = Path(root).resolve(strict=True)
    pack_path = confined_file(root, "docs/contracts/ocla-contract-pack-v1.json")
    pack_raw = _regular_file(pack_path, MAX_ARTIFACT_BYTES, "contract pack")
    try:
        pack = json.loads(pack_raw)
    except (TypeError, ValueError, UnicodeDecodeError) as error:
        _fail(f"contract pack is not valid JSON: {error}")
    verify_contract_pack_metadata(pack, root)
    if manifest["contracts"]["pack_digest"] != "sha256:" + _sha256(canonical_json(pack)) or manifest["contracts"]["pack_version"] != pack.get("version"):
        _fail("contract pack binding mismatch")
    # Promotion accepts an explicit pack version; the separate metadata gate
    # enforces the compatibility publication policy before release.
    _artifact(root, manifest["evidence"]["sbom"], "SBOM")
    provenance_path, provenance_raw = _artifact(root, manifest["evidence"]["provenance"], "provenance")
    _artifact(root, manifest["evidence"]["vulnerability_report"], "vulnerability report")
    signature_path, _ = _artifact(root, manifest["evidence"]["signature"], "signature")
    migration = confined_file(root, manifest["configuration"]["migration"])
    _regular_file(migration, MAX_ARTIFACT_BYTES, "migration")
    try:
        sbom, _ = _canonical_object(confined_file(root, manifest["evidence"]["sbom"]["path"]), MAX_ARTIFACT_BYTES, "SBOM")
        provenance = json.loads(provenance_raw)
        vulnerability, _ = _canonical_object(confined_file(root, manifest["evidence"]["vulnerability_report"]["path"]), MAX_ARTIFACT_BYTES, "vulnerability report")
        receipt, _ = _canonical_object(signature_path, MAX_ARTIFACT_BYTES, "signature")
    except InvalidManifest:
        raise
    if sbom.get("bomFormat") != "CycloneDX" or sbom.get("metadata", {}).get("component", {}).get("name") != manifest["component"]["name"] or sbom.get("metadata", {}).get("component", {}).get("version") != manifest["component"]["version"]:
        _fail("SBOM does not bind the delivered component")
    source = provenance.get("predicate", {}).get("buildDefinition", {}).get("externalParameters", {}).get("source")
    subject = provenance.get("subject")
    if provenance.get("_type") != "https://in-toto.io/Statement/v1" or provenance.get("predicateType") != "https://slsa.dev/provenance/v1" or source != manifest["source"] or not isinstance(subject, list) or subject != [{"name": manifest["image"]["reference"], "digest": {"sha256": manifest["image"]["digest"][7:]}}]:
        _fail("provenance does not bind delivered source and image")
    if vulnerability.get("artifactName") != manifest["image"]["reference"]:
        _fail("vulnerability report does not bind delivered image")
    try:
        if rotation_plan is not None:
            TRUST.verify_rotation(manifest, receipt, rotation_plan, root)
        else:
            trust_path, _ = _path_under_root(root, trust_root, 16 * 1024, "trust root")
            public, key_id = TRUST.read_public_key(trust_path)
            TRUST.verify_receipt(manifest, receipt, public, key_id)
    except TRUST.TrustError as error:
        _fail(str(error))
    return manifest


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest")
    parser.add_argument("--root", required=True)
    policy = parser.add_mutually_exclusive_group(required=True)
    policy.add_argument("--trust-root")
    policy.add_argument("--rotation-plan")
    arguments = parser.parse_args(argv)
    try:
        verify(arguments.manifest, arguments.root, arguments.trust_root, arguments.rotation_plan)
    except InvalidManifest as error:
        print(f"delivery manifest rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
