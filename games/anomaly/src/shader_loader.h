// SPDX-License-Identifier: MIT
#pragma once
// RSF motion shader variants for the engine's shader load path (create_shader, motion_profile.h).
//
// The engine keeps its stock shader: stock bytecode is created, reflected and its VS input signature extracted
// exactly as before, on the cache-hit and the compile path alike. For an allowlisted root RSF then builds the
// matching motion variant of THAT permutation, before the engine can bind the shader:
//   1. the engine's own compile inputs are read from shader_compile's frame (root source, entry, target, flags and
//      the complete define list; the frame is only trusted when create_shader returns to one of its two calls);
//   2. the root source is identified by SHA-256 in pack metadata (never by file name alone);
//   3. the acquired ORIGINAL tree is compiled with those inputs and must reproduce the engine's bytecode byte for
//      byte, which proves root, includes, defines, compiler and flags all match;
//   4. the runtime-patched tree is compiled with the same inputs;
//   5. the variant's reflection must keep every stock resource and constant-buffer layout and the VS input
//      signature, and may add only the b13 RSF buffer (VS, DS) or SV_Target2 (PS) and the RSF interpolants
//      (including the hull and domain shaders of the tessellation pass).
// Any failure leaves only the stock shader (counted by reason). Verified variants are cached on disk under a key
// over namespace, root, defines, target, entry, flags and the original bytecode hash; a cache entry is still
// reflection-checked. Production uses the Runtime source-patch service and the engine includer.
// The deployed-tree constructor remains a development fixture for the Python parity oracle.
#include "motion_profile.h"
#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

struct rsf_anomaly_shader_admitted;
struct rsf_anomaly_shader_snapshot;
extern "C" void rsf_anomaly_shader_snapshot_drop(rsf_anomaly_shader_snapshot*) noexcept;
struct rsf_anomaly_shader_d3d;
extern "C" rsf_anomaly_shader_d3d* rsf_anomaly_shader_d3d_load(const uint16_t*) noexcept;
extern "C" void rsf_anomaly_shader_d3d_unload(rsf_anomaly_shader_d3d*) noexcept;

namespace rsf::anomaly::motion {
// C++ data and callback facades for the native motion fixtures.
// TessHull/TessDomain: DX11\tess.hs and DX11\tess.ds of X-Ray's G-buffer tessellation pass (uber_deffer.cpp r_TessPass,
// r4_enable_tessellation): the motion hull stage carries the previous control data, the motion domain stage rebuilds the
// previous PN patch and writes the previous clip position for the motion PS.
// Stable byte values shared with shader_family.rs.
enum class Family : std::uint8_t {
    None = 0, CoreStatic = 1, CoreModel = 2, CorePixel = 3, FoliageTree = 4,
    FoliageDetail = 5, TessHull = 6, TessDomain = 7, Count = 8
};
// Borrowed manifest text, not NUL-terminated.
struct ManifestText {
    const char* data;
    std::size_t size;
    operator std::string_view() const noexcept { return {data, size}; }
};
// One admitted root (rsf_anomaly_shader_entry). Rows are owned by the Rust manifest and stay valid while a Manifest
// sharing it lives.
struct ManifestEntry {
    ManifestText name;       // root file name, relative to r3 (one subdirectory allowed: dx11/tess.hs)
    ManifestText original_sha256, motion_sha256;
    char stage;              // 'v', 'p', 'h' or 'd'
    Family family;
};
// Line format (tools/anomaly_motion_deploy.py):
//   rsf-anomaly-motion-manifest 1
//   namespace <64 hex>
//   root <vs|ps|hs|ds> <family> <name> <original sha256> <motion sha256>
// Rust validates either form and owns the rows and their original-SHA-256 index. Copies share that immutable manifest;
// a default Manifest is empty.
class Manifest {
public:
    Manifest() noexcept = default;
    // Takes ownership of an admitted manifest (shader_loader_platform.h).
    explicit Manifest(rsf_anomaly_shader_admitted* owned) noexcept;
    static std::optional<Manifest> parse(std::string_view text, std::string& error);
    std::string_view name_space() const noexcept { return name_space_; }
    // Ordered by the original root source SHA-256.
    std::span<const ManifestEntry> entries() const noexcept { return entries_; }
    const ManifestEntry* find(std::string_view original_sha256) const noexcept;
    const rsf_anomaly_shader_admitted* admitted() const noexcept { return owner_.get(); }
private:
    std::shared_ptr<const rsf_anomaly_shader_admitted> owner_;
    std::string_view name_space_;
    std::span<const ManifestEntry> entries_;
};

struct Define {
    std::string name, value;
    bool operator==(const Define&) const = default;
};
// What create_shader and its caller frame say about one permutation.
struct ShaderRequest {
    std::string target;                 // create_shader's pTarget, already the final vs_5_0/ps_5_0
    std::string entry;
    std::uint32_t flags{};
    std::vector<Define> defines;        // engine order
    std::vector<std::byte> source;      // the engine's root source text
    std::span<const std::byte> bytecode;  // what the engine just created (borrowed)
    std::string file_name;              // engine cache/object path, diagnostics only
};
struct ReflectionDrop {
    void operator()(rsf_anomaly_shader_snapshot* snapshot) const noexcept { rsf_anomaly_shader_snapshot_drop(snapshot); }
};
using Reflection = std::unique_ptr<rsf_anomaly_shader_snapshot, ReflectionDrop>;
bool keeps_buffer_sizes(const Reflection& stock, const Reflection& variant) noexcept;

// D3D_SIT_CBUFFER = 0.
enum class Compatibility : std::uint8_t { Ok, Version, MissingBinding, ExtraBinding, Buffer, Inputs, Outputs, PatchConstants };
// Linkage of a producer stage with a consumer stage: every consumer input must be a producer output with the same name,
// index and register and a covering mask.
bool linkable(const Reflection& vertex, const Reflection& pixel) noexcept;
// The full tessellated chain VS -> HS -> DS -> PS: each control-point interface links, and every domain patch-constant
// input is a hull patch-constant output with the same register.
bool tess_chain_linkable(const Reflection& vertex, const Reflection& hull, const Reflection& domain, const Reflection& pixel) noexcept;

enum class Outcome : std::uint8_t {
    NotAllowlisted, Replaced, ReplacedFromCache, UnknownCaller, Unreadable, OriginalCompileFailed, OriginalMismatch,
    MotionCompileFailed, ReflectionMismatch, CreateFailed, Count
};
const char* outcome_name(Outcome) noexcept;
struct Prepared {
    Outcome outcome{Outcome::NotAllowlisted};
    const ManifestEntry* entry{};
    int skin{-2};                      // -1 SKIN_NONE, 0..4 SKIN_n, -2 none of the skin defines
    std::vector<std::byte> variant;
    Reflection reflection;             // of the variant
    Reflection stock;                  // of the engine's own bytecode (its constant-buffer indices are the engine's)
    Compatibility compatibility{};
    std::string detail;
};

// Device fixture projections. Rust runs the real compiler, stock proof, and reflection.
bool compile_deployed(const rsf_anomaly_shader_d3d&, const std::filesystem::path& tree,
    const ShaderRequest&, std::vector<std::byte>&, std::string& error);
bool reflect_deployed(const rsf_anomaly_shader_d3d&, std::span<const std::byte>, Reflection&);
Prepared prepare_deployed(const rsf_anomaly_shader_d3d&, const Manifest&, const std::filesystem::path& deployment,
    const ShaderRequest&);

}
