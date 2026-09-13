#!/usr/bin/env bash
# Unified Paritr Protocol 9 setup for Linux, macOS and FreeBSD.
set -Eeuo pipefail

NODE_VERSION="4.0.1-rc.1"
PROTOCOL_VERSION="9"
SOURCE_BASE="${PARITR_SOURCE:-https://paritr.highactive.de/downloads}"
DEPLOYMENT="auto"
IMAGE_REF="${PARITR_IMAGE_REF:-}"
INSTALL_USER="${SUDO_USER:-$(id -un)}"
if [[ -n "${SUDO_USER:-}" ]] && command -v getent >/dev/null 2>&1; then
  INSTALL_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
else
  INSTALL_HOME="$HOME"
fi
NODE_DIR="${PARITR_DIR:-$INSTALL_HOME/paritr-node}"
PORT=5050
PORT_SET=0
ADDRESS=""
PUBLIC_URL=""
PORTAL_URL="${PARITR_PORTAL_URL:-}"
PAIR_CODE=""
CORES=0
INTENSITY=100
MODE="light"
OPEN_FIREWALL=0
NO_AUTOSTART=0
ASSUME_YES=0

usage() {
  cat <<'TXT'
Usage: setup.sh [options]
  --deployment auto|docker|native
  --image IMAGE@sha256:DIGEST    Override the release image
  --address P...                 Enable mining and use this reward address
  --port 5050                    Public RPC/P2P port
  --public-url https://...       Existing public HTTPS endpoint
  --portal-url https://...       Wallet portal URL used for pairing
  --pair-code PRTR-...           One-time wallet portal code
  --dir PATH                     Installation directory
  --source URL                   Versioned release download base
  --cores N                      Mining workers, 0 = automatic
  --intensity 5..100             Mining CPU duty cycle
  --fast | --light               RandomX mode
  --open-firewall                Open the public port where supported
  --no-autostart                 Install without starting the node
  --yes                          Non-interactive defaults/arguments
TXT
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --deployment) DEPLOYMENT="$2"; shift 2 ;;
    --image) IMAGE_REF="$2"; shift 2 ;;
    --address) ADDRESS="$2"; shift 2 ;;
    --port) PORT="$2"; PORT_SET=1; shift 2 ;;
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
    --yes|-y) ASSUME_YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

SOURCE_BASE="${SOURCE_BASE%/}"
OS="$(uname -s)"
ARCH="$(uname -m)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
cleanup() { rm -rf -- "$TMP"; }
trap cleanup EXIT

as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then "$@"
  elif command -v sudo >/dev/null 2>&1; then sudo "$@"
  else echo "Root privileges are required for: $*" >&2; return 1
  fi
}

ask_yes_no() {
  local prompt="$1" default="$2" reply
  if [[ "$ASSUME_YES" -eq 1 || ! -r /dev/tty ]]; then [[ "$default" == yes ]]; return; fi
  if [[ "$default" == yes ]]; then prompt="$prompt [Y/n] "; else prompt="$prompt [y/N] "; fi
  read -r -p "$prompt" reply </dev/tty
  reply="${reply:-$default}"
  [[ "$reply" =~ ^([Yy]|[Yy][Ee][Ss]|[Jj]|[Jj][Aa])$ ]]
}

ask_value() {
  local variable="$1" prompt="$2" default="${3:-}" value
  if [[ "$ASSUME_YES" -eq 1 || ! -r /dev/tty ]]; then return; fi
  if [[ -n "$default" ]]; then read -r -p "$prompt [$default]: " value </dev/tty
  else read -r -p "$prompt: " value </dev/tty; fi
  printf -v "$variable" '%s' "${value:-$default}"
}

