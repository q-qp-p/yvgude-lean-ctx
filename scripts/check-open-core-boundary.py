#!/usr/bin/env python3
"""Check the public Rust tree against the documented open-core boundary.

GitLab CI integration TODO:

    boundary-check:
      script: python3 scripts/check-open-core-boundary.py
      rules:
        - changes: ["rust/**", "docs/contracts/**"]

The classification document is intentionally optional during the staged
rollout.  Import and strategic-data checks still run when it is absent.
"""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path
from typing import Iterable, List, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[1]
CLASSIFICATION_PATH = ROOT / "docs/internal/architecture/MODULE_CLASSIFICATION.md"
RUST_SOURCE_ROOT = ROOT / "rust/src"
RUST_CRATES_ROOT = ROOT / "rust/crates"

CLASS_TOKEN = re.compile(
    r"\b(?:class|classification)\s*[:=\-]?\s*([A-E])\b", re.IGNORECASE
)
INLINE_PATH = re.compile(r"(?:^|\s)(rust/(?:src|crates)/[^\s|)`]+)")
PRIVATE_IMPORT = re.compile(
    r"(?:^|::)(?:"
    r"lean[_-]?ctx[_-]?(?:enterprise|private)|"
    r"leanctx[_-]?(?:enterprise|private)|"
    r"private|proprietary|commercial|strategic_data|"
    r"enterprise::(?:control_plane|scheduler|economics|knowledge_hub|fleet)"
    r")(?:$|::)",
    re.IGNORECASE,
)

BENCHMARK_CORPUS_PATH = re.compile(
    r"(?:^|/)(?:benchmark[_-]?(?:corpus|dataset)|"
    r"private[_-]?benchmark|customer[_-]?benchmark)(?:[._/-]|$)",
    re.IGNORECASE,
)
PROVIDER_RATE_PATH = re.compile(
    r"(?:^|/)(?:provider[_-]?(?:rate|rates|pricing|prices|costs)|"
    r"model[_-]?rates|rate[_-]?card|pricing[_-]?table)(?:[._/-]|$)",
    re.IGNORECASE,
)
BENCHMARK_CORPUS_DATA = re.compile(
    r"\b(?:private|customer|proprietary)\s+benchmark\s+(?:corpus|dataset)\b|"
    r"\bbenchmark[_-]?(?:corpus|dataset)\b",
    re.IGNORECASE,
)
PROVIDER_RATE_DATA = re.compile(
    r"(?:\"(?:provider|model)_(?:rate|rates|pricing|price|prices)\"\s*:|"
    r"\b(?:provider|model)_(?:rate|rates|pricing|price|prices)_micros\b|"
    r"\b(?:provider|model)_(?:rate|rates|pricing|price|prices)\s*=|"
    r"\b(?:input|output|cached|reasoning)_rate_micros\b)",
    re.IGNORECASE,
)


class ClassificationRule:
    __slots__ = ("pattern", "class_name", "line_number")

    def __init__(self, pattern: str, class_name: str, line_number: int):
        self.pattern = pattern
        self.class_name = class_name
        self.line_number = line_number


def relative_path(path: Path) -> str:
    """Return a stable POSIX path relative to the repository root."""

    return path.relative_to(ROOT).as_posix()


def rust_modules() -> List[Path]:
    """Enumerate every Rust module in rust/src and rust/crates."""

    modules: List[Path] = []
    for root in (RUST_SOURCE_ROOT, RUST_CRATES_ROOT):
        if root.is_dir():
            modules.extend(path for path in root.rglob("*.rs") if path.is_file())
    return sorted(modules, key=relative_path)


def public_path(path: Path) -> bool:
    """Return whether a path is in the public Rust surface checked here."""

    name = relative_path(path)
    return name.startswith("rust/src/") or name.startswith("rust/crates/lean-ctx-")


def normalize_pattern(candidate: str) -> Optional[str]:
    """Normalize a classification path/glob into a repository-relative form."""

    value = candidate.strip().strip("`\"'")
    value = re.sub(r"^\./", "", value).replace("\\", "/")
    value = value.rstrip(",;:)")
    if not value:
        return None
    if value.startswith(str(ROOT).replace("\\", "/") + "/"):
        value = value[len(str(ROOT).replace("\\", "/")) + 1 :]
    if value.startswith("src/") or value.startswith("crates/"):
        value = "rust/" + value
    if not value.startswith("rust/"):
        return None
    return value


