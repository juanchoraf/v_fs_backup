#!/usr/bin/env sh
set -eu

APP_NAME="v_fs_backup"
CLI_BIN="v_fs_backup"
PKG_IDENTIFIER="com.thevelasquez.v-fs-backup"
FSB_TYPE_ID="com.thevelasquez.v-fs-backup.archive"
VERSIONS_DIR="versions"
UPDATE_DEPS=1
CARGO_LOCKED=""

printf '\n'

usage() {
    cat <<'USAGE'
Usage: sh scripts/build_binaries_macos.sh [--locked] [--no-update]

Builds macOS 64-bit v_fs_backup artifacts for the current Mac.

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
    printf '\033[32m%s\033[0m\n' "$1"
}

package_version() {
    sed -n 's/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | sed -n '1p'
}

logo_png() {
    if [ -f assets/v_fs_backup_logo_1024.png ]; then
        printf '%s\n' assets/v_fs_backup_logo_1024.png
    else
        printf '%s\n' assets/v_fs_backup_logo.png
    fi
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

make_icon() {
    src="$1"
    out="$2"

    if [ -f assets/v_fs_backup_logo.icns ]; then
        cp assets/v_fs_backup_logo.icns "$out"
        return 0
    fi

    if ! command -v sips >/dev/null 2>&1 || ! command -v iconutil >/dev/null 2>&1; then
        return 1
    fi

    iconset="$OUT_DIR/.iconset"
    rm -rf "$iconset"
    mkdir -p "$iconset"
    sips -z 16 16 "$src" --out "$iconset/icon_16x16.png" >/dev/null
    sips -z 32 32 "$src" --out "$iconset/icon_16x16@2x.png" >/dev/null
    sips -z 32 32 "$src" --out "$iconset/icon_32x32.png" >/dev/null
    sips -z 64 64 "$src" --out "$iconset/icon_32x32@2x.png" >/dev/null
    sips -z 128 128 "$src" --out "$iconset/icon_128x128.png" >/dev/null
    sips -z 256 256 "$src" --out "$iconset/icon_128x128@2x.png" >/dev/null
    sips -z 256 256 "$src" --out "$iconset/icon_256x256.png" >/dev/null
    sips -z 512 512 "$src" --out "$iconset/icon_256x256@2x.png" >/dev/null
    sips -z 512 512 "$src" --out "$iconset/icon_512x512.png" >/dev/null
    sips -z 1024 1024 "$src" --out "$iconset/icon_512x512@2x.png" >/dev/null
    iconutil -c icns "$iconset" -o "$out"
    rm -rf "$iconset"
}

write_app_bundle() {
    app_dir="$1"
    icon_path="$2"

    mkdir -p "$app_dir/Contents/MacOS"
    mkdir -p "$app_dir/Contents/Resources"
    cp "$CLI_BINARY" "$app_dir/Contents/MacOS/${CLI_BIN}-bin"
    chmod 0755 "$app_dir/Contents/MacOS/${CLI_BIN}-bin"
    cat > "$app_dir/Contents/MacOS/$APP_NAME" <<EOF
#!/bin/sh
APP_DIR=\$(CDPATH= cd -- "\$(dirname -- "\$0")" && pwd)
BIN="\$APP_DIR/${CLI_BIN}-bin"
if [ ! -x "\$BIN" ] && [ -x "/usr/local/bin/$CLI_BIN" ]; then
    BIN="/usr/local/bin/$CLI_BIN"
fi
if command -v osascript >/dev/null 2>&1; then
    exec /usr/bin/osascript -e 'tell application "Terminal"' -e 'activate' -e "do script quoted form of \"\$BIN\"" -e 'end tell'
fi
exec "\$BIN"
EOF
    chmod 0755 "$app_dir/Contents/MacOS/$APP_NAME"

    if [ -f "$icon_path" ]; then
        cp "$icon_path" "$app_dir/Contents/Resources/${APP_NAME}.icns"
        icon_key="<key>CFBundleIconFile</key><string>${APP_NAME}</string>"
    else
        icon_key=""
    fi

    cat > "$app_dir/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>v_fs_backup</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  $icon_key
  <key>CFBundleIdentifier</key><string>$PKG_IDENTIFIER</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>v_fs_backup</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeExtensions</key><array><string>fsb</string></array>
      <key>CFBundleTypeIconFile</key><string>${APP_NAME}</string>
      <key>CFBundleTypeName</key><string>v_fs_backup archive</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key><array><string>$FSB_TYPE_ID</string></array>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>$FSB_TYPE_ID</string>
      <key>UTTypeDescription</key><string>v_fs_backup archive</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key><array><string>fsb</string></array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
EOF
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
STAGE_DIR="$OUT_DIR/.stage-macos"
CLI_BINARY="target/release/$CLI_BIN"
ICON_ICNS="$OUT_DIR/${APP_NAME}.icns"
ARTIFACT_ARCH="macos_$PLATFORM_ARCH"
ARTIFACT_BASENAME="${VERSIONED_NAME}_${ARTIFACT_ARCH}"
PORTABLE_TAR="$ARTIFACT_BASENAME.tar.gz"
PORTABLE_ZIP="$ARTIFACT_BASENAME.zip"
PKG_NAME="$ARTIFACT_BASENAME.pkg"

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

if ! make_icon "$LOGO_PNG" "$ICON_ICNS"; then
    echo "note: sips/iconutil not found; app bundle will use the default icon"
fi

cp "$CLI_BINARY" "$OUT_DIR/$ARTIFACT_BASENAME"
cp "$CLI_BINARY" "$STAGE_DIR/$APP_NAME/bin/$CLI_BIN"
cp README.md "$STAGE_DIR/$APP_NAME/docs/README.md"
cp "$LOGO_PNG" "$STAGE_DIR/$APP_NAME/assets/${APP_NAME}_logo.png"
chmod 0755 "$OUT_DIR/$ARTIFACT_BASENAME"
chmod 0755 "$STAGE_DIR/$APP_NAME/bin/$CLI_BIN"
write_app_bundle "$STAGE_DIR/$APP_NAME/${APP_NAME}.app" "$ICON_ICNS"

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
    cp "$CLI_BINARY" "$PKG_ROOT/usr/local/bin/$CLI_BIN"
    chmod 0755 "$PKG_ROOT/usr/local/bin/$CLI_BIN"
    cp -R "$STAGE_DIR/$APP_NAME/${APP_NAME}.app" "$PKG_ROOT/Applications/${APP_NAME}.app"
    pkgbuild \
        --root "$PKG_ROOT" \
        --identifier "$PKG_IDENTIFIER" \
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
rm -f "$ICON_ICNS"

print_success "macOS artifacts created under $OUT_DIR"
printf '\n'
