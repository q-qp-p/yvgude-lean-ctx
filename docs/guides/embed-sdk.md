# Preview — embed LeanCTX in a custom host

> **Preview integration surface.** The current Engine is implementation
> substrate; it is not yet the stable public Embed facade or a general
> agent-building framework.

## What Embed means

Embed places LeanCTX inside a custom agent or application so the host can use
the context lifecycle before inference:

    select → shape → reuse → recover → measure

The host remains responsible for its agent loop, task logic, model selection,
tools, retries, data governance, and user-visible behavior. LeanCTX does not
take ownership of the application or silently send traffic or retry its calls.

## Current status

- The Rust Engine and related code are useful implementation substrate for
  narrow evaluation work.
- Python SDK v1 and its declared OpenAI Agents reference-wrapper scope are
  **Preview**.
- A general adapter framework, common session facade, typed lifecycle contract,
  and common Receipt flow are still converging.

Do not infer a generally supported SDK contract from an internal crate,
example, method, or package name. Any Embed integration must declare its host
version, visibility limits, recovery behavior, and evidence scope.

## Evaluation discipline

Evaluate a custom integration against a named workload with a quality gate and
a comparable baseline/treatment. Do not claim a general result from a local
experiment or a context counter.
