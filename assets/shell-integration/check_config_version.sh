#!/bin/bash
# Kaku config version check

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

CURRENT_CONFIG_VERSION=8
CONFIG_DIR="$HOME/.config/kaku"
STATE_FILE="$CONFIG_DIR/state.json"
LEGACY_VERSION_FILE="$CONFIG_DIR/.kaku_config_version"
LEGACY_GEOMETRY_FILE="$CONFIG_DIR/.kaku_window_geometry"

read_config_version() {
	if [[ ! -f "$STATE_FILE" ]]; then
		printf '%s\n' "0"
		return
	fi

	local version
	version="$(sed -nE 's/.*"config_version"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
	if [[ "$version" =~ ^[0-9]+$ ]]; then
		printf '%s\n' "$version"
	else
		printf '%s\n' "0"
	fi
}

persist_config_version() {
	local target_version="${1:-$CURRENT_CONFIG_VERSION}"
	mkdir -p "$CONFIG_DIR"

	local width height geometry_json
	width=""
	height=""
	geometry_json=""

	if [[ -f "$STATE_FILE" ]]; then
		width="$(sed -nE 's/.*"width"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
		height="$(sed -nE 's/.*"height"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
	fi

	if [[ -z "$width" || -z "$height" ]] && [[ -f "$LEGACY_GEOMETRY_FILE" ]]; then
		local geometry
		geometry="$(tr -d '[:space:]' < "$LEGACY_GEOMETRY_FILE" || true)"
		local a b c d
		IFS=',' read -r a b c d <<< "$geometry"
		if [[ "${c:-}" =~ ^[0-9]+$ && "${d:-}" =~ ^[0-9]+$ ]]; then
			width="$c"
			height="$d"
		elif [[ "${a:-}" =~ ^[0-9]+$ && "${b:-}" =~ ^[0-9]+$ ]]; then
			width="$a"
			height="$b"
		fi
	fi

	if [[ -n "$width" && -n "$height" ]]; then
		geometry_json="$(printf ',\n  "window_geometry": {\n    "width": %s,\n    "height": %s\n  }' "$width" "$height")"
	fi

	printf "{\n  \"config_version\": %s%s\n}\n" "$target_version" "$geometry_json" >"$STATE_FILE"

	rm -f "$LEGACY_VERSION_FILE" "$LEGACY_GEOMETRY_FILE"
}

detect_login_shell() {
	if [[ -n "${SHELL:-}" && -x "${SHELL:-}" ]]; then
		printf '%s\n' "$SHELL"
		return
	fi

	local current_user resolved_shell passwd_entry
	current_user="${USER:-}"
	if [[ -z "$current_user" ]]; then
		current_user="$(id -un 2>/dev/null || true)"
	fi

	if [[ -n "$current_user" ]] && command -v dscl &>/dev/null; then
		resolved_shell="$(dscl . -read "/Users/$current_user" UserShell 2>/dev/null | awk '/UserShell:/ { print $2 }')"
		if [[ -n "$resolved_shell" && -x "$resolved_shell" ]]; then
			printf '%s\n' "$resolved_shell"
			return
		fi
	fi

	if [[ -n "$current_user" ]] && command -v getent &>/dev/null; then
		passwd_entry="$(getent passwd "$current_user" 2>/dev/null || true)"
		resolved_shell="${passwd_entry##*:}"
		if [[ -n "$resolved_shell" && -x "$resolved_shell" ]]; then
			printf '%s\n' "$resolved_shell"
			return
		fi
	fi

	if [[ -x "/bin/zsh" ]]; then
		printf '%s\n' "/bin/zsh"
	else
		printf '%s\n' "/bin/sh"
	fi
}

# Determine resource dir
if [[ -d "/Applications/Kaku.app/Contents/Resources" ]]; then
	RESOURCE_DIR="/Applications/Kaku.app/Contents/Resources"
else
	RESOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

user_version="$(read_config_version)"

