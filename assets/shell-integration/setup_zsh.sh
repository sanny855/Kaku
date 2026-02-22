#!/bin/bash
# Kaku Zsh Setup Script
# This script configures a "batteries-included" Zsh environment using Kaku's bundled resources.
# It is designed to be safe: it backs up existing configurations and can be re-run.

set -euo pipefail

UPDATE_ONLY=false
for arg in "$@"; do
	case "$arg" in
	--update-only)
		UPDATE_ONLY=true
		;;
	esac
done

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Thin entrypoint: delegate to `kaku init` whenever possible.
# The rust command owns wrapper installation and orchestration.
if [[ "${KAKU_INIT_INTERNAL:-0}" != "1" ]]; then
	if [[ -n "${KAKU_BIN:-}" && -x "${KAKU_BIN}" ]]; then
		exec "${KAKU_BIN}" init "$@"
	fi

	for candidate in \
		"$SCRIPT_DIR/../MacOS/kaku" \
		"/Applications/Kaku.app/Contents/MacOS/kaku" \
		"$HOME/Applications/Kaku.app/Contents/MacOS/kaku"; do
		if [[ -x "$candidate" ]]; then
			exec "$candidate" init "$@"
		fi
	done
fi

# Resolve resources by script location first so setup works regardless of app install path.
# - App bundle:   setup_zsh.sh in Resources/, vendor in Resources/vendor
# - Dev checkout: setup_zsh.sh in assets/shell-integration/, vendor in assets/vendor
if [[ -d "$SCRIPT_DIR/vendor" ]]; then
	RESOURCES_DIR="$SCRIPT_DIR"
elif [[ -d "$SCRIPT_DIR/../vendor" ]]; then
	RESOURCES_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
elif [[ -d "/Applications/Kaku.app/Contents/Resources/vendor" ]]; then
	RESOURCES_DIR="/Applications/Kaku.app/Contents/Resources"
elif [[ -d "$HOME/Applications/Kaku.app/Contents/Resources/vendor" ]]; then
	RESOURCES_DIR="$HOME/Applications/Kaku.app/Contents/Resources"
else
	echo -e "${YELLOW}Error: Could not locate Kaku resources (vendor directory missing).${NC}"
	exit 1
fi

VENDOR_DIR="$RESOURCES_DIR/vendor"
TOOL_INSTALL_SCRIPT="$SCRIPT_DIR/install_cli_tools.sh"
if [[ ! -f "$TOOL_INSTALL_SCRIPT" ]]; then
	TOOL_INSTALL_SCRIPT="$RESOURCES_DIR/install_cli_tools.sh"
fi
USER_CONFIG_DIR="$HOME/.config/kaku/zsh"
KAKU_INIT_FILE="$USER_CONFIG_DIR/kaku.zsh"
STARSHIP_CONFIG="$HOME/.config/starship.toml"
ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
BACKUP_SUFFIX=".kaku-backup-$(date +%s)"
ZSHRC_BACKED_UP=0

backup_zshrc_once() {
	if [[ -f "$ZSHRC" ]] && [[ "$ZSHRC_BACKED_UP" -eq 0 ]]; then
		cp "$ZSHRC" "$ZSHRC$BACKUP_SUFFIX"
		ZSHRC_BACKED_UP=1
	fi
}

# Ensure vendor resources exist
if [[ ! -d "$VENDOR_DIR" ]]; then
	echo -e "${YELLOW}Error: Vendor resources not found in $VENDOR_DIR${NC}"
	exit 1
fi

echo -e "${BOLD}Setting up Kaku Shell Environment${NC}"

# 1. Prepare User Config Directory
mkdir -p "$USER_CONFIG_DIR"
mkdir -p "$USER_CONFIG_DIR/plugins"
mkdir -p "$USER_CONFIG_DIR/bin"

# 2. Optional external tools bootstrap (Homebrew-managed)
if [[ "${KAKU_SKIP_TOOL_BOOTSTRAP:-0}" != "1" ]]; then
	if [[ -f "$TOOL_INSTALL_SCRIPT" ]]; then
		if ! bash "$TOOL_INSTALL_SCRIPT"; then
			echo -e "${YELLOW}Warning: optional CLI tool bootstrap failed.${NC}"
		fi
	else
		echo -e "${YELLOW}Warning: missing tool bootstrap script at $TOOL_INSTALL_SCRIPT${NC}"
	fi
fi

