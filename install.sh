#!/usr/bin/env bash
# Paritr Protocol 10 installer for Linux, macOS and FreeBSD.
set -Eeuo pipefail

NODE_VERSION="4.1.0-rc.1"
PROTOCOL_VERSION="10"
CHAIN_ID="paritr-mainnet"
RANDOMX_TAG="v1.2.3"
RANDOMX_COMMIT="12f2c2ffe2108d6cf54c391fee33c8bc3646cdab"
SOURCE_BASE="${PARITR_SOURCE:-https://paritr.highactive.de/downloads}"
SERVICE_NAME="${PARITR_SERVICE:-paritr-node}"
INSTALL_USER="${SUDO_USER:-$(id -un)}"
if [[ -n "${SUDO_USER:-}" ]] && command -v getent >/dev/null 2>&1; then
  INSTALL_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
else
  INSTALL_HOME="$HOME"
fi
NODE_DIR="${PARITR_DIR:-$INSTALL_HOME/paritr-node}"
PORT=5050
ADDRESS=""
PUBLIC_URL=""
PORTAL_URL=""
PAIR_CODE=""
CORES=0
INTENSITY=100
MODE="light"
NO_AUTOSTART=0
OPEN_FIREWALL=0

usage() {
  cat <<'TXT'
Usage: ./install.sh [options]
  --address P...             Enable mining and pay this address
  --port 5050                Public RPC/P2P TCP port
  --public-url https://...   Advertised reverse-proxy URL
  --portal-url https://...   Optional wallet portal URL
  --pair-code PRTR-...       Optional one-time pairing code
  --dir PATH                 Installation directory
  --source URL               Checksummed release base URL
  --cores N                  Mining workers, 0 = automatic
  --intensity 5..100         Mining CPU duty cycle
  --fast                     RandomX fast mode (~2.1 GiB, mining)
  --light                    RandomX light mode (~256 MiB, validation)
  --open-firewall            Open the public TCP port where supported
  --no-autostart             Install without enabling the OS service
TXT
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --address) ADDRESS="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --public-url) PUBLIC_URL="$2"; shift 2 ;;
    --portal-url) PORTAL_URL="$2"; shift 2 ;;
    --pair-code|--pair) PAIR_CODE="$2"; shift 2 ;;
    --dir) NODE_DIR="$2"; shift 2 ;;
    --source) SOURCE_BASE="${2%/}"; shift 2 ;;
    --cores) CORES="$2"; shift 2 ;;
    --intensity) INTENSITY="$2"; shift 2 ;;
    --fast) MODE="fast"; shift ;;
    --light) MODE="light"; shift ;;
    --open-firewall) OPEN_FIREWALL=1; shift ;;
    --no-autostart) NO_AUTOSTART=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

SOURCE_BASE="${SOURCE_BASE%/}"
[[ "$SERVICE_NAME" =~ ^[A-Za-z0-9_.@-]+$ ]] || { echo "Invalid service name" >&2; exit 2; }
[[ "$INSTALL_USER" =~ ^[A-Za-z0-9_.-]+$ ]] || { echo "Invalid install user" >&2; exit 2; }
[[ "$NODE_DIR" != *$'\n'* && "$NODE_DIR" != *'"'* ]] || { echo "Install path contains unsupported characters" >&2; exit 2; }
[[ "$PORT" =~ ^[0-9]+$ ]] && (( PORT > 0 && PORT < 65536 )) || { echo "Invalid port" >&2; exit 2; }
[[ "$CORES" =~ ^[0-9]+$ ]] && (( CORES <= 1024 )) || { echo "Invalid core count" >&2; exit 2; }
[[ "$INTENSITY" =~ ^[0-9]+$ ]] && (( INTENSITY >= 5 && INTENSITY <= 100 )) || { echo "Invalid intensity" >&2; exit 2; }
[[ -z "$PUBLIC_URL" || "$PUBLIC_URL" == https://* ]] || { echo "Public URL must use HTTPS" >&2; exit 2; }
[[ -z "$PORTAL_URL" || "$PORTAL_URL" == https://* ]] || { echo "Portal URL must use HTTPS" >&2; exit 2; }
[[ -z "$PAIR_CODE" || -n "$PORTAL_URL" ]] || { echo "--pair-code requires --portal-url" >&2; exit 2; }
[[ -z "$PORTAL_URL" || -n "$PAIR_CODE" ]] || { echo "--portal-url requires --pair-code" >&2; exit 2; }

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS:$ARCH" in
  Linux:x86_64|Linux:amd64) TARGET="x86_64-unknown-linux-gnu" ;;
  Linux:aarch64|Linux:arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  Linux:armv7l|Linux:armv7) TARGET="armv7-unknown-linux-gnueabihf"; MODE="light" ;;
  Linux:riscv64) TARGET="riscv64gc-unknown-linux-gnu" ;;
  Linux:ppc64le) TARGET="powerpc64le-unknown-linux-gnu" ;;
  Darwin:x86_64) TARGET="x86_64-apple-darwin" ;;
  Darwin:arm64|Darwin:aarch64) TARGET="aarch64-apple-darwin" ;;
  FreeBSD:x86_64|FreeBSD:amd64) TARGET="x86_64-unknown-freebsd" ;;
  FreeBSD:aarch64|FreeBSD:arm64) TARGET="aarch64-unknown-freebsd" ;;
  *) echo "No maintained binary target for $OS/$ARCH; see BUILDING.md for source ports" >&2; exit 2 ;;
