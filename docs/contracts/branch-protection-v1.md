# Branch Protection Contract v1

## Purpose

This contract makes branch protection reviewable infrastructure.
`security/branch-protection-policy-v1.json` is the sole declarative source for
the supported GitHub and GitLab branch rules. It is not a record of settings
made manually in a provider UI.

## Protected branches

`main` is protected on GitHub and GitLab. It is the release and integration
branch, so it must reject history-rewriting operations and require the declared
automated checks before a change becomes authoritative.

The policy intentionally does not require pull-request reviews. The repository
owner may integrate agent work directly on `main`; this does not bypass the
status checks because GitHub administrator enforcement is enabled.

## GitHub requirements

The GitHub `main` rule has these controls:

| Control | Contract |
| --- | --- |
| Administrator enforcement | Enabled; administrators are subject to the same required checks. |
| Status checks | `CI Green` and `Security Scan`. |
| Strict up-to-date checks | Disabled; each required check must still succeed on the submitted commit. |
| Pull-request reviews | Not required. |
| Commit signatures | Not required by this contract. |
| Force pushes and deletion | Disabled. |
| Linear history | Not required. |

`CI Green` is the aggregate workflow gate for formatting, static analysis,
tests, and other required CI jobs. `Security Scan` remains a separate required
check for the security boundary; no nonexistent `public-clean-room` context is
required.

## GitLab requirements

The GitLab `main` rule grants push and merge access only to level 40 (Owner) and
disables force pushes. The GitLab host and project identifier are policy data so
the same change remains reviewable with the GitHub configuration.

## Applying a reviewed change

1. Modify `security/branch-protection-policy-v1.json` in a reviewed change.
2. Update the recorded SHA-256 values when either enforcement script changes.
3. Run `python3 scripts/apply-branch-protection.py dry-run --target all`.
4. After integration, use a short-lived token with the minimum provider
   administration scope and run `apply --target github`, `gitlab`, or `all`.
5. Run `python3 scripts/verify-branch-protection.py verify` with a GitHub token
   and retain its JSON output as change evidence.

The apply tool never stores tokens. It reads `GITHUB_TOKEN` and `GITLAB_TOKEN`,
or accepts the equivalent explicit command-line options for controlled use.

## Emergency override

Use an override only to restore availability or mitigate an active incident.
An Owner records the incident, exact setting, actor, and expiry before making a
temporary provider-side change. Restore the policy-defined rule as soon as the
incident is contained, run the verification command, and file a follow-up
review to document the outcome. Permanent exceptions require a policy change;
UI-only changes are not an accepted steady state.

## Tamper evidence

The policy records the SHA-256 digests of both enforcement scripts. The apply
tool validates those digests before it performs a dry run or remote mutation,
so a changed scanner cannot silently run under the old approved policy.
