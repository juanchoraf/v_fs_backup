#!/usr/bin/env sh
set -eu

APP_NAME="v_fs_backup"
CLI_BIN="v_fs_backup"
VERSIONS_DIR="versions"
UPDATE_DEPS=1
CARGO_LOCKED=""

printf '\n'

usage() {
    cat <<'USAGE'
Usage: sh scripts/build_binaries_unix.sh [--locked] [--no-update]

Builds portable 64-bit v_fs_backup artifacts for BSD, illumos/Solaris, and other Unix-like systems.

Options:
  --locked     Use Cargo.lock exactly as-is
  --no-update  Do not run cargo update before building
  -h, --help   Show this help
USAGE
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

case "$(uname -s)" in
    Linux)
        echo "error: use scripts/build_binaries_linux.sh on Linux" >&2
        exit 1
        ;;
    Darwin)
        echo "error: use scripts/build_binaries_macos.sh on macOS" >&2
        exit 1
        ;;
    FreeBSD)
        PLATFORM_OS="freebsd"
        ;;
    OpenBSD)
        PLATFORM_OS="openbsd"
        ;;
    NetBSD)
        PLATFORM_OS="netbsd"
        ;;
    DragonFly)
        PLATFORM_OS="dragonfly"
        ;;
    SunOS)
        PLATFORM_OS="solaris"
        ;;
    *)
        PLATFORM_OS=$(uname -s | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9' '_')
        PLATFORM_OS=${PLATFORM_OS%_}
        ;;
esac

case "$(uname -m)" in
    x86_64|amd64)
        PLATFORM_ARCH="x86_64"
        ;;
    aarch64|arm64)
        PLATFORM_ARCH="arm64"
        ;;
    *)
        echo "error: unsupported architecture: $(uname -m). Only 64-bit builds are supported." >&2
        exit 1
        ;;
esac

need_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: missing required command: $1" >&2
        exit 1
    fi
}

print_success() {
    printf '\033[32m%s\033[0m\n' "$1"
}

package_version() {
    sed -n 's/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | sed -n '1p'
}

logo_png() {
    if [ -f assets/v_fs_backup_logo_256.png ]; then
        printf '%s\n' assets/v_fs_backup_logo_256.png
    else
        printf '%s\n' assets/v_fs_backup_logo.png
    fi
}

write_checksums() {
    checksums_file="$1"
    shift
    checksums_name=$(basename "$checksums_file")

    rm -f "$checksums_file"
    if command -v sha256sum >/dev/null 2>&1; then
        (
            cd "$OUT_DIR"
            for artifact in "$@"; do
                [ -f "$artifact" ] && sha256sum "$artifact" >> "$checksums_name"
            done
        )
    elif command -v shasum >/dev/null 2>&1; then
        (
            cd "$OUT_DIR"
            for artifact in "$@"; do
                [ -f "$artifact" ] && shasum -a 256 "$artifact" >> "$checksums_name"
            done
        )
    else
        echo "note: sha256sum/shasum not found; skipped checksums"
    fi
}

need_command cargo
need_command tar

VERSION=$(package_version)
if [ -z "$VERSION" ]; then
    echo "error: unable to read package version from Cargo.toml" >&2
    exit 1
fi

LOGO_PNG=$(logo_png)
if [ ! -f "$LOGO_PNG" ]; then
    echo "error: missing logo asset: $LOGO_PNG" >&2
    exit 1
fi

VERSIONED_NAME="${APP_NAME}_v${VERSION}"
OUT_DIR="$VERSIONS_DIR/$VERSIONED_NAME"
STAGE_DIR="$OUT_DIR/.stage-$PLATFORM_OS"
CLI_BINARY="target/release/$CLI_BIN"
ARTIFACT_ARCH="${PLATFORM_OS}_$PLATFORM_ARCH"
ARTIFACT_BASENAME="${VERSIONED_NAME}_${ARTIFACT_ARCH}"
PORTABLE_TAR="$ARTIFACT_BASENAME.tar.gz"
PORTABLE_ZIP="$ARTIFACT_BASENAME.zip"

if [ "$UPDATE_DEPS" -eq 1 ]; then
    cargo update
fi

cargo build --release $CARGO_LOCKED

if [ ! -x "$CLI_BINARY" ]; then
    echo "error: release binary not found at $CLI_BINARY" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
rm -rf "$STAGE_DIR"
rm -f "$OUT_DIR/$ARTIFACT_BASENAME" "$OUT_DIR/$ARTIFACT_BASENAME".*
mkdir -p "$STAGE_DIR/$APP_NAME/bin"
mkdir -p "$STAGE_DIR/$APP_NAME/docs"
mkdir -p "$STAGE_DIR/$APP_NAME/assets"

cp "$CLI_BINARY" "$OUT_DIR/$ARTIFACT_BASENAME"
cp "$CLI_BINARY" "$STAGE_DIR/$APP_NAME/bin/$CLI_BIN"
cp README.md "$STAGE_DIR/$APP_NAME/docs/README.md"
cp "$LOGO_PNG" "$STAGE_DIR/$APP_NAME/assets/${APP_NAME}_logo.png"
chmod 0755 "$OUT_DIR/$ARTIFACT_BASENAME"
chmod 0755 "$STAGE_DIR/$APP_NAME/bin/$CLI_BIN"

tar -czf "$OUT_DIR/$PORTABLE_TAR" -C "$STAGE_DIR" "$APP_NAME"
echo "packaged $OUT_DIR/$PORTABLE_TAR"

if command -v zip >/dev/null 2>&1; then
    (
        cd "$STAGE_DIR"
        zip -qr "../$PORTABLE_ZIP" "$APP_NAME"
    )
    echo "packaged $OUT_DIR/$PORTABLE_ZIP"
else
    echo "note: zip not found; skipped $OUT_DIR/$PORTABLE_ZIP"
fi

write_checksums "$OUT_DIR/$ARTIFACT_BASENAME.sha256" \
    "$ARTIFACT_BASENAME" \
    "$PORTABLE_TAR" \
    "$PORTABLE_ZIP"

rm -rf "$STAGE_DIR"

print_success "$PLATFORM_OS artifacts created under $OUT_DIR"
printf '\n'
