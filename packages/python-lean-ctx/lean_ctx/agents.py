"""OpenAI Agents SDK detection with no import-time optional dependency."""

from __future__ import annotations

from .errors import LeanCtxError


def looks_like_agents_sdk_agent(agent: object) -> bool:
    """Recognize the public SDK Agent from its stable implementation module."""
    agent_type = type(agent)
    return agent_type.__name__ == "Agent" and agent_type.__module__.startswith("agents")


def is_agents_sdk_agent(agent: object) -> bool:
    """Return whether an object is an installed OpenAI Agents SDK Agent."""
    if not looks_like_agents_sdk_agent(agent):
        return False
    try:
        from agents import Agent
    except ImportError as exc:
        raise LeanCtxError(
            "OpenAI Agents SDK support requires pip install 'lean-ctx-python[openai-agents]'"
        ) from exc
    return isinstance(agent, Agent)


def make_agents_sdk_adapter(agent: object):
    """Import the Model bridge only after the optional SDK is confirmed."""
    from .agents_model import AgentsSdkAdapter

    return AgentsSdkAdapter(agent)