# Validate required plugin directories up front.
# setup_zsh.sh may be run standalone, so provide a clear dependency hint.
for plugin in zsh-z zsh-autosuggestions zsh-syntax-highlighting zsh-completions; do
	if [[ ! -d "$VENDOR_DIR/$plugin" ]]; then
		echo -e "${YELLOW}Error: Missing plugin vendor directory: $VENDOR_DIR/$plugin${NC}"
		echo -e "${YELLOW}Hint: Run scripts/download_vendor.sh before setup_zsh.sh.${NC}"
		exit 1
	fi
done

# Copy Plugins
cp -R "$VENDOR_DIR/zsh-z" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-autosuggestions" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-syntax-highlighting" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-completions" "$USER_CONFIG_DIR/plugins/"
echo -e "  ${GREEN}✓${NC} ${BOLD}Tools${NC}       Installed Zsh plugins ${NC}(~/.config/kaku/zsh/plugins)${NC}"

# Copy Starship Config (if not exists)
STARSHIP_CONFIG_CREATED=false
if [[ ! -f "$STARSHIP_CONFIG" ]]; then
	if [[ -f "$VENDOR_DIR/starship.toml" ]]; then
		mkdir -p "$(dirname "$STARSHIP_CONFIG")"
		cp "$VENDOR_DIR/starship.toml" "$STARSHIP_CONFIG"
		STARSHIP_CONFIG_CREATED=true
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized starship.toml ${NC}(~/.config/starship.toml)${NC}"
	fi
fi

# Keep default prompt clean for local development.
# User-owned starship.toml should not be modified after creation.
if [[ "$STARSHIP_CONFIG_CREATED" == "true" ]] && [[ -f "$STARSHIP_CONFIG" ]]; then
	starship_cloud_patched=false

	ensure_starship_cloud_module_disabled() {
		local module="$1"
		if ! grep -Eq "^[[:space:]]*\\[$module\\]" "$STARSHIP_CONFIG"; then
			cat <<EOF >>"$STARSHIP_CONFIG"

[$module]
disabled = true
EOF
			starship_cloud_patched=true
		fi
	}

	for module in aws gcloud azure kubernetes openstack docker_context terraform; do
		ensure_starship_cloud_module_disabled "$module"
	done

	if [[ "$starship_cloud_patched" == "true" ]]; then
		echo -e "  ${GREEN}✓${NC} ${BOLD}Prompt${NC}      Initialized cloud context defaults in starship.toml"
	fi
fi

# 3. Create/Update Kaku Init File (managed by Kaku)
cat <<EOF >"$KAKU_INIT_FILE"
# Kaku Zsh Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.

export KAKU_ZSH_DIR="\$HOME/.config/kaku/zsh"

# Add Kaku managed bin to PATH (kaku wrapper and user tools)
export PATH="\$KAKU_ZSH_DIR/bin:\$PATH"

# Initialize Starship (Cross-shell prompt)
# Use system installation managed by Homebrew (or user PATH).
if command -v starship &> /dev/null; then
    eval "\$(starship init zsh)"
fi

# Enable color output for ls
export CLICOLOR=1
export LSCOLORS="Gxfxcxdxbxegedabagacad"

# Smart History Configuration
HISTSIZE="\${HISTSIZE:-50000}"
SAVEHIST="\${SAVEHIST:-50000}"
if [[ -z "\${HISTFILE:-}" ]]; then
    HISTFILE="\${ZDOTDIR:-\$HOME}/.zsh_history"
fi
setopt HIST_IGNORE_ALL_DUPS      # Remove older duplicate when new entry is added
setopt HIST_FIND_NO_DUPS         # Do not display duplicates when searching history
setopt HIST_REDUCE_BLANKS        # Remove blank lines from history
setopt HIST_IGNORE_SPACE         # Skip commands that start with a space
setopt SHARE_HISTORY             # Share history between all sessions
setopt APPEND_HISTORY            # Append history to the history file (no overwriting)
setopt INC_APPEND_HISTORY        # Write each command to history file immediately
setopt EXTENDED_HISTORY          # Include timestamps in saved history

# Set default Zsh options
setopt interactive_comments
bindkey -e