def path_candidates(line: str) -> List[str]:
    """Extract path/glob cells from a Markdown classification line."""

    candidates: List[str] = []
    candidates.extend(re.findall(r"`([^`]+)`", line))
    candidates.extend(INLINE_PATH.findall(line))
    if "|" in line:
        candidates.extend(cell.strip() for cell in line.split("|") if cell.strip())
    normalized: List[str] = []
    for candidate in candidates:
        match = re.search(r"(?:^|\s)(rust/(?:src|crates)/[^\s|)`]+)", candidate)
        value = match.group(1) if match else candidate
        normalized_value = normalize_pattern(value)
        if normalized_value and normalized_value not in normalized:
            normalized.append(normalized_value)
    return normalized


def class_from_line(line: str) -> Optional[str]:
    """Extract an explicit A-E class from a heading, list item, or table row."""

    match = CLASS_TOKEN.search(line)
    if match:
        return match.group(1).upper()
    heading = re.match(r"^\s*#{1,6}\s*(?:class\s*)?([A-E])(?:\s|[-—:]|$)", line, re.I)
    if heading:
        return heading.group(1).upper()
    bold = re.search(r"\*\*([A-E])\*\*", line, re.I)
    if bold:
        return bold.group(1).upper()
    if "|" in line:
        for cell in line.split("|"):
            if re.fullmatch(r"\s*([A-E])\s*", cell, re.IGNORECASE):
                return cell.strip().upper()
    return None


def parse_classification_document(
    path: Path,
) -> Tuple[List[ClassificationRule], List[str]]:
    """Parse both explicit rows and class-scoped path lists."""

    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError) as error:
        return [], ["cannot read classification document: %s" % error]

    rules: List[ClassificationRule] = []
    errors: List[str] = []
    current_class: Optional[str] = None
    for line_number, line in enumerate(lines, start=1):
        heading = re.match(r"^\s*#{1,6}\s+.*", line)
        explicit_class = class_from_line(line)
        if heading and explicit_class:
            current_class = explicit_class
        candidates = path_candidates(line)
        if not candidates:
            continue
        selected_class = explicit_class or current_class
        if selected_class is None:
            errors.append(
                "classification path without class at line %d: %s"
                % (line_number, line.strip())
            )
            continue
        for pattern in candidates:
            rules.append(ClassificationRule(pattern, selected_class, line_number))
    return rules, errors


def pattern_matches(pattern: str, path_name: str) -> bool:
    """Match exact paths, directory rules, and ordinary Markdown globs."""

    alternatives = (path_name, path_name.removeprefix("rust/"))
    for candidate in alternatives:
        if fnmatch.fnmatchcase(candidate, pattern):
            return True
        if candidate == pattern or candidate.startswith(pattern.rstrip("/") + "/"):
            return True
    return False


def best_classification(path: Path, rules: Sequence[ClassificationRule]) -> Optional[str]:
    """Choose the most-specific matching rule, preserving deterministic output."""

    name = relative_path(path)
    matches = [rule for rule in rules if pattern_matches(rule.pattern, name)]
    if not matches:
        return None
    matches.sort(key=lambda rule: (len(rule.pattern.replace("*", "")), rule.line_number), reverse=True)
    return matches[0].class_name


def check_classifications(modules: Sequence[Path]) -> List[str]:
    """Check module coverage and reject D/E assignments in public Rust paths."""

    if not CLASSIFICATION_PATH.exists():
        return []

    rules, parse_errors = parse_classification_document(CLASSIFICATION_PATH)
    violations = ["[classification] %s" % error for error in parse_errors]
    if not rules:
        violations.append(
            "[classification] no usable path/class rules found in %s"
            % relative_path(CLASSIFICATION_PATH)
        )
    for module in modules:
        class_name = best_classification(module, rules)
        name = relative_path(module)
        if class_name is None:
            violations.append("[classification] %s has no A-E classification" % name)
        elif class_name in ("D", "E") and public_path(module):
            violations.append(
                "[public-private-boundary] %s is Class %s in a public path"
                % (name, class_name)
            )
    return violations


def check_private_imports(modules: Sequence[Path]) -> List[str]:
    """Reject imports from recognizable private namespaces in public modules."""

    violations: List[str] = []
    for module in modules:
        if not public_path(module):
            continue
        try:
            lines = module.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeError) as error:
            violations.append("[read] %s: %s" % (relative_path(module), error))
            continue
        for line_number, line in enumerate(lines, start=1):
            match = re.match(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);", line)
            if match and PRIVATE_IMPORT.search(match.group(1)):
                violations.append(
                    "[private-import] %s:%d imports private namespace: %s"
                    % (relative_path(module), line_number, match.group(1).strip())
                )
    return violations