esac
[[ "$OS" != FreeBSD || "$NODE_DIR" != *' '* ]] || { echo "FreeBSD service paths cannot contain spaces" >&2; exit 2; }

as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then "$@"
  elif command -v sudo >/dev/null 2>&1; then sudo "$@"
  else echo "Root privileges are required for this service action" >&2; return 1
  fi
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$NODE_DIR/lib"
NODE_DIR="$(cd "$NODE_DIR" && pwd)"
if [[ "$OS" == Linux && ( "$NODE_DIR" == *'%'* || "$NODE_DIR" == *'$'* || "$NODE_DIR" == *'\'* || "$NODE_DIR" == *$'\r'* ) ]]; then
  echo 'Linux service paths cannot contain %, $, backslash or carriage return.' >&2
  exit 2
fi
CONFIG="$NODE_DIR/config.json"
STAMP="$(date +%Y%m%d-%H%M%S)"
if [[ -f "$CONFIG" ]] && ! grep -Eq '"network"[[:space:]]*:[[:space:]]*"paritr-mainnet"' "$CONFIG"; then
  mv "$CONFIG" "$NODE_DIR/config.pre-mainnet-backup-$STAMP.json"
  [[ ! -d "$NODE_DIR/data" ]] || mv "$NODE_DIR/data" "$NODE_DIR/data-pre-mainnet-backup-$STAMP"
  echo "Pre-mainnet data was preserved as a timestamped backup."
fi

TMP="$(mktemp -d)"
cleanup() { rm -rf -- "$TMP"; }
trap cleanup EXIT

echo "Installing Paritr $NODE_VERSION / Protocol $PROTOCOL_VERSION for $TARGET"
# Stop the existing supervised process before replacing its executable/opening
# its redb database. A failed upgrade leaves it stopped for explicit recovery.
if [[ "$OS" == Linux ]] && command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet "$SERVICE_NAME.service"; then
  as_root systemctl stop "$SERVICE_NAME.service"
elif [[ "$OS" == Darwin ]] && launchctl print "gui/$(id -u)/de.oe-net.paritr-node" >/dev/null 2>&1; then
  launchctl bootout "gui/$(id -u)" "$INSTALL_HOME/Library/LaunchAgents/de.oe-net.paritr-node.plist"
elif [[ "$OS" == FreeBSD ]] && service paritr_node status >/dev/null 2>&1; then
  as_root service paritr_node stop
fi
if [[ -f "$SCRIPT_DIR/Cargo.toml" && -d "$SCRIPT_DIR/src" ]]; then
  for tool in cargo cmake git; do command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required for a source install" >&2; exit 1; }; done
  (cd "$SCRIPT_DIR" && cargo build --release --locked)
  install -m 0755 "$SCRIPT_DIR/target/release/paritr-node" "$NODE_DIR/paritr-node"
  git clone --quiet --depth 1 --branch "$RANDOMX_TAG" https://github.com/tevador/RandomX.git "$TMP/RandomX"
  [[ "$(git -C "$TMP/RandomX" rev-parse HEAD)" == "$RANDOMX_COMMIT" ]] || { echo "RandomX tag identity mismatch" >&2; exit 1; }
  CMAKE_LINKER_FLAGS=()
  [[ "$OS" != Linux ]] || CMAKE_LINKER_FLAGS+=("-DCMAKE_SHARED_LINKER_FLAGS=-Wl,-z,noexecstack")
  cmake -S "$TMP/RandomX" -B "$TMP/RandomX/build" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON -DARCH=native "${CMAKE_LINKER_FLAGS[@]}" >/dev/null
  cmake --build "$TMP/RandomX/build" --config Release --parallel >/dev/null
  RX="$(find "$TMP/RandomX/build" -type f \( -name 'librandomx.so' -o -name 'librandomx.dylib' \) | head -n 1)"
  [[ -n "$RX" ]] || { echo "RandomX shared library build failed" >&2; exit 1; }
  install -m 0755 "$RX" "$NODE_DIR/lib/$(basename "$RX")"
  install -m 0755 "$SCRIPT_DIR/manage.sh" "$NODE_DIR/manage.sh"
else
  command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
  ARCHIVE="paritr-node-$TARGET.tar.gz"
  curl --proto '=https' --tlsv1.2 -fsSLo "$TMP/$ARCHIVE" "$SOURCE_BASE/v$NODE_VERSION/$ARCHIVE"
  curl --proto '=https' --tlsv1.2 -fsSLo "$TMP/$ARCHIVE.sha256" "$SOURCE_BASE/v$NODE_VERSION/$ARCHIVE.sha256"
  EXPECTED="$(awk 'NR==1 {print tolower($1)}' "$TMP/$ARCHIVE.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then ACTUAL="$(sha256sum "$TMP/$ARCHIVE" | awk '{print $1}')"
  else ACTUAL="$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{print $1}')"; fi
  [[ "$ACTUAL" == "$EXPECTED" ]] || { echo "Release checksum mismatch" >&2; exit 1; }
  tar -xzf "$TMP/$ARCHIVE" -C "$TMP"
  BIN="$(find "$TMP" -type f -name paritr-node | head -n 1)"
  RX="$(find "$TMP" -type f \( -name 'librandomx.so' -o -name 'librandomx.dylib' \) | head -n 1)"
  MANAGE="$(find "$TMP" -type f -name manage.sh | head -n 1)"
  FREEBSD_RC="$(find "$TMP" -type f -name paritr-node.freebsd-rc | head -n 1)"
  [[ -n "$BIN" && -n "$RX" && -n "$MANAGE" ]] || { echo "Release bundle is incomplete" >&2; exit 1; }
  install -m 0755 "$BIN" "$NODE_DIR/paritr-node"
  install -m 0755 "$RX" "$NODE_DIR/lib/$(basename "$RX")"
  install -m 0755 "$MANAGE" "$NODE_DIR/manage.sh"
  [[ -z "$FREEBSD_RC" ]] || install -m 0555 "$FREEBSD_RC" "$NODE_DIR/paritr-node.freebsd-rc"
fi

if [[ ! -f "$CONFIG" ]]; then
  PUBLIC_BIND_HOST="127.0.0.1"
  [[ -z "$PUBLIC_URL" && "$OPEN_FIREWALL" -eq 0 ]] || PUBLIC_BIND_HOST="0.0.0.0"
  INIT=("$NODE_DIR/paritr-node" --config "$CONFIG" init --public-bind "$PUBLIC_BIND_HOST:$PORT" --admin-bind "127.0.0.1:5051" --management-bind "0.0.0.0:5051" --mining-threads "$CORES" --mining-intensity "$INTENSITY" --randomx-mode "$MODE")
  [[ -z "$ADDRESS" ]] || INIT+=(--miner-address "$ADDRESS" --enable-mining)
  [[ -z "$PUBLIC_URL" ]] || INIT+=(--public-url "$PUBLIC_URL")
  "${INIT[@]}"
else
  echo "Existing Protocol-9 configuration retained: $CONFIG"
fi
chmod 0600 "$CONFIG"
if [[ -n "$PAIR_CODE" ]]; then
  "$NODE_DIR/paritr-node" --config "$CONFIG" pair --portal-url "$PORTAL_URL" --code "$PAIR_CODE" || {
    echo "Pairing failed. Retry later with manage.sh pair." >&2
  }
fi
if [[ "$(id -u)" -eq 0 && "$INSTALL_USER" != root ]]; then
  chown -R "$INSTALL_USER:$(id -gn "$INSTALL_USER")" "$NODE_DIR"
fi
"$NODE_DIR/paritr-node" --config "$CONFIG" check

if [[ "$OS" == Linux ]] && command -v systemctl >/dev/null 2>&1; then
  UNIT="$TMP/$SERVICE_NAME.service"
  cat >"$UNIT" <<EOF
[Unit]
Description=Paritr Protocol 10 full node
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$INSTALL_USER
WorkingDirectory=$NODE_DIR
ExecStart="$NODE_DIR/paritr-node" --config "$CONFIG" run
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
KillSignal=SIGINT
LimitNOFILE=65536
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths="$NODE_DIR"

[Install]
WantedBy=multi-user.target
EOF
  if command -v systemd-analyze >/dev/null 2>&1; then
    systemd-analyze verify "$UNIT"
  fi
  as_root install -m 0644 "$UNIT" "/etc/systemd/system/$SERVICE_NAME.service"
  as_root systemctl daemon-reload
  if [[ "$NO_AUTOSTART" -eq 0 ]]; then
    as_root systemctl enable "$SERVICE_NAME.service" >/dev/null
    as_root systemctl restart "$SERVICE_NAME.service"
  else
    as_root systemctl disable --now "$SERVICE_NAME.service" >/dev/null 2>&1 || true
  fi
elif [[ "$OS" == Darwin ]]; then
  PLIST="$INSTALL_HOME/Library/LaunchAgents/de.oe-net.paritr-node.plist"
  mkdir -p "$(dirname "$PLIST")"
  xml_escape() { printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'; }
  XML_NODE_DIR="$(xml_escape "$NODE_DIR")"
  XML_CONFIG="$(xml_escape "$CONFIG")"
  cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>de.oe-net.paritr-node</string>
<key>ProgramArguments</key><array><string>$XML_NODE_DIR/paritr-node</string><string>--config</string><string>$XML_CONFIG</string><string>run</string></array>
<key>WorkingDirectory</key><string>$XML_NODE_DIR</string>
<key>KeepAlive</key><true/><key>RunAtLoad</key><true/>
<key>StandardOutPath</key><string>$XML_NODE_DIR/paritr.log</string>
<key>StandardErrorPath</key><string>$XML_NODE_DIR/paritr.err.log</string>
</dict></plist>
EOF
  if [[ "$NO_AUTOSTART" -eq 0 ]]; then
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "$PLIST"
  else
    launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
  fi
else
  RC_SOURCE="$SCRIPT_DIR/packaging/paritr-node.freebsd-rc"
  [[ -f "$RC_SOURCE" ]] || RC_SOURCE="$NODE_DIR/paritr-node.freebsd-rc"
  [[ -f "$RC_SOURCE" ]] || { echo "FreeBSD rc.d script missing from bundle" >&2; exit 1; }
  as_root install -m 0555 "$RC_SOURCE" /usr/local/etc/rc.d/paritr_node
  RC_CONF="$TMP/paritr_node"
  printf 'paritr_node_enable="%s"\nparitr_node_dir="%s"\nparitr_node_config="%s"\nparitr_node_user="%s"\n' \
    "$([[ "$NO_AUTOSTART" -eq 0 ]] && printf YES || printf NO)" "$NODE_DIR" "$CONFIG" "$INSTALL_USER" >"$RC_CONF"
  as_root mkdir -p /usr/local/etc/rc.conf.d
  as_root install -m 0644 "$RC_CONF" /usr/local/etc/rc.conf.d/paritr_node
  [[ "$NO_AUTOSTART" -eq 1 ]] || as_root service paritr_node restart || as_root service paritr_node start
fi

if [[ "$OPEN_FIREWALL" -eq 1 ]]; then
  if command -v ufw >/dev/null 2>&1; then as_root ufw allow "$PORT/tcp"
  elif command -v firewall-cmd >/dev/null 2>&1; then as_root firewall-cmd --permanent --add-port="$PORT/tcp"; as_root firewall-cmd --reload
  else echo "Open TCP $PORT manually in the host firewall."; fi
fi

# The management UI and mDNS advertisement are LAN-only. When a supported
# firewall is active, allow them only from private address ranges.
if command -v ufw >/dev/null 2>&1 && as_root ufw status | grep -q '^Status: active'; then
  for subnet in 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16; do
    as_root ufw allow from "$subnet" to any port 5051 proto tcp >/dev/null
    as_root ufw allow from "$subnet" to any port 5353 proto udp >/dev/null
  done
elif command -v firewall-cmd >/dev/null 2>&1 && as_root firewall-cmd --state >/dev/null 2>&1; then
  as_root firewall-cmd --permanent --zone=home --add-port=5051/tcp >/dev/null
  as_root firewall-cmd --permanent --zone=home --add-port=5353/udp >/dev/null
  as_root firewall-cmd --reload >/dev/null
fi

trap - EXIT
cleanup
echo "Installed in $NODE_DIR. Use $NODE_DIR/manage.sh status."
"$NODE_DIR/paritr-node" --config "$CONFIG" admin-access
