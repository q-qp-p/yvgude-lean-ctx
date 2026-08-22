# External OCLA capability example

This self-contained Rust program is a reference-only, deterministic OCLA capability executed as a local process. It accepts at most 65,536 bytes of UTF-8 on standard input and writes one bounded JSON result to standard output. `manifest.json` declares its CLI surface, local-only data movement, no-network boundary, limits, and input/output schemas.

It makes no registration, marketplace, or public-product claim. Hosts must opt in with a fixed executable and manifest; LeanCTX's `ExternalProcessAdapter` validates the manifest, clears the process environment, sends only bounded stdin, caps stdout, and enforces a timeout. Oversized or invalid UTF-8 input is rejected without output.

## Run

```bash
cd cookbook/examples/external-capability
printf 'Hello, OCLA!\n' | cargo run --quiet
```

```json
{"word_count":2,"char_count":13,"line_count":1}
```

`word_count` uses Unicode whitespace boundaries, `char_count` counts Unicode scalar values, and `line_count` follows Rust's `str::lines` semantics.

Copy `manifest.json` with the executable when registering this capability in an OCLA-compatible host.
