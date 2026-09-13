#!/usr/bin/env bash
# Unified Paritr Protocol 9 management for native and Docker installations.
set -Eeuo pipefail

SERVICE_NAME="${PARITR_SERVICE:-paritr-node}"
NODE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$NODE_DIR/paritr-node"
CFG="$NODE_DIR/config.json"
STATE="$NODE_DIR/.paritr-deployment"
OS="$(uname -s)"
LABEL="de.oe-net.paritr-node"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
DEPLOYMENT="native"
SOURCE_BASE="${PARITR_SOURCE:-https://paritr.highactive.de/downloads}"
if [[ -f "$STATE" ]]; then
  value="$(sed -nE 's/^mode=(docker|native)$/\1/p' "$STATE" | head -n 1)"; [[ -z "$value" ]] || DEPLOYMENT="$value"
  value="$(sed -nE 's/^source=(https:\/\/[^[:space:]]+)$/\1/p' "$STATE" | head -n 1)"; [[ -z "$value" ]] || SOURCE_BASE="$value"
fi

as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then "$@"
  elif command -v sudo >/dev/null 2>&1; then sudo "$@"
  else echo "Root privileges are required for this action" >&2; return 1
  fi
}

docker_cmd() {
  if docker info >/dev/null 2>&1; then docker "$@"; else as_root docker "$@"; fi
}

compose() {
  docker_cmd compose --project-directory "$NODE_DIR" --env-file "$NODE_DIR/.env" -f "$NODE_DIR/docker-compose.yml" "$@"
}

need_node() {
  if [[ "$DEPLOYMENT" == docker ]]; then
    [[ -f "$NODE_DIR/docker-compose.yml" && -f "$NODE_DIR/.env" ]] || {
      echo "Incomplete Docker installation in $NODE_DIR" >&2; exit 1;
    }
  else
    [[ -x "$BIN" && -f "$CFG" ]] || { echo "Paritr Protocol 9 is not installed in $NODE_DIR" >&2; exit 1; }
  fi
}

service_action() {
  local action="$1"
  if [[ "$DEPLOYMENT" == docker ]]; then
    case "$action" in
      start) compose up -d paritr-node ;;
      stop) compose stop paritr-node ;;
      restart) compose restart paritr-node ;;
      *) return 2 ;;
    esac
    return
  fi
  case "$OS" in
    Linux) as_root systemctl "$action" "$SERVICE_NAME.service" ;;
    Darwin)
      case "$action" in
        start) launchctl print "gui/$(id -u)/$LABEL" >/dev/null 2>&1 || launchctl bootstrap "gui/$(id -u)" "$PLIST"; launchctl kickstart "gui/$(id -u)/$LABEL" ;;
        stop) launchctl bootout "gui/$(id -u)" "$PLIST" ;;
        restart) launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true; launchctl bootstrap "gui/$(id -u)" "$PLIST" ;;
        *) return 2 ;;
      esac
      ;;
    FreeBSD) as_root service paritr_node "$action" ;;
    *) echo "Unsupported service manager on $OS" >&2; return 1 ;;
  esac
}

is_running() {
  if [[ "$DEPLOYMENT" == docker ]]; then
    [[ "$(compose ps --status running --quiet paritr-node 2>/dev/null)" != "" ]]
    return
  fi
  case "$OS" in
    Linux) systemctl is-active --quiet "$SERVICE_NAME.service" ;;
    Darwin) launchctl print "gui/$(id -u)/$LABEL" >/dev/null 2>&1 ;;
    FreeBSD) service paritr_node status >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}

public_port() {
  if [[ "$DEPLOYMENT" == docker ]]; then
    sed -nE 's/^PARITR_PUBLIC_PORT=([0-9]+)$/\1/p' "$NODE_DIR/.env" | head -n 1
  else
    sed -nE 's/^[[:space:]]*"public_bind"[[:space:]]*:[[:space:]]*"[^\"]*:([0-9]+)".*/\1/p' "$CFG" | head -n 1
  fi
}

show_status_api() {
  local port
  port="$(public_port)"; port="${port:-5050}"
  if command -v curl >/dev/null 2>&1; then
    curl --connect-timeout 2 --max-time 5 -fsS "http://127.0.0.1:$port/status" || true
    echo
  fi
}

node_command() {
  if [[ "$DEPLOYMENT" == docker ]]; then compose run --rm --no-deps paritr-node "$@"
  else "$BIN" --config "$CFG" "$@"; fi
}

