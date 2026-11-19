# Run your own Toybox server

Put your asset library on an Ubuntu machine and open it from the Toybox desktop
app anywhere, over HTTPS, with access tokens. About five minutes. The details
and the API are in [docs/server.md](../docs/server.md).

## What you need

- An Ubuntu 22.04 or 24.04 machine you can `sudo` on (a small VPS is plenty;
  disk space is the thing to size).
- For HTTPS: a domain name whose DNS record points at that machine, and ports
  80 and 443 open to the internet.

## 1. Get the program onto the machine

Either build it on the machine (the installer does this and installs the Rust
compiler if needed; allow ten minutes):

```sh
git clone <your Toybox repository> toybox
cd toybox/deploy
```

or copy a `toybox-server` program you built for Linux next to `install.sh` and
use `--binary` in the next step.

## 2. Install

```sh
# build from source and serve https://assets.example.com
sudo bash install.sh --source .. --domain assets.example.com

# or, with a program you already have
sudo bash install.sh --binary ./toybox-server --domain assets.example.com

# or the latest signed release from GitHub (Linux x86-64 or ARM64), updating itself every day
sudo bash install.sh --release --auto-update --domain assets.example.com
```

`--release [VERSION]` downloads the released program and checks its signature with `minisign`
(a package it installs) against the Toybox release key before installing it. After that,
`toybox-server update` keeps it current with the same key built into the program.

Leave out `--domain` to try it first: the server then only answers on the
machine itself (`http://127.0.0.1:8787`). Run the same command again later with
`--domain` to add HTTPS. Running the installer again is always safe; it updates
the program and never touches your library or tokens.

What it does: creates a `toybox` system user, installs
`/usr/local/bin/toybox-server`, sets up `/var/lib/toybox` (an empty library and
an admin token), installs and starts the `toybox-server` systemd service
(hardened, restarts on failure), and with `--domain` installs Caddy from its
official apt repository and points your domain at the server. Caddy fetches and
renews the certificate by itself.

## 3. Copy the token

The installer ends with the address and an **admin token**. The token is shown
only once and only the hash is stored, so copy it now. In Toybox, add a remote
library with that address and token.

## 4. Give people access

```sh
sudo -u toybox toybox-server token add --name alice --role read
sudo -u toybox toybox-server token add --name bob --role write
sudo -u toybox toybox-server token list
sudo -u toybox toybox-server token revoke alice
```

`read` can browse and download, `write` can also import and delete, `admin` can
also manage tokens. Changes apply immediately; no restart. Run these as the
`toybox` user (`sudo -u toybox`) so the files keep the right owner.

## Putting an existing library on the server

Copy a library folder (the one with `_library_config/` and `library/` inside)
to `/var/lib/toybox/library` **before** the first install, or stop the service
and replace the folder, then `sudo chown -R toybox:toybox /var/lib/toybox` and
start it again. The server notices the new content and reindexes. A library from
before Toybox used SQLite (one with `_library_config/catalog.json`) is converted
to `_library_config/library.db` the first time the server opens it; a library that
already has `library.db` is used as it is. Favourites and personal tags are not
stored on the server (they are in each app's own `user.db`, not in the library
folder); each Toybox app keeps its own.

## Day to day

| | |
| --- | --- |
| Is it running? | `sudo -u toybox toybox-server status` |
| Logs | `journalctl -u toybox-server -f` (one line per request, never tokens) |
| Update | `sudo toybox-server update` (checks the signature, keeps `toybox-server.previous`, restarts, goes back if it does not start); look first with `toybox-server update --check` (exit 10 = available). Every day by itself: `sudo bash install.sh --auto-update` (`--no-auto-update` stops it). See "Versions and updating" in docs/server.md. |
| Before an update that changes the library | `sudo -u toybox toybox-server migrate --check` (changes nothing) |
| Backup | copy `/var/lib/toybox`, including the `-wal` and `-shm` files next to `library/_library_config/library.db` (see docs/server.md) |
| Remove | `sudo bash uninstall.sh` (leaves `/var/lib/toybox` alone) |

## Files in this folder

| File | |
| --- | --- |
| `install.sh` | the installer described above |
| `uninstall.sh` | removes the service and the program, keeps your data |
| `toybox-server.service` | the systemd unit the installer installs |
| `toybox-server-update.service`, `.timer` | the daily update, installed by `install.sh --auto-update` |
| `Caddyfile.example` | the HTTPS reverse-proxy configuration, for doing it by hand |

## Doing it without the installer

1. Install the program as `/usr/local/bin/toybox-server`.
2. `sudo useradd --system --user-group --no-create-home --home-dir /var/lib/toybox --shell /usr/sbin/nologin toybox`
3. `sudo install -d -o toybox -g toybox -m 0750 /var/lib/toybox`
4. `sudo -u toybox toybox-server init --data /var/lib/toybox --name "My Toybox"` and keep the token.
5. Copy `toybox-server.service` to `/etc/systemd/system/`, then `sudo systemctl enable --now toybox-server`.
6. Install Caddy ([official instructions](https://caddyserver.com/docs/install#debian-ubuntu-raspbian)) and use `Caddyfile.example` with your domain.

## If something goes wrong

- **The installer says the program does not run:** it must be built for Linux
  (x86-64 or arm64 matching the machine). A Windows `.exe` will not work.
- **No certificate:** the domain must resolve to this machine and ports 80 and
  443 must be open (`sudo ufw allow 80/tcp && sudo ufw allow 443/tcp`).
  `journalctl -u caddy` says what Caddy is waiting for.
- **Locked out after typing a wrong token several times:** that address waits
  15 minutes. Restarting the service clears it: `sudo systemctl restart toybox-server`.
- **Lost the admin token:** make a new one with `sudo -u toybox toybox-server token add --name admin2 --role admin`.
