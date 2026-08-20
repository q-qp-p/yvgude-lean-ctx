#!/usr/bin/env python3
"""Compressed-arm benchmark: simulates realistic agent context.

Instead of sending bare HumanEval prompts, this embeds each prompt in a
realistic Python module context (imports, docstring, surrounding functions),
then reads it through lean-ctx compression before sending to the LLM.

This simulates the actual agent workflow: read file → lean-ctx compresses → LLM generates code.
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
LEAN_CTX_BIN = os.path.expanduser("~/.local/bin/lean-ctx")

SURROUNDING_MODULE = '''"""Utility module for algorithmic operations.

This module provides various helper functions for common programming tasks
including string manipulation, mathematical operations, list processing,
and data transformations. All functions include comprehensive type hints
and documentation.

Author: Engineering Team
Version: 2.4.1
License: MIT
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
    """Validate that input matches expected type."""
    if not isinstance(data, expected_type):
        raise TypeError(f"Expected {{expected_type.__name__}}, got {{type(data).__name__}}")
    return True


def safe_divide(a: float, b: float, default: float = 0.0) -> float:
    """Safe division with zero-division protection."""
    return a / b if b != 0 else default


def flatten_list(nested: List) -> List:
    """Flatten arbitrarily nested lists."""
    result = []
    for item in nested:
        if isinstance(item, list):
            result.extend(flatten_list(item))
        else:
            result.append(item)
    return result


def memoize(func):
    """Simple memoization decorator."""
    cache = {{}}
    def wrapper(*args):
        if args not in cache:
            cache[args] = func(*args)
        return cache[args]
    return wrapper


class DataProcessor:
    """Generic data processor with pipeline support."""

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

{prompt}
'''


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
    if not Path(path).exists():
        return {}
    with open(path) as f:
        data = json.load(f)
    return {r["task_id"]: r for r in data.get("results", [])}


def save_progress(output_path, results, total_tasks):
    passed = sum(1 for r in results if r.get("passed"))
    report = {
        "benchmark": "humaneval",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "arm": "compressed",
        "python_version": "3.11",
        "total_tasks": total_tasks,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }
    Path(output_path).write_text(json.dumps(report, indent=2))


def compress_context(prompt):
    """Compress SURROUNDING context only, keep target prompt intact.

    Simulates real agent workflow: lean-ctx compresses the file the agent reads,
    but the target function (prompt) is shown in full. The savings come from
    compressing the surrounding module boilerplate.
    """
    surrounding_only = SURROUNDING_MODULE.replace("{prompt}", "# (target function omitted)")

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(surrounding_only)
        f.flush()
        try:
            clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
            clean_env["HOME"] = os.environ["HOME"]
            clean_env["PATH"] = os.environ["PATH"]
            result = subprocess.run(
                [LEAN_CTX_BIN, "read", f.name, "-m", "map"],
                capture_output=True,
                text=True,
                timeout=10,
                env=clean_env,
            )
            compressed_ctx = result.stdout.strip()
        except Exception:
            compressed_ctx = surrounding_only
        finally:
            os.unlink(f.name)

    full_content = SURROUNDING_MODULE.format(prompt=prompt)
    combined = f"{compressed_ctx}\n\n# === TARGET FUNCTION (full) ===\n\n{prompt}"
    return combined, len(full_content), len(combined)


def solve_with_codex(compressed_context, entry_point):
    """Send compressed context to Codex CLI for code generation."""
    full_prompt = (
        f"Below is a Python module (possibly in compressed/summarized form). "
        f"Write ONLY the complete implementation for the function `{entry_point}`. "
        f"No explanations, no tests, no markdown fences, just the raw Python code. "
        f"Include all necessary imports at the top.\n\n"
        f"--- Module Context ---\n{compressed_context}\n--- End Context ---\n\n"
        f"Implement `{entry_point}`:"
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
        if f"from {module} import" not in code and not any(f"from {module} import" in imp for imp in imports):
            imports.append(f"from {module} import {', '.join(needed)}")

    if imports:
        return "\n".join(imports) + "\n" + code
    return code


def extract_helper_functions(prompt, entry_point):
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


def run_test(solution, test_code, entry_point, helpers=""):
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
    results = list(existing_results.values())
    completed_ids = set(existing_results.keys())
    passed = sum(1 for r in results if r.get("passed"))
    total = len(tasks)
    skipped = 0
    total_orig_chars = 0
    total_comp_chars = 0

    for i, task in enumerate(tasks):
        task_id = task["task_id"]

        if task_id in completed_ids:
            skipped += 1
            continue

        idx = i + 1
        print(f"[{idx}/{total}] {task_id}...", end=" ", flush=True)

        compressed_context, orig_len, comp_len = compress_context(task["prompt"])
        total_orig_chars += orig_len
        total_comp_chars += comp_len
        compression_pct = round((1 - comp_len / orig_len) * 100, 1) if orig_len > 0 else 0

        helpers = extract_helper_functions(task["prompt"], task["entry_point"])
        solution, elapsed = solve_with_codex(compressed_context, task["entry_point"])

        if not solution:
            print(f"SKIP ({compression_pct}% compressed)")
            result = {
                "task_id": task_id,
                "passed": False,
                "error": "codex returned no output",
                "elapsed_s": round(elapsed, 2),
                "compression_pct": compression_pct,
            }
        else:
            ok, stderr = run_test(solution, task["test"], task["entry_point"], helpers)
            if ok:
                passed += 1
                print(f"PASS ({elapsed:.1f}s, {compression_pct}% compressed)")
            else:
                print(f"FAIL ({elapsed:.1f}s, {compression_pct}% compressed)")
            result = {
                "task_id": task_id,
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "solution_preview": solution[:200],
                "compression_pct": compression_pct,
            }

        results.append(result)
        if output_path:
            save_progress(output_path, results, total)

    if skipped:
        print(f"(Skipped {skipped} already-completed tasks)")

    avg_compression = round((1 - total_comp_chars / total_orig_chars) * 100, 1) if total_orig_chars else 0
    print(f"\nAvg context compression: {avg_compression}%")

    return {
        "benchmark": "humaneval",
        "engine": "codex-cli",
        "model": "gpt-5.6-terra (via ChatGPT subscription)",
        "arm": "compressed",
        "python_version": "3.11",
        "avg_compression_pct": avg_compression,
        "total_tasks": total,
        "tasks_completed": len(results),
        "tasks_passed": passed,
        "pass_rate": round(passed / len(results), 4) if results else 0,
        "results": results,
    }


def main():
    parser = argparse.ArgumentParser(description="HumanEval benchmark (compressed arm)")
    parser.add_argument("--tasks", type=int, default=None)
    parser.add_argument("--output", "-o", type=str, default=None)
    parser.add_argument("--resume", type=str, default=None)
    args = parser.parse_args()

    if not HUMANEVAL_PATH.exists():
        print(f"Error: {HUMANEVAL_PATH} not found.", file=sys.stderr)
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

    print(f"Running {len(tasks)} HumanEval tasks (COMPRESSED arm)...")
    remaining = len(tasks) - len(existing)
    print(f"Remaining: {remaining} tasks (~{remaining * 6}s estimated)")
    print(f"Python: {PYTHON_BIN}")
    print()

    report = run_benchmark(tasks, output_path, existing)

    print()
    print(f"=== Results (Compressed Arm) ===")
    print(f"Pass rate: {report['tasks_passed']}/{report['tasks_completed']} ({report['pass_rate']*100:.1f}%)")

    if output_path:
        print(f"Report saved to {output_path}")
    else:
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
