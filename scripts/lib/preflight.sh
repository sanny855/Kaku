#!/usr/bin/env bash
# Shared helpers for kaku release scripts.
# Source from scripts/release.sh and scripts/prep_release.sh after REPO_ROOT is set.

LIB_RED='\033[0;31m'
LIB_GREEN='\033[0;32m'
LIB_YELLOW='\033[1;33m'
LIB_NC='\033[0m'

log_info() { echo -e "${LIB_GREEN}[INFO]${LIB_NC} $*"; }
log_warn() { echo -e "${LIB_YELLOW}[WARN]${LIB_NC} $*"; }
log_error() { echo -e "${LIB_RED}[ERROR]${LIB_NC} $*" >&2; }

die() {
    log_error "$*"
    exit 1
}

# Read package.version from kaku/Cargo.toml.
get_cargo_version() {
    local root="${1:-${REPO_ROOT:-.}}"
    grep '^version =' "$root/kaku/Cargo.toml" | head -n1 | cut -d'"' -f2
}

# Read package.version from kaku-gui/Cargo.toml.
get_kaku_gui_version() {
    local root="${1:-${REPO_ROOT:-.}}"
    grep '^version =' "$root/kaku-gui/Cargo.toml" | head -n1 | cut -d'"' -f2
}

# Verify git tree is clean, on main, in sync with origin/main.
check_clean_git() {
    log_info "Checking git status..."
    if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
        git status
        die "Working tree is not clean. Commit or stash changes before continuing."
    fi

    local branch
    branch=$(git rev-parse --abbrev-ref HEAD)
    if [[ "$branch" != "main" ]]; then
        die "Not on main branch (currently on: $branch)."
    fi

    log_info "Checking main is synchronized with origin/main..."
    git fetch origin main
    local head origin_main
    head=$(git rev-parse HEAD)
    origin_main=$(git rev-parse origin/main)
    if [[ "$head" != "$origin_main" ]]; then
        die "Local main is not synchronized with origin/main. Pull or push before continuing."
    fi
}

is_valid_semver() {
    [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

# Bump semver. Args: current_version, bump_type (patch|minor|major).
bump_semver() {
    local current="$1"
    local bump_type="$2"
    if ! is_valid_semver "$current"; then
        die "Invalid current version: $current"
    fi
    local major minor patch
    IFS='.' read -r major minor patch <<<"$current"
    case "$bump_type" in
        patch) patch=$((patch + 1)) ;;
        minor) minor=$((minor + 1)); patch=0 ;;
        major) major=$((major + 1)); minor=0; patch=0 ;;
        *) die "Unknown bump type: $bump_type (use patch|minor|major)" ;;
    esac
    printf '%d.%d.%d\n' "$major" "$minor" "$patch"
}

# Compare semver: returns 0 if a < b, 1 otherwise.
semver_lt() {
    local a="$1" b="$2"
    local a1 a2 a3 b1 b2 b3
    IFS='.' read -r a1 a2 a3 <<<"$a"
    IFS='.' read -r b1 b2 b3 <<<"$b"
    if (( a1 != b1 )); then (( a1 < b1 )); return $?; fi
    if (( a2 != b2 )); then (( a2 < b2 )); return $?; fi
    (( a3 < b3 ))
}
