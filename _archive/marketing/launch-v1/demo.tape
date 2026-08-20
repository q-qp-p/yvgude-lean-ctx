# lean-ctx v1.0 launch demo — render with: vhs marketing/launch-v1/demo.tape
# Produces demo.gif + demo.mp4 (~2 min). Uses the release build from this
# worktree so the video shows real 1.0 functionality (doctor --migrate-check).
# Run from the repo root.

Output marketing/launch-v1/demo.gif
Output marketing/launch-v1/demo.mp4

Set FontSize 18
Set Width 1280
Set Height 720
Set Theme "Catppuccin Mocha"
Set Padding 16
Set TypingSpeed 40ms

Env PATH "rust/target/release:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

# --- Scene 1: what is this ---
Type "# lean-ctx — Context Runtime for AI agents. One binary, local, zero telemetry."
Enter
Sleep 2.5s
Type "lean-ctx --help | head -3"
Enter
Sleep 3s

# --- Scene 2: the problem & the read modes ---
Type "# Your agent reads a 366-line file to answer one question. That's the waste."
Enter
Sleep 2.5s
Type "wc -l rust/src/core/contracts.rs"
Enter
Sleep 2s
Type "lean-ctx read rust/src/core/contracts.rs --mode signatures | head -14"
Enter
Sleep 4.5s
Type "# Full API surface, ~92% fewer tokens. 10 modes like this. Re-reads: ~13 tokens."
Enter
Sleep 3s

# --- Scene 3: shell compression ---
Type "# Same idea for every shell command your agent runs:"
Enter
Sleep 2s
Type 'lean-ctx -c "git log --oneline -12"'
Enter
Sleep 3.5s

# --- Scene 4: the 1.0 moment ---
Type "# v1.0 is a stability promise. The entire migration from 0.x:"
Enter
Sleep 2.5s
Type "lean-ctx doctor --migrate-check"
Enter
Sleep 5s

# --- Scene 5: receipts ---
Type "# And it keeps the receipts — measured locally, never phoned home:"
Enter
Sleep 2s
Type "lean-ctx gain"
Enter
Sleep 5.5s
Type "# leanctx.com — Apache-2.0, local-first, free forever."
Enter
Sleep 3.5s
