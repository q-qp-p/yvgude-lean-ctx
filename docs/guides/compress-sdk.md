# Python SDK v1 `compress()` compatibility API — Preview

`compress()` is a compatibility helper in the canonical Python SDK v1. The
supported product path remains the declared `LeanCTX` / session / `wrap()`
reference-wrapper flow described in
[`packages/python-lean-ctx/README.md`](../../packages/python-lean-ctx/README.md).

Install the sole current SDK distribution:

```bash
pip install lean-ctx-python
```

```python
from lean_ctx import compress

messages = [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": large_log_or_file_dump},
]
result = compress(messages, model="claude-sonnet-4")
```

The helper targets a configured local Runtime and preserves unsupported or
unobserved details rather than fabricating evidence. Token fields are diagnostic
signals, not a savings or accepted-outcome claim. Any performance claim requires
a comparable baseline/treatment, declared quality threshold, and reproducible
Receipt with offline verification.

TypeScript and Go compression SDK prototypes are archived and are not current
installation, compatibility, or support surfaces.
