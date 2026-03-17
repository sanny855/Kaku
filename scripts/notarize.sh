#!/usr/bin/env bash
set -euo pipefail

# Notarization script for Kaku macOS app
# Usage: ./scripts/notarize.sh [--staple-only]
#
# Prerequisites:
# 1. App must be signed with Developer ID
# 2. Set environment variables (or use macOS Keychain):
#    - KAKU_NOTARIZE_APPLE_ID: Your Apple ID email
#    - KAKU_NOTARIZE_TEAM_ID: Your Team ID (10 characters)
#    - KAKU_NOTARIZE_PASSWORD: App-specific password (not your Apple ID password)
#
# To generate app-specific password:
# https://appleid.apple.com/account/manage -> Sign-In and Security -> App-Specific Passwords

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_NAME="Kaku"
OUT_DIR="${OUT_DIR:-dist}"
APP_BUNDLE="${OUT_DIR}/${APP_NAME}.app"
DMG_PATH="${OUT_DIR}/${APP_NAME}.dmg"
NOTARY_SUBMIT_MAX_ATTEMPTS="${NOTARY_SUBMIT_MAX_ATTEMPTS:-3}"
NOTARY_SUBMIT_RETRY_DELAY="${NOTARY_SUBMIT_RETRY_DELAY:-20}"

STAPLE_ONLY=0
for arg in "$@"; do
	case "$arg" in
	--staple-only) STAPLE_ONLY=1 ;;
	esac
done

is_valid_team_id() {
	[[ "$1" =~ ^[A-Z0-9]{10}$ ]]
}

require_developer_id_signature() {
	local metadata
	local signed_team_id

	metadata=$(codesign -dvvvv "$APP_BUNDLE" 2>&1) || {
		echo "Error: failed to inspect app signature." >&2
		return 1
	}

	if ! grep -q "^Authority=Developer ID Application:" <<<"$metadata"; then
		echo "Error: App must be signed with a Developer ID Application certificate before notarization." >&2
		echo "$metadata" | grep -E "^(Authority=|TeamIdentifier=|Signature=)" >&2 || true
		return 1
	fi

	signed_team_id=$(echo "$metadata" | awk -F= '/^TeamIdentifier=/{print $2; exit}')
	if ! is_valid_team_id "$signed_team_id"; then
		echo "Error: App signature does not contain a valid TeamIdentifier." >&2
		echo "$metadata" | grep -E "^(Authority=|TeamIdentifier=|Signature=)" >&2 || true
		return 1
	fi
}

# Check if app exists
if [[ ! -d "$APP_BUNDLE" ]]; then
	echo "Error: $APP_BUNDLE not found. Run ./scripts/build.sh first."
	exit 1
fi

# Verify signing
if ! codesign -v "$APP_BUNDLE" 2>/dev/null; then
	echo "Error: App is not signed. Run build with KAKU_SIGNING_IDENTITY set."
	exit 1
fi

require_developer_id_signature || exit 1

echo "App: $APP_BUNDLE"
echo "DMG: $DMG_PATH"

# Get credentials from environment or Keychain
APPLE_ID="${KAKU_NOTARIZE_APPLE_ID:-}"
TEAM_ID="${KAKU_NOTARIZE_TEAM_ID:-}"
PASSWORD="${KAKU_NOTARIZE_PASSWORD:-}"

if [[ -n "$TEAM_ID" ]] && ! is_valid_team_id "$TEAM_ID"; then
	echo "Warning: ignoring invalid KAKU_NOTARIZE_TEAM_ID: $TEAM_ID"
	TEAM_ID=""
fi

# If not set via env, try to read from Keychain
if [[ -z "$APPLE_ID" ]]; then
	echo "Checking Keychain for notarization credentials..."
	APPLE_ID=$(security find-generic-password -s "kaku-notarize-apple-id" -w 2>/dev/null || true)
fi

if [[ -z "$PASSWORD" ]]; then
	PASSWORD=$(security find-generic-password -s "kaku-notarize-password" -w 2>/dev/null || true)
fi

if [[ -z "$TEAM_ID" ]]; then
	# Try to extract from signing identity
	TEAM_ID=$(codesign -dv "$APP_BUNDLE" 2>&1 | grep TeamIdentifier | head -1 | awk -F= '{print $2}')
	if [[ -n "$TEAM_ID" ]] && ! is_valid_team_id "$TEAM_ID"; then
		echo "Warning: ignoring invalid TeamIdentifier from app signature: $TEAM_ID"
		TEAM_ID=""
	fi
	if [[ -n "$TEAM_ID" ]]; then
		echo "Using Team ID from signature: $TEAM_ID"
	fi
fi

