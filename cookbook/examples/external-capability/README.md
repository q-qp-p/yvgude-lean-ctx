# External OCLA capability example

This self-contained Rust program demonstrates a third-party, deterministic OCLA capability executed as a local process. It accepts UTF-8 text on standard input and writes one JSON result to standard output. `manifest.json` advertises its CLI surface, local-only data movement, and input/output schemas.

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
