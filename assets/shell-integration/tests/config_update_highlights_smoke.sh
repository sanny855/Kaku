#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../state_common.sh
source "$SCRIPT_DIR/state_common.sh"

output="$(print_config_update_highlights "$SCRIPT_DIR" 12 15)"

[[ "$output" != *"  v12"* ]]
[[ "$output" != *"  v13"* ]]
[[ "$output" != *"  v14"* ]]
[[ "$output" == *"Shell integration compatibility is improved for SSH"* ]]
[[ "$output" == *"Starship prompt and AI shell hooks are more reliable"* ]]
[[ "$output" == *"regenerate the managed script correctly"* ]]
[[ "$output" == *"Yazi now follows Kaku dark and light themes automatically"* ]]

echo "config_update_highlights smoke test passed"
