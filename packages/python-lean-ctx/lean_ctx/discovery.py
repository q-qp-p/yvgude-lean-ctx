"""Local discovery of the lean-ctx proxy endpoint.

Mirrors the daemon's own resolution so ``compress()`` works out of the box once
``lean-ctx proxy enable`` has run, while every step stays overridable via the
same environment variables the CLI honours:

* URL    — ``LEAN_CTX_PROXY_URL`` else ``http://127.0.0.1:<port>``
* Port   — ``LEAN_CTX_PROXY_PORT`` → ``config.toml`` ``proxy_port`` → UID-derived
* Token  — ``LEAN_CTX_PROXY_TOKEN`` → ``<data_dir>/session_token``
* Dirs   — ``LEAN_CTX_DATA_DIR`` / XDG, matching the Rust ``data_dir`` rules
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import List, Optional

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9/3.10
    import tomli as tomllib

# Base port the daemon derives per-UID (see proxy_setup::uid_based_port).
_DEFAULT_PORT = 4444
_MAX_PORT = 65535
_DATA_MARKERS = ("stats.json", "sessions", "vectors", "graphs", "knowledge")


def _env_path(name: str) -> Optional[Path]:
    value = os.environ.get(name, "").strip()
    return Path(value) if value else None


def _xdg_base(name: str, fallback: str) -> Path:
    value = os.environ.get(name, "").strip()
    return Path(value) if value else Path.home() / fallback


def _parse_u16(value: str, *, allow_underscores: bool = False) -> Optional[int]:
    """Parse the same bounded integer domain as Rust's ``u16`` parser."""
    if not isinstance(value, str) or not value:
        return None
    digits = value[1:] if value.startswith("+") else value
    if not digits:
        return None
    if allow_underscores:
        if digits.startswith("_") or digits.endswith("_") or "__" in digits:
            return None
        if any(char != "_" and not ("0" <= char <= "9") for char in digits):
            return None
        if any(
            digits[index] == "_"
            and (
                index == 0
                or index + 1 == len(digits)
                or not ("0" <= digits[index - 1] <= "9")
                or not ("0" <= digits[index + 1] <= "9")
            )
            for index in range(len(digits))
        ):
            return None
        digits = digits.replace("_", "")
    elif any(not ("0" <= char <= "9") for char in digits):
        return None
    try:
        value_int = int(digits, 10)
    except ValueError:
        return None
    return value_int if value_int <= _MAX_PORT else None


def _marker_has_data(path: Path) -> bool:
    try:
        if path.is_dir():
            return next(path.iterdir(), None) is not None
        return path.stat().st_size > 0
    except OSError:
        return False


def _has_data_files(directory: Path) -> bool:
    return any(_marker_has_data(directory / marker) for marker in _DATA_MARKERS)


def _rust_layout_mode(line: str) -> Optional[str]:
    """Parse one layout mode line with Rust's string handling."""
    rest = line.strip()
    if not rest.startswith("mode"):
        return None
    rest = rest[len("mode") :].lstrip()
    if not rest.startswith("="):
        return None
    value = rest[1:].strip().strip('"').strip()
    return value or None


def _is_xdg_pinned(config_base: Path) -> bool:
    """Match Rust's layout.toml XDG commitment pin."""
    try:
        text = (config_base / "lean-ctx" / "layout.toml").read_text(encoding="utf-8")
    except OSError:
        return False
    return any(_rust_layout_mode(line) == "xdg" for line in text.splitlines())


def _single_dir_override() -> Optional[Path]:
    """Mirror Rust's legacy/mixed-install collapse for config resolution."""
    data_override = _env_path("LEAN_CTX_DATA_DIR")
    standard_data = _xdg_base("XDG_DATA_HOME", ".local/share") / "lean-ctx"
    if data_override is not None and data_override != standard_data:
        return data_override

    home = Path.home()
    config_base = _xdg_base("XDG_CONFIG_HOME", ".config")
    if _is_xdg_pinned(config_base):
        return None

    legacy = home / ".lean-ctx"
    if _has_data_files(legacy):
        return legacy

    mixed = config_base / "lean-ctx"
    if _has_data_files(mixed):
        data_dir = standard_data
        if _has_data_files(data_dir):
            return None
        return mixed
    return None


def _config_dir() -> Path:
    """Resolve the global config directory like Rust's ``paths::config_dir``."""
    explicit = _env_path("LEAN_CTX_CONFIG_DIR")
    if explicit is not None:
        return explicit
    single = _single_dir_override()
    if single is not None:
        return single
    return _xdg_base("XDG_CONFIG_HOME", ".config") / "lean-ctx"


def _candidate_dirs() -> List[Path]:
    """Ordered runtime-data directories used for session-token discovery."""
    dirs: List[Path] = []
    env = _env_path("LEAN_CTX_DATA_DIR")
    if env is not None:
        dirs.append(env)

    home = Path.home()
    dirs.append(home / ".lean-ctx")

    xdg_data = os.environ.get("XDG_DATA_HOME", "").strip()
    dirs.append(Path(xdg_data) / "lean-ctx" if xdg_data else home / ".local" / "share" / "lean-ctx")

    xdg_config = os.environ.get("XDG_CONFIG_HOME", "").strip()
    dirs.append(Path(xdg_config) / "lean-ctx" if xdg_config else home / ".config" / "lean-ctx")

    # De-duplicate while preserving order.
    seen = set()
    unique: List[Path] = []
    for d in dirs:
        if d not in seen:
            seen.add(d)
            unique.append(d)
    return unique


def _uid_port() -> int:
    """Replicate proxy_setup::uid_based_port (UID 1000 → 4444, +offset, base for <1000)."""
    getuid = getattr(os, "getuid", None)
    if getuid is None:  # Windows
        return _DEFAULT_PORT
    uid = int(getuid()) & _MAX_PORT
    offset = (uid - 1000) % 1000 if uid >= 1000 else 0
    return _DEFAULT_PORT + offset


def _config_port() -> Optional[int]:
    """Read Rust's top-level ``proxy_port`` from the resolved config file."""
    try:
        text = (_config_dir() / "config.toml").read_text(encoding="utf-8")
        config = tomllib.loads(text)
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        return None

    value = config.get("proxy_port")
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    return value if 0 <= value <= _MAX_PORT else None


def resolve_port() -> int:
    env = os.environ.get("LEAN_CTX_PROXY_PORT")
    if env is not None:
        parsed = _parse_u16(env)
        if parsed is not None:
            return parsed
    cfg = _config_port()
    if cfg is not None:
        return cfg
    return _uid_port()


def resolve_base_url(base_url: Optional[str] = None) -> str:
    if base_url:
        return base_url.rstrip("/")
    env = os.environ.get("LEAN_CTX_PROXY_URL", "").strip()
    if env:
        return env.rstrip("/")
    return f"http://127.0.0.1:{resolve_port()}"


def resolve_token(token: Optional[str] = None) -> Optional[str]:
    if token:
        return token
    env = os.environ.get("LEAN_CTX_PROXY_TOKEN", "").strip()
    if env:
        return env
    for directory in _candidate_dirs():
        try:
            value = (directory / "session_token").read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if value:
            return value
    return None
