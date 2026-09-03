#!/usr/bin/env sh
set -eu

printf '\n'

if [ "$(uname -s)" != "Linux" ]; then
    echo "error: scripts/setup_linux_build_deps.sh must run on Linux" >&2
    exit 1
fi

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
        return
    fi
    if command -v sudo >/dev/null 2>&1; then
        sudo "$@"
        return
    fi
    echo "error: this command needs root privileges and sudo is not installed: $*" >&2
    exit 1
}

if command -v apt-get >/dev/null 2>&1; then
    as_root apt-get update
    as_root apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        desktop-file-utils \
        dpkg-dev \
        file \
        hicolor-icon-theme \
        pkg-config \
        shared-mime-info \
        tar \
        zip
elif command -v dnf >/dev/null 2>&1; then
    as_root dnf install -y \
        ca-certificates \
        desktop-file-utils \
        gcc \
        gcc-c++ \
        hicolor-icon-theme \
        pkgconf-pkg-config \
        shared-mime-info \
        tar \
        zip
elif command -v pacman >/dev/null 2>&1; then
    as_root pacman -Sy --needed --noconfirm \
        base-devel \
        ca-certificates \
        desktop-file-utils \
        hicolor-icon-theme \
        pkgconf \
        shared-mime-info \
        tar \
        zip
else
    echo "error: unsupported Linux package manager. Install C build tools, pkg-config, tar, zip, dpkg-deb, desktop-file-utils, and shared-mime-info." >&2
    exit 1
fi

echo "Linux build dependencies are ready."
printf '\n'
