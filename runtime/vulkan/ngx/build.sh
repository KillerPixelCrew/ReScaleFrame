#!/usr/bin/env bash
# Builds VulkanStory's NGX shim (see vulkanstory_ngx.h for why it exists).
#
#   build.sh <output directory>
#
# The first positional argument is the output directory (created if needed).
# CC selects one compiler executable, default cc. The output name is
# VulkanStoryNgx.dll on MinGW/MSYS/Cygwin and libVulkanStoryNgx.so otherwise;
# Linux additionally links libdl. A target newer than source/header is reused.
# Missing arguments exit 2. Missing compiler or compilation failure exits 0
# after removing target/temporary output, so callers must inspect the artifact.
# Successful compilation replaces the target through its sibling .tmp file.
#
# The shim is the only call site NGX will accept: libnvidia-ngx.so.1 resolves
# its caller's module from the return address, and a .NET P/Invoke stub lives in
# anonymous JIT memory, which aborts the process inside the driver.
#
# Degrades, never fails the build: with no C compiler on the host it prints one
# line and exits 0, and the managed side then reports NGX as unavailable
# (NgxShim.Availability). That is the documented behaviour - a machine without
# cc can still build and run VulkanStory, just without DLSS.
set -u

out="${1:-}"
if [ -z "$out" ]; then
    echo "usage: build.sh <output directory>" >&2
    exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$here/vulkanstory_ngx.c"

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) name="VulkanStoryNgx.dll" ;;
    *) name="libVulkanStoryNgx.so" ;;
esac

cc="${CC:-cc}"
target="$out/$name"
if ! command -v "$cc" >/dev/null 2>&1; then
    # Same reason as the failed-compile path below: a library left over from a
    # build on a host that still had a compiler would be packaged by the
    # native bundle staging and shipped beside a managed side it no longer
    # matches. Without a compiler the only correct output is no output.
    rm -f "$target" "$target.tmp"
    echo "vulkanstory-ngx: no C compiler ($cc) on this host; skipping the NGX shim - DLSS will report unavailable."
    exit 0
fi

mkdir -p "$out" || exit 0

# Only relink when the source or header is newer during explicit bridge builds.
if [ -f "$target" ] && [ "$target" -nt "$src" ] && [ "$target" -nt "$here/vulkanstory_ngx.h" ]; then
    exit 0
fi

libs=""
[ "$(uname -s)" = "Linux" ] && libs="-ldl"

# shellcheck disable=SC2086
# -fno-optimize-sibling-calls is load-bearing, not tidiness: a tail call pops
# the shim's frame before entering NGX, and NGX then reads the *managed*
# caller's return address and aborts - exactly the failure the shim exists to
# prevent (measured 2026-09-12). The source also stores every forwarded result
# in a volatile local, so this flag is the second of two guards.
#
# It compiles to a temporary file and only replaces the shipped library once the
# compiler said yes: a failed compile that left the previous library in place
# would be picked up by native bundle staging and shipped beside a
# managed side it no longer matches.
if ! "$cc" -std=c99 -O2 -fPIC -shared -fvisibility=hidden \
        -fno-optimize-sibling-calls \
        -Wall -Wextra -Wno-unused-parameter \
        "$src" -o "$target.tmp" $libs; then
    echo "vulkanstory-ngx: $cc failed to build the NGX shim; DLSS will report unavailable." >&2
    rm -f "$target.tmp" "$target"
    exit 0
fi
mv -f "$target.tmp" "$target"

echo "vulkanstory-ngx: built $target"
exit 0
