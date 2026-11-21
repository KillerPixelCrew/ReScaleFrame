#!/usr/bin/env bash
# Builds VulkanStory's NGX shim and its Rust loader.
#
#   build.sh <output directory>
#
# CC selects one compiler executable, default cc. The output name is
# VulkanStoryNgx.dll on MinGW/MSYS/Cygwin and libVulkanStoryNgx.so otherwise.
# A target newer than the build inputs is reused. Missing arguments exit 2.
# Missing tools or a failed build exit 0 and preserve any previous library.
# Only a successful link publishes a new library.
set -u

out="${1:-}"
if [ -z "$out" ]; then
    echo "usage: build.sh <output directory>" >&2
    exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 0
workspace="$(cd "$here/../../.." && pwd)" || exit 0
src="$here/vulkanstory_ngx.c"

rust_args=()
rust_release=release
libs=()
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        name="VulkanStoryNgx.dll"
        rust_target=x86_64-pc-windows-gnu
        rust_args=(--target "$rust_target")
        rust_release="$rust_target/release"
        libs=(-ladvapi32 -lntdll -luserenv -lws2_32 -ldbghelp)
        ;;
    Linux)
        name="libVulkanStoryNgx.so"
        libs=(-Wl,--exclude-libs,librsf_vulkanstory_ngx.a -Wl,--no-undefined -ldl -lpthread -lm -lrt -lutil)
        ;;
    *) name="libVulkanStoryNgx.so" ;;
esac

cc="${CC:-cc}"
target="$out/$name"
if [ -f "$target" ]; then
    fresh=1
    for input in "$src" "$here/vulkanstory_ngx.h" "$here/src/lib.rs" "$here/Cargo.toml" \
            "$workspace/Cargo.toml" "$workspace/Cargo.lock" "$workspace/rust-toolchain.toml" "$here/build.sh"; do
        if [ ! -f "$input" ] || [ ! "$target" -nt "$input" ]; then
            fresh=0
            break
        fi
    done
    if [ "$fresh" -eq 1 ]; then
        exit 0
    fi
fi

skip_build() {
    echo "vulkanstory-ngx: $1; any previous library was retained." >&2
    exit 0
}

if ! command -v "$cc" >/dev/null 2>&1; then
    skip_build "no C compiler ($cc) on this host"
fi
if ! command -v cargo >/dev/null 2>&1; then
    skip_build "no Cargo on this host"
fi

mkdir -p -- "$out" || exit 0
output="$(cd -- "$out" && pwd)" || exit 0
scratch="$(mktemp -d "$output/.vulkanstory-ngx.XXXXXXXX")" || exit 0
trap 'rm -rf -- "$scratch"' EXIT
if ! (cd "$workspace" && cargo build --locked --release -p rsf-vulkanstory-ngx \
        "${rust_args[@]}" --target-dir "$scratch/cargo"); then
    skip_build "Rust loader compilation failed"
fi
archive="$scratch/cargo/$rust_release/librsf_vulkanstory_ngx.a"
temporary="$scratch/$name"

# Keep the native caller frame across NGX calls. The wrappers also store
# forwarded results in a volatile local.
if ! "$cc" -std=c99 -O2 -fPIC -shared -fvisibility=hidden \
        -fno-optimize-sibling-calls \
        -Wall -Wextra -Wno-unused-parameter \
        "$src" "$archive" -o "$temporary" "${libs[@]}"; then
    skip_build "$cc failed to link the NGX shim"
fi
if ! mv -f -- "$temporary" "$target"; then
    skip_build "could not publish the NGX shim"
fi

echo "vulkanstory-ngx: built $target"
exit 0
