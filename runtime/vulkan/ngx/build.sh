#!/usr/bin/env bash
# Builds the Rust NGX caller module.
#
#   build.sh <output directory>
#
# The output name is
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
rust_args=()
rust_release=release
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        name="VulkanStoryNgx.dll"
        rust_target=x86_64-pc-windows-gnu
        rust_args=(--target "$rust_target")
        rust_release="$rust_target/release"
        artifact=rsf_vulkanstory_ngx.dll
        ;;
    Linux)
        name="libVulkanStoryNgx.so"
        artifact=librsf_vulkanstory_ngx.so
        ;;
    Darwin) name="libVulkanStoryNgx.so"; artifact=librsf_vulkanstory_ngx.dylib ;;
    *) name="libVulkanStoryNgx.so"; artifact=librsf_vulkanstory_ngx.so ;;
esac

target="$out/$name"
if [ -f "$target" ]; then
    fresh=1
    for input in "$here/vulkanstory_ngx.h" "$here/src/lib.rs" "$here/src/loader.rs" "$here/src/parameters.rs" "$here/Cargo.toml" \
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

if ! command -v cargo >/dev/null 2>&1; then
    skip_build "no Cargo on this host"
fi

mkdir -p -- "$out" || exit 0
output="$(cd -- "$out" && pwd)" || exit 0
scratch="$(mktemp -d "$output/.vulkanstory-ngx.XXXXXXXX")" || exit 0
trap 'rm -rf -- "$scratch"' EXIT
if ! (cd "$workspace" && cargo build --locked --release -p rsf-vulkanstory-ngx \
        "${rust_args[@]}" --target-dir "$scratch/cargo"); then
    skip_build "Rust caller module build failed"
fi
temporary="$scratch/cargo/$rust_release/$artifact"
if ! mv -f -- "$temporary" "$target"; then
    skip_build "could not publish the NGX shim"
fi

echo "vulkanstory-ngx: built $target"
exit 0
