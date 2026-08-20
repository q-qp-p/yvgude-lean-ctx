# **FAQ — Windows**

---

**Q: Is Windows supported?**
Yes! lean-ctx supports Windows with PowerShell and Git Bash.

**Install/Update**
- Prefer prebuilt binaries (fastest): `cargo binstall lean-ctx` (or `cargo install-update -a` if you already use `cargo-update`).
- If install/update falls back to source builds, make sure you have the MSVC toolchain + Windows SDK set up (Visual Studio Build Tools).

**Known pitfalls (fixed)**
- `cargo-binstall` requiring `gen_mcp_manifest.exe`: fixed by making dev-tools binaries optional (`--features dev-tools`) so end-user installs only require `lean-ctx.exe`.
- `lean-ctx doctor` incorrectly treating `~/.bashrc` as active on PowerShell when `SHELL` is empty: fixed (PowerShell sessions should not be blocked by a `.bashrc` warning).

**Q: Bash hook strips slashes from paths on Windows!**
This was a path-handling bug in Claude Code's hook execution on Windows with Git Bash. Fixed in v3.2.4. Update: `lean-ctx update`.
