#!/usr/bin/env python3
"""Benchmark runner using Codex CLI as the LLM backend.

Reads HumanEval tasks, sends each to `codex exec`, runs the generated
code in a sandboxed subprocess, and produces a JSON report.

Supports batch mode: saves progress after each task and can resume
from an existing partial result file.

Usage:
    python3 scripts/benchmark_codex.py --tasks 5     # first 5 tasks (quick test)
    python3 scripts/benchmark_codex.py                # all 164 tasks
    python3 scripts/benchmark_codex.py --output report.json
    python3 scripts/benchmark_codex.py --resume report.json  # resume partial run
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

DATA_DIR = Path.home() / "Library" / "Application Support" / "lean-ctx" / "benchmark-data"
HUMANEVAL_PATH = DATA_DIR / "humaneval.ndjson"
TIMEOUT_CODEX = 120
TIMEOUT_SANDBOX = 30
PYTHON_BIN = "/opt/homebrew/bin/python3.11"


def load_humaneval(path, limit=None):
    tasks = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            tasks.append(json.loads(line))
            if limit and len(tasks) >= limit:
                break
    return tasks


def load_partial_results(path):
    """Load previously completed task results for resume."""
    if not Path(path).exists():
        return {}
    with open(path) as f:
        data = json.load(f)
    return {r["task_id"]: r for r in data.get("results", [])}


def save_progress(output_path, results, total_tasks):
    """Save current results to JSON (called after each task)."""
    passed = sum(1 for r in results if r.get("passed"))
    report = {
        "benchmark": "humaneval",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "python_version": "3.11",
        "total_tasks": total_tasks,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }
    Path(output_path).write_text(json.dumps(report, indent=2))


def extract_helper_functions(prompt, entry_point):
    """Extract helper function/import definitions from the prompt that the test needs.

    HumanEval tasks like /32, /38, /50 define helper functions (poly, encode_cyclic,
    encode_shift) in the prompt. The test calls these helpers, so they must be
    included in the test environment alongside the model's solution.
    """
    lines = prompt.split("\n")
    helpers = []
    current_block = []
    in_function = False
    current_func_name = None

    for line in lines:
        if line.startswith("def "):
            match = re.match(r"def\s+(\w+)\s*\(", line)
            if match:
                if current_block and current_func_name and current_func_name != entry_point:
                    helpers.append("\n".join(current_block))
                current_func_name = match.group(1)
                current_block = [line]
                in_function = True
                continue

        if line.startswith("import ") or line.startswith("from "):
            if not in_function:
                helpers.append(line)
                continue

        if in_function:
            if line and not line[0].isspace() and not line.startswith("def "):
                if current_block and current_func_name != entry_point:
                    helpers.append("\n".join(current_block))
                in_function = False
                current_func_name = None
                current_block = []
            else:
                current_block.append(line)

    if current_block and current_func_name and current_func_name != entry_point:
        helpers.append("\n".join(current_block))

    return "\n\n".join(helpers)


def solve_with_codex(prompt):
    """Call codex exec with a coding prompt. Returns (code, elapsed_seconds)."""
    full_prompt = (
        f"Write ONLY the complete Python function. No explanations, no tests, "
        f"no markdown fences, just the raw Python code. "
        f"Include all necessary imports (e.g. from typing import List, Tuple, Optional, etc.) "
        f"at the top if the function signature uses them.\n\n{prompt}"
    )
    codex_bin = os.environ.get("CODEX_BIN", os.path.expanduser("~/.local/bin/codex"))
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
    clean_env["LEAN_CTX_DISABLED"] = "1"
    clean_env["HOME"] = os.environ["HOME"]
    clean_env["PATH"] = os.environ["PATH"]

    start = time.time()
    try:
        result = subprocess.run(
            [codex_bin, "exec", "--sandbox", "read-only", full_prompt],
            capture_output=True,
            text=True,
            timeout=TIMEOUT_CODEX,
            env=clean_env,
        )
        elapsed = time.time() - start
        output = result.stdout.strip()
        return extract_code(output), elapsed
    except subprocess.TimeoutExpired:
        return "", time.time() - start
    except Exception:
        return "", time.time() - start


def extract_code(text):
    """Extract Python code from Codex output."""
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
        if line.startswith("def ") or line.startswith("    ") or line.startswith("class ") or not line.strip():
            lines.append(line)
        elif lines:
            break
    if lines:
        return "\n".join(lines).strip()

    return text.strip()


def ensure_imports(code):
    """Add missing imports for typing hints, stdlib modules, and common names."""
    imports = []

    typing_names = ["List", "Tuple", "Dict", "Set", "Optional", "Any", "Union"]
    used = [t for t in typing_names if t in code]
    if used and "from typing import" not in code and "import typing" not in code:
        imports.append(f"from typing import {', '.join(used)}")

    stdlib_modules = [
        "math", "re", "collections", "itertools", "functools",
        "string", "heapq", "bisect", "operator", "decimal", "fractions",
        "hashlib", "copy", "sys",
    ]
    for mod in stdlib_modules:
        if f"{mod}." in code and f"import {mod}" not in code:
            imports.append(f"import {mod}")

    bare_names = {
        "decimal": ["Decimal", "ROUND_HALF_UP", "ROUND_HALF_DOWN", "ROUND_HALF_EVEN"],
        "collections": ["defaultdict", "Counter", "deque", "OrderedDict"],
        "functools": ["reduce", "lru_cache", "cache"],
    }
    for module, names in bare_names.items():
        needed = [n for n in names if n in code]
        if not needed:
            continue
        already_in_code = f"from {module} import" in code
        already_in_imports = any(f"from {module} import" in imp for imp in imports)
        if not already_in_code and not already_in_imports:
            imports.append(f"from {module} import {', '.join(needed)}")

    if imports:
        return "\n".join(imports) + "\n" + code
    return code


def run_test(solution, test_code, entry_point, helpers=""):
    """Run the solution + test harness in a sandboxed subprocess."""
    solution = ensure_imports(solution)
    parts = []
    if helpers.strip():
        parts.append(helpers)
    parts.append(solution)
    parts.append(test_code)
    parts.append(f"check({entry_point})")
    script = "\n\n".join(parts) + "\n"

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
            return result.returncode == 0, result.stderr
        except subprocess.TimeoutExpired:
            return False, "timeout"
        finally:
            os.unlink(f.name)


def run_benchmark(tasks, output_path, existing_results):
    """Run benchmark with incremental saves and resume support."""
    results = list(existing_results.values())
    completed_ids = set(existing_results.keys())
    passed = sum(1 for r in results if r.get("passed"))
    total = len(tasks)
    skipped = 0

    for i, task in enumerate(tasks):
        task_id = task["task_id"]

        if task_id in completed_ids:
            skipped += 1
            continue

        idx = i + 1
        print(f"[{idx}/{total}] {task_id}...", end=" ", flush=True)

        helpers = extract_helper_functions(task["prompt"], task["entry_point"])
        solution, elapsed = solve_with_codex(task["prompt"])

        if not solution:
            print("SKIP (no output)")
            result = {
                "task_id": task_id,
                "passed": False,
                "error": "codex returned no output",
                "elapsed_s": round(elapsed, 2),
            }
        else:
            ok, stderr = run_test(solution, task["test"], task["entry_point"], helpers)
            if ok:
                passed += 1
                print(f"PASS ({elapsed:.1f}s)")
            else:
                print(f"FAIL ({elapsed:.1f}s)")
            result = {
                "task_id": task_id,
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "solution_preview": solution[:200],
            }

        results.append(result)
        if output_path:
            save_progress(output_path, results, total)

    if skipped:
        print(f"(Skipped {skipped} already-completed tasks)")

    return {
        "benchmark": "humaneval",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "python_version": "3.11",
        "total_tasks": total,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }


def main():
    parser = argparse.ArgumentParser(description="HumanEval benchmark via Codex CLI")
    parser.add_argument("--tasks", type=int, default=None, help="Limit to first N tasks")
    parser.add_argument("--output", "-o", type=str, default=None, help="Output JSON path")
    parser.add_argument("--resume", type=str, default=None,
                        help="Resume from partial result file (also used as output)")
    args = parser.parse_args()

    if not HUMANEVAL_PATH.exists():
        print(f"Error: {HUMANEVAL_PATH} not found.", file=sys.stderr)
        sys.exit(1)

    if not Path(PYTHON_BIN).exists():
        print(f"Error: {PYTHON_BIN} not found. Install Python 3.11+.", file=sys.stderr)
        sys.exit(1)

    output_path = args.resume or args.output

    existing = {}
    if args.resume:
        existing = load_partial_results(args.resume)
        if existing:
            print(f"Resuming: {len(existing)} tasks already completed")

    tasks = load_humaneval(HUMANEVAL_PATH)
    if args.tasks:
        tasks = tasks[:args.tasks]

    print(f"Running {len(tasks)} HumanEval tasks via Codex CLI...")
    remaining = len(tasks) - len(existing)
    print(f"Remaining: {remaining} tasks (~{remaining * 5}s estimated)")
    print(f"Python: {PYTHON_BIN}")
    print()

    report = run_benchmark(tasks, output_path, existing)

    print()
    print(f"=== Results ===")
    print(f"Pass rate: {report['tasks_passed']}/{report['tasks_completed']} ({report['pass_rate']*100:.1f}%)")

    if output_path:
        save_progress(output_path, report["results"], report["total_tasks"])
        print(f"Report saved to {output_path}")
    else:
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
