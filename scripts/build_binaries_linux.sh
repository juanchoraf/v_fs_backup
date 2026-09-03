#!/usr/bin/env sh
set -eu

APP_NAME="v_fs_backup"
CLI_BIN="v_fs_backup"
PUBLISHER="TheVelasquez.com"
DESKTOP_ID="com.thevelasquez.v-fs-backup"
MIME_TYPE="application/x-v-fs-backup"
MIME_ICON="application-x-v-fs-backup"
VERSIONS_DIR="versions"
UPDATE_DEPS=1
CARGO_LOCKED=""

printf '\n'

usage() {
    cat <<'USAGE'
Usage: sh scripts/build_binaries_linux.sh [--locked] [--no-update]

Builds Linux 64-bit v_fs_backup artifacts for the current machine.

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

if [ "$(uname -s)" != "Linux" ]; then
    echo "error: scripts/build_binaries_linux.sh must run on Linux" >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64|amd64)
        PLATFORM_ARCH="x86_64"
        DEB_ARCH="amd64"
        ;;
    aarch64|arm64)
        PLATFORM_ARCH="arm64"
        DEB_ARCH="arm64"
        ;;
    *)
        echo "error: unsupported Linux architecture: $(uname -m). Only 64-bit builds are supported." >&2
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
STAGE_DIR="$OUT_DIR/.stage-linux"
CLI_BINARY="target/release/$CLI_BIN"
ARTIFACT_ARCH="linux_$PLATFORM_ARCH"
ARTIFACT_BASENAME="${VERSIONED_NAME}_${ARTIFACT_ARCH}"
PORTABLE_TAR="$ARTIFACT_BASENAME.tar.gz"
PORTABLE_ZIP="$ARTIFACT_BASENAME.zip"
DEB_NAME="$ARTIFACT_BASENAME.deb"

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

if command -v dpkg-deb >/dev/null 2>&1; then
    DEB_TMP_PARENT=$(mktemp -d "${TMPDIR:-/tmp}/${APP_NAME}-deb.XXXXXX")
    trap 'rm -rf "$DEB_TMP_PARENT"' EXIT
    DEB_ROOT="$DEB_TMP_PARENT/root"
    DEB_PACKAGE=$(printf '%s' "$APP_NAME" | tr '_' '-')
    mkdir -p "$DEB_ROOT/DEBIAN"
    mkdir -p "$DEB_ROOT/usr/local/bin"
    mkdir -p "$DEB_ROOT/usr/share/doc/$APP_NAME"
    mkdir -p "$DEB_ROOT/usr/share/applications"
    mkdir -p "$DEB_ROOT/usr/share/icons/hicolor/256x256/apps"
    mkdir -p "$DEB_ROOT/usr/share/icons/hicolor/256x256/mimetypes"
    mkdir -p "$DEB_ROOT/usr/share/metainfo"
    mkdir -p "$DEB_ROOT/usr/share/mime/packages"
    cp "$CLI_BINARY" "$DEB_ROOT/usr/local/bin/$CLI_BIN"
    cp README.md "$DEB_ROOT/usr/share/doc/$APP_NAME/README.md"
    cp "$LOGO_PNG" "$DEB_ROOT/usr/share/icons/hicolor/256x256/apps/$APP_NAME.png"
    cp "$LOGO_PNG" "$DEB_ROOT/usr/share/icons/hicolor/256x256/mimetypes/$MIME_ICON.png"
    chmod 0755 "$DEB_ROOT/usr/local/bin/$CLI_BIN"
    cat > "$DEB_ROOT/usr/share/applications/$DESKTOP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=v_fs_backup
Comment=Fast compressed filesystem backups
Exec=v_fs_backup
Icon=v_fs_backup
Terminal=true
Categories=Utility;Archiving;
Keywords=backup;archive;compress;zstd;filesystem;fsb;
MimeType=$MIME_TYPE;
StartupNotify=false
EOF
    cat > "$DEB_ROOT/usr/share/metainfo/$DESKTOP_ID.metainfo.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>$DESKTOP_ID</id>
  <name>v_fs_backup</name>
  <summary>Fast compressed filesystem backups</summary>
  <developer_name>$PUBLISHER</developer_name>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <launchable type="desktop-id">$DESKTOP_ID.desktop</launchable>
  <description>
    <p>Create and restore compressed .fsb filesystem backup archives.</p>
  </description>
</component>
EOF
    cat > "$DEB_ROOT/usr/share/mime/packages/$DESKTOP_ID.xml" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="$MIME_TYPE">
    <comment>v_fs_backup archive</comment>
    <glob pattern="*.fsb"/>
    <icon name="$MIME_ICON"/>
  </mime-type>
</mime-info>
EOF
    cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: $DEB_PACKAGE
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: $PUBLISHER
Description: Fast compressed filesystem backups
 v_fs_backup creates and restores compressed .fsb filesystem backup archives.
EOF
    cat > "$DEB_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database /usr/share/mime || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
EOF
    cat > "$DEB_ROOT/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database /usr/share/mime || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
EOF
    find "$DEB_ROOT" -type d -exec chmod 0755 {} +
    chmod 0755 "$DEB_ROOT/DEBIAN/postinst" "$DEB_ROOT/DEBIAN/postrm"
    chmod 0644 "$DEB_ROOT/DEBIAN/control"
    chmod 0644 "$DEB_ROOT/usr/share/doc/$APP_NAME/README.md"
    chmod 0644 "$DEB_ROOT/usr/share/applications/$DESKTOP_ID.desktop"
    chmod 0644 "$DEB_ROOT/usr/share/icons/hicolor/256x256/apps/$APP_NAME.png"
    chmod 0644 "$DEB_ROOT/usr/share/icons/hicolor/256x256/mimetypes/$MIME_ICON.png"
    chmod 0644 "$DEB_ROOT/usr/share/metainfo/$DESKTOP_ID.metainfo.xml"
    chmod 0644 "$DEB_ROOT/usr/share/mime/packages/$DESKTOP_ID.xml"
    DPKG_ROOT_OWNER=""
    if dpkg-deb --help 2>/dev/null | grep -q -- '--root-owner-group'; then
        DPKG_ROOT_OWNER="--root-owner-group"
    fi
    dpkg-deb $DPKG_ROOT_OWNER --build "$DEB_ROOT" "$OUT_DIR/$DEB_NAME" >/dev/null
    echo "packaged $OUT_DIR/$DEB_NAME"
else
    echo "note: dpkg-deb not found; skipped $OUT_DIR/$DEB_NAME"
fi

write_checksums "$OUT_DIR/$ARTIFACT_BASENAME.sha256" \
    "$ARTIFACT_BASENAME" \
    "$PORTABLE_TAR" \
    "$PORTABLE_ZIP" \
    "$DEB_NAME"

rm -rf "$STAGE_DIR"

print_success "Linux artifacts created under $OUT_DIR"
printf '\n'
