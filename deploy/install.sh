#!/usr/bin/env bash
# Install (or update) the Toybox library server on Ubuntu 22.04 / 24.04.
#
#   sudo bash install.sh --binary ./toybox-server --domain assets.example.com
#   sudo bash install.sh --source /path/to/toybox          # build from a checkout
#
# Safe to run again: it updates the program and the service and never touches
# your library, your tokens or an existing server setup.
#
# Run it from the deploy/ folder of a Toybox checkout (it needs the files next to it).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY=""
SOURCE=""
DOMAIN=""
DATA_DIR="/var/lib/toybox"
SERVER_NAME="Toybox"
PORT="8787"
BUILD_DIR="/var/tmp/toybox-server-build"
BIN_PATH="/usr/local/bin/toybox-server"
UNIT_PATH="/etc/systemd/system/toybox-server.service"
CADDY_MARK="# Managed by the Toybox installer (deploy/install.sh)."

usage() {
  cat <<'USAGE'
Usage: sudo bash install.sh (--binary PATH | --source DIR) [options]

  --binary PATH     install this toybox-server program (built for Linux)
  --source DIR      build the program from a Toybox source checkout (installs
                    rustup and a compiler if they are missing; takes a while)
  --domain NAME     also install Caddy and serve https://NAME with an
                    automatic certificate (the domain must point at this machine)
  --name TEXT       server name shown in Toybox, for a new setup (default: Toybox)
  --data DIR        data folder (default: /var/lib/toybox)
  --port N          local port behind the proxy (default: 8787)
  -h, --help        show this help
USAGE
}

say() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY="${2:?--binary needs a path}"; shift 2 ;;
    --source) SOURCE="${2:?--source needs a folder}"; shift 2 ;;
    --domain) DOMAIN="${2:?--domain needs a name}"; shift 2 ;;
    --name) SERVER_NAME="${2:?--name needs text}"; shift 2 ;;
    --data) DATA_DIR="${2:?--data needs a folder}"; shift 2 ;;
    --port) PORT="${2:?--port needs a number}"; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done

[[ "$(id -u)" -eq 0 ]] || die "run this as root: sudo bash install.sh ..."
if [[ -n "$BINARY" && -n "$SOURCE" ]] || [[ -z "$BINARY" && -z "$SOURCE" ]]; then
  usage >&2
  die "give exactly one of --binary or --source"