if [[ $user_version -eq 0 && ! -f "$STATE_FILE" ]]; then
	if [[ -f "$LEGACY_VERSION_FILE" || -f "$LEGACY_GEOMETRY_FILE" ]]; then
		legacy_version=0
		if [[ -f "$LEGACY_VERSION_FILE" ]]; then
			candidate="$(tr -d '[:space:]' < "$LEGACY_VERSION_FILE" || true)"
			if [[ "$candidate" =~ ^[0-9]+$ ]]; then
				legacy_version="$candidate"
			fi
		fi

		if [[ $legacy_version -eq 0 ]]; then
			legacy_version="$CURRENT_CONFIG_VERSION"
		fi

		persist_config_version "$legacy_version"
		user_version="$legacy_version"
	fi
fi

# Corrupted state file fallback: repair and continue with safe defaults.
if [[ -f "$STATE_FILE" && $user_version -eq 0 ]]; then
	persist_config_version
	user_version="$CURRENT_CONFIG_VERSION"
fi

# Skip if already up to date or new user
if [[ $user_version -eq 0 || $user_version -ge $CURRENT_CONFIG_VERSION ]]; then
	exit 0
fi

echo -e "${BOLD}Kaku config update available!${NC} v$user_version -> v$CURRENT_CONFIG_VERSION"
echo ""

# Show what's new
echo -e "${BOLD}What's new:${NC}"
if [[ $user_version -lt 2 ]]; then
	echo "  • 40% faster ZSH startup"
	echo "  • Deferred syntax highlighting"
	echo "  • Delta - syntax-highlighted git diffs"
	echo "  • Better aliases"
fi
if [[ $user_version -lt 3 ]]; then
	echo "  • More reliable setup path detection"
	echo "  • Respect ZDOTDIR when patching .zshrc"
	echo "  • Prevent repeated first-run onboarding loops"
fi
if [[ $user_version -lt 4 ]]; then
	echo "  • Delta defaults to side-by-side with line numbers"
	echo "  • Mouse wheel scrolling enabled in diff pager"
	echo "  • Cleaner file labels and theme-aligned highlighting"
fi
if [[ $user_version -lt 5 ]]; then
	echo "  • Refined diff header display to avoid duplicate file hints"
	echo "  • Updated Delta default theme and label readability"
	echo "  • Better protection for user custom kaku.lua during onboarding"
fi
if [[ $user_version -lt 6 ]]; then
	echo "  • Added zsh-completions to default shell setup"
	echo "  • Richer command and subcommand Tab completion coverage"
	echo "  • Tab now accepts inline autosuggestions first"
	echo "  • If no suggestion is shown, Tab still performs normal completion"
fi
if [[ $user_version -lt 7 ]]; then
	echo "  • Migrate legacy inline Kaku shell blocks out of .zshrc"
	echo "  • Keep only one Kaku source line in .zshrc"
	echo "  • Hide default cloud context segments in Starship prompt"
fi
if [[ $user_version -lt 8 ]]; then
	echo "  • Preserve complete Zsh history persistence across sessions"
	echo "  • Respect ZDOTDIR and existing HISTFILE/HISTSIZE defaults"
	echo "  • Write history entries immediately with timestamps"
fi
echo ""

read -p "Apply update? [Y/n] " -n 1 -r
echo

if [[ $REPLY =~ ^[Nn]$ ]]; then
	persist_config_version
	echo -e "${YELLOW}Skipped${NC}"
	echo ""
	echo "Press any key to continue..."
	read -n 1 -s
	exit 0
fi

# Apply updates
if [[ -f "$RESOURCE_DIR/setup_zsh.sh" ]]; then
	bash "$RESOURCE_DIR/setup_zsh.sh" --update-only
fi

if ! command -v delta &>/dev/null; then
	if [[ -f "$RESOURCE_DIR/install_delta.sh" ]]; then
		read -p "Install Delta for better git diffs? [Y/n] " -n 1 -r
		echo
		if [[ ! $REPLY =~ ^[Nn]$ ]]; then
			if ! bash "$RESOURCE_DIR/install_delta.sh"; then
				echo ""
				echo -e "${YELLOW}Delta installation failed.${NC}"
				echo "You can retry later with:"
				echo "  bash \"$RESOURCE_DIR/install_delta.sh\""
			fi
		fi
	fi
fi

persist_config_version

echo ""
echo -e "\033[1;32m🎃 Kaku environment is ready! Enjoy coding.\033[0m"
echo ""
echo "Press any key to continue..."
read -n 1 -s

# Start a new shell instead of exiting
TARGET_SHELL="$(detect_login_shell)"
exec "$TARGET_SHELL" -l