if [[ -z "$APPLE_ID" || -z "$PASSWORD" || -z "$TEAM_ID" ]]; then
	echo ""
	echo "Error: Notarization credentials not found."
	echo ""
	echo "Please set environment variables:"
	echo "  export KAKU_NOTARIZE_APPLE_ID='your-apple-id@example.com'"
	echo "  export KAKU_NOTARIZE_TEAM_ID='YOURTEAMID'"
	echo "  export KAKU_NOTARIZE_PASSWORD='xxxx-xxxx-xxxx-xxxx'"
	echo ""
	echo "Or store in Keychain:"
	echo "  security add-generic-password -s 'kaku-notarize-apple-id' -a 'kaku' -w 'your-apple-id@example.com'"
	echo "  security add-generic-password -s 'kaku-notarize-password' -a 'kaku' -w 'your-app-specific-password'"
	echo ""
	echo "To generate app-specific password: https://appleid.apple.com/account/manage"
	exit 1
fi

if [[ "$STAPLE_ONLY" == "1" ]]; then
	echo "Stapling existing notarization ticket..."

	echo "Stapling app bundle..."
	xcrun stapler staple "$APP_BUNDLE"

	if [[ -f "$DMG_PATH" ]]; then
		echo "Stapling DMG..."
		xcrun stapler staple "$DMG_PATH"
	fi

	echo "✅ Staple complete!"
	echo ""
	echo "Verifying notarization:"
	spctl -a -vv "$APP_BUNDLE" 2>&1 || true
	exit 0
fi

is_transient_notary_failure() {
	local output="$1"
	[[ "$output" == *"statusCode: Optional(500)"* ]] ||
		[[ "$output" == *"statusCode\": 500"* ]] ||
		[[ "$output" == *"code = \"UNEXPECTED_ERROR\""* ]] ||
		[[ "$output" == *"title = \"Uncaught server exception\""* ]]
}

submit_for_notarization() {
	local attempt=1
	local delay="$NOTARY_SUBMIT_RETRY_DELAY"

	while true; do
		if SUBMIT_OUTPUT=$(xcrun notarytool submit "$SUBMISSION_PATH" \
			--apple-id "$APPLE_ID" \
			--team-id "$TEAM_ID" \
			--password "$PASSWORD" \
			--wait 2>&1); then
			return 0
		fi

		if (( attempt >= NOTARY_SUBMIT_MAX_ATTEMPTS )) || ! is_transient_notary_failure "$SUBMIT_OUTPUT"; then
			return 1
		fi

		echo "Apple notarization service returned a transient 500 error (attempt ${attempt}/${NOTARY_SUBMIT_MAX_ATTEMPTS})."
		echo "Retrying in ${delay}s..."
		sleep "$delay"

		attempt=$((attempt + 1))
		delay=$((delay * 2))
	done
}

# Submit for notarization
echo "Submitting for notarization..."
echo "  Apple ID: $APPLE_ID"
echo "  Team ID: $TEAM_ID"

# Submit the DMG if it exists, otherwise submit the app
if [[ -f "$DMG_PATH" ]]; then
	SUBMISSION_PATH="$DMG_PATH"
	echo "  Submitting DMG..."
else
	SUBMISSION_PATH="$APP_BUNDLE"
	echo "  Submitting app bundle..."
fi

# Submit and capture output
echo ""
echo "Uploading to Apple notarization service (this may take a few minutes)..."
submit_for_notarization || {
	echo "Notarization submission failed:"
	echo "$SUBMIT_OUTPUT"
	exit 1
}

echo "$SUBMIT_OUTPUT"

# Check if accepted
if echo "$SUBMIT_OUTPUT" | grep -q "Accepted"; then
	echo ""
	echo "✅ Notarization accepted! Stapling ticket..."

	xcrun stapler staple "$APP_BUNDLE"

	if [[ -f "$DMG_PATH" ]]; then
		xcrun stapler staple "$DMG_PATH"
	fi

	echo ""
	echo "✅ Done! App is notarized and ready for distribution."
	echo ""
	echo "Verifying notarization:"
	spctl -a -vv "$APP_BUNDLE" 2>&1 || true
else
	echo ""
	echo "❌ Notarization failed or returned unexpected status."
	echo "Full output:"
	echo "$SUBMIT_OUTPUT"

	# Extract submission ID and fetch detailed log
	SUBMISSION_ID=$(echo "$SUBMIT_OUTPUT" | grep "id:" | head -1 | awk '{print $2}')
	if [[ -n "$SUBMISSION_ID" ]]; then
		echo ""
		echo "Fetching detailed notarization log..."
		xcrun notarytool log "$SUBMISSION_ID" \
			--apple-id "$APPLE_ID" \
			--team-id "$TEAM_ID" \
			--password "$PASSWORD" 2>&1 || true
	fi

	exit 1
fi
