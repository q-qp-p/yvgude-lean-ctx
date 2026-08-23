"""Regression coverage for public narrative-governance enforcement."""

from __future__ import annotations

import io
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path("scripts/check-narrative-governance.py")


class NarrativeGovernanceTests(unittest.TestCase):
    def test_clean_archive_contains_everything_the_guard_requires(self) -> None:
        tree = subprocess.check_output(["git", "write-tree"], cwd=ROOT, text=True).strip()
        archive = subprocess.check_output(
            ["git", "archive", "--format=tar", tree], cwd=ROOT
        )

        with tempfile.TemporaryDirectory() as temporary_directory:
            archive_root = Path(temporary_directory).resolve()
            with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
                for member in tar.getmembers():
                    destination = (archive_root / member.name).resolve()
                    self.assertFalse(member.issym() or member.islnk())
                    self.assertTrue(
                        destination == archive_root or archive_root in destination.parents
                    )
                tar.extractall(archive_root)

            result = subprocess.run(
                ["python3", str(SCRIPT)],
                cwd=archive_root,
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Narrative governance passed.", result.stdout)


if __name__ == "__main__":
    unittest.main()
