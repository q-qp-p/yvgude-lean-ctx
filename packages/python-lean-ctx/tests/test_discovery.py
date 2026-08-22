"""Endpoint/token discovery — isolated from the developer's real environment."""

import os

import pytest

from lean_ctx import discovery

_ENV_KEYS = (
    "LEAN_CTX_PROXY_URL",
    "LEAN_CTX_PROXY_PORT",
    "LEAN_CTX_PROXY_TOKEN",
    "LEAN_CTX_DATA_DIR",
    "LEAN_CTX_CONFIG_DIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
)


@pytest.fixture
def isolated(tmp_path, monkeypatch):
    """Clear every discovery env var and root HOME at an empty tmp dir."""
    for key in _ENV_KEYS:
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setenv("HOME", str(tmp_path))
    return tmp_path


def test_base_url_explicit_strips_trailing_slash():
    assert discovery.resolve_base_url("http://host:9/") == "http://host:9"


def test_base_url_from_env(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_PROXY_URL", "http://h:1234/")
    assert discovery.resolve_base_url() == "http://h:1234"


def test_base_url_defaults_to_loopback(isolated, monkeypatch):
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    assert discovery.resolve_base_url() == "http://127.0.0.1:4444"


def test_port_env_wins(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_PROXY_PORT", "5005")
    assert discovery.resolve_port() == 5005


@pytest.mark.parametrize("value", ("-1", "65536", "70000", "4500 ", " not-a-port"))
def test_invalid_env_port_falls_back_to_rust_config(isolated, monkeypatch, value):
    monkeypatch.setenv("LEAN_CTX_PROXY_PORT", value)
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(isolated / "config"))
    config_dir = isolated / "config"
    config_dir.mkdir()
    (config_dir / "config.toml").write_text("proxy_port = 4555\n", encoding="utf-8")
    assert discovery.resolve_port() == 4555


@pytest.mark.parametrize("value", ("0", "65535", "+4500"))
def test_u16_boundary_env_ports_are_accepted(isolated, monkeypatch, value):
    monkeypatch.setenv("LEAN_CTX_PROXY_PORT", value)
    assert discovery.resolve_port() == int(value)


def test_split_xdg_config_wins_over_xdg_data(isolated, monkeypatch):
    data_base = isolated / "xdg-data"
    config_base = isolated / "xdg-config"
    data_dir = data_base / "lean-ctx"
    config_dir = config_base / "lean-ctx"
    data_dir.mkdir(parents=True)
    config_dir.mkdir(parents=True)
    (data_dir / "config.toml").write_text("proxy_port = 4600\n", encoding="utf-8")
    (config_dir / "config.toml").write_text("proxy_port = 4601\n", encoding="utf-8")
    monkeypatch.setenv("XDG_DATA_HOME", str(data_base))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config_base))
    assert discovery.resolve_port() == 4601


def test_explicit_config_dir_wins_over_data_and_xdg_config(isolated, monkeypatch):
    data_dir = isolated / "data"
    config_dir = isolated / "config"
    xdg_config = isolated / "xdg-config"
    for directory, port in ((data_dir, 4600), (config_dir, 4601), (xdg_config, 4602)):
        directory.mkdir(parents=True)
        (directory / "config.toml").write_text(f"proxy_port = {port}\n", encoding="utf-8")
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(data_dir))
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(config_dir))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(xdg_config.parent))
    assert discovery.resolve_port() == 4601


def test_standard_xdg_data_pin_does_not_collapse_config(isolated, monkeypatch):
    data_base = isolated / "xdg-data"
    config_base = isolated / "xdg-config"
    data_dir = data_base / "lean-ctx"
    config_dir = config_base / "lean-ctx"
    data_dir.mkdir(parents=True)
    config_dir.mkdir(parents=True)
    (data_dir / "config.toml").write_text("proxy_port = 4600\n", encoding="utf-8")
    (config_dir / "config.toml").write_text("proxy_port = 4601\n", encoding="utf-8")
    monkeypatch.setenv("XDG_DATA_HOME", str(data_base))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config_base))
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(data_dir))
    assert discovery.resolve_port() == 4601


def test_legacy_data_markers_precede_split_xdg_config(isolated, monkeypatch):
    legacy = isolated / ".lean-ctx"
    xdg_config = isolated / "xdg-config"
    legacy.mkdir()
    (legacy / "stats.json").write_text("{}", encoding="utf-8")
    (legacy / "config.toml").write_text("proxy_port = 4700\n", encoding="utf-8")
    config_dir = xdg_config / "lean-ctx"
    config_dir.mkdir(parents=True)
    (config_dir / "config.toml").write_text("proxy_port = 4701\n", encoding="utf-8")
    monkeypatch.setenv("XDG_CONFIG_HOME", str(xdg_config))
    assert discovery.resolve_port() == 4700


