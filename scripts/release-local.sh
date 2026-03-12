#!/usr/bin/env bash
# 本地打包并发布到 GitHub Release
# 用法: ./scripts/release-local.sh [version]
# 如果不指定 version，自动从 Cargo.toml 读取
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# ── Config ────────────────────────────────────────────────────────────────────
VERSION="${1:-}"
OUT_DIR="${OUT_DIR:-$HOME/Downloads}"

# ── Step 1: Determine version ─────────────────────────────────────────────────
if [[ -z "$VERSION" ]]; then
    VERSION=$(grep '^version =' kaku/Cargo.toml | head -n1 | cut -d'"' -f2)
    echo "Using version from Cargo.toml: $VERSION"
fi

# Validate version format
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Invalid version format: $VERSION (expected: X.Y.Z)"
    exit 1
fi

TAG="V$VERSION"

# ── Step 2: Pre-release checks ────────────────────────────────────────────────
echo "[1/6] Running pre-release checks..."

# Check working tree is clean
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Error: Working tree is not clean. Please commit or stash changes."
    git status --short
    exit 1
fi

# Check version consistency
kaku_version=$(grep '^version =' kaku/Cargo.toml | head -n1 | cut -d'"' -f2)
gui_version=$(grep '^version =' kaku-gui/Cargo.toml | head -n1 | cut -d'"' -f2)
if [[ "$kaku_version" != "$gui_version" ]]; then
    echo "Error: Version mismatch between kaku ($kaku_version) and kaku-gui ($gui_version)"
    exit 1
fi
if [[ "$kaku_version" != "$VERSION" ]]; then
    echo "Error: Specified version ($VERSION) doesn't match Cargo.toml ($kaku_version)"
    exit 1
fi

# Check RELEASE_NOTES.md exists and version matches
if [[ ! -f ".github/RELEASE_NOTES.md" ]]; then
    echo "Error: .github/RELEASE_NOTES.md not found"
    exit 1
fi

notes_version=$(head -n1 .github/RELEASE_NOTES.md | grep -oE '[vV]?[0-9]+\.[0-9]+\.[0-9]+' | sed 's/^[vV]//' || true)
if [[ "$notes_version" != "$VERSION" ]]; then
    echo "Error: RELEASE_NOTES.md version ($notes_version) doesn't match release version ($VERSION)"
    exit 1
fi

# Check config version
echo "Checking config version..."
CONFIG_VERSION_FILE="assets/shell-integration/config_version.txt"
CONFIG_HIGHLIGHTS_FILE="assets/shell-integration/config_update_highlights.tsv"

current_config_version=$(cat "$CONFIG_VERSION_FILE" | tr -d '[:space:]')
expected_config_version=$((current_config_version + 1))

# Check if there are new highlights for the next version
if grep -q "^$expected_config_version	" "$CONFIG_HIGHLIGHTS_FILE" 2>/dev/null; then
    echo "Config version $expected_config_version found in highlights"
    echo "Updating config version: $current_config_version -> $expected_config_version"
    echo "$expected_config_version" > "$CONFIG_VERSION_FILE"
    git add "$CONFIG_VERSION_FILE" "$CONFIG_HIGHLIGHTS_FILE"
    git commit -m "chore: bump config version to $expected_config_version" || true
else
    echo "No new config highlights for version $expected_config_version (skipping config bump)"
fi

# Run tests
echo "Running tests..."
make test

# ── Step 3: Build locally ─────────────────────────────────────────────────────
echo "[2/6] Building locally with ~/.config/kaku/build.sh..."
bash ~/.config/kaku/build.sh

# Check outputs
DMG_PATH="$OUT_DIR/Kaku.dmg"
if [[ ! -f "$DMG_PATH" ]]; then
    echo "Error: Build failed - $DMG_PATH not found"
    exit 1
fi

echo "Build complete: $DMG_PATH"

# ── Step 4: Verify signature and notarization ─────────────────────────────────
echo "[3/6] Verifying signature and notarization..."

# Check signature
if ! codesign -v --deep "$DMG_PATH" 2>/dev/null; then
    echo "Warning: DMG signature verification failed"
fi

# Check notarization
if ! spctl -a -t open --context context:primary-signature "$DMG_PATH" 2>/dev/null; then
    echo "Warning: Notarization verification failed"
fi

# ── Step 5: Create GitHub Release ─────────────────────────────────────────────
echo "[4/6] Creating GitHub Release $TAG..."

# Check if tag exists
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "Tag $TAG already exists"
else
    # Create and push tag
    git tag -a "$TAG" -m "Release $TAG"
    git push origin "$TAG"
    echo "Created and pushed tag: $TAG"
fi

# Extract release notes (skip title line)
RELEASE_NOTES=$(tail -n +2 .github/RELEASE_NOTES.md)

# Create release
gh release create "$TAG" \
    --title "$TAG" \
    --notes "$RELEASE_NOTES" \
    2>/dev/null || echo "Release already exists, will update assets"

# ── Step 6: Upload assets ─────────────────────────────────────────────────────
echo "[5/6] Uploading assets to GitHub Release..."

# Calculate SHA256
SHA256=$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')

# Create update zip for auto-updater
APP_PATH="$OUT_DIR/Kaku.app"
if [[ -d "$APP_PATH" ]]; then
    rm -rf "$APP_PATH"
fi

# Extract app from DMG for update zip
hdiutil attach "$DMG_PATH" -quiet -nobrowse
MOUNT_POINT=$(hdiutil info | grep "/Volumes/Kaku" | awk '{print $NF}' | head -1)
cp -R "$MOUNT_POINT/Kaku.app" "$OUT_DIR/"
hdiutil detach "$MOUNT_POINT" -quiet

# Create update zip
UPDATE_ZIP="$OUT_DIR/kaku_for_update.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$UPDATE_ZIP"
echo "$SHA256" > "$UPDATE_ZIP.sha256"

# Upload to GitHub
gh release upload "$TAG" "$DMG_PATH" --clobber
gh release upload "$TAG" "$UPDATE_ZIP" --clobber
gh release upload "$TAG" "$UPDATE_ZIP.sha256" --clobber

echo "Assets uploaded to GitHub Release"

# ── Step 7: Trigger Homebrew tap update ───────────────────────────────────────
echo "[6/6] Triggering Homebrew tap update..."

# Check for HOMEBREW_TAP_TOKEN
if [[ -n "${HOMEBREW_TAP_TOKEN:-}" ]]; then
    gh api repos/tw93/homebrew-tap/dispatches \
        --method POST \
        --field event_type=kaku_release_published \
        --field "client_payload[version]=$VERSION" \
        --field "client_payload[sha256]=$SHA256" \
        2>/dev/null || echo "Failed to trigger tap update (token may be missing)"
else
    echo "HOMEBREW_TAP_TOKEN not set, skipping tap update trigger"
    echo "You can manually update the tap later with:"
    echo "  gh api repos/tw93/homebrew-tap/dispatches --method POST --field event_type=kaku_release_published --field client_payload[version]=$VERSION --field client_payload[sha256]=$SHA256"
fi

# ── Cleanup ───────────────────────────────────────────────────────────────────
CONFIG_VERSION=$(cat assets/shell-integration/config_version.txt | tr -d '[:space:]')
echo ""
echo "✓ Release $TAG complete!"
echo "   Config version: $CONFIG_VERSION"
echo ""
echo "Assets:"
echo "  - $DMG_PATH"
echo "  - $UPDATE_ZIP"
echo ""
echo "GitHub Release: https://github.com/tw93/Kaku/releases/tag/$TAG"
