.PHONY: setup-hooks install dev test preflight preflight-fast enterprise-ci-check help

ENTERPRISE_DIR ?= ../lean-ctx-enterprise

# ── Setup ─────────────────────────────────────────────────

setup-hooks: ## Configure git to use .githooks/ for hooks
	git config core.hooksPath .githooks
	@echo "Git hooks configured: .githooks/"

# ── Build & Install ──────────────────────────────────────

install: ## Build release + install to ~/.local/bin
	cd rust && cargo install --path . --force --locked --root "$$HOME/.local"
	@echo "Installed: $$(lean-ctx --version)"

dev: ## Quick debug build + copy to ~/.local/bin
	cd rust && cargo build
	@mkdir -p "$$HOME/.local/bin"
	@# rm before cp: overwriting an existing Mach-O in place invalidates the
	@# ad-hoc code signature on Apple Silicon — the copied binary dies with
	@# SIGKILL (exit 137) on every exec. Removing first gives the new file a
	@# fresh identity; re-sign ad hoc for good measure where codesign exists.
	rm -f "$$HOME/.local/bin/lean-ctx"
	cp rust/target/debug/lean-ctx "$$HOME/.local/bin/lean-ctx"
	@command -v codesign >/dev/null 2>&1 && codesign -f -s - "$$HOME/.local/bin/lean-ctx" 2>/dev/null || true
	@echo "Dev installed: $$(lean-ctx --version)"

test: ## Run all Rust tests + clippy
	cd rust && cargo test && cargo clippy

# ── CI-parity gate ───────────────────────────────────────
# Mirrors .github/workflows/ci.yml so green-here => green-in-CI for the
# deterministic jobs (fmt/clippy/doc/gen_docs/cross-platform compile).

preflight: ## Full local CI-parity gate (fmt, clippy, doc, gen_docs, win-check, lib tests)
	scripts/preflight.sh full

preflight-fast: ## Static CI-parity gate (no full test run) — what pre-push runs
	scripts/preflight.sh fast

# ── Enterprise CI ────────────────────────────────────────────

enterprise-ci-check: ## Validate the separate enterprise checkout for feature-flag CI
	@test -d "$(ENTERPRISE_DIR)" || { echo "Enterprise checkout not found: $(ENTERPRISE_DIR)"; exit 1; }
	@test -f "$(ENTERPRISE_DIR)/Cargo.toml" || { echo "Missing Cargo.toml in $(ENTERPRISE_DIR)"; exit 1; }
	@cd "$(ENTERPRISE_DIR)" && cargo metadata --no-deps --format-version 1 >/dev/null
	@echo "Enterprise checkout ready: $(ENTERPRISE_DIR)"

# ── Help ──────────────────────────────────────────────────

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
