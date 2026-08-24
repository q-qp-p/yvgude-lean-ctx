//! Clean-checkout proof for the committed, provider-free customer-proof V2 fixture.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COPY_INDEX: AtomicUsize = AtomicUsize::new(0);

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/provider-free-v2")
}

fn run_verifier(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_leanctx-verify"))
        .args([
            "v2",
            root.join("customer-proof.json").to_str().unwrap(),
            "--trust-store",
            root.join("trust-store.json").to_str().unwrap(),
            "--artifact-root",
            root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("standalone verifier binary must execute")
}

fn collect_files(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let file_type = entry.file_type().expect("read fixture entry type");
        assert!(!file_type.is_symlink(), "fixture must not contain symlinks");
        if file_type.is_dir() {
            collect_files(root, &entry.path(), files);
        } else {
            files.insert(
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("fixture entry stays below root")
                    .to_path_buf(),
            );
        }
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture copy directory");
    for entry in fs::read_dir(source).expect("read fixture source") {
        let entry = entry.expect("read fixture source entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("read source entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

struct FixtureCopy(PathBuf);

impl FixtureCopy {
    fn new() -> Self {
        let index = COPY_INDEX.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "leanctx-provider-free-v2-{}-{index}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        copy_tree(&fixture_root(), &root);
        Self(root)
    }
}

impl Drop for FixtureCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn committed_provider_free_fixture_is_exact_and_verifies_offline() {
    let root = fixture_root();
    let mut actual = BTreeSet::new();
    collect_files(&root, &root, &mut actual);
    let expected = [
        "arms/control.json",
        "arms/treatment.json",
        "customer-proof.json",
        "lineage/control-invocation.json",
        "lineage/control-plan.json",
        "lineage/control-task.json",
        "lineage/identity.json",
        "lineage/policy.json",
        "lineage/treatment-invocation.json",
        "lineage/treatment-plan.json",
        "lineage/treatment-task.json",
        "quality/comparison.json",
        "replay/input.json",
        "replay/result.json",
        "trust-store.json",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    assert_eq!(actual, expected, "fixture file set changed");

    let output = run_verifier(&root);
    assert!(
        output.status.success(),
        "provider-free fixture must verify: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("decode verifier report");
    assert_eq!(report["valid"], Value::Bool(true));
    assert_eq!(report["proof_eligible"], Value::Bool(true));
    assert!(report["steps"]
        .as_array()
        .expect("verifier steps")
        .iter()
        .all(|step| step["status"] == "pass"));
}

#[test]
fn committed_provider_free_fixture_rejects_tampered_artifact() {
    let fixture = FixtureCopy::new();
    fs::write(fixture.0.join("replay/input.json"), b"tampered")
        .expect("tamper copied replay input");

    let output = run_verifier(&fixture.0);
    assert!(!output.status.success(), "tampered fixture must be invalid");
    let report: Value = serde_json::from_slice(&output.stdout).expect("decode verifier report");
    assert_eq!(report["valid"], Value::Bool(false));
    assert_eq!(report["proof_eligible"], Value::Bool(false));
    assert_eq!(report["steps"][0]["name"], "artifact inventory");
    assert_eq!(report["steps"][0]["status"], "fail");
}
