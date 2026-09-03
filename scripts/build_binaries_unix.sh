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

v_concat ""

usage() {
    v_concat "$(cat <<'USAGE'
Usage: sh scripts/build_binaries_unix.sh [--locked] [--no-update]

Builds generic 64-bit Unix/BSD v_fs_backup portable artifacts for the current
machine. Linux and macOS have richer native package builders; this script is
for BSD and other Unix-like systems where a portable tarball/zip is the release
format.

Options:
  --locked     Use Cargo.lock exactly as-is
  --no-update  Do not run cargo update before building
  -h, --help   Show this help
USAGE
)"
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

print_success() {
    v_concat "$(printf '\033[32m%s\033[0m' "$1")"
}

package_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | sed -n '1p'
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
    elif command -v sha256 >/dev/null 2>&1; then
        (
            cd "$OUT_DIR"
            for artifact in "$@"; do
                [ -f "$artifact" ] && printf '%s  %s\n' "$(sha256 -q "$artifact")" "$artifact" >> "$checksums_name"
            done
        )
    else
        echo "note: SHA-256 command not found; skipped checksums"
    fi
}

need_command cargo
need_command tar

VERSION=$(package_version)
if [ -z "$VERSION" ]; then
    echo "error: unable to read package version from Cargo.toml" >&2
    exit 1
fi

PLATFORM_OS=$(normalize_os)
PLATFORM_ARCH=$(normalize_arch)
VERSIONED_NAME="${APP_NAME}_v${VERSION}"
OUT_DIR="$VERSIONS_DIR/$VERSIONED_NAME"
STAGE_DIR="$OUT_DIR/.stage-$PLATFORM_OS"
BINARY="target/release/$APP_NAME"
LOGO_PNG="assets/v_fs_backup_logo_256.png"
LOGO_ICNS="assets/v_fs_backup_logo.icns"
ARTIFACT_ARCH="${PLATFORM_OS}_${PLATFORM_ARCH}"
ARTIFACT_BASENAME="${VERSIONED_NAME}_${ARTIFACT_ARCH}"
PORTABLE_TAR="$ARTIFACT_BASENAME.tar.gz"
PORTABLE_ZIP="$ARTIFACT_BASENAME.zip"

if [ ! -f "$LOGO_PNG" ]; then
    echo "error: missing logo asset: $LOGO_PNG. Run scripts/prepare_logo_assets.py first." >&2
    exit 1
fi

if [ "$UPDATE_DEPS" -eq 1 ]; then
    cargo update
fi

cargo build --release $CARGO_LOCKED

if [ ! -x "$BINARY" ]; then
    echo "error: release binary not found at $BINARY" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
rm -rf "$STAGE_DIR"
rm -f "$OUT_DIR/$ARTIFACT_BASENAME" "$OUT_DIR/$ARTIFACT_BASENAME".*
mkdir -p "$STAGE_DIR/$APP_NAME/bin"
mkdir -p "$STAGE_DIR/$APP_NAME/docs"
mkdir -p "$STAGE_DIR/$APP_NAME/assets"

cp "$BINARY" "$OUT_DIR/$ARTIFACT_BASENAME"
cp "$BINARY" "$STAGE_DIR/$APP_NAME/bin/$APP_NAME"
cp README.md "$STAGE_DIR/$APP_NAME/docs/README.md"
cp "$LOGO_PNG" "$STAGE_DIR/$APP_NAME/assets/v_fs_backup_logo.png"
if [ -f "$LOGO_ICNS" ]; then
    cp "$LOGO_ICNS" "$STAGE_DIR/$APP_NAME/assets/v_fs_backup_logo.icns"
fi
chmod 0755 "$OUT_DIR/$ARTIFACT_BASENAME"
chmod 0755 "$STAGE_DIR/$APP_NAME/bin/$APP_NAME"

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

print_success "Unix/BSD artifacts created under $OUT_DIR"
v_concat ""