def public_files() -> Iterable[Path]:
    """Yield files under the public source and public OCLA crate roots."""

    roots = [RUST_SOURCE_ROOT]
    if RUST_CRATES_ROOT.is_dir():
        roots.extend(sorted(RUST_CRATES_ROOT.glob("lean-ctx-*")))
    for root in roots:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*"), key=relative_path):
            if path.is_file():
                yield path


def check_strategic_data() -> List[str]:
    """Reject obvious private benchmark corpora and provider-rate data."""

    violations: List[str] = []
    for path in public_files():
        name = relative_path(path)
        reason: Optional[str] = None
        if BENCHMARK_CORPUS_PATH.search(name):
            reason = "benchmark corpus/dataset path"
        elif PROVIDER_RATE_PATH.search(name):
            reason = "provider rate/pricing path"
        else:
            try:
                if path.stat().st_size > 4 * 1024 * 1024:
                    continue
                lines = path.read_text(encoding="utf-8").splitlines()
            except (OSError, UnicodeError):
                continue
            for line in lines:
                if BENCHMARK_CORPUS_DATA.search(line):
                    reason = "benchmark corpus/dataset content"
                    break
                if PROVIDER_RATE_DATA.search(line):
                    reason = "provider rate/pricing data"
                    break
        if reason:
            violations.append("[strategic-data] %s contains %s" % (name, reason))
    return violations


MANIFEST_PATH = ROOT / "security/public-protocol-surface-freeze-v1.json"
MANIFEST_SCHEMA = "leanctx.public-protocol-surface-freeze/v1"
FROZEN_SURFACES = ("auto_routing", "control_plane", "fleet_control", "rollout", "value_share")
MANIFEST_BYTES = 256 * 1024
RELEASE_RE = re.compile(r"^v[0-9]+(?:\.[0-9]+)*$")
RUST_USE = re.compile(r"(?ms)^[ \t]*(?:(?:pub(?:\([^)]*\))?[ \t]+)?use\s+[^;]+;)")
RUST_RAW_STRING = re.compile(r'(?:br|cr|r)(#+)?"')
RUST_PUBLIC_ITEM = re.compile(
    r"^pub\s+(?:(?:async|unsafe)\s+)?(struct|enum|trait|fn|type|const|static|mod)\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)\b"
)


class ManifestError(ValueError):
    """A freeze manifest is unsafe to use as an allowlist."""


def canonical_json(value: object) -> bytes:
    """Serialize reports without filesystem, hash, or insertion-order noise."""

    return (json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")) + "\n").encode(
        "utf-8"
    )


