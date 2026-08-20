#!/usr/bin/env bash
# Lean-ctx enterprise pilot installer for macOS.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/yvgude/lean-ctx/main/scripts/pilot-setup.sh | bash
#   ./scripts/pilot-setup.sh [--uninstall]
#
# Optional deployment controls:
#   LEANCTX_VERSION=vX.Y.Z        Release tag to install (default: latest)
#   LEANCTX_REPOSITORY=org/repo   GitHub release/source repository
#   LEANCTX_SHA256=<digest>       Required SHA-256 for the downloaded release asset
#   LEANCTX_PROXY_PORT=4444       Loopback proxy port
#   LEANCTX_SOURCE_REF=<ref>      Source ref used only if a release download fails

set -Eeuo pipefail

readonly LABEL="com.leanctx.proxy"
readonly REPOSITORY="${LEANCTX_REPOSITORY:-yvgude/lean-ctx}"
readonly VERSION="${LEANCTX_VERSION:-latest}"
readonly PROXY_PORT="${LEANCTX_PROXY_PORT:-4444}"
readonly BIN_DIR="${HOME}/.local/bin"
readonly BIN_PATH="${BIN_DIR}/lean-ctx"
readonly CONFIG_DIR="${HOME}/.config/lean-ctx"
readonly CONFIG_PATH="${CONFIG_DIR}/config.toml"
readonly PLIST_DIR="${HOME}/Library/LaunchAgents"
readonly PLIST_PATH="${PLIST_DIR}/${LABEL}.plist"
readonly STATE_DIR="${HOME}/.local/state/lean-ctx"
readonly CURSOR_DIR="${HOME}/.cursor"
readonly CURSOR_CONFIG="${CURSOR_DIR}/mcp.json"
readonly ZPROFILE="${HOME}/.zprofile"
readonly PROFILE_START="# >>> lean-ctx pilot PATH >>>"
readonly PROFILE_END="# <<< lean-ctx pilot PATH <<<"

WORK_DIR=""
NEW_BINARY=""
RESOLVED_BINARY=""

if [[ -t 1 ]]; then
  RED=$'\033[0;31m'
  GREEN=$'\033[0;32m'
  YELLOW=$'\033[0;33m'
  BLUE=$'\033[0;34m'
  BOLD=$'\033[1m'
  RESET=$'\033[0m'
else
  RED="" GREEN="" YELLOW="" BLUE="" BOLD="" RESET=""
fi

cleanup() {
  local status=$?
  if [[ -n "${NEW_BINARY}" && -e "${NEW_BINARY}" ]]; then
    rm -f -- "${NEW_BINARY}"
  fi
  if [[ -n "${WORK_DIR}" && -d "${WORK_DIR}" ]]; then
    rm -rf -- "${WORK_DIR}"
  fi
  exit "${status}"
}

on_error() {
  local line=$1 status=$2
  printf '%sERROR:%s setup stopped at line %s (exit %s).\n' \
    "${RED}" "${RESET}" "${line}" "${status}" >&2
  printf '%sRun this installer again after fixing the reported prerequisite or network issue.%s\n' \
    "${YELLOW}" "${RESET}" >&2
}

trap cleanup EXIT
trap 'on_error "$LINENO" "$?"' ERR

headline() {
  printf '\n%sLean-ctx Enterprise Pilot Setup%s\n' "${BOLD}" "${RESET}"
  printf '───────────────────────────────────\n'
}

step() {
  printf '\n%s[%s/7]%s %s\n' "${BLUE}" "$1" "${RESET}" "$2"
}

success() {
  printf '  %s✓%s %s\n' "${GREEN}" "${RESET}" "$1"
}

info() {
  printf '  %s•%s %s\n' "${BLUE}" "${RESET}" "$1"
}

warn() {
  printf '  %s!%s %s\n' "${YELLOW}" "${RESET}" "$1" >&2
}

