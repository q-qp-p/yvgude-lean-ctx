"""Preflight: verify the machine can run the benchmark before spending money.

Checks everything the runbook needs — binaries, auth, docker, network, the
frozen task lock — and prints one PASS/FAIL line each. Exit 1 if anything
required for the requested stage is missing.

Usage:
    python3 -m swebench_harness.preflight             # checks for agent runs
    python3 -m swebench_harness.preflight --evaluate  # also checks docker
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import urllib.request

from . import BENCH_ROOT, load_config


def run(cmd: list) -> "tuple[int, str]":
    try:
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=30)
        return res.returncode, (res.stdout or "").strip()
    except FileNotFoundError:
        return 127, "not found"
    except subprocess.TimeoutExpired:
        return 124, "timed out"


class Preflight:
    def __init__(self) -> None:
        self.failed = False

    def check(self, name: str, ok: bool, detail: str) -> None:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name:<28} {detail}")
        if not ok:
            self.failed = True


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--evaluate", action="store_true", help="also check the docker evaluation prerequisites")
    args = ap.parse_args()

    cfg = load_config()
    pf = Preflight()
    print("agent-run prerequisites:")

    code, out = run([cfg["agent"]["binary"], "--version"])
    pf.check("claude CLI", code == 0, out.splitlines()[0] if out else "not found")

    code, out = run([cfg["leanctx"]["binary"], "--version"])
    pf.check("lean-ctx CLI", code == 0, out.splitlines()[0] if out else "not found")

    code, out = run(["git", "--version"])
    pf.check("git", code == 0, out)

    has_key = bool(os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_AUTH_TOKEN"))
    pf.check(
        "ANTHROPIC_API_KEY",
        has_key,
        "set" if has_key else "missing — fresh-HOME runs have no stored login (export it first)",
    )

    lock = BENCH_ROOT / "tasks.lock.json"
    if lock.exists():
        n = json.loads(lock.read_text())["n"]
        pf.check("tasks.lock.json", True, f"frozen, n={n}")
    else:
        pf.check("tasks.lock.json", False, "missing — python3 -m swebench_harness.select_tasks (one-time)")

    try:
        with urllib.request.urlopen("https://github.com", timeout=10) as resp:
            pf.check("github.com reachable", resp.status < 500, f"HTTP {resp.status} (repo mirrors clone from here)")
    except OSError as e:
        pf.check("github.com reachable", False, str(e))

    free_gb = shutil.disk_usage(BENCH_ROOT).free / 1e9
    pf.check("disk space", free_gb > 20, f"{free_gb:.0f} GB free (mirrors ~2 GB, eval images need more)")

    if args.evaluate:
        print("evaluation prerequisites:")
        code, out = run(["docker", "info", "--format", "{{.ServerVersion}}"])
        pf.check("docker daemon", code == 0, f"server {out}" if code == 0 else out[-120:])
        code, out = run([sys.executable, "-c", "import swebench; print(swebench.__version__)"])
        pf.check("swebench package", code == 0, out if code == 0 else "pip install -r requirements.txt")

    if pf.failed:
        sys.exit(1)
    print("all checks passed.")


if __name__ == "__main__":
    main()
