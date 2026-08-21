# OCLA Capability Manifest v1

The [JSON Schema](ocla-capability-manifest-v1.schema.json) is the formal wire
shape for `CapabilityManifestV1`. A manifest declares a provider capability,
its supported invocation surfaces, execution locality, data handling boundary,
and measurement support.

All fields are required unless marked optional in the schema. `input_schema_ref`
and `output_schema_ref` are optional capability-wide references. Each
`support_matrix` value is a closed `SurfaceSupportV1` object; likewise,
`measurement_support` is a closed `MeasurementSupportV1` object. The top-level
object deliberately permits additional properties: Rust retains them in the
manifest's `extra` map.

## Compatibility

`schema_version` is fixed at `1`. Evolution is additive only:

- Add new optional top-level fields; consumers must preserve and ignore fields
  they do not understand.
- Do not remove, rename, or change the meaning or JSON type of a v1 field.
- Do not add values to existing enums in a v1-compatible change; enum expansion
  requires a new schema version because current consumers deserialize enums
  strictly.
- Do not add fields to `measurement_support` or support-matrix values in v1;
  those Rust structures reject unknown nested fields.

## Validation

Conforming producers must satisfy this schema and deserialize as
`CapabilityManifestV1`. They must also pass `validate_manifest()` before
registration or invocation. Runtime validation enforces non-empty capability
IDs and versions, a non-empty unique surface list, at least one execution
locality, a declared movement boundary for remote execution, and locality rules
for confidential or restricted data.

`kind`, `reversibility`, `determinism`, and `data_movement` use the exact
snake-case Rust enum wire values documented by the schema. Data classifications
retain their Rust variant spelling: `Public`, `Internal`, `Confidential`, and
`Restricted`.

## Example

```json
{
  "schema_version": 1,
  "capability_id": "capability://example/search",
  "provider": "example",
  "kind": "tool",
  "version": "1.0.0",
  "surfaces": ["mcp"],
  "support_matrix": {
    "mcp": {"supported": true}
  },
  "local": true,
  "remote": false,
  "reversibility": "reversible",
  "determinism": "deterministic",
  "data_movement": "local_only",
  "supported_classifications": ["Public", "Internal"],
  "measurement_support": {"latency": true, "tokens": true, "quality": false},
  "conformance_version": 1,
  "provider_extension": {"rollout": "stable"}
}
```
