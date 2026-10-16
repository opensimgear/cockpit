#!/usr/bin/env bash
# Activate the existing pinned SDK for one command; do not modify shell profiles.
set -e
app_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$(cd -- "$app_root/../.." && pwd)"
cached_sdk="$repo_root/.research/esp32-idf-v5.5.2"
sdk="${OSG_ESP_IDF_PATH:-${IDF_PATH:-$cached_sdk}}"
if [[ ! -f "$sdk/export.sh" ]]; then
    printf '%s\n' 'ESP-IDF v5.5.2 not found. Set OSG_ESP_IDF_PATH to your installation.' >&2
    exit 1
fi
if [[ "$sdk" == "$cached_sdk" && -z "${IDF_TOOLS_PATH:-}" ]]; then
    export IDF_TOOLS_PATH="$repo_root/.research/esp32-idf-tools"
fi
source "$sdk/export.sh"
cd -- "$app_root"
exec python "$sdk/tools/idf.py" "$@"
