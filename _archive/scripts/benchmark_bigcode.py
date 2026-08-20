#!/usr/bin/env python3
"""BigCodeBench-Instruct benchmark via Codex CLI.

Tests code generation with optional lean-ctx context compression.

Usage:
    python3 scripts/benchmark_bigcode.py --tasks 100 --output baseline.json
    python3 scripts/benchmark_bigcode.py --tasks 100 --compressed --output compressed.json
    python3 scripts/benchmark_bigcode.py --resume partial.json
"""

import argparse
import ast
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

DATA_DIR = Path.home() / "Library" / "Application Support" / "lean-ctx" / "benchmark-data"
BIGCODE_PATH = DATA_DIR / "bigcodebench.jsonl"
TIMEOUT_CODEX = 120
TIMEOUT_SANDBOX = 30
PYTHON_BIN = "/opt/homebrew/bin/python3.11"
LEAN_CTX_BIN = os.path.expanduser("~/.local/bin/lean-ctx")

AVAILABLE_LIBS = {
    "numpy",
    "pandas",
    "matplotlib",
    "scipy",
    "sklearn",
    "scikit-learn",
    "seaborn",
    "re",
    "os",
    "sys",
    "json",
    "csv",
    "math",
    "random",
    "collections",
    "itertools",
    "functools",
    "datetime",
    "time",
    "string",
    "textwrap",
    "hashlib",
    "base64",
    "urllib",
    "pathlib",
    "io",
    "struct",
    "typing",
    "copy",
    "operator",
    "statistics",
    "heapq",
    "bisect",
    "decimal",
    "fractions",
    "html",
    "xml",
    "sqlite3",
    "tempfile",
    "pickle",
    "gzip",
    "zipfile",
    "shutil",
    "glob",
    "fnmatch",
    "difflib",
    "unicodedata",
    "calendar",
    "abc",
    "contextlib",
    "logging",
    "warnings",
    "traceback",
    "unittest",
    "pytest",
    "sympy",
    "PIL",
    "Pillow",
}

SURROUNDING_MODULE = '''
import os
import sys
import json
from typing import Optional, List, Dict, Any
from dataclasses import dataclass, field

@dataclass
class DataProcessor:
    name: str
    version: str = "1.0.0"
    config: Dict[str, Any] = field(default_factory=dict)
    _cache: Dict[str, Any] = field(default_factory=dict, repr=False)

    def process(self, data: Any) -> Any:
        """Process data according to config."""
        if self.config.get("normalize"):
            data = self._normalize(data)
        if self.config.get("validate"):
            self._validate(data)
        return data

    def _normalize(self, data: Any) -> Any:
        if isinstance(data, str):
            return data.strip().lower()
        if isinstance(data, list):
            return [self._normalize(item) for item in data]
        return data

    def _validate(self, data: Any) -> None:
        if data is None:
            raise ValueError("Data cannot be None")
        if isinstance(data, (list, dict)) and len(data) == 0:
            raise ValueError("Data cannot be empty")

def load_data(path: str, format: str = "json") -> Any:
    """Load data from file in specified format."""
    with open(path) as f:
        if format == "json":
            return json.load(f)
        elif format == "csv":
            import csv
            return list(csv.DictReader(f))
        elif format == "lines":
            return f.read().splitlines()
        raise ValueError(f"Unknown format: {format}")

def save_data(data: Any, path: str, format: str = "json") -> None:
    """Save data to file in specified format."""
    with open(path, "w") as f:
        if format == "json":
            json.dump(data, f, indent=2)
        elif format == "lines":
            f.write("\\n".join(str(item) for item in data))
        else:
            raise ValueError(f"Unknown format: {format}")

class ConfigManager:
    """Manages application configuration with defaults and overrides."""
    def __init__(self, defaults: Optional[Dict] = None):
        self._config = defaults or {}
        self._overrides: Dict[str, Any] = {}

    def get(self, key: str, default: Any = None) -> Any:
        return self._overrides.get(key, self._config.get(key, default))

    def set(self, key: str, value: Any) -> None:
        self._overrides[key] = value

    def reset(self) -> None:
        self._overrides.clear()
'''


