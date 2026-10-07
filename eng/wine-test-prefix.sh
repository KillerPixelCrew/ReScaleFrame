#!/usr/bin/env bash
# Prepare a dedicated Wine prefix for graphics fixtures using installed Proton DLLs.
# Copies DXVK/vkd3d-proton into system32 and sets native-only registry overrides, replacing
# any existing graphics DLLs in the selected prefix. Nothing is downloaded or added to git.
# Shared NT-handle fixtures need an implementation beyond WineD3D's unsupported path;
# fixture results remain distinct from validation inside a game.
#
# Usage:
#   eng/wine-test-prefix.sh [--proton DIR] [--prefix DIR]
#   WINEPREFIX=.local/wine-test-prefix wine build/linux-cross-x64/bin/rsf_shared_surface.exe
#
# --proton: installed Proton files/ directory containing both graphics implementations.
#           Without it, the last matching installed path wins; versions are not sorted.
# --prefix: destination Wine prefix; default is .local/wine-test-prefix under the repo.
# Requirements: wineboot, wineserver, regedit and readable x86_64 Proton DLLs.
# Outputs: system32 graphics DLLs and overrides.reg in the selected prefix.
# Existing prefix contents are updated in place; no backup or rollback is provided.
# Failed setup may leave partial copies. Inspect output before treating fixtures as covered.
# Exit 2 indicates an unknown option; missing graphics inputs exit 1.
# Use ctest --preset linux-cross-dxvk with the default prefix, or pass a matching environment.
set -euo pipefail

proton=""
prefix="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.local/wine-test-prefix"

while [ $# -gt 0 ]; do
    case "$1" in
        --proton) proton="$2"; shift 2 ;;
        --prefix) prefix="$2"; shift 2 ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$proton" ]; then
    # The last matching glob entry wins; pass --proton to select a specific installed build.
    for candidate in \
        /usr/share/steam/compatibilitytools.d/*/files \
        "$HOME"/.local/share/Steam/steamapps/common/Proton*/files \
        "$HOME"/.steam/steam/steamapps/common/Proton*/files
    do
        if [ -d "$candidate/lib/wine/dxvk/x86_64-windows" ]; then
            proton="$candidate"
        fi
    done
fi

if [ -z "$proton" ] || [ ! -d "$proton/lib/wine/dxvk/x86_64-windows" ]; then
    echo "no Proton with DXVK found; pass --proton <proton>/files" >&2
    exit 1
fi

dxvk="$proton/lib/wine/dxvk/x86_64-windows"
vkd3d="$proton/lib/wine/vkd3d-proton/x86_64-windows"
if [ ! -d "$vkd3d" ]; then
    echo "no vkd3d-proton in $proton; D3D12 will be Wine's own and the bridge cannot be tested" >&2
    exit 1
fi

echo "proton: $proton"
echo "prefix: $prefix"

mkdir -p "$prefix"
export WINEPREFIX="$prefix"
export WINEDEBUG="${WINEDEBUG:--all}"

# Wait for prefix initialization before replacing files that setup could overwrite.
wineboot -u >/dev/null 2>&1 || true
wineserver -w

system32="$prefix/drive_c/windows/system32"
mkdir -p "$system32"

# Replace system32 copies and use native-only overrides so missing native support cannot silently
# fall back to the builtins.
for dll in d3d11 dxgi d3d10core; do
    if [ -f "$dxvk/$dll.dll" ]; then
        cp -f "$dxvk/$dll.dll" "$system32/$dll.dll"
        echo "  dxvk $dll"
    fi
done
for dll in d3d12 d3d12core; do
    if [ -f "$vkd3d/$dll.dll" ]; then
        cp -f "$vkd3d/$dll.dll" "$system32/$dll.dll"
        echo "  vkd3d-proton $dll"
    fi
done

# Native, so the copies above are what loads rather than Wine's builtins of the same name.
overrides="d3d11,d3d10core,dxgi,d3d12,d3d12core=n"
cat > "$prefix/overrides.reg" <<REG
Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\\Software\\Wine\\DllOverrides]
"d3d11"="native"
"d3d10core"="native"
"dxgi"="native"
"d3d12"="native"
"d3d12core"="native"
REG
wine regedit "$prefix/overrides.reg" >/dev/null 2>&1 || true
wineserver -w

echo
echo "ready. To use it:"
echo "  WINEPREFIX=$prefix WINEDLLOVERRIDES=\"$overrides\" wine <test>.exe"
echo
echo "or the whole suite:"
echo "  ctest --preset linux-cross-dxvk"
