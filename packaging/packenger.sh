#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
action=${1:-}

usage() {
    printf 'Usage: %s {install|uninstall}\n' "${0##*/}"
}

if (( $# == 1 )) && [[ "$action" == "-h" || "$action" == "--help" ]]; then
    usage
    exit 0
fi

if (( $# != 1 )) || [[ "$action" != "install" && "$action" != "uninstall" ]]; then
    usage >&2
    exit 2
fi

if (( EUID != 0 )) && ! command -v sudo >/dev/null 2>&1; then
    printf 'Run as root or install sudo to continue.\n' >&2
    exit 1
fi

run_privileged() {
    if (( EUID == 0 )); then
        "$@"
    else
        sudo "$@"
    fi
}

refresh_desktop_database() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        run_privileged update-desktop-database /usr/share/applications
    fi
    if (( EUID != 0 )); then
        if command -v kbuildsycoca6 >/dev/null 2>&1; then
            kbuildsycoca6 --noincremental
        elif command -v kbuildsycoca5 >/dev/null 2>&1; then
            kbuildsycoca5 --noincremental
        fi
    fi
}

case "$action" in
    install)
        if ! command -v cargo >/dev/null 2>&1; then
            printf 'Cargo was not found. Install the Rust toolchain before running this script.\n' >&2
            exit 1
        fi

        cargo build --release --locked --manifest-path "$repo_root/Cargo.toml"

        run_privileged install -Dm755 \
            "$repo_root/target/release/viperdiskinfo" \
            /usr/bin/viperdiskinfo
        run_privileged install -Dm644 \
            "$repo_root/packaging/viperdiskinfo.desktop" \
            /usr/share/applications/viperdiskinfo.desktop
        run_privileged install -Dm644 \
            "$repo_root/assets/logo.png" \
            /usr/share/pixmaps/viperdiskinfo.png

        refresh_desktop_database
        printf 'Viper Disk Info installed. Launch it with `viperdiskinfo` or from your application menu.\n'
        ;;
    uninstall)
        run_privileged rm -f \
            /usr/bin/viperdiskinfo \
            /usr/share/applications/viperdiskinfo.desktop \
            /usr/share/pixmaps/viperdiskinfo.png

        refresh_desktop_database
        printf 'Viper Disk Info was removed. Settings were left untouched.\n'
        ;;
esac