fi
[[ "$PORT" =~ ^[0-9]+$ ]] || die "--port must be a number"
[[ "$DATA_DIR" == /* ]] || die "--data must be an absolute path"
[[ -z "$DOMAIN" || "$DOMAIN" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]] || die "--domain must look like assets.example.com"
[[ -f "$SCRIPT_DIR/toybox-server.service" ]] || die "toybox-server.service is missing next to install.sh; run it from the deploy/ folder"

if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  if [[ "${ID:-}" != "ubuntu" ]] || [[ "${VERSION_ID:-}" != "22.04" && "${VERSION_ID:-}" != "24.04" ]]; then
    warn "this installer is written for Ubuntu 22.04 and 24.04; you have ${PRETTY_NAME:-an unknown system}. Continuing."
  fi
fi
command -v systemctl >/dev/null 2>&1 || die "systemd is required"

export DEBIAN_FRONTEND=noninteractive

# --- 1. Packages -----------------------------------------------------------
say "Installing prerequisites"
apt-get update -qq
apt-get install -y -qq ca-certificates curl >/dev/null

# --- 2. The program --------------------------------------------------------
if [[ -n "$SOURCE" ]]; then
  [[ -f "$SOURCE/src-tauri/Cargo.toml" ]] || die "$SOURCE does not look like a Toybox checkout (no src-tauri/Cargo.toml)"
  say "Building toybox-server from $SOURCE (this can take several minutes)"
  apt-get install -y -qq build-essential pkg-config >/dev/null
  if ! command -v cargo >/dev/null 2>&1 && [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    say "Installing the Rust toolchain with rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
  # An existing toolchain may be too old for the dependencies: bring the stable
  # one up to date (without changing the default) and build with it.
  CARGO=(cargo)
  if command -v rustup >/dev/null 2>&1; then
    say "Making sure the stable Rust toolchain is current"
    rustup toolchain install stable --profile minimal >/dev/null
    CARGO=(cargo +stable)
  fi
  mkdir -p "$BUILD_DIR"
  (cd "$SOURCE/src-tauri" && CARGO_TARGET_DIR="$BUILD_DIR" "${CARGO[@]}" build --release -p toybox-server)
  BINARY="$BUILD_DIR/release/toybox-server"
fi
[[ -f "$BINARY" ]] || die "no program at $BINARY"

say "Installing $BIN_PATH"
# Copy next to the target and rename, so a running server is never half-overwritten.
install -m 0755 "$BINARY" "$BIN_PATH.new"
mv -f "$BIN_PATH.new" "$BIN_PATH"
"$BIN_PATH" --version >/dev/null || die "the installed program does not run on this system (is it built for Linux?)"

# --- 3. User and data folder -----------------------------------------------
if ! id -u toybox >/dev/null 2>&1; then
  say "Creating the toybox system user"
  useradd --system --user-group --no-create-home --home-dir "$DATA_DIR" --shell /usr/sbin/nologin toybox
fi
install -d -o toybox -g toybox -m 0750 "$DATA_DIR"

TOKEN=""
if [[ ! -f "$DATA_DIR/server.json" ]]; then
  say "Setting up a new server in $DATA_DIR"
  INIT_OUTPUT="$(runuser -u toybox -- "$BIN_PATH" init --data "$DATA_DIR" --name "$SERVER_NAME")"
  printf '%s\n' "$INIT_OUTPUT"
  TOKEN="$(grep -Eo 'tbx_[A-Za-z0-9_-]+' <<<"$INIT_OUTPUT" | head -n 1 || true)"
else
  say "$DATA_DIR is already set up; keeping your library and tokens"
fi

# --- 4. The service --------------------------------------------------------
say "Installing the systemd service"
sed -e "s|/var/lib/toybox|$DATA_DIR|g" -e "s|127.0.0.1:8787|127.0.0.1:$PORT|g" \
  "$SCRIPT_DIR/toybox-server.service" >"$UNIT_PATH"
chmod 0644 "$UNIT_PATH"
systemctl daemon-reload
systemctl enable toybox-server >/dev/null 2>&1
systemctl restart toybox-server

say "Waiting for the server to answer"
ok=""
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:$PORT/api/v1/health" >/dev/null 2>&1; then
    ok=1
    break
  fi
  sleep 1
done
if [[ -z "$ok" ]]; then
  journalctl -u toybox-server -n 20 --no-pager >&2 || true
  die "the server did not start. See the log above, or: journalctl -u toybox-server"
fi

# --- 5. HTTPS with Caddy (optional) ------------------------------------------
URL="http://127.0.0.1:$PORT"
if [[ -n "$DOMAIN" ]]; then
  if ! command -v caddy >/dev/null 2>&1; then
    say "Installing Caddy from its official apt repository"
    apt-get install -y -qq debian-keyring debian-archive-keyring apt-transport-https gpg >/dev/null
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' |
      gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' -o /etc/apt/sources.list.d/caddy-stable.list
    chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
    apt-get update -qq
    apt-get install -y -qq caddy >/dev/null
  fi

  SITE_FILE="/etc/caddy/Caddyfile"
  # Replace the main Caddyfile only when it is Caddy's untouched sample or ours;
  # otherwise put the site in its own file and ask the user to import it.
  if [[ -f "$SITE_FILE" ]] && ! grep -qF "$CADDY_MARK" "$SITE_FILE" && ! grep -q 'root \* /usr/share/caddy' "$SITE_FILE"; then
    SITE_FILE="/etc/caddy/toybox.caddy"
    warn "/etc/caddy/Caddyfile has your own settings, so the Toybox site goes to $SITE_FILE. Add this line to the Caddyfile: import $SITE_FILE"
  elif [[ -f "$SITE_FILE" ]] && ! grep -qF "$CADDY_MARK" "$SITE_FILE"; then
    cp -p "$SITE_FILE" "$SITE_FILE.bak-$(date +%Y%m%d%H%M%S)"
  fi
  say "Writing $SITE_FILE for $DOMAIN"
  {
    printf '%s Re-running it rewrites this file.\n\n' "$CADDY_MARK"
    sed -e "s|^example\.com {|$DOMAIN {|" -e "s|127.0.0.1:8787|127.0.0.1:$PORT|" \
      -e '/^#/d' "$SCRIPT_DIR/Caddyfile.example" | sed -e '1{/^$/d}'
  } >"$SITE_FILE"
  caddy validate --config "$SITE_FILE" --adapter caddyfile >/dev/null 2>&1 ||
    warn "Caddy did not accept $SITE_FILE; check it with: caddy validate --config $SITE_FILE"
  systemctl enable caddy >/dev/null 2>&1
  systemctl reload caddy 2>/dev/null || systemctl restart caddy
  URL="https://$DOMAIN"
  if ! getent hosts "$DOMAIN" >/dev/null 2>&1; then
    warn "$DOMAIN does not resolve yet. Point its DNS record at this machine; Caddy gets the certificate as soon as it does."
  fi
  if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q 'Status: active'; then
    warn "the firewall (ufw) is on. Open the web ports with: ufw allow 80/tcp && ufw allow 443/tcp"
  fi
fi

# --- 6. What next ------------------------------------------------------------
echo
echo "Toybox server is running."
echo
echo "  Address to enter in Toybox:  $URL"
if [[ -n "$TOKEN" ]]; then
  echo "  Admin token (shown only now): $TOKEN"
else
  echo "  Token: use one you already have, or create another with the command below."
fi
echo
echo "Create a token for someone else (the secret is printed once):"
echo "  sudo -u toybox $BIN_PATH token add --name alice --role read   # or write / admin"
echo "See what is set up:  sudo -u toybox $BIN_PATH status"
echo "Logs:                journalctl -u toybox-server -f"
if [[ -z "$DOMAIN" ]]; then
  echo
  echo "No --domain was given, so the server only answers on this machine. To reach it"
  echo "from the internet with HTTPS, run this script again with --domain your.domain."
fi
