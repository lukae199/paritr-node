#!/usr/bin/env bash
# Validate GitHub release assets and create an upload-ready download tree.
set -Eeuo pipefail

usage() { echo "Usage: $0 VERSION ARTIFACT_DIRECTORY OUTPUT_DIRECTORY" >&2; }
[[ $# -eq 3 ]] || { usage; exit 2; }
VERSION="${1#v}"
ARTIFACT_DIR="$2"
OUTPUT_DIR="$3"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$ ]] || { echo "Invalid version" >&2; exit 2; }
[[ -d "$ARTIFACT_DIR" ]] || { echo "Artifact directory not found: $ARTIFACT_DIR" >&2; exit 2; }
[[ ! -e "$OUTPUT_DIR" ]] || { echo "Output already exists: $OUTPUT_DIR" >&2; exit 2; }

for required in bootstrap.sh bootstrap.ps1 setup.sh setup.ps1 install.sh install.ps1 \
  manage.sh manage.ps1 docker-compose.release.yml release.env SHA256SUMS; do
  [[ -f "$ARTIFACT_DIR/$required" ]] || { echo "Missing release asset: $required" >&2; exit 1; }
done

(
  cd "$ARTIFACT_DIR"
  sha256sum -c SHA256SUMS
)

mkdir -p "$OUTPUT_DIR/v$VERSION"
cp -a "$ARTIFACT_DIR/." "$OUTPUT_DIR/v$VERSION/"
for current in bootstrap.sh bootstrap.ps1 setup.sh setup.ps1; do
  cp "$ARTIFACT_DIR/$current" "$OUTPUT_DIR/$current"
  cp "$ARTIFACT_DIR/$current.sha256" "$OUTPUT_DIR/$current.sha256"
done
printf '%s\n' "$VERSION" >"$OUTPUT_DIR/STABLE"

echo "Prepared: $OUTPUT_DIR"
echo "Publish its contents as the HTTPS download root without modifying v$VERSION."
