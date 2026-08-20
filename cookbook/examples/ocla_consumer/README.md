# OCLA consumer example

This standalone Rust binary consumes the public OCLA Wire API over HTTP. It
does not depend on the `lean-ctx` crate or any internal implementation types.

## Build

```bash
cd examples/ocla_consumer
cargo build
```

## Usage

The default server URL is `http://localhost:3333`; override it with `--url`:

```bash
cargo run -- health
cargo run -- --url http://localhost:3333 capabilities
cargo run -- validate envelope.json
cargo run -- summary
```

`validate` first parses the file with local wire-contract structs, then sends
the original JSON to `POST /ocla/v1/envelope`. The other commands use the
public `GET` endpoints for health, capabilities, and ledger summary data.