die() {
  printf '%sERROR:%s %s\n' "${RED}" "${RESET}" "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

validate_inputs() {
  [[ "${REPOSITORY}" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] || \
    die "LEANCTX_REPOSITORY must be in the form organization/repository."
  [[ "${VERSION}" != *"/"* && "${VERSION}" != *".."* ]] || \
    die "LEANCTX_VERSION must be a release tag, not a path."
  [[ "${PROXY_PORT}" =~ ^[0-9]+$ ]] && (( PROXY_PORT >= 1024 && PROXY_PORT <= 65535 )) || \
    die "LEANCTX_PROXY_PORT must be an unprivileged TCP port (1024-65535)."
}

check_prerequisites() {
  [[ "$(uname -s)" == "Darwin" ]] || die "This installer supports macOS only."
  require_command curl
  require_command git
  require_command codesign
  require_command launchctl
  require_command tar
  require_command unzip
  require_command file
  require_command shasum
  require_command plutil
  validate_inputs
  success "macOS prerequisites available"
}

release_base_url() {
  if [[ "${VERSION}" == "latest" ]]; then
    printf 'https://github.com/%s/releases/latest/download' "${REPOSITORY}"
  else
    printf 'https://github.com/%s/releases/download/%s' "${REPOSITORY}" "${VERSION}"
  fi
}

release_assets() {
  local machine=$1
  case "${machine}" in
    arm64|aarch64)
      printf '%s\n' \
        'lean-ctx-aarch64-apple-darwin.tar.gz' \
        'lean-ctx-darwin-arm64.tar.gz' \
        'lean-ctx-macos-arm64.tar.gz' \
        'lean-ctx-arm64-apple-darwin.tar.gz' \
        'lean-ctx-aarch64-apple-darwin.zip' \
        'lean-ctx-darwin-arm64.zip' \
        'lean-ctx-macos-arm64.zip'
      ;;
    x86_64)
      printf '%s\n' \
        'lean-ctx-x86_64-apple-darwin.tar.gz' \
        'lean-ctx-darwin-x86_64.tar.gz' \
        'lean-ctx-macos-x86_64.tar.gz' \
        'lean-ctx-x86_64-apple-darwin.zip' \
        'lean-ctx-darwin-x86_64.zip' \
        'lean-ctx-macos-x86_64.zip'
      ;;
    *) die "Unsupported macOS architecture: ${machine}" ;;
  esac
}

verify_checksum() {
  local artifact=$1 asset_name=$2 base_url=$3 expected="" actual="" checksums

  if [[ -n "${LEANCTX_SHA256:-}" ]]; then
    expected="${LEANCTX_SHA256}"
  else
    checksums="${WORK_DIR}/checksums.txt"
    if curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
      --retry 2 "${base_url}/SHA256SUMS" -o "${checksums}" 2>/dev/null; then
      expected="$(awk -v name="${asset_name}" '$NF == name || $NF == "*" name { print $1; exit }' "${checksums}")"
    elif curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
      --retry 2 "${base_url}/${asset_name}.sha256" -o "${checksums}" 2>/dev/null; then
      expected="$(awk '{ print $1; exit }' "${checksums}")"
    fi
  fi

  if [[ -z "${expected}" ]]; then
    warn "No release checksum was published; relying on GitHub HTTPS transport."
    return 0
  fi
  [[ "${expected}" =~ ^[A-Fa-f0-9]{64}$ ]] || die "Release checksum for ${asset_name} is malformed."
  actual="$(shasum -a 256 "${artifact}" | awk '{ print $1 }')"
  expected="$(printf '%s' "${expected}" | tr '[:upper:]' '[:lower:]')"
  [[ "${actual}" == "${expected}" ]] || die "SHA-256 verification failed for ${asset_name}."
  success "release checksum verified"
}