verify_hash() {
  local file="$1" checksum="$2" expected actual
  expected="$(awk 'NR==1 {print tolower($1)}' "$checksum")"
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || { echo "Invalid checksum file: $checksum" >&2; return 1; }
  if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$file" | awk '{print $1}')"
  else actual="$(shasum -a 256 "$file" | awk '{print $1}')"; fi
  [[ "$actual" == "$expected" ]] || { echo "Checksum mismatch for $(basename "$file")" >&2; return 1; }
}

fetch_asset() {
  local name="$1" destination="$2" local_file
  local_file="$SCRIPT_DIR/$name"
  if [[ -f "$local_file" ]]; then
    cp "$local_file" "$destination"
    return
  fi
  command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; return 1; }
  curl --proto '=https' --tlsv1.2 -fsSLo "$destination" "$SOURCE_BASE/v$NODE_VERSION/$name"
  curl --proto '=https' --tlsv1.2 -fsSLo "$destination.sha256" "$SOURCE_BASE/v$NODE_VERSION/$name.sha256"
  verify_hash "$destination" "$destination.sha256"
}

docker_cmd() {
  if docker info >/dev/null 2>&1; then docker "$@"; else as_root docker "$@"; fi
}

install_docker_linux() {
  local installed=0
  echo "Docker is not installed. Installing the distribution-maintained engine and Compose plugin."
  if command -v apt-get >/dev/null 2>&1; then
    as_root apt-get update
    as_root apt-get install -y ca-certificates curl docker.io
    as_root apt-get install -y docker-compose-v2 || as_root apt-get install -y docker-compose-plugin
    installed=1
  elif command -v dnf >/dev/null 2>&1; then
    as_root dnf install -y moby-engine docker-compose
    installed=1
  elif command -v pacman >/dev/null 2>&1; then
    as_root pacman -Sy --needed --noconfirm docker docker-compose
    installed=1
  elif command -v zypper >/dev/null 2>&1; then
    as_root zypper --non-interactive install docker docker-compose
    installed=1
  elif command -v apk >/dev/null 2>&1; then
    as_root apk add docker docker-cli-compose
    installed=1
  fi
  [[ "$installed" -eq 1 ]] || {
    echo "No supported package manager was found. Install Docker Engine and Compose, then rerun setup." >&2
    return 1
  }
  if command -v systemctl >/dev/null 2>&1; then as_root systemctl enable --now docker
  elif command -v rc-update >/dev/null 2>&1; then as_root rc-update add docker default; as_root service docker start
  fi
}

install_build_tools() {
  case "$OS" in
    Linux)
      if command -v apt-get >/dev/null 2>&1; then as_root apt-get update; as_root apt-get install -y build-essential cmake git curl ca-certificates
      elif command -v dnf >/dev/null 2>&1; then as_root dnf install -y gcc gcc-c++ make cmake git curl ca-certificates
      elif command -v pacman >/dev/null 2>&1; then as_root pacman -Sy --needed --noconfirm base-devel cmake git curl ca-certificates
      elif command -v zypper >/dev/null 2>&1; then as_root zypper --non-interactive install -t pattern devel_basis; as_root zypper --non-interactive install cmake git curl ca-certificates
      elif command -v apk >/dev/null 2>&1; then as_root apk add build-base cmake git curl ca-certificates
      else echo "No supported package manager was found for a source build." >&2; return 1; fi
      ;;
    FreeBSD) as_root pkg install -y llvm cmake git curl bash ;;
    Darwin) xcode-select -p >/dev/null 2>&1 || xcode-select --install; command -v brew >/dev/null 2>&1 && brew install cmake git || true ;;
    *) echo "No source-build bootstrap for $OS" >&2; return 1 ;;
  esac
  if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSLo "$TMP/rustup-init.sh" https://sh.rustup.rs
    sh "$TMP/rustup-init.sh" -y --profile minimal
    export PATH="$INSTALL_HOME/.cargo/bin:$PATH"
  fi
  for tool in cargo cmake git; do command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required for the source build" >&2; return 1; }; done
}