def _unique_object(pairs: List[Tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ManifestError("duplicate JSON key: %s" % key)
        result[key] = value
    return result


def _exact_keys(value: object, keys: Sequence[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ManifestError("%s must contain exactly %s" % (label, sorted(keys)))


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ManifestError("%s must be a non-empty string" % label)
    return value


def _safe_relative(value: object, label: str) -> str:
    path = _string(value, label)
    if "\\" in path or path.startswith("/") or "//" in path:
        raise ManifestError("%s must be a repository-relative POSIX path" % label)
    parts = path.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        raise ManifestError("%s must not escape the repository" % label)
    return "/".join(parts)


def _bool(value: object, label: str) -> bool:
    if type(value) is not bool:
        raise ManifestError("%s must be boolean" % label)
    return value


def _sorted_unique_strings(value: object, label: str, allow_empty: bool = False) -> List[str]:
    if not isinstance(value, list) or (not allow_empty and not value):
        raise ManifestError("%s must be a non-empty list" % label)
    strings = [_string(item, "%s entry" % label) for item in value]
    if len(set(strings)) != len(strings):
        raise ManifestError("%s contains duplicate entries" % label)
    if strings != sorted(strings):
        raise ManifestError("%s must be sorted" % label)
    return strings


def _validate_manifest(value: object) -> dict:
    top_keys = (
        "schema_version",
        "policy_source_path",
        "owner",
        "status",
        "experimental",
        "v1",
        "review_release",
        "removal_not_before_release",
        "surfaces",
    )
    _exact_keys(value, top_keys, "manifest")
    if value["schema_version"] != MANIFEST_SCHEMA:
        raise ManifestError("unsupported schema_version")
    if value["policy_source_path"] != "security/public-protocol-surface-freeze-v1.json":
        raise ManifestError("policy_source_path must name the committed freeze manifest")
    metadata = {
        "owner": _string(value["owner"], "manifest.owner"),
        "status": _string(value["status"], "manifest.status"),
        "experimental": _bool(value["experimental"], "manifest.experimental"),
        "v1": _bool(value["v1"], "manifest.v1"),
        "review_release": _string(value["review_release"], "manifest.review_release"),
        "removal_not_before_release": _string(
            value["removal_not_before_release"], "manifest.removal_not_before_release"
        ),
    }
    if metadata["status"] != "experimental-non-v1" or not metadata["experimental"] or metadata["v1"]:
        raise ManifestError("manifest must mark every surface experimental and non-v1")
    for key in ("review_release", "removal_not_before_release"):
        if not RELEASE_RE.fullmatch(metadata[key]):
            raise ManifestError("manifest.%s is not a release identifier" % key)

    surfaces = value["surfaces"]
    if not isinstance(surfaces, dict) or set(surfaces) != set(FROZEN_SURFACES):
        raise ManifestError("surfaces must contain exactly %s" % list(FROZEN_SURFACES))

    surface_keys = (
        "owner",
        "status",
        "experimental",
        "v1",
        "review_release",
        "removal_not_before_release",
        "module_path",
        "module_sha256",
        "module_roots",
        "root_reexports",
        "exported_symbols",
        "approved_consumers",
    )
    for surface_name in FROZEN_SURFACES:
        surface = surfaces[surface_name]
        _exact_keys(surface, surface_keys, "surfaces.%s" % surface_name)
        for key, expected in metadata.items():
            if surface[key] != expected:
                raise ManifestError("surfaces.%s.%s disagrees with manifest metadata" % (surface_name, key))
        module_path = _safe_relative(surface["module_path"], "surfaces.%s.module_path" % surface_name)
        expected_module = "rust/crates/lean-ctx-protocol/src/%s.rs" % surface_name
        if module_path != expected_module:
            raise ManifestError("surfaces.%s.module_path is not the protocol module" % surface_name)
        module_sha256 = _string(
            surface["module_sha256"], "surfaces.%s.module_sha256" % surface_name
        )
        if not re.fullmatch(r"[0-9a-f]{64}", module_sha256):
            raise ManifestError(
                "surfaces.%s.module_sha256 must be lowercase SHA-256" % surface_name
            )
        for collection_name, allow_empty in (("module_roots", False), ("root_reexports", False), ("approved_consumers", True)):
            collection = surface[collection_name]
            if not isinstance(collection, list) or (not collection and not allow_empty):
                raise ManifestError("surfaces.%s.%s must be a list" % (surface_name, collection_name))
            seen_paths = set()
            for index, item in enumerate(collection):
                _exact_keys(item, ("path", "declarations") if collection_name == "module_roots" else ("path", "statements"), "%s.%s[%d]" % (surface_name, collection_name, index))
                path = _safe_relative(item["path"], "%s.%s[%d].path" % (surface_name, collection_name, index))
                if path in seen_paths:
                    raise ManifestError("surfaces.%s.%s repeats %s" % (surface_name, collection_name, path))
                seen_paths.add(path)
                field = "declarations" if collection_name == "module_roots" else "statements"
                _sorted_unique_strings(item[field], "%s.%s[%d].%s" % (surface_name, collection_name, index, field))
            if [item["path"] for item in collection] != sorted(item["path"] for item in collection):
                raise ManifestError("surfaces.%s.%s must be sorted by path" % (surface_name, collection_name))
        symbols = surface["exported_symbols"]
        if not isinstance(symbols, list) or not symbols:
            raise ManifestError("surfaces.%s.exported_symbols must be a non-empty list" % surface_name)
        parsed_symbols = []
        for index, item in enumerate(symbols):
            _exact_keys(item, ("kind", "name"), "surfaces.%s.exported_symbols[%d]" % (surface_name, index))
            kind = _string(item["kind"], "exported symbol kind")
            name = _string(item["name"], "exported symbol name")
            if kind not in {"const", "enum", "fn", "mod", "static", "struct", "trait", "type", "use"}:
                raise ManifestError("unsupported exported symbol kind: %s" % kind)
            parsed_symbols.append((kind, name))
        if len(set(parsed_symbols)) != len(parsed_symbols) or parsed_symbols != sorted(parsed_symbols):
            raise ManifestError("surfaces.%s.exported_symbols must be sorted and unique" % surface_name)
    return value


def load_manifest(path: Path) -> dict:
    """Read and validate a freeze manifest, rejecting ambiguity and truncation."""

    try:
        metadata = os.lstat(path)
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise ManifestError("manifest is not a regular file")
        if metadata.st_size > MANIFEST_BYTES:
            raise ManifestError("manifest exceeds byte bound")
        raw = path.read_bytes()
    except ManifestError:
        raise
    except (OSError, UnicodeError) as error:
        raise ManifestError("cannot read manifest: %s" % error) from error
    if len(raw) > MANIFEST_BYTES:
        raise ManifestError("manifest exceeds byte bound")
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object)
    except ManifestError:
        raise
    except (UnicodeDecodeError, TypeError, ValueError) as error:
        raise ManifestError("manifest is not valid UTF-8 JSON: %s" % error) from error
    return _validate_manifest(value)


def _root_path(root: Path, relative: str, label: str) -> Path:
    candidate = root / relative
    current = root
    try:
        for part in Path(relative).parts:
            current /= part
            metadata = os.lstat(current)
            if stat.S_ISLNK(metadata.st_mode):
                raise ManifestError("%s uses a symlink path" % label)
        metadata = os.lstat(candidate)
    except ManifestError:
        raise
    except OSError as error:
        raise ManifestError("%s is unavailable: %s" % (label, error)) from error
    if not stat.S_ISREG(metadata.st_mode):
        raise ManifestError("%s is not a regular file" % label)
    return candidate


def _relative(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


def _rust_files(root: Path) -> List[Path]:
    paths = []
    for source_root in (root / "rust/src", root / "rust/crates"):
        if source_root.is_dir():
            for path in source_root.rglob("*.rs"):
                metadata = os.lstat(path)
                if stat.S_ISLNK(metadata.st_mode):
                    raise ManifestError("Rust source uses a symlink path: %s" % _relative(root, path))
                if stat.S_ISREG(metadata.st_mode):
                    paths.append(path)
    return sorted(paths, key=lambda path: _relative(root, path))


def _read_source(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ManifestError("cannot read %s: %s" % (path, error)) from error


def _normalize_statement(statement: str) -> str:
    return " ".join(statement.strip().split())


def _mask_rust_non_code(source: str) -> str:
    """Mask comments and literals while preserving source offsets/newlines."""

    masked = list(source)
    length = len(source)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if source[index] not in "\r\n":
                masked[index] = " "

    def quoted_end(start: int, quote: str) -> int:
        index = start + 1
        while index < length:
            if source[index] == "\\":
                index += 2
            elif source[index] == quote:
                return index + 1
            else:
                index += 1
        return length

    index = 0
    while index < length:
        if source.startswith("//", index):
            end = source.find("\n", index)
            end = length if end < 0 else end
            blank(index, end)
            index = end
            continue
        if source.startswith("/*", index):
            end = index + 2
            depth = 1
            while end < length and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            blank(index, end)
            index = end
            continue

        raw = RUST_RAW_STRING.match(source, index)
        if raw:
            terminator = '"%s' % (raw.group(1) or "")
            end = source.find(terminator, raw.end())
            end = length if end < 0 else end + len(terminator)
            blank(index, end)
            index = end
            continue

        if source[index] == '"' or source[index : index + 2] in {"b\"", "c\""}:
            start = index
            quote = index if source[index] == '"' else index + 1
            end = quoted_end(quote, '"')
            blank(start, end)
            index = end
            continue

        if source[index] == "'":
            end = index + 1
            if end < length and source[end] == "\\":
                end += 2
            elif end < length:
                end += 1
            if end < length and source[end] == "'":
                blank(index, end + 1)
                index = end + 1
                continue

        index += 1
    return "".join(masked)


def _rust_views(source: str) -> Tuple[List[Tuple[int, str]], str]:
    """Parse imports and executable code from one lexical masking pass."""

    masked = _mask_rust_non_code(source)
    matches = list(RUST_USE.finditer(masked))
    statements = [
        (
            source.count("\n", 0, match.start()) + 1,
            _normalize_statement(source[match.start() : match.end()]),
        )
        for match in matches
    ]
    code = list(masked)
    for match in matches:
        for index in range(match.start(), match.end()):
            if code[index] not in "\r\n":
                code[index] = " "
    return statements, "".join(code)


def _use_statements(source: str) -> List[Tuple[int, str]]:
    return _rust_views(source)[0]


def _code_without_use_statements(source: str) -> str:
    """Return code with comments, literals, and use declarations removed."""

    return _rust_views(source)[1]


def _surface_reexport(statement: str, surface: str) -> bool:
    if not statement.startswith("pub use "):
        return False
    target = statement[len("pub use ") :].removesuffix(";").strip()
    return (
        target in {surface, "lean_ctx_protocol::" + surface}
        or target.startswith(surface + "::")
        or target.startswith("lean_ctx_protocol::" + surface + "::")
    )


def _top_level_use_items(value: str) -> List[str]:
    """Split a braced use list without descending into nested groups."""

    items = []
    start = 0
    depth = 0
    for index, character in enumerate(value):
        if character in "{[(":
            depth += 1
        elif character in "}])":
            depth = max(0, depth - 1)
        elif character == "," and depth == 0:
            items.append(value[start:index].strip())
            start = index + 1
    items.append(value[start:].strip())
    return [item for item in items if item]


def _surface_import(
    statement: str,
    surface: str,
    root_symbols: Sequence[str] = (),
    same_protocol_crate: bool = False,
) -> bool:
    if "use " not in statement:
        return False
    target = _mask_rust_non_code(statement.split("use ", 1)[1].removesuffix(";").strip())
    alias = r"(?:\s+as\s+[A-Za-z_][A-Za-z0-9_]*)"
    if re.fullmatch(r"(?:::)?lean_ctx_protocol" + alias, target):
        return True
    if same_protocol_crate and re.fullmatch(r"crate" + alias, target):
        return True
    surface_path = re.escape(surface)
    if re.search(
        r"(?:^|[^A-Za-z0-9_])(?:crate|self|super|lean_ctx_protocol)::\s*"
        r"(?:\{\s*)?%s(?:::|(?=[\s,};]|$))" % surface_path,
        target,
    ):
        return True
    if re.match(r"%s(?:::|(?=[\s,};]|$))" % surface_path, target):
        return True
    qualified = re.match(
        r"(?:::)?(?:crate|self|super|lean_ctx_protocol)::(.+)$",
        target,
    )
    if qualified and qualified.group(1).lstrip().startswith("{"):
        remainder = qualified.group(1).strip()
        if remainder.endswith("}"):
            for item in _top_level_use_items(remainder[1:-1]):
                if re.match(r"%s(?:::|(?=[\s,};]|$))" % surface_path, item):
                    return True

    root = re.match(r"(?:::)?(?:crate|lean_ctx_protocol)::(.+)$", target)
    if not root or not root_symbols:
        return False
    remainder = root.group(1).strip()
    if remainder in {"*", "{*}"}:
        return True
    if remainder.startswith("{") and remainder.endswith("}"):
        for item in _top_level_use_items(remainder[1:-1]):
            if any(re.match(r"%s(?:\s+as\s+[A-Za-z_][A-Za-z0-9_]*)?$" % re.escape(symbol), item) for symbol in root_symbols):
                return True
    else:
        first = re.match(r"[A-Za-z_][A-Za-z0-9_]*", remainder)
        if first and first.group(0) in root_symbols:
            return True
    return False


def _root_reexport_symbols(surface: str, spec: dict) -> List[str]:
    """Return symbols reachable from the protocol crate root for a surface."""

    root_path = "rust/crates/lean-ctx-protocol/src/lib.rs"
    symbols = sorted(item["name"] for item in spec["exported_symbols"])
    selected = set()
    for item in spec["root_reexports"]:
        if item["path"] != root_path:
            continue
        for statement in item["statements"]:
            target = statement[len("pub use ") :].removesuffix(";").strip()
            if target in {surface + "::*", "lean_ctx_protocol::" + surface + "::*"}:
                return symbols
            if target.startswith(surface + "::{") or target.startswith("lean_ctx_protocol::" + surface + "::{"):
                selected.update(
                    symbol
                    for symbol in symbols
                    if re.search(r"(?:^|[,{\s])%s(?:\s|[,}])" % re.escape(symbol), target)
                )
    return sorted(selected)


def _surface_references(
    source: str,
    surface: str,
    root_symbols: Sequence[str],
    code: Optional[str] = None,
) -> List[Tuple[int, str]]:
    """Find qualified frozen paths outside import declarations."""

    components = r"[A-Za-z_][A-Za-z0-9_]*"
    module_path = re.escape(surface) + r"(?:::%s)*" % components
    alternatives = [
        r"(?:crate|self|super|lean_ctx_protocol)::%s" % module_path,
        r"extern\s+crate\s+lean_ctx_protocol(?:\s+as\s+[A-Za-z_][A-Za-z0-9_]*)?",
    ]
    if root_symbols:
        alternatives.append(
            r"(?:crate|lean_ctx_protocol)::(?:%s)"
            % "|".join(re.escape(symbol) for symbol in root_symbols)
        )
    pattern = re.compile(
        r"(?<![A-Za-z0-9_:])(?:%s)(?![A-Za-z0-9_])" % "|".join(alternatives)
    )
    if code is None:
        code = _code_without_use_statements(source)
    return [
        (source.count("\n", 0, match.start()) + 1, match.group(0))
        for match in pattern.finditer(code)
    ]


def _shape_mapping(
    root: Path,
    files: Sequence[Path],
    surface: str,
    kind: str,
    use_cache: Optional[dict] = None,
    source_cache: Optional[dict] = None,
) -> dict:
    mapping = {}
    for path in files:
        source = source_cache.get(path) if source_cache is not None else None
        if source is None:
            source = _read_source(path)
        if kind == "module_roots":
            statements = [
                _normalize_statement(match.group(0))
                for match in re.finditer(r"(?m)^[ \t]*pub\s+mod\s+%s\s*;" % re.escape(surface), source)
            ]
        else:
            use_statements = use_cache[path] if use_cache is not None else _use_statements(source)
            statements = [statement for _, statement in use_statements if _surface_reexport(statement, surface)]
        if statements:
            mapping[_relative(root, path)] = sorted(statements)
    return mapping


def _expected_mapping(root: Path, entries: Sequence[dict], field: str, label: str) -> dict:
    mapping = {}
    for entry in entries:
        path = _safe_relative(entry["path"], "%s.path" % label)
        _root_path(root, path, "%s/%s" % (label, path))
        mapping[path] = sorted(_normalize_statement(statement) for statement in entry[field])
    return mapping


def _shape_findings(
    root: Path,
    files: Sequence[Path],
    surface: str,
    spec: dict,
    use_cache: Optional[dict] = None,
    source_cache: Optional[dict] = None,
) -> List[str]:
    findings = []
    for kind, field in (("module_roots", "declarations"), ("root_reexports", "statements")):
        expected = _expected_mapping(root, spec[kind], field, "%s.%s" % (surface, kind))
        actual = _shape_mapping(root, files, surface, kind, use_cache, source_cache)
        if actual != expected:
            findings.append(
                "[shape] %s %s drift: expected %s; found %s"
                % (surface, kind, canonical_json(expected).decode().strip(), canonical_json(actual).decode().strip())
            )
    module_path = _safe_relative(spec["module_path"], "%s.module_path" % surface)
    module = _root_path(root, module_path, "%s module" % surface)
    module_source = source_cache.get(module) if source_cache is not None else None
    if module_source is None:
        module_source = _read_source(module)
    actual_digest = hashlib.sha256(module_source.encode("utf-8")).hexdigest()
    if actual_digest != spec["module_sha256"]:
        findings.append(
            "[shape] %s module digest drift: expected %s; found %s"
            % (surface, spec["module_sha256"], actual_digest)
        )
    actual_symbols = []
    for line in module_source.splitlines():
        match = RUST_PUBLIC_ITEM.match(line)
        if match:
            actual_symbols.append({"kind": match.group(1), "name": match.group(2)})
    actual_symbols.sort(key=lambda item: (item["kind"], item["name"]))
    if actual_symbols != spec["exported_symbols"]:
        findings.append(
            "[shape] %s exported symbols drift: expected %s; found %s"
            % (
                surface,
                canonical_json(spec["exported_symbols"]).decode().strip(),
                canonical_json(actual_symbols).decode().strip(),
            )
        )
    return findings


def _consumer_findings(
    root: Path,
    files: Sequence[Path],
    surface: str,
    spec: dict,
    source_cache: Optional[dict] = None,
    use_cache: Optional[dict] = None,
    code_cache: Optional[dict] = None,
) -> List[str]:
    approved = {}
    for item in spec["approved_consumers"]:
        path = _safe_relative(item["path"], "%s.approved_consumer.path" % surface)
        _root_path(root, path, "%s approved consumer" % surface)
        approved[path] = {_normalize_statement(statement) for statement in item["statements"]}
    module_path = _safe_relative(spec["module_path"], "%s.module_path" % surface)
    root_symbols = _root_reexport_symbols(surface, spec)
    actual = {}
    findings = []
    for path in files:
        relative = _relative(root, path)
        if not (relative.startswith("rust/src/") or relative.startswith("rust/crates/lean-ctx-")):
            continue
        if relative == module_path:
            continue
        source = source_cache[path] if source_cache is not None else _read_source(path)
        statements = use_cache[path] if use_cache is not None else _use_statements(source)
        for line, statement in statements:
            if _surface_reexport(statement, surface):
                if statement in approved.get(relative, set()):
                    actual.setdefault(relative, []).append((line, statement))
                continue
            if _surface_import(
                statement,
                surface,
                root_symbols,
                relative.startswith("rust/crates/lean-ctx-protocol/"),
            ):
                actual.setdefault(relative, []).append((line, statement))
        code = code_cache[path] if code_cache is not None else None
        for line, reference in _surface_references(source, surface, root_symbols, code):
            findings.append(
                "[new-consumer] %s %s:%d references frozen surface: %s"
                % (surface, relative, line, reference)
            )
    for path in sorted(actual):
        allowed = approved.get(path, set())
        for line, statement in sorted(actual[path], key=lambda item: (item[0], item[1])):
            if statement not in allowed:
                findings.append(
                    "[new-consumer] %s %s:%d imports frozen surface: %s"
                    % (surface, path, line, statement)
                )
    for path in sorted(approved):
        current = {statement for _, statement in actual.get(path, [])}
        for statement in sorted(approved[path] - current):
            findings.append(
                "[consumer] %s approved consumer missing: %s: %s" % (surface, path, statement)
            )
    return findings


def _private_import_findings(
    root: Path,
    files: Sequence[Path],
    use_cache: Optional[dict] = None,
) -> List[str]:
    findings = []
    for path in files:
        statements = use_cache[path] if use_cache is not None else _use_statements(_read_source(path))
        for line, statement in statements:
            target = statement.split("use ", 1)[1].removesuffix(";").strip()
            if PRIVATE_IMPORT.search(target):
                findings.append(
                    "[private-import] %s:%d imports private namespace: %s"
                    % (_relative(root, path), line, target)
                )
    return findings


def _needs_consumer_scan(source: str, manifest: dict) -> bool:
    """Skip lexical masking when source cannot mention a frozen/private path."""

    symbols = {
        item["name"]
        for surface in manifest["surfaces"].values()
        for item in surface["exported_symbols"]
    }
    if any(symbol in source for symbol in symbols):
        return True
    lowered = source.lower()
    needles = FROZEN_SURFACES + (
        "use crate",
        "use lean_ctx_protocol",
        "use ::lean_ctx_protocol",
        "enterprise",
        "private",
        "proprietary",
        "commercial",
        "strategic_data",
    )
    return any(needle in lowered for needle in needles)


def _manifest_path(root: Path, manifest_path: Optional[Path]) -> Path:
    candidate = (root / "security/public-protocol-surface-freeze-v1.json") if manifest_path is None else Path(manifest_path)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        candidate.resolve().relative_to(root.resolve())
        return candidate
    except ValueError as error:
        raise ManifestError("manifest path escapes repository root") from error


def check_repo(root: Path = ROOT, manifest_path: Optional[Path] = None) -> List[str]:
    """Return stable findings; any unreadable or incomplete input fails closed."""

    root = Path(root).resolve()
    if not root.is_dir():
        return ["[root] repository root is unavailable"]
    try:
        policy = _manifest_path(root, manifest_path)
        manifest = load_manifest(policy)
        files = [
            path
            for path in _rust_files(root)
            if _relative(root, path).startswith("rust/src/")
            or _relative(root, path).startswith("rust/crates/lean-ctx-")
        ]
        if not files:
            raise ManifestError("no Rust source files found")
        source_cache = {}
        use_cache = {}
        code_cache = {}
        for path in files:
            relative = _relative(root, path)
            source = _read_source(path)
            source_cache[path] = source
            if _needs_consumer_scan(source, manifest):
                statements, code = _rust_views(source)
            else:
                statements, code = [], ""
            use_cache[path] = statements
            if relative.startswith("rust/src/") or relative.startswith("rust/crates/lean-ctx-"):
                code_cache[path] = code
        findings = []
        for surface in FROZEN_SURFACES:
            spec = manifest["surfaces"][surface]
            findings.extend(
                _shape_findings(root, files, surface, spec, use_cache, source_cache)
            )
            findings.extend(
                _consumer_findings(root, files, surface, spec, source_cache, use_cache, code_cache)
            )
        findings.extend(_private_import_findings(root, files, use_cache))
        if root == ROOT:
            findings.extend(check_classifications(rust_modules()))
            findings.extend(check_strategic_data())
        return sorted(set(findings))
    except ManifestError as error:
        return ["[manifest] %s" % error]


check = check_repo
check_open_core_boundary = check_repo


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--json", action="store_true", dest="as_json")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    findings = check_repo(root, args.manifest)
    if args.as_json:
        report = {
            "manifest": "security/public-protocol-surface-freeze-v1.json",
            "status": "fail" if findings else "pass",
            "surfaces": list(FROZEN_SURFACES),
            "violations": findings,
        }
        sys.stdout.buffer.write(canonical_json(report))
        return 1 if findings else 0
    if findings:
        print("Open-core boundary: FAIL")
        for finding in findings:
            print("- %s" % finding)
        return 1
    print("Open-core boundary: PASS (protocol freeze: %d surfaces)" % len(FROZEN_SURFACES))
    return 0


if __name__ == "__main__":
    sys.exit(main())
