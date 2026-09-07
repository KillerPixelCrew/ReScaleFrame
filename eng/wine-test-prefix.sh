#!/usr/bin/env bash
# Build a Wine prefix with DXVK and vkd3d-proton, for the tests that need them.
#
# The default prefix on this machine has Wine's own D3D11, which does not implement shared NT
# handles: `CreateSharedHandle` returns E_NOTIMPL, so `shared_surface` skips and the presentation
# bridge cannot be measured. That is a fact about WineD3D and says nothing about DXVK, which is what
# the game actually runs under. This prefix is how the difference gets measured here instead of
# being deferred to a run of the game.
#
# The DLLs are borrowed from an installed Proton rather than downloaded. Same builds the game runs
# with, nothing fetched, and nothing added to the repository.
#
# Usage:
#   eng/wine-test-prefix.sh [--proton DIR] [--prefix DIR]
#   WINEPREFIX=.local/wine-test-prefix wine build/linux-cross-x64/bin/rsf_shared_surface.exe
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
    # Newest first, so a machine with several gets the one most likely to match the game's.
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

# Quietly, and waited for: wineboot returns before the prefix is finished and copying into a
# half-built system32 loses the files to the rest of the setup.
wineboot -u >/dev/null 2>&1 || true
wineserver -w

system32="$prefix/drive_c/windows/system32"
mkdir -p "$system32"

# Overwriting Wine's own, which is what an override of "native" then selects. The alternative,
# leaving both and relying on the override alone, silently falls back to the builtin when the
# native one fails to load, and a silent fallback is exactly what this prefix exists to avoid.
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
