#!/usr/bin/env python3
"""Multi-File Context benchmark: tests whether LLMs can correctly
implement code that depends on understanding multiple files.

Each task presents 2-4 file contexts (raw or lean-ctx compressed) and asks
the LLM to implement a function that requires cross-file dependencies.

Usage:
    python3 scripts/benchmark_multifile.py --output report.json
    python3 scripts/benchmark_multifile.py --tasks 3 --output quick.json
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

TIMEOUT_CODEX = 120
TIMEOUT_SANDBOX = 30
PYTHON_BIN = "/opt/homebrew/bin/python3.11"
LEAN_CTX_BIN = os.path.expanduser("~/.local/bin/lean-ctx")


def load_tasks():
    data_path = Path(__file__).parent / "multifile_benchmark_tasks.json"
    with open(data_path) as f:
        payload = json.load(f)
    return payload["tasks"]


def compress_file(content, filename):
    """Write content to a tempfile and compress via lean-ctx map mode."""
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith("LEAN_CTX")}
    clean_env["HOME"] = os.environ["HOME"]
    clean_env["PATH"] = os.environ["PATH"]

    suffix = Path(filename).suffix or ".py"
    with tempfile.NamedTemporaryFile(mode="w", suffix=suffix, delete=False) as f:
        f.write(content)
        f.flush()
        try:
            result = subprocess.run(
                [LEAN_CTX_BIN, "read", f.name, "-m", "map"],
                capture_output=True,
                text=True,
                timeout=10,
                env=clean_env,
            )
            return result.stdout.strip()
        except Exception:
            return content
        finally:
            os.unlink(f.name)


def strip_local_imports(code, file_names):
    """Remove import statements that reference the shown project files."""
    module_names = {fn.replace(".py", "") for fn in file_names}
    lines = []
    for line in code.split("\n"):
        skip = False
        for mod in module_names:
            if f"import {mod}" in line or f"from {mod}" in line:
                skip = True
                break
        if not skip:
            lines.append(line)
    return "\n".join(lines)


def build_prompt(task, compressed=False):
    """Assemble file contexts and the task question."""
    file_sections = []
    total_orig = 0
    total_comp = 0

    for filename, content in task["files"].items():
        total_orig += len(content)
        if compressed:
            comp = compress_file(content, filename)
            total_comp += len(comp)
            file_sections.append(f"--- {filename} (compressed) ---\n{comp}")
        else:
            total_comp += len(content)
            file_sections.append(f"--- {filename} ---\n{content}")

    files_text = "\n\n".join(file_sections)
    compression_pct = round((1 - total_comp / total_orig) * 100, 1) if total_orig else 0

    prompt = (
        f"You have access to the following project files:\n\n"
        f"{files_text}\n\n"
        f"{task['question']}\n\n"
        f"Write ONLY the Python function(s) requested. No explanations, no tests, "
        f"no markdown fences. Include stdlib imports only — do NOT import the "
        f"project modules shown above (they will be concatenated with your code)."
    )
    return prompt, compression_pct, total_orig, total_comp


def solve_with_codex(prompt_text):
    """Call codex exec and return (code, elapsed_seconds)."""
    codex_bin = os.environ.get("CODEX_BIN", os.path.expanduser("~/.local/bin/codex"))
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


def run_test(solution, test_code, file_contents):
    """Concatenate original files + solution + test and execute."""
    file_names = list(file_contents.keys())
    solution = strip_local_imports(solution, file_names)
    test_code = strip_local_imports(test_code, file_names)
    parts = [strip_local_imports(v, file_names) for v in file_contents.values()]
    parts.append(solution)
    parts.append(test_code)
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
            ok = result.returncode == 0 and "ALL TESTS PASSED" in result.stdout
            return ok, result.stderr
        except subprocess.TimeoutExpired:
            return False, "timeout"
        finally:
            os.unlink(f.name)


def summarize_by_complexity(tasks, results):
    by_complexity = {}
    result_map = {r["task_id"]: r for r in results}

    for task in tasks:
        complexity = task.get("complexity", "unknown")
        bucket = by_complexity.setdefault(
            complexity,
            {"total": 0, "passed": 0, "control": 0, "compressed": 0},
        )
        bucket["total"] += 1
        if result_map.get(task["id"], {}).get("passed"):
            bucket["passed"] += 1

    return by_complexity


def merge_complexity_rates(by_ctrl, by_comp):
    merged = {}
    for complexity in sorted(set(by_ctrl) | set(by_comp)):
        ctrl = by_ctrl.get(complexity, {"total": 0, "passed": 0})
        comp = by_comp.get(complexity, {"total": 0, "passed": 0})
        total = ctrl["total"]
        merged[complexity] = {
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

    for task in tasks:
        tid = task["id"]
        complexity = task.get("complexity", "unknown")
        prompt, comp_pct, orig_len, comp_len = build_prompt(task, compressed=compressed)

        if compressed:
            total_orig += orig_len
            total_comp += comp_len

        sfx = f" ({comp_pct}% compressed)" if compressed else ""
        print(f"  {tid}{sfx}...", end=" ", flush=True)

        solution, elapsed = solve_with_codex(prompt)

        if not solution:
            print(f"SKIP ({elapsed:.1f}s)")
            results.append(
                {
                    "task_id": tid,
                    "complexity": complexity,
                    "passed": False,
                    "error": "codex returned no output",
                    "elapsed_s": round(elapsed, 2),
                    "compression_pct": comp_pct if compressed else 0,
                }
            )
            continue

        ok, stderr = run_test(solution, task["test"], task["files"])
        if ok:
            passed += 1
            print(f"PASS ({elapsed:.1f}s)")
        else:
            print(f"FAIL ({elapsed:.1f}s)")

        results.append(
            {
                "task_id": tid,
                "complexity": complexity,
                "passed": ok,
                "error": None if ok else (stderr[:500] if stderr else "failed"),
                "elapsed_s": round(elapsed, 2),
                "compression_pct": comp_pct if compressed else 0,
            }
        )

    avg_comp = round((1 - total_comp / total_orig) * 100, 1) if total_orig else 0
    return results, passed, avg_comp


def main():
    parser = argparse.ArgumentParser(description="Multi-file context benchmark")
    parser.add_argument("--output", "-o", type=str, default=None)
    parser.add_argument("--tasks", type=int, default=None, help="Limit number of tasks")
    args = parser.parse_args()

    if not Path(PYTHON_BIN).exists():
        print(f"Error: {PYTHON_BIN} not found.", file=sys.stderr)
        sys.exit(1)

    all_tasks = load_tasks()
    tasks = all_tasks[: args.tasks] if args.tasks else all_tasks
    total = len(tasks)

    print(f"=== Multi-File Context Benchmark v2 ({total} tasks) ===\n")

    print("--- Control (full files) ---")
    ctrl_results, ctrl_passed, _ = run_benchmark(tasks, compressed=False)
    print(f"Control: {ctrl_passed}/{total} ({ctrl_passed / total * 100:.1f}%)\n")

    print("--- Compressed (lean-ctx map mode) ---")
    comp_results, comp_passed, avg_comp = run_benchmark(tasks, compressed=True)
    print(f"Compressed: {comp_passed}/{total} ({comp_passed / total * 100:.1f}%)")
    print(f"Avg compression: {avg_comp}%\n")

    by_complexity = merge_complexity_rates(
        summarize_by_complexity(tasks, ctrl_results),
        summarize_by_complexity(tasks, comp_results),
    )

    report = {
        "benchmark": "multifile-context-v2",
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
        "by_complexity": by_complexity,
    }

    print("=== Summary ===")
    print(f"Control:    {ctrl_passed}/{total} ({ctrl_passed / total * 100:.1f}%)")
    print(f"Compressed: {comp_passed}/{total} ({comp_passed / total * 100:.1f}%)")
    print(f"Delta:      {report['delta_pp']:+.1f}pp")
    print("\nBy complexity:")
    for complexity, stats in by_complexity.items():
        print(
            f"  {complexity:8s} control={stats['control']}/{stats['total']} "
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
