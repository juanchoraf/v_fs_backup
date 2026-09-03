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
Usage: sh scripts/build_binaries_macos.sh [--locked] [--no-update]

Builds macOS 64-bit v_fs_backup artifacts for the current Mac.

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

if [ "$(uname -s)" != "Darwin" ]; then
    echo "error: scripts/build_binaries_macos.sh must run on macOS" >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64)
        PLATFORM_ARCH="x86_64"
        ;;
    arm64|aarch64)
        PLATFORM_ARCH="arm64"
        ;;
    *)
        echo "error: unsupported macOS architecture: $(uname -m). Only 64-bit builds are supported." >&2
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
    v_concat "$(printf '\033[32m%s\033[0m' "$1")"
}

package_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | sed -n '1p'
}

write_checksums() {
    checksums_file="$1"
    shift
    checksums_name=$(basename "$checksums_file")

    if ! command -v shasum >/dev/null 2>&1; then
        echo "note: shasum not found; skipped checksums"
        return
    fi

    rm -f "$checksums_file"
    (
        cd "$OUT_DIR"
        for artifact in "$@"; do
            [ -f "$artifact" ] && shasum -a 256 "$artifact" >> "$checksums_name"
        done
    )
}

write_macos_app_bundle() {
    bundle_root="$1"
    macos_dir="$bundle_root/Contents/MacOS"
    resources_dir="$bundle_root/Contents/Resources"
    launcher="$macos_dir/$APP_NAME"
    plist="$bundle_root/Contents/Info.plist"

    mkdir -p "$macos_dir" "$resources_dir"
    cp "$LOGO_ICNS" "$resources_dir/v_fs_backup_logo.icns"

    cat > "$launcher" <<EOF
#!/usr/bin/env sh
if command -v osascript >/dev/null 2>&1; then
    osascript <<'APPLESCRIPT'
tell application "Terminal"
    activate
    do script "/usr/local/bin/v_fs_backup"
end tell
APPLESCRIPT
else
    /usr/local/bin/v_fs_backup
fi
EOF

    cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeExtensions</key>
      <array>
        <string>fsb</string>
      </array>
      <key>CFBundleTypeIconFile</key>
      <string>v_fs_backup_logo</string>
      <key>CFBundleTypeName</key>
      <string>v_fs_backup archive</string>
      <key>CFBundleTypeRole</key>
      <string>Viewer</string>
      <key>LSHandlerRank</key>
      <string>Owner</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>com.thevelasquez.v-fs-backup.archive</string>
      </array>
    </dict>
  </array>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIconFile</key>
  <string>v_fs_backup_logo</string>
  <key>CFBundleIdentifier</key>
  <string>com.thevelasquez.v-fs-backup</string>
  <key>CFBundleName</key>
  <string>v_fs_backup</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.13</string>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeConformsTo</key>
      <array>
        <string>public.data</string>
      </array>
      <key>UTTypeDescription</key>
      <string>v_fs_backup archive</string>
      <key>UTTypeIdentifier</key>
      <string>com.thevelasquez.v-fs-backup.archive</string>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key>
        <array>
          <string>fsb</string>
        </array>
        <key>public.mime-type</key>
        <string>application/x-v-fs-backup</string>
      </dict>
    </dict>
  </array>
</dict>
</plist>
EOF

    chmod 0755 "$launcher"
    chmod 0644 "$plist" "$resources_dir/v_fs_backup_logo.icns"
}

need_command cargo
need_command tar

VERSION=$(package_version)
if [ -z "$VERSION" ]; then
    echo "error: unable to read package version from Cargo.toml" >&2
    exit 1
fi

VERSIONED_NAME="${APP_NAME}_v${VERSION}"
OUT_DIR="$VERSIONS_DIR/$VERSIONED_NAME"
STAGE_DIR="$OUT_DIR/.stage-macos"
BINARY="target/release/$APP_NAME"
LOGO_PNG="assets/v_fs_backup_logo_256.png"
LOGO_ICNS="assets/v_fs_backup_logo.icns"
ARTIFACT_ARCH="macos_$PLATFORM_ARCH"
ARTIFACT_BASENAME="${VERSIONED_NAME}_${ARTIFACT_ARCH}"
PORTABLE_TAR="$ARTIFACT_BASENAME.tar.gz"
PORTABLE_ZIP="$ARTIFACT_BASENAME.zip"
PKG_NAME="$ARTIFACT_BASENAME.pkg"

if [ ! -f "$LOGO_PNG" ] || [ ! -f "$LOGO_ICNS" ]; then
    echo "error: missing logo assets. Run scripts/prepare_logo_assets.py first." >&2
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
cp "$LOGO_ICNS" "$STAGE_DIR/$APP_NAME/assets/v_fs_backup_logo.icns"
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

if command -v pkgbuild >/dev/null 2>&1; then
    PKG_TMP_PARENT=$(mktemp -d "${TMPDIR:-/tmp}/${APP_NAME}-pkg.XXXXXX")
    trap 'rm -rf "$PKG_TMP_PARENT"' EXIT
    PKG_ROOT="$PKG_TMP_PARENT/root"
    mkdir -p "$PKG_ROOT/Applications"
    mkdir -p "$PKG_ROOT/usr/local/bin"
    mkdir -p "$PKG_ROOT/usr/local/share/$APP_NAME/docs"
    mkdir -p "$PKG_ROOT/usr/local/share/$APP_NAME/assets"
    cp "$BINARY" "$PKG_ROOT/usr/local/bin/$APP_NAME"
    cp README.md "$PKG_ROOT/usr/local/share/$APP_NAME/docs/README.md"
    cp "$LOGO_PNG" "$PKG_ROOT/usr/local/share/$APP_NAME/assets/v_fs_backup_logo.png"
    cp "$LOGO_ICNS" "$PKG_ROOT/usr/local/share/$APP_NAME/assets/v_fs_backup_logo.icns"
    write_macos_app_bundle "$PKG_ROOT/Applications/$APP_NAME.app"
    chmod 0755 "$PKG_ROOT/usr/local/bin/$APP_NAME"
    chmod 0644 "$PKG_ROOT/usr/local/share/$APP_NAME/docs/README.md"
    chmod 0644 "$PKG_ROOT/usr/local/share/$APP_NAME/assets/v_fs_backup_logo.png"
    chmod 0644 "$PKG_ROOT/usr/local/share/$APP_NAME/assets/v_fs_backup_logo.icns"
    pkgbuild \
        --root "$PKG_ROOT" \
        --identifier "com.thevelasquez.v-fs-backup" \
        --version "$VERSION" \
        --install-location "/" \
        "$OUT_DIR/$PKG_NAME" >/dev/null
    echo "packaged $OUT_DIR/$PKG_NAME"
else
    echo "note: pkgbuild not found; skipped $OUT_DIR/$PKG_NAME"
fi

write_checksums "$OUT_DIR/$ARTIFACT_BASENAME.sha256" \
    "$ARTIFACT_BASENAME" \
    "$PORTABLE_TAR" \
    "$PORTABLE_ZIP" \
    "$PKG_NAME"

rm -rf "$STAGE_DIR"

print_success "macOS artifacts created under $OUT_DIR"
v_concat ""
