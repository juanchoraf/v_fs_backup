#!/usr/bin/env sh
set -eu

APP_NAME="v_fs_backup"
VERSIONS_DIR="versions"
UPDATE_DEPS=1
CARGO_LOCKED=""

v_concat() {
    printf '\n%s\n\n' "$*"
}

echo() {
    v_concat "$*"
}

usage() {
    v_concat "Usage: sh scripts/build_binaries_unix.sh [--locked] [--no-update]

Builds the current Unix/BSD 64-bit self-installing v_fs_backup binary.

Options:
  --locked     Use Cargo.lock exactly as-is
  --no-update  Do not run cargo update before building
  -h, --help   Show this help"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --locked)
            UPDATE_DEPS=0
            CARGO_LOCKED="--locked"
            shift
            ;;
        --no-update)
            UPDATE_DEPS=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

cd "$REPO_DIR"

normalize_os() {
    os_name=$(uname -s 2>/dev/null || printf unknown)
    case "$os_name" in
        FreeBSD) printf 'freebsd\n' ;;
        OpenBSD) printf 'openbsd\n' ;;
        NetBSD) printf 'netbsd\n' ;;
        DragonFly) printf 'dragonfly\n' ;;
        SunOS) printf 'sunos\n' ;;
        Linux) printf 'linux\n' ;;
        Darwin) printf 'macos\n' ;;
        *) printf '%s\n' "$os_name" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9' '_' | sed 's/_$//' ;;
    esac
}

normalize_arch() {
    arch_name=$(uname -m 2>/dev/null || printf unknown)
    case "$arch_name" in
        x86_64|amd64) printf 'x86_64\n' ;;
        aarch64|arm64) printf 'arm64\n' ;;
        *)
            echo "error: unsupported architecture: $arch_name. Only 64-bit builds are supported." >&2
            exit 1
            ;;
    esac
}

need_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: missing required command: $1" >&2
        exit 1
    fi
}

package_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | sed -n '1p'
}

write_checksum() {
    artifact="$1"
    checksums_file="$OUT_DIR/$artifact.sha256"
    rm -f "$checksums_file"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$OUT_DIR" && sha256sum "$artifact" > "$artifact.sha256")
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$OUT_DIR" && shasum -a 256 "$artifact" > "$artifact.sha256")
    elif command -v sha256 >/dev/null 2>&1; then
        (cd "$OUT_DIR" && printf '%s  %s\n' "$(sha256 -q "$artifact")" "$artifact" > "$artifact.sha256")
    else
        echo "note: SHA-256 command not found; skipped checksum"
    fi
}

need_command cargo

VERSION=$(package_version)
if [ -z "$VERSION" ]; then
    echo "error: unable to read package version from Cargo.toml" >&2
    exit 1
fi

PLATFORM_OS=$(normalize_os)
PLATFORM_ARCH=$(normalize_arch)
VERSIONED_NAME="${APP_NAME}_v${VERSION}"
OUT_DIR="$VERSIONS_DIR/$VERSIONED_NAME"
BINARY="target/release/$APP_NAME"
ARTIFACT="${VERSIONED_NAME}_${PLATFORM_OS}_${PLATFORM_ARCH}"

if [ "$UPDATE_DEPS" -eq 1 ]; then
    cargo update
fi

cargo build --release $CARGO_LOCKED

if [ ! -x "$BINARY" ]; then
    echo "error: release binary not found at $BINARY" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/$ARTIFACT" "$OUT_DIR/$ARTIFACT.sha256"
cp "$BINARY" "$OUT_DIR/$ARTIFACT"
chmod 0755 "$OUT_DIR/$ARTIFACT"
write_checksum "$ARTIFACT"

v_concat "$(printf '\033[32m%s\033[0m' "Unix/BSD binary created: $OUT_DIR/$ARTIFACT")"
