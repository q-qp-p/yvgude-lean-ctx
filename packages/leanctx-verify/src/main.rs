//! leanctx-verify — standalone offline verifier for LeanCTX evidence
//! bundles (`evidence-bundle-v1` and customer-proof `v2`).
//!
//! Designed for auditors: no LeanCTX installation, no network, no shared
//! code with the engine. Implements the published contract
//! (`docs/contracts/evidence-bundle-v1.md` and the V2 verification companion)
//! independently —
//! a PASS means the *specification* holds.
//!
//! Usage: `leanctx-verify <bundle.zip> [--pubkey <hex>] [--json]` or
//! `leanctx-verify v2 <bundle.json> --trust-store <path> --artifact-root <dir>`.

use std::io::Read;
use std::process::ExitCode;

mod v2;
mod verify;

use v2::verify_v2_document;
use verify::{verify_bundle, StepStatus, MAX_ARCHIVE_BYTES};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "v2") {
        return run_v2(&args[1..]);
    }
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

fn run_v2(args: &[String]) -> ExitCode {
    let json_out = args.iter().any(|arg| arg == "--json");
    let trust_store = required_flag(args, "--trust-store");
    let artifact_root = required_flag(args, "--artifact-root");
    let positional: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(index, arg)| {
            !arg.starts_with("--")
                && index
                    .checked_sub(1)
                    .and_then(|previous| args.get(previous))
                    .is_none_or(|previous| {
                        previous != "--trust-store" && previous != "--artifact-root"
                    })
        })
        .map(|(_, arg)| arg.as_str())
        .collect();
    let Some(bundle_path) = positional
        .first()
        .copied()
        .filter(|_| positional.len() == 1)
    else {
        return v2_usage();
    };
    let (Some(trust_store), Some(artifact_root)) = (trust_store, artifact_root) else {
        return v2_usage();
    };
    let raw = match read_limited(bundle_path, MAX_ARCHIVE_BYTES) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("cannot read {bundle_path}: {error}");
            return ExitCode::from(2);
        }
    };
    let trust = match read_limited(trust_store, MAX_ARCHIVE_BYTES) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("cannot read trust store {trust_store}: {error}");
            return ExitCode::from(2);
        }
    };
    let report = verify_v2_document(
        &raw,
        Some(&trust),
        Some(std::path::Path::new(artifact_root)),
    );
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("report serializes")
        );
    } else {
        println!("leanctx-verify — customer-proof-evidence-bundle-v2\nbundle: {bundle_path}\n");
        for step in &report.steps {
            let mark = match step.status {
                StepStatus::Pass => "PASS",
                StepStatus::Fail => "FAIL",
                StepStatus::Skipped => "SKIP",
            };
            println!("  [{mark}] {:<38} {}", step.name, step.detail);
        }
        println!(
            "\nresult: {} (proof eligible: {})",
            if report.valid { "VALID" } else { "INVALID" },
            if report.proof_eligible { "yes" } else { "no" }
        );
    }
    if report.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn required_flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

fn read_limited(path: &str, max_bytes: u64) -> Result<Vec<u8>, std::io::Error> {
    let mut raw = Vec::new();
    std::fs::File::open(path)?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut raw)?;
    if u64::try_from(raw.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "input exceeds verifier byte limit",
        ));
    }
    Ok(raw)
}

fn v2_usage() -> ExitCode {
    eprintln!(
        "leanctx-verify v2 — independently verify a customer-proof bundle\n\n\
USAGE:\n  leanctx-verify v2 <bundle.json> --trust-store <trust.json> --artifact-root <dir> [--json]\n\n\
V2 never accepts an embedded or self-attested public key. Docs:\n\
docs/contracts/evidence-bundle-v2-verification-v1.md"
    );
    ExitCode::from(2)
}
