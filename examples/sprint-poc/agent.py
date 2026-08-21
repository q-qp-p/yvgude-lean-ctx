"""Reference code-review agent for the Agent Tuning Sprint harness.

Stock path has no LeanCTX import. Treatment uses LeanCTX.wrap() and the
ContextAware `leanctx=` keyword so proxy compression is opt-in, not global.
"""

from __future__ import annotations

import json
import os
import sys
import re
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
FIXTURE = ROOT / "fixture" / "checkout.py"
MANIFEST = json.loads((ROOT / "workload-manifest.json").read_text(encoding="utf-8"))

SYSTEM = """You review source for correctness and security.
Return ONLY JSON with this shape:
{"findings":[{"id":"sql_injection|missing_authz|discount_off_by_one|other","severity":"high|medium|low","location":"checkout.py:<function>","summary":"<one sentence>"}]}
Use the canonical ids when the defect matches. Do not invent files that are not in the source.
"""


class ReferenceCodeReviewAgent:
    name = "ReferenceCodeReviewAgent"
    version = MANIFEST["agent_version"]

    def describe(self) -> dict[str, str]:
        return {
            "agent": f"{self.name} v{self.version}",
            "framework": "ContextAwareAgent (OpenAI Chat Completions)",
            "model": os.environ.get("OPENAI_MODEL", MANIFEST["model_default"]),
            "output_schema": "ReviewResult v1",
            "fixture": str(FIXTURE.relative_to(ROOT)),
            "leanctx_attached": "via wrap() only",
        }

    def run(self, task: str, *, leanctx: Any = None) -> dict[str, Any]:
        source = FIXTURE.read_text(encoding="utf-8")
        user = f"{task}\n\nSOURCE fixture/checkout.py:\n{source}"
        messages = [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ]
        if leanctx is not None:
            compressed = leanctx.compress(messages, model=self._model())
            messages = list(compressed.messages)
        content = self._complete(messages)
        return _parse_review(content)

    def _model(self) -> str:
        return os.environ.get("OPENAI_MODEL", MANIFEST["model_default"])

    def _complete(self, messages: list[dict[str, str]]) -> str:
        api_key = os.environ.get("OPENAI_API_KEY", "").strip()
        if not api_key:
            raise RuntimeError(
                "OPENAI_API_KEY is required for sprint-poc run. "
                "doctor() and quality tests do not call the model."
            )
        try:
            from openai import OpenAI
        except ImportError as exc:
            raise RuntimeError(
                "Install openai to run the live agent: pip install openai lean-ctx-python"
            ) from exc
        client = OpenAI(api_key=api_key)
        response = client.chat.completions.create(
            model=self._model(),
            temperature=0,
            response_format={"type": "json_object"},
            messages=messages,
        )
        choice = response.choices[0].message.content
        if not choice:
            raise RuntimeError("model returned empty content")
        return choice


def _parse_review(content: str) -> dict[str, Any]:
    match = re.search(r"\{.*\}", content, re.DOTALL)
    raw = match.group(0) if match else content
    parsed = json.loads(raw)
    if not isinstance(parsed, dict) or "findings" not in parsed:
        raise ValueError("model output is not ReviewResult v1 JSON")
    return parsed


def main() -> None:
    agent = ReferenceCodeReviewAgent()
    if sys.argv[1:] == ["describe"]:
        print(json.dumps(agent.describe(), indent=2))
        return
    print(json.dumps(agent.describe(), indent=2))


if __name__ == "__main__":
    main()