ensure_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    case "$OS" in
      Linux) install_docker_linux ;;
      Darwin)
        if command -v brew >/dev/null 2>&1; then
          brew install --cask docker
        else
          local docker_arch docker_dmg="$TMP/Docker.dmg" docker_mount="$TMP/docker-volume"
          case "$ARCH" in arm64|aarch64) docker_arch="arm64" ;; x86_64|amd64) docker_arch="amd64" ;; *) echo "Unsupported macOS architecture: $ARCH" >&2; return 1 ;; esac
          curl --proto '=https' --tlsv1.2 -fsSLo "$docker_dmg" "https://desktop.docker.com/mac/main/$docker_arch/Docker.dmg"
          mkdir -p "$docker_mount"
          (
            as_root hdiutil attach "$docker_dmg" -nobrowse -readonly -mountpoint "$docker_mount" >/dev/null
            trap 'as_root hdiutil detach "$docker_mount" >/dev/null 2>&1 || true' EXIT
            codesign --verify --deep --strict "$docker_mount/Docker.app"
            spctl --assess --type execute "$docker_mount/Docker.app"
            as_root "$docker_mount/Docker.app/Contents/MacOS/install" --accept-license --user="$INSTALL_USER"
          )
        fi
        open -a Docker
        ;;
      *) echo "Docker deployment is not maintained on $OS; use --deployment native." >&2; return 1 ;;
    esac
  fi
  if [[ "$OS" == Darwin ]] && ! docker info >/dev/null 2>&1; then
    open -a Docker >/dev/null 2>&1 || true
    echo "Waiting for Docker Desktop..."
    local attempt
    for attempt in {1..90}; do docker info >/dev/null 2>&1 && break; sleep 2; done
  fi
  docker_cmd compose version >/dev/null 2>&1 || {
    echo "Docker Compose v2 is unavailable. Install the Compose plugin and rerun setup." >&2; return 1;
  }
}

if [[ "$ASSUME_YES" -eq 0 && -r /dev/tty ]]; then
  echo "Paritr Protocol $PROTOCOL_VERSION setup"
  echo "This wizard never asks for a wallet private key or seed phrase."
  if [[ "$DEPLOYMENT" == auto ]]; then
    DEPLOYMENT="native"
    ask_value DEPLOYMENT "Installation type (docker/native)" "$DEPLOYMENT"
  fi
  echo "Mining, wallet pairing and the device name are configured in the local browser after installation."
fi

[[ "$DEPLOYMENT" =~ ^(auto|docker|native)$ ]] || { echo "Invalid deployment: $DEPLOYMENT" >&2; exit 2; }
if [[ "$DEPLOYMENT" == auto ]]; then
  DEPLOYMENT="native"
