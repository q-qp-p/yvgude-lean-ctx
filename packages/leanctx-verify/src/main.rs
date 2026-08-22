//! leanctx-verify — standalone offline verifier for LeanCTX evidence
//! bundles (`evidence-bundle-v1`).
//!
//! Designed for auditors: no LeanCTX installation, no network, no shared
//! code with the engine. Implements the published contract
//! (`docs/contracts/evidence-bundle-v1.md`, OCP Part 4) independently —
//! a PASS means the *specification* holds.
//!
//! Usage: `leanctx-verify <bundle.zip> [--pubkey <hex>] [--json]`

use std::io::Read;
use std::process::ExitCode;

mod verify;

use verify::{verify_bundle, StepStatus, MAX_ARCHIVE_BYTES};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_out = args.iter().any(|a| a == "--json");
    let pubkey = args
        .iter()
        .position(|a| a == "--pubkey")
        .and_then(|pos| args.get(pos + 1).cloned());
    let bundle_path = args
        .iter()
        .find(|a| !a.starts_with("--") && Some(a.as_str()) != pubkey.as_deref());

    let Some(bundle_path) = bundle_path else {
        eprintln!(
            "leanctx-verify — offline verifier for LeanCTX evidence bundles\n\n\
USAGE:\n  leanctx-verify <bundle.zip> [--pubkey <hex ed25519 key>] [--json]\n\n\
Without --pubkey the manifest's embedded key is used (self-attested mode);\n\
auditors should obtain the organisation's public key out-of-band.\n\n\
Docs: docs/enterprise/reading-evidence.md in the LeanCTX repository."
        );
        return ExitCode::from(2);
    };

    let mut raw = Vec::new();
    match std::fs::File::open(bundle_path) {
        Ok(f) => {
            if let Err(e) = f
                .take(MAX_ARCHIVE_BYTES.saturating_add(1))
                .read_to_end(&mut raw)
            {
                eprintln!("cannot read {bundle_path}: {e}");
                return ExitCode::from(2);
            }
        }
        Err(e) => {
            eprintln!("cannot open {bundle_path}: {e}");
            return ExitCode::from(2);
        }
    }

    let report = verify_bundle(&raw, pubkey.as_deref());

    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("report serializes")
        );
    } else {
        println!("leanctx-verify — evidence-bundle-v1\nbundle: {bundle_path}\n");
        for step in &report.steps {
            let mark = match step.status {
                StepStatus::Pass => "PASS",
                StepStatus::Fail => "FAIL",
                StepStatus::Skipped => "SKIP",
            };
            println!("  [{mark}] {:<38} {}", step.name, step.detail);
        }
        println!(
            "\nresult: {} ({} steps, {} failed){}",
            if report.valid { "VALID" } else { "INVALID" },
            report.steps.len(),
            report
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Fail)
                .count(),
            if report.key_self_attested {
                "\nnote: manifest key was self-attested — obtain the public key out-of-band for full provenance"
            } else {
                ""
            }
        );
    }

    if report.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
