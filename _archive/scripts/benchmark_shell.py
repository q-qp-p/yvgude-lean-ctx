#!/usr/bin/env python3
"""Shell output compression benchmark via Codex CLI.

Tests whether LLMs can parse and understand shell command output when
presented raw vs lean-ctx compressed (PATH-stub approach triggers the
real production shell compression pipeline).

Usage:
    python3 scripts/benchmark_shell.py --output report.json
    python3 scripts/benchmark_shell.py --category parsing --output quick.json
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

TIMEOUT_CODEX = 90
TIMEOUT_SANDBOX = 30
PYTHON_BIN = "/opt/homebrew/bin/python3.11"
LEAN_CTX_BIN = os.path.expanduser("~/.local/bin/lean-ctx")
TASKS_PATH = Path(__file__).parent / "shell_benchmark_tasks.json"


def load_tasks(path=None, category=None):
    data_path = path or TASKS_PATH
    with open(data_path) as f:
        tasks = json.load(f)
    if category:
        tasks = [t for t in tasks if t.get("category") == category]
    return tasks


def compress_output(raw_output, command):
    """PATH-stub: fake binary outputs synthetic data, lean-ctx -c compresses it."""
    bin_name = command.split()[0]
    fake_dir = tempfile.mkdtemp(prefix="lc_bench_")
    data_file = os.path.join(fake_dir, "data.txt")
    try:
        with open(data_file, "w") as f:
            f.write(raw_output)
        stub_path = os.path.join(fake_dir, bin_name)
        with open(stub_path, "w") as f:
            f.write(f"#!/bin/sh\ncat '{data_file}'\n")
        os.chmod(stub_path, 0o755)
        env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
        env["HOME"] = os.environ["HOME"]
        env["PATH"] = f"{fake_dir}:{os.environ['PATH']}"
        result = subprocess.run(
            [LEAN_CTX_BIN, "-c", command],
            capture_output=True,
            text=True,
            timeout=15,
            env=env,
        )
        compressed = result.stdout.strip()
        if compressed and len(compressed) < len(raw_output):
            return compressed, round((1 - len(compressed) / len(raw_output)) * 100, 1)
        return raw_output, 0.0
    except Exception:
        return raw_output, 0.0
    finally:
        shutil.rmtree(fake_dir, ignore_errors=True)


def build_prompt(task, compressed=False):
    raw = task["raw_output"]
    if compressed:
        output_text, comp_pct = compress_output(raw, task["command"])
    else:
        output_text, comp_pct = raw, 0.0

    label = "compressed" if compressed else "raw"
    prompt = (
        f"The following is {label} output from running: `{task['command']}`\n\n"
        f"```\n{output_text}\n```\n\n"
        f"{task['question']}\n\n"
        f"Write ONLY the Python function(s) requested. No explanations, no tests, "
        f"no markdown fences."
    )
    return prompt, comp_pct, len(raw), len(output_text)


def solve_with_codex(prompt_text):
    """Call codex exec and return (code, elapsed_seconds)."""
    codex_bin = os.path.expanduser("~/.local/bin/codex")
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
    clean_env["LEAN_CTX_DISABLED"] = "1"
    clean_env["HOME"] = os.environ["HOME"]
    clean_env["PATH"] = os.environ["PATH"]

    start = time.time()
    try:
        result = subprocess.run(
            [codex_bin, "exec", "--sandbox", "read-only", prompt_text],
            capture_output=True,
            text=True,
            timeout=TIMEOUT_CODEX,
            env=clean_env,
        )
        elapsed = time.time() - start
        return extract_code(result.stdout.strip()), elapsed
    except subprocess.TimeoutExpired:
        return "", time.time() - start
    except Exception:
        return "", time.time() - start


def extract_code(text):
    if "```python" in text:
        match = re.search(r"```python\s*\n(.*?)```", text, re.DOTALL)
        if match:
            return match.group(1).strip()
    if "```" in text:
        match = re.search(r"```\w*\s*\n(.*?)```", text, re.DOTALL)
        if match:
            return match.group(1).strip()

    lines = []
    for line in text.split("\n"):
        if (
            line.startswith("def ")
            or line.startswith("    ")
            or line.startswith("class ")
            or line.startswith("import ")
            or line.startswith("from ")
            or not line.strip()
        ):
            lines.append(line)
        elif lines:
            break
    return "\n".join(lines).strip() if lines else text.strip()


def run_test(solution, test_code, raw_output):
    """Execute solution + test with OUTPUT set to raw_output."""
    escaped = raw_output.replace("\\", "\\\\").replace("'''", "\\'\\'\\'")
    script = f"{solution}\n\nOUTPUT = '''{escaped}'''\n\n{test_code}\n"

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        f.flush()
        try:
            result = subprocess.run(
                [PYTHON_BIN, f.name],
                capture_output=True,
                text=True,
                timeout=TIMEOUT_SANDBOX,
                env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
            )
            ok = result.returncode == 0
            err = result.stderr or result.stdout
            return ok, err
        except subprocess.TimeoutExpired:
            return False, "timeout"
        finally:
            os.unlink(f.name)


def summarize_by_category(tasks, results):
    by_category = {}
    result_map = {r["task_id"]: r for r in results}

    for task in tasks:
        cat = task.get("category", "unknown")
        bucket = by_category.setdefault(
            cat, {"total": 0, "passed": 0, "control": 0, "compressed": 0}
        )
        bucket["total"] += 1
        if result_map.get(task["id"], {}).get("passed"):
            bucket["passed"] += 1

    return by_category


def merge_category_rates(by_ctrl, by_comp):
    merged = {}
    for cat in sorted(set(by_ctrl) | set(by_comp)):
        ctrl = by_ctrl.get(cat, {"total": 0, "passed": 0})
        comp = by_comp.get(cat, {"total": 0, "passed": 0})
        total = ctrl["total"]
        merged[cat] = {
            "total": total,
            "control": ctrl["passed"],
            "compressed": comp["passed"],
            "delta_pp": round((comp["passed"] - ctrl["passed"]) / total * 100, 1) if total else 0,
        }
    return merged


def run_benchmark(tasks, compressed=False):
    results = []
    passed = 0
    total_orig = 0
    total_comp = 0
    compression_pcts = []

    for task in tasks:
        tid = task["id"]
        category = task.get("category", "unknown")
        prompt, comp_pct, orig_len, comp_len = build_prompt(task, compressed=compressed)

        if compressed:
            total_orig += orig_len
            total_comp += comp_len
            compression_pcts.append(comp_pct)

        sfx = f" ({comp_pct}% compressed)" if compressed else ""
        print(f"  {tid}{sfx}...", end=" ", flush=True)

        solution, elapsed = solve_with_codex(prompt)

        if not solution:
            print(f"SKIP ({elapsed:.1f}s)")
            results.append(
                {
                    "task_id": tid,
                    "category": category,
                    "passed": False,
                    "error": "codex returned no output",
                    "elapsed_s": round(elapsed, 2),
                    "compression_pct": comp_pct if compressed else 0,
                }
            )
            continue

        ok, stderr = run_test(solution, task["test"], task["raw_output"])
        if ok:
            passed += 1
            print(f"PASS ({elapsed:.1f}s)")
        else:
            print(f"FAIL ({elapsed:.1f}s)")

        results.append(
            {
                "task_id": tid,
                "category": category,
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "compression_pct": comp_pct if compressed else 0,
            }
        )

    avg_comp = round(sum(compression_pcts) / len(compression_pcts), 1) if compression_pcts else 0
    if compressed and total_orig:
        avg_comp = round((1 - total_comp / total_orig) * 100, 1)
    return results, passed, avg_comp


def main():
    parser = argparse.ArgumentParser(description="Shell output compression benchmark")
    parser.add_argument("--output", "-o", type=str, default=None)
    parser.add_argument("--tasks", type=int, default=None, help="Limit number of tasks")
    parser.add_argument(
        "--tasks-file",
        type=str,
        default=None,
        help="Path to tasks JSON (default: scripts/shell_benchmark_tasks.json)",
    )
    parser.add_argument(
        "--category",
        type=str,
        choices=["parsing", "understanding"],
        default=None,
        help="Run only parsing or understanding tasks",
    )
    args = parser.parse_args()

    if not Path(PYTHON_BIN).exists():
        print(f"Error: {PYTHON_BIN} not found.", file=sys.stderr)
        sys.exit(1)
    if not Path(LEAN_CTX_BIN).exists():
        print(f"Error: {LEAN_CTX_BIN} not found.", file=sys.stderr)
        sys.exit(1)

    tasks_path = Path(args.tasks_file) if args.tasks_file else TASKS_PATH
    if not tasks_path.exists():
        print(f"Error: {tasks_path} not found.", file=sys.stderr)
        sys.exit(1)

    all_tasks = load_tasks(tasks_path, category=args.category)
    tasks = all_tasks[: args.tasks] if args.tasks else all_tasks
    total = len(tasks)

    print(f"=== Shell Compression Benchmark v2 ({total} tasks) ===\n")

    print("--- Control (raw output) ---")
    ctrl_results, ctrl_passed, _ = run_benchmark(tasks, compressed=False)
    print(f"Control: {ctrl_passed}/{total} ({ctrl_passed / total * 100:.1f}%)\n")

    print("--- Compressed (lean-ctx shell pipeline) ---")
    comp_results, comp_passed, avg_comp = run_benchmark(tasks, compressed=True)
    print(f"Compressed: {comp_passed}/{total} ({comp_passed / total * 100:.1f}%)")
    print(f"Avg compression: {avg_comp}%\n")

    by_category = merge_category_rates(
        summarize_by_category(tasks, ctrl_results),
        summarize_by_category(tasks, comp_results),
    )

    report = {
        "benchmark": "shell-compression-v2",
        "total_tasks": total,
        "control": {
            "passed": ctrl_passed,
            "rate": round(ctrl_passed / total, 4) if total else 0,
            "results": ctrl_results,
        },
        "compressed": {
            "passed": comp_passed,
            "rate": round(comp_passed / total, 4) if total else 0,
            "avg_compression_pct": avg_comp,
            "results": comp_results,
        },
        "delta_pp": round((comp_passed - ctrl_passed) / total * 100, 1) if total else 0,
        "by_category": by_category,
    }

    print("=== Summary ===")
    print(f"Control:    {ctrl_passed}/{total} ({ctrl_passed / total * 100:.1f}%)")
    print(f"Compressed: {comp_passed}/{total} ({comp_passed / total * 100:.1f}%)")
    print(f"Delta:      {report['delta_pp']:+.1f}pp")
    print("\nBy category:")
    for cat, stats in by_category.items():
        print(
            f"  {cat:14s} control={stats['control']}/{stats['total']} "
            f"compressed={stats['compressed']}/{stats['total']} "
            f"delta={stats['delta_pp']:+.1f}pp"
        )

    if args.output:
        Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
        print(f"\nReport saved to {args.output}")
    else:
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