fi
[[ "$PORT" =~ ^[0-9]+$ ]] && (( PORT > 0 && PORT < 65536 )) || { echo "Invalid port" >&2; exit 2; }
[[ "$CORES" =~ ^[0-9]+$ ]] && (( CORES <= 1024 )) || { echo "Invalid core count" >&2; exit 2; }
[[ "$INTENSITY" =~ ^[0-9]+$ ]] && (( INTENSITY >= 5 && INTENSITY <= 100 )) || { echo "Invalid intensity" >&2; exit 2; }
[[ -z "$PUBLIC_URL" || "$PUBLIC_URL" == https://* ]] || { echo "Public URL must use HTTPS" >&2; exit 2; }
[[ -z "$PORTAL_URL" || "$PORTAL_URL" == https://* ]] || { echo "Portal URL must use HTTPS" >&2; exit 2; }
[[ -z "$PAIR_CODE" || -n "$PORTAL_URL" ]] || { echo "Pairing requires a portal URL" >&2; exit 2; }
[[ -z "$PORTAL_URL" || -n "$PAIR_CODE" ]] || { echo "Portal URL requires a pairing code" >&2; exit 2; }

mkdir -p "$NODE_DIR"
NODE_DIR="$(cd "$NODE_DIR" && pwd)"
if [[ "$PORT_SET" -eq 0 && -f "$NODE_DIR/.env" ]]; then
  existing_port="$(sed -nE 's/^PARITR_PUBLIC_PORT=([0-9]+)$/\1/p' "$NODE_DIR/.env" | head -n 1)"
  [[ -z "$existing_port" ]] || PORT="$existing_port"
fi
[[ "$PORT" =~ ^[0-9]+$ ]] && (( PORT > 0 && PORT < 65536 )) || { echo "Invalid stored port" >&2; exit 2; }

install_native() {
  local installer="$TMP/install.sh" source_required=0
  case "$OS:$ARCH" in
    Linux:x86_64|Linux:amd64|Linux:aarch64|Linux:arm64|Darwin:x86_64|Darwin:arm64|Darwin:aarch64) ;;
    Linux:armv7l|Linux:armv7|Linux:riscv64|Linux:ppc64le|FreeBSD:x86_64|FreeBSD:amd64|FreeBSD:aarch64|FreeBSD:arm64) source_required=1 ;;
    *) echo "No maintained Paritr target for $OS/$ARCH" >&2; return 1 ;;
  esac
  if [[ "$source_required" -eq 1 ]]; then
    echo "Tier-2 target detected; installing build tools and compiling the audited release source."
    install_build_tools
    local source_archive="$TMP/paritr-node-$NODE_VERSION-source.tar.gz" source_tree="$TMP/source"
    fetch_asset "paritr-node-$NODE_VERSION-source.tar.gz" "$source_archive"
    mkdir -p "$source_tree"
    tar -xzf "$source_archive" -C "$source_tree" --strip-components=1
    installer="$source_tree/install.sh"
  else
    fetch_asset install.sh "$installer"
  fi
  chmod 0755 "$installer"
  local args=(--dir "$NODE_DIR" --source "$SOURCE_BASE" --port "$PORT" --cores "$CORES" --intensity "$INTENSITY")
  [[ "$MODE" == fast ]] && args+=(--fast) || args+=(--light)
  [[ -z "$ADDRESS" ]] || args+=(--address "$ADDRESS")
  [[ -z "$PUBLIC_URL" ]] || args+=(--public-url "$PUBLIC_URL")
  [[ -z "$PAIR_CODE" ]] || args+=(--portal-url "$PORTAL_URL" --pair-code "$PAIR_CODE")
  [[ "$OPEN_FIREWALL" -eq 0 ]] || args+=(--open-firewall)
  [[ "$NO_AUTOSTART" -eq 0 ]] || args+=(--no-autostart)
  bash "$installer" "${args[@]}"
  install -m 0755 "$0" "$NODE_DIR/setup.sh"
  printf 'mode=native\nversion=%s\nsource=%s\n' "$NODE_VERSION" "$SOURCE_BASE" >"$NODE_DIR/.paritr-deployment"
}

resolve_image() {
  [[ -z "$IMAGE_REF" ]] || return
  local metadata="$TMP/release.env"
  fetch_asset release.env "$metadata"
  IMAGE_REF="$(sed -nE 's/^PARITR_IMAGE_REF=([^[:space:]]+)$/\1/p' "$metadata" | head -n 1)"
  [[ "$IMAGE_REF" =~ ^[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}$ ]] || {
    echo "release.env does not contain a digest-pinned PARITR_IMAGE_REF" >&2; return 1;
  }
}

install_container() {
  [[ "$OS" != FreeBSD ]] || { echo "FreeBSD uses the native rc.d installation." >&2; return 1; }
  ensure_docker
  resolve_image
  local compose_file="$NODE_DIR/docker-compose.yml"
  fetch_asset docker-compose.release.yml "$compose_file"
  fetch_asset manage.sh "$NODE_DIR/manage.sh"
  install -m 0755 "$0" "$NODE_DIR/setup.sh"
  chmod 0755 "$NODE_DIR/manage.sh"
  local listen_host="127.0.0.1" existing_listen=""
  if [[ -f "$NODE_DIR/.env" ]]; then existing_listen="$(sed -nE 's/^PARITR_LISTEN_HOST=(127\.0\.0\.1|0\.0\.0\.0)$/\1/p' "$NODE_DIR/.env" | head -n 1)"; fi
  [[ -z "$existing_listen" ]] || listen_host="$existing_listen"
  [[ -z "$PUBLIC_URL" && "$OPEN_FIREWALL" -eq 0 ]] || listen_host="0.0.0.0"
  printf 'PARITR_IMAGE_REF=%s\nPARITR_LISTEN_HOST=%s\nPARITR_PUBLIC_PORT=%s\nPARITR_MANAGEMENT_HOST=0.0.0.0\nPARITR_MANAGEMENT_PORT=5052\n' "$IMAGE_REF" "$listen_host" "$PORT" >"$NODE_DIR/.env"
  chmod 0600 "$NODE_DIR/.env"
  printf 'mode=docker\nversion=%s\nsource=%s\n' "$NODE_VERSION" "$SOURCE_BASE" >"$NODE_DIR/.paritr-deployment"
  chmod 0600 "$NODE_DIR/.paritr-deployment"

  compose() {
    docker_cmd compose --project-directory "$NODE_DIR" --env-file "$NODE_DIR/.env" -f "$compose_file" "$@"
  }
  compose pull paritr-node
  if ! compose run --rm --no-deps --entrypoint /bin/sh paritr-node -c 'test -f /var/lib/paritr/config.json'; then
    local init_args=(run --rm --no-deps paritr-node init --public-bind "0.0.0.0:5050" --admin-bind "127.0.0.1:5051" --management-bind "0.0.0.0:5052" --mining-threads "$CORES" --mining-intensity "$INTENSITY" --randomx-mode "$MODE")
    [[ -z "$ADDRESS" ]] || init_args+=(--miner-address "$ADDRESS" --enable-mining)
    [[ -z "$PUBLIC_URL" ]] || init_args+=(--public-url "$PUBLIC_URL")
    compose "${init_args[@]}"
  else
    echo "Existing Protocol-9 container configuration retained."
  fi
  if [[ -n "$PAIR_CODE" ]]; then
    compose stop paritr-node >/dev/null 2>&1 || true
    compose run --rm --no-deps paritr-node pair --portal-url "$PORTAL_URL" --code "$PAIR_CODE" || {
      echo "Pairing failed. Retry later with manage.sh pair." >&2
    }
  fi
  compose run --rm --no-deps paritr-node check
  [[ "$NO_AUTOSTART" -eq 1 ]] || compose up -d paritr-node

  if [[ "$OPEN_FIREWALL" -eq 1 && "$OS" == Linux ]]; then
    if command -v ufw >/dev/null 2>&1; then as_root ufw allow "$PORT/tcp"
    elif command -v firewall-cmd >/dev/null 2>&1; then as_root firewall-cmd --permanent --add-port="$PORT/tcp"; as_root firewall-cmd --reload
    else echo "Open TCP $PORT manually in the host firewall."; fi
  fi
}

echo "Installing Paritr $NODE_VERSION / Protocol $PROTOCOL_VERSION ($DEPLOYMENT) on $OS/$ARCH"
if [[ "$DEPLOYMENT" == docker ]]; then install_container; else install_native; fi

trap - EXIT
cleanup
echo
echo "Installation complete: $NODE_DIR"
echo "Management: $NODE_DIR/manage.sh status"
echo "Local API:  http://127.0.0.1:$PORT"
if [[ "$DEPLOYMENT" == docker ]]; then
  compose run --rm --no-deps paritr-node admin-access
fi
echo "Private keys and wallet seed phrases are never requested by this installer."