# Prefix history search on Up/Down (e.g. type "curl" then press Up)
# This is shell behavior, not terminal behavior, so Kaku configures it here.
autoload -U up-line-or-beginning-search down-line-or-beginning-search
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search
zmodload zsh/terminfo 2>/dev/null || true
for _kaku_keymap in emacs viins; do
    [[ -n "\${terminfo[kcuu1]:-}" ]] && bindkey -M "\$_kaku_keymap" "\${terminfo[kcuu1]}" up-line-or-beginning-search
    [[ -n "\${terminfo[kcud1]:-}" ]] && bindkey -M "\$_kaku_keymap" "\${terminfo[kcud1]}" down-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[[A' up-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[[B' down-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[OA' up-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[OB' down-line-or-beginning-search
done
unset _kaku_keymap

# Kaku line-selection widgets for modified arrows in prompt editing.
_kaku_select_left_char() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle backward-char
}
_kaku_select_right_char() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle forward-char
}
_kaku_select_line_start() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle beginning-of-line
}
_kaku_select_line_end() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle end-of-line
}
_kaku_has_active_region() {
    emulate -L zsh
    if (( REGION_ACTIVE )); then
        return 0
    fi
    # Ctrl-Space often sets MARK without toggling REGION_ACTIVE in emacs mode.
    if (( MARK != CURSOR )); then
        return 0
    fi
    return 1
}
_kaku_deactivate_region() {
    emulate -L zsh
    if ! _kaku_has_active_region; then
        return 1
    fi
    if (( \${+widgets[deactivate-region]} )); then
        zle deactivate-region
    else
        MARK=\$CURSOR
        REGION_ACTIVE=0
        zle redisplay
    fi
    return 0
}
_kaku_backspace_or_kill_region() {
    emulate -L zsh
    if _kaku_has_active_region; then
        zle kill-region
    else
        zle backward-delete-char
    fi
}
_kaku_delete_or_kill_region() {
    emulate -L zsh
    if _kaku_has_active_region; then
        zle kill-region
    else
        zle delete-char
    fi
}
_kaku_send_break_or_deactivate_region() {
    emulate -L zsh
    if ! _kaku_deactivate_region; then
        zle send-break
    fi
}
zle -N _kaku_select_left_char
zle -N _kaku_select_right_char
zle -N _kaku_select_line_start
zle -N _kaku_select_line_end
zle -N _kaku_backspace_or_kill_region
zle -N _kaku_delete_or_kill_region
zle -N _kaku_send_break_or_deactivate_region

# Shift+Left/Right: char expand; Shift+Home/End: to line boundary.
bindkey '^[[1;2D' _kaku_select_left_char
bindkey '^[[1;2C' _kaku_select_right_char
bindkey '^[[1;2H' _kaku_select_line_start
bindkey '^[[1;2F' _kaku_select_line_end

# Backspace/Delete: prefer deleting active region, otherwise normal behavior.
bindkey '^?' _kaku_backspace_or_kill_region
bindkey '^H' _kaku_backspace_or_kill_region
bindkey '^[[3~' _kaku_delete_or_kill_region
bindkey '^G' _kaku_send_break_or_deactivate_region

# Directory Navigation Options
setopt auto_cd
setopt auto_pushd
setopt pushd_ignore_dups
setopt pushdminus

# Common Aliases (Intuitive defaults)
alias ll='ls -lhF'   # Detailed list (human-readable sizes, no hidden files)
alias la='ls -lAhF'  # List all (including hidden, except . and ..)
alias l='ls -CF'     # Compact list

# Directory Navigation
alias ...='../..'
alias ....='../../..'
alias .....='../../../..'
alias ......='../../../../..'

alias md='mkdir -p'
alias rd=rmdir

# Grep Colors
alias grep='grep --color=auto'
alias egrep='grep -E --color=auto'
alias fgrep='grep -F --color=auto'

# Common Git Aliases (The Essentials)
alias g='git'
alias ga='git add'
alias gaa='git add --all'
alias gb='git branch'
alias gbd='git branch -d'
alias gc='git commit -v'
alias gcmsg='git commit -m'
alias gco='git checkout'
alias gcb='git checkout -b'
alias gd='git diff'
alias gds='git diff --staged'
alias gf='git fetch'
alias gl='git pull'
alias gp='git push'
alias gst='git status'
alias gss='git status -s'
alias glo='git log --oneline --decorate'
alias glg='git log --stat'
alias glgp='git log --stat -p'

# Load Plugins (Performance Optimized)

# Load zsh-completions into fpath before compinit
if [[ -d "\$KAKU_ZSH_DIR/plugins/zsh-completions/src" ]]; then
    fpath=("\$KAKU_ZSH_DIR/plugins/zsh-completions/src" \$fpath)
fi

