# P3 Engine transport decision

Status: accepted for the P3 real-preview slice
Baseline: `github/main@8ebf61a21c063a1d0a86be33511588d27d7ca71e`

## Decision

P3 uses one versioned, local process transport for a single bounded context-view
operation and exact recovery. The Python Preview starts the installed
`lean-ctx` executable directly, sends a strict JSON request, and receives a
strict JSON result containing the public Engine invocation and observation.
Recovery is a separate digest-checked Engine operation.

The transport is an Engine-operation boundary, not a Product-session service.
It exposes no `/v1/sessions`, completion, abort, agent-loop, planning, tenant,
Cloud, Profile, or Kit lifecycle.

## Ownership

Python Preview owns:

- Product task/session identity and state;
- `ContextSource`, explicit `ContextPlan`, and `ContextView` relationships;
- host/OpenAI Agents SDK execution;
- complete/abort and explicit host outcome;
- `ContextReceipt` projection and degradation policy.

Apache Engine owns:

- rooted source admission and PathJail enforcement;
- bounded context shaping;
- versioned invocation and factual observation;
- measured values, output identity, receipt linkage, and exact recovery checks.

Opaque caller correlation may cross the boundary but never makes Engine the
session-lifecycle authority.

## Why this is the smallest real path

The integrated Engine already executes the real native context capability and
emits `EngineInvocationV1`/`EngineObservationV1` with canonical receipt
artifacts. The existing `lean-ctx call` command proves local one-shot
dispatch, but its human-text output is not a stable SDK transport and does not
return the complete factual record. A narrow JSON CLI surface therefore reuses
the real Engine implementation without creating a Runtime session platform or
requiring MCP internals in the Embed path.

## Compatibility and failure rules

- Request, response, Engine interface, and protocol versions are explicit.
- Unknown fields and unsupported versions fail closed.
- Python enforces its configured process deadline.
- Rejection, malformed observations, digest mismatch, missing recovery data,
  and receipt verification failure remain explicit errors.
- Preview `fail_open` may preserve host execution only with a degraded,
  unsealed receipt; it never fabricates Engine coverage or task acceptance.
- Successful Engine delivery never implies accepted task quality.
