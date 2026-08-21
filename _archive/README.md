# `_archive/` — restoration index

**Status:** historical source only. Not a supported product surface.  
**Updated:** 2026-08-21  
**Owner:** Yves Gugger

WS-1 moved inactive trees here with `git mv` so Git history stays intact. Nothing in this directory is part of the current SDK, runtime, or website.

## How to restore a tree

```bash
git mv _archive/<name> <original-path>
# then repair CI, docs, and package metadata before considering a re-release
```

Do not copy files out of `_archive/` and present them as current. Restoration is a product decision plus a PR.

## Canonical replacements

| Need | Use instead |
|---|---|
| Python SDK | `packages/python-lean-ctx/` (`lean-ctx-python` on PyPI) |
| Node / TypeScript SDK | `packages/node-lean-ctx/` or `cookbook/sdk/` |
| OCLA wire contracts | `docs/contracts/` (includes merged `ocla.proto` + capability manifests) |
| Benchmarks | `scripts/benchmark/` |
| Rust runtime | `rust/` |
| Examples | `cookbook/examples/` |

## Inventory

| Archive path | Original path | Why archived | Restore? |
|---|---|---|---|
| `py-sdk/` | `py-sdk/` | Duplicate Python SDK | No — use `packages/python-lean-ctx/` |
| `python-sdk/` | `python-sdk/` | Duplicate Python SDK | No |
| `clients-python/` | `clients/python/` | Duplicate Python client | No |
| `marketing/` | `marketing/` | Private collateral | Keep archived |
| `email-templates/` | `email-templates/` | Private collateral | Keep archived |
| `demo/` | `demo/` | Sample/demo, not product | Keep archived |
| `blog/` | `blog/` | Content moved off-repo | Keep archived |
| `lean/` | `lean/` | Unused tree | Keep archived |
| `bench/` `benchmark/` `benchmarks/` | `bench/` `benchmark/` `benchmarks/` | Consolidated; runner is `scripts/benchmark/` | Only if reproducing a historical study |
| `specs/` | `specs/` | Implemented historical issue specs | Keep archived |
| `discord-faq/` | `discord-faq/` | Orphan FAQ copy (`discord-faq.md` remains at repo root) | Keep archived |
| `vscode-extension/` | `vscode-extension/` | No current release/support commitment | Product decision required |
| `jetbrains-lean-ctx/` | `packages/jetbrains-lean-ctx/` | Same | Product decision required |
| `chrome-lean-ctx/` | `packages/chrome-lean-ctx/` | Same | Product decision required |
| `emacs-lean-ctx/` | `packages/emacs-lean-ctx/` | Same | Product decision required |
| `neovim-lean-ctx/` | `packages/neovim-lean-ctx/` | Same | Product decision required |
| `sublime-lean-ctx/` | `packages/sublime-lean-ctx/` | Same | Product decision required |
| `pi-lean-ctx/` | `packages/pi-lean-ctx/` | Same | Product decision required |
| `go-sdk/` | `go-sdk/` | Unmaintained community SDK | Product decision required |
| `integrations/` | `integrations/` | Hermes + Datadog, stale | Product decision required |
| `scripts/` | selected files from `scripts/` | One-off bench/demo/pilot/monitor scripts | Keep archived |

Gitignored (never in this archive, still local-only): `lab/`, `cloud/`, `website/`, `discord-bot/`, `n8n-workflows/`, `memory-bank/`, `docs/internal/`.

## Commits

- Wave 1 Python: `f615c8d`
- Wave 2 unused dirs: `6441f73`
- Wave 3 bench dirs: `6679c89`
- Prio 1 duplicates: `8ea700f`
- Prio 2+3 archive: `6ff1e48`
- Prio 4+5 scripts: `235d135`
