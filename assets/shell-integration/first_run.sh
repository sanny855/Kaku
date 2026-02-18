#!/bin/bash
# Kaku First Run Experience
# This script is launched automatically on the first run of Kaku.

set -euo pipefail

CURRENT_CONFIG_VERSION=10
CONFIG_DIR="$HOME/.config/kaku"
STATE_FILE="$CONFIG_DIR/state.json"
LEGACY_VERSION_FILE="$CONFIG_DIR/.kaku_config_version"
LEGACY_GEOMETRY_FILE="$CONFIG_DIR/.kaku_window_geometry"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_SCRIPT="$SCRIPT_DIR/state_common.sh"

if [[ ! -f "$COMMON_SCRIPT" ]]; then
	echo "Error: missing shared state script: $COMMON_SCRIPT"
	exit 1
fi
# shellcheck source=state_common.sh
source "$COMMON_SCRIPT"

# Always persist config version at script exit to avoid repeated onboarding loops
# when optional setup steps fail on user machines.
trap persist_config_version EXIT

# Resources directory resolution
if [[ -f "$SCRIPT_DIR/setup_zsh.sh" ]]; then
	RESOURCES_DIR="$SCRIPT_DIR"
elif [[ -f "/Applications/Kaku.app/Contents/Resources/setup_zsh.sh" ]]; then
	RESOURCES_DIR="/Applications/Kaku.app/Contents/Resources"
elif [[ -f "$HOME/Applications/Kaku.app/Contents/Resources/setup_zsh.sh" ]]; then
	RESOURCES_DIR="$HOME/Applications/Kaku.app/Contents/Resources"
else
	# Fallback for dev environment
	RESOURCES_DIR="$SCRIPT_DIR"
fi

SETUP_SCRIPT="$RESOURCES_DIR/setup_zsh.sh"
TOOLS_SCRIPT="$RESOURCES_DIR/install_cli_tools.sh"

# Clear screen
clear

# Display Welcome Message
echo -e "\033[1;35m"
echo "  _  __      _          "
echo " | |/ /     | |         "
echo " | ' / __ _ | | __ _   _ "
echo " |  < / _\` || |/ /| | | |"
echo " | . \ (_| ||   < | |_| |"
echo " |_|\_\__,_||_|\_\ \__,_|"
echo -e "\033[0m"
echo "Welcome to Kaku!"
echo "A fast, out-of-the-box terminal built for AI coding."
echo "--------------------------------------------------------"
echo "Would you like to install Kaku's enhanced shell features?"
echo "This includes:"
echo "  - z - Smart Directory Jumper"
echo "  - zsh-completions - Rich Tab Completions"
echo "  - Zsh Syntax Highlighting"
echo "  - Zsh Autosuggestions"
echo "  - Optional CLI tools via Homebrew: Starship, Delta, Lazygit"
echo ""
echo "Shell config model:"
echo "  - Kaku writes managed shell config to ~/.config/kaku/zsh/kaku.zsh"
echo "  - .zshrc only gets one source line"
echo "  - You can roll back anytime with: kaku reset"
echo "--------------------------------------------------------"
echo ""

# Interactive Prompt
read -p "Install enhanced shell features? [Y/n] " -n 1 -r
echo ""

INSTALL_SHELL=false
if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
	INSTALL_SHELL=true
fi

# Kaku Theme Prompt
echo "--------------------------------------------------------"
echo "Would you like to use the Kaku Theme?"
echo "A modern, high-contrast dark theme optimized for AI coding."
echo "Perfect for Claude, Codex, and late-night hacking."
echo "--------------------------------------------------------"
read -p "Apply Kaku Theme? [Y/n] " -n 1 -r
echo ""

INSTALL_THEME=false
if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
	INSTALL_THEME=true
fi

# Process Shell Features
if [[ "$INSTALL_SHELL" == "true" ]]; then
	if [[ -f "$SETUP_SCRIPT" ]]; then
		if ! KAKU_SKIP_TOOL_BOOTSTRAP=1 bash "$SETUP_SCRIPT"; then
			echo ""
			echo "Warning: shell setup failed. You can retry manually:"
			echo "  bash \"$SETUP_SCRIPT\""
		fi
	else
		echo "Error: setup_zsh.sh not found at $SETUP_SCRIPT"
	fi
else
	echo ""
	echo "Skipping shell setup. You can run it manually later:"
	echo "$SETUP_SCRIPT"
fi

mkdir -p "$CONFIG_DIR"

resolve_kaku_cli() {
	local candidates=(
		"$RESOURCES_DIR/../MacOS/kaku"
		"/Applications/Kaku.app/Contents/MacOS/kaku"
		"$HOME/Applications/Kaku.app/Contents/MacOS/kaku"
	)

	local candidate
	for candidate in "${candidates[@]}"; do
		if [[ -x "$candidate" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	if command -v kaku >/dev/null 2>&1; then
		command -v kaku
		return 0
	fi

	return 1
}

ensure_user_config_via_cli() {
	local kaku_lua_dest="$HOME/.config/kaku/kaku.lua"
	if [[ -f "$kaku_lua_dest" ]]; then
		echo "Keeping existing user config: $kaku_lua_dest"
		return 0
	fi

	local kaku_bin
	if ! kaku_bin="$(resolve_kaku_cli)"; then
		echo "Warning: kaku CLI not found, skipped config initialization."
		return 0
	fi

	if "$kaku_bin" config --ensure-only >/dev/null 2>&1; then
		echo "Created minimal user config: $kaku_lua_dest"
	else
		echo "Warning: failed to initialize user config via '$kaku_bin config --ensure-only'."
	fi
}

# Process Kaku Theme
if [[ "$INSTALL_THEME" == "true" ]]; then
	ensure_user_config_via_cli
fi

# Process optional CLI tool installation (single prompt)
if [[ "$INSTALL_SHELL" == "true" ]]; then
	if [[ -f "$TOOLS_SCRIPT" ]]; then
		echo ""
		if ! bash "$TOOLS_SCRIPT"; then
			echo "Warning: optional tool installation failed."
		fi
	else
		echo "Warning: install_cli_tools.sh not found at $TOOLS_SCRIPT"
	fi
fi

echo -e "\n\033[1;32m🎃 Kaku environment is ready! Enjoy coding.\033[0m"

# `exec` replaces the shell process and skips EXIT trap handlers.
# Persist explicitly here so successful first-run/upgrade paths are recorded.
persist_config_version

# Replace current process with the user's login shell
TARGET_SHELL="$(detect_login_shell)"
exec "$TARGET_SHELL" -l