update_installation() {
  command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; return 1; }
  local temporary installer checksum expected actual result=0
  temporary="$(mktemp -d)"; installer="$temporary/setup.sh"; checksum="$installer.sha256"
  trap 'rm -rf -- "$temporary"' RETURN EXIT
  curl --proto '=https' --tlsv1.2 -fsSLo "$installer" "$SOURCE_BASE/setup.sh"
  curl --proto '=https' --tlsv1.2 -fsSLo "$checksum" "$SOURCE_BASE/setup.sh.sha256"
  expected="$(awk 'NR==1 {print tolower($1)}' "$checksum")"
  if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$installer" | awk '{print $1}')"
  else actual="$(shasum -a 256 "$installer" | awk '{print $1}')"; fi
  [[ "$expected" =~ ^[0-9a-f]{64}$ && "$actual" == "$expected" ]] || { echo "Setup checksum mismatch" >&2; return 1; }
  bash "$installer" --deployment "$DEPLOYMENT" --dir "$NODE_DIR" --source "$SOURCE_BASE" --port "$(public_port)" --yes || result=$?
  trap - RETURN EXIT
  rm -rf -- "$temporary"
  return "$result"
}

backup_installation() {
  local backup_dir archive stamp was_running=0 result=0
  backup_dir="${PARITR_BACKUP_DIR:-$NODE_DIR/backups}"
  mkdir -p "$backup_dir"
  stamp="$(date +%Y%m%d-%H%M%S)"
  archive="$backup_dir/paritr-mainnet-$stamp.tar.gz"
  is_running && was_running=1
  [[ "$was_running" -eq 0 ]] || service_action stop
  if [[ "$DEPLOYMENT" == docker ]]; then
    compose run --rm --no-deps -T --entrypoint tar paritr-node -C /var/lib/paritr -czf - . >"$archive" || result=$?
  else
    tar -C "$NODE_DIR" -czf "$archive" config.json data || result=$?
  fi
  [[ "$was_running" -eq 0 ]] || service_action start
  if [[ "$result" -ne 0 ]]; then echo "Backup failed; incomplete file: $archive" >&2; return "$result"; fi
  chmod 0600 "$archive"
  echo "Backup written: $archive"
}

doctor() {
  need_node
  echo "Deployment: $DEPLOYMENT"
  echo "Directory:  $NODE_DIR"
  if [[ "$DEPLOYMENT" == docker ]]; then
    docker_cmd version --format 'Docker: {{.Server.Version}}'
    compose config --quiet
    compose ps
  else
    echo "Binary:     $($BIN --version)"
  fi
  node_command check
  show_status_api
}

cmd="${1:-help}"
case "$cmd" in
  start|stop|restart) need_node; service_action "$cmd" ;;
  status)
    need_node
    if is_running; then echo "Service: running"; else echo "Service: stopped"; fi
    [[ "$DEPLOYMENT" != docker ]] || compose ps
    show_status_api
    ;;
  logs)
    need_node
    if [[ "$DEPLOYMENT" == docker ]]; then compose logs --tail 200 -f paritr-node
    else
      case "$OS" in
        Linux) journalctl -u "$SERVICE_NAME.service" -n 200 -f ;;
        Darwin) touch "$NODE_DIR/paritr.log" "$NODE_DIR/paritr.err.log"; tail -F "$NODE_DIR/paritr.log" "$NODE_DIR/paritr.err.log" ;;
        FreeBSD) touch "$NODE_DIR/paritr.log"; tail -F "$NODE_DIR/paritr.log" ;;
        *) exit 1 ;;
      esac
    fi
    ;;
  check) need_node; node_command check ;;
  doctor) doctor ;;
  update) need_node; update_installation ;;
  backup) need_node; backup_installation ;;
  pair)
    need_node
    code="${2:-}"; portal="${3:-${PARITR_PORTAL_URL:-}}"
    [[ -n "$code" ]] || read -rp "Pairing code: " code
    [[ -n "$portal" ]] || read -rp "Portal HTTPS URL: " portal
    [[ "$portal" == https://* ]] || { echo "An HTTPS portal URL is required" >&2; exit 2; }
    running=0; is_running && running=1
    [[ "$running" -eq 0 ]] || service_action stop
    result=0; node_command pair --portal-url "$portal" --code "$code" || result=$?
    [[ "$running" -eq 0 ]] || service_action start
    exit "$result"
    ;;
  unpair)
    need_node
    running=0; is_running && running=1
    [[ "$running" -eq 0 ]] || service_action stop
    result=0; node_command unpair || result=$?
    [[ "$running" -eq 0 ]] || service_action start
    exit "$result"
    ;;
  config) need_node; node_command show-config ;;
  access) need_node; node_command admin-access ;;
  help|*)
    cat <<'TXT'
Paritr Protocol 9 management

  ./manage.sh status
  ./manage.sh start|stop|restart
  ./manage.sh logs
  ./manage.sh check|doctor
  ./manage.sh pair <PAIRING-CODE> <https://portal.example>
  ./manage.sh unpair
  ./manage.sh config
  ./manage.sh access
  ./manage.sh update
  ./manage.sh backup
TXT
    ;;
esac
