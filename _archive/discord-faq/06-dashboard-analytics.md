# **FAQ — Dashboard & Analytics**

---

**Q: How do I see my savings?**
```bash
lean-ctx gain              # terminal dashboard
lean-ctx gain --live       # real-time mode
lean-ctx gain --web        # opens web dashboard at localhost:3333
```

**Q: Dashboard shows 0% / no results!**
- Make sure your AI tool is actually using lean-ctx tools (check `lean-ctx doctor`)
- Shell hook savings and MCP savings are tracked separately
- Run a few AI-assisted coding tasks first, then check again
- Fixed display issues in v3.2.6 — update: `lean-ctx update`

**Q: "Dashboard indicates update available" but the version doesn't exist yet?**
This was a bug in v3.2.4 where the update check compared against an unreleased version. Fixed in v3.2.5+.

**Q: What is the Runtime Control Plane panel?**
The web dashboard (`lean-ctx gain --web`) includes a **Runtime Control Plane** panel that shows:

- **IDE indicator** — which IDE/agent is connected and its MCP capability tier (1–4)
- **Pressure gauge** — real-time context pressure with budget utilization percentage
- **Bounce stats** — number of bounces detected, tokens wasted, and learned patterns
- **Dynamic tool categories** — which of the 6 tool categories are currently loaded, with per-category call counts

This panel gives you a live view of how lean-ctx is adapting to your IDE and optimizing context in real time.

**Q: Can I run the dashboard without an auth token?**
Yes. By default the dashboard requires a Bearer token (auto-generated, or pinned via `--auth-token` / `LEAN_CTX_HTTP_TOKEN`). If juggling a token is inconvenient — e.g. a purely local setup or a Docker container you reach from the host — you can turn it off:

```bash
lean-ctx dashboard --no-auth                  # one run (alias: --auth=false)
LEAN_CTX_DASHBOARD_AUTH=false lean-ctx dashboard
lean-ctx config set dashboard_auth false      # persist it
```

No-auth mode is **not** unprotected. Instead of a token, the dashboard blocks browser cross-origin/CSRF and DNS-rebinding attacks with request-header validation on every `/api/*` and `/metrics` request:

- **`Sec-Fetch-Site`** — requests the browser marks as `cross-site`/`same-site` are rejected; only `same-origin` and direct navigation (`none`) pass.
- **`Origin`** — when present, must be same-origin as the dashboard.
- **`Host` allowlist** — the request `Host` must be a loopback host (`127.0.0.1`/`localhost`/`[::1]`, **on any port**), the host you bound to, or an entry in `LEAN_CTX_DASHBOARD_ALLOWED_HOSTS`. This stops DNS-rebinding attacks (loopback hosts can't be a rebinding target, so they're accepted regardless of port).

Non-browser clients (curl, Prometheus scraping `/metrics`) don't send those headers and keep working, as long as they target an allowlisted host.

**Best practice:** keep no-auth on loopback. For Docker, bind the container to `0.0.0.0` but publish **only** to the host loopback:

```bash
lean-ctx dashboard --host=0.0.0.0 --no-auth
docker run ... -p 127.0.0.1:3333:3333 ...
# Remapping to a different host port works out of the box too — the Host is
# still loopback, just on another port:
docker run ... -p 127.0.0.1:60000:3333 ...   # reach it at http://127.0.0.1:60000
```

If you reach the dashboard via a custom (non-loopback) hostname, add it: `LEAN_CTX_DASHBOARD_ALLOWED_HOSTS=box.local:3333`. Avoid binding to `0.0.0.0` with no-auth on an untrusted network — browser attacks stay blocked, but any non-browser client that can reach the port has full access.