extract_binary() {
  local artifact=$1 asset_name=$2 extract_dir candidate
  extract_dir="${WORK_DIR}/extract"
  rm -rf -- "${extract_dir}"
  mkdir -p "${extract_dir}"

  case "${asset_name}" in
    *.tar.gz|*.tgz) tar -xzf "${artifact}" -C "${extract_dir}" ;;
    *.zip) unzip -q "${artifact}" -d "${extract_dir}" ;;
    *)
      if file "${artifact}" | grep -q 'Mach-O'; then
        printf '%s\n' "${artifact}"
        return 0
      fi
      return 1
      ;;
  esac

  while IFS= read -r candidate; do
    if [[ -x "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done < <(find "${extract_dir}" -type f -name lean-ctx -print)
  return 1
}

download_release_binary() {
  local base_url asset url archive candidate
  base_url="$(release_base_url)"
  while IFS= read -r asset; do
    url="${base_url}/${asset}"
    archive="${WORK_DIR}/${asset}"
    info "trying release asset ${asset}" >&2
    if curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
      --retry 3 --retry-delay 1 "${url}" -o "${archive}" 2>/dev/null; then
      verify_checksum "${archive}" "${asset}" "${base_url}" >&2
      if candidate="$(extract_binary "${archive}" "${asset}")"; then
        [[ -n "${candidate}" ]] || die "Release asset ${asset} did not contain lean-ctx."
        RESOLVED_BINARY="${candidate}"
        return 0
      fi
      warn "Release asset ${asset} does not contain an executable lean-ctx binary."
    fi
  done < <(release_assets "$(uname -m)")
  return 1
}

build_from_source() {
  local source_dir source_ref binary
  require_command cargo
  source_dir="${WORK_DIR}/source"
  source_ref="${LEANCTX_SOURCE_REF:-}"
  if [[ -z "${source_ref}" && "${VERSION}" != "latest" ]]; then
    source_ref="${VERSION}"
  fi

  info "cloning ${REPOSITORY} for source fallback" >&2
  if [[ -n "${source_ref}" ]]; then
    git clone --depth 1 --branch "${source_ref}" \
      "https://github.com/${REPOSITORY}.git" "${source_dir}" >&2
  else
    git clone --depth 1 "https://github.com/${REPOSITORY}.git" "${source_dir}" >&2
  fi

  [[ -f "${source_dir}/rust/Cargo.toml" ]] || \
    die "Source fallback could not find rust/Cargo.toml in ${REPOSITORY}."
  (
    cd "${source_dir}/rust"
    cargo build --release >&2
  )
  binary="${source_dir}/rust/target/release/lean-ctx"
  [[ -x "${binary}" ]] || die "Source build completed without producing lean-ctx."
  RESOLVED_BINARY="${binary}"
}

install_binary() {
  local source_binary
  mkdir -p "${BIN_DIR}"
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/lean-ctx-pilot.XXXXXX")"

  if download_release_binary; then
    source_binary="${RESOLVED_BINARY}"
    success "downloaded release binary"
  else
    warn "No compatible release asset found; building from source instead."
    build_from_source
    source_binary="${RESOLVED_BINARY}"
    success "built lean-ctx from source"
  fi

  NEW_BINARY="${BIN_DIR}/.lean-ctx.new.$$"
  install -m 0755 "${source_binary}" "${NEW_BINARY}"
  codesign --force --sign - "${NEW_BINARY}" >/dev/null
  codesign --verify --strict --verbose=2 "${NEW_BINARY}" >/dev/null
  mv -f "${NEW_BINARY}" "${BIN_PATH}"
  NEW_BINARY=""
  success "installed and ad-hoc signed ${BIN_PATH}"
}

ensure_config() {
  mkdir -p "${CONFIG_DIR}"
  chmod 700 "${CONFIG_DIR}"
  if [[ -e "${CONFIG_PATH}" && ! -f "${CONFIG_PATH}" ]]; then
    die "Configuration path exists but is not a file: ${CONFIG_PATH}"
  fi
  if [[ -f "${CONFIG_PATH}" ]]; then
    success "existing configuration preserved"
    return 0
  fi

  umask 077
  cat >"${CONFIG_PATH}" <<EOF
# Lean-ctx pilot configuration -- managed baseline.
# Keep this file under user control; enterprise policy can be layered separately.
hook_mode = "replace"
minimal_overhead = true
proxy_port = ${PROXY_PORT}
EOF
  chmod 600 "${CONFIG_PATH}"
  success "created enterprise baseline configuration"
}

write_launch_agent() {
  mkdir -p "${PLIST_DIR}" "${STATE_DIR}"
  chmod 700 "${STATE_DIR}"
  umask 077
  cat >"${PLIST_PATH}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/sandbox-exec</string>
    <string>-p</string>
    <string>(version 1) (allow default) (deny file-read* file-write* (subpath "${HOME}/Documents") (subpath "${HOME}/Desktop") (subpath "${HOME}/Downloads"))</string>
    <string>${BIN_PATH}</string>
    <string>proxy</string>
    <string>start</string>
    <string>--port=${PROXY_PORT}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>${HOME}</string>
    <key>PATH</key>
    <string>${BIN_DIR}:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    <key>LEAN_CTX_CONFIG</key>
    <string>${CONFIG_PATH}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>StandardOutPath</key>
  <string>${STATE_DIR}/proxy.out.log</string>
  <key>StandardErrorPath</key>
  <string>${STATE_DIR}/proxy.err.log</string>
</dict>
</plist>
EOF
  chmod 644 "${PLIST_PATH}"
  plutil -lint "${PLIST_PATH}" >/dev/null || die "Generated LaunchAgent plist is invalid."
}

load_launch_agent() {
  local uid domain service
  uid="$(id -u)"
  domain="gui/${uid}"
  service="${domain}/${LABEL}"

  launchctl bootout "${service}" >/dev/null 2>&1 || true
  if ! launchctl bootstrap "${domain}" "${PLIST_PATH}"; then
    die "Could not load ${LABEL}. Run from a logged-in macOS user session."
  fi
  launchctl kickstart -k "${service}" || die "Could not start ${LABEL}."
  success "installed and started macOS LaunchAgent"
}

configure_zsh_path() {
  touch "${ZPROFILE}"
  if grep -Fqx "${PROFILE_START}" "${ZPROFILE}"; then
    success "zsh PATH already configured"
    return 0
  fi
  cat >>"${ZPROFILE}" <<'EOF'

# >>> lean-ctx pilot PATH >>>
export PATH="$HOME/.local/bin:$PATH"
# <<< lean-ctx pilot PATH <<<
EOF
  success "added ~/.local/bin to zsh login PATH"
}

cursor_detected() {
  [[ -d "${CURSOR_DIR}" || -d "/Applications/Cursor.app" || \
    -d "${HOME}/Applications/Cursor.app" ]] || command -v cursor >/dev/null 2>&1
}

configure_cursor_mcp() {
  if ! cursor_detected; then
    info "Cursor not detected; skipping MCP configuration"
    return 0
  fi
  require_command python3
  mkdir -p "${CURSOR_DIR}"
  python3 - "${CURSOR_CONFIG}" "${BIN_PATH}" <<'PY'
import json
import os
import sys
import tempfile

path, binary = sys.argv[1:]
data = {}
if os.path.exists(path):
    try:
        with open(path, encoding="utf-8") as source:
            data = json.load(source)
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit("Cursor MCP config is not valid JSON: {}".format(error))
if not isinstance(data, dict):
    raise SystemExit("Cursor MCP config root must be a JSON object.")
servers = data.setdefault("mcpServers", {})
if not isinstance(servers, dict):
    raise SystemExit("Cursor MCP config field 'mcpServers' must be a JSON object.")
servers["lean-ctx"] = {
    "command": binary,
    "args": ["mcp", "serve"],
}
directory = os.path.dirname(path)
fd, temporary = tempfile.mkstemp(prefix=".mcp.", suffix=".json", dir=directory)
try:
    with os.fdopen(fd, "w", encoding="utf-8") as target:
        json.dump(data, target, indent=2, sort_keys=True)
        target.write("\n")
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
except BaseException:
    try:
        os.unlink(temporary)
    except OSError:
        pass
    raise
PY
  success "configured lean-ctx MCP server for Cursor"
}

run_doctor() {
  if "${BIN_PATH}" doctor; then
    success "lean-ctx doctor completed successfully"
  else
    die "lean-ctx doctor reported a problem. Review ${STATE_DIR}/proxy.err.log."
  fi
}

remove_profile_block() {
  local temporary
  [[ -f "${ZPROFILE}" ]] || return 0
  grep -Fqx "${PROFILE_START}" "${ZPROFILE}" || return 0
  temporary="${ZPROFILE}.lean-ctx.$$"
  sed "/^${PROFILE_START}$/,/^${PROFILE_END}$/d" "${ZPROFILE}" >"${temporary}"
  mv -f "${temporary}" "${ZPROFILE}"
}

remove_cursor_mcp() {
  [[ -f "${CURSOR_CONFIG}" ]] || return 0
  if ! command -v python3 >/dev/null 2>&1; then
    warn "Python 3 unavailable; left the lean-ctx Cursor MCP entry in place."
    return 0
  fi
  if ! python3 - "${CURSOR_CONFIG}" <<'PY'
import json
import os
import sys
import tempfile

path = sys.argv[1]
try:
    with open(path, encoding="utf-8") as source:
        data = json.load(source)
except (OSError, json.JSONDecodeError) as error:
    raise SystemExit("Cursor MCP config was left unchanged: {}".format(error))
if not isinstance(data, dict) or not isinstance(data.get("mcpServers", {}), dict):
    raise SystemExit("Cursor MCP config was left unchanged: unsupported JSON shape.")
if "lean-ctx" not in data["mcpServers"]:
    raise SystemExit(0)
del data["mcpServers"]["lean-ctx"]
directory = os.path.dirname(path)
fd, temporary = tempfile.mkstemp(prefix=".mcp.", suffix=".json", dir=directory)
try:
    with os.fdopen(fd, "w", encoding="utf-8") as target:
        json.dump(data, target, indent=2, sort_keys=True)
        target.write("\n")
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
except BaseException:
    try:
        os.unlink(temporary)
    except OSError:
        pass
    raise
PY
  then
    warn "Cursor MCP config was left unchanged."
  fi
}

uninstall() {
  local uid
  headline
  printf '%sRemoving Lean-ctx pilot installation%s\n' "${BOLD}" "${RESET}"
  uid="$(id -u)"
  launchctl bootout "gui/${uid}/${LABEL}" >/dev/null 2>&1 || true
  rm -f -- "${PLIST_PATH}" "${BIN_PATH}"
  remove_profile_block
  remove_cursor_mcp
  if [[ -f "${CONFIG_PATH}" ]] && grep -Fqx '# Lean-ctx pilot configuration -- managed baseline.' "${CONFIG_PATH}"; then
    rm -f -- "${CONFIG_PATH}"
    rmdir "${CONFIG_DIR}" 2>/dev/null || true
  else
    info "configuration preserved at ${CONFIG_PATH}"
  fi
  rmdir "${BIN_DIR}" 2>/dev/null || true
  success "removed LaunchAgent, binary, managed shell PATH, and Cursor MCP entry"
  printf 'User data and logs remain in %s for review.\n' "${STATE_DIR}"
}

main() {
  case "${1:-}" in
    --uninstall)
      [[ $# -eq 1 ]] || die "Usage: $0 [--uninstall]"
      uninstall
      ;;
    "")
      headline
      step 1 "Checking prerequisites"
      check_prerequisites
      step 2 "Installing lean-ctx ${VERSION}"
      install_binary
      step 3 "Creating configuration"
      ensure_config
      step 4 "Installing proxy LaunchAgent"
      write_launch_agent
      load_launch_agent
      step 5 "Configuring zsh PATH"
      configure_zsh_path
      step 6 "Configuring Cursor MCP (when installed)"
      configure_cursor_mcp
      step 7 "Verifying installation"
      run_doctor
      printf '\n%sLean-ctx pilot setup is complete.%s\n' "${GREEN}${BOLD}" "${RESET}"
      printf 'Open a new zsh session, or run: export PATH="$HOME/.local/bin:$PATH"\n'
      ;;
    *) die "Usage: $0 [--uninstall]" ;;
  esac
}

main "$@"