# Optimized compinit: Use cache and only rebuild when needed (~30ms saved)
autoload -Uz compinit
if [[ -n "\${ZDOTDIR:-\$HOME}/.zcompdump"(#qN.mh+24) ]]; then
    # Rebuild completion cache if older than 24 hours
    compinit
else
    # Load from cache (much faster)
    compinit -C
fi

# Load zsh-z (smart directory jumping) - Fast, no delay needed
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh" ]]; then
    # Default to smart case matching so z kaku prefers Kaku over lowercase
    # path entries. Users can still override this in their own shell config.
    : "\${ZSHZ_CASE:=smart}"
    export ZSHZ_CASE
    source "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh"

    # z supports fuzzy directory jumps, but users also expect cd + Tab to
    # reuse visited paths in a layered way. Keep default _cd behavior and
    # only fall back to zsh-z history when filesystem completion has no match.
    # Delegate ranking/matching to zshz --complete so behavior stays aligned
    # with the plugin (frecency ordering, smart-case, future plugin changes).
    _kaku_cd_history_complete() {
        emulate -L zsh
        setopt extended_glob no_sh_word_split

        _cd
        local ret=\$?
        local nmatches="\${compstate[nmatches]:-0}"
        if (( nmatches > 0 )); then
            return \$ret
        fi

        local token="\${PREFIX:-}"
        [[ -n "\$token" ]] || return \$ret
        [[ "\$token" != -* ]] || return \$ret
        [[ "\$token" == */* ]] || return \$ret

        (( \${+functions[zshz]} )) || return \$ret

        local -a matches
        local match
        while IFS= read -r match; do
            [[ -n "\$match" ]] || continue
            matches+=("\$match")
        done < <(zshz --complete -- "\$token" 2>/dev/null)

        (( \${#matches[@]} )) || return \$ret

        compadd -Q -U -X "zsh-z history dirs" -- "\${matches[@]}"
        return 0
    }
    compdef _kaku_cd_history_complete cd
fi

# Load zsh-autosuggestions - Async, minimal impact
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh" ]]; then
    source "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh"
fi

# Smart Tab behavior:
# - Use completion while typing arguments/path-like tokens
# - Accept inline suggestion first only for the first command token
_kaku_tab_widget() {
    emulate -L zsh

    local has_suggestion=0
    if (( \${+widgets[autosuggest-accept]} )) && [[ -n "\${POSTDISPLAY:-}" ]]; then
        has_suggestion=1
    fi

    # Use completion while typing arguments (e.g. 'vim READ<Tab>')
    # and for path-like command tokens ('./scr<Tab>').
    local lbuf="\${LBUFFER}"
    local trimmed="\${lbuf#\${lbuf%%[![:space:]]*}}"
    local current_token="\${lbuf##*[[:space:]]}"

    if [[ -z "\$trimmed" || "\$trimmed" == *[[:space:]]* || "\$current_token" == */* ]]; then
        zle expand-or-complete
        return
    fi

    if (( has_suggestion )); then
        zle autosuggest-accept
    else
        zle expand-or-complete
    fi
}
zle -N _kaku_tab_widget
bindkey '^I' _kaku_tab_widget

# Defer zsh-syntax-highlighting to first prompt (~40ms saved at startup)
# This plugin must be loaded LAST, and we delay it for faster shell startup
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" ]]; then
    # Simplified highlighters for better performance (removed brackets, pattern, cursor)
    export ZSH_HIGHLIGHT_HIGHLIGHTERS=(main)

    # Defer loading until first prompt display
    zsh_syntax_highlighting_defer() {
        source "\$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"

        # Remove this hook after first run
        precmd_functions=("\${precmd_functions[@]:#zsh_syntax_highlighting_defer}")
    }

    # Hook into precmd (runs before prompt is displayed)
    precmd_functions+=(zsh_syntax_highlighting_defer)
fi

# Kaku AI fix hooks (error-only):
# - preexec captures the command text
# - precmd captures the previous command exit code
# Lua listens to these user vars and only suggests fixes when exit code != 0.
_kaku_set_user_var() {
    local name="\$1"
    local value="\$2"

    if [[ "\$TERM" != "kaku" ]]; then
        return
    fi

    if [[ "\${WEZTERM_SHELL_SKIP_USER_VARS:-}" == "1" ]]; then
        return
    fi

    local encoded=""
    if command -v base64 >/dev/null 2>&1; then
        encoded="\$(printf '%s' "\$value" | base64 | tr -d '\r\n')"
    else
        return
    fi

    if [[ -n "\${TMUX:-}" ]]; then
        printf "\033Ptmux;\033\033]1337;SetUserVar=%s=%s\007\033\\\\" "\$name" "\$encoded"
    else
        printf "\033]1337;SetUserVar=%s=%s\007" "\$name" "\$encoded"
    fi
}

# Only emit exit code when a real command was executed.
# Empty Enter should not re-trigger AI suggestions for the previous failure.
typeset -g _kaku_ai_cmd_pending=0

_kaku_ai_preexec() {
    if [[ -n "\${KAKU_AUTO_DISABLE:-}" ]]; then
        return
    fi
    _kaku_ai_cmd_pending=1
    _kaku_set_user_var "kaku_last_cmd" "\$1"
}

_kaku_ai_precmd() {
    local last_exit_code="\$?"
    if [[ -n "\${KAKU_AUTO_DISABLE:-}" ]]; then
        _kaku_ai_cmd_pending=0
        return 0
    fi
    if [[ "\${_kaku_ai_cmd_pending:-0}" != "1" ]]; then
        return 0
    fi
    _kaku_set_user_var "kaku_last_exit_code" "\$last_exit_code"
    _kaku_ai_cmd_pending=0
    return 0
}

if [[ \${preexec_functions[(Ie)_kaku_ai_preexec]} -eq 0 ]]; then
    preexec_functions+=(_kaku_ai_preexec)
fi
if [[ \${precmd_functions[(Ie)_kaku_ai_precmd]} -eq 0 ]]; then
    precmd_functions=(_kaku_ai_precmd "\${precmd_functions[@]}")
fi

# Auto-set TERM to xterm-256color for SSH connections when running under kaku,
# since remote hosts typically lack the kaku terminfo entry.
# Also auto-detect 1Password SSH agent and add IdentitiesOnly=yes to prevent
# "Too many authentication failures" caused by 1Password offering all stored keys.
# Set KAKU_SSH_SKIP_1PASSWORD_FIX=1 to disable the 1Password behavior.
ssh() {
    local -a extra_opts=()

    # 1Password SSH agent fix: auto-add IdentitiesOnly=yes to prevent
    # "Too many authentication failures" when 1Password offers all stored keys.
    # Set KAKU_SSH_SKIP_1PASSWORD_FIX=1 to disable.
    if [[ -z "\${KAKU_SSH_SKIP_1PASSWORD_FIX-}" ]]; then
        local sock="\${SSH_AUTH_SOCK:-}"
        if [[ "\$sock" == *1password* || "\$sock" == *2BUA8C4S2C* ]]; then
            local has_identitiesonly=false prev=""
            for arg in "\$@"; do
                [[ "\$prev" == "-o" && "\$arg" == IdentitiesOnly=* ]] && has_identitiesonly=true
                [[ "\$arg" == -oIdentitiesOnly=* ]] && has_identitiesonly=true
                prev="\$arg"
            done
            \$has_identitiesonly || extra_opts+=(-o "IdentitiesOnly=yes")
        fi
    fi

    if [[ "\$TERM" == "kaku" ]]; then
        TERM=xterm-256color command ssh "\${extra_opts[@]}" "\$@"
    else
        command ssh "\${extra_opts[@]}" "\$@"
    fi
}
EOF

echo -e "  ${GREEN}✓${NC} ${BOLD}Script${NC}      Generated kaku.zsh init script"

# 4. Configure .zshrc
SOURCE_LINE="[[ -f \"\$HOME/.config/kaku/zsh/kaku.zsh\" ]] && source \"\$HOME/.config/kaku/zsh/kaku.zsh\" # Kaku Shell Integration"

# Migrate legacy inline block from older versions to the single source-line model.
cleanup_legacy_inline_block() {
	if [[ ! -f "$ZSHRC" ]]; then
		return
	fi

	if ! grep -q "^# Kaku Shell Integration$" "$ZSHRC"; then
		return
	fi

	if ! grep -q "zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" "$ZSHRC"; then
		return
	fi

	if ! grep -q "KAKU_ZSH_DIR" "$ZSHRC"; then
		return
	fi

	local tmp_file
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-zshrc.XXXXXX")"

	if awk '
BEGIN { in_block = 0; saw_syntax = 0; saw_kaku_var = 0 }
{
	if (!in_block && $0 == "# Kaku Shell Integration") {
		in_block = 1
		saw_syntax = 0
		saw_kaku_var = 0
		next
	}

	if (in_block) {
		if ($0 ~ /KAKU_ZSH_DIR/) {
			saw_kaku_var = 1
			next
		}

		if ($0 ~ /zsh-syntax-highlighting\/zsh-syntax-highlighting\.zsh/) {
			saw_syntax = 1
			next
		}

		if (saw_kaku_var && saw_syntax && $0 ~ /^[[:space:]]*fi[[:space:]]*$/) {
			in_block = 0
			saw_syntax = 0
			saw_kaku_var = 0
			next
		}

		next
	}

	print
}
END {
	if (in_block) {
		exit 42
	}
}
' "$ZSHRC" >"$tmp_file"; then
		if ! cmp -s "$ZSHRC" "$tmp_file"; then
			backup_zshrc_once
			mv "$tmp_file" "$ZSHRC"
			echo -e "  ${GREEN}✓${NC} ${BOLD}Migrate${NC}     Removed legacy inline Kaku block from .zshrc"
		else
			rm -f "$tmp_file"
		fi
	else
		rm -f "$tmp_file"
		echo -e "${YELLOW}Warning: found legacy Kaku block but failed to migrate it safely; leaving .zshrc unchanged.${NC}"
	fi
}

cleanup_legacy_inline_block

has_kaku_source_line() {
	if [[ ! -f "$ZSHRC" ]]; then
		return 1
	fi

	# Prefer exact match for the managed line to avoid false positives from comments.
	if grep -Fqx "$SOURCE_LINE" "$ZSHRC"; then
		return 0
	fi

	# Fallback: accept equivalent active source lines while avoiding comment-only matches.
	grep -Eq '^[[:space:]]*\[\[ -f .+kaku/zsh/kaku\.zsh.+\]\][[:space:]]*&&[[:space:]]*source[[:space:]].*kaku/zsh/kaku\.zsh([[:space:]]|$)' "$ZSHRC"
}

# Check if the source line already exists
if has_kaku_source_line; then
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Already linked in .zshrc"
else
	# Backup existing .zshrc only if it doesn't have Kaku logic yet
	backup_zshrc_once

	# Append the single source line
	echo -e "\n$SOURCE_LINE" >>"$ZSHRC"
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Successfully patched .zshrc"
fi

# 5. Configure TouchID for Sudo (Optional)
# Reference: logic from www/mole/bin/touchid.sh
configure_touchid() {
	PAM_SUDO_FILE="/etc/pam.d/sudo"
	PAM_SUDO_LOCAL_FILE="/etc/pam.d/sudo_local"
	PAM_TID_LINE="auth       sufficient     pam_tid.so"

	# 1. Check if already enabled
	if grep -q "pam_tid.so" "$PAM_SUDO_LOCAL_FILE" 2>/dev/null || grep -q "pam_tid.so" "$PAM_SUDO_FILE" 2>/dev/null; then
		return 0
	fi

	# 2. Check compatibility (Apple Silicon or Intel Macs with TouchID)
	if ! command -v bioutil &>/dev/null; then
		# Fallback check for arm64
		if [[ "$(uname -m)" != "arm64" ]]; then
			return 0
		fi
	fi

	echo -en "\n${BOLD}TouchID for sudo${NC}  Enable fingerprint authentication? (Y/n) "
	read -p "" -n 1 -r
	# Default to Yes (proceed if reply is empty or y/Y)
	if [[ -n "$REPLY" && ! $REPLY =~ ^[Yy]$ ]]; then
		echo "" # Clear the line after Skip
		return 0
	fi
	echo "" # Move to next line for result display

	# Try the modern sudo_local method (macOS Sonoma+)
	if grep -q "sudo_local" "$PAM_SUDO_FILE" 2>/dev/null; then
		echo "# sudo_local: local customizations for sudo" | sudo tee "$PAM_SUDO_LOCAL_FILE" >/dev/null
		echo "$PAM_TID_LINE" | sudo tee -a "$PAM_SUDO_LOCAL_FILE" >/dev/null
		sudo chmod 444 "$PAM_SUDO_LOCAL_FILE"
		sudo chown root:wheel "$PAM_SUDO_LOCAL_FILE"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Sudo${NC}        Enabled via sudo_local"
	else
		# Fallback to editing /etc/pam.d/sudo
		sudo awk -v line="$PAM_TID_LINE" 'NR==2{print line} 1' "$PAM_SUDO_FILE" >"${PAM_SUDO_FILE}.tmp" &&
			sudo mv "${PAM_SUDO_FILE}.tmp" "$PAM_SUDO_FILE"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Sudo${NC}        Enabled via /etc/pam.d/sudo"
	fi
}

if [[ "$UPDATE_ONLY" != "true" ]]; then
	configure_touchid
fi
