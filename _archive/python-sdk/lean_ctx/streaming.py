"""Server-sent events helpers for the OCLA Wire API."""
from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx


async def stream_events(response: httpx.Response) -> AsyncIterator[dict[str, Any]]:
    """Parse SSE format from an httpx streaming response."""
    event_type = ""
    data_lines: list[str] = []
    async for line in response.aiter_lines():
        if line.startswith("event:"):
            event_type = line[6:].strip()
        elif line.startswith("data:"):
            data_lines.append(line[5:].strip())
        elif line == "":
            if data_lines:
                yield {"type": event_type, "data": json.loads("\n".join(data_lines))}
            event_type = ""
            data_lines = []