def libs_available(libs_field):
    """Return True if all required libs are in AVAILABLE_LIBS."""
    try:
        libs = ast.literal_eval(libs_field)
    except (ValueError, SyntaxError):
        return False
    return all(lib in AVAILABLE_LIBS for lib in libs)


def load_tasks(path, limit=None):
    """Load BigCodeBench tasks filtered by available libraries."""
    tasks = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            task = json.loads(line)
            if not libs_available(task.get("libs", "[]")):
                continue
            tasks.append(task)
            if limit and len(tasks) >= limit:
                break
    return tasks


def load_partial_results(path):
    if not Path(path).exists():
        return {}
    with open(path) as f:
        data = json.load(f)
    return {r["task_id"]: r for r in data.get("results", [])}


def save_progress(output_path, results, total_tasks, arm, avg_compression=0):
    passed = sum(1 for r in results if r.get("passed"))
    report = {
        "benchmark": "bigcodebench-instruct",
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
    Path(output_path).write_text(json.dumps(report, indent=2) + "\n")


def compress_context(prompt):
    """Compress only the surrounding module, keep task prompt in full."""
    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(SURROUNDING_MODULE)
        f.flush()
        try:
            result = subprocess.run(
                [LEAN_CTX_BIN, "read", f.name, "-m", "map"],
                capture_output=True,
                text=True,
                timeout=10,
                env={k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")},
            )
            compressed_module = result.stdout.strip()
        except Exception:
            compressed_module = SURROUNDING_MODULE
        finally:
            os.unlink(f.name)

    compression_pct = round((1 - len(compressed_module) / len(SURROUNDING_MODULE)) * 100, 1)
    combined = f"# Surrounding module context:\n{compressed_module}\n\n# Your task:\n{prompt}"
    return combined, compression_pct


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


def ensure_imports(code):
    imports = []
    typing_names = ["List", "Tuple", "Dict", "Set", "Optional", "Any", "Union"]
    used = [t for t in typing_names if t in code]
    if used and "from typing import" not in code and "import typing" not in code:
        imports.append(f"from typing import {', '.join(used)}")

    stdlib_modules = [
        "math",
        "re",
        "collections",
        "itertools",
        "functools",
        "string",
        "heapq",
        "bisect",
        "operator",
        "decimal",
        "fractions",
        "hashlib",
        "copy",
        "sys",
        "random",
        "unittest",
    ]
    for mod in stdlib_modules:
        if f"{mod}." in code and f"import {mod}" not in code:
            imports.append(f"import {mod}")

    if imports:
        return "\n".join(imports) + "\n" + code
    return code


def run_test(solution, test_code):
    """Run solution against BigCodeBench unittest tests."""
    solution = ensure_imports(solution)
    script = f"{solution}\n\n{test_code}\n"

    test_env = {
        **os.environ,
        "PYTHONDONTWRITEBYTECODE": "1",
        "MPLBACKEND": "Agg",
    }

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        f.flush()
        path = f.name
        try:
            pytest_result = subprocess.run(
                [PYTHON_BIN, "-m", "pytest", path, "-x", "--tb=short"],
                capture_output=True,
                text=True,
                timeout=TIMEOUT_SANDBOX,
                env=test_env,
            )
            if pytest_result.returncode == 0:
                return True, pytest_result.stderr

            fallback_script = script
            if "unittest.main()" not in script:
                fallback_script += "\nif __name__ == '__main__':\n    import unittest\n    unittest.main()\n"

            with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as fb:
                fb.write(fallback_script)
                fb.flush()
                fb_path = fb.name

            try:
                py_result = subprocess.run(
                    [PYTHON_BIN, fb_path],
                    capture_output=True,
                    text=True,
                    timeout=TIMEOUT_SANDBOX,
                    env=test_env,
                )
                ok = py_result.returncode == 0
                err = py_result.stderr or pytest_result.stderr
                return ok, err
            finally:
                os.unlink(fb_path)
        except subprocess.TimeoutExpired:
            return False, "timeout"
        finally:
            os.unlink(path)


def run_benchmark(tasks, output_path, existing_results, compressed=False):
    results = list(existing_results.values())
    completed_ids = set(existing_results.keys())
    passed = sum(1 for r in results if r.get("passed"))
    total = len(tasks)
    skipped = 0
    compression_pcts = []
    arm = "compressed" if compressed else "control"

    for i, task in enumerate(tasks):
        task_id = task["task_id"]
        if task_id in completed_ids:
            skipped += 1
            continue

        idx = i + 1
        prompt = task["instruct_prompt"]
        compression_pct = 0

        if compressed:
            prompt, compression_pct = compress_context(task["instruct_prompt"])
            compression_pcts.append(compression_pct)

        sfx = f" ({compression_pct}% compr)" if compressed else ""
        print(f"[{idx}/{total}] {task_id}{sfx}...", end=" ", flush=True)

        solution, elapsed = solve_with_codex(prompt)

        if not solution:
            print(f"SKIP ({elapsed:.1f}s)")
            result = {
                "task_id": task_id,
                "entry_point": task.get("entry_point"),
                "passed": False,
                "error": "codex returned no output",
                "elapsed_s": round(elapsed, 2),
                "compression_pct": compression_pct,
            }
        else:
            ok, stderr = run_test(solution, task["test"])
            if ok:
                passed += 1
                print(f"PASS ({elapsed:.1f}s)")
            else:
                print(f"FAIL ({elapsed:.1f}s)")
            result = {
                "task_id": task_id,
                "entry_point": task.get("entry_point"),
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "compression_pct": compression_pct,
                "solution_preview": solution[:200],
            }

        results.append(result)
        if output_path:
            avg_comp = round(sum(compression_pcts) / len(compression_pcts), 1) if compression_pcts else 0
            save_progress(output_path, results, total, arm, avg_comp)

    if skipped:
        print(f"(Skipped {skipped} already-completed tasks)")

    avg_comp = round(sum(compression_pcts) / len(compression_pcts), 1) if compression_pcts else 0
    if compressed and compression_pcts:
        print(f"\nAvg context compression: {avg_comp}%")

    return {
        "benchmark": "bigcodebench-instruct",
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
    parser = argparse.ArgumentParser(description="BigCodeBench-Instruct benchmark via Codex CLI")
    parser.add_argument("--tasks", type=int, default=None, help="Limit number of runnable tasks")
    parser.add_argument("--output", "-o", type=str, default=None, help="Output JSON path")
    parser.add_argument("--resume", type=str, default=None, help="Resume from partial result file")
    parser.add_argument("--compressed", action="store_true", help="Run compressed arm")
    args = parser.parse_args()

    if not BIGCODE_PATH.exists():
        print(f"Error: {BIGCODE_PATH} not found.", file=sys.stderr)
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

    tasks = load_tasks(BIGCODE_PATH, limit=args.tasks)
    arm_label = "COMPRESSED" if args.compressed else "CONTROL"

    print(f"=== BigCodeBench-Instruct Benchmark ({arm_label}) ===")
    print(f"Running {len(tasks)} tasks (filtered by available libs)...")
    remaining = len(tasks) - len(existing)
    print(f"Remaining: {remaining} tasks (~{remaining * 8}s estimated)")
    print(f"Python: {PYTHON_BIN}")
    print()

    report = run_benchmark(tasks, output_path, existing, compressed=args.compressed)

    print()
    print(f"=== Results ({arm_label}) ===")
    print(
        f"Pass rate: {report['tasks_passed']}/{report['tasks_completed']} "
        f"({report['pass_rate'] * 100:.1f}%)"
    )
    if args.compressed:
        print(f"Avg compression: {report['avg_compression_pct']}%")

    if output_path:
        save_progress(
            output_path,
            report["results"],
            report["total_tasks"],
            report["arm"],
            report["avg_compression_pct"],
        )
        print(f"Report saved to {output_path}")
    else:
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
