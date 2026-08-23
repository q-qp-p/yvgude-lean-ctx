import unittest
from pathlib import Path


WORKFLOW = (
    Path(__file__).resolve().parents[2] / ".github/workflows/security-check.yml"
)


class SecurityCheckWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text()

    def test_critical_pattern_detections_fail_closed(self):
        for pattern in (
            r'\.env("LD_PRELOAD")',
            r'\.env("DYLD_',
            r'sk_live_\|sk_test_\|AKIA[0-9A-Z]\|ghp_[a-zA-Z0-9]',
        ):
            start = self.workflow.index(pattern)
            end = self.workflow.index("          fi", start)
            self.assertIn("FATAL=1", self.workflow[start:end])

        guard_start = self.workflow.index('if [ "$FATAL" -eq 1 ]; then')
        guard_end = self.workflow.index("          fi", guard_start)
        guard = self.workflow[guard_start:guard_end]
        self.assertIn('echo "::error::Security pattern scan failed"', guard)
        self.assertIn("            exit 1", guard)

    def test_protocol_surface_freeze_gate_is_mandatory_before_dependency_audit(self):
        command = "python3 scripts/check-open-core-boundary.py"
        self.assertEqual(self.workflow.count(command), 1)

        gate_start = self.workflow.index("      - name: Protocol surface freeze gate")
        audit_start = self.workflow.index("      - name: Dependency audit")
        self.assertLess(gate_start, audit_start)

        gate = self.workflow[gate_start:audit_start]
        self.assertIn(f"        run: {command}\n", gate)
        self.assertNotIn("continue-on-error", gate)


if __name__ == "__main__":
    unittest.main()
