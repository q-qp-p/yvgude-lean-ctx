# Sprint POC — 15-minute evidence harness

Pinned code-review fixture + `LeanCTX.wrap()` for the paid Agent Tuning Sprint.

**Integrity:** no synthetic cost. A cheaper treatment is a win only if **both**
arms pass `expected-findings-v1`.

## Setup

```bash
cd examples/sprint-poc
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
export OPENAI_API_KEY=...          # required for `run`, not for tests
# lean-ctx proxy should already be running
```

```bash
python poc.py doctor
python poc.py run --arm stock --out ../../.sprint-poc
python poc.py run --arm leanctx --out ../../.sprint-poc
python poc.py compare --out ../../.sprint-poc
python poc.py verify --out ../../.sprint-poc
```

Quality tests (no API key):

```bash
python -m pytest test_quality.py
```

## What the buyer sees

Same agent, same `fixture/checkout.py`, two arms: stock vs wrap().
Output is ReviewResult JSON, a quality gate, and a treatment receipt when
the local proxy seals one.
