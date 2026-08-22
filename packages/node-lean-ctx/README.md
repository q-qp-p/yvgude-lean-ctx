# lean-ctx (Node SDK)

> **Status: research implementation reference — not a supported public SDK.**
> LeanCTX is **The Context SDK for AI Agents**, with local Runtime/CLI/MCP/proxy
> capability available. The declared SDK Preview is Python SDK v1 and its
> OpenAI Agents reference wrapper. This Node package must not be presented as a
> general agent platform, universal model wrapper, hosted service, or a
> performance guarantee. Product scope and status are governed by
> [`docs/internal/README.md`](../../docs/internal/README.md).

An experimental, dependency-free client for a local lean-ctx Runtime. Discover
the Runtime's capabilities and handle unavailable routes explicitly.

```bash
npm install lean-ctx-sdk
```

## Drop-in `compress(messages, { model })`

Compress a chat-style `messages` array before sending it to any model. Only text
payloads are rewritten through lean-ctx's deterministic funnel; images,
tool-call blocks and ids pass through untouched, and the output is byte-stable so
it stays friendly to provider prompt caching.

```ts
import { compress } from "lean-ctx-sdk";

let messages = [
  { role: "system", content: "You are a helpful assistant." },
  { role: "user", content: largeLogOrFileDump },
];

messages = await compress(messages, { model: "claude-sonnet-4" });
// → send `messages` to your provider as usual
```

Works with both OpenAI-style (`content: "string"`) and Anthropic-style
(`content: [{ type: "text", … }, { type: "tool_result", … }]`) messages.

### Token-savings stats

```ts
import { ProxyClient } from "lean-ctx-sdk";

const result = await new ProxyClient().compress(messages, "gpt-4o");
console.log(result.stats.saved_tokens, result.stats.saved_pct);
messages = result.messages;
```

## Configuration

The endpoint and session token are auto-discovered from the running daemon. Every
step is overridable:

| Setting | Env var | Default |
| --- | --- | --- |
| Proxy URL | `LEAN_CTX_PROXY_URL` | `http://127.0.0.1:<port>` |
| Proxy port | `LEAN_CTX_PROXY_PORT` | `config.toml` `proxy_port`, else UID-derived |
| Session token | `LEAN_CTX_PROXY_TOKEN` | `<data_dir>/session_token` |

Or pass them explicitly (useful in CI / against a remote proxy):

```ts
await compress(messages, { baseUrl: "http://127.0.0.1:4444", token: "…" });
```

If the daemon is not running, `compress()` rejects with `LeanCtxConnectionError`;
an unauthenticated request rejects with `LeanCtxAuthError`. Both extend
`LeanCtxError`.

## Vercel AI SDK

Compress every prompt automatically with language-model middleware:

```ts
import { wrapLanguageModel } from "ai";
import { openai } from "@ai-sdk/openai";
import { leanCtxMiddleware } from "lean-ctx-sdk";

const model = wrapLanguageModel({
  model: openai("gpt-4o"),
  middleware: leanCtxMiddleware({ model: "gpt-4o" }),
});
// every generateText / streamText call now sends a compressed prompt
```

`withLeanCtx(openai("gpt-4o"))` is a one-liner shortcut (needs the optional `ai`
peer dependency). A compaction failure never breaks a generation — the original,
uncompressed prompt is sent instead.

## Other helpers

`LeanCtxClient` wraps the `lean-ctx` binary for `read` / `search` / `shell` /
`gain` / `benchmark`, and `createLeanCtxTool` exposes a Vercel AI SDK search tool.

## Learn more

- [compress() SDK cookbook](https://github.com/yvgude/lean-ctx/blob/main/docs/guides/compress-sdk.md) — Python + TypeScript recipes
- [lean-ctx vs Headroom](https://github.com/yvgude/lean-ctx/blob/main/docs/comparisons/vs-headroom.md) — comparison + reproducible benchmark

## License

MIT
