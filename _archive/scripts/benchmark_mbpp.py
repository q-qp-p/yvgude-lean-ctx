#!/usr/bin/env python3
"""MBPP benchmark runner using Codex CLI.

Runs the MBPP sanitized test split (257 tasks) with optional lean-ctx
context compression for A/B comparison.

Usage:
    python3 scripts/benchmark_mbpp.py --tasks 10                    # quick test
    python3 scripts/benchmark_mbpp.py --output report.json          # baseline
    python3 scripts/benchmark_mbpp.py --compressed --output c.json  # compressed arm
    python3 scripts/benchmark_mbpp.py --resume report.json          # resume
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
MBPP_PATH = DATA_DIR / "mbpp-sanitized.json"
TIMEOUT_CODEX = 120
TIMEOUT_SANDBOX = 30
PYTHON_BIN = "/opt/homebrew/bin/python3.11"
LEAN_CTX_BIN = os.path.expanduser("~/.local/bin/lean-ctx")

SURROUNDING_MODULE = '''"""Utility module for algorithmic operations.

Provides helper functions for common programming tasks including
string manipulation, mathematical operations, list processing,
and data transformations.

Version: 2.4.1
"""

import math
import re
import sys
from typing import List, Tuple, Dict, Set, Optional, Any, Union
from collections import defaultdict, Counter, deque
from functools import reduce, lru_cache
from itertools import chain, combinations, permutations
from decimal import Decimal, ROUND_HALF_UP
import hashlib
import copy
import heapq
import bisect
import operator
import string


def validate_input(data: Any, expected_type: type) -> bool:
    if not isinstance(data, expected_type):
        raise TypeError(f"Expected {{expected_type.__name__}}, got {{type(data).__name__}}")
    return True


def safe_divide(a: float, b: float, default: float = 0.0) -> float:
    return a / b if b != 0 else default


def flatten_list(nested: List) -> List:
    result = []
    for item in nested:
        if isinstance(item, list):
            result.extend(flatten_list(item))
        else:
            result.append(item)
    return result


class DataProcessor:
    def __init__(self, data: List[Any]):
        self.data = data
        self._transforms = []

    def add_transform(self, fn):
        self._transforms.append(fn)
        return self

    def execute(self) -> List[Any]:
        result = self.data
        for fn in self._transforms:
            result = [fn(item) for item in result]
        return result


# === TARGET FUNCTION BELOW ===

'''


def load_mbpp(path, limit=None):
    """Load MBPP sanitized test split (task IDs 11-510)."""
    with open(path) as f:
        data = json.load(f)
    tasks = [t for t in data if 11 <= t["task_id"] <= 510]
    if limit:
        tasks = tasks[:limit]
    return tasks


def infer_function_name(test_list):
    """Extract the expected function name from test assertions."""
    for test in test_list:
        cleaned = test.replace("assert ", "").replace("assert(", "").strip()
        match = re.match(r"(\w+)\s*\(", cleaned)
        if match:
            name = match.group(1)
            if name not in ("set", "list", "tuple", "sorted", "len", "isinstance",
                            "True", "False", "round", "type", "max", "min", "sum"):
                return name
        match = re.search(r"(\w+)\s*\(", cleaned)
        if match:
            name = match.group(1)
            if name not in ("set", "list", "tuple", "sorted", "len", "isinstance",
                            "True", "False", "round", "type", "max", "min", "sum"):
                return name
    return None


def load_partial_results(path):
    if not Path(path).exists():
        return {}
    with open(path) as f:
        data = json.load(f)
    return {r["task_id"]: r for r in data.get("results", [])}


def save_progress(output_path, results, total_tasks, arm, avg_compression=0):
    passed = sum(1 for r in results if r.get("passed"))
    report = {
        "benchmark": "mbpp-sanitized",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "arm": arm,
        "python_version": "3.11",
        "avg_compression_pct": avg_compression,
        "total_tasks": total_tasks,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }
    Path(output_path).write_text(json.dumps(report, indent=2))


def compress_context(prompt, fn_name):
    """Compress surrounding module context, keep prompt + fn_name intact."""
    target_section = f"# Task: {prompt}\n# Expected function name: {fn_name}"
    module = SURROUNDING_MODULE + target_section

    surrounding_only = SURROUNDING_MODULE + "# (target function omitted)"
    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(surrounding_only)
        f.flush()
        try:
            clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
            clean_env["HOME"] = os.environ["HOME"]
            clean_env["PATH"] = os.environ["PATH"]
            result = subprocess.run(
                [LEAN_CTX_BIN, "read", f.name, "-m", "map"],
                capture_output=True, text=True, timeout=10, env=clean_env,
            )
            compressed_ctx = result.stdout.strip()
        except Exception:
            compressed_ctx = surrounding_only
        finally:
            os.unlink(f.name)

    combined = f"{compressed_ctx}\n\n# === TASK ===\n{target_section}"
    return combined, len(module), len(combined)


def build_prompt(task_prompt, fn_name, test_list, compressed=False, compressed_context=None):
    """Build the full prompt for Codex CLI."""
    tests_str = "\n".join(test_list[:2])

    if compressed and compressed_context:
        return (
            f"Below is a Python module context (compressed). "
            f"Implement the function `{fn_name}` as described.\n\n"
            f"--- Module Context ---\n{compressed_context}\n--- End Context ---\n\n"
            f"Task: {task_prompt}\n\n"
            f"Example tests:\n{tests_str}\n\n"
            f"Write ONLY the Python function `{fn_name}`. No explanations, no tests, "
            f"no markdown. Include necessary imports."
        )

    return (
        f"Write ONLY the complete Python function. No explanations, no tests, "
        f"no markdown fences, just the raw Python code. "
        f"Include all necessary imports at the top.\n\n"
        f"Task: {task_prompt}\n\n"
        f"The function should be named `{fn_name}`.\n\n"
        f"Example tests:\n{tests_str}"
    )


def solve_with_codex(prompt_text):
    """Call codex exec. Returns (code, elapsed_seconds)."""
    codex_bin = os.environ.get("CODEX_BIN", os.path.expanduser("~/.local/bin/codex"))
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
    clean_env["LEAN_CTX_DISABLED"] = "1"
    clean_env["HOME"] = os.environ["HOME"]
    clean_env["PATH"] = os.environ["PATH"]

    start = time.time()
    try:
        result = subprocess.run(
            [codex_bin, "exec", "--sandbox", "read-only", prompt_text],
            capture_output=True, text=True, timeout=TIMEOUT_CODEX, env=clean_env,
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
        if (line.startswith("def ") or line.startswith("    ") or
                line.startswith("class ") or line.startswith("import ") or
                line.startswith("from ") or not line.strip()):
            lines.append(line)
        elif lines:
            break
    if lines:
        return "\n".join(lines).strip()
    return text.strip()


def ensure_imports(code):
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
        "decimal": ["Decimal", "ROUND_HALF_UP", "ROUND_HALF_DOWN"],
        "collections": ["defaultdict", "Counter", "deque", "OrderedDict"],
        "functools": ["reduce", "lru_cache", "cache"],
    }
    for module, names in bare_names.items():
        needed = [n for n in names if n in code]
        if not needed:
            continue
        if f"from {module} import" not in code and not any(f"from {module} import" in i for i in imports):
            imports.append(f"from {module} import {', '.join(needed)}")

    if imports:
        return "\n".join(imports) + "\n" + code
    return code


def run_test(solution, test_list, test_imports=None):
    """Run solution against MBPP test assertions."""
    solution = ensure_imports(solution)
    parts = []
    if test_imports:
        parts.extend(test_imports)
    parts.append(solution)
    parts.extend(test_list)
    script = "\n\n".join(parts) + "\n"

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        f.flush()
        try:
            result = subprocess.run(
                [PYTHON_BIN, f.name],
                capture_output=True, text=True, timeout=TIMEOUT_SANDBOX,
                env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
            )
            return result.returncode == 0, result.stderr
        except subprocess.TimeoutExpired:
            return False, "timeout"
        finally:
            os.unlink(f.name)


def run_benchmark(tasks, output_path, existing_results, compressed=False):
    results = list(existing_results.values())
    completed_ids = set(existing_results.keys())
    passed = sum(1 for r in results if r.get("passed"))
    total = len(tasks)
    skipped = 0
    total_orig = 0
    total_comp = 0
    arm = "compressed" if compressed else "control"

    for i, task in enumerate(tasks):
        task_id = task["task_id"]

        if task_id in completed_ids:
            skipped += 1
            continue

        idx = i + 1
        fn_name = infer_function_name(task["test_list"])
        if not fn_name:
            fn_name = "solution"

        print(f"[{idx}/{total}] MBPP/{task_id} ({fn_name})...", end=" ", flush=True)

        compression_pct = 0
        if compressed:
            ctx, orig_len, comp_len = compress_context(task["prompt"], fn_name)
            total_orig += orig_len
            total_comp += comp_len
            compression_pct = round((1 - comp_len / orig_len) * 100, 1) if orig_len else 0
            prompt_text = build_prompt(task["prompt"], fn_name, task["test_list"],
                                       compressed=True, compressed_context=ctx)
        else:
            prompt_text = build_prompt(task["prompt"], fn_name, task["test_list"])

        solution, elapsed = solve_with_codex(prompt_text)

        if not solution:
            status_suffix = f", {compression_pct}% compr" if compressed else ""
            print(f"SKIP ({elapsed:.1f}s{status_suffix})")
            result = {
                "task_id": task_id,
                "fn_name": fn_name,
                "passed": False,
                "error": "codex returned no output",
                "elapsed_s": round(elapsed, 2),
            }
        else:
            ok, stderr = run_test(solution, task["test_list"], task.get("test_imports"))
            status_suffix = f", {compression_pct}% compr" if compressed else ""
            if ok:
                passed += 1
                print(f"PASS ({elapsed:.1f}s{status_suffix})")
            else:
                print(f"FAIL ({elapsed:.1f}s{status_suffix})")
            result = {
                "task_id": task_id,
                "fn_name": fn_name,
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "solution_preview": solution[:200],
            }
            if compressed:
                result["compression_pct"] = compression_pct

        results.append(result)
        if output_path:
            avg_comp = round((1 - total_comp / total_orig) * 100, 1) if total_orig else 0
            save_progress(output_path, results, total, arm, avg_comp)

    if skipped:
        print(f"(Skipped {skipped} already-completed tasks)")

    avg_comp = round((1 - total_comp / total_orig) * 100, 1) if total_orig else 0
    if compressed:
        print(f"\nAvg context compression: {avg_comp}%")

    return {
        "benchmark": "mbpp-sanitized",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "arm": arm,
        "python_version": "3.11",
        "avg_compression_pct": avg_comp,
        "total_tasks": total,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }


def main():
    parser = argparse.ArgumentParser(description="MBPP benchmark via Codex CLI")
    parser.add_argument("--tasks", type=int, default=None)
    parser.add_argument("--output", "-o", type=str, default=None)
    parser.add_argument("--resume", type=str, default=None)
    parser.add_argument("--compressed", action="store_true", help="Run compressed arm")
    args = parser.parse_args()

    if not MBPP_PATH.exists():
        print(f"Error: {MBPP_PATH} not found.", file=sys.stderr)
        sys.exit(1)
    if not Path(PYTHON_BIN).exists():
        print(f"Error: {PYTHON_BIN} not found.", file=sys.stderr)
        sys.exit(1)

    output_path = args.resume or args.output
    existing = {}
    if args.resume:
        existing = load_partial_results(args.resume)
        if existing:
            print(f"Resuming: {len(existing)} tasks already completed")

    tasks = load_mbpp(MBPP_PATH, limit=args.tasks)
    arm_label = "COMPRESSED" if args.compressed else "CONTROL"

    print(f"Running {len(tasks)} MBPP tasks ({arm_label} arm)...")
    remaining = len(tasks) - len(existing)
    print(f"Remaining: {remaining} tasks (~{remaining * 5}s estimated)")
    print(f"Python: {PYTHON_BIN}")
    print()

    report = run_benchmark(tasks, output_path, existing, compressed=args.compressed)

    print()
    print(f"=== Results ({arm_label}) ===")
    print(f"Pass rate: {report['tasks_passed']}/{report['tasks_completed']} "
          f"({report['pass_rate']*100:.1f}%)")

    if output_path:
        save_progress(output_path, report["results"], report["total_tasks"],
                      report["arm"], report["avg_compression_pct"])
        print(f"Report saved to {output_path}")
    else:
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
