#!/usr/bin/env sh
# Stable one-command bootstrap. The downloaded setup script performs all prompts.
set -eu

source_base="${PARITR_SOURCE:-https://paritr.highactive.de/downloads}"
temporary="$(mktemp -d)"
cleanup() { rm -rf -- "$temporary"; }
trap cleanup EXIT HUP INT TERM

curl --proto '=https' --tlsv1.2 -fsSLo "$temporary/setup.sh" "${source_base%/}/setup.sh"
curl --proto '=https' --tlsv1.2 -fsSLo "$temporary/setup.sh.sha256" "${source_base%/}/setup.sh.sha256"
expected="$(awk 'NR==1 {print tolower($1)}' "$temporary/setup.sh.sha256")"
case "$expected" in *[!0-9a-f]*|'') echo 'Invalid setup checksum' >&2; exit 1 ;; esac
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$temporary/setup.sh" | awk '{print $1}')"
else
  actual="$(shasum -a 256 "$temporary/setup.sh" | awk '{print $1}')"
fi
[ "${#expected}" -eq 64 ] && [ "$actual" = "$expected" ] || { echo 'Setup checksum mismatch' >&2; exit 1; }
chmod 0755 "$temporary/setup.sh"
bash "$temporary/setup.sh" "$@"
