#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
arch=$(uname -m)
if [[ "$arch" != "x86_64" ]]; then
    printf 'AppImage packaging currently supports x86_64; detected %s.\n' "$arch" >&2
    exit 1
fi

for tool in cargo curl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf '%s was not found. Install it before building the AppImage.\n' "$tool" >&2
        exit 1
    fi
done

tools_dir="${XDG_CACHE_HOME:-$HOME/.cache}/viperdiskinfo/appimage-tools"
linuxdeploy="$tools_dir/linuxdeploy-x86_64.AppImage"
appimage_dir="$repo_root/target/appimage"
appdir="$appimage_dir/AppDir"
app_icon="$appimage_dir/viperdiskinfo.png"
output="$appimage_dir/Viper-Disk-Info-x86_64.AppImage"
version=$(awk -F '"' '/^version = / { print $2; exit }' "$repo_root/Cargo.toml")

mkdir -p "$tools_dir" "$appimage_dir"
if [[ ! -x "$linuxdeploy" ]]; then
    curl --fail --location --retry 3 \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage \
        --output "$linuxdeploy"
    chmod +x "$linuxdeploy"
fi

cargo build --release --locked --manifest-path "$repo_root/Cargo.toml"
cp "$repo_root/assets/logo.png" "$app_icon"
rm -rf "$appdir"

cd "$appimage_dir"
LDAI_OUTPUT="$output" \
LDAI_VERSION="$version" \
APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy" \
    --appdir "$appdir" \
    --executable "$repo_root/target/release/viperdiskinfo" \
    --desktop-file "$repo_root/packaging/viperdiskinfo.desktop" \
    --icon-file "$app_icon" \
    --output appimage
printf 'Built %s\n' "$output"
