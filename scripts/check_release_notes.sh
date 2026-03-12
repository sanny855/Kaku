#!/usr/bin/env bash
# Check if RELEASE_NOTES.md version matches Cargo.toml version
# Usage: ./check_release_notes.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RELEASE_NOTES="$REPO_ROOT/.github/RELEASE_NOTES.md"

# Get version from Cargo.toml
cargo_version="$(grep '^version =' "$REPO_ROOT/kaku/Cargo.toml" | head -n1 | cut -d'"' -f2)"

if [[ ! -f "$RELEASE_NOTES" ]]; then
    echo "❌ RELEASE_NOTES.md not found at $RELEASE_NOTES" >&2
    exit 1
fi

# Extract version from RELEASE_NOTES.md title (format: # V0.7.0 or # 0.7.0)
notes_version=$(head -n1 "$RELEASE_NOTES" | grep -oE '[vV]?[0-9]+\.[0-9]+\.[0-9]+' | sed 's/^[vV]//' || true)

if [[ -z "$notes_version" ]]; then
    echo "❌ Could not extract version from RELEASE_NOTES.md title" >&2
    echo "   Expected format: '# V0.7.0' or '# 0.7.0'" >&2
    exit 1
fi

if [[ "$notes_version" != "$cargo_version" ]]; then
    echo "❌ Version mismatch!" >&2
    echo "   Cargo.toml:  $cargo_version" >&2
    echo "   RELEASE_NOTES: $notes_version" >&2
    exit 1
fi

echo "✓ RELEASE_NOTES.md version matches: v$cargo_version"
