"""LeanCTX facade configuration and lifecycle factories."""

from __future__ import annotations

import hashlib
import math
import os
from collections import OrderedDict
from collections.abc import Mapping
from contextvars import ContextVar
from dataclasses import dataclass
from typing import TYPE_CHECKING, Optional

from .kit import ContextKit, load_kit
from .proxy import ProxyClient

if TYPE_CHECKING:  # pragma: no cover
    from .session import ContextSession
    from .wrap import WrappedAgent

_CONFIG_KEYS = {
    "project",
    "agent_id",
    "proxy_url",
    "proxy_token",
    "timeout",
    "default_profile",
    "fail_open",
    "integration_depth",
    "engine_binary",
    "engine_timeout",
}
_DEPTHS = {"attach", "wrap", "embed"}
_KIT_CACHE_LIMIT = 128


@dataclass(frozen=True)
class LeanCTXConfig:
    project: Optional[str] = None
    agent_id: Optional[str] = None
    proxy_url: Optional[str] = None
    proxy_token: Optional[str] = None
    timeout: float = 30.0
    default_profile: str = "balanced"
    fail_open: bool = True
    integration_depth: str = "wrap"
    engine_binary: str = "lean-ctx"
    engine_timeout: float = 30.0

    def __post_init__(self) -> None:
        if self.project is not None and not isinstance(self.project, str):
            raise ValueError("project must be a string or None")
        if self.proxy_url is not None and not isinstance(self.proxy_url, str):
            raise ValueError("proxy_url must be a string or None")
        if self.proxy_token is not None and not isinstance(self.proxy_token, str):
            raise ValueError("proxy_token must be a string or None")
        if self.agent_id is not None:
            if (
                not isinstance(self.agent_id, str)
                or not self.agent_id
                or len(self.agent_id) > 256
                or any(not (33 <= ord(char) <= 126) for char in self.agent_id)
            ):
                raise ValueError("agent_id must be a bounded opaque ASCII identifier")
        if isinstance(self.timeout, bool) or not isinstance(self.timeout, (int, float)):
            raise ValueError("timeout must be finite and greater than zero")
        if not math.isfinite(float(self.timeout)) or float(self.timeout) <= 0:
            raise ValueError("timeout must be finite and greater than zero")
        if not isinstance(self.default_profile, str) or not self.default_profile.strip():
            raise ValueError("default_profile must be a non-empty string")
        if not isinstance(self.fail_open, bool):
            raise ValueError("fail_open must be a boolean")
        if self.integration_depth not in _DEPTHS:
            raise ValueError("integration_depth must be attach, wrap, or embed")
        if not isinstance(self.engine_binary, str) or not self.engine_binary.strip():
            raise ValueError("engine_binary must be a non-empty string")
        if isinstance(self.engine_timeout, bool) or not isinstance(self.engine_timeout, (int, float)):
            raise ValueError("engine_timeout must be finite and greater than zero")
        if not math.isfinite(float(self.engine_timeout)) or float(self.engine_timeout) <= 0:
            raise ValueError("engine_timeout must be finite and greater than zero")


def _normalize_config(config: object) -> LeanCTXConfig:
    if config is None:
        return LeanCTXConfig(engine_binary=os.environ.get("LEAN_CTX_ENGINE_BINARY", "lean-ctx"))
    if isinstance(config, LeanCTXConfig):
        return config
    if isinstance(config, Mapping):
        keys = set(config.keys())
        unknown = keys - _CONFIG_KEYS
        if unknown:
            raise ValueError("unknown LeanCTX config key: {}".format(sorted(unknown)[0]))
        return LeanCTXConfig(**dict(config))
    raise ValueError("config must be None, LeanCTXConfig, or a mapping")


class LeanCTX:
    """Lightweight facade that owns reusable proxy and Kit cache state."""

    def __init__(self, config=None) -> None:
        self.config = _normalize_config(config)
        self._proxy = ProxyClient(
            # ``ProxyClient`` applies the shared Runtime discovery contract
            # when no explicit SDK endpoint is configured.
            base_url=self.config.proxy_url,
            token=self.config.proxy_token,
            timeout=float(self.config.timeout),
        )
        self._current_session: ContextVar[Optional["ContextSession"]] = ContextVar(
            "lean_ctx_current_session", default=None
        )
        self._kit_cache: "OrderedDict[tuple[str, str, str], ContextKit]" = OrderedDict()

    def wrap(self, agent, kit=None, profile=None) -> "WrappedAgent":
        from .wrap import WrappedAgent

        if self.config.integration_depth == "embed":
            raise ValueError("integration_depth='embed' is not supported by Python SDK v1")
        return WrappedAgent(self, agent, kit=kit, profile=profile)

    def session(
        self,
        task: Optional[str] = None,
        *,
        integration_depth: Optional[str] = None,
        project_root: Optional[str] = None,
        fail_open: Optional[bool] = None,
    ) -> "ContextSession":
        from .session import ContextSession

        return ContextSession(
            self,
            task=task,
            integration_depth=integration_depth,
            project_root=project_root,
            fail_open=fail_open,
        )

    def embed(
        self,
        task: str,
        *,
        project_root: Optional[str] = None,
        fail_open: Optional[bool] = None,
    ) -> "ContextSession":
        """Create an explicit host-controlled Preview Embed session."""
        return self.session(
            task,
            integration_depth="embed",
            project_root=project_root,
            fail_open=fail_open,
        )

    def load_kit(self, name) -> ContextKit:
        if isinstance(name, ContextKit):
            return name
        kit = load_kit(
            name,
            proxy=self._proxy,
            cache=self._kit_cache,
            timeout=float(self.config.timeout),
        )
        self._kit_cache.move_to_end((kit.id, kit.version, kit.package_hash))
        while len(self._kit_cache) > _KIT_CACHE_LIMIT:
            self._kit_cache.popitem(last=False)
        return kit

    def _agent_id_for(self, agent: object) -> str:
        if self.config.agent_id is not None:
            return self.config.agent_id
        agent_type = type(agent)
        identity = "{}.{}".format(agent_type.__module__, agent_type.__qualname__)
        # A stable digest prevents class/module naming from exposing a local path
        # or caller-controlled task content in trusted lineage headers.
        return "python-agent-" + hashlib.sha256(identity.encode("utf-8")).hexdigest()[:32]
