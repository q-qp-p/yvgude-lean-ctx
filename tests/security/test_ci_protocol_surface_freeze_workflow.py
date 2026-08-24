import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"


class ProtocolSurfaceFreezeWorkflowTests(unittest.TestCase):
    def test_gate_is_mandatory_in_delivery_security_job(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        command = "python3 scripts/check-open-core-boundary.py"
        self.assertEqual(workflow.count(command), 1)

        job_start = workflow.index("  delivery-security-gate:")
        job_end = workflow.index("\n  ci-green:", job_start)
        job = workflow[job_start:job_end]
        gate_start = job.index("      - name: Verify public protocol surface freeze")
        test_start = job.index("      - name: Run delivery, security, and OCLA contract tests")
        history_start = job.index("      - name: Verify audited public-tree delta")
        self.assertLess(gate_start, test_start)
        self.assertLess(gate_start, history_start)

        gate = job[gate_start:test_start]
        self.assertIn(f"        run: {command}\n", gate)
        self.assertNotIn("continue-on-error", gate)


if __name__ == "__main__":
    unittest.main()