def test_xdg_data_markers_do_not_recollapse_config(isolated, monkeypatch):
    data_base = isolated / "xdg-data"
    config_base = isolated / "xdg-config"
    data_dir = data_base / "lean-ctx"
    config_dir = config_base / "lean-ctx"
    data_dir.mkdir(parents=True)
    config_dir.mkdir(parents=True)
    (data_dir / "stats.json").write_text("{}", encoding="utf-8")
    (data_dir / "config.toml").write_text("proxy_port = 4800\n", encoding="utf-8")
    (config_dir / "config.toml").write_text("proxy_port = 4801\n", encoding="utf-8")
    monkeypatch.setenv("XDG_DATA_HOME", str(data_base))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config_base))
    assert discovery.resolve_port() == 4801


def test_xdg_layout_pin_ignores_legacy_marker(isolated, monkeypatch):
    legacy = isolated / ".lean-ctx"
    config_base = isolated / "xdg-config"
    legacy.mkdir()
    (legacy / "stats.json").write_text("{}", encoding="utf-8")
    config_dir = config_base / "lean-ctx"
    config_dir.mkdir(parents=True)
    (config_dir / "layout.toml").write_text('mode = "xdg"\n', encoding="utf-8")
    (config_dir / "config.toml").write_text("proxy_port = 4901\n", encoding="utf-8")
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config_base))
    assert discovery.resolve_port() == 4901


def test_single_quoted_xdg_layout_pin_does_not_ignore_legacy_marker(isolated, monkeypatch):
    legacy = isolated / ".lean-ctx"
    config_base = isolated / "xdg-config"
    legacy.mkdir()
    (legacy / "stats.json").write_text("{}", encoding="utf-8")
    (legacy / "config.toml").write_text("proxy_port = 4900\n", encoding="utf-8")
    config_dir = config_base / "lean-ctx"
    config_dir.mkdir(parents=True)
    (config_dir / "layout.toml").write_text("mode = 'xdg'\n", encoding="utf-8")
    (config_dir / "config.toml").write_text("proxy_port = 4901\n", encoding="utf-8")
    monkeypatch.setenv("XDG_CONFIG_HOME", str(config_base))
    assert discovery.resolve_port() == 4900


def test_malformed_config_port_falls_back_to_uid(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(isolated))
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    (isolated / "config.toml").write_text("proxy_port = 70000\n", encoding="utf-8")
    assert discovery.resolve_port() == 4444


def test_uid_port_matches_rust_formula(isolated, monkeypatch):
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    assert discovery._uid_port() == 4444
    monkeypatch.setattr(os, "getuid", lambda: 2999, raising=False)
    assert discovery._uid_port() == 5443
    monkeypatch.setattr(os, "getuid", lambda: 500, raising=False)
    assert discovery._uid_port() == 4444


def test_port_from_config_toml(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(isolated))
    (isolated / "config.toml").write_text("proxy_port = 4500\n", encoding="utf-8")
    assert discovery.resolve_port() == 4500


def test_toml_hex_proxy_port_matches_rust_u16(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(isolated))
    (isolated / "config.toml").write_text("proxy_port = 0x1234\n", encoding="utf-8")
    assert discovery.resolve_port() == 0x1234


def test_toml_leading_zero_proxy_port_is_rejected(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(isolated))
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    (isolated / "config.toml").write_text("proxy_port = 04500\n", encoding="utf-8")
    assert discovery.resolve_port() == 4444


def test_unrelated_malformed_toml_falls_back_to_uid(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_CONFIG_DIR", str(isolated))
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    (isolated / "config.toml").write_text(
        "proxy_port = 4555\nunrelated = [\n", encoding="utf-8"
    )
    assert discovery.resolve_port() == 4444


def test_commented_proxy_port_is_ignored(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(isolated))
    (isolated / "config.toml").write_text("# proxy_port = 3128\n", encoding="utf-8")
    monkeypatch.setattr(os, "getuid", lambda: 1000, raising=False)
    assert discovery.resolve_port() == 4444


def test_token_env_precedence(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_PROXY_TOKEN", "envtok")
    assert discovery.resolve_token() == "envtok"


def test_token_from_session_file(isolated, monkeypatch):
    monkeypatch.setenv("LEAN_CTX_DATA_DIR", str(isolated))
    (isolated / "session_token").write_text("deadbeef\n", encoding="utf-8")
    assert discovery.resolve_token() == "deadbeef"


def test_token_absent_returns_none(isolated):
    assert discovery.resolve_token() is None
