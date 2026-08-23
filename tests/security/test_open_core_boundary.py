import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/check-open-core-boundary.py"
SPEC = importlib.util.spec_from_file_location("open_core_boundary", SCRIPT)
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)


class OpenCoreBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        for relative in (
            "rust/crates/lean-ctx-protocol/src/lib.rs",
            "rust/crates/lean-ctx-protocol/src/auto_routing.rs",
            "rust/crates/lean-ctx-protocol/src/control_plane.rs",
            "rust/crates/lean-ctx-protocol/src/eligibility.rs",
            "rust/crates/lean-ctx-protocol/src/fleet_control.rs",
            "rust/crates/lean-ctx-protocol/src/outcome_engine.rs",
            "rust/crates/lean-ctx-protocol/src/rollout.rs",
            "rust/crates/lean-ctx-protocol/src/value_share.rs",
            "rust/src/proxy/mod.rs",
            "rust/src/proxy/auto_routing.rs",
            "rust/src/proxy/rollout.rs",
        ):
            destination = self.repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, destination)
        self.manifest = self.repo / "security/public-protocol-surface-freeze-v1.json"
        self.manifest.parent.mkdir()
        shutil.copy2(ROOT / "security/public-protocol-surface-freeze-v1.json", self.manifest)

    def tearDown(self):
        self.temp.cleanup()

    def write_manifest(self, value):
        self.manifest.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")

    def read_manifest(self):
        return json.loads(self.manifest.read_text(encoding="utf-8"))

    def test_current_tree_passes(self):
        self.assertEqual(GATE.check_repo(ROOT), [])

    def test_fixture_tree_passes_and_approved_local_import_is_not_private(self):
        findings = GATE.check_repo(self.repo)
        self.assertEqual(findings, [])
        self.assertNotIn("private-import", "\n".join(findings))

    def test_missing_manifest_fails_closed(self):
        self.manifest.unlink()
        findings = GATE.check_repo(self.repo)
        self.assertTrue(findings)
        self.assertTrue(findings[0].startswith("[manifest]"))

    def test_malformed_manifest_fails_closed(self):
        self.manifest.write_text('{"schema_version":', encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(findings[0].startswith("[manifest]"))

    def test_manifest_metadata_drift_fails_closed(self):
        manifest = self.read_manifest()
        manifest["surfaces"]["rollout"]["status"] = "stable"
        self.write_manifest(manifest)
        self.assertTrue(any("[manifest]" in finding for finding in GATE.check_repo(self.repo)))

    def test_manifest_rejects_invalid_module_digest(self):
        manifest = self.read_manifest()
        manifest["surfaces"]["rollout"]["module_sha256"] = "not-a-digest"
        self.write_manifest(manifest)
        self.assertTrue(any("[manifest]" in finding for finding in GATE.check_repo(self.repo)))

    def test_public_module_root_drift_is_rejected(self):
        path = self.repo / "rust/src/proxy/mod.rs"
        path.write_text(path.read_text(encoding="utf-8") + "\npub mod value_share;\n", encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("value_share module_roots drift" in finding for finding in findings))

    def test_root_reexport_drift_is_rejected(self):
        path = self.repo / "rust/src/proxy/rollout.rs"
        source = path.read_text(encoding="utf-8").replace("RolloutConfig, assign_rollout", "RolloutConfig")
        path.write_text(source, encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("rollout root_reexports drift" in finding for finding in findings))

    def test_new_export_is_rejected(self):
        path = self.repo / "rust/crates/lean-ctx-protocol/src/control_plane.rs"
        path.write_text(path.read_text(encoding="utf-8") + "\npub struct UnreviewedControlPlaneExport;\n", encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("control_plane exported symbols drift" in finding for finding in findings))

    def test_existing_public_wire_shape_drift_is_rejected(self):
        path = self.repo / "rust/crates/lean-ctx-protocol/src/control_plane.rs"
        source = path.read_text(encoding="utf-8").replace(
            "    pub available_capabilities: Vec<CapabilityManifestV1>,\n",
            "    pub available_capabilities: Vec<CapabilityManifestV1>,\n"
            "    pub unreviewed_field: bool,\n",
        )
        path.write_text(source, encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("control_plane module digest drift" in finding for finding in findings))

    def test_new_consumer_is_rejected(self):
        consumer = self.repo / "rust/src/unreviewed_consumer.rs"
        consumer.write_text(
            "use lean_ctx_protocol::control_plane::ControlPlaneRequest;\n",
            encoding="utf-8",
        )
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("[new-consumer] control_plane" in finding for finding in findings))
        self.assertFalse(any("[private-import]" in finding for finding in findings))

    def assert_new_consumer(self, filename, source, surface):
        consumer = self.repo / "rust/src" / filename
        consumer.write_text(source, encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("[new-consumer] %s" % surface in finding for finding in findings), findings)

    def test_same_crate_root_symbol_import_is_rejected(self):
        self.assert_new_consumer(
            "root_symbol_consumer.rs",
            "use crate::{ControlPlaneRequest};\n",
            "control_plane",
        )

    def test_same_crate_module_symbol_import_is_rejected(self):
        self.assert_new_consumer(
            "module_symbol_consumer.rs",
            "use crate::control_plane::ControlPlaneRequest;\n",
            "control_plane",
        )

    def test_fully_qualified_root_symbol_reference_is_rejected(self):
        self.assert_new_consumer(
            "qualified_consumer.rs",
            "fn consume(value: crate::ControlPlaneRequest) { let _ = value; }\n",
            "control_plane",
        )

    def test_external_fully_qualified_root_symbol_reference_is_rejected(self):
        self.assert_new_consumer(
            "external_qualified_consumer.rs",
            "fn consume(value: lean_ctx_protocol::ControlPlaneRequest) { let _ = value; }\n",
            "control_plane",
        )

    def test_external_root_symbol_import_is_rejected(self):
        self.assert_new_consumer(
            "external_root_consumer.rs",
            "use lean_ctx_protocol::ControlPlaneRequest;\n",
            "control_plane",
        )

    def test_relevant_glob_imports_are_rejected(self):
        consumer = self.repo / "rust/src/glob_consumer.rs"
        consumer.write_text(
            "use crate::*;\nuse crate::control_plane::*;\n"
            "use lean_ctx_protocol::control_plane::*;\n",
            encoding="utf-8",
        )
        findings = GATE.check_repo(self.repo)
        for surface in ("control_plane", "fleet_control", "value_share"):
            self.assertTrue(any("[new-consumer] %s" % surface in finding for finding in findings), findings)

    def test_external_crate_alias_is_rejected(self):
        consumer = self.repo / "rust/src/alias_consumer.rs"
        consumer.write_text("use lean_ctx_protocol as protocol;\n", encoding="utf-8")
        findings = GATE.check_repo(self.repo)
        for surface in GATE.FROZEN_SURFACES:
            self.assertTrue(any("[new-consumer] %s" % surface in finding for finding in findings), findings)

    def test_same_crate_alias_and_extern_crate_are_rejected(self):
        consumer = self.repo / "rust/crates/lean-ctx-protocol/src/alias_consumer.rs"
        consumer.write_text(
            "use crate as protocol;\nextern crate lean_ctx_protocol as external;\n",
            encoding="utf-8",
        )
        findings = GATE.check_repo(self.repo)
        for surface in GATE.FROZEN_SURFACES:
            self.assertTrue(any("[new-consumer] %s" % surface in finding for finding in findings), findings)

    def test_comments_and_literals_are_not_consumers(self):
        consumer = self.repo / "rust/src/non_consumer_text.rs"
        consumer.write_text(
            "// crate::ControlPlaneRequest\n"
            "const TEXT: &str = \"lean_ctx_protocol::ControlPlaneRequest\";\n"
            "const RAW: &str = r#\"use crate::control_plane::ControlPlaneRequest;\"#;\n",
            encoding="utf-8",
        )
        self.assertEqual(GATE.check_repo(self.repo), [])

    def test_surface_module_references_are_not_consumers(self):
        module = self.repo / "rust/crates/lean-ctx-protocol/src/control_plane.rs"
        module.write_text(
            module.read_text(encoding="utf-8")
            + "\nfn local_check(value: crate::ControlPlaneRequest) { let _ = value; }\n",
            encoding="utf-8",
        )
        findings = GATE.check_repo(self.repo)
        self.assertFalse(any("[new-consumer] control_plane" in finding for finding in findings))
        self.assertTrue(any("control_plane module digest drift" in finding for finding in findings))

    def test_private_cloud_import_remains_distinct(self):
        consumer = self.repo / "rust/src/private_consumer.rs"
        consumer.write_text(
            "use lean_ctx_enterprise::control_plane::ControlPlaneRequest;\n",
            encoding="utf-8",
        )
        findings = GATE.check_repo(self.repo)
        self.assertTrue(any("[private-import]" in finding for finding in findings))

    def test_cli_json_output_is_deterministic(self):
        command = [sys.executable, str(SCRIPT), "--root", str(self.repo), "--json"]
        first = subprocess.run(command, capture_output=True, check=False)
        second = subprocess.run(command, capture_output=True, check=False)
        self.assertEqual(first.returncode, 0)
        self.assertEqual(first.stdout, second.stdout)
        self.assertEqual(first.stderr, second.stderr)
        self.assertEqual(json.loads(first.stdout)["status"], "pass")


if __name__ == "__main__":
    unittest.main()
