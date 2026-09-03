#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

v_concat() {
    printf '\n%s\n\n' "$*"
}

echo() {
    v_concat "$*"
}

DOTNET_CLI_TELEMETRY_OPTOUT=1
DOTNET_NOLOGO=true
POWERSHELL_TELEMETRY_OPTOUT=1
POWERSHELL_UPDATECHECK=Off
POWERSHELL_DIAGNOSTICS_OPTOUT=1
VSCMD_SKIP_SENDTELEMETRY=1
RUSTUP_NO_UPDATE_CHECK=1
DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE=1
DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1
export DOTNET_CLI_TELEMETRY_OPTOUT DOTNET_NOLOGO POWERSHELL_TELEMETRY_OPTOUT
export POWERSHELL_UPDATECHECK POWERSHELL_DIAGNOSTICS_OPTOUT
export VSCMD_SKIP_SENDTELEMETRY RUSTUP_NO_UPDATE_CHECK
export DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE DOTNET_SKIP_FIRST_TIME_EXPERIENCE

run_windows_builder() {
    if command -v pwsh >/dev/null 2>&1; then
        exec pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/build_binaries_windows.ps1" "$@"
    fi
    if command -v powershell >/dev/null 2>&1; then
        exec powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/build_binaries_windows.ps1" "$@"
    fi
    if command -v powershell.exe >/dev/null 2>&1; then
        exec powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/build_binaries_windows.ps1" "$@"
    fi

    echo "error: Windows builds need PowerShell. Run scripts/build_binaries_windows.ps1 from PowerShell." >&2
    exit 1
}

case "${OS:-}" in
    Windows_NT)
        run_windows_builder "$@"
        ;;
esac

case "$(uname -s 2>/dev/null || printf unknown)" in
    Linux)
        exec sh "$SCRIPT_DIR/build_binaries_linux.sh" "$@"
        ;;
    Darwin)
        exec sh "$SCRIPT_DIR/build_binaries_macos.sh" "$@"
        ;;
    FreeBSD|OpenBSD|NetBSD|DragonFly|SunOS)
        exec sh "$SCRIPT_DIR/build_binaries_unix.sh" "$@"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        run_windows_builder "$@"
        ;;
    *)
        echo "error: unsupported build OS. Use one of:" >&2
        echo "  scripts/build_binaries_linux.sh" >&2
        echo "  scripts/build_binaries_macos.sh" >&2
        echo "  scripts/build_binaries_unix.sh" >&2
        echo "  scripts/build_binaries_windows.ps1" >&2
        exit 1
        ;;
esac
