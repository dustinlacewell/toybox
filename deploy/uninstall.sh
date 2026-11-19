#!/usr/bin/env bash
# Remove the Toybox server program and service. Your data folder (library,
# tokens) is left exactly as it is.
#
#   sudo bash uninstall.sh

set -euo pipefail

BIN_PATH="/usr/local/bin/toybox-server"
UNIT_PATH="/etc/systemd/system/toybox-server.service"

say() { printf '==> %s\n' "$*"; }

if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: run this as root: sudo bash uninstall.sh" >&2
  exit 1
fi

if systemctl list-unit-files toybox-server.service >/dev/null 2>&1; then
  say "Stopping the service"
  systemctl disable --now toybox-server >/dev/null 2>&1 || true
fi
systemctl disable --now toybox-server-update.timer >/dev/null 2>&1 || true
rm -f /etc/systemd/system/toybox-server-update.timer /etc/systemd/system/toybox-server-update.service
if [[ -f "$UNIT_PATH" ]]; then
  rm -f "$UNIT_PATH"
  systemctl daemon-reload
fi
if [[ -f "$BIN_PATH" ]]; then
  say "Removing $BIN_PATH"
  rm -f "$BIN_PATH" "$BIN_PATH.previous"
fi
systemctl daemon-reload 2>/dev/null || true

echo
echo "The Toybox server program and service are removed."
echo "Left in place on purpose:"
echo "  /var/lib/toybox (or the data folder you chose)  your library, tokens and caches"
echo "  the toybox system user"
echo "  Caddy and its site file (/etc/caddy/Caddyfile or /etc/caddy/toybox.caddy)"
echo "To remove those too:  sudo userdel toybox  and  sudo apt remove caddy"
